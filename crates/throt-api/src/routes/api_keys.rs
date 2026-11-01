use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Extension;
use std::sync::Arc;
use throt_core::{Application, GeneratedApiKey};
use throt_storage::PostgresPool;
use uuid::Uuid;

use crate::response::{ApiErrorResponse, ApiResponse};

#[derive(Clone)]
pub struct ApiKeyApiState {
    pub postgres: PostgresPool,
}

/// POST /v1/api-keys
pub async fn create_api_key_handler(
    State(state): State<Arc<ApiKeyApiState>>,
    Extension(app): Extension<Application>,
) -> Response {
    let key = GeneratedApiKey::generate(app.id);

    match state.postgres.create_api_key(&key).await {
        Ok(_) => ApiResponse::created(key).into_response(),
        Err(e) => ApiErrorResponse::new("STORAGE_ERROR", e.to_string())
            .to_response(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// GET /v1/api-keys
pub async fn list_api_keys_handler(
    State(state): State<Arc<ApiKeyApiState>>,
    Extension(app): Extension<Application>,
) -> Response {
    match state.postgres.list_api_keys(app.id).await {
        Ok(keys) => ApiResponse::ok(keys).into_response(),
        Err(e) => ApiErrorResponse::new("STORAGE_ERROR", e.to_string())
            .to_response(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// DELETE /v1/api-keys/:id
pub async fn revoke_api_key_handler(
    State(state): State<Arc<ApiKeyApiState>>,
    Extension(app): Extension<Application>,
    Path(id): Path<Uuid>,
) -> Response {
    match state.postgres.revoke_api_key(id, app.id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => ApiErrorResponse::new("NOT_FOUND", "API key not found or already revoked")
            .to_response(StatusCode::NOT_FOUND),
        Err(e) => ApiErrorResponse::new("STORAGE_ERROR", e.to_string())
            .to_response(StatusCode::INTERNAL_SERVER_ERROR),
    }
}
