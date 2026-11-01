use crate::redis::RedisStorage;
use async_trait::async_trait;
use throt_core::{
    AlgorithmType, Decision, Identity, RateLimitEvaluator, RateLimitPolicy, ThrotError,
};
use uuid::Uuid;

/// Redis-backed rate-limit evaluator ensuring distributed atomicity.
#[derive(Clone)]
pub struct RedisRateLimitEvaluator {
    redis: RedisStorage,
}

impl RedisRateLimitEvaluator {
    pub fn new(redis: RedisStorage) -> Self {
        Self { redis }
    }

    /// Formats a Redis key using cluster-safe hash tags.
    pub fn format_key(application_id: Uuid, policy_id: Uuid, identity: &Identity) -> String {
        format!("throt:{{{}:{}}}:{}", application_id, policy_id, identity)
    }
}

#[async_trait]
impl RateLimitEvaluator for RedisRateLimitEvaluator {
    async fn evaluate_request(
        &self,
        application_id: Uuid,
        policy: &RateLimitPolicy,
        identity: &Identity,
        cost: u64,
    ) -> Result<Decision, ThrotError> {
        let key = Self::format_key(application_id, policy.id, identity);

        match policy.algorithm {
            AlgorithmType::FixedWindow => {
                self.redis
                    .evaluate_fixed_window(&key, cost, policy.request_limit, policy.window_seconds)
                    .await
            }
            AlgorithmType::SlidingWindowLog => {
                self.redis
                    .evaluate_sliding_log(&key, cost, policy.request_limit, policy.window_seconds)
                    .await
            }
            AlgorithmType::SlidingWindowCounter => {
                self.redis
                    .evaluate_sliding_counter(
                        &key,
                        cost,
                        policy.request_limit,
                        policy.window_seconds,
                    )
                    .await
            }
            AlgorithmType::TokenBucket => {
                let capacity = policy.burst_capacity.unwrap_or(policy.request_limit);
                let refill_rate = policy
                    .refill_rate
                    .unwrap_or(policy.request_limit as f64 / policy.window_seconds as f64);
                self.redis
                    .evaluate_token_bucket(&key, cost, capacity, refill_rate)
                    .await
            }
            AlgorithmType::LeakyBucket => {
                let capacity = policy.burst_capacity.unwrap_or(policy.request_limit);
                let leak_rate = policy
                    .refill_rate
                    .unwrap_or(policy.request_limit as f64 / policy.window_seconds as f64);
                self.redis
                    .evaluate_leaky_bucket(&key, cost, capacity, leak_rate)
                    .await
            }
        }
    }
}
