pub mod admission;
pub mod cli;
pub mod config;
pub mod context;
pub mod copilot;
pub mod expand;
pub mod gitops;
pub mod jira;
pub mod models;
pub mod prompt;
pub mod review_layers;
pub mod review_parser;
pub mod review_render;
pub mod review_schema;
pub mod review_validate;
pub mod risk;
pub mod session;

use admission::check_admission;
use anyhow::{bail, Result};
use clap::Parser;
use cli::{AuthCommand, Cli, Commands};
use config::load_config;
use jira::{enrich_prompt_args, maybe_expand_context_files};
use prompt::{
    build_prompt, build_prompt_from_sources, print_template, PromptOutput, PromptSummary,
};
use review_parser::parse_review_text;
use review_render::render_review_result_text;
use review_validate::validate_and_repair_review_result;
use risk::analyze_risks;
use regex::Regex;
use session::SessionStore;
use std::collections::BTreeSet;

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let cfg = load_config()?;
    let store = SessionStore::new_default()?;

    match cli.command {
        Commands::Prompt(mut args) => {
            config::apply_config_defaults(&mut args, &cfg);
            let repo_files = args.files.clone();
            enrich_prompt_args(&mut args, &repo_files)?;
            maybe_expand_context_files(&mut args, &repo_files);
            let admission = check_admission(
                &args,
                args.diff_file.is_some(),
                !args.context_files.is_empty() || !args.files.is_empty(),
            );
            let prompt = build_prompt(&args)?;
            output_prompt(
                args.format,
                admission.score,
                admission.ok,
                prompt,
                PromptSummary::from_prompt_args(&args),
            )?;
        }
        Commands::Assemble(mut args) => {
            config::apply_config_defaults(&mut args, &cfg);
            let repo_files = args.files.clone();
            enrich_prompt_args(&mut args, &repo_files)?;
            maybe_expand_context_files(&mut args, &repo_files);
            println!("{}", serde_json::to_string_pretty(&args)?);
        }
        Commands::Run(mut args) => {
            config::apply_config_defaults(&mut args.prompt, &cfg);
            if let Some(include_context) = cfg.review.include_context {
                if !args.include_context {
                    args.include_context = include_context;
                }
            }
            if args.context_budget_bytes == 48_000 {
                if let Some(v) = cfg.review.context_budget_bytes { args.context_budget_bytes = v; }
            }
            if args.context_file_max_bytes == 12_000 {
                if let Some(v) = cfg.review.context_file_max_bytes { args.context_file_max_bytes = v; }
            }
            gitops::ensure_git_repo(&args.repo)?;
            let diff = gitops::git_diff(&args.repo, &args.git)?;
            if diff.trim().is_empty() {
                bail!("git diff is empty for range {}", args.git);
            }
            let files = gitops::git_changed_files(&args.repo, &args.git)?;
            let repo_files = gitops::list_repo_files(&args.repo)?;
            let contexts = if args.include_context {
                context::read_repo_context_with_budget(
                    &args.repo,
                    &files,
                    args.context_budget_bytes,
                    args.context_file_max_bytes,
                )?
            } else {
                context::ContextCollection::default()
            };
            let mut prompt_args = args.to_prompt_args(files.clone());
            enrich_prompt_args(&mut prompt_args, &files)?;
            maybe_expand_context_files(&mut prompt_args, &files);
            auto_expand_context_paths(&mut prompt_args, &repo_files, &files);
            let contexts = if args.include_context {
                context::read_repo_context_with_budget(
                    &args.repo,
                    &prompt_args
                        .context_files
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>(),
                    args.context_budget_bytes,
                    args.context_file_max_bytes,
                )?
            } else {
                contexts
            };
            let admission = check_admission(
                &prompt_args,
                true,
                !contexts.files.is_empty() || !files.is_empty(),
            );
            if !admission.ok {
                bail!("review blocked: {}", admission.block_reasons.join(" | "));
            }
            let prompt = build_prompt_from_sources(&prompt_args, Some(diff), contexts)?;
            output_prompt(
                prompt_args.format,
                admission.score,
                admission.ok,
                prompt,
                PromptSummary::from_prompt_args(&prompt_args),
            )?;
        }
        Commands::DeepReview(mut args) => {
            config::apply_config_defaults(&mut args.prompt, &cfg);
            if args.model.is_none() {
                args.model = cfg.llm.model.clone();
            }
            if let Some(include_context) = cfg.review.include_context {
                if !args.include_context {
                    args.include_context = include_context;
                }
            }
            if args.context_budget_bytes == 48_000 {
                if let Some(v) = cfg.review.context_budget_bytes { args.context_budget_bytes = v; }
            }
            if args.context_file_max_bytes == 12_000 {
                if let Some(v) = cfg.review.context_file_max_bytes { args.context_file_max_bytes = v; }
            }
            run_deep_review(&store, args)?;
        }
        Commands::Models { format } => {
            let models = models::list_models(&cfg)?;
            models.print(format)?;
        }
        Commands::Template { format } => print_template(format)?,
        Commands::Validate(mut args) => {
            config::apply_config_defaults(&mut args, &cfg);
            let repo_files = args.files.clone();
            jira::enrich_prompt_args(&mut args, &repo_files)?;
            let admission = check_admission(
                &args,
                args.diff_file.is_some(),
                !args.context_files.is_empty() || !args.files.is_empty(),
            );
            admission.print(args.format)?;
        }
        Commands::Auth { command } => match command {
            AuthCommand::Init { format } => {
                let status = copilot::status(&store)?;
                if !status.logged_in {
                    let args = cli::LoginArgs { host: "https://github.com".into(), no_open: false };
                    let _ = copilot::login(&args, &store)?;
                }
                let models = models::list_models(&cfg)?;
                println!("Config path: {}", config::default_config_path()?.display());
                models.print(cli::OutputFormat::Text)?;
                println!("Run `code-review auth select-model --index <n>` to save your default model.");
                let status = copilot::status(&store)?;
                status.print(format)?;
            }
            AuthCommand::Login(args) => {
                let record = copilot::login(&args, &store)?;
                println!("Logged in via {}.", record.provider_source);
                println!("Session saved at {}", store.path().display());
            }
            AuthCommand::Models { format } => {
                let models = models::list_models(&cfg)?;
                models.print(format)?;
            }
            AuthCommand::SelectModel { model, index, format } => {
                let mut cfg = cfg.clone();
                let models = models::list_models(&cfg)?;
                let chosen = if let Some(model) = model {
                    model
                } else if let Some(index) = index {
                    if index == 0 || index > models.models.len() {
                        bail!("model index out of range");
                    }
                    models.models[index - 1].clone()
                } else {
                    bail!("provide --model or --index");
                };
                cfg.llm.model = Some(chosen.clone());
                if cfg.llm.provider.is_none() {
                    cfg.llm.provider = Some("copilot".into());
                }
                let path = config::save_config(&cfg)?;
                match format {
                    cli::OutputFormat::Text => {
                        println!("selected_model: {}", chosen);
                        println!("config: {}", path.display());
                    }
                    cli::OutputFormat::Json => {
                        println!("{}", serde_json::json!({"selected_model": chosen, "config": path.display().to_string() }));
                    }
                }
            }
            AuthCommand::Status { format } => {
                let status = copilot::status(&store)?;
                status.print(format)?;
                if let Some(model) = &cfg.llm.model {
                    println!("default_model: {}", model);
                }
                println!("config: {}", config::default_config_path()?.display());
            }
            AuthCommand::Logout { clear_remote } => {
                copilot::logout(&store, clear_remote)?;
                println!("Logged out. Local session removed.");
            }
            AuthCommand::Refresh { format } => {
                let status = copilot::refresh(&store)?;
                status.print(format)?;
            }
            AuthCommand::Whoami { format } => {
                let info = copilot::whoami(&store)?;
                info.print(format)?;
            }
        },
        Commands::Review(mut args) => {
            let status = copilot::status(&store)?;
            if !status.logged_in {
                bail!("Copilot is not authenticated. Run `code-review auth login` first.");
            }
            let default_model = cfg.llm.model.clone();
            if args.model.is_none() {
                args.model = default_model;
            }
            let (prompt, mode, used_rules, admission) = if let Some(prompt) = args.prompt.clone() {
                (
                    prompt,
                    cli::ReviewMode::Standard,
                    Vec::new(),
                    None,
                )
            } else if let Some(mut prompt_args) = args.to_prompt_args() {
                config::apply_config_defaults(&mut prompt_args, &cfg);
                let repo_files = prompt_args.files.clone();
                enrich_prompt_args(&mut prompt_args, &repo_files)?;
                maybe_expand_context_files(&mut prompt_args, &repo_files);
                let admission = check_admission(
                    &prompt_args,
                    prompt_args.diff_file.is_some(),
                    !prompt_args.context_files.is_empty() || !prompt_args.files.is_empty(),
                );
                if !admission.ok {
                    bail!("review blocked: {}", admission.block_reasons.join(" | "));
                }
                (
                    build_prompt(&prompt_args)?,
                    prompt_args.mode,
                    prompt_args.rules.clone(),
                    Some(admission),
                )
            } else {
                bail!("provide --prompt or enough prompt-building flags");
            };
            let response = copilot::run_review(&store, &prompt, args.model.as_deref())?;
            let mut parsed = parse_review_text(mode, &response, used_rules);
            if let Some(admission) = admission {
                parsed.apply_admission(admission.ok, admission.level, admission.score, admission.confidence);
            }
            if let Some(prompt_args) = args.to_prompt_args() {
                let changed_files = if !prompt_args.files.is_empty() { prompt_args.files.clone() } else { Vec::new() };
                let risk_analysis = analyze_risks(&prompt_args, &changed_files, None);
                parsed.apply_risk_analysis(risk_analysis);
                parsed.finalize();
                let report = validate_and_repair_review_result(mode, &mut parsed);
                parsed.apply_validation_report(report.clone());
                if !report.ok {
                    parsed.repair_attempted = true;
                    let repaired_prompt = build_repair_prompt(&response, mode);
                    if let Ok(repaired_text) = copilot::run_review(&store, &repaired_prompt, args.model.as_deref()) {
                        let mut repaired = parse_review_text(mode, &repaired_text, prompt_args.rules.clone());
                        if let Some(admission) = check_admission_for_prompt_args(&prompt_args) {
                            repaired.apply_admission(admission.ok, admission.level, admission.score, admission.confidence);
                        }
                        repaired.apply_risk_analysis(analyze_risks(&prompt_args, &changed_files, None));
                        repaired.finalize();
                        let second_report = validate_and_repair_review_result(mode, &mut repaired);
                        repaired.apply_validation_report(second_report.clone());
                        repaired.repair_attempted = true;
                        repaired.repair_succeeded = second_report.ok;
                        if second_report.ok {
                            parsed = repaired;
                        }
                    }
                }
            }
            match args.prompt_args.format {
                cli::OutputFormat::Text => println!("{}", render_review_result_text(&parsed)),
                cli::OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&parsed)?),
            }
        }
    }

    Ok(())
}

fn auto_expand_context_paths(
    args: &mut cli::PromptArgs,
    repo_files: &[String],
    changed_files: &[String],
) {
    let extra = expand::expand_related_files(changed_files, repo_files);
    let mut seen = args
        .context_files
        .iter()
        .map(|p| p.display().to_string())
        .collect::<BTreeSet<_>>();
    for file in extra {
        if seen.insert(file.clone()) {
            args.context_files.push(std::path::PathBuf::from(file));
        }
    }
}

fn extract_stage2_focus(stage1: &str) -> (Vec<String>, Vec<String>) {
    let file_re = Regex::new(r"([A-Za-z0-9_./-]+\.(rs|ts|tsx|js|jsx|java|go|py|sql|yml|yaml))").unwrap();
    let fn_re = Regex::new(r"([A-Za-z_][A-Za-z0-9_]{2,})\s*(?:\(|函数|method)").unwrap();
    let mut files = BTreeSet::new();
    let mut hints = Vec::new();
    for cap in file_re.captures_iter(stage1) {
        files.insert(cap[1].to_string());
    }
    for line in stage1.lines() {
        if line.contains("不确定") || line.to_lowercase().contains("uncertain") {
            hints.push(line.trim().to_string());
        }
        if line.contains("高风险") || line.contains("High") {
            hints.push(line.trim().to_string());
        }
    }
    for cap in fn_re.captures_iter(stage1) {
        let name = cap[1].to_string();
        if !hints.iter().any(|h| h.contains(&name)) {
            hints.push(format!("重点复核函数/方法: {}", name));
        }
    }
    (files.into_iter().collect(), hints.into_iter().take(8).collect())
}

fn run_deep_review(store: &SessionStore, args: cli::DeepReviewArgs) -> Result<()> {
    let status = copilot::status(store)?;
    if !status.logged_in {
        bail!("Copilot is not authenticated. Run `code-review auth login` first.");
    }

    gitops::ensure_git_repo(&args.repo)?;
    let diff = gitops::git_diff(&args.repo, &args.git)?;
    if diff.trim().is_empty() {
        bail!("git diff is empty for range {}", args.git);
    }
    let changed_files = gitops::git_changed_files(&args.repo, &args.git)?;
    let repo_files = gitops::list_repo_files(&args.repo)?;

    let mut prompt_args = args.to_prompt_args(changed_files.clone());
    enrich_prompt_args(&mut prompt_args, &changed_files)?;
    maybe_expand_context_files(&mut prompt_args, &changed_files);
    auto_expand_context_paths(&mut prompt_args, &repo_files, &changed_files);

    let stage1_contexts = if args.include_context {
        context::read_repo_context_with_budget(
            &args.repo,
            &prompt_args
                .context_files
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>(),
            args.context_budget_bytes,
            args.context_file_max_bytes,
        )?
    } else {
        context::ContextCollection::default()
    };

    let stage1_admission = check_admission(
        &prompt_args,
        true,
        !stage1_contexts.files.is_empty() || !changed_files.is_empty(),
    );
    if !stage1_admission.ok {
        bail!("deep-review blocked: {}", stage1_admission.block_reasons.join(" | "));
    }

    let stage1_prompt = build_prompt_from_sources(&prompt_args, Some(diff.clone()), stage1_contexts)?;
    let stage1_output = copilot::run_review(store, &stage1_prompt, args.model.as_deref())?;
    let mut stage1_parsed = parse_review_text(prompt_args.mode, &stage1_output, prompt_args.rules.clone());
    stage1_parsed.apply_admission(
        stage1_admission.ok,
        stage1_admission.level,
        stage1_admission.score,
        stage1_admission.confidence,
    );
    stage1_parsed.apply_risk_analysis(analyze_risks(&prompt_args, &changed_files, Some(&diff)));
    stage1_parsed.finalize();
    let stage1_report = validate_and_repair_review_result(prompt_args.mode, &mut stage1_parsed);
    stage1_parsed.apply_validation_report(stage1_report);

    let (stage2_files, stage2_hints) = extract_stage2_focus(&stage1_output);
    let mut stage2_args = prompt_args.clone();
    stage2_args.focus.extend(stage2_hints.clone());
    for file in stage2_files {
        if !stage2_args.files.contains(&file) {
            stage2_args.files.push(file.clone());
        }
        let pb = std::path::PathBuf::from(&file);
        if !stage2_args.context_files.contains(&pb) {
            stage2_args.context_files.push(pb);
        }
    }
    let stage2_existing_files = stage2_args.files.clone();
    auto_expand_context_paths(&mut stage2_args, &repo_files, &stage2_existing_files);

    let stage2_contexts = if args.include_context {
        context::read_repo_context_with_budget(
            &args.repo,
            &stage2_args
                .context_files
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>(),
            args.context_budget_bytes,
            args.context_file_max_bytes,
        )?
    } else {
        context::ContextCollection::default()
    };

    let mut stage2_prompt = build_prompt_from_sources(&stage2_args, Some(diff), stage2_contexts)?;
    stage2_prompt.push_str("\n\n## Stage 2 Mission\n请基于第一阶段发现的高风险点、不确定点和关联文件，重点验证：\n");
    for hint in stage2_hints {
        stage2_prompt.push_str(&format!("- {}\n", hint));
    }
    stage2_prompt.push_str("请避免重复第一阶段结论，优先确认真正的业务逻辑问题、实现逻辑问题和跨文件联动风险。\n");
    let stage2_output = copilot::run_review(store, &stage2_prompt, args.model.as_deref())?;
    let mut stage2_parsed = parse_review_text(stage2_args.mode, &stage2_output, stage2_args.rules.clone());
    stage2_parsed.apply_admission(
        stage1_admission.ok,
        stage1_admission.level,
        stage1_admission.score,
        stage1_admission.confidence,
    );
    stage2_parsed.apply_risk_analysis(analyze_risks(&stage2_args, &stage2_args.files, None));
    stage2_parsed.finalize();
    let stage2_report = validate_and_repair_review_result(stage2_args.mode, &mut stage2_parsed);
    stage2_parsed.apply_validation_report(stage2_report.clone());
    if !stage2_report.ok {
        stage2_parsed.repair_attempted = true;
        let repaired_prompt = build_repair_prompt(&stage2_output, stage2_args.mode);
        if let Ok(repaired_text) = copilot::run_review(store, &repaired_prompt, args.model.as_deref()) {
            let mut repaired = parse_review_text(stage2_args.mode, &repaired_text, stage2_args.rules.clone());
            repaired.apply_admission(
                stage1_admission.ok,
                stage1_admission.level,
                stage1_admission.score,
                stage1_admission.confidence,
            );
            repaired.apply_risk_analysis(analyze_risks(&stage2_args, &stage2_args.files, None));
            repaired.finalize();
            let second_report = validate_and_repair_review_result(stage2_args.mode, &mut repaired);
            repaired.apply_validation_report(second_report.clone());
            repaired.repair_attempted = true;
            repaired.repair_succeeded = second_report.ok;
            if second_report.ok {
                stage2_parsed = repaired;
            }
        }
    }

    match args.prompt.format {
        cli::OutputFormat::Text => {
            println!("## Stage 1 Review\n{}\n", render_review_result_text(&stage1_parsed));
            println!("## Stage 2 Review\n{}", render_review_result_text(&stage2_parsed));
        }
        cli::OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "stage1": stage1_parsed,
                    "stage2": stage2_parsed
                }))?
            );
        }
    }
    Ok(())
}

fn build_repair_prompt(raw_output: &str, mode: cli::ReviewMode) -> String {
    let mut prompt = String::new();
    prompt.push_str("请把下面这份 code review 结果修复为更严格的结构化输出，不要新增无根据结论，只重排和补齐格式。\n");
    prompt.push_str("必须包含：\n1. 高风险问题\n2. 中风险问题\n3. 低风险优化建议\n4. 缺失的测试场景\n5. 总结结论\n");
    if matches!(mode, cli::ReviewMode::Critical) {
        prompt.push_str("6. 风险影响面\n7. 发布建议 / 人工确认项\n");
    }
    prompt.push_str("每个风险问题尽量包含：文件/位置、原因、触发条件、影响、建议。证据不足就写“不确定，需要补充上下文”。\n\n原始输出如下：\n");
    prompt.push_str(raw_output);
    prompt
}

fn check_admission_for_prompt_args(args: &cli::PromptArgs) -> Option<admission::AdmissionResult> {
    Some(check_admission(
        args,
        args.diff_file.is_some(),
        !args.context_files.is_empty() || !args.files.is_empty(),
    ))
}

fn output_prompt(
    format: cli::OutputFormat,
    score: u8,
    ok: bool,
    prompt: String,
    summary: PromptSummary,
) -> Result<()> {
    match format {
        cli::OutputFormat::Text => {
            println!("# Review Readiness Score: {}/100", score);
            if !ok {
                println!("# Warning: context is incomplete; AI review quality may be limited.\n");
            }
            println!("{}", prompt);
        }
        cli::OutputFormat::Json => {
            let output = PromptOutput {
                ok,
                score,
                prompt,
                summary,
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_stage2_files_and_hints() {
        let stage1 = r#"
高风险问题
- src/order/service.rs: 幂等处理可能失效
- 不确定：OrderService.createPayment 在重试时是否有事务保护
- src/order/dto.rs: 契约可能变化
"#;
        let (files, hints) = extract_stage2_focus(stage1);
        assert!(files.iter().any(|f| f == "src/order/service.rs"));
        assert!(files.iter().any(|f| f == "src/order/dto.rs"));
        assert!(hints.iter().any(|h| h.contains("不确定")));
    }

    #[test]
    fn auto_expand_context_adds_related_files() {
        let mut args = cli::PromptArgs {
            mode: cli::ReviewMode::Standard,
            stack: None,
            goal: None,
            why: None,
            rules: vec![],
            risks: vec![],
            expected_normal: None,
            expected_error: None,
            expected_edge: None,
            issue: None,
            test_results: vec![],
            jira: None,
            jira_base_url: None,
            jira_provider: "native".into(),
            jira_command: None,
            diff_file: None,
            context_files: vec![],
            files: vec!["src/order/service.rs".into()],
            focus: vec![],
            baseline_files: vec![],
            change_type: None,
            format: cli::OutputFormat::Text,
        };
        let repo_files = vec![
            "src/order/service.rs".into(),
            "src/order/service_test.rs".into(),
            "src/order/dto.rs".into(),
        ];
        let changed_files = vec!["src/order/service.rs".into()];
        auto_expand_context_paths(&mut args, &repo_files, &changed_files);
        let collected = args.context_files.iter().map(|p| p.display().to_string()).collect::<Vec<_>>();
        assert!(collected.iter().any(|f| f.ends_with("service_test.rs")));
    }
}
