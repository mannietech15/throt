use std::sync::Arc;
use throt_algorithms::InMemoryRateLimitEvaluator;
use throt_core::{
    AlgorithmType, EvaluationRequest, FailureMode, InMemoryPolicyStore, RateLimitEngine,
    RateLimitEvaluator, RateLimitPolicy,
};
use uuid::Uuid;

fn make_policy(algo: AlgorithmType, limit: u64, window_secs: u64) -> RateLimitPolicy {
    RateLimitPolicy::builder(format!("test-{algo:?}"), algo)
        .application_id(Uuid::new_v4())
        .request_limit(limit)
        .window_seconds(window_secs)
        .burst_capacity(limit)
        .refill_rate(limit as f64 / window_secs as f64)
        .failure_mode(FailureMode::FailClosed)
        .build()
        .expect("Valid policy")
}

#[tokio::test]
async fn test_in_memory_fixed_window_boundary_and_rejection() {
    let evaluator = InMemoryRateLimitEvaluator::new();
    let policy = make_policy(AlgorithmType::FixedWindow, 10, 60);
    let app_id = policy.application_id;
    let identity = throt_core::Identity::new("client:fw_boundary").unwrap();

    // 1. Cost equal to exact limit should allow and leave remaining = 0
    let dec1 = evaluator
        .evaluate_request(app_id, &policy, &identity, 10)
        .await
        .unwrap();
    assert!(dec1.allowed);
    assert_eq!(dec1.remaining, 0);

    // 2. Subsequent cost 1 must be rejected
    let dec2 = evaluator
        .evaluate_request(app_id, &policy, &identity, 1)
        .await
        .unwrap();
    assert!(!dec2.allowed);
    assert_eq!(dec2.remaining, 0);

    // 3. Rejection must NOT increment the counter
    let dec3 = evaluator
        .evaluate_request(app_id, &policy, &identity, 1)
        .await
        .unwrap();
    assert!(!dec3.allowed);
    assert_eq!(dec3.remaining, 0);
}

#[tokio::test]
async fn test_in_memory_sliding_window_log() {
    let evaluator = InMemoryRateLimitEvaluator::new();
    let policy = make_policy(AlgorithmType::SlidingWindowLog, 5, 60);
    let app_id = policy.application_id;
    let identity = throt_core::Identity::new("client:swl").unwrap();

    // Consume 3
    let d1 = evaluator
        .evaluate_request(app_id, &policy, &identity, 3)
        .await
        .unwrap();
    assert!(d1.allowed);
    assert_eq!(d1.remaining, 2);

    // Consume remaining 2
    let d2 = evaluator
        .evaluate_request(app_id, &policy, &identity, 2)
        .await
        .unwrap();
    assert!(d2.allowed);
    assert_eq!(d2.remaining, 0);

    // Exceed
    let d3 = evaluator
        .evaluate_request(app_id, &policy, &identity, 1)
        .await
        .unwrap();
    assert!(!d3.allowed);
    assert!(d3.retry_after.is_some());
}

#[tokio::test]
async fn test_in_memory_sliding_window_counter() {
    let evaluator = InMemoryRateLimitEvaluator::new();
    let policy = make_policy(AlgorithmType::SlidingWindowCounter, 20, 60);
    let app_id = policy.application_id;
    let identity = throt_core::Identity::new("client:swc").unwrap();

    let d1 = evaluator
        .evaluate_request(app_id, &policy, &identity, 10)
        .await
        .unwrap();
    assert!(d1.allowed);
    assert_eq!(d1.remaining, 10);

    let d2 = evaluator
        .evaluate_request(app_id, &policy, &identity, 10)
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

#[tokio::test]
async fn test_in_memory_token_bucket_burst_and_refill() {
    let evaluator = InMemoryRateLimitEvaluator::new();
    let policy = RateLimitPolicy::builder("tb-burst", AlgorithmType::TokenBucket)
        .application_id(Uuid::new_v4())
        .request_limit(50)
        .window_seconds(60)
        .burst_capacity(100)
        .refill_rate(10.0) // 10 tokens / sec
        .build()
        .unwrap();
    let app_id = policy.application_id;
    let identity = throt_core::Identity::new("client:tb").unwrap();

    // Consume full burst capacity of 100
    let d1 = evaluator
        .evaluate_request(app_id, &policy, &identity, 100)
        .await
        .unwrap();
    assert!(d1.allowed);
    assert_eq!(d1.remaining, 0);

    // Immediately reject next token
    let d2 = evaluator
        .evaluate_request(app_id, &policy, &identity, 1)
        .await
        .unwrap();
    assert!(!d2.allowed);
}

#[tokio::test]
async fn test_in_memory_leaky_bucket_policing() {
    let evaluator = InMemoryRateLimitEvaluator::new();
    let policy = RateLimitPolicy::builder("lb-police", AlgorithmType::LeakyBucket)
        .application_id(Uuid::new_v4())
        .request_limit(10)
        .window_seconds(10)
        .burst_capacity(10)
        .refill_rate(2.0) // 2 leaks per second
        .build()
        .unwrap();
    let app_id = policy.application_id;
    let identity = throt_core::Identity::new("client:lb").unwrap();

    // Fill the bucket to capacity
    let d1 = evaluator
        .evaluate_request(app_id, &policy, &identity, 10)
        .await
        .unwrap();
    assert!(d1.allowed);
    assert_eq!(d1.remaining, 0);

    // Next request overflows
    let d2 = evaluator
        .evaluate_request(app_id, &policy, &identity, 1)
        .await
        .unwrap();
    assert!(!d2.allowed);
}

#[tokio::test]
async fn test_rate_limit_engine_with_in_memory_evaluator() {
    let policy_store = Arc::new(InMemoryPolicyStore::new());
    let app_id = Uuid::new_v4();

    let fw_policy = RateLimitPolicy::builder("web-standard", AlgorithmType::FixedWindow)
        .application_id(app_id)
        .request_limit(50)
        .window_seconds(60)
        .build()
        .unwrap();
    policy_store.insert_policy(fw_policy.clone());
    policy_store.set_default_policy(app_id, fw_policy);

    let evaluator = Arc::new(InMemoryRateLimitEvaluator::new());
    let engine = RateLimitEngine::new(policy_store, evaluator);

    let req =
        EvaluationRequest::for_application(app_id, "user:integration", Some("web-standard"), 5)
            .unwrap();

    let decision = engine.evaluate(req).await.unwrap();
    assert!(decision.allowed);
    assert_eq!(decision.remaining, 45);
}

#[tokio::test]
async fn test_in_memory_simultaneous_concurrent_evaluations() {
    let evaluator = Arc::new(InMemoryRateLimitEvaluator::new());
    let policy = make_policy(AlgorithmType::FixedWindow, 1000, 60);
    let app_id = policy.application_id;

    // Launch 100 concurrent async tasks evaluating across 10 distinct client identities
    let mut handles = Vec::new();
    for i in 0..100 {
        let eval_clone = Arc::clone(&evaluator);
        let pol_clone = policy.clone();
        handles.push(tokio::spawn(async move {
            let client_key = format!("concurrent_user_{}", i % 10);
            let ident = throt_core::Identity::new(client_key).unwrap();
            eval_clone
                .evaluate_request(app_id, &pol_clone, &ident, 1)
                .await
        }));
    }

    for handle in handles {
        let res = handle
            .await
            .expect("Task join failed")
            .expect("Evaluation succeeded");
        assert!(res.allowed);
    }
}
