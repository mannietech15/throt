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
