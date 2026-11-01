use std::sync::Arc;
use throt_core::{AlgorithmType, FailureMode, Identity, RateLimitEvaluator, RateLimitPolicy};
use throt_storage::{RedisRateLimitEvaluator, RedisStorage};
use uuid::Uuid;

async fn get_evaluator() -> Option<Arc<RedisRateLimitEvaluator>> {
    let redis_url =
        std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://localhost:6379".to_string());
    match RedisStorage::connect(&redis_url).await {
        Ok(storage) => Some(Arc::new(RedisRateLimitEvaluator::new(storage))),
        Err(_) => None,
    }
}

#[tokio::test]
async fn test_redis_fixed_window_concurrency() {
    let evaluator = match get_evaluator().await {
        Some(e) => e,
        None => return,
    };

    let app_id = Uuid::new_v4();
    let policy_id = Uuid::new_v4();
    let identity = Identity::new(format!("user:concurrent:{}", Uuid::new_v4())).unwrap();

    let policy = RateLimitPolicy {
        id: policy_id,
        application_id: app_id,
        name: "fixed-window-concurrency".to_string(),
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
    for _ in 0..20 {
        let eval = evaluator.clone();
        let pol = policy.clone();
        let id = identity.clone();

        handles.push(tokio::spawn(async move {
            eval.evaluate_request(app_id, &pol, &id, 1).await
        }));
    }

    let mut allowed = 0;
    let mut rejected = 0;
    for handle in handles {
        let d = handle.await.unwrap().unwrap();
        if d.allowed {
            allowed += 1;
        } else {
            rejected += 1;
            assert!(d.retry_after.unwrap() > 0);
        }
    }

    assert_eq!(allowed, 10);
    assert_eq!(rejected, 10);
}

#[tokio::test]
async fn test_redis_token_bucket_flow() {
    let evaluator = match get_evaluator().await {
        Some(e) => e,
        None => return,
    };

    let app_id = Uuid::new_v4();
    let policy_id = Uuid::new_v4();
    let identity = Identity::new(format!("user:tb:{}", Uuid::new_v4())).unwrap();

    let policy = RateLimitPolicy {
        id: policy_id,
        application_id: app_id,
        name: "token-bucket-redis".to_string(),
        algorithm: AlgorithmType::TokenBucket,
        request_limit: 5,
        window_seconds: 60,
        burst_capacity: Some(5),
        refill_rate: Some(10.0), // 10 tokens/sec
        failure_mode: FailureMode::FailClosed,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    // Consume all 5 burst tokens
    let d1 = evaluator
        .evaluate_request(app_id, &policy, &identity, 5)
        .await
        .unwrap();
    assert!(d1.allowed);
    assert_eq!(d1.remaining, 0);

    // Immediate next token rejected
    let d2 = evaluator
        .evaluate_request(app_id, &policy, &identity, 1)
        .await
        .unwrap();
    assert!(!d2.allowed);

    // Sleep 250ms -> 2.5 tokens replenished at 10/sec
    tokio::time::sleep(tokio::time::Duration::from_millis(250)).await;
    let d3 = evaluator
        .evaluate_request(app_id, &policy, &identity, 2)
        .await
        .unwrap();
    assert!(d3.allowed);
}

#[tokio::test]
async fn test_redis_leaky_bucket_flow() {
    let evaluator = match get_evaluator().await {
        Some(e) => e,
        None => return,
    };

    let app_id = Uuid::new_v4();
    let policy_id = Uuid::new_v4();
    let identity = Identity::new(format!("user:lb:{}", Uuid::new_v4())).unwrap();

    let policy = RateLimitPolicy {
        id: policy_id,
        application_id: app_id,
        name: "leaky-bucket-redis".to_string(),
        algorithm: AlgorithmType::LeakyBucket,
        request_limit: 3,
        window_seconds: 60,
        burst_capacity: Some(3),
        refill_rate: Some(5.0), // 5 units leak per sec
        failure_mode: FailureMode::FailClosed,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    // Fill capacity with 3 units
    let d1 = evaluator
        .evaluate_request(app_id, &policy, &identity, 3)
        .await
        .unwrap();
    assert!(d1.allowed);
    assert_eq!(d1.remaining, 0);

    // 4th unit rejected
    let d2 = evaluator
        .evaluate_request(app_id, &policy, &identity, 1)
        .await
        .unwrap();
    assert!(!d2.allowed);

    // Wait 300ms -> 1.5 units leak out
    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
    let d3 = evaluator
        .evaluate_request(app_id, &policy, &identity, 1)
        .await
        .unwrap();
    assert!(d3.allowed);
}

#[tokio::test]
async fn test_redis_sliding_log_flow() {
    let evaluator = match get_evaluator().await {
        Some(e) => e,
        None => return,
    };

    let app_id = Uuid::new_v4();
    let identity = Identity::new(format!("user:swl:{}", Uuid::new_v4())).unwrap();

    let policy = RateLimitPolicy::builder("swl-redis", AlgorithmType::SlidingWindowLog)
        .application_id(app_id)
        .request_limit(4)
        .window_seconds(60)
        .build()
        .unwrap();

    // Consume 3 out of 4
    let d1 = evaluator
        .evaluate_request(app_id, &policy, &identity, 3)
        .await
        .unwrap();
    assert!(d1.allowed);
    assert_eq!(d1.remaining, 1);

    // Consume remaining 1
    let d2 = evaluator
        .evaluate_request(app_id, &policy, &identity, 1)
        .await
        .unwrap();
    assert!(d2.allowed);
    assert_eq!(d2.remaining, 0);

    // 5th rejected
    let d3 = evaluator
        .evaluate_request(app_id, &policy, &identity, 1)
        .await
        .unwrap();
    assert!(!d3.allowed);
}

#[tokio::test]
async fn test_redis_sliding_counter_flow() {
    let evaluator = match get_evaluator().await {
        Some(e) => e,
        None => return,
    };

    let app_id = Uuid::new_v4();
    let identity = Identity::new(format!("user:swc:{}", Uuid::new_v4())).unwrap();

    let policy = RateLimitPolicy::builder("swc-redis", AlgorithmType::SlidingWindowCounter)
        .application_id(app_id)
        .request_limit(10)
        .window_seconds(60)
        .build()
        .unwrap();

    let d1 = evaluator
        .evaluate_request(app_id, &policy, &identity, 6)
        .await
        .unwrap();
    assert!(d1.allowed);
    assert_eq!(d1.remaining, 4);

    let d2 = evaluator
        .evaluate_request(app_id, &policy, &identity, 4)
        .await
        .unwrap();
    assert!(d2.allowed);
    assert_eq!(d2.remaining, 0);

    let d3 = evaluator
        .evaluate_request(app_id, &policy, &identity, 1)
        .await
        .unwrap();
    assert!(!d3.allowed);
}
