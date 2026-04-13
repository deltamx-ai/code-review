use crate::admission::{AdmissionLevel, ReviewConfidence};
use crate::cli::ReviewMode;
use crate::review_validate::ReviewValidationReport;
use crate::risk::RiskAnalysis;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Default)]
pub struct ReviewIssue {
    pub title: String,
    pub file: Option<String>,
    pub location: Option<String>,
    pub reason: Option<String>,
    pub trigger: Option<String>,
    pub impact: Option<String>,
    pub suggestion: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct MissingTestCase {
    pub title: String,
    pub scenario: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct RiskHintView {
    pub title: String,
    pub detail: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct ReviewResult {
    pub mode: String,
    pub input_ok: bool,
    pub input_level: String,
    pub input_score: u8,
    pub confidence: String,
    pub high_risk: Vec<ReviewIssue>,
    pub medium_risk: Vec<ReviewIssue>,
    pub low_risk: Vec<ReviewIssue>,
    pub missing_tests: Vec<MissingTestCase>,
    pub summary: String,
    pub needs_human_review: bool,
    pub used_rules: Vec<String>,
    pub impact_scope: Vec<String>,
    pub release_checks: Vec<String>,
    pub risk_hints: Vec<RiskHintView>,
    pub validation_report: Option<ReviewValidationReport>,
    pub repair_attempted: bool,
    pub repair_succeeded: bool,
    pub raw_text: String,
}

impl ReviewResult {
    pub fn new(mode: ReviewMode, raw_text: String) -> Self {
        Self {
            mode: review_mode_str(mode).into(),
            raw_text,
            ..Default::default()
        }
    }

    pub fn apply_admission(&mut self, ok: bool, level: AdmissionLevel, score: u8, confidence: ReviewConfidence) {
        self.input_ok = ok;
        self.input_level = admission_level_str(level).into();
        self.input_score = score;
        self.confidence = confidence_str(confidence).into();
    }

    pub fn apply_risk_analysis(&mut self, analysis: RiskAnalysis) {
        for item in analysis.impact_scope {
            if !self.impact_scope.iter().any(|v| v == &item) {
                self.impact_scope.push(item);
            }
        }
        for item in analysis.release_checks {
            if !self.release_checks.iter().any(|v| v == &item) {
                self.release_checks.push(item);
            }
        }
        for hint in analysis.hints {
            self.risk_hints.push(RiskHintView {
                title: hint.title,
                detail: hint.detail,
                source: hint.source,
            });
        }
    }

    pub fn apply_validation_report(&mut self, report: ReviewValidationReport) {
        self.validation_report = Some(report);
    }

    pub fn finalize(&mut self) {
        if self.summary.trim().is_empty() {
            self.summary = if self.high_risk.is_empty() && self.medium_risk.is_empty() {
                "未发现明显高风险问题。".into()
            } else {
                format!(
                    "发现 {} 个高风险问题、{} 个中风险问题，建议人工复核。",
                    self.high_risk.len(),
                    self.medium_risk.len()
                )
            };
        }
        self.needs_human_review = !self.high_risk.is_empty() || !self.impact_scope.is_empty() || !self.release_checks.is_empty() || !self.risk_hints.is_empty();
    }
}

pub fn review_mode_str(mode: ReviewMode) -> &'static str {
    match mode {
        ReviewMode::Lite => "lite",
        ReviewMode::Standard => "standard",
        ReviewMode::Critical => "critical",
    }
}

fn admission_level_str(level: AdmissionLevel) -> &'static str {
    match level {
        AdmissionLevel::Pass => "pass",
        AdmissionLevel::Warn => "warn",
        AdmissionLevel::Block => "block",
    }
}

fn confidence_str(confidence: ReviewConfidence) -> &'static str {
    match confidence {
        ReviewConfidence::High => "high",
        ReviewConfidence::Medium => "medium",
        ReviewConfidence::Low => "low",
    }
}
