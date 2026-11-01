use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use std::sync::Arc;
use throt_storage::{PostgresPool, RedisStorage};

/// App state containing storage clients for dependency health checking.
#[derive(Clone)]
pub struct AppState {
    pub redis: Option<RedisStorage>,
    pub postgres: Option<PostgresPool>,
}

#[derive(Debug, Serialize)]
pub struct LivenessResponse {
    pub status: &'static str,
    pub timestamp: i64,
}

#[derive(Debug, Serialize)]
pub struct ReadinessResponse {
    pub ready: bool,
    pub redis: &'static str,
    pub postgres: &'static str,
    pub timestamp: i64,
}

/// Liveness probe: returns 200 OK if service process is running.
pub async fn health_handler() -> impl IntoResponse {
    let resp = LivenessResponse {
        status: "pass",
        timestamp: chrono::Utc::now().timestamp(),
    };
    (StatusCode::OK, Json(resp))
}

/// Readiness probe: returns 200 OK only if Redis and PostgreSQL are operational.
pub async fn readiness_handler(State(state): State<Arc<AppState>>) -> Response {
    let now = chrono::Utc::now().timestamp();
    let mut is_ready = true;
    let mut redis_status = "unconfigured";
    let mut postgres_status = "unconfigured";

    if let Some(ref redis) = state.redis {
        match redis.ping().await {
            Ok(()) => redis_status = "ok",
            Err(_) => {
                redis_status = "error";
                is_ready = false;
            }
        }
    }

    if let Some(ref pg) = state.postgres {
        match pg.ping().await {
            Ok(()) => postgres_status = "ok",
            Err(_) => {
                postgres_status = "error";
                is_ready = false;
            }
        }
    }

    let resp = ReadinessResponse {
        ready: is_ready,
        redis: redis_status,
        postgres: postgres_status,
        timestamp: now,
    };

    if is_ready {
        (StatusCode::OK, Json(resp)).into_response()
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, Json(resp)).into_response()
    }
}

pub fn health_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(health_handler))
        .route("/ready", get(readiness_handler))
        .with_state(state)
}
