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
            other => Err(ThrotError::Internal(format!(
                "Algorithm '{:?}' not yet registered in Redis evaluator",
                other
            ))),
        }
    }
}
