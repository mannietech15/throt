use async_trait::async_trait;
use throt_core::{Decision, RateLimitPolicy, ThrotError};

use crate::RateLimiter;

/// Redis Lua script for distributed atomic Token Bucket evaluation.
pub const TOKEN_BUCKET_LUA_SCRIPT: &str = r#"
local now = redis.call('TIME')
local current_sec = tonumber(now[1])
local current_usec = tonumber(now[2])
local now_f = current_sec + (current_usec / 1000000.0)

local cost = tonumber(ARGV[1])
local capacity = tonumber(ARGV[2])
local refill_rate = tonumber(ARGV[3]) -- tokens per second

local data = redis.call('HMGET', KEYS[1], 'tokens', 'last_updated')
local tokens = tonumber(data[1])
local last_updated = tonumber(data[2])

if not tokens or not last_updated then
    tokens = capacity
    last_updated = now_f
else
    local elapsed = math.max(0.0, now_f - last_updated)
    tokens = math.min(capacity, tokens + (elapsed * refill_rate))
    last_updated = now_f
end

local reset_at = math.ceil(now_f + math.max(0.0, (capacity - tokens) / refill_rate))

if tokens >= cost then
    tokens = tokens - cost
    redis.call('HMSET', KEYS[1], 'tokens', tokens, 'last_updated', last_updated)
    local ttl = math.max(1, math.ceil(capacity / refill_rate) * 2)
    redis.call('EXPIRE', KEYS[1], ttl)

    return { 1, math.floor(tokens), reset_at, 0 }
else
    local needed = cost - tokens
    local retry_after = math.max(1, math.ceil(needed / refill_rate))
    redis.call('HMSET', KEYS[1], 'tokens', tokens, 'last_updated', last_updated)
    return { 0, math.floor(tokens), reset_at, retry_after }
end
"#;

#[derive(Debug, Clone, Copy)]
struct BucketState {
    tokens: f64,
    last_updated_secs: f64,
}

/// In-memory Token Bucket rate limiter.
#[derive(Default)]
pub struct TokenBucketRateLimiter {
    state: crate::sharded::ShardedState<BucketState>,
}

impl TokenBucketRateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn evaluate_at(
        &self,
        key: &str,
        cost: u64,
        policy: &RateLimitPolicy,
        now_secs: f64,
    ) -> Result<Decision, ThrotError> {
        let capacity = policy.burst_capacity.unwrap_or(policy.request_limit) as f64;
        let refill_rate = policy
            .refill_rate
            .unwrap_or(policy.request_limit as f64 / policy.window_seconds as f64);

        if refill_rate <= 0.0 {
            return Err(ThrotError::Validation(
                "Refill rate must be greater than 0".to_string(),
            ));
        }

        let shard = self.state.get_shard(key);
        let mut buckets = shard.write().unwrap();
        let bucket = buckets.entry(key.to_string()).or_insert(BucketState {
            tokens: capacity,
            last_updated_secs: now_secs,
        });

        // Replenish tokens based on elapsed time
        let elapsed = (now_secs - bucket.last_updated_secs).max(0.0);
        bucket.tokens = (bucket.tokens + elapsed * refill_rate).min(capacity);
        bucket.last_updated_secs = now_secs;

        let reset_at = (now_secs + ((capacity - bucket.tokens) / refill_rate)).ceil() as i64;

        if bucket.tokens >= cost as f64 {
            bucket.tokens -= cost as f64;
            let remaining = bucket.tokens.floor() as u64;
            Ok(Decision::allow(capacity as u64, remaining, reset_at))
        } else {
            let needed = cost as f64 - bucket.tokens;
            let retry_after = (needed / refill_rate).ceil().max(1.0) as u64;
            Ok(Decision::reject(capacity as u64, reset_at, retry_after))
        }
    }
}

#[async_trait]
impl RateLimiter for TokenBucketRateLimiter {
    async fn evaluate(
        &self,
        key: &str,
        cost: u64,
        policy: &RateLimitPolicy,
    ) -> Result<Decision, ThrotError> {
        let now_secs = chrono::Utc::now().timestamp_micros() as f64 / 1_000_000.0;
        self.evaluate_at(key, cost, policy, now_secs)
    }
}

#[async_trait]
impl throt_core::RateLimitEvaluator for TokenBucketRateLimiter {
    async fn evaluate_request(
        &self,
        application_id: uuid::Uuid,
        policy: &RateLimitPolicy,
        identity: &throt_core::Identity,
        cost: u64,
    ) -> Result<Decision, ThrotError> {
        let key = format!("throt:{{{}:{}}}:{}", application_id, policy.id, identity);
        self.evaluate(&key, cost, policy).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use throt_core::{AlgorithmType, FailureMode};
    use uuid::Uuid;

    fn make_policy(burst: u64, refill_rate: f64) -> RateLimitPolicy {
        RateLimitPolicy {
            id: Uuid::new_v4(),
            application_id: Uuid::new_v4(),
            name: "token-bucket-test".to_string(),
            algorithm: AlgorithmType::TokenBucket,
            request_limit: burst,
            window_seconds: 60,
            burst_capacity: Some(burst),
            refill_rate: Some(refill_rate),
            failure_mode: FailureMode::FailClosed,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn test_token_bucket_burst_and_refill() {
        let limiter = TokenBucketRateLimiter::new();
        // Capacity: 10 tokens, refill rate: 2 tokens per second
        let policy = make_policy(10, 2.0);

        // t = 0s: consume burst of 8
        let d1 = limiter.evaluate_at("user:1", 8, &policy, 0.0).unwrap();
        assert!(d1.allowed);
        assert_eq!(d1.remaining, 2);

        // t = 0s: consume 2 more (bucket empty)
        let d2 = limiter.evaluate_at("user:1", 2, &policy, 0.0).unwrap();
        assert!(d2.allowed);
        assert_eq!(d2.remaining, 0);

        // t = 0s: request 1 rejected (needed 1 token at 2 tokens/sec -> retry_after 1s)
        let d3 = limiter.evaluate_at("user:1", 1, &policy, 0.0).unwrap();
        assert!(!d3.allowed);
        assert_eq!(d3.retry_after, Some(1));

        // t = 2.0s: 4 tokens refilled (2.0s * 2 tokens/s = 4 tokens)
        let d4 = limiter.evaluate_at("user:1", 3, &policy, 2.0).unwrap();
        assert!(d4.allowed);
        assert_eq!(d4.remaining, 1);
    }
}
