use crate::cli::{AnalyzeArgs, DeepReviewArgs, PromptArgs, ReviewArgs, RunArgs};
use crate::config::load_config;
use crate::conversation::{
    FindingPatch, ReviewArtifact, ReviewFinding, ReviewMessage, ReviewSession, ReviewTurn,
    SessionListFilter, SessionSummary,
};
use crate::conversation_store::ConversationStore;
use crate::models;
use crate::orchestrator::{
    continue_session, start_session, ContinueReviewTurnRequest, StartReviewSessionRequest,
};
use crate::providers::copilot::CopilotCliProvider;
use crate::services::review_service::{
    execute_analyze, execute_assemble, execute_deep_review, execute_prompt, execute_review, execute_run,
    execute_validate,
};
use crate::session::SessionStore;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::task;
use tower_http::cors::CorsLayer;

#[derive(Clone)]
pub struct ApiState {
    pub store: SessionStore,
    pub conversation_store: ConversationStore,
    pub cfg: crate::config::AppConfig,
}

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub ok: bool,
    pub service: &'static str,
}

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub error: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (self.status, Json(serde_json::json!({"error": self.error}))).into_response()
    }
}

pub fn app(state: ApiState) -> Router {
    let cors_permissive = state.cfg.api.cors_permissive.unwrap_or(false);
    let router = Router::new()
        .route("/api/health", get(health))
        .route("/api/models", get(models_handler))
        .route("/api/validate", post(validate_handler))
        .route("/api/prompt", post(prompt_handler))
        .route("/api/assemble", post(assemble_handler))
        .route("/api/run", post(run_handler))
        .route("/api/analyze", post(analyze_handler))
        .route("/api/review", post(review_handler))
        .route("/api/deep-review", post(deep_review_handler))
        .route(
            "/api/review-sessions",
            get(list_review_sessions_handler).post(create_review_session_handler),
        )
        .route(
            "/api/review-sessions/{id}",
            get(get_review_session_handler).delete(delete_review_session_handler),
        )
        .route("/api/review-sessions/{id}/turns", post(append_review_turn_handler))
        .route(
            "/api/review-sessions/{id}/findings/{finding_id}",
            patch(update_review_finding_handler),
        );
    let router = if cors_permissive {
        router.layer(CorsLayer::permissive())
    } else {
        router
    };
    router.with_state(state)
}

pub async fn serve(bind: &str) -> anyhow::Result<()> {
    let cfg = load_config()?;
    let store = SessionStore::new_default()?;
    let conversation_store = ConversationStore::new_default()?;
    let state = ApiState { store, conversation_store, cfg };
    let listener = tokio::net::TcpListener::bind(bind).await?;
    axum::serve(listener, app(state)).await?;
    Ok(())
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { ok: true, service: "code-review" })
}

async fn models_handler(State(state): State<ApiState>) -> Result<Json<models::ModelList>, ApiError> {
    let cfg = state.cfg.clone();
    let models = task::spawn_blocking(move || models::list_models(&cfg))
        .await
        .map_err(|e| api_error(anyhow::anyhow!("models task join error: {}", e)))?
        .map_err(api_error)?;
    Ok(Json(models))
}

async fn validate_handler(Json(req): Json<PromptArgs>) -> Result<Json<crate::admission::AdmissionResult>, ApiError> {
    let execution = task::spawn_blocking(move || execute_validate(&req))
        .await
        .map_err(|e| api_error(anyhow::anyhow!("validate task join error: {}", e)))?
        .map_err(api_error)?;
    Ok(Json(execution.admission))
}

async fn prompt_handler(Json(req): Json<PromptArgs>) -> Result<Json<crate::prompt::PromptOutput>, ApiError> {
    let execution = task::spawn_blocking(move || execute_prompt(&req))
        .await
        .map_err(|e| api_error(anyhow::anyhow!("prompt task join error: {}", e)))?
        .map_err(api_error)?;
    Ok(Json(crate::prompt::PromptOutput {
        ok: execution.ok,
        score: execution.score,
        prompt: execution.prompt,
        summary: execution.summary,
    }))
}

async fn assemble_handler(Json(req): Json<PromptArgs>) -> Result<Json<PromptArgs>, ApiError> {
    let execution = task::spawn_blocking(move || execute_assemble(&req))
        .await
        .map_err(|e| api_error(anyhow::anyhow!("assemble task join error: {}", e)))?
        .map_err(api_error)?;
    Ok(Json(execution.prompt_args))
}

async fn run_handler(Json(req): Json<RunArgs>) -> Result<Json<crate::prompt::PromptOutput>, ApiError> {
    let execution = task::spawn_blocking(move || execute_run(&req))
        .await
        .map_err(|e| api_error(anyhow::anyhow!("run task join error: {}", e)))?
        .map_err(api_error)?;
    Ok(Json(crate::prompt::PromptOutput {
        ok: execution.ok,
        score: execution.score,
        prompt: execution.prompt,
        summary: execution.summary,
    }))
}

#[derive(Debug, Serialize)]
pub struct AnalyzeApiResponse {
    pub strategy: String,
    pub admission: crate::admission::AdmissionResult,
    pub prompt: crate::prompt::PromptOutput,
    pub review: Option<crate::review_schema::ReviewResult>,
    pub stage1: Option<crate::review_schema::ReviewResult>,
    pub stage2: Option<crate::review_schema::ReviewResult>,
    pub exit_code: i32,
}

async fn analyze_handler(
    State(state): State<ApiState>,
    Json(mut req): Json<AnalyzeArgs>,
) -> Result<Json<AnalyzeApiResponse>, ApiError> {
    if req.model.is_none() {
        req.model = state.cfg.llm.model.clone();
    }
    let store = state.store.clone();
    let cfg_default_model = state.cfg.llm.model.clone();
    let execution = task::spawn_blocking(move || execute_analyze(&store, cfg_default_model, &req))
        .await
        .map_err(|e| api_error(anyhow::anyhow!("analyze task join error: {}", e)))?
        .map_err(api_error)?;
    Ok(Json(AnalyzeApiResponse {
        strategy: execution.strategy,
        admission: execution.admission,
        prompt: execution.prompt,
        review: execution.review,
        stage1: execution.stage1,
        stage2: execution.stage2,
        exit_code: execution.exit_code,
    }))
}

#[derive(Debug, Serialize)]
pub struct ReviewApiResponse {
    pub exit_code: i32,
    pub result: crate::review_schema::ReviewResult,
}

async fn review_handler(
    State(state): State<ApiState>,
    Json(mut req): Json<ReviewArgs>,
) -> Result<Json<ReviewApiResponse>, ApiError> {
    if req.model.is_none() {
        req.model = state.cfg.llm.model.clone();
    }
    let store = state.store.clone();
    let cfg_default_model = state.cfg.llm.model.clone();
    let execution = task::spawn_blocking(move || execute_review(&store, cfg_default_model, &mut req))
        .await
        .map_err(|e| api_error(anyhow::anyhow!("review task join error: {}", e)))?
        .map_err(api_error)?;
    Ok(Json(ReviewApiResponse {
        exit_code: execution.exit_code,
        result: execution.result,
    }))
}

#[derive(Debug, Serialize)]
pub struct DeepReviewApiResponse {
    pub exit_code: i32,
    pub stage1: crate::review_schema::ReviewResult,
    pub stage2: crate::review_schema::ReviewResult,
}

async fn deep_review_handler(
    State(state): State<ApiState>,
    Json(mut req): Json<DeepReviewArgs>,
) -> Result<Json<DeepReviewApiResponse>, ApiError> {
    if req.model.is_none() {
        req.model = state.cfg.llm.model.clone();
    }
    let store = state.store.clone();
    let execution = task::spawn_blocking(move || execute_deep_review(&store, &req))
        .await
        .map_err(|e| api_error(anyhow::anyhow!("deep-review task join error: {}", e)))?
        .map_err(api_error)?;
    Ok(Json(DeepReviewApiResponse {
        exit_code: execution.exit_code,
        stage1: execution.stage1,
        stage2: execution.stage2,
    }))
}

#[derive(Debug, Deserialize)]
pub struct CreateReviewSessionApiRequest {
    pub repo_root: String,
    pub review_mode: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub base_ref: Option<String>,
    pub head_ref: Option<String>,
    pub diff_text: Option<String>,
    pub prompt_args: PromptArgs,
    pub initial_instruction: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AppendReviewTurnApiRequest {
    pub instruction: Option<String>,
    #[serde(default)]
    pub attached_files: Vec<String>,
    #[serde(default)]
    pub extra_context: Vec<String>,
    #[serde(default)]
    pub focus_finding_ids: Vec<String>,
    pub finalize: Option<bool>,
    pub model: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ReviewSessionDetailApiResponse {
    pub session: ReviewSession,
    pub turns: Vec<ReviewTurn>,
    pub messages: Vec<ReviewMessage>,
    pub findings: Vec<ReviewFinding>,
    pub artifacts: Vec<ReviewArtifact>,
}

#[derive(Debug, Serialize)]
pub struct SessionListApiResponse {
    pub items: Vec<SessionSummary>,
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
}

async fn list_review_sessions_handler(
    State(state): State<ApiState>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<SessionListApiResponse>, ApiError> {
    let convo_store = state.conversation_store.clone();
    let filter = SessionListFilter {
        repo: params.get("repo").cloned(),
        status: params.get("status").cloned(),
        mode: params.get("mode").cloned(),
        limit: params.get("limit").and_then(|v| v.parse().ok()),
        offset: params.get("offset").and_then(|v| v.parse().ok()),
    };
    let limit = filter.limit.unwrap_or(20);
    let offset = filter.offset.unwrap_or(0);
    let filter_for_count = filter.clone();
    let filter_for_list = filter.clone();
    let result: Result<SessionListApiResponse, anyhow::Error> = task::spawn_blocking(move || {
        let total = convo_store.count_sessions(&filter_for_count)?;
        let items = convo_store.list_sessions(&filter_for_list)?;
        Ok(SessionListApiResponse { items, total, limit, offset })
    })
    .await
    .map_err(|e| api_error(anyhow::anyhow!("list sessions task join error: {}", e)))?;
    Ok(Json(result.map_err(api_error)?))
}

async fn create_review_session_handler(
    State(state): State<ApiState>,
    Json(req): Json<CreateReviewSessionApiRequest>,
) -> Result<Json<ReviewSessionDetailApiResponse>, ApiError> {
    let review_mode = parse_review_mode(&req.review_mode)?;
    let convo_store = state.conversation_store.clone();
    let session_store = state.store.clone();
    let default_model = state.cfg.llm.model.clone();
    let result = task::spawn_blocking(move || {
        let provider = CopilotCliProvider::new(session_store);
        let orchestration = start_session(
            &convo_store,
            &provider,
            StartReviewSessionRequest {
                repo_root: req.repo_root.into(),
                review_mode,
                provider: req.provider,
                model: req.model.or(default_model),
                base_ref: req.base_ref,
                head_ref: req.head_ref,
                diff_text: req.diff_text,
                prompt_args: req.prompt_args,
                initial_instruction: req.initial_instruction,
            },
        )?;
        let turns = convo_store.load_turns(&orchestration.session.id)?;
        let messages = convo_store.load_messages(&orchestration.session.id)?;
        let findings = convo_store.load_findings(&orchestration.session.id)?;
        let artifacts = convo_store.list_artifacts(&orchestration.session.id)?;
        anyhow::Ok(ReviewSessionDetailApiResponse {
            session: orchestration.session,
            turns,
            messages,
            findings,
            artifacts,
        })
    })
    .await
    .map_err(|e| api_error(anyhow::anyhow!("create review session task join error: {}", e)))?
    .map_err(api_error)?;
    Ok(Json(result))
}

async fn append_review_turn_handler(
    State(state): State<ApiState>,
    Path(id): Path<String>,
    Json(req): Json<AppendReviewTurnApiRequest>,
) -> Result<Json<ReviewSessionDetailApiResponse>, ApiError> {
    let convo_store = state.conversation_store.clone();
    let session_store = state.store.clone();
    let result = task::spawn_blocking(move || {
        let provider = CopilotCliProvider::new(session_store);
        let orchestration = continue_session(
            &convo_store,
            &provider,
            ContinueReviewTurnRequest {
                session_id: id.clone(),
                instruction: req.instruction,
                attached_files: req.attached_files,
                extra_context: req.extra_context,
                focus_finding_ids: req.focus_finding_ids,
                generate_final_report: req.finalize.unwrap_or(false),
                model: req.model,
            },
        )?;
        let turns = convo_store.load_turns(&orchestration.session.id)?;
        let messages = convo_store.load_messages(&orchestration.session.id)?;
        let findings = convo_store.load_findings(&orchestration.session.id)?;
        let artifacts = convo_store.list_artifacts(&orchestration.session.id)?;
        anyhow::Ok(ReviewSessionDetailApiResponse {
            session: orchestration.session,
            turns,
            messages,
            findings,
            artifacts,
        })
    })
    .await
    .map_err(|e| api_error(anyhow::anyhow!("append review turn task join error: {}", e)))?
    .map_err(api_error)?;
    Ok(Json(result))
}

async fn get_review_session_handler(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<Json<ReviewSessionDetailApiResponse>, ApiError> {
    let convo_store = state.conversation_store.clone();
    let result = task::spawn_blocking(move || {
        let session = convo_store.load_session(&id)?;
        let turns = convo_store.load_turns(&id)?;
        let messages = convo_store.load_messages(&id)?;
        let findings = convo_store.load_findings(&id)?;
        let artifacts = convo_store.list_artifacts(&id)?;
        anyhow::Ok(ReviewSessionDetailApiResponse {
            session,
            turns,
            messages,
            findings,
            artifacts,
        })
    })
    .await
    .map_err(|e| api_error(anyhow::anyhow!("get review session task join error: {}", e)))?
    .map_err(api_error)?;
    Ok(Json(result))
}

async fn delete_review_session_handler(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let convo_store = state.conversation_store.clone();
    task::spawn_blocking(move || convo_store.delete_session(&id))
        .await
        .map_err(|e| api_error(anyhow::anyhow!("delete session task join error: {}", e)))?
        .map_err(api_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn update_review_finding_handler(
    State(state): State<ApiState>,
    Path((id, finding_id)): Path<(String, String)>,
    Json(patch): Json<FindingPatch>,
) -> Result<Json<ReviewFinding>, ApiError> {
    let convo_store = state.conversation_store.clone();
    let now = chrono_like_now();
    let result = task::spawn_blocking(move || convo_store.update_finding(&id, &finding_id, &patch, &now))
        .await
        .map_err(|e| api_error(anyhow::anyhow!("update finding task join error: {}", e)))?
        .map_err(api_error)?;
    Ok(Json(result))
}

fn chrono_like_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    secs.to_string()
}

fn parse_review_mode(mode: &str) -> Result<crate::cli::ReviewMode, ApiError> {
    match mode.to_lowercase().as_str() {
        "lite" => Ok(crate::cli::ReviewMode::Lite),
        "standard" => Ok(crate::cli::ReviewMode::Standard),
        "critical" => Ok(crate::cli::ReviewMode::Critical),
        _ => Err(ApiError {
            status: StatusCode::BAD_REQUEST,
            error: format!("invalid review_mode: {}", mode),
        }),
    }
}

fn api_error(err: anyhow::Error) -> ApiError {
    let msg = err.to_string();
    let lower = msg.to_lowercase();
    let status = if lower.contains("not authenticated") || lower.contains("auth login") {
        StatusCode::UNAUTHORIZED
    } else if lower.contains("critical 模式必须使用两阶段 review") {
        StatusCode::CONFLICT
    } else if lower.contains("invalid status transition") {
        StatusCode::CONFLICT
    } else if lower.contains("blocked") {
        StatusCode::UNPROCESSABLE_ENTITY
    } else if lower.contains("session not found")
        || lower.contains("finding not found")
        || lower.contains("failed to read")
        || lower.contains("no such file")
    {
        StatusCode::NOT_FOUND
    } else if lower.contains("invalid session id")
        || lower.contains("failed to parse")
        || lower.contains("provide --prompt")
        || lower.contains("is empty")
        || lower.contains("out of range")
        || lower.contains("invalid review_mode")
    {
        StatusCode::BAD_REQUEST
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    ApiError { status, error: msg }
}
