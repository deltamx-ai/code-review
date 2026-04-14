use crate::cli::{OutputFormat, PromptArgs, ReviewMode};
use anyhow::Result;
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionLevel {
    Pass,
    Warn,
    Block,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ReviewConfidence {
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdmissionResult {
    pub ok: bool,
    pub level: AdmissionLevel,
    pub score: u8,
    pub confidence: ReviewConfidence,
    pub missing_p0: Vec<String>,
    pub missing_p1: Vec<String>,
    pub missing_p2: Vec<String>,
    pub warnings: Vec<String>,
    pub block_reasons: Vec<String>,
    pub suggestions: Vec<String>,
}

impl AdmissionResult {
    pub fn print(&self, format: OutputFormat) -> Result<()> {
        match format {
            OutputFormat::Text => {
                println!("ok: {}", self.ok);
                println!("level: {:?}", self.level);
                println!("score: {}", self.score);
                println!("confidence: {:?}", self.confidence);
                if !self.missing_p0.is_empty() {
                    println!("missing_p0: {}", self.missing_p0.join(", "));
                }
                if !self.missing_p1.is_empty() {
                    println!("missing_p1: {}", self.missing_p1.join(", "));
                }
                if !self.missing_p2.is_empty() {
                    println!("missing_p2: {}", self.missing_p2.join(", "));
                }
                if !self.warnings.is_empty() {
                    println!("warnings: {}", self.warnings.join(" | "));
                }
                if !self.block_reasons.is_empty() {
                    println!("block_reasons: {}", self.block_reasons.join(" | "));
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

pub fn check_admission(args: &PromptArgs, has_diff: bool, has_context: bool) -> AdmissionResult {
    let mut score = 0u8;
    let mut missing_p0 = Vec::new();
    let mut missing_p1 = Vec::new();
    let mut missing_p2 = Vec::new();
    let mut warnings = Vec::new();
    let mut block_reasons = Vec::new();
    let mut suggestions = Vec::new();

    if has_diff {
        score += 30;
    } else {
        missing_p0.push("diff".into());
    }

    if args.goal.is_some() {
        score += 20;
    } else {
        missing_p0.push("goal".into());
    }

    if !args.rules.is_empty() {
        score += 15;
    } else {
        missing_p0.push("rules".into());
    }

    let mut p1_count = 0u8;
    if args.expected_normal.is_some() || args.expected_error.is_some() || args.expected_edge.is_some() {
        score += 10;
        p1_count += 1;
    } else {
        missing_p1.push("expected_behavior".into());
    }
    if args.stack.is_some() {
        score += 10;
        p1_count += 1;
    } else {
        missing_p1.push("stack".into());
    }
    if has_context {
        score += 10;
        p1_count += 1;
    } else {
        missing_p1.push("context".into());
    }
    if args.issue.is_some() {
        score += 5;
        p1_count += 1;
    } else {
        missing_p1.push("issue".into());
    }
    if !args.test_results.is_empty() {
        score += 5;
        p1_count += 1;
    } else {
        missing_p1.push("test_results".into());
    }

    let has_p2 = !args.baseline_files.is_empty() || !args.incident_files.is_empty() || !args.focus.is_empty() || args.jira.is_some();
    if has_p2 {
        score += 5;
    } else {
        missing_p2.push("baseline_or_focus_or_jira".into());
    }

    let (ok, level, confidence) = match args.mode {
        ReviewMode::Lite => {
            if !has_diff {
                block_reasons.push("lite 模式至少需要 diff 才能进行 review".into());
                (false, AdmissionLevel::Block, ReviewConfidence::Low)
            } else if !missing_p0.is_empty() {
                warnings.push("lite 模式允许缺少部分业务上下文，但结果置信度会降低".into());
                (true, AdmissionLevel::Warn, ReviewConfidence::Low)
            } else if missing_p1.len() >= 3 {
                warnings.push("lite 模式上下文较薄，建议补充预期行为、技术栈或上下文文件".into());
                (true, AdmissionLevel::Warn, ReviewConfidence::Medium)
            } else {
                (true, AdmissionLevel::Pass, ReviewConfidence::High)
            }
        }
        ReviewMode::Standard => {
            if !missing_p0.is_empty() {
                block_reasons.push(format!(
                    "standard 模式缺少必需上下文: {}",
                    missing_p0.join(", ")
                ));
                (false, AdmissionLevel::Block, ReviewConfidence::Low)
            } else if missing_p1.len() > 2 {
                warnings.push("standard 模式缺少超过 2 项 P1，上下文不足，结论可能受限".into());
                (true, AdmissionLevel::Warn, ReviewConfidence::Medium)
            } else if missing_p1.is_empty() {
                (true, AdmissionLevel::Pass, ReviewConfidence::High)
            } else {
                (true, AdmissionLevel::Warn, ReviewConfidence::Medium)
            }
        }
        ReviewMode::Critical => {
            if !missing_p0.is_empty() {
                block_reasons.push(format!(
                    "critical 模式缺少必需上下文: {}",
                    missing_p0.join(", ")
                ));
                (false, AdmissionLevel::Block, ReviewConfidence::Low)
            } else if p1_count < 2 {
                block_reasons.push("critical 模式至少需要 2 项 P1 上下文（如预期行为、技术栈、上下文代码、Issue、测试结果）".into());
                (false, AdmissionLevel::Block, ReviewConfidence::Low)
            } else if !has_p2 {
                block_reasons.push("critical 模式至少需要一类 P2 增强信息：baseline / focus / jira".into());
                (false, AdmissionLevel::Block, ReviewConfidence::Low)
            } else if missing_p1.len() > 2 {
                warnings.push("critical 模式虽然通过最低门槛，但 P1 信息仍偏少，建议补充更多上下文".into());
                (true, AdmissionLevel::Warn, ReviewConfidence::Medium)
            } else {
                (true, AdmissionLevel::Pass, ReviewConfidence::High)
            }
        }
    };

    if args.stack.is_none() {
        suggestions.push("补充技术栈，方便判断框架惯例和工程隐患".into());
    }
    if !has_context && !matches!(args.mode, ReviewMode::Lite) {
        suggestions.push("补充上下文文件或关联代码，减少 AI 幻觉和误判".into());
    }
    if args.issue.is_none() && !matches!(args.mode, ReviewMode::Lite) {
        suggestions.push("补充 Issue/需求描述，方便判断改动是否偏题".into());
    }
    if args.test_results.is_empty() && !matches!(args.mode, ReviewMode::Lite) {
        suggestions.push("补充测试结果，方便判断风险是否已覆盖".into());
    }

    AdmissionResult {
        ok,
        level,
        score,
        confidence,
        missing_p0,
        missing_p1,
        missing_p2,
        warnings,
        block_reasons,
        suggestions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{OutputFormat, PromptArgs};

    fn base_args(mode: ReviewMode) -> PromptArgs {
        PromptArgs {
            mode,
            stack: Some("Rust".into()),
            goal: Some("fix bug".into()),
            why: None,
            rules: vec!["rule1".into()],
            risks: vec![],
            expected_normal: Some("ok".into()),
            expected_error: None,
            expected_edge: None,
            issue: Some("issue desc".into()),
            test_results: vec!["unit ok".into()],
            jira: None,
            jira_base_url: None,
            jira_provider: "native".into(),
            jira_command: None,
            diff_file: None,
            context_files: vec![],
            files: vec!["src/main.rs".into()],
            focus: vec![],
            baseline_files: vec![],
            incident_files: vec![],
            change_type: None,
            format: OutputFormat::Text,
        }
    }

    #[test]
    fn standard_blocks_without_goal() {
        let mut args = base_args(ReviewMode::Standard);
        args.goal = None;
        let result = check_admission(&args, true, true);
        assert!(!result.ok);
        assert_eq!(result.level, AdmissionLevel::Block);
        assert!(result.missing_p0.iter().any(|v| v == "goal"));
    }

    #[test]
    fn standard_blocks_without_rules() {
        let mut args = base_args(ReviewMode::Standard);
        args.rules.clear();
        let result = check_admission(&args, true, true);
        assert!(!result.ok);
        assert!(result.missing_p0.iter().any(|v| v == "rules"));
    }

    #[test]
    fn lite_allows_missing_goal_with_warning() {
        let mut args = base_args(ReviewMode::Lite);
        args.goal = None;
        let result = check_admission(&args, true, false);
        assert!(result.ok);
        assert_eq!(result.level, AdmissionLevel::Warn);
        assert_eq!(result.confidence, ReviewConfidence::Low);
    }

    #[test]
    fn critical_requires_p2_support() {
        let args = base_args(ReviewMode::Critical);
        let result = check_admission(&args, true, true);
        assert!(!result.ok);
        assert!(result.block_reasons.iter().any(|v| v.contains("P2")));
    }

    #[test]
    fn critical_passes_with_focus_as_p2() {
        let mut args = base_args(ReviewMode::Critical);
        args.focus.push("payment safety".into());
        let result = check_admission(&args, true, true);
        assert!(result.ok);
    }
}
