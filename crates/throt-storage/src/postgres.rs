use async_trait::async_trait;
use sqlx::postgres::PgPoolOptions;
use sqlx::{Pool, Postgres, Row};
use std::time::Duration;
use throt_core::{
    AlgorithmType, ApiKeyMetadata, Application, FailureMode, GeneratedApiKey, Identity,
    PolicyStore, RateLimitPolicy, ThrotError,
};
use tracing::info;
use uuid::Uuid;

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

    // --- Application Operations ---

    pub async fn create_application(&self, name: &str) -> Result<Application, ThrotError> {
        let row = sqlx::query(
            r#"
            INSERT INTO applications (name)
            VALUES ($1)
            RETURNING id, name, status, created_at, updated_at
            "#,
        )
        .bind(name)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| ThrotError::Storage(format!("Failed to create application: {e}")))?;

        Ok(Application {
            id: row.get("id"),
            name: row.get("name"),
            status: row.get("status"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        })
    }

    pub async fn get_application(&self, id: Uuid) -> Result<Option<Application>, ThrotError> {
        let row_opt = sqlx::query(
            r#"
            SELECT id, name, status, created_at, updated_at
            FROM applications
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| ThrotError::Storage(format!("Failed to query application: {e}")))?;

        Ok(row_opt.map(|row| Application {
            id: row.get("id"),
            name: row.get("name"),
            status: row.get("status"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        }))
    }

    // --- API Key Operations ---

    pub async fn create_api_key(
        &self,
        key: &GeneratedApiKey,
    ) -> Result<ApiKeyMetadata, ThrotError> {
        let row = sqlx::query(
            r#"
            INSERT INTO api_keys (id, application_id, key_hash, key_prefix, status)
            VALUES ($1, $2, $3, $4, 'active')
            RETURNING id, application_id, key_prefix, status, created_at, expires_at, revoked_at
            "#,
        )
        .bind(key.id)
        .bind(key.application_id)
        .bind(&key.key_hash)
        .bind(&key.key_prefix)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| ThrotError::Storage(format!("Failed to store API key: {e}")))?;

        Ok(ApiKeyMetadata {
            id: row.get("id"),
            application_id: row.get("application_id"),
            key_prefix: row.get("key_prefix"),
            status: row.get("status"),
            created_at: row.get("created_at"),
            expires_at: row.get("expires_at"),
            revoked_at: row.get("revoked_at"),
        })
    }

    pub async fn list_api_keys(&self, app_id: Uuid) -> Result<Vec<ApiKeyMetadata>, ThrotError> {
        let rows = sqlx::query(
            r#"
            SELECT id, application_id, key_prefix, status, created_at, expires_at, revoked_at
            FROM api_keys
            WHERE application_id = $1 AND status != 'revoked'
            ORDER BY created_at DESC
            "#,
        )
        .bind(app_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ThrotError::Storage(format!("Failed to list API keys: {e}")))?;

        Ok(rows
            .into_iter()
            .map(|row| ApiKeyMetadata {
                id: row.get("id"),
                application_id: row.get("application_id"),
                key_prefix: row.get("key_prefix"),
                status: row.get("status"),
                created_at: row.get("created_at"),
                expires_at: row.get("expires_at"),
                revoked_at: row.get("revoked_at"),
            })
            .collect())
    }

    pub async fn revoke_api_key(&self, id: Uuid, app_id: Uuid) -> Result<bool, ThrotError> {
        let res = sqlx::query(
            r#"
            UPDATE api_keys
            SET status = 'revoked', revoked_at = NOW()
            WHERE id = $1 AND application_id = $2 AND status != 'revoked'
            "#,
        )
        .bind(id)
        .bind(app_id)
        .execute(&self.pool)
        .await
        .map_err(|e| ThrotError::Storage(format!("Failed to revoke API key: {e}")))?;

        Ok(res.rows_affected() > 0)
    }

    pub async fn verify_api_key(&self, raw_key: &str) -> Result<Option<Application>, ThrotError> {
        let hash = GeneratedApiKey::hash_key(raw_key);
        let row_opt = sqlx::query(
            r#"
            SELECT a.id, a.name, a.status, a.created_at, a.updated_at
            FROM applications a
            INNER JOIN api_keys k ON a.id = k.application_id
            WHERE k.key_hash = $1
              AND k.status = 'active'
              AND a.status = 'active'
              AND (k.expires_at IS NULL OR k.expires_at > NOW())
            "#,
        )
        .bind(hash)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| ThrotError::Storage(format!("Failed to verify API key: {e}")))?;

        Ok(row_opt.map(|row| Application {
            id: row.get("id"),
            name: row.get("name"),
            status: row.get("status"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        }))
    }

    // --- Rate Limit Policy Operations ---

    pub async fn create_policy(
        &self,
        policy: &RateLimitPolicy,
    ) -> Result<RateLimitPolicy, ThrotError> {
        let algo_str = match policy.algorithm {
            AlgorithmType::FixedWindow => "fixed_window",
            AlgorithmType::SlidingWindowLog => "sliding_window_log",
            AlgorithmType::SlidingWindowCounter => "sliding_window_counter",
            AlgorithmType::TokenBucket => "token_bucket",
            AlgorithmType::LeakyBucket => "leaky_bucket",
        };

        let mode_str = match policy.failure_mode {
            FailureMode::FailOpen => "fail_open",
            FailureMode::FailClosed => "fail_closed",
        };

        let row = sqlx::query(
            r#"
            INSERT INTO rate_limit_policies
                (id, application_id, name, algorithm, request_limit, window_seconds, burst_capacity, refill_rate, failure_mode)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            RETURNING id, application_id, name, algorithm, request_limit, window_seconds, burst_capacity, refill_rate, failure_mode, created_at, updated_at
            "#,
        )
        .bind(policy.id)
        .bind(policy.application_id)
        .bind(&policy.name)
        .bind(algo_str)
        .bind(policy.request_limit as i64)
        .bind(policy.window_seconds as i64)
        .bind(policy.burst_capacity.map(|v| v as i64))
        .bind(policy.refill_rate)
        .bind(mode_str)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| ThrotError::Storage(format!("Failed to create policy: {e}")))?;

        parse_policy_row(&row)
    }

    pub async fn get_policy(
        &self,
        id: Uuid,
        app_id: Uuid,
    ) -> Result<Option<RateLimitPolicy>, ThrotError> {
        let row_opt = sqlx::query(
            r#"
            SELECT id, application_id, name, algorithm, request_limit, window_seconds, burst_capacity, refill_rate, failure_mode, created_at, updated_at
            FROM rate_limit_policies
            WHERE id = $1 AND application_id = $2
            "#,
        )
        .bind(id)
        .bind(app_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| ThrotError::Storage(format!("Failed to get policy: {e}")))?;

        match row_opt {
            Some(row) => Ok(Some(parse_policy_row(&row)?)),
            None => Ok(None),
        }
    }

    pub async fn list_policies(&self, app_id: Uuid) -> Result<Vec<RateLimitPolicy>, ThrotError> {
        let rows = sqlx::query(
            r#"
            SELECT id, application_id, name, algorithm, request_limit, window_seconds, burst_capacity, refill_rate, failure_mode, created_at, updated_at
            FROM rate_limit_policies
            WHERE application_id = $1
            ORDER BY created_at DESC
            "#,
        )
        .bind(app_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ThrotError::Storage(format!("Failed to list policies: {e}")))?;

        let mut out = Vec::new();
        for r in rows {
            out.push(parse_policy_row(&r)?);
        }
        Ok(out)
    }
}

fn parse_policy_row(row: &sqlx::postgres::PgRow) -> Result<RateLimitPolicy, ThrotError> {
    let algo_str: String = row.get("algorithm");
    let algorithm = match algo_str.as_str() {
        "fixed_window" => AlgorithmType::FixedWindow,
        "sliding_window_log" => AlgorithmType::SlidingWindowLog,
        "sliding_window_counter" => AlgorithmType::SlidingWindowCounter,
        "token_bucket" => AlgorithmType::TokenBucket,
        "leaky_bucket" => AlgorithmType::LeakyBucket,
        other => {
            return Err(ThrotError::Internal(format!(
                "Unknown algorithm in database: {other}"
            )))
        }
    };

    let mode_str: String = row.get("failure_mode");
    let failure_mode = match mode_str.as_str() {
        "fail_open" => FailureMode::FailOpen,
        _ => FailureMode::FailClosed,
    };

    let req_limit: i64 = row.get("request_limit");
    let win_sec: i64 = row.get("window_seconds");
    let burst: Option<i64> = row.get("burst_capacity");

    Ok(RateLimitPolicy {
        id: row.get("id"),
        application_id: row.get("application_id"),
        name: row.get("name"),
        algorithm,
        request_limit: req_limit as u64,
        window_seconds: win_sec as u64,
        burst_capacity: burst.map(|v| v as u64),
        refill_rate: row.get("refill_rate"),
        failure_mode,
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}

#[async_trait]
impl PolicyStore for PostgresPool {
    async fn get_by_name(
        &self,
        application_id: Uuid,
        name: &str,
    ) -> Result<Option<RateLimitPolicy>, ThrotError> {
        let row_opt = sqlx::query(
            r#"
            SELECT id, application_id, name, algorithm, request_limit, window_seconds, burst_capacity, refill_rate, failure_mode, created_at, updated_at
            FROM rate_limit_policies
            WHERE application_id = $1 AND name = $2
            "#,
        )
        .bind(application_id)
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| ThrotError::Storage(format!("Failed to query policy by name: {e}")))?;

        match row_opt {
            Some(row) => Ok(Some(parse_policy_row(&row)?)),
            None => Ok(None),
        }
    }

    async fn get_default(
        &self,
        application_id: Uuid,
    ) -> Result<Option<RateLimitPolicy>, ThrotError> {
        self.get_by_name(application_id, "default").await
    }

    async fn get_by_identity(
        &self,
        application_id: Uuid,
        identity: &Identity,
    ) -> Result<Option<RateLimitPolicy>, ThrotError> {
        let row_opt = sqlx::query(
            r#"
            SELECT p.id, p.application_id, p.name, p.algorithm, p.request_limit, p.window_seconds, p.burst_capacity, p.refill_rate, p.failure_mode, p.created_at, p.updated_at
            FROM rate_limit_policies p
            INNER JOIN policy_assignments a ON p.id = a.policy_id
            WHERE p.application_id = $1 AND a.identity_value = $2
            "#,
        )
        .bind(application_id)
        .bind(identity.as_str())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| ThrotError::Storage(format!("Failed to query policy by identity: {e}")))?;

        match row_opt {
            Some(row) => Ok(Some(parse_policy_row(&row)?)),
            None => Ok(None),
        }
    }
}
