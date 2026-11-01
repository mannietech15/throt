use std::sync::Arc;
use throt_core::{AlgorithmType, FailureMode, Identity, RateLimitEvaluator, RateLimitPolicy};
use throt_storage::{RedisRateLimitEvaluator, RedisStorage};
use uuid::Uuid;

#[tokio::test]
async fn test_redis_atomic_concurrency_protection() {
    let redis_url =
        std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string());
    let redis = match RedisStorage::connect(&redis_url).await {
        Ok(storage) => storage,
        Err(_) => {
            eprintln!(
                "Skipping Redis integration test: Redis not reachable on {}",
                redis_url
            );
            return;
        }
    };

    let evaluator = Arc::new(RedisRateLimitEvaluator::new(redis));
    let app_id = Uuid::new_v4();
    let policy_id = Uuid::new_v4();
    let identity = Identity::new(format!("user:concurrent:{}", Uuid::new_v4())).unwrap();

    let policy = RateLimitPolicy {
        id: policy_id,
        application_id: app_id,
        name: "concurrency-test".to_string(),
        algorithm: AlgorithmType::FixedWindow,
        request_limit: 10,
        window_seconds: 60,
        burst_capacity: None,
        refill_rate: None,
        failure_mode: FailureMode::FailClosed,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    let mut handles = Vec::new();

    // Spawn 20 concurrent tasks all attempting to consume 1 token simultaneously
    for _ in 0..20 {
        let eval = evaluator.clone();
        let pol = policy.clone();
        let id = identity.clone();

        handles.push(tokio::spawn(async move {
            eval.evaluate_request(app_id, &pol, &id, 1).await
        }));
    }

    let mut allowed_count = 0;
    let mut rejected_count = 0;

    for handle in handles {
        let decision = handle
            .await
            .unwrap()
            .expect("Redis evaluation must succeed");
        if decision.allowed {
            allowed_count += 1;
        } else {
            rejected_count += 1;
            assert!(decision.retry_after.unwrap() > 0);
        }
    }

    // Exactly 10 allowed and exactly 10 rejected
    assert_eq!(
        allowed_count, 10,
        "Concurrency race condition detected: more than configured limit allowed!"
    );
    assert_eq!(
        rejected_count, 10,
        "Rejections count mismatch: expected 10 rejected requests"
    );
}
