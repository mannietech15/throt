//! Rate limiting algorithm implementations and interfaces.

use async_trait::async_trait;
use throt_core::{Decision, RateLimitPolicy, ThrotError};

/// Common trait for all rate-limiting algorithms.
#[async_trait]
pub trait RateLimiter: Send + Sync {
    /// Evaluates whether a request with a given `cost` should be allowed.
    async fn evaluate(
        &self,
        key: &str,
        cost: u64,
        policy: &RateLimitPolicy,
    ) -> Result<Decision, ThrotError>;
}

pub mod fixed_window;
pub mod leaky_bucket;
pub mod sliding_counter;
pub mod sliding_log;
pub mod token_bucket;
