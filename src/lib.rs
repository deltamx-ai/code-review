pub mod cli;
pub mod context;
pub mod copilot;
pub mod expand;
pub mod gitops;
pub mod jira;
pub mod prompt;
pub mod session;

use anyhow::{bail, Result};
use clap::Parser;
use cli::{AuthCommand, Cli, Commands};
use jira::{enrich_prompt_args, maybe_expand_context_files};
use prompt::{
    build_prompt, build_prompt_from_sources, print_template, validate_args, PromptOutput,
    PromptSummary,
};
use regex::Regex;
use session::SessionStore;
use std::collections::BTreeSet;

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let store = SessionStore::new_default()?;

    match cli.command {
        Commands::Prompt(mut args) => {
            let repo_files = args.files.clone();
            enrich_prompt_args(&mut args, &repo_files)?;
            maybe_expand_context_files(&mut args, &repo_files);
            let validation = validate_args(
                &args,
                args.diff_file.is_some(),
                !args.context_files.is_empty() || !args.files.is_empty(),
            );
            let prompt = build_prompt(&args)?;
            output_prompt(
                args.format,
                validation.score,
                validation.ok,
                prompt,
                PromptSummary::from_prompt_args(&args),
            )?;
        }
        Commands::Assemble(mut args) => {
            let repo_files = args.files.clone();
            enrich_prompt_args(&mut args, &repo_files)?;
            maybe_expand_context_files(&mut args, &repo_files);
            println!("{}", serde_json::to_string_pretty(&args)?);
        }
        Commands::Run(args) => {
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
            let validation = validate_args(
                &prompt_args,
                true,
                !contexts.files.is_empty() || !files.is_empty(),
            );
            let prompt = build_prompt_from_sources(&prompt_args, Some(diff), contexts)?;
            output_prompt(
                prompt_args.format,
                validation.score,
                validation.ok,
                prompt,
                PromptSummary::from_prompt_args(&prompt_args),
            )?;
        }
        Commands::DeepReview(args) => {
            run_deep_review(&store, args)?;
        }
        Commands::Template { format } => print_template(format)?,
        Commands::Validate(mut args) => {
            let repo_files = args.files.clone();
            jira::enrich_prompt_args(&mut args, &repo_files)?;
            let validation = validate_args(
                &args,
                args.diff_file.is_some(),
                !args.context_files.is_empty() || !args.files.is_empty(),
            );
            validation.print(args.format)?;
        }
        Commands::Auth { command } => match command {
            AuthCommand::Login(args) => {
                let record = copilot::login(&args, &store)?;
                println!("Logged in via {}.", record.provider_source);
                println!("Session saved at {}", store.path().display());
            }
            AuthCommand::Status { format } => {
                let status = copilot::status(&store)?;
                status.print(format)?;
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
        Commands::Review(args) => {
            let status = copilot::status(&store)?;
            if !status.logged_in {
                bail!("Copilot is not authenticated. Run `code-review auth login` first.");
            }
            let prompt = if let Some(prompt) = args.prompt.clone() {
                prompt
            } else if let Some(mut prompt_args) = args.to_prompt_args() {
                let repo_files = prompt_args.files.clone();
                enrich_prompt_args(&mut prompt_args, &repo_files)?;
                maybe_expand_context_files(&mut prompt_args, &repo_files);
                let validation = validate_args(
                    &prompt_args,
                    prompt_args.diff_file.is_some(),
                    !prompt_args.context_files.is_empty() || !prompt_args.files.is_empty(),
                );
                if validation.score < 40 {
                    bail!("review input is too thin; add more context or use --prompt directly");
                }
                build_prompt(&prompt_args)?
            } else {
                bail!("provide --prompt or enough prompt-building flags");
            };
            let response = copilot::run_review(&store, &prompt, args.model.as_deref())?;
            println!("{}", response);
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

    let stage1_prompt = build_prompt_from_sources(&prompt_args, Some(diff.clone()), stage1_contexts)?;
    let stage1_output = copilot::run_review(store, &stage1_prompt, args.model.as_deref())?;

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

    println!("## Stage 1 Review\n{}\n\n## Stage 2 Review\n{}", stage1_output, stage2_output);
    Ok(())
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
