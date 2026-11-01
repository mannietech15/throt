use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use throt_core::{Application, GeneratedApiKey};
use throt_storage::PostgresPool;

use crate::response::{ApiErrorResponse, ApiResponse};

#[derive(Clone)]
pub struct AppApiState {
    pub postgres: PostgresPool,
}

#[derive(Debug, Deserialize)]
pub struct CreateAppRequest {
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct AppRegistrationResponse {
    pub application: Application,
    pub api_key: GeneratedApiKey,
}

/// POST /v1/apps
pub async fn register_app_handler(
    State(state): State<Arc<AppApiState>>,
    Json(body): Json<CreateAppRequest>,
) -> Response {
    if body.name.trim().is_empty() {
        return ApiErrorResponse::new("VALIDATION_ERROR", "Application name cannot be empty")
            .to_response(StatusCode::BAD_REQUEST);
    }

    match state.postgres.create_application(body.name.trim()).await {
        Ok(app) => {
            let key = GeneratedApiKey::generate(app.id);
            match state.postgres.create_api_key(&key).await {
                Ok(_) => {
                    let resp = AppRegistrationResponse {
                        application: app,
                        api_key: key,
                    };
                    ApiResponse::created(resp).into_response()
                }
                Err(e) => ApiErrorResponse::new("API_KEY_CREATION_FAILED", e.to_string())
                    .to_response(StatusCode::INTERNAL_SERVER_ERROR),
            }
        }
        Err(e) => ApiErrorResponse::new("APP_CREATION_FAILED", e.to_string())
            .to_response(StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// GET /v1/apps/me
pub async fn get_current_app_handler(Extension(app): Extension<Application>) -> Response {
    ApiResponse::ok(app).into_response()
}
