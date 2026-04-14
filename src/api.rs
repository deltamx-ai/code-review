use crate::cli::{DeepReviewArgs, PromptArgs, ReviewArgs, RunArgs};
use crate::config::load_config;
use crate::models;
use crate::services::review_service::{
    execute_assemble, execute_deep_review, execute_prompt, execute_review, execute_run,
    execute_validate,
};
use crate::session::SessionStore;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Serialize;
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
    models::list_models(&state.cfg)
        .map(Json)
        .map_err(api_error)
}

async fn validate_handler(Json(req): Json<PromptArgs>) -> Result<Json<crate::admission::AdmissionResult>, ApiError> {
    let execution = execute_validate(&req).map_err(api_error)?;
    Ok(Json(execution.admission))
}

async fn prompt_handler(Json(req): Json<PromptArgs>) -> Result<Json<crate::prompt::PromptOutput>, ApiError> {
    let execution = execute_prompt(&req).map_err(api_error)?;
    Ok(Json(crate::prompt::PromptOutput {
        ok: execution.ok,
        score: execution.score,
        prompt: execution.prompt,
        summary: execution.summary,
    }))
}

async fn assemble_handler(Json(req): Json<PromptArgs>) -> Result<Json<PromptArgs>, ApiError> {
    let execution = execute_assemble(&req).map_err(api_error)?;
    Ok(Json(execution.prompt_args))
}

async fn run_handler(Json(req): Json<RunArgs>) -> Result<Json<crate::prompt::PromptOutput>, ApiError> {
    let execution = execute_run(&req).map_err(api_error)?;
    Ok(Json(crate::prompt::PromptOutput {
        ok: execution.ok,
        score: execution.score,
        prompt: execution.prompt,
        summary: execution.summary,
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
    let execution = execute_review(&state.store, state.cfg.llm.model.clone(), &mut req).map_err(api_error)?;
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
    let execution = execute_deep_review(&state.store, &req).map_err(api_error)?;
    Ok(Json(DeepReviewApiResponse {
        exit_code: execution.exit_code,
        stage1: execution.stage1,
        stage2: execution.stage2,
    }))
}

fn api_error(err: anyhow::Error) -> ApiError {
    ApiError { error: err.to_string() }
}
