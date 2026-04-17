use crate::cli::{PromptArgs, ReviewMode};
use crate::conversation::{
    ContentFormat, ConversationStatus, MessageRole, ReviewFinding, ReviewMessage, ReviewSession, ReviewTurn,
    TurnKind, TurnStatus,
};
use crate::conversation_store::ConversationStore;
use crate::providers::{ChatInputMessage, ChatRequest, LlmProvider};
use crate::review_schema::ReviewResult;
use anyhow::{bail, Result};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

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
        req.repo_root,
        req.provider.unwrap_or_else(|| provider.name().to_string()),
        req.model.clone().unwrap_or_else(|| "default".into()),
        now.clone(),
    );
    session.base_ref = req.base_ref;
    session.head_ref = req.head_ref;
    session.status = ConversationStatus::Running;

    let turn_id = new_id("turn");
    let system_text = build_system_prompt(req.review_mode);
    let user_text = build_initial_user_prompt(&req.prompt_args, req.diff_text.as_deref(), req.initial_instruction.as_deref());

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

    let chat_req = ChatRequest {
        model: session.model.clone(),
        messages: to_chat_messages(&messages),
        temperature: None,
        max_tokens: None,
        metadata: BTreeMap::new(),
    };
    let response = provider.chat(&chat_req)?;

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

    let turn = ReviewTurn {
        id: turn_id,
        session_id: session_id.clone(),
        turn_no: 1,
        kind: TurnKind::Discovery,
        status: TurnStatus::Completed,
        input_summary: Some("initial review turn".into()),
        instruction: req.initial_instruction,
        requested_files: Vec::new(),
        attached_files: req.prompt_args.context_files.iter().map(|p| p.display().to_string()).collect(),
        focus_finding_ids: Vec::new(),
        prompt_text: None,
        response_text: Some(response.content.clone()),
        parsed_result: None,
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

    store.save_session(&session)?;
    store.save_turn(&turn)?;
    for msg in &messages {
        store.append_message(msg)?;
    }
    store.append_message(&assistant_msg)?;
    store.save_findings(&session.id, &[])?;

    Ok(ReviewOrchestrationResult {
        session,
        turn,
        new_messages: vec![messages[0].clone(), messages[1].clone(), assistant_msg],
        new_findings: Vec::new(),
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
    let existing_findings = store.load_findings(&req.session_id)?;
    let next_seq = store.next_message_seq(&req.session_id)?;
    let turn_no = session.total_turns + 1;
    let turn_id = new_id("turn");

    let user_text = build_continue_user_prompt(&req);
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

    let chat_req = ChatRequest {
        model: session.model.clone(),
        messages: to_chat_messages(&all_messages),
        temperature: session.temperature,
        max_tokens: None,
        metadata: BTreeMap::new(),
    };
    let response = provider.chat(&chat_req)?;

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
        let mut report = ReviewResult::new(session.review_mode, response.content.clone());
        report.summary = response.content.lines().next().unwrap_or("review completed").to_string();
        report.finalize();
        session.final_summary = Some(report.summary.clone());
        session.final_report = Some(report.clone());
        session.status = ConversationStatus::Completed;
        session.completed_at = Some(now.clone());
        Some(report)
    } else {
        None
    };

    let turn = ReviewTurn {
        id: turn_id,
        session_id: session.id.clone(),
        turn_no,
        kind: if req.generate_final_report { TurnKind::FinalReport } else { TurnKind::DeepDive },
        status: TurnStatus::Completed,
        input_summary: Some("follow-up review turn".into()),
        instruction: req.instruction.clone(),
        requested_files: Vec::new(),
        attached_files: req.attached_files.clone(),
        focus_finding_ids: req.focus_finding_ids.clone(),
        prompt_text: None,
        response_text: Some(response.content.clone()),
        parsed_result: final_report.clone(),
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
    session.state.attached_files.extend(req.attached_files.clone());
    session.state.requested_files.extend(req.extra_context.clone());

    store.save_turn(&turn)?;
    store.append_message(&user_msg)?;
    store.append_message(&assistant_msg)?;
    store.save_findings(&session.id, &existing_findings)?;
    store.save_session(&session)?;

    Ok(ReviewOrchestrationResult {
        session,
        turn,
        new_messages: vec![user_msg, assistant_msg],
        new_findings: Vec::new(),
        final_report,
    })
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

fn build_continue_user_prompt(req: &ContinueReviewTurnRequest) -> String {
    let mut out = String::new();
    if let Some(instruction) = &req.instruction {
        out.push_str("继续审查要求:\n");
        out.push_str(instruction);
        out.push_str("\n");
    }
    if !req.focus_finding_ids.is_empty() {
        out.push_str("重点复核问题:\n");
        for id in &req.focus_finding_ids {
            out.push_str(&format!("- {}\n", id));
        }
    }
    if !req.attached_files.is_empty() {
        out.push_str("补充文件:\n");
        for file in &req.attached_files {
            out.push_str(&format!("- {}\n", file));
        }
    }
    if !req.extra_context.is_empty() {
        out.push_str("补充上下文:\n");
        for item in &req.extra_context {
            out.push_str(&format!("- {}\n", item));
        }
    }
    if req.generate_final_report {
        out.push_str("请基于前面所有轮次内容输出最终总结。\n");
    }
    out
}

fn now_string() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    secs.to_string()
}

fn new_id(prefix: &str) -> String {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    format!("{}-{}", prefix, nanos)
}
