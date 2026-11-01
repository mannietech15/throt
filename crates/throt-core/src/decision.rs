use serde::{Deserialize, Serialize};

/// Represents the explicit decision returned by THROT for an evaluation request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    /// Whether the request is permitted under the applicable rate limit.
    pub allowed: bool,
    /// Maximum allowed requests within the configured policy window.
    pub limit: u64,
    /// Remaining request quota available in the current window.
    pub remaining: u64,
    /// Unix timestamp (in seconds) when the current window or quota resets.
    pub reset_at: i64,
    /// Number of seconds the client must wait before retrying, if rejected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_after: Option<u64>,
}

impl Decision {
    /// Creates an allowed rate limit decision.
    pub fn allow(limit: u64, remaining: u64, reset_at: i64) -> Self {
        Self {
            allowed: true,
            limit,
            remaining,
            reset_at,
            retry_after: None,
        }
    }

    /// Creates a rejected rate limit decision with a retry-after interval.
    pub fn reject(limit: u64, reset_at: i64, retry_after: u64) -> Self {
        Self {
            allowed: false,
            limit,
            remaining: 0,
            reset_at,
            retry_after: Some(retry_after),
        }
    }
}
