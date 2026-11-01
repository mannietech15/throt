use axum::extract::{Request, State};
use axum::http::header::AUTHORIZATION;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;
use std::sync::Arc;
use throt_api_response::ApiErrorResponse;
use throt_core::Application;
use throt_storage::PostgresPool;

mod throt_api_response {
    pub use crate::response::ApiErrorResponse;
}

/// Authentication state wrapping the PostgreSQL metadata pool.
#[derive(Clone)]
pub struct AuthState {
    pub postgres: Option<PostgresPool>,
}

/// Middleware that authenticates requests using an API key from `X-API-Key` or `Authorization: Bearer <key>`.
pub async fn require_api_key(
    State(auth): State<Arc<AuthState>>,
    mut req: Request,
    next: Next,
) -> Response {
    let key_opt = req
        .headers()
        .get("X-API-Key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .or_else(|| {
            req.headers()
                .get(AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| {
                    s.trim()
                        .strip_prefix("Bearer ")
                        .map(|stripped| stripped.trim().to_string())
                })
        });

    let raw_key = match key_opt {
        Some(k) if !k.is_empty() => k,
        _ => {
            return ApiErrorResponse::new(
                "UNAUTHORIZED",
                "Missing or empty API key in 'X-API-Key' or 'Authorization: Bearer <key>' header",
            )
            .to_response(StatusCode::UNAUTHORIZED);
        }
    };

    if let Some(ref pg) = auth.postgres {
        match pg.verify_api_key(&raw_key).await {
            Ok(Some(app)) => {
                req.extensions_mut().insert(app);
                next.run(req).await
            }
            Ok(None) => {
                ApiErrorResponse::new("UNAUTHORIZED", "Invalid, expired, or revoked API key")
                    .to_response(StatusCode::UNAUTHORIZED)
            }
            Err(e) => ApiErrorResponse::new(
                "INTERNAL_ERROR",
                format!("Failed to verify authentication: {e}"),
            )
            .to_response(StatusCode::INTERNAL_SERVER_ERROR),
        }
    } else {
        // If Postgres is unconfigured in test/dev, generate a mock application
        let mock_app = Application {
            id: uuid::Uuid::nil(),
            name: "Development Sandbox".to_string(),
            status: "active".to_string(),
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        req.extensions_mut().insert(mock_app);
        next.run(req).await
    }
}
