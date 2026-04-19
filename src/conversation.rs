use crate::admission::{AdmissionLevel, AdmissionResult, ReviewConfidence};
use crate::cli::ReviewMode;
use crate::review_schema::ReviewResult;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConversationStatus {
    Created,
    Running,
    WaitingInput,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TurnKind {
    Discovery,
    DeepDive,
    BusinessCheck,
    FinalReport,
    ManualFollowup,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TurnStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContentFormat {
    Text,
    Markdown,
    Json,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FindingSeverity {
    Critical,
    High,
    Medium,
    Low,
    Info,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FindingStatus {
    Suspected,
    Confirmed,
    Dismissed,
    Fixed,
    AcceptedRisk,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FindingCategory {
    Logic,
    Security,
    Performance,
    Compatibility,
    Data,
    Testability,
    Release,
    Maintainability,
    Style,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactType {
    Diff,
    ContextFile,
    Prompt,
    Response,
    Report,
    Jira,
    TestResult,
    Snapshot,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewSession {
    pub id: String,
    pub title: Option<String>,
    pub status: ConversationStatus,
    pub review_mode: ReviewMode,
    pub strategy: String,
    pub repo_root: PathBuf,
    pub base_ref: Option<String>,
    pub head_ref: Option<String>,
    pub provider: String,
    pub model: String,
    pub temperature: Option<f32>,
    pub current_turn: u32,
    pub total_turns: u32,
    pub admission: Option<AdmissionSnapshot>,
    pub state: ReviewConversationState,
    pub final_summary: Option<String>,
    pub final_report: Option<ReviewResult>,
    pub last_error: Option<String>,
    pub created_by: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
}

impl ReviewSession {
    pub fn new(
        id: String,
        review_mode: ReviewMode,
        strategy: impl Into<String>,
        repo_root: PathBuf,
        provider: impl Into<String>,
        model: impl Into<String>,
        created_at: String,
    ) -> Self {
        Self {
            id,
            title: None,
            status: ConversationStatus::Created,
            review_mode,
            strategy: strategy.into(),
            repo_root,
            base_ref: None,
            head_ref: None,
            provider: provider.into(),
            model: model.into(),
            temperature: None,
            current_turn: 0,
            total_turns: 0,
            admission: None,
            state: ReviewConversationState::default(),
            final_summary: None,
            final_report: None,
            last_error: None,
            created_by: None,
            created_at: created_at.clone(),
            updated_at: created_at,
            completed_at: None,
        }
    }

    pub fn attach_admission(&mut self, admission: &AdmissionResult) {
        self.admission = Some(AdmissionSnapshot::from(admission));
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdmissionSnapshot {
    pub ok: bool,
    pub level: String,
    pub score: u8,
    pub confidence: String,
    pub block_reasons: Vec<String>,
    pub missing_required: Vec<String>,
}

impl From<&AdmissionResult> for AdmissionSnapshot {
    fn from(value: &AdmissionResult) -> Self {
        let mut missing_required = Vec::new();
        missing_required.extend(value.missing_p0.clone());
        missing_required.extend(value.missing_p1.clone());
        missing_required.extend(value.missing_p2.clone());
        Self {
            ok: value.ok,
            level: admission_level_to_str(value.level).into(),
            score: value.score,
            confidence: confidence_to_str(value.confidence).into(),
            block_reasons: value.block_reasons.clone(),
            missing_required,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReviewConversationState {
    pub requested_files: Vec<String>,
    pub attached_files: Vec<String>,
    pub findings: Vec<ReviewFinding>,
    pub pending_finding_ids: Vec<String>,
    pub confirmed_finding_ids: Vec<String>,
    pub dismissed_finding_ids: Vec<String>,
    pub release_checks: Vec<String>,
    pub impact_scope: Vec<String>,
    pub extra: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewTurn {
    pub id: String,
    pub session_id: String,
    pub turn_no: u32,
    pub kind: TurnKind,
    pub status: TurnStatus,
    pub input_summary: Option<String>,
    pub instruction: Option<String>,
    pub requested_files: Vec<String>,
    pub attached_files: Vec<String>,
    pub focus_finding_ids: Vec<String>,
    pub prompt_text: Option<String>,
    pub response_text: Option<String>,
    pub parsed_result: Option<ReviewResult>,
    pub token_input: Option<u32>,
    pub token_output: Option<u32>,
    pub latency_ms: Option<u64>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewMessage {
    pub id: String,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub seq_no: u64,
    pub role: MessageRole,
    pub author: Option<String>,
    pub content: String,
    pub format: ContentFormat,
    pub meta: BTreeMap<String, String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewFinding {
    pub id: String,
    pub code: Option<String>,
    pub session_id: String,
    pub source_turn_id: Option<String>,
    pub severity: FindingSeverity,
    pub category: FindingCategory,
    pub status: FindingStatus,
    pub title: String,
    pub description: String,
    pub rationale: Option<String>,
    pub suggestion: Option<String>,
    pub confidence: Option<f32>,
    pub owner: Option<String>,
    pub location: Option<CodeLocation>,
    pub evidence: Vec<FindingEvidence>,
    pub related_files: Vec<String>,
    pub tags: Vec<String>,
    pub last_seen_turn: Option<u32>,
    pub created_at: String,
    pub updated_at: String,
    pub resolved_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeLocation {
    pub file_path: String,
    pub line_start: Option<u32>,
    pub line_end: Option<u32>,
    pub symbol: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindingEvidence {
    pub kind: String,
    pub summary: String,
    pub content: Option<String>,
    pub artifact_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewArtifact {
    pub id: String,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub artifact_type: ArtifactType,
    pub name: String,
    pub path: Option<PathBuf>,
    pub content: Option<String>,
    pub mime_type: Option<String>,
    pub size_bytes: Option<u64>,
    pub hash: Option<String>,
    pub meta: BTreeMap<String, String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewCheckpoint {
    pub id: String,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub checkpoint_type: String,
    pub snapshot_json: String,
    pub created_at: String,
}

fn admission_level_to_str(level: AdmissionLevel) -> &'static str {
    match level {
        AdmissionLevel::Pass => "pass",
        AdmissionLevel::Warn => "warn",
        AdmissionLevel::Block => "block",
    }
}

fn confidence_to_str(confidence: ReviewConfidence) -> &'static str {
    match confidence {
        ReviewConfidence::High => "high",
        ReviewConfidence::Medium => "medium",
        ReviewConfidence::Low => "low",
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FindingCounts {
    pub total: u32,
    pub high: u32,
    pub medium: u32,
    pub low: u32,
    pub confirmed: u32,
    pub dismissed: u32,
}

impl FindingCounts {
    pub fn from_findings(findings: &[ReviewFinding]) -> Self {
        let mut out = FindingCounts::default();
        for f in findings {
            out.total += 1;
            match f.severity {
                FindingSeverity::Critical | FindingSeverity::High => out.high += 1,
                FindingSeverity::Medium => out.medium += 1,
                FindingSeverity::Low | FindingSeverity::Info => out.low += 1,
            }
            match f.status {
                FindingStatus::Confirmed => out.confirmed += 1,
                FindingStatus::Dismissed => out.dismissed += 1,
                _ => {}
            }
        }
        out
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub title: Option<String>,
    pub status: ConversationStatus,
    pub review_mode: ReviewMode,
    pub repo_root: String,
    pub provider: String,
    pub model: String,
    pub current_turn: u32,
    pub total_turns: u32,
    pub finding_counts: FindingCounts,
    pub admission_ok: Option<bool>,
    pub last_error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
}

impl SessionSummary {
    pub fn from_session(session: &ReviewSession, findings: &[ReviewFinding]) -> Self {
        Self {
            id: session.id.clone(),
            title: session.title.clone(),
            status: session.status.clone(),
            review_mode: session.review_mode,
            repo_root: session.repo_root.display().to_string(),
            provider: session.provider.clone(),
            model: session.model.clone(),
            current_turn: session.current_turn,
            total_turns: session.total_turns,
            finding_counts: FindingCounts::from_findings(findings),
            admission_ok: session.admission.as_ref().map(|a| a.ok),
            last_error: session.last_error.clone(),
            created_at: session.created_at.clone(),
            updated_at: session.updated_at.clone(),
            completed_at: session.completed_at.clone(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SessionListFilter {
    pub repo: Option<String>,
    pub status: Option<String>,
    pub mode: Option<String>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FindingPatch {
    pub status: Option<FindingStatus>,
    pub owner: Option<String>,
    pub tags: Option<Vec<String>>,
}
