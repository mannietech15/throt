pub mod api_keys;
pub mod apps;
pub mod health;
pub mod limits;
pub mod metrics;
pub mod policies;

use axum::middleware;
use axum::routing::{delete, get, post};
use axum::Router;
use metrics_exporter_prometheus::PrometheusHandle;
use std::sync::Arc;
use throt_core::RateLimitEngine;
use throt_storage::PostgresPool;

use crate::middleware::auth::{require_api_key, AuthState};
use api_keys::{
    create_api_key_handler, list_api_keys_handler, revoke_api_key_handler, ApiKeyApiState,
};
use apps::{get_current_app_handler, register_app_handler, AppApiState};
use health::{health_router, AppState};
use limits::{evaluate_handler, LimitsState};
use metrics::{metrics_handler, MetricsState};
use policies::{create_policy_handler, get_policy_handler, list_policies_handler, PolicyApiState};

/// Builds the complete THROT HTTP API router.
pub fn create_router(
    app_state: Arc<AppState>,
    engine: Arc<RateLimitEngine>,
    postgres: Option<PostgresPool>,
    metrics_handle: Option<PrometheusHandle>,
) -> Router {
    // 1. Public health router (/health, /ready)
    let mut router = health_router(app_state);

    // 2. Metrics endpoint (/metrics)
    if let Some(handle) = metrics_handle {
        let metrics_state = Arc::new(MetricsState { handle });
        let metrics_router = Router::new()
            .route("/metrics", get(metrics_handler))
            .with_state(metrics_state);
        router = router.merge(metrics_router);
    }

    // 2. Public application registration route (POST /v1/apps)
    if let Some(ref pg) = postgres {
        let app_state = Arc::new(AppApiState {
            postgres: pg.clone(),
        });
        let public_app_router = Router::new()
            .route("/v1/apps", post(register_app_handler))
            .with_state(app_state);
        router = router.merge(public_app_router);
    }

    // 3. Authenticated routes (under require_api_key middleware)
    let auth_state = Arc::new(AuthState {
        postgres: postgres.clone(),
    });

    let limits_state = Arc::new(LimitsState {
        engine: engine.clone(),
    });
    let limits_router = Router::new()
        .route("/v1/limits/evaluate", post(evaluate_handler))
        .with_state(limits_state);

    let mut authenticated_router = Router::new().merge(limits_router);

    if let Some(ref pg) = postgres {
        let policy_state = Arc::new(PolicyApiState {
            postgres: pg.clone(),
        });
        let policy_router = Router::new()
            .route(
                "/v1/policies",
                post(create_policy_handler).get(list_policies_handler),
            )
            .route("/v1/policies/:id", get(get_policy_handler))
            .with_state(policy_state);

        let key_state = Arc::new(ApiKeyApiState {
            postgres: pg.clone(),
        });
        let key_router = Router::new()
            .route(
                "/v1/api-keys",
                post(create_api_key_handler).get(list_api_keys_handler),
            )
            .route("/v1/api-keys/:id", delete(revoke_api_key_handler))
            .with_state(key_state);

        let app_me_router = Router::new().route("/v1/apps/me", get(get_current_app_handler));

        authenticated_router = authenticated_router
            .merge(policy_router)
            .merge(key_router)
            .merge(app_me_router);
    }

    let secured_routes =
        authenticated_router.layer(middleware::from_fn_with_state(auth_state, require_api_key));

    router.merge(secured_routes)
}
