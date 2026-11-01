pub mod config;
pub mod startup;

use config::Config;
use startup::shutdown_signal;
use std::sync::Arc;
use throt_api::AppState;
use throt_storage::{PostgresPool, RedisStorage};
use tower_http::trace::TraceLayer;
use tracing::{info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load configuration
    let config = Config::from_env().map_err(|e| {
        eprintln!("Configuration error: {e}");
        anyhow::anyhow!(e)
    })?;

    // Initialize structured tracing
    tracing_subscriber::registry()
        .with(EnvFilter::new(&config.log_level))
        .with(tracing_subscriber::fmt::layer().json())
        .init();

    info!(
        app_env = %config.app_env,
        host = %config.host,
        port = %config.port,
        "Starting THROT rate-limiting infrastructure service"
    );

    // Initialize storage connections
    let redis_storage = match RedisStorage::connect(&config.redis_url).await {
        Ok(storage) => Some(storage),
        Err(e) => {
            warn!(error = %e, "Redis connection failed at startup; readiness check will report unready");
            None
        }
    };

    let postgres_pool = match PostgresPool::connect(&config.database_url).await {
        Ok(pool) => Some(pool),
        Err(e) => {
            warn!(error = %e, "PostgreSQL connection failed at startup; readiness check will report unready");
            None
        }
    };

    let app_state = Arc::new(AppState {
        redis: redis_storage.clone(),
        postgres: postgres_pool.clone(),
    });

    // Initialize Rate Limit Engine
    let policy_store: Arc<dyn throt_core::PolicyStore> = if let Some(ref pg) = postgres_pool {
        Arc::new(pg.clone())
    } else {
        Arc::new(throt_core::InMemoryPolicyStore::new())
    };

    let evaluator: Arc<dyn throt_core::RateLimitEvaluator> = if let Some(ref r) = redis_storage {
        Arc::new(throt_storage::RedisRateLimitEvaluator::new(r.clone()))
    } else {
        Arc::new(throt_algorithms::FixedWindowRateLimiter::new())
    };

    let engine = Arc::new(throt_core::RateLimitEngine::new(policy_store, evaluator));

    // Build Axum application router
    let app = throt_api::create_router(app_state, engine, postgres_pool)
        .layer(TraceLayer::new_for_http());

    let addr = config.socket_addr()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!("THROT service listening on http://{}", addr);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    info!("THROT service has stopped gracefully");
    Ok(())
}
