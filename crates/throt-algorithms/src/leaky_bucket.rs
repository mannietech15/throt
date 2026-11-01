use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::RwLock;
use throt_core::{Decision, RateLimitPolicy, ThrotError};

use crate::RateLimiter;

/// Redis Lua script for distributed atomic Leaky Bucket (Policing Meter) evaluation.
pub const LEAKY_BUCKET_LUA_SCRIPT: &str = r#"
local now = redis.call('TIME')
local current_sec = tonumber(now[1])
local current_usec = tonumber(now[2])
local now_f = current_sec + (current_usec / 1000000.0)

local cost = tonumber(ARGV[1])
local capacity = tonumber(ARGV[2])
local leak_rate = tonumber(ARGV[3]) -- tokens/requests leaked per second

local data = redis.call('HMGET', KEYS[1], 'water', 'last_leak')
local water = tonumber(data[1])
local last_leak = tonumber(data[2])

if not water or not last_leak then
    water = 0.0
    last_leak = now_f
else
    local elapsed = math.max(0.0, now_f - last_leak)
    water = math.max(0.0, water - (elapsed * leak_rate))
    last_leak = now_f
end

local reset_at = math.ceil(now_f + (water / leak_rate))

if water + cost <= capacity then
    water = water + cost
    redis.call('HMSET', KEYS[1], 'water', water, 'last_leak', last_leak)
    local ttl = math.max(1, math.ceil(capacity / leak_rate) * 2)
    redis.call('EXPIRE', KEYS[1], ttl)

    local remaining = math.floor(capacity - water)
    return { 1, remaining, reset_at, 0 }
else
    local overflow = (water + cost) - capacity
    local retry_after = math.max(1, math.ceil(overflow / leak_rate))
    redis.call('HMSET', KEYS[1], 'water', water, 'last_leak', last_leak)
    return { 0, 0, reset_at, retry_after }
end
"#;

#[derive(Debug, Clone, Copy)]
struct LeakyBucketState {
    water: f64,
    last_leak_secs: f64,
}

/// In-memory Leaky Bucket (Policing Meter) rate limiter.
#[derive(Default)]
pub struct LeakyBucketRateLimiter {
    state: RwLock<HashMap<String, LeakyBucketState>>,
}

impl LeakyBucketRateLimiter {
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
        let leak_rate = policy
            .refill_rate
            .unwrap_or(policy.request_limit as f64 / policy.window_seconds as f64);

        if leak_rate <= 0.0 {
            return Err(ThrotError::Validation(
                "Leak rate must be greater than 0".to_string(),
            ));
        }

        let mut buckets = self.state.write().unwrap();
        let bucket = buckets.entry(key.to_string()).or_insert(LeakyBucketState {
            water: 0.0,
            last_leak_secs: now_secs,
        });

        // Leak water based on elapsed time
        let elapsed = (now_secs - bucket.last_leak_secs).max(0.0);
        bucket.water = (bucket.water - elapsed * leak_rate).max(0.0);
        bucket.last_leak_secs = now_secs;

        let reset_at = (now_secs + (bucket.water / leak_rate)).ceil() as i64;

        if bucket.water + cost as f64 <= capacity {
            bucket.water += cost as f64;
            let remaining = (capacity - bucket.water).floor() as u64;
            Ok(Decision::allow(capacity as u64, remaining, reset_at))
        } else {
            let overflow = (bucket.water + cost as f64) - capacity;
            let retry_after = (overflow / leak_rate).ceil().max(1.0) as u64;
            Ok(Decision::reject(capacity as u64, reset_at, retry_after))
        }
    }
}

#[async_trait]
impl RateLimiter for LeakyBucketRateLimiter {
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use throt_core::{AlgorithmType, FailureMode};
    use uuid::Uuid;

    fn make_policy(capacity: u64, leak_rate: f64) -> RateLimitPolicy {
        RateLimitPolicy {
            id: Uuid::new_v4(),
            application_id: Uuid::new_v4(),
            name: "leaky-bucket-test".to_string(),
            algorithm: AlgorithmType::LeakyBucket,
            request_limit: capacity,
            window_seconds: 60,
            burst_capacity: Some(capacity),
            refill_rate: Some(leak_rate),
            failure_mode: FailureMode::FailClosed,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn test_leaky_bucket_policing_and_leak() {
        let limiter = LeakyBucketRateLimiter::new();
        // Capacity: 5 units, leak rate: 1 unit/sec
        let policy = make_policy(5, 1.0);

        // Fill bucket with 5 units
        let d1 = limiter.evaluate_at("ip:1", 5, &policy, 0.0).unwrap();
        assert!(d1.allowed);
        assert_eq!(d1.remaining, 0);

        // 6th unit exceeds capacity at t = 0s
        let d2 = limiter.evaluate_at("ip:1", 1, &policy, 0.0).unwrap();
        assert!(!d2.allowed);
        assert_eq!(d2.retry_after, Some(1)); // 1 unit needs to leak at 1/sec

        // t = 2.0s: 2 units leaked, water level drops to 3
        let d3 = limiter.evaluate_at("ip:1", 2, &policy, 2.0).unwrap();
        assert!(d3.allowed);
        assert_eq!(d3.remaining, 0);
    }
}
