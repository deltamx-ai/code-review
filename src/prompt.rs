use crate::cli::{OutputFormat, PromptArgs};
use crate::context::ContextCollection;
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
    pub rules_count: usize,
    pub risks: Vec<String>,
    pub files: Vec<String>,
    pub context_files: Vec<String>,
    pub has_diff: bool,
}

impl PromptSummary {
    pub fn from_prompt_args(args: &PromptArgs) -> Self {
        Self {
            stack: args.stack.clone(),
            goal: args.goal.clone(),
            rules_count: args.rules.len(),
            risks: args.risks.clone(),
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
    let mut score = 0u8;
    let mut missing = Vec::new();
    let mut suggestions = Vec::new();

    if args.goal.is_some() {
        score += 20;
    } else {
        missing.push("goal".into());
    }
    if args.stack.is_some() {
        score += 10;
    } else {
        suggestions.push("补充技术栈，方便判断框架惯例和隐患".into());
    }
    if has_diff {
        score += 30;
    } else {
        missing.push("diff".into());
    }
    if has_context {
        score += 20;
    } else {
        suggestions.push("补充上下文文件或涉及模块，减少误判".into());
    }
    if !args.rules.is_empty() {
        score += 10;
    } else {
        suggestions.push("补充业务规则，AI 才知道什么算 bug".into());
    }
    if args.expected_normal.is_some()
        || args.expected_error.is_some()
        || args.expected_edge.is_some()
    {
        score += 10;
    }

    ValidationResult {
        ok: score >= 40,
        score,
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
    for path in &args.context_files {
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
    if let Some(v) = &args.goal {
        out.push_str(&format!("改动目标: {}\n", v));
    }
    if let Some(v) = &args.why {
        out.push_str(&format!("背景原因: {}\n", v));
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
    if !args.files.is_empty() {
        out.push_str(&format!("涉及文件:\n- {}\n", args.files.join("\n- ")));
    }
    if !args.focus.is_empty() {
        out.push_str(&format!("额外关注点:\n- {}\n", args.focus.join("\n- ")));
    }

    out.push_str("\n输出要求:\n1. 只报高价值问题\n2. 每个问题给出文件/函数定位\n3. 说明风险等级、触发条件、影响范围、修复建议\n4. 证据不足时明确写“不确定，需要补充上下文”\n\n");

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
        "rules": ["一个订单只能支付一次", "幂等键必须生效"],
        "risks": ["并发", "事务一致性"],
        "expected_normal": "首次提交成功",
        "expected_error": "重复提交返回冲突",
        "expected_edge": "网络重试不应双写"
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

    #[test]
    fn validate_scores() {
        let args = PromptArgs {
            stack: Some("Rust".into()),
            goal: Some("fix".into()),
            why: None,
            rules: vec!["rule".into()],
            risks: vec![],
            expected_normal: None,
            expected_error: None,
            expected_edge: None,
            diff_file: None,
            context_files: vec![],
            files: vec![],
            focus: vec![],
            format: OutputFormat::Text,
        };
        let result = validate_args(&args, true, false);
        assert!(result.score >= 40);
    }
}
