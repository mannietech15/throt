use async_trait::async_trait;
use throt_core::{Decision, RateLimitPolicy, ThrotError};

use crate::RateLimiter;

/// Redis Lua script for distributed atomic Sliding Window Counter evaluation.
pub const SLIDING_COUNTER_LUA_SCRIPT: &str = r#"
local now = redis.call('TIME')
local current_sec = tonumber(now[1])
local cost = tonumber(ARGV[1])
local limit = tonumber(ARGV[2])
local window_sec = tonumber(ARGV[3])

local current_window = math.floor(current_sec / window_sec) * window_sec
local reset_at = current_window + window_sec
local retry_after = reset_at - current_sec
if retry_after <= 0 then retry_after = 1 end

local data = redis.call('HMGET', KEYS[1], 'w', 'c', 'p')
local stored_w = tonumber(data[1] or '0')
local current_c = tonumber(data[2] or '0')
local prev_c = tonumber(data[3] or '0')

if current_window > stored_w then
    if current_window == stored_w + window_sec then
        prev_c = current_c
    else
        prev_c = 0
    end
    current_c = 0
    stored_w = current_window
end

local elapsed = current_sec - current_window
local weight = (window_sec - elapsed) / window_sec
local estimated = math.floor(prev_c * weight) + current_c

if estimated + cost <= limit then
    current_c = current_c + cost
    redis.call('HMSET', KEYS[1], 'w', stored_w, 'c', current_c, 'p', prev_c)
    redis.call('EXPIRE', KEYS[1], window_sec * 2)

    local remaining = limit - (estimated + cost)
    return { 1, remaining, reset_at, 0 }
else
    return { 0, 0, reset_at, retry_after }
end
"#;

#[derive(Debug, Default, Clone, Copy)]
struct WindowCounts {
    current_window: i64,
    current_count: u64,
    prev_count: u64,
}

/// In-memory sliding window counter rate limiter.
#[derive(Default)]
pub struct SlidingCounterRateLimiter {
    state: crate::sharded::ShardedState<WindowCounts>,
}

impl SlidingCounterRateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn evaluate_at(
        &self,
        key: &str,
        cost: u64,
        policy: &RateLimitPolicy,
        now_sec: i64,
    ) -> Result<Decision, ThrotError> {
        let window_sec = policy.window_seconds as i64;
        let limit = policy.request_limit;

        let current_window = (now_sec / window_sec) * window_sec;
        let reset_at = current_window + window_sec;
        let retry_after = (reset_at - now_sec).max(1) as u64;

        let shard = self.state.get_shard(key);
        let mut buckets = shard.write().unwrap();
        let bucket = buckets.entry(key.to_string()).or_insert(WindowCounts {
            current_window,
            current_count: 0,
            prev_count: 0,
        });

        // Window transition logic
        if current_window > bucket.current_window {
            if current_window == bucket.current_window + window_sec {
                bucket.prev_count = bucket.current_count;
            } else {
                bucket.prev_count = 0;
            }
            bucket.current_count = 0;
            bucket.current_window = current_window;
        }

        let elapsed = (now_sec - current_window) as f64;
        let weight = (window_sec as f64 - elapsed) / (window_sec as f64);
        let estimated = ((bucket.prev_count as f64) * weight).floor() as u64 + bucket.current_count;

        if estimated + cost <= limit {
            bucket.current_count += cost;
            let remaining = limit - (estimated + cost);
            Ok(Decision::allow(limit, remaining, reset_at))
        } else {
            Ok(Decision::reject(limit, reset_at, retry_after))
        }
    }
}

#[async_trait]
impl RateLimiter for SlidingCounterRateLimiter {
    async fn evaluate(
        &self,
        key: &str,
        cost: u64,
        policy: &RateLimitPolicy,
    ) -> Result<Decision, ThrotError> {
        let now_sec = chrono::Utc::now().timestamp();
        self.evaluate_at(key, cost, policy, now_sec)
    }
}

#[async_trait]
impl throt_core::RateLimitEvaluator for SlidingCounterRateLimiter {
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
            name: "sliding-counter-test".to_string(),
            algorithm: AlgorithmType::SlidingWindowCounter,
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
    fn test_sliding_counter_smoothing() {
        let limiter = SlidingCounterRateLimiter::new();
        let policy = make_policy(10, 100);

        // Previous window (t = 0..100): consumes 10 requests
        for _ in 0..10 {
            assert!(
                limiter
                    .evaluate_at("user:1", 1, &policy, 50)
                    .unwrap()
                    .allowed
            );
        }

        // At t = 130 (30% through current window 100..200):
        // Weight from prev window is (100 - 30) / 100 = 0.70
        // Estimated prev contribution = 10 * 0.7 = 7 requests
        // Available quota in current window: 10 - 7 = 3 requests
        let d1 = limiter.evaluate_at("user:1", 3, &policy, 130).unwrap();
        assert!(d1.allowed);
        assert_eq!(d1.remaining, 0);

        // Request 4 at t = 130 should now be rejected
        let d2 = limiter.evaluate_at("user:1", 1, &policy, 130).unwrap();
        assert!(!d2.allowed);

        // At t = 180 (80% through current window):
        // Weight from prev window decreases to (100 - 80) / 100 = 0.20
        // Prev contribution = 10 * 0.2 = 2 requests
        // Current count is 3. Total estimated = 2 + 3 = 5.
        // Available quota = 10 - 5 = 5 requests!
        let d3 = limiter.evaluate_at("user:1", 2, &policy, 180).unwrap();
        assert!(d3.allowed);
    }
}
