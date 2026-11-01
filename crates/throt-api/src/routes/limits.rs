use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use serde::Deserialize;
use std::sync::Arc;
use throt_core::{Application, EvaluationRequest, Identity, RateLimitEngine};

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
    let identity = match Identity::new(payload.identity) {
        Ok(id) => id,
        Err(e) => {
            return ApiErrorResponse::new("INVALID_IDENTITY", e)
                .to_response(StatusCode::BAD_REQUEST);
        }
    };

    let req = EvaluationRequest {
        application_id: app.id,
        identity,
        policy_name: payload.policy,
        cost: payload.cost.unwrap_or(1),
    };

    match state.engine.evaluate(req).await {
        Ok(decision) => (StatusCode::OK, Json(decision)).into_response(),
        Err(throt_core::ThrotError::PolicyNotFound(msg)) => {
            ApiErrorResponse::new("POLICY_NOT_FOUND", msg).to_response(StatusCode::NOT_FOUND)
        }
        Err(e) => ApiErrorResponse::new("EVALUATION_FAILED", e.to_string())
            .to_response(StatusCode::INTERNAL_SERVER_ERROR),
    }
}
