use redis::aio::ConnectionManager;
use redis::Client;
use throt_core::ThrotError;
use tracing::info;

/// Redis storage manager for atomic rate-limiting scripts and shared state.
#[derive(Clone)]
pub struct RedisStorage {
    client: Client,
    connection_manager: ConnectionManager,
}

impl RedisStorage {
    /// Connects to Redis and establishes an asynchronous connection manager.
    pub async fn connect(redis_url: &str) -> Result<Self, ThrotError> {
        let client = Client::open(redis_url)
            .map_err(|e| ThrotError::Storage(format!("Invalid Redis URL: {e}")))?;

        let connection_manager = ConnectionManager::new(client.clone())
            .await
            .map_err(|e| ThrotError::Storage(format!("Failed to connect to Redis: {e}")))?;

        info!("Successfully connected to Redis distributed store");
        Ok(Self {
            client,
            connection_manager,
        })
    }

    /// Healthcheck: pings Redis and verifies responsiveness.
    pub async fn ping(&self) -> Result<(), ThrotError> {
        let mut conn = self.connection_manager.clone();
        let pong: String = redis::cmd("PING")
            .query_async(&mut conn)
            .await
            .map_err(|e| ThrotError::Storage(format!("Redis ping failed: {e}")))?;

        if pong == "PONG" {
            Ok(())
        } else {
            Err(ThrotError::Storage(format!(
                "Unexpected Redis ping response: {pong}"
            )))
        }
    }

    /// Evaluates a rate limit check atomically via Redis Lua script.
    pub async fn evaluate_fixed_window(
        &self,
        key: &str,
        cost: u64,
        limit: u64,
        window_seconds: u64,
    ) -> Result<throt_core::Decision, ThrotError> {
        let mut conn = self.connection_manager.clone();
        let script = redis::Script::new(throt_algorithms::FIXED_WINDOW_LUA_SCRIPT);

        let result: Vec<i64> = script
            .key(key)
            .arg(cost)
            .arg(limit)
            .arg(window_seconds)
            .invoke_async(&mut conn)
            .await
            .map_err(|e| ThrotError::Storage(format!("Redis script execution failed: {e}")))?;

        if result.len() < 4 {
            return Err(ThrotError::Storage(
                "Redis rate limit script returned invalid response format".to_string(),
            ));
        }

        let allowed = result[0] == 1;
        let remaining = result[1].max(0) as u64;
        let reset_at = result[2];
        let retry_after = result[3].max(0) as u64;

        if allowed {
            Ok(throt_core::Decision::allow(limit, remaining, reset_at))
        } else {
            Ok(throt_core::Decision::reject(limit, reset_at, retry_after))
        }
    }

    /// Returns a reference to the underlying Redis client.
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Returns a cloned connection manager for async operations.
    pub fn get_connection(&self) -> ConnectionManager {
        self.connection_manager.clone()
    }
}
