use crate::admission::check_admission;
use crate::cli::{PromptArgs, ReviewMode};
use crate::context::read_repo_context_with_budget;
use crate::conversation::{
    ArtifactType, CodeLocation, ContentFormat, ConversationStatus, FindingCategory,
    FindingEvidence, FindingSeverity, FindingStatus, MessageRole, ReviewArtifact, ReviewFinding,
    ReviewMessage, ReviewSession, ReviewTurn, TurnKind, TurnStatus,
};
use crate::conversation_store::ConversationStore;
use crate::providers::{ChatInputMessage, ChatRequest, LlmProvider};
use crate::review_parser::parse_review_text;
use crate::review_schema::{ReviewIssue, ReviewResult};
use anyhow::{bail, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const ATTACHED_FILES_BUDGET_BYTES: usize = 32_000;
const ATTACHED_FILE_MAX_BYTES: usize = 10_000;

#[derive(Debug, Clone)]
pub struct StartReviewSessionRequest {
    pub repo_root: PathBuf,
    pub review_mode: ReviewMode,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub base_ref: Option<String>,
    pub head_ref: Option<String>,
    pub diff_text: Option<String>,
    pub prompt_args: PromptArgs,
    pub initial_instruction: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ContinueReviewTurnRequest {
    pub session_id: String,
    pub instruction: Option<String>,
    pub attached_files: Vec<String>,
    pub extra_context: Vec<String>,
    pub focus_finding_ids: Vec<String>,
    pub generate_final_report: bool,
    pub model: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ReviewOrchestrationResult {
    pub session: ReviewSession,
    pub turn: ReviewTurn,
    pub new_messages: Vec<ReviewMessage>,
    pub new_findings: Vec<ReviewFinding>,
    pub final_report: Option<ReviewResult>,
}

pub fn start_session(
    store: &ConversationStore,
    provider: &dyn LlmProvider,
    req: StartReviewSessionRequest,
) -> Result<ReviewOrchestrationResult> {
    let now = now_string();
    let session_id = new_id("rs");
    let mut session = ReviewSession::new(
        session_id.clone(),
        req.review_mode,
        "conversation",
        req.repo_root.clone(),
        req.provider.unwrap_or_else(|| provider.name().to_string()),
        req.model.clone().unwrap_or_else(|| "default".into()),
        now.clone(),
    );
    session.base_ref = req.base_ref.clone();
    session.head_ref = req.head_ref.clone();

    let has_diff = req
        .diff_text
        .as_deref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
        || req.prompt_args.diff_file.is_some();
    let has_context =
        !req.prompt_args.context_files.is_empty() || !req.prompt_args.files.is_empty();
    let admission = check_admission(&req.prompt_args, has_diff, has_context);
    session.attach_admission(&admission);

    if !admission.ok {
        session.status = ConversationStatus::Failed;
        session.last_error = Some(format!(
            "admission blocked: {}",
            admission.block_reasons.join(" | ")
        ));
        session.updated_at = now.clone();
        store.save_session(&session)?;
        let turn = ReviewTurn {
            id: new_id("turn"),
            session_id: session.id.clone(),
            turn_no: 0,
            kind: TurnKind::Discovery,
            status: TurnStatus::Skipped,
            input_summary: Some("admission blocked".into()),
            instruction: None,
            requested_files: Vec::new(),
            attached_files: Vec::new(),
            focus_finding_ids: Vec::new(),
            prompt_text: None,
            response_text: None,
            parsed_result: None,
            token_input: None,
            token_output: None,
            latency_ms: None,
            started_at: Some(now.clone()),
            completed_at: Some(now.clone()),
            created_at: now.clone(),
            updated_at: now.clone(),
        };
        return Ok(ReviewOrchestrationResult {
            session,
            turn,
            new_messages: Vec::new(),
            new_findings: Vec::new(),
            final_report: None,
        });
    }

    session.status = ConversationStatus::Running;

    let turn_id = new_id("turn");
    let system_text = build_system_prompt(req.review_mode);
    let user_text = build_initial_user_prompt(
        &req.prompt_args,
        req.diff_text.as_deref(),
        req.initial_instruction.as_deref(),
    );

    let messages = vec![
        ReviewMessage {
            id: new_id("msg"),
            session_id: session_id.clone(),
            turn_id: Some(turn_id.clone()),
            seq_no: 1,
            role: MessageRole::System,
            author: Some("orchestrator".into()),
            content: system_text.clone(),
            format: ContentFormat::Markdown,
            meta: BTreeMap::new(),
            created_at: now.clone(),
        },
        ReviewMessage {
            id: new_id("msg"),
            session_id: session_id.clone(),
            turn_id: Some(turn_id.clone()),
            seq_no: 2,
            role: MessageRole::User,
            author: Some("orchestrator".into()),
            content: user_text.clone(),
            format: ContentFormat::Markdown,
            meta: BTreeMap::new(),
            created_at: now.clone(),
        },
    ];

    let prompt_text = render_prompt_text(&messages);

    let chat_req = ChatRequest {
        model: session.model.clone(),
        messages: to_chat_messages(&messages),
        temperature: None,
        max_tokens: None,
        metadata: BTreeMap::new(),
    };
    let response = provider.chat(&chat_req)?;
    let parsed = parse_review_text(req.review_mode, &response.content, req.prompt_args.rules.clone());
    let new_findings = findings_from_result(&session_id, &turn_id, 1, &now, &parsed);
    apply_findings_to_session(&mut session, &new_findings, &parsed);

    let assistant_msg = ReviewMessage {
        id: new_id("msg"),
        session_id: session_id.clone(),
        turn_id: Some(turn_id.clone()),
        seq_no: 3,
        role: MessageRole::Assistant,
        author: Some(provider.name().into()),
        content: response.content.clone(),
        format: ContentFormat::Markdown,
        meta: BTreeMap::new(),
        created_at: now.clone(),
    };

    let attached_files: Vec<String> = req
        .prompt_args
        .context_files
        .iter()
        .map(|p| p.display().to_string())
        .collect();

    let turn = ReviewTurn {
        id: turn_id.clone(),
        session_id: session_id.clone(),
        turn_no: 1,
        kind: TurnKind::Discovery,
        status: TurnStatus::Completed,
        input_summary: Some("initial review turn".into()),
        instruction: req.initial_instruction.clone(),
        requested_files: Vec::new(),
        attached_files: attached_files.clone(),
        focus_finding_ids: new_findings.iter().map(|f| f.id.clone()).collect(),
        prompt_text: Some(prompt_text.clone()),
        response_text: Some(response.content.clone()),
        parsed_result: Some(parsed.clone()),
        token_input: response.usage.as_ref().and_then(|u| u.input_tokens),
        token_output: response.usage.as_ref().and_then(|u| u.output_tokens),
        latency_ms: None,
        started_at: Some(now.clone()),
        completed_at: Some(now.clone()),
        created_at: now.clone(),
        updated_at: now.clone(),
    };

    session.current_turn = 1;
    session.total_turns = 1;
    session.updated_at = now.clone();
    session.state.attached_files = attached_files;

    store.save_session(&session)?;
    store.save_turn(&turn)?;
    for msg in &messages {
        store.append_message(msg)?;
    }
    store.append_message(&assistant_msg)?;
    store.save_findings(&session.id, &session.state.findings)?;
    save_turn_artifacts(
        store,
        &session.id,
        &turn_id,
        &now,
        Some(&prompt_text),
        Some(&response.content),
        req.diff_text.as_deref(),
    );

    Ok(ReviewOrchestrationResult {
        session,
        turn,
        new_messages: vec![messages[0].clone(), messages[1].clone(), assistant_msg],
        new_findings,
        final_report: None,
    })
}

pub fn continue_session(
    store: &ConversationStore,
    provider: &dyn LlmProvider,
    req: ContinueReviewTurnRequest,
) -> Result<ReviewOrchestrationResult> {
    if !store.session_exists(&req.session_id) {
        bail!("session not found: {}", req.session_id);
    }

    let now = now_string();
    let mut session = store.load_session(&req.session_id)?;
    let history = store.load_messages(&req.session_id)?;
    let mut existing_findings = store.load_findings(&req.session_id)?;
    let next_seq = store.next_message_seq(&req.session_id)?;
    let turn_no = session.total_turns + 1;
    let turn_id = new_id("turn");

    let attached_contents = read_attached_file_contents(&session.repo_root, &req.attached_files);

    let user_text = build_continue_user_prompt(&req, &attached_contents, &session);
    let user_msg = ReviewMessage {
        id: new_id("msg"),
        session_id: session.id.clone(),
        turn_id: Some(turn_id.clone()),
        seq_no: next_seq,
        role: MessageRole::User,
        author: Some("orchestrator".into()),
        content: user_text.clone(),
        format: ContentFormat::Markdown,
        meta: BTreeMap::new(),
        created_at: now.clone(),
    };

    let mut all_messages = history.clone();
    all_messages.push(user_msg.clone());

    let turn_model = req.model.clone().unwrap_or_else(|| session.model.clone());
    let chat_req = ChatRequest {
        model: turn_model.clone(),
        messages: to_chat_messages(&all_messages),
        temperature: session.temperature,
        max_tokens: None,
        metadata: BTreeMap::new(),
    };
    let response = provider.chat(&chat_req)?;

    let prompt_text = render_prompt_text(&all_messages);

    let parsed = parse_review_text(session.review_mode, &response.content, vec![]);
    let mut new_findings = findings_from_result(&session.id, &turn_id, turn_no, &now, &parsed);
    let deduped = merge_findings(&mut existing_findings, &mut new_findings, turn_no, &now);
    apply_findings_to_session(&mut session, &existing_findings, &parsed);

    let assistant_msg = ReviewMessage {
        id: new_id("msg"),
        session_id: session.id.clone(),
        turn_id: Some(turn_id.clone()),
        seq_no: next_seq + 1,
        role: MessageRole::Assistant,
        author: Some(provider.name().into()),
        content: response.content.clone(),
        format: ContentFormat::Markdown,
        meta: BTreeMap::new(),
        created_at: now.clone(),
    };

    let final_report = if req.generate_final_report {
        let mut report = parsed.clone();
        if report.summary.trim().is_empty() {
            report.summary = response.content.lines().next().unwrap_or("review completed").to_string();
            report.finalize();
        }
        session.final_summary = Some(report.summary.clone());
        session.final_report = Some(report.clone());
        session.status = ConversationStatus::Completed;
        session.completed_at = Some(now.clone());
        Some(report)
    } else {
        None
    };

    let turn = ReviewTurn {
        id: turn_id.clone(),
        session_id: session.id.clone(),
        turn_no,
        kind: if req.generate_final_report { TurnKind::FinalReport } else { TurnKind::DeepDive },
        status: TurnStatus::Completed,
        input_summary: Some("follow-up review turn".into()),
        instruction: req.instruction.clone(),
        requested_files: Vec::new(),
        attached_files: req.attached_files.clone(),
        focus_finding_ids: deduped.iter().map(|f| f.id.clone()).collect(),
        prompt_text: Some(prompt_text.clone()),
        response_text: Some(response.content.clone()),
        parsed_result: Some(parsed.clone()),
        token_input: response.usage.as_ref().and_then(|u| u.input_tokens),
        token_output: response.usage.as_ref().and_then(|u| u.output_tokens),
        latency_ms: None,
        started_at: Some(now.clone()),
        completed_at: Some(now.clone()),
        created_at: now.clone(),
        updated_at: now.clone(),
    };

    session.current_turn = turn_no;
    session.total_turns = turn_no;
    session.updated_at = now.clone();
    for file in &req.attached_files {
        if !session.state.attached_files.contains(file) {
            session.state.attached_files.push(file.clone());
        }
    }
    for ctx in &req.extra_context {
        if !session.state.requested_files.contains(ctx) {
            session.state.requested_files.push(ctx.clone());
        }
    }
    if req.model.is_some() {
        session
            .state
            .extra
            .insert(format!("turn_{}_model", turn_no), turn_model.clone());
    }

    store.save_turn(&turn)?;
    store.append_message(&user_msg)?;
    store.append_message(&assistant_msg)?;
    store.save_findings(&session.id, &existing_findings)?;
    store.save_session(&session)?;
    save_turn_artifacts(
        store,
        &session.id,
        &turn_id,
        &now,
        Some(&prompt_text),
        Some(&response.content),
        None,
    );

    Ok(ReviewOrchestrationResult {
        session,
        turn,
        new_messages: vec![user_msg, assistant_msg],
        new_findings: deduped,
        final_report,
    })
}

fn read_attached_file_contents(repo_root: &Path, files: &[String]) -> Vec<(String, String, bool)> {
    if files.is_empty() {
        return Vec::new();
    }
    let repo = repo_root.to_path_buf();
    match read_repo_context_with_budget(
        &repo,
        files,
        ATTACHED_FILES_BUDGET_BYTES,
        ATTACHED_FILE_MAX_BYTES,
    ) {
        Ok(collection) => collection
            .files
            .into_iter()
            .map(|f| (f.path, f.content, f.truncated))
            .collect(),
        Err(_) => Vec::new(),
    }
}

fn save_turn_artifacts(
    store: &ConversationStore,
    session_id: &str,
    turn_id: &str,
    now: &str,
    prompt: Option<&str>,
    response: Option<&str>,
    diff: Option<&str>,
) {
    if let Some(text) = prompt {
        let artifact = ReviewArtifact {
            id: new_id("art"),
            session_id: session_id.into(),
            turn_id: Some(turn_id.into()),
            artifact_type: ArtifactType::Prompt,
            name: "prompt.txt".into(),
            path: None,
            content: Some(text.to_string()),
            mime_type: Some("text/plain".into()),
            size_bytes: Some(text.len() as u64),
            hash: None,
            meta: BTreeMap::new(),
            created_at: now.into(),
        };
        let _ = store.save_artifact(&artifact);
    }
    if let Some(text) = response {
        let artifact = ReviewArtifact {
            id: new_id("art"),
            session_id: session_id.into(),
            turn_id: Some(turn_id.into()),
            artifact_type: ArtifactType::Response,
            name: "response.txt".into(),
            path: None,
            content: Some(text.to_string()),
            mime_type: Some("text/plain".into()),
            size_bytes: Some(text.len() as u64),
            hash: None,
            meta: BTreeMap::new(),
            created_at: now.into(),
        };
        let _ = store.save_artifact(&artifact);
    }
    if let Some(text) = diff {
        if !text.trim().is_empty() {
            let artifact = ReviewArtifact {
                id: new_id("art"),
                session_id: session_id.into(),
                turn_id: Some(turn_id.into()),
                artifact_type: ArtifactType::Diff,
                name: "diff.patch".into(),
                path: None,
                content: Some(text.to_string()),
                mime_type: Some("text/x-diff".into()),
                size_bytes: Some(text.len() as u64),
                hash: None,
                meta: BTreeMap::new(),
                created_at: now.into(),
            };
            let _ = store.save_artifact(&artifact);
        }
    }
}

fn render_prompt_text(messages: &[ReviewMessage]) -> String {
    let mut out = String::new();
    for m in messages {
        let role = match m.role {
            MessageRole::System => "system",
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::Tool => "tool",
        };
        out.push_str(&format!("[{}]\n{}\n\n", role, m.content));
    }
    out
}

fn findings_from_result(
    session_id: &str,
    turn_id: &str,
    turn_no: u32,
    now: &str,
    result: &ReviewResult,
) -> Vec<ReviewFinding> {
    let mut out = Vec::new();
    append_issue_findings(&mut out, session_id, turn_id, turn_no, now, &result.high_risk, FindingSeverity::High);
    append_issue_findings(&mut out, session_id, turn_id, turn_no, now, &result.medium_risk, FindingSeverity::Medium);
    append_issue_findings(&mut out, session_id, turn_id, turn_no, now, &result.low_risk, FindingSeverity::Low);
    out
}

fn append_issue_findings(
    out: &mut Vec<ReviewFinding>,
    session_id: &str,
    turn_id: &str,
    turn_no: u32,
    now: &str,
    issues: &[ReviewIssue],
    severity: FindingSeverity,
) {
    for issue in issues {
        let file_path = issue.file.clone().unwrap_or_else(|| "unknown".into());
        let symbol = issue.location.clone();
        let mut related = Vec::new();
        if let Some(file) = &issue.file {
            related.push(file.clone());
        }
        let description = issue.reason.clone().unwrap_or_else(|| issue.title.clone());
        out.push(ReviewFinding {
            id: new_id("finding"),
            code: None,
            session_id: session_id.into(),
            source_turn_id: Some(turn_id.into()),
            severity: severity.clone(),
            category: infer_category(issue),
            status: FindingStatus::Suspected,
            title: issue.title.clone(),
            description: description.clone(),
            rationale: issue.reason.clone(),
            suggestion: issue.suggestion.clone(),
            confidence: Some(match severity {
                FindingSeverity::High => 0.85,
                FindingSeverity::Medium => 0.65,
                FindingSeverity::Low => 0.45,
                _ => 0.3,
            }),
            owner: None,
            location: Some(CodeLocation {
                file_path,
                line_start: None,
                line_end: None,
                symbol,
            }),
            evidence: vec![FindingEvidence {
                kind: "model_reasoning".into(),
                summary: description,
                content: issue.impact.clone().or(issue.trigger.clone()),
                artifact_id: None,
            }],
            related_files: related,
            tags: vec![format!("turn:{}", turn_no)],
            last_seen_turn: Some(turn_no),
            created_at: now.into(),
            updated_at: now.into(),
            resolved_at: None,
        });
    }
}

fn merge_findings(
    existing: &mut Vec<ReviewFinding>,
    incoming: &mut Vec<ReviewFinding>,
    turn_no: u32,
    now: &str,
) -> Vec<ReviewFinding> {
    let mut appended = Vec::new();
    for finding in incoming.iter() {
        if let Some(found) = existing.iter_mut().find(|f| same_finding(f, finding)) {
            found.last_seen_turn = Some(turn_no);
            found.updated_at = now.into();
            if matches!(found.status, FindingStatus::Dismissed) {
                found.status = FindingStatus::Suspected;
            }
        } else {
            appended.push(finding.clone());
            existing.push(finding.clone());
        }
    }
    appended
}

fn same_finding(a: &ReviewFinding, b: &ReviewFinding) -> bool {
    let a_file = a.location.as_ref().map(|l| l.file_path.as_str()).unwrap_or("");
    let b_file = b.location.as_ref().map(|l| l.file_path.as_str()).unwrap_or("");
    a.title == b.title && a_file == b_file
}

fn apply_findings_to_session(session: &mut ReviewSession, findings: &[ReviewFinding], parsed: &ReviewResult) {
    session.state.findings = findings.to_vec();
    session.state.pending_finding_ids = findings
        .iter()
        .filter(|f| matches!(f.status, FindingStatus::Suspected))
        .map(|f| f.id.clone())
        .collect();
    session.state.confirmed_finding_ids = findings
        .iter()
        .filter(|f| matches!(f.status, FindingStatus::Confirmed))
        .map(|f| f.id.clone())
        .collect();
    session.state.dismissed_finding_ids = findings
        .iter()
        .filter(|f| matches!(f.status, FindingStatus::Dismissed))
        .map(|f| f.id.clone())
        .collect();
    session.state.release_checks = parsed.release_checks.clone();
    session.state.impact_scope = parsed.impact_scope.clone();
}

fn infer_category(issue: &ReviewIssue) -> FindingCategory {
    let text = format!(
        "{} {} {} {}",
        issue.title,
        issue.reason.clone().unwrap_or_default(),
        issue.impact.clone().unwrap_or_default(),
        issue.suggestion.clone().unwrap_or_default()
    )
    .to_lowercase();

    if text.contains("兼容") || text.contains("contract") || text.contains("api") {
        FindingCategory::Compatibility
    } else if text.contains("sql") || text.contains("数据") || text.contains("migration") {
        FindingCategory::Data
    } else if text.contains("发布") || text.contains("回滚") || text.contains("灰度") {
        FindingCategory::Release
    } else if text.contains("测试") {
        FindingCategory::Testability
    } else if text.contains("性能") || text.contains("超时") {
        FindingCategory::Performance
    } else if text.contains("安全") || text.contains("鉴权") {
        FindingCategory::Security
    } else {
        FindingCategory::Logic
    }
}

fn to_chat_messages(messages: &[ReviewMessage]) -> Vec<ChatInputMessage> {
    messages
        .iter()
        .map(|m| ChatInputMessage {
            role: m.role.clone(),
            content: m.content.clone(),
        })
        .collect()
}

fn build_system_prompt(mode: ReviewMode) -> String {
    format!(
        "你是资深代码审查工程师。当前审查模式: {:?}。请基于上下文做严格、具体、可验证的代码审查。",
        mode
    )
}

fn build_initial_user_prompt(args: &PromptArgs, diff_text: Option<&str>, instruction: Option<&str>) -> String {
    let mut out = String::new();
    if let Some(goal) = &args.goal {
        out.push_str(&format!("目标: {}\n", goal));
    }
    if !args.rules.is_empty() {
        out.push_str("规则:\n");
        for rule in &args.rules {
            out.push_str(&format!("- {}\n", rule));
        }
    }
    if let Some(text) = diff_text {
        out.push_str("\nDiff:\n");
        out.push_str(text);
        out.push('\n');
    }
    if let Some(extra) = instruction {
        out.push_str("\n附加说明:\n");
        out.push_str(extra);
        out.push('\n');
    }
    out
}

fn build_continue_user_prompt(
    req: &ContinueReviewTurnRequest,
    attached_contents: &[(String, String, bool)],
    session: &ReviewSession,
) -> String {
    let mut out = String::new();
    if let Some(instruction) = &req.instruction {
        out.push_str("继续审查要求:\n");
        out.push_str(instruction);
        out.push_str("\n");
    }
    if !req.focus_finding_ids.is_empty() {
        out.push_str("\n重点复核问题:\n");
        for id in &req.focus_finding_ids {
            if let Some(f) = session.state.findings.iter().find(|x| &x.id == id) {
                out.push_str(&format!(
                    "- [{}] {} ({})\n",
                    severity_str(&f.severity),
                    f.title,
                    f.location
                        .as_ref()
                        .map(|l| l.file_path.as_str())
                        .unwrap_or("")
                ));
            } else {
                out.push_str(&format!("- {}\n", id));
            }
        }
    }
    if !req.extra_context.is_empty() {
        out.push_str("\n补充上下文:\n");
        for item in &req.extra_context {
            out.push_str(&format!("- {}\n", item));
        }
    }
    if !attached_contents.is_empty() {
        out.push_str("\n## 补充文件\n");
        for (path, content, truncated) in attached_contents {
            out.push_str(&format!("\n### {}\n", path));
            if *truncated {
                out.push_str("> 注意：以下内容已被截断。\n");
            }
            out.push_str("```\n");
            out.push_str(content);
            if !content.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("```\n");
        }
    } else if !req.attached_files.is_empty() {
        out.push_str("\n补充文件(无法读取内容):\n");
        for file in &req.attached_files {
            out.push_str(&format!("- {}\n", file));
        }
    }
    if req.generate_final_report {
        out.push_str("\n## 请输出最终结论\n");
        out.push_str("请基于前面所有轮次的推理历史，整合输出完整的最终审查报告。");
        if !session.state.impact_scope.is_empty() {
            out.push_str("\n已识别的影响面：\n");
            for item in &session.state.impact_scope {
                out.push_str(&format!("- {}\n", item));
            }
        }
        if !session.state.release_checks.is_empty() {
            out.push_str("\n已识别的发布风险：\n");
            for item in &session.state.release_checks {
                out.push_str(&format!("- {}\n", item));
            }
        }
    }
    out
}

fn severity_str(s: &FindingSeverity) -> &'static str {
    match s {
        FindingSeverity::Critical => "critical",
        FindingSeverity::High => "high",
        FindingSeverity::Medium => "medium",
        FindingSeverity::Low => "low",
        FindingSeverity::Info => "info",
    }
}

fn now_string() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    secs.to_string()
}

fn new_id(prefix: &str) -> String {
    format!("{}-{}", prefix, uuid::Uuid::new_v4().simple())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_findings_from_review_result() {
        let parsed = parse_review_text(
            ReviewMode::Standard,
            "高风险问题\n- src/order/service.rs:create_order 可能重复下单 原因: 缺少幂等校验\n总结结论\n- 有风险",
            vec![],
        );
        let findings = findings_from_result("s1", "t1", 1, "1", &parsed);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, FindingSeverity::High);
        assert_eq!(findings[0].location.as_ref().unwrap().file_path, "src/order/service.rs");
    }
}
