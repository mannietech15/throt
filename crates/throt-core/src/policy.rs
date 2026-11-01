use crate::errors::ThrotError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Supported rate-limiting algorithms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlgorithmType {
    FixedWindow,
    SlidingWindowLog,
    SlidingWindowCounter,
    TokenBucket,
    LeakyBucket,
}

/// Fallback behavior when a storage failure occurs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FailureMode {
    #[default]
    FailClosed,
    FailOpen,
}

/// A rate-limiting policy governing request allowances.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RateLimitPolicy {
    pub id: Uuid,
    pub application_id: Uuid,
    pub name: String,
    pub algorithm: AlgorithmType,
    pub request_limit: u64,
    pub window_seconds: u64,
    pub burst_capacity: Option<u64>,
    pub refill_rate: Option<f64>,
    pub failure_mode: FailureMode,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl RateLimitPolicy {
    /// Returns a new fluent builder for constructing validated rate-limit policies.
    pub fn builder(name: impl Into<String>, algorithm: AlgorithmType) -> RateLimitPolicyBuilder {
        RateLimitPolicyBuilder::new(name, algorithm)
    }
}

/// Strongly typed builder for `RateLimitPolicy`.
#[derive(Debug, Clone)]
pub struct RateLimitPolicyBuilder {
    id: Option<Uuid>,
    application_id: Option<Uuid>,
    name: String,
    algorithm: AlgorithmType,
    request_limit: Option<u64>,
    window_seconds: Option<u64>,
    burst_capacity: Option<u64>,
    refill_rate: Option<f64>,
    failure_mode: FailureMode,
}

impl RateLimitPolicyBuilder {
    /// Creates a builder with the required policy name and algorithm.
    pub fn new(name: impl Into<String>, algorithm: AlgorithmType) -> Self {
        Self {
            id: None,
            application_id: None,
            name: name.into(),
            algorithm,
            request_limit: None,
            window_seconds: None,
            burst_capacity: None,
            refill_rate: None,
            failure_mode: FailureMode::FailClosed,
        }
    }

    /// Sets an explicit policy ID (auto-generates UUIDv4 if omitted).
    pub fn id(mut self, id: Uuid) -> Self {
        self.id = Some(id);
        self
    }

    /// Sets the application ID. Must be non-nil.
    pub fn application_id(mut self, app_id: Uuid) -> Self {
        self.application_id = Some(app_id);
        self
    }

    /// Sets the application ID from a UUID string representation.
    pub fn application_str(self, app_id_str: &str) -> Result<Self, ThrotError> {
        let app_id = Uuid::parse_str(app_id_str.trim()).map_err(|e| {
            ThrotError::Validation(format!("Invalid UUID format for application ID: {e}"))
        })?;
        Ok(self.application_id(app_id))
    }

    /// Sets the maximum number of requests allowed within the window.
    pub fn request_limit(mut self, limit: u64) -> Self {
        self.request_limit = Some(limit);
        self
    }

    /// Sets the window interval in seconds. Defaults to 60 seconds if omitted.
    pub fn window_seconds(mut self, seconds: u64) -> Self {
        self.window_seconds = Some(seconds);
        self
    }

    /// Sets the maximum burst capacity (must be >= request_limit if provided).
    pub fn burst_capacity(mut self, burst: u64) -> Self {
        self.burst_capacity = Some(burst);
        self
    }

    /// Sets the refill or leak rate (must be > 0.0 if provided).
    pub fn refill_rate(mut self, rate: f64) -> Self {
        self.refill_rate = Some(rate);
        self
    }

    /// Sets the failure mode for dependency outages (`FailClosed` by default).
    pub fn failure_mode(mut self, mode: FailureMode) -> Self {
        self.failure_mode = mode;
        self
    }

    /// Validates all configuration invariants and builds the `RateLimitPolicy`.
    pub fn build(self) -> Result<RateLimitPolicy, ThrotError> {
        let name = self.name.trim().to_string();
        if name.is_empty() {
            return Err(ThrotError::Validation(
                "Policy name cannot be empty".to_string(),
            ));
        }

        let application_id = match self.application_id {
            Some(id) if !id.is_nil() => id,
            Some(_) => {
                return Err(ThrotError::Validation(
                    "Application ID cannot be nil (00000000-0000-0000-0000-000000000000)"
                        .to_string(),
                ))
            }
            None => {
                return Err(ThrotError::Validation(
                    "Application ID is required to build a rate-limit policy".to_string(),
                ))
            }
        };

        let request_limit = match self.request_limit {
            Some(lim) if lim > 0 => lim,
            Some(_) => {
                return Err(ThrotError::Validation(
                    "Request limit must be greater than 0".to_string(),
                ))
            }
            None => {
                return Err(ThrotError::Validation(
                    "Request limit is required to build a rate-limit policy".to_string(),
                ))
            }
        };

        let window_seconds = match self.window_seconds {
            Some(w) if w > 0 => w,
            Some(_) => {
                return Err(ThrotError::Validation(
                    "Window seconds must be greater than 0".to_string(),
                ))
            }
            None => 60, // Sensible default: 60 seconds
        };

        if let Some(burst) = self.burst_capacity {
            if burst < request_limit {
                return Err(ThrotError::Validation(
                    "Burst capacity cannot be less than request limit".to_string(),
                ));
            }
        }

        if let Some(refill) = self.refill_rate {
            if refill <= 0.0 {
                return Err(ThrotError::Validation(
                    "Refill rate must be greater than 0.0".to_string(),
                ));
            }
        }

        let now = Utc::now();
        Ok(RateLimitPolicy {
            id: self.id.unwrap_or_else(Uuid::new_v4),
            application_id,
            name,
            algorithm: self.algorithm,
            request_limit,
            window_seconds,
            burst_capacity: self.burst_capacity,
            refill_rate: self.refill_rate,
            failure_mode: self.failure_mode,
            created_at: now,
            updated_at: now,
        })
    }
}
