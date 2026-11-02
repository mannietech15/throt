use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use serde_json::json;
use std::sync::Arc;
use throt_algorithms::InMemoryRateLimitEvaluator;
use throt_api::{create_router, AppState};
use throt_core::{
    AlgorithmType, EvaluationRequest, FailureMode, InMemoryPolicyStore, RateLimitEngine,
    RateLimitPolicy,
};
use tower::ServiceExt;

#[tokio::test]
async fn test_parity_standalone_daemon_vs_in_process_engine() {
    let app_id = throt_api::middleware::DEV_SANDBOX_APP_ID;

    // Policies to test across all 5 algorithms
    let algorithms = vec![
        ("fixed_window_parity", AlgorithmType::FixedWindow, 10, 60),
        (
            "sliding_log_parity",
            AlgorithmType::SlidingWindowLog,
            10,
            60,
        ),
        (
            "sliding_counter_parity",
            AlgorithmType::SlidingWindowCounter,
            10,
            60,
        ),
        ("token_bucket_parity", AlgorithmType::TokenBucket, 10, 60),
        ("leaky_bucket_parity", AlgorithmType::LeakyBucket, 10, 60),
    ];

    for (name, algo, limit, window_secs) in algorithms {
        let policy_store = Arc::new(InMemoryPolicyStore::new());
        let mut builder = RateLimitPolicy::builder(name, algo)
            .application_id(app_id)
            .request_limit(limit)
            .window_seconds(window_secs)
            .failure_mode(FailureMode::FailClosed);

        if algo == AlgorithmType::TokenBucket || algo == AlgorithmType::LeakyBucket {
            builder = builder.burst_capacity(15).refill_rate(2.0);
        }

        let policy = builder.build().expect("Valid policy");
        policy_store.insert_policy(policy);

        // In-process engine
        let in_proc_evaluator = Arc::new(InMemoryRateLimitEvaluator::new());
        let in_proc_engine = Arc::new(RateLimitEngine::new(
            policy_store.clone(),
            in_proc_evaluator,
        ));

        // Standalone HTTP daemon engine
        let daemon_evaluator = Arc::new(InMemoryRateLimitEvaluator::new());
        let daemon_engine = Arc::new(RateLimitEngine::new(policy_store.clone(), daemon_evaluator));

        let app_state = Arc::new(AppState {
            redis: None,
            postgres: None,
        });
        let daemon_app = create_router(app_state, daemon_engine, None, None);

        // Test sequences:
        // Request 1: cost 4 (allowed -> remaining: 6)
        // Request 2: cost 4 (allowed -> remaining: 2)
        // Request 3: cost 3 (rejected -> remaining: 0)
        // Request 4: cost 10 (rejected -> remaining: 0)
        let request_costs = vec![4, 4, 3, 10];

        for (step, cost) in request_costs.into_iter().enumerate() {
            let identity = format!("client:parity_{name}");

            // 1. Direct in-process engine evaluation
            let in_proc_req = EvaluationRequest::for_application(
                app_id,
                identity.clone(),
                Some(name.to_string()),
                cost,
            )
            .unwrap();
            let in_proc_decision = in_proc_engine.evaluate(in_proc_req).await.unwrap();

            // 2. Standalone HTTP daemon evaluation via Axum REST endpoint
            let daemon_body = json!({
                "identity": identity,
                "policy": name,
                "cost": cost
            });

            let resp = daemon_app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/v1/limits/evaluate")
                        .header("Content-Type", "application/json")
                        .header("X-API-Key", "throt_live_testkey123456789")
                        .body(Body::from(serde_json::to_vec(&daemon_body).unwrap()))
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(resp.status(), StatusCode::OK);
            let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
            let daemon_val: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

            // Assert absolute parity between In-Process and Standalone Daemon
            assert_eq!(
                daemon_val["allowed"].as_bool().unwrap(),
                in_proc_decision.allowed,
                "Parity mismatch on 'allowed' for {name} step {step}"
            );

            assert_eq!(
                daemon_val["limit"].as_u64().unwrap(),
                in_proc_decision.limit,
                "Parity mismatch on 'limit' for {name} step {step}"
            );

            assert_eq!(
                daemon_val["remaining"].as_u64().unwrap(),
                in_proc_decision.remaining,
                "Parity mismatch on 'remaining' for {name} step {step}"
            );

            let daemon_retry = daemon_val.get("retry_after").and_then(|v| v.as_u64());
            assert_eq!(
                daemon_retry, in_proc_decision.retry_after,
                "Parity mismatch on 'retry_after' for {name} step {step}"
            );
        }
    }
}
