use sqlx::postgres::PgPoolOptions;
use sqlx::{Pool, Postgres};
use std::time::Duration;
use throt_core::ThrotError;
use tracing::info;

/// Wrapper around SQLx PostgreSQL connection pool.
#[derive(Clone)]
pub struct PostgresPool {
    pool: Pool<Postgres>,
}

impl PostgresPool {
    /// Creates and validates a PostgreSQL connection pool.
    pub async fn connect(database_url: &str) -> Result<Self, ThrotError> {
        let pool = PgPoolOptions::new()
            .max_connections(20)
            .acquire_timeout(Duration::from_secs(5))
            .connect(database_url)
            .await
            .map_err(|e| ThrotError::Storage(format!("Failed to connect to PostgreSQL: {e}")))?;

        info!("Successfully connected to PostgreSQL metadata store");
        Ok(Self { pool })
    }

    /// Healthcheck: executes a quick query to verify PostgreSQL responsiveness.
    pub async fn ping(&self) -> Result<(), ThrotError> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .map_err(|e| ThrotError::Storage(format!("PostgreSQL ping failed: {e}")))?;
        Ok(())
    }

    /// Returns a reference to the underlying SQLx pool.
    pub fn inner(&self) -> &Pool<Postgres> {
        &self.pool
    }
}
