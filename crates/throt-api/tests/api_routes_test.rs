use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use serde_json::json;
use std::sync::Arc;
use throt_api::{create_router, AppState};
use throt_core::{
    AlgorithmType, FailureMode, InMemoryPolicyStore, RateLimitEngine, RateLimitPolicy,
};
use tower::ServiceExt;
use uuid::Uuid;

#[tokio::test]
async fn test_auth_middleware_rejects_missing_key() {
    let app_state = Arc::new(AppState {
        redis: None,
        postgres: None,
    });
    let policy_store = Arc::new(InMemoryPolicyStore::new());
    let evaluator = Arc::new(throt_algorithms::FixedWindowRateLimiter::new());
    let engine = Arc::new(RateLimitEngine::new(policy_store, evaluator));

    let app = create_router(app_state, engine, None);

    let req_body = json!({
        "identity": "user:123",
        "policy": "standard",
        "cost": 1
    });

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/limits/evaluate")
                .header("Content-Type", "application/json")
                .body(Body::from(serde_json::to_vec(&req_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_evaluate_endpoint_end_to_end() {
    let app_state = Arc::new(AppState {
        redis: None,
        postgres: None,
    });
    let policy_store = Arc::new(InMemoryPolicyStore::new());
    let app_id = Uuid::nil();

    let policy = RateLimitPolicy {
        id: Uuid::new_v4(),
        application_id: app_id,
        name: "standard".to_string(),
        algorithm: AlgorithmType::FixedWindow,
        request_limit: 10,
        window_seconds: 60,
        burst_capacity: None,
        refill_rate: None,
        failure_mode: FailureMode::FailClosed,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };
    policy_store.insert_policy(policy);

    let evaluator = Arc::new(throt_algorithms::FixedWindowRateLimiter::new());
    let engine = Arc::new(RateLimitEngine::new(policy_store, evaluator));

    let app = create_router(app_state, engine, None);

    let req_body = json!({
        "identity": "user:test_client",
        "policy": "standard",
        "cost": 2
    });

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/limits/evaluate")
                .header("Content-Type", "application/json")
                .header("X-API-Key", "throt_live_testkey123456789")
                .body(Body::from(serde_json::to_vec(&req_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json_val: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(json_val["allowed"], true);
    assert_eq!(json_val["limit"], 10);
    assert_eq!(json_val["remaining"], 8);
}

#[tokio::test]
async fn test_evaluate_endpoint_policy_not_found() {
    let app_state = Arc::new(AppState {
        redis: None,
        postgres: None,
    });
    let policy_store = Arc::new(InMemoryPolicyStore::new());
    let evaluator = Arc::new(throt_algorithms::FixedWindowRateLimiter::new());
    let engine = Arc::new(RateLimitEngine::new(policy_store, evaluator));

    let app = create_router(app_state, engine, None);

    let req_body = json!({
        "identity": "user:test_client",
        "policy": "non_existent_policy",
        "cost": 1
    });

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/limits/evaluate")
                .header("Content-Type", "application/json")
                .header("X-API-Key", "throt_live_testkey123456789")
                .body(Body::from(serde_json::to_vec(&req_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_evaluate_endpoint_invalid_identity() {
    let app_state = Arc::new(AppState {
        redis: None,
        postgres: None,
    });
    let policy_store = Arc::new(InMemoryPolicyStore::new());
    let evaluator = Arc::new(throt_algorithms::FixedWindowRateLimiter::new());
    let engine = Arc::new(RateLimitEngine::new(policy_store, evaluator));

    let app = create_router(app_state, engine, None);

    let req_body = json!({
        "identity": "   ",
        "policy": "standard",
        "cost": 1
    });

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/limits/evaluate")
                .header("Content-Type", "application/json")
                .header("X-API-Key", "throt_live_testkey123456789")
                .body(Body::from(serde_json::to_vec(&req_body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
