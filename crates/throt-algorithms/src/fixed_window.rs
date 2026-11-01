use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::RwLock;
use throt_core::{Decision, RateLimitPolicy, ThrotError};

use crate::RateLimiter;

/// Redis Lua script for distributed atomic fixed window evaluation.
pub const FIXED_WINDOW_LUA_SCRIPT: &str = r#"
local now = redis.call('TIME')
local current_sec = tonumber(now[1])
local cost = tonumber(ARGV[1])
local limit = tonumber(ARGV[2])
local window_sec = tonumber(ARGV[3])

local window_start = math.floor(current_sec / window_sec) * window_sec
local reset_at = window_start + window_sec
local ttl = reset_at - current_sec
if ttl <= 0 then ttl = 1 end

local current_val = tonumber(redis.call('GET', KEYS[1]) or '0')

if current_val + cost <= limit then
    local new_val = redis.call('INCRBY', KEYS[1], cost)
    if current_val == 0 then
        redis.call('EXPIRE', KEYS[1], ttl)
    end
    return { 1, limit - new_val, reset_at, 0 }
else
    return { 0, 0, reset_at, ttl }
end
"#;

/// State for an in-memory fixed window bucket.
#[derive(Debug, Clone, Copy)]
struct BucketState {
    counter: u64,
    window_start: i64,
}

/// Fixed Window Counter rate limiter.
/// Supports both in-memory local evaluation (for unit tests / standalone engine)
/// and distributed atomic Redis evaluation.
#[derive(Default)]
pub struct FixedWindowRateLimiter {
    state: RwLock<HashMap<String, BucketState>>,
}

impl FixedWindowRateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Evaluates a fixed window rate-limit request against a provided Unix timestamp (for deterministic testing).
    pub fn evaluate_at(
        &self,
        key: &str,
        cost: u64,
        policy: &RateLimitPolicy,
        now_secs: i64,
    ) -> Result<Decision, ThrotError> {
        let window_secs = policy.window_seconds as i64;
        if window_secs == 0 {
            return Err(ThrotError::Validation(
                "Window seconds must be greater than 0".to_string(),
            ));
        }

        let limit = policy.request_limit;
        let window_start = (now_secs / window_secs) * window_secs;
        let reset_at = window_start + window_secs;
        let retry_after = (reset_at - now_secs).max(1) as u64;

        let mut buckets = self.state.write().unwrap();
        let bucket = buckets.entry(key.to_string()).or_insert(BucketState {
            counter: 0,
            window_start,
        });

        // If the window has transitioned, reset counter to 0 and advance window_start
        if now_secs >= bucket.window_start + window_secs {
            bucket.counter = 0;
            bucket.window_start = window_start;
        }

        if bucket.counter + cost <= limit {
            bucket.counter += cost;
            let remaining = limit - bucket.counter;
            Ok(Decision::allow(limit, remaining, reset_at))
        } else {
            // Rejection must NOT increment the counter
            Ok(Decision::reject(limit, reset_at, retry_after))
        }
    }
}

#[async_trait]
impl RateLimiter for FixedWindowRateLimiter {
    async fn evaluate(
        &self,
        key: &str,
        cost: u64,
        policy: &RateLimitPolicy,
    ) -> Result<Decision, ThrotError> {
        let now = chrono::Utc::now().timestamp();
        self.evaluate_at(key, cost, policy, now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use throt_core::{AlgorithmType, FailureMode};
    use uuid::Uuid;

    fn make_test_policy(limit: u64, window_seconds: u64) -> RateLimitPolicy {
        RateLimitPolicy {
            id: Uuid::new_v4(),
            application_id: Uuid::new_v4(),
            name: "test-policy".to_string(),
            algorithm: AlgorithmType::FixedWindow,
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
    fn test_fixed_window_basic_flow() {
        let limiter = FixedWindowRateLimiter::new();
        let policy = make_test_policy(10, 60);
        let now = 1000;

        // First request: cost 1
        let d1 = limiter.evaluate_at("user_1", 1, &policy, now).unwrap();
        assert!(d1.allowed);
        assert_eq!(d1.remaining, 9);
        assert_eq!(d1.limit, 10);
        assert_eq!(d1.reset_at, 1020); // (1000/60)*60 = 960 + 60 = 1020

        // Second request: cost 4
        let d2 = limiter.evaluate_at("user_1", 4, &policy, now).unwrap();
        assert!(d2.allowed);
        assert_eq!(d2.remaining, 5);

        // Third request: cost 5 (reaches exactly limit 10)
        let d3 = limiter.evaluate_at("user_1", 5, &policy, now).unwrap();
        assert!(d3.allowed);
        assert_eq!(d3.remaining, 0);

        // Fourth request: cost 1 (exceeds limit)
        let d4 = limiter.evaluate_at("user_1", 1, &policy, now).unwrap();
        assert!(!d4.allowed);
        assert_eq!(d4.remaining, 0);
        assert_eq!(d4.retry_after, Some(20)); // 1020 - 1000 = 20

        // Ensure rejected request did not increase counter: next cost 0 or check
        let d5 = limiter.evaluate_at("user_1", 1, &policy, now).unwrap();
        assert!(!d5.allowed);
        assert_eq!(d5.remaining, 0);
    }

    #[test]
    fn test_fixed_window_transition() {
        let limiter = FixedWindowRateLimiter::new();
        let policy = make_test_policy(5, 60);

        let t1 = 960; // window 960..1020
        let d1 = limiter.evaluate_at("user_1", 5, &policy, t1).unwrap();
        assert!(d1.allowed);
        assert_eq!(d1.remaining, 0);

        // Advance to next window: t2 = 1020 (window 1020..1080)
        let t2 = 1020;
        let d2 = limiter.evaluate_at("user_1", 2, &policy, t2).unwrap();
        assert!(d2.allowed);
        assert_eq!(d2.remaining, 3);
        assert_eq!(d2.reset_at, 1080);
    }

    #[test]
    fn test_identity_isolation() {
        let limiter = FixedWindowRateLimiter::new();
        let policy = make_test_policy(2, 60);
        let now = 100;

        // user_1 exhausts quota
        assert!(
            limiter
                .evaluate_at("user_1", 2, &policy, now)
                .unwrap()
                .allowed
        );
        assert!(
            !limiter
                .evaluate_at("user_1", 1, &policy, now)
                .unwrap()
                .allowed
        );

        // user_2 has independent full quota
        let d_user2 = limiter.evaluate_at("user_2", 1, &policy, now).unwrap();
        assert!(d_user2.allowed);
        assert_eq!(d_user2.remaining, 1);
    }
}
