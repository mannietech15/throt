use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use chrono::Utc;
use serde::Deserialize;
use std::sync::Arc;
use throt_core::{AlgorithmType, Application, FailureMode, RateLimitPolicy};
use throt_storage::PostgresPool;
use uuid::Uuid;

use crate::response::{ApiErrorResponse, ApiResponse};

#[derive(Clone)]
pub struct PolicyApiState {
    pub postgres: PostgresPool,
}

#[derive(Debug, Deserialize)]
pub struct CreatePolicyRequest {
    pub name: String,
    pub algorithm: AlgorithmType,
    pub request_limit: u64,
    pub window_seconds: u64,
    pub burst_capacity: Option<u64>,
    pub refill_rate: Option<f64>,
    pub failure_mode: Option<FailureMode>,
}

/// POST /v1/policies
pub async fn create_policy_handler(
    State(state): State<Arc<PolicyApiState>>,
    Extension(app): Extension<Application>,
    Json(body): Json<CreatePolicyRequest>,
) -> Response {
    if body.name.trim().is_empty() {
        return ApiErrorResponse::new("VALIDATION_ERROR", "Policy name cannot be empty")
            .to_response(StatusCode::BAD_REQUEST);
    }
    if body.window_seconds == 0 {
        return ApiErrorResponse::new("VALIDATION_ERROR", "Window seconds must be greater than 0")
            .to_response(StatusCode::BAD_REQUEST);
    }
    if body.request_limit == 0 {
        return ApiErrorResponse::new("VALIDATION_ERROR", "Request limit must be greater than 0")
            .to_response(StatusCode::BAD_REQUEST);
    }

    let policy = RateLimitPolicy {
        id: Uuid::new_v4(),
        application_id: app.id,
        name: body.name.trim().to_string(),
        algorithm: body.algorithm,
        request_limit: body.request_limit,
        window_seconds: body.window_seconds,
        burst_capacity: body.burst_capacity,
        refill_rate: body.refill_rate,
        failure_mode: body.failure_mode.unwrap_or(FailureMode::FailClosed),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    match state.postgres.create_policy(&policy).await {
        Ok(created) => ApiResponse::created(created).into_response(),
        Err(e) => ApiErrorResponse::new("STORAGE_ERROR", e.to_string())
            .to_response(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// GET /v1/policies
pub async fn list_policies_handler(
    State(state): State<Arc<PolicyApiState>>,
    Extension(app): Extension<Application>,
) -> Response {
    match state.postgres.list_policies(app.id).await {
        Ok(policies) => ApiResponse::ok(policies).into_response(),
        Err(e) => ApiErrorResponse::new("STORAGE_ERROR", e.to_string())
            .to_response(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// GET /v1/policies/:id
pub async fn get_policy_handler(
    State(state): State<Arc<PolicyApiState>>,
    Extension(app): Extension<Application>,
    Path(id): Path<Uuid>,
) -> Response {
    match state.postgres.get_policy(id, app.id).await {
        Ok(Some(policy)) => ApiResponse::ok(policy).into_response(),
        Ok(None) => ApiErrorResponse::new("NOT_FOUND", "Policy not found")
            .to_response(StatusCode::NOT_FOUND),
        Err(e) => ApiErrorResponse::new("STORAGE_ERROR", e.to_string())
            .to_response(StatusCode::INTERNAL_SERVER_ERROR),
    }
}
