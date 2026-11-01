use async_trait::async_trait;
use throt_core::{Decision, RateLimitPolicy, ThrotError};

use crate::RateLimiter;

/// Redis Lua script for distributed atomic Sliding Window Log evaluation.
/// Uses a Redis sorted set where score is the timestamp in milliseconds.
pub const SLIDING_LOG_LUA_SCRIPT: &str = r#"
local now = redis.call('TIME')
local current_sec = tonumber(now[1])
local current_usec = tonumber(now[2])
local current_ms = (current_sec * 1000) + math.floor(current_usec / 1000)

local cost = tonumber(ARGV[1])
local limit = tonumber(ARGV[2])
local window_sec = tonumber(ARGV[3])
local window_ms = window_sec * 1000
local threshold_ms = current_ms - window_ms

-- 1. Remove expired entries older than the rolling window
redis.call('ZREMRANGEBYSCORE', KEYS[1], '-inf', threshold_ms)

-- 2. Count current elements in window
local current_count = redis.call('ZCARD', KEYS[1])

if current_count + cost <= limit then
    -- Add entries with unique member identifiers
    for i = 1, cost do
        local member = tostring(current_ms) .. ':' .. tostring(current_usec) .. ':' .. tostring(i)
        redis.call('ZADD', KEYS[1], current_ms, member)
    end
    redis.call('EXPIRE', KEYS[1], window_sec + 1)

    local remaining = limit - (current_count + cost)
    local reset_at = current_sec + window_sec
    return { 1, remaining, reset_at, 0 }
else
    -- Compute retry_after from the oldest entry in the window
    local oldest = redis.call('ZRANGE', KEYS[1], 0, 0, 'WITHSCORES')
    local retry_after = window_sec
    if #oldest >= 2 then
        local oldest_ms = tonumber(oldest[2])
        local wait_ms = (oldest_ms + window_ms) - current_ms
        retry_after = math.max(1, math.ceil(wait_ms / 1000))
    end

    local reset_at = current_sec + retry_after
    return { 0, 0, reset_at, retry_after }
end
"#;

/// In-memory sliding window log rate limiter.
#[derive(Default)]
pub struct SlidingLogRateLimiter {
    state: crate::sharded::ShardedState<Vec<i64>>,
}

impl SlidingLogRateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Evaluates against a provided millisecond timestamp.
    pub fn evaluate_at(
        &self,
        key: &str,
        cost: u64,
        policy: &RateLimitPolicy,
        now_ms: i64,
    ) -> Result<Decision, ThrotError> {
        let window_ms = (policy.window_seconds * 1000) as i64;
        let limit = policy.request_limit;
        let threshold_ms = now_ms - window_ms;
        let current_sec = now_ms / 1000;

        let shard = self.state.get_shard(key);
        let mut logs = shard.write().unwrap();
        let log = logs.entry(key.to_string()).or_default();

        // Prune timestamps older than threshold
        log.retain(|&ts| ts > threshold_ms);

        if (log.len() as u64) + cost <= limit {
            for _ in 0..cost {
                log.push(now_ms);
            }
            let remaining = limit - (log.len() as u64);
            let reset_at = current_sec + policy.window_seconds as i64;
            Ok(Decision::allow(limit, remaining, reset_at))
        } else {
            let retry_after = if let Some(&oldest_ms) = log.first() {
                let wait_ms = (oldest_ms + window_ms) - now_ms;
                ((wait_ms + 999) / 1000).max(1) as u64
            } else {
                policy.window_seconds
            };

            let reset_at = current_sec + retry_after as i64;
            Ok(Decision::reject(limit, reset_at, retry_after))
        }
    }
}

#[async_trait]
impl RateLimiter for SlidingLogRateLimiter {
    async fn evaluate(
        &self,
        key: &str,
        cost: u64,
        policy: &RateLimitPolicy,
    ) -> Result<Decision, ThrotError> {
        let now_ms = chrono::Utc::now().timestamp_millis();
        self.evaluate_at(key, cost, policy, now_ms)
    }
}

#[async_trait]
impl throt_core::RateLimitEvaluator for SlidingLogRateLimiter {
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

    fn make_policy(limit: u64, window_seconds: u64) -> RateLimitPolicy {
        RateLimitPolicy {
            id: Uuid::new_v4(),
            application_id: Uuid::new_v4(),
            name: "sliding-log-test".to_string(),
            algorithm: AlgorithmType::SlidingWindowLog,
            request_limit: limit,
            window_seconds,
            burst_capacity: None,
            refill_rate: None,
            failure_mode: FailureMode::FailClosed,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn test_sliding_log_rolling_expiry() {
        let limiter = SlidingLogRateLimiter::new();
        let policy = make_policy(3, 10); // 3 requests per 10 seconds

        // t = 1000ms: request 1
        assert!(
            limiter
                .evaluate_at("user:1", 1, &policy, 1000)
                .unwrap()
                .allowed
        );
        // t = 4000ms: request 2
        assert!(
            limiter
                .evaluate_at("user:1", 1, &policy, 4000)
                .unwrap()
                .allowed
        );
        // t = 7000ms: request 3
        let d3 = limiter.evaluate_at("user:1", 1, &policy, 7000).unwrap();
        assert!(d3.allowed);
        assert_eq!(d3.remaining, 0);

        // t = 8000ms: request 4 exceeds limit (all 3 still in 10s window)
        let d4 = limiter.evaluate_at("user:1", 1, &policy, 8000).unwrap();
        assert!(!d4.allowed);
        assert_eq!(d4.retry_after, Some(3)); // oldest is 1000ms; expires at 11000ms; 11000 - 8000 = 3000ms -> 3s

        // t = 11001ms: oldest (1000ms) has expired; now 2 entries remain in window (4000ms and 7000ms)
        let d5 = limiter.evaluate_at("user:1", 1, &policy, 11001).unwrap();
        assert!(d5.allowed);
        assert_eq!(d5.remaining, 0); // now 3 entries (4000ms, 7000ms, 11001ms)
    }
}
