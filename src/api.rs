use crate::cli::{AnalyzeArgs, DeepReviewArgs, PromptArgs, ReviewArgs, RunArgs};
use crate::config::load_config;
use crate::models;
use crate::services::review_service::{
    execute_analyze, execute_assemble, execute_deep_review, execute_prompt, execute_review, execute_run,
    execute_validate,
};
use crate::session::SessionStore;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Serialize;
use tokio::task;
use tower_http::cors::CorsLayer;

#[derive(Clone)]
pub struct ApiState {
    pub store: SessionStore,
    pub cfg: crate::config::AppConfig,
}

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub ok: bool,
    pub service: &'static str,
}

#[derive(Debug, Serialize)]
pub struct ApiError {
    pub error: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (StatusCode::BAD_REQUEST, Json(self)).into_response()
    }
}

pub fn app(state: ApiState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/models", get(models_handler))
        .route("/api/validate", post(validate_handler))
        .route("/api/prompt", post(prompt_handler))
        .route("/api/assemble", post(assemble_handler))
        .route("/api/run", post(run_handler))
        .route("/api/analyze", post(analyze_handler))
        .route("/api/review", post(review_handler))
        .route("/api/deep-review", post(deep_review_handler))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

pub async fn serve(bind: &str) -> anyhow::Result<()> {
    let cfg = load_config()?;
    let store = SessionStore::new_default()?;
    let state = ApiState { store, cfg };
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

fn api_error(err: anyhow::Error) -> ApiError {
    ApiError { error: err.to_string() }
}
