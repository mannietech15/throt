use async_trait::async_trait;
use throt_core::{
    AlgorithmType, Decision, Identity, RateLimitEvaluator, RateLimitPolicy, ThrotError,
};
use uuid::Uuid;

use crate::{
    fixed_window::FixedWindowRateLimiter, leaky_bucket::LeakyBucketRateLimiter,
    sliding_counter::SlidingCounterRateLimiter, sliding_log::SlidingLogRateLimiter,
    token_bucket::TokenBucketRateLimiter, RateLimiter,
};

/// Unified in-memory rate-limiting evaluator supporting all 5 algorithms.
///
/// Provides high-throughput, lock-striped local rate limiting ideal for standalone deployments,
/// in-process bindings (e.g. Node.js NAPI-RS / Python PyO3), or distributed fallback caching.
#[derive(Default)]
pub struct InMemoryRateLimitEvaluator {
    fixed_window: FixedWindowRateLimiter,
    sliding_log: SlidingLogRateLimiter,
    sliding_counter: SlidingCounterRateLimiter,
    token_bucket: TokenBucketRateLimiter,
    leaky_bucket: LeakyBucketRateLimiter,
}

impl InMemoryRateLimitEvaluator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Formats a canonical key matching the Redis cluster format.
    pub fn format_key(application_id: Uuid, policy_id: Uuid, identity: &Identity) -> String {
        format!("throt:{{{}:{}}}:{}", application_id, policy_id, identity)
    }

    pub fn fixed_window(&self) -> &FixedWindowRateLimiter {
        &self.fixed_window
    }

    pub fn sliding_log(&self) -> &SlidingLogRateLimiter {
        &self.sliding_log
    }

    pub fn sliding_counter(&self) -> &SlidingCounterRateLimiter {
        &self.sliding_counter
    }

    pub fn token_bucket(&self) -> &TokenBucketRateLimiter {
        &self.token_bucket
    }

    pub fn leaky_bucket(&self) -> &LeakyBucketRateLimiter {
        &self.leaky_bucket
    }
}

#[async_trait]
impl RateLimitEvaluator for InMemoryRateLimitEvaluator {
    async fn evaluate_request(
        &self,
        application_id: Uuid,
        policy: &RateLimitPolicy,
        identity: &Identity,
        cost: u64,
    ) -> Result<Decision, ThrotError> {
        let key = Self::format_key(application_id, policy.id, identity);

        match policy.algorithm {
            AlgorithmType::FixedWindow => self.fixed_window.evaluate(&key, cost, policy).await,
            AlgorithmType::SlidingWindowLog => self.sliding_log.evaluate(&key, cost, policy).await,
            AlgorithmType::SlidingWindowCounter => {
                self.sliding_counter.evaluate(&key, cost, policy).await
            }
            AlgorithmType::TokenBucket => self.token_bucket.evaluate(&key, cost, policy).await,
            AlgorithmType::LeakyBucket => self.leaky_bucket.evaluate(&key, cost, policy).await,
        }
    }
}
