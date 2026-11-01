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

    /// Returns a reference to the underlying Redis client.
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Returns a cloned connection manager for async operations.
    pub fn get_connection(&self) -> ConnectionManager {
        self.connection_manager.clone()
    }
}
