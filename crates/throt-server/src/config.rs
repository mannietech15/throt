use std::env;
use std::net::SocketAddr;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Missing required environment variable: {0}")]
    Missing(String),

    #[error("Invalid configuration for {0}: {1}")]
    Invalid(String, String),
}

/// Server runtime configuration loaded from environment variables.
#[derive(Debug, Clone)]
pub struct Config {
    pub app_env: String,
    pub host: String,
    pub port: u16,
    pub database_url: String,
    pub redis_url: String,
    pub log_level: String,
}

impl Config {
    /// Loads configuration from the environment with explicit validation.
    pub fn from_env() -> Result<Self, ConfigError> {
        let app_env = env::var("APP_ENV").unwrap_or_else(|_| "development".to_string());
        let host = env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());

        let port_str = env::var("PORT").unwrap_or_else(|_| "8080".to_string());
        let port = port_str
            .parse::<u16>()
            .map_err(|e| ConfigError::Invalid("PORT".to_string(), e.to_string()))?;

        let database_url = env::var("DATABASE_URL")
            .unwrap_or_else(|_| "postgres://throt:throt_secret@localhost:5432/throt".to_string());

        let redis_url =
            env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string());

        let log_level = env::var("LOG_LEVEL").unwrap_or_else(|_| "info".to_string());

        Ok(Self {
            app_env,
            host,
            port,
            database_url,
            redis_url,
            log_level,
        })
    }

    /// Resolves the socket address to bind to.
    pub fn socket_addr(&self) -> Result<SocketAddr, ConfigError> {
        let addr_str = format!("{}:{}", self.host, self.port);
        addr_str
            .parse::<SocketAddr>()
            .map_err(|e| ConfigError::Invalid("HOST/PORT".to_string(), e.to_string()))
    }
}
