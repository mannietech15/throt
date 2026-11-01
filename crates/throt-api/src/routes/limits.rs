use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use serde::Deserialize;
use std::sync::Arc;
use throt_core::{Application, EvaluationRequest, RateLimitEngine};

use crate::response::ApiErrorResponse;

/// App state containing the shared RateLimitEngine.
#[derive(Clone)]
pub struct LimitsState {
    pub engine: Arc<RateLimitEngine>,
}

#[derive(Debug, Deserialize)]
pub struct EvaluateRequestBody {
    pub identity: String,
    pub policy: Option<String>,
    pub cost: Option<u64>,
}

/// POST /v1/limits/evaluate
pub async fn evaluate_handler(
    State(state): State<Arc<LimitsState>>,
    Extension(app): Extension<Application>,
    Json(payload): Json<EvaluateRequestBody>,
) -> Response {
    let req = match EvaluationRequest::for_application(
        app.id,
        payload.identity,
        payload.policy,
        payload.cost.unwrap_or(1),
    ) {
        Ok(r) => r,
        Err(e) => {
            return ApiErrorResponse::new("INVALID_REQUEST", e.to_string())
                .to_response(StatusCode::BAD_REQUEST);
        }
    };

    match state.engine.evaluate(req).await {
        Ok(decision) => (StatusCode::OK, Json(decision)).into_response(),
        Err(throt_core::ThrotError::PolicyNotFound(msg)) => {
            ApiErrorResponse::new("POLICY_NOT_FOUND", msg).to_response(StatusCode::NOT_FOUND)
        }
        Err(throt_core::ThrotError::Validation(msg)) => {
            ApiErrorResponse::new("VALIDATION_ERROR", msg).to_response(StatusCode::BAD_REQUEST)
        }
        Err(e) => ApiErrorResponse::new("EVALUATION_FAILED", e.to_string())
            .to_response(StatusCode::INTERNAL_SERVER_ERROR),
    }
}
