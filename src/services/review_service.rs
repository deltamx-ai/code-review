use crate::admission::{check_admission, AdmissionResult};
use crate::cli::{AnalyzeArgs, AnalyzeStrategy, DeepReviewArgs, OutputFormat, PromptArgs, ReviewArgs, ReviewMode, RunArgs};
use crate::context;
use crate::copilot;
use crate::gitops;
use crate::jira::{enrich_prompt_args, maybe_expand_context_files};
use crate::prompt::build_prompt_from_sources;
use crate::review_parser::parse_review_text;
use crate::review_render::render_review_result_text;
use crate::review_schema::ReviewResult;
use crate::review_validate::validate_and_repair_review_result;
use crate::risk::analyze_risks;
use crate::session::SessionStore;
use anyhow::{bail, Result};
use regex::Regex;
use std::collections::BTreeSet;
use std::path::PathBuf;

pub struct ReviewExecution {
    pub result: ReviewResult,
    pub exit_code: i32,
}

pub struct DeepReviewExecution {
    pub stage1: ReviewResult,
    pub stage2: ReviewResult,
    pub exit_code: i32,
}

pub struct PromptExecution {
    pub prompt: String,
    pub score: u8,
    pub ok: bool,
    pub summary: crate::prompt::PromptSummary,
    pub exit_code: i32,
}

pub struct AssembleExecution {
    pub prompt_args: PromptArgs,
}

pub struct ValidateExecution {
    pub admission: AdmissionResult,
    pub exit_code: i32,
}

pub struct AnalyzeExecution {
    pub strategy: String,
    pub admission: AdmissionResult,
    pub prompt: crate::prompt::PromptOutput,
    pub review: Option<ReviewResult>,
    pub stage1: Option<ReviewResult>,
    pub stage2: Option<ReviewResult>,
    pub exit_code: i32,
}

pub fn execute_prompt(args: &PromptArgs) -> Result<PromptExecution> {
    let mut prompt_args = args.clone();
    let repo_files = prompt_args.files.clone();
    enrich_prompt_args(&mut prompt_args, &repo_files)?;
    maybe_expand_context_files(&mut prompt_args, &repo_files);
    let admission = check_admission(
        &prompt_args,
        prompt_args.diff_file.is_some(),
        !prompt_args.context_files.is_empty() || !prompt_args.files.is_empty(),
    );
    let prompt = crate::prompt::build_prompt(&prompt_args)?;
    Ok(PromptExecution {
        prompt,
        score: admission.score,
        ok: admission.ok,
        summary: crate::prompt::PromptSummary::from_prompt_args(&prompt_args),
        exit_code: if admission.ok { 0 } else { 3 },
    })
}

pub fn execute_assemble(args: &PromptArgs) -> Result<AssembleExecution> {
    let mut prompt_args = args.clone();
    let repo_files = prompt_args.files.clone();
    enrich_prompt_args(&mut prompt_args, &repo_files)?;
    maybe_expand_context_files(&mut prompt_args, &repo_files);
    Ok(AssembleExecution { prompt_args })
}

pub fn execute_validate(args: &PromptArgs) -> Result<ValidateExecution> {
    let mut prompt_args = args.clone();
    let repo_files = prompt_args.files.clone();
    enrich_prompt_args(&mut prompt_args, &repo_files)?;
    maybe_expand_context_files(&mut prompt_args, &repo_files);
    let admission = check_admission(
        &prompt_args,
        prompt_args.diff_file.is_some(),
        !prompt_args.context_files.is_empty() || !prompt_args.files.is_empty(),
    );
    let exit_code = if admission.ok { 0 } else { 3 };
    Ok(ValidateExecution { admission, exit_code })
}

pub fn execute_run(args: &RunArgs) -> Result<PromptExecution> {
    let (prompt_args, diff, contexts, admission) = prepare_run_prompt(args)?;
    let prompt = build_prompt_from_sources(&prompt_args, Some(diff), contexts)?;
    Ok(PromptExecution {
        prompt,
        score: admission.score,
        ok: admission.ok,
        summary: crate::prompt::PromptSummary::from_prompt_args(&prompt_args),
        exit_code: if admission.ok { 0 } else { 3 },
    })
}

pub fn execute_analyze(
    store: &SessionStore,
    cfg_default_model: Option<String>,
    args: &AnalyzeArgs,
) -> Result<AnalyzeExecution> {
    let run_args = args.to_run_args();
    let prompt_execution = execute_run(&run_args)?;
    let assembled = execute_assemble(&run_args.prompt)?;
    let admission = check_admission(
        &assembled.prompt_args,
        assembled.prompt_args.diff_file.is_some() || !prompt_execution.summary.files.is_empty(),
        !assembled.prompt_args.context_files.is_empty() || !assembled.prompt_args.files.is_empty(),
    );

    match args.strategy {
        AnalyzeStrategy::Standard => {
            let mut review_args = ReviewArgs {
                prompt: Some(prompt_execution.prompt.clone()),
                model: args.model.clone().or(cfg_default_model),
                prompt_args: assembled.prompt_args.clone(),
            };
            let review = execute_review(store, review_args.model.clone(), &mut review_args)?;
            Ok(AnalyzeExecution {
                strategy: "standard".into(),
                admission,
                prompt: crate::prompt::PromptOutput {
                    ok: prompt_execution.ok,
                    score: prompt_execution.score,
                    prompt: prompt_execution.prompt,
                    summary: prompt_execution.summary,
                },
                review: Some(review.result),
                stage1: None,
                stage2: None,
                exit_code: review.exit_code,
            })
        }
        AnalyzeStrategy::Deep => {
            let mut deep_args = args.to_deep_review_args();
            if deep_args.model.is_none() {
                deep_args.model = cfg_default_model;
            }
            let deep = execute_deep_review(store, &deep_args)?;
            Ok(AnalyzeExecution {
                strategy: "deep".into(),
                admission,
                prompt: crate::prompt::PromptOutput {
                    ok: prompt_execution.ok,
                    score: prompt_execution.score,
                    prompt: prompt_execution.prompt,
                    summary: prompt_execution.summary,
                },
                review: None,
                stage1: Some(deep.stage1),
                stage2: Some(deep.stage2),
                exit_code: deep.exit_code,
            })
        }
    }
}

fn prepare_run_prompt(args: &RunArgs) -> Result<(PromptArgs, String, context::ContextCollection, AdmissionResult)> {
    gitops::ensure_git_repo(&args.repo)?;
    let diff = gitops::git_diff(&args.repo, &args.git)?;
    if diff.trim().is_empty() {
        bail!("git diff is empty for range {}", args.git);
    }
    let files = gitops::git_changed_files(&args.repo, &args.git)?;
    let repo_files = gitops::list_repo_files(&args.repo)?;
    let base_contexts = if args.include_context {
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
            &prompt_args.context_files.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
            args.context_budget_bytes,
            args.context_file_max_bytes,
        )?
    } else {
        base_contexts
    };

    let admission = check_admission(
        &prompt_args,
        true,
        !contexts.files.is_empty() || !files.is_empty(),
    );

    Ok((prompt_args, diff, contexts, admission))
}

pub fn execute_review(
    store: &SessionStore,
    cfg_default_model: Option<String>,
    args: &mut ReviewArgs,
) -> Result<ReviewExecution> {
    let status = copilot::status(store)?;
    if !status.logged_in {
        bail!("Copilot is not authenticated. Run `code-review auth login` first.");
    }
    if args.model.is_none() {
        args.model = cfg_default_model;
    }

    let (prompt, mode, used_rules, admission, prompt_args_opt) = if let Some(prompt) = args.prompt.clone() {
        (prompt, ReviewMode::Standard, Vec::new(), None, None)
    } else if let Some(prompt_args) = args.to_prompt_args() {
        let admission = check_admission(
            &prompt_args,
            prompt_args.diff_file.is_some(),
            !prompt_args.context_files.is_empty() || !prompt_args.files.is_empty(),
        );
        if !admission.ok {
            bail!("review blocked: {}", admission.block_reasons.join(" | "));
        }
        (
            crate::prompt::build_prompt(&prompt_args)?,
            prompt_args.mode,
            prompt_args.rules.clone(),
            Some(admission),
            Some(prompt_args),
        )
    } else {
        bail!("provide --prompt or enough prompt-building flags");
    };

    let response = copilot::run_review(store, &prompt, args.model.as_deref())?;
    let mut parsed = parse_review_text(mode, &response, used_rules);
    if let Some(admission) = admission {
        parsed.apply_admission(admission.ok, admission.level, admission.score, admission.confidence);
    }
    if let Some(prompt_args) = prompt_args_opt {
        let changed_files = if !prompt_args.files.is_empty() { prompt_args.files.clone() } else { Vec::new() };
        parsed.apply_risk_analysis(analyze_risks(&prompt_args, &changed_files, None));
        parsed.finalize();
        let report = validate_and_repair_review_result(mode, &mut parsed);
        parsed.apply_validation_report(report.clone());
        if !report.ok {
            parsed.repair_attempted = true;
            let repaired_prompt = build_repair_prompt(&response, mode);
            if let Ok(repaired_text) = copilot::run_review(store, &repaired_prompt, args.model.as_deref()) {
                let mut repaired = parse_review_text(mode, &repaired_text, prompt_args.rules.clone());
                let admission = check_admission_for_prompt_args(&prompt_args);
                repaired.apply_admission(admission.ok, admission.level, admission.score, admission.confidence);
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

    let exit_code = exit_code_for_result(&parsed);
    Ok(ReviewExecution { result: parsed, exit_code })
}

pub fn execute_deep_review(store: &SessionStore, args: &DeepReviewArgs) -> Result<DeepReviewExecution> {
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
            &prompt_args.context_files.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
            args.context_budget_bytes,
            args.context_file_max_bytes,
        )?
    } else {
        context::ContextCollection::default()
    };

    let stage1_admission = check_admission(&prompt_args, true, !stage1_contexts.files.is_empty() || !changed_files.is_empty());
    if !stage1_admission.ok {
        bail!("deep-review blocked: {}", stage1_admission.block_reasons.join(" | "));
    }

    let stage1_prompt = build_prompt_from_sources(&prompt_args, Some(diff.clone()), stage1_contexts)?;
    let stage1_output = copilot::run_review(store, &stage1_prompt, args.model.as_deref())?;
    let mut stage1 = parse_review_text(prompt_args.mode, &stage1_output, prompt_args.rules.clone());
    stage1.apply_admission(stage1_admission.ok, stage1_admission.level, stage1_admission.score, stage1_admission.confidence);
    stage1.apply_risk_analysis(analyze_risks(&prompt_args, &changed_files, Some(&diff)));
    stage1.finalize();
    let stage1_report = validate_and_repair_review_result(prompt_args.mode, &mut stage1);
    stage1.apply_validation_report(stage1_report);

    let (stage2_files, stage2_hints) = extract_stage2_focus(&stage1_output);
    let mut stage2_args = prompt_args.clone();
    stage2_args.focus.extend(stage2_hints.clone());
    for file in stage2_files {
        if !stage2_args.files.contains(&file) {
            stage2_args.files.push(file.clone());
        }
        let pb = PathBuf::from(&file);
        if !stage2_args.context_files.contains(&pb) {
            stage2_args.context_files.push(pb);
        }
    }
    let stage2_existing_files = stage2_args.files.clone();
    auto_expand_context_paths(&mut stage2_args, &repo_files, &stage2_existing_files);

    let stage2_contexts = if args.include_context {
        context::read_repo_context_with_budget(
            &args.repo,
            &stage2_args.context_files.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
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
    let mut stage2 = parse_review_text(stage2_args.mode, &stage2_output, stage2_args.rules.clone());
    stage2.apply_admission(stage1_admission.ok, stage1_admission.level, stage1_admission.score, stage1_admission.confidence);
    stage2.apply_risk_analysis(analyze_risks(&stage2_args, &stage2_args.files, None));
    stage2.finalize();
    let stage2_report = validate_and_repair_review_result(stage2_args.mode, &mut stage2);
    stage2.apply_validation_report(stage2_report.clone());
    if !stage2_report.ok {
        stage2.repair_attempted = true;
        let repaired_prompt = build_repair_prompt(&stage2_output, stage2_args.mode);
        if let Ok(repaired_text) = copilot::run_review(store, &repaired_prompt, args.model.as_deref()) {
            let mut repaired = parse_review_text(stage2_args.mode, &repaired_text, stage2_args.rules.clone());
            repaired.apply_admission(stage1_admission.ok, stage1_admission.level, stage1_admission.score, stage1_admission.confidence);
            repaired.apply_risk_analysis(analyze_risks(&stage2_args, &stage2_args.files, None));
            repaired.finalize();
            let second_report = validate_and_repair_review_result(stage2_args.mode, &mut repaired);
            repaired.apply_validation_report(second_report.clone());
            repaired.repair_attempted = true;
            repaired.repair_succeeded = second_report.ok;
            if second_report.ok {
                stage2 = repaired;
            }
        }
    }

    let exit_code = exit_code_for_result(&stage2);
    Ok(DeepReviewExecution { stage1, stage2, exit_code })
}

pub fn render_prompt_execution(format: OutputFormat, execution: &PromptExecution) -> Result<()> {
    match format {
        OutputFormat::Text => {
            println!("# Review Readiness Score: {}/100", execution.score);
            if !execution.ok {
                println!("# Warning: context is incomplete; AI review quality may be limited.\n");
            }
            println!("{}", execution.prompt);
        }
        OutputFormat::Json => {
            let output = crate::prompt::PromptOutput {
                ok: execution.ok,
                score: execution.score,
                prompt: execution.prompt.clone(),
                summary: crate::prompt::PromptSummary {
                    stack: execution.summary.stack.clone(),
                    goal: execution.summary.goal.clone(),
                    issue: execution.summary.issue.clone(),
                    rules_count: execution.summary.rules_count,
                    risks: execution.summary.risks.clone(),
                    test_results_count: execution.summary.test_results_count,
                    files: execution.summary.files.clone(),
                    context_files: execution.summary.context_files.clone(),
                    has_diff: execution.summary.has_diff,
                },
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
    }
    Ok(())
}

pub fn render_assemble_execution(execution: &AssembleExecution) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(&execution.prompt_args)?);
    Ok(())
}

pub fn render_validate_execution(format: OutputFormat, execution: &ValidateExecution) -> Result<()> {
    execution.admission.print(format)
}

pub fn render_review_execution(format: OutputFormat, execution: &ReviewExecution) -> Result<()> {
    match format {
        OutputFormat::Text => println!("{}", render_review_result_text(&execution.result)),
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&execution.result)?),
    }
    Ok(())
}

pub fn render_deep_review_execution(format: OutputFormat, execution: &DeepReviewExecution) -> Result<()> {
    match format {
        OutputFormat::Text => {
            println!("## Stage 1 Review\n{}\n", render_review_result_text(&execution.stage1));
            println!("## Stage 2 Review\n{}", render_review_result_text(&execution.stage2));
        }
        OutputFormat::Json => {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "stage1": execution.stage1,
                    "stage2": execution.stage2
                }))?
            );
        }
    }
    Ok(())
}

pub fn render_analyze_execution(format: OutputFormat, execution: &AnalyzeExecution) -> Result<()> {
    match format {
        OutputFormat::Text => {
            println!("strategy: {}", execution.strategy);
            println!("admission_ok: {}", execution.admission.ok);
            println!("admission_score: {}", execution.admission.score);
            println!("\n## Prompt Summary");
            println!("prompt_ok: {}", execution.prompt.ok);
            println!("prompt_score: {}", execution.prompt.score);
            println!("files: {}", execution.prompt.summary.files.join(", "));
            if let Some(review) = &execution.review {
                println!("\n## Review");
                println!("{}", render_review_result_text(review));
            }
            if let Some(stage1) = &execution.stage1 {
                println!("\n## Stage 1 Review");
                println!("{}", render_review_result_text(stage1));
            }
            if let Some(stage2) = &execution.stage2 {
                println!("\n## Stage 2 Review");
                println!("{}", render_review_result_text(stage2));
            }
        }
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                "strategy": execution.strategy,
                "admission": execution.admission,
                "prompt": execution.prompt,
                "review": execution.review,
                "stage1": execution.stage1,
                "stage2": execution.stage2,
                "exit_code": execution.exit_code,
            }))?);
        }
    }
    Ok(())
}

fn check_admission_for_prompt_args(args: &PromptArgs) -> AdmissionResult {
    check_admission(args, args.diff_file.is_some(), !args.context_files.is_empty() || !args.files.is_empty())
}

fn exit_code_for_result(result: &ReviewResult) -> i32 {
    if result.validation_report.as_ref().map(|r| !r.ok).unwrap_or(false) {
        4
    } else if result.needs_human_review || !result.high_risk.is_empty() {
        2
    } else {
        0
    }
}

pub fn build_repair_prompt(raw_output: &str, mode: ReviewMode) -> String {
    let mut prompt = String::new();
    prompt.push_str("请把下面这份 code review 结果修复为更严格的结构化输出，不要新增无根据结论，只重排和补齐格式。\n");
    prompt.push_str("必须包含：\n1. 高风险问题\n2. 中风险问题\n3. 低风险优化建议\n4. 缺失的测试场景\n5. 总结结论\n");
    if matches!(mode, ReviewMode::Critical) {
        prompt.push_str("6. 风险影响面\n7. 发布建议 / 人工确认项\n");
    }
    prompt.push_str("每个风险问题尽量包含：文件/位置、原因、触发条件、影响、建议。证据不足就写“不确定，需要补充上下文”。\n\n原始输出如下：\n");
    prompt.push_str(raw_output);
    prompt
}

pub fn auto_expand_context_paths(args: &mut PromptArgs, repo_files: &[String], changed_files: &[String]) {
    let extra = crate::expand::expand_related_files(changed_files, repo_files);
    let mut seen = args.context_files.iter().map(|p| p.display().to_string()).collect::<BTreeSet<_>>();
    for file in extra {
        if seen.insert(file.clone()) {
            args.context_files.push(PathBuf::from(file));
        }
    }
}

pub fn extract_stage2_focus(stage1: &str) -> (Vec<String>, Vec<String>) {
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
