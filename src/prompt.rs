use crate::admission::{check_admission, AdmissionResult};
use crate::cli::{OutputFormat, PromptArgs};
use crate::context::ContextCollection;
use crate::review_layers::{build_review_layers, render_layers_prompt};
use crate::risk::analyze_risks;
use anyhow::{Context, Result};
use serde::Serialize;
use std::fs;

#[derive(Debug, Serialize)]
pub struct ValidationResult {
    pub ok: bool,
    pub score: u8,
    pub missing_required: Vec<String>,
    pub suggestions: Vec<String>,
}

impl ValidationResult {
    pub fn print(&self, format: OutputFormat) -> Result<()> {
        match format {
            OutputFormat::Text => {
                println!("ok: {}", self.ok);
                println!("score: {}", self.score);
                if !self.missing_required.is_empty() {
                    println!("missing: {}", self.missing_required.join(", "));
                }
                if !self.suggestions.is_empty() {
                    println!("suggestions: {}", self.suggestions.join(" | "));
                }
            }
            OutputFormat::Json => println!("{}", serde_json::to_string_pretty(self)?),
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
pub struct PromptOutput {
    pub ok: bool,
    pub score: u8,
    pub prompt: String,
    pub summary: PromptSummary,
}

#[derive(Debug, Serialize)]
pub struct PromptSummary {
    pub stack: Option<String>,
    pub goal: Option<String>,
    pub issue: Option<String>,
    pub rules_count: usize,
    pub risks: Vec<String>,
    pub test_results_count: usize,
    pub files: Vec<String>,
    pub context_files: Vec<String>,
    pub has_diff: bool,
}

impl PromptSummary {
    pub fn from_prompt_args(args: &PromptArgs) -> Self {
        Self {
            stack: args.stack.clone(),
            goal: args.goal.clone(),
            issue: args.issue.clone(),
            rules_count: args.rules.len(),
            risks: args.risks.clone(),
            test_results_count: args.test_results.len(),
            files: args.files.clone(),
            context_files: args
                .context_files
                .iter()
                .map(|p| p.display().to_string())
                .collect(),
            has_diff: args.diff_file.is_some(),
        }
    }
}

pub fn validate_args(args: &PromptArgs, has_diff: bool, has_context: bool) -> ValidationResult {
    let admission = check_admission(args, has_diff, has_context);
    validation_from_admission(admission)
}

fn validation_from_admission(admission: AdmissionResult) -> ValidationResult {
    let mut missing = admission.missing_p0;
    missing.extend(admission.missing_p1);

    let mut suggestions = admission.suggestions;
    suggestions.extend(admission.warnings);
    suggestions.extend(admission.block_reasons);

    ValidationResult {
        ok: admission.ok,
        score: admission.score,
        missing_required: missing,
        suggestions,
    }
}

pub fn build_prompt(args: &PromptArgs) -> Result<String> {
    let diff = match &args.diff_file {
        Some(path) => Some(
            fs::read_to_string(path)
                .with_context(|| format!("failed to read {}", path.display()))?,
        ),
        None => None,
    };

    let mut contexts = ContextCollection::default();
    for path in args.context_files.iter().chain(args.baseline_files.iter()) {
        let content = fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        contexts.files.push(crate::context::ContextFile {
            path: path.display().to_string(),
            content,
            truncated: false,
        });
    }
    build_prompt_from_sources(args, diff, contexts)
}

pub fn build_prompt_from_sources(
    args: &PromptArgs,
    diff: Option<String>,
    contexts: ContextCollection,
) -> Result<String> {
    let mut out = String::new();
    out.push_str("你是一个资深代码审查工程师。只关注真实缺陷、回归风险、安全问题、并发/事务问题和边界条件。\n\n");
    if let Some(v) = &args.stack {
        out.push_str(&format!("技术栈: {}\n", v));
    }
    out.push_str(&format!("Review 模式: {:?}\n", args.mode));
    if let Some(v) = &args.goal {
        out.push_str(&format!("改动目标: {}\n", v));
    }
    if let Some(v) = &args.why {
        out.push_str(&format!("背景原因: {}\n", v));
    }
    if let Some(v) = &args.issue {
        out.push_str(&format!("Issue/需求描述: {}\n", v));
    }
    if !args.rules.is_empty() {
        out.push_str(&format!("业务规则:\n- {}\n", args.rules.join("\n- ")));
    }
    if !args.risks.is_empty() {
        out.push_str(&format!("重点风险:\n- {}\n", args.risks.join("\n- ")));
    }
    if let Some(v) = &args.expected_normal {
        out.push_str(&format!("正常预期: {}\n", v));
    }
    if let Some(v) = &args.expected_error {
        out.push_str(&format!("异常预期: {}\n", v));
    }
    if let Some(v) = &args.expected_edge {
        out.push_str(&format!("边界预期: {}\n", v));
    }
    if !args.test_results.is_empty() {
        out.push_str(&format!("测试结果:\n- {}\n", args.test_results.join("\n- ")));
    }
    if !args.files.is_empty() {
        out.push_str(&format!("涉及文件:\n- {}\n", args.files.join("\n- ")));
    }
    if !args.focus.is_empty() {
        out.push_str(&format!("额外关注点:\n- {}\n", args.focus.join("\n- ")));
    }
    if !args.baseline_files.is_empty() {
        out.push_str(&format!("基线/红线文件:\n- {}\n", args.baseline_files.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("\n- ")));
    }
    if !args.baseline_files.is_empty() {
        out.push_str(&format!("基线/红线参考文件:\n- {}\n", args.baseline_files.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("\n- ")));
    }
    let layers = build_review_layers(args);
    out.push('\n');
    out.push_str(&render_layers_prompt(&layers));
    out.push('\n');

    let context_paths = contexts.files.iter().map(|f| f.path.clone()).collect::<Vec<_>>();
    let changed_files = if !args.files.is_empty() {
        args.files.clone()
    } else {
        context_paths.clone()
    };
    let diff_ref = diff.as_deref();
    let risk_analysis = analyze_risks(args, &changed_files, diff_ref);
    if !risk_analysis.hints.is_empty() || !risk_analysis.impact_scope.is_empty() || !risk_analysis.release_checks.is_empty() {
        out.push_str("程序级风险提示:\n");
        for hint in &risk_analysis.hints {
            out.push_str(&format!("- {}: {}（source: {}）\n", hint.title, hint.detail, hint.source));
        }
        for item in &risk_analysis.impact_scope {
            out.push_str(&format!("- 风险影响面: {}\n", item));
        }
        for item in &risk_analysis.release_checks {
            out.push_str(&format!("- 发布确认项: {}\n", item));
        }
        out.push('\n');
    }

    out.push_str("输出约束:\n请严格按照以下格式输出你的 Review 结果：\n1. 高风险问题（优先展示漏洞、业务逻辑错误、并发/事务/安全问题，每个问题给出文件/函数定位、危险原因、触发场景、修复建议）\n2. 中风险问题（架构破坏、分层违规、严重性能隐患等）\n3. 低风险优化建议（仅包含高价值优化，忽略纯格式、命名风格、“考虑抽离个函数”等无关紧要的重构废话）\n4. 缺失的测试场景（正常/异常/边界未覆盖的情况）\n5. 总结结论（如果没有明显问题，明确说明“未发现高风险问题”）\n");
    if matches!(args.mode, crate::cli::ReviewMode::Critical) {
        out.push_str("6. 风险影响面（必须列出兼容性、迁移、联调、上下游影响）\n7. 发布建议 / 人工确认项（必须列出灰度、回滚、人工确认建议）\n");
    }
    out.push_str("证据不足时明确写“不确定，需要补充上下文”。\n最后补一句：本结果仅作为第一轮筛查建议，人类保留最终合并与发布决策权。\n\n");

    if let Some(diff) = diff {
        out.push_str("## Diff\n```diff\n");
        out.push_str(&diff);
        out.push_str("\n```\n");
    }

    if !contexts.files.is_empty() {
        out.push_str("\n## Context Files\n");
        for file in &contexts.files {
            out.push_str(&format!(
                "### {}{}\n```\n{}\n```\n",
                file.path,
                if file.truncated { " (truncated)" } else { "" },
                file.content
            ));
        }
    }
    if !contexts.skipped.is_empty() || !contexts.truncated.is_empty() {
        out.push_str("\n## Context Summary\n");
        if !contexts.truncated.is_empty() {
            out.push_str(&format!("truncated: {}\n", contexts.truncated.join(", ")));
        }
        if !contexts.skipped.is_empty() {
            out.push_str(&format!("skipped: {}\n", contexts.skipped.join(", ")));
        }
    }
    Ok(out)
}

pub fn print_template(format: OutputFormat) -> Result<()> {
    let tpl = serde_json::json!({
        "stack": "Rust + Axum + PostgreSQL",
        "goal": "修复重复下单",
        "why": "线上偶发重复提交",
        "issue": "支付接口在网络重试下出现重复创建订单",
        "rules": ["一个订单只能支付一次", "幂等键必须生效"],
        "risks": ["并发", "事务一致性"],
        "expected_normal": "首次提交成功",
        "expected_error": "重复提交返回冲突",
        "expected_edge": "网络重试不应双写",
        "test_results": ["订单单测通过", "幂等集成测试待补"]
    });
    match format {
        OutputFormat::Text => println!("{}", serde_json::to_string_pretty(&tpl)?),
        OutputFormat::Json => println!("{}", serde_json::to_string_pretty(&tpl)?),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_args(mode: crate::cli::ReviewMode) -> PromptArgs {
        PromptArgs {
            mode,
            stack: Some("Rust".into()),
            goal: Some("fix".into()),
            why: None,
            rules: vec!["rule".into()],
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
            files: vec![],
            focus: vec![],
            baseline_files: vec![],
            change_type: Some("db".into()),
            format: OutputFormat::Text,
        }
    }

    #[test]
    fn validate_scores() {
        let args = sample_args(crate::cli::ReviewMode::Standard);
        let result = validate_args(&args, true, false);
        assert!(result.ok);
        assert!(result.missing_required.iter().any(|m| m == "expected_behavior" || m == "context" || m == "issue" || m == "test_results"));
    }

    #[test]
    fn prompt_contains_layer_sections() {
        let args = sample_args(crate::cli::ReviewMode::Critical);
        let prompt = build_prompt_from_sources(&args, None, ContextCollection::default()).unwrap();
        assert!(prompt.contains("四层审查要求"));
        assert!(prompt.contains("基础层"));
        assert!(prompt.contains("工程层"));
        assert!(prompt.contains("业务层"));
        assert!(prompt.contains("风险层"));
        assert!(prompt.contains("风险影响面"));
    }

    #[test]
    fn prompt_contains_programmatic_risk_hints() {
        let mut args = sample_args(crate::cli::ReviewMode::Critical);
        args.files = vec!["migrations/001_add_order.sql".into(), "src/order/api.rs".into()];
        let prompt = build_prompt_from_sources(&args, Some("ALTER TABLE orders ADD COLUMN foo INT".into()), ContextCollection::default()).unwrap();
        assert!(prompt.contains("程序级风险提示"));
        assert!(prompt.contains("数据库迁移风险"));
        assert!(prompt.contains("API / 契约变更风险"));
    }
}
