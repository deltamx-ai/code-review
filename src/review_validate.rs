use crate::cli::ReviewMode;
use crate::review_schema::{ReviewIssue, ReviewResult};
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ValidationSeverity {
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationFinding {
    pub severity: ValidationSeverity,
    pub field: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReviewValidationReport {
    pub ok: bool,
    pub repaired: bool,
    pub findings: Vec<ValidationFinding>,
}

pub fn validate_and_repair_review_result(mode: ReviewMode, result: &mut ReviewResult) -> ReviewValidationReport {
    let mut findings = Vec::new();
    let mut repaired = false;

    repair_summary(result, &mut findings, &mut repaired);
    repair_issue_fields(&mut result.high_risk, "high_risk", &mut findings, &mut repaired);
    repair_issue_fields(&mut result.medium_risk, "medium_risk", &mut findings, &mut repaired);
    repair_issue_fields(&mut result.low_risk, "low_risk", &mut findings, &mut repaired);

    if result.high_risk.is_empty() && result.medium_risk.is_empty() && result.low_risk.is_empty() {
        findings.push(ValidationFinding {
            severity: ValidationSeverity::Warning,
            field: "issues".into(),
            message: "未解析出任何风险问题，可能是模型输出过于自由或格式不稳定。".into(),
        });
    }

    if matches!(mode, ReviewMode::Critical) {
        if result.impact_scope.is_empty() {
            findings.push(ValidationFinding {
                severity: ValidationSeverity::Error,
                field: "impact_scope".into(),
                message: "critical 模式缺少风险影响面。".into(),
            });
        }
        if result.release_checks.is_empty() {
            findings.push(ValidationFinding {
                severity: ValidationSeverity::Error,
                field: "release_checks".into(),
                message: "critical 模式缺少发布建议 / 人工确认项。".into(),
            });
        }
    }

    let ok = !findings.iter().any(|f| matches!(f.severity, ValidationSeverity::Error));
    ReviewValidationReport { ok, repaired, findings }
}

fn repair_summary(result: &mut ReviewResult, findings: &mut Vec<ValidationFinding>, repaired: &mut bool) {
    if result.summary.trim().is_empty() {
        result.finalize();
        *repaired = true;
        findings.push(ValidationFinding {
            severity: ValidationSeverity::Warning,
            field: "summary".into(),
            message: "summary 为空，已根据解析结果自动补全。".into(),
        });
    }
}

fn repair_issue_fields(issues: &mut [ReviewIssue], field: &str, findings: &mut Vec<ValidationFinding>, repaired: &mut bool) {
    for (idx, issue) in issues.iter_mut().enumerate() {
        let entry = format!("{}[{}]", field, idx);
        if issue.title.trim().is_empty() {
            issue.title = "未命名问题".into();
            *repaired = true;
            findings.push(ValidationFinding {
                severity: ValidationSeverity::Warning,
                field: format!("{}.title", entry),
                message: "title 缺失，已填充默认标题。".into(),
            });
        }
        if issue.reason.as_deref().unwrap_or("").trim().is_empty() {
            issue.reason = Some("证据不足，需要补充上下文或更规范的模型输出。".into());
            *repaired = true;
            findings.push(ValidationFinding {
                severity: ValidationSeverity::Warning,
                field: format!("{}.reason", entry),
                message: "reason 缺失，已填充兜底说明。".into(),
            });
        }
        if issue.suggestion.as_deref().unwrap_or("").trim().is_empty() {
            issue.suggestion = Some("建议结合上下文复核后补充具体修复动作。".into());
            *repaired = true;
            findings.push(ValidationFinding {
                severity: ValidationSeverity::Warning,
                field: format!("{}.suggestion", entry),
                message: "suggestion 缺失，已填充兜底建议。".into(),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review_schema::ReviewResult;

    #[test]
    fn repairs_missing_summary_and_issue_fields() {
        let mut result = ReviewResult::new(ReviewMode::Standard, "raw".into());
        result.high_risk.push(ReviewIssue {
            title: "".into(),
            file: None,
            location: None,
            reason: None,
            trigger: None,
            impact: None,
            suggestion: None,
        });

        let report = validate_and_repair_review_result(ReviewMode::Standard, &mut result);
        assert!(report.ok);
        assert!(report.repaired);
        assert!(!result.summary.is_empty());
        assert_eq!(result.high_risk[0].title, "未命名问题");
        assert!(result.high_risk[0].reason.is_some());
        assert!(result.high_risk[0].suggestion.is_some());
    }

    #[test]
    fn critical_fails_without_required_sections() {
        let mut result = ReviewResult::new(ReviewMode::Critical, "raw".into());
        result.summary = "something".into();
        let report = validate_and_repair_review_result(ReviewMode::Critical, &mut result);
        assert!(!report.ok);
        assert!(report.findings.iter().any(|f| f.field == "impact_scope"));
        assert!(report.findings.iter().any(|f| f.field == "release_checks"));
    }

    #[test]
    fn report_can_be_attached_to_result() {
        let mut result = ReviewResult::new(ReviewMode::Standard, "raw".into());
        let report = validate_and_repair_review_result(ReviewMode::Standard, &mut result);
        result.apply_validation_report(report.clone());
        assert!(result.validation_report.is_some());
        assert_eq!(result.validation_report.unwrap().ok, report.ok);
    }
}
