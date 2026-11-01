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
pub mod in_memory;
pub mod leaky_bucket;
pub(crate) mod sharded;
pub mod sliding_counter;
pub mod sliding_log;
pub mod token_bucket;

pub use fixed_window::{FixedWindowRateLimiter, FIXED_WINDOW_LUA_SCRIPT};
pub use in_memory::InMemoryRateLimitEvaluator;
pub use leaky_bucket::{LeakyBucketRateLimiter, LEAKY_BUCKET_LUA_SCRIPT};
pub use sliding_counter::{SlidingCounterRateLimiter, SLIDING_COUNTER_LUA_SCRIPT};
pub use sliding_log::{SlidingLogRateLimiter, SLIDING_LOG_LUA_SCRIPT};
pub use token_bucket::{TokenBucketRateLimiter, TOKEN_BUCKET_LUA_SCRIPT};
