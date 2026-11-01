use async_trait::async_trait;
use std::sync::Arc;
use throt_core::{
    AlgorithmType, Decision, EvaluationRequest, FailureMode, Identity, InMemoryPolicyStore,
    RateLimitEngine, RateLimitEvaluator, RateLimitPolicy, ThrotError,
};
use uuid::Uuid;

struct MockEvaluator;

#[async_trait]
impl RateLimitEvaluator for MockEvaluator {
    async fn evaluate_request(
        &self,
        _app_id: Uuid,
        policy: &RateLimitPolicy,
        _identity: &Identity,
        cost: u64,
    ) -> Result<Decision, ThrotError> {
        if cost > policy.request_limit {
            Ok(Decision::reject(policy.request_limit, 1000, 10))
        } else {
            Ok(Decision::allow(
                policy.request_limit,
                policy.request_limit - cost,
                1000,
            ))
        }
    }
}

struct FailingEvaluator;

#[async_trait]
impl RateLimitEvaluator for FailingEvaluator {
    async fn evaluate_request(
        &self,
        _app_id: Uuid,
        _policy: &RateLimitPolicy,
        _identity: &Identity,
        _cost: u64,
    ) -> Result<Decision, ThrotError> {
        Err(ThrotError::Storage("Storage disconnected".to_string()))
    }
}

fn create_policy(
    app_id: Uuid,
    name: &str,
    limit: u64,
    failure_mode: FailureMode,
) -> RateLimitPolicy {
    RateLimitPolicy::builder(name, AlgorithmType::FixedWindow)
        .application_id(app_id)
        .request_limit(limit)
        .window_seconds(60)
        .failure_mode(failure_mode)
        .build()
        .expect("Valid policy")
}

#[tokio::test]
async fn test_engine_explicit_policy_resolution() {
    let app_id = Uuid::new_v4();
    let store = Arc::new(InMemoryPolicyStore::new());
    let policy = create_policy(app_id, "standard", 100, FailureMode::FailClosed);
    store.insert_policy(policy);

    let engine = RateLimitEngine::new(store, Arc::new(MockEvaluator));

    let req = EvaluationRequest::for_application(app_id, "user:123", Some("standard"), 1)
        .expect("Valid request");

    let decision = engine
        .evaluate(req)
        .await
        .expect("Evaluation should succeed");
    assert!(decision.allowed);
    assert_eq!(decision.remaining, 99);
}

#[tokio::test]
async fn test_engine_identity_assignment_precedence() {
    let app_id = Uuid::new_v4();
    let store = Arc::new(InMemoryPolicyStore::new());

    let default_policy = create_policy(app_id, "default", 10, FailureMode::FailClosed);
    let vip_policy = create_policy(app_id, "vip", 1000, FailureMode::FailClosed);

    store.set_default_policy(app_id, default_policy);
    let vip_user = Identity::new("user:vip_42").unwrap();
    store.assign_identity_policy(app_id, &vip_user, vip_policy);

    let engine = RateLimitEngine::new(store, Arc::new(MockEvaluator));

    // Request without explicit policy name: should match identity-specific vip policy (limit 1000)
    let req = EvaluationRequest::for_application(app_id, vip_user, None::<String>, 5)
        .expect("Valid request");

    let decision = engine.evaluate(req).await.unwrap();
    assert_eq!(decision.limit, 1000);
    assert_eq!(decision.remaining, 995);

    // Regular user should fallback to default policy (limit 10)
    let regular_user = Identity::new("user:regular").unwrap();
    let req2 = EvaluationRequest::for_application(app_id, regular_user, None::<String>, 1)
        .expect("Valid request");

    let decision2 = engine.evaluate(req2).await.unwrap();
    assert_eq!(decision2.limit, 10);
    assert_eq!(decision2.remaining, 9);
}

#[tokio::test]
async fn test_engine_fail_open_behavior() {
    let app_id = Uuid::new_v4();
    let store = Arc::new(InMemoryPolicyStore::new());
    let policy = create_policy(app_id, "resilient", 50, FailureMode::FailOpen);
    store.insert_policy(policy);

    let engine = RateLimitEngine::new(store, Arc::new(FailingEvaluator));

    let req =
        EvaluationRequest::for_application(app_id, "user:fail_open_test", Some("resilient"), 1)
            .expect("Valid request");

    let decision = engine.evaluate(req).await.expect("Should fail open");
    assert!(decision.allowed);
    assert_eq!(decision.limit, 50);
}

#[tokio::test]
async fn test_engine_fail_closed_behavior() {
    let app_id = Uuid::new_v4();
    let store = Arc::new(InMemoryPolicyStore::new());
    let policy = create_policy(app_id, "strict", 50, FailureMode::FailClosed);
    store.insert_policy(policy);

    let engine = RateLimitEngine::new(store, Arc::new(FailingEvaluator));

    let req =
        EvaluationRequest::for_application(app_id, "user:fail_closed_test", Some("strict"), 1)
            .expect("Valid request");

    let result = engine.evaluate(req).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_engine_rejects_missing_or_nil_application_id() {
    let store = Arc::new(InMemoryPolicyStore::new());
    let engine = RateLimitEngine::new(store, Arc::new(MockEvaluator));

    // 1. Missing application_id
    let unassigned_req = EvaluationRequest::new("user:unassigned", None::<String>, 1).unwrap();
    let res = engine.evaluate(unassigned_req).await;
    assert!(res.is_err());
    let err_str = res.unwrap_err().to_string();
    assert!(err_str.contains("Application ID is required"));

    // 2. Nil application_id manually placed on struct
    let nil_req = EvaluationRequest {
        application_id: Some(Uuid::nil()),
        identity: Identity::new("user:nil_test").unwrap(),
        policy_name: None,
        cost: 1,
    };
    let res_nil = engine.evaluate(nil_req).await;
    assert!(res_nil.is_err());
    let err_str_nil = res_nil.unwrap_err().to_string();
    assert!(err_str_nil.contains("cannot be nil"));
}

#[tokio::test]
async fn test_engine_unknown_policy_error() {
    let app_id = Uuid::new_v4();
    let store = Arc::new(InMemoryPolicyStore::new());
    let engine = RateLimitEngine::new(store, Arc::new(MockEvaluator));

    let req =
        EvaluationRequest::for_application(app_id, "user:anon", Some("non_existent"), 1).unwrap();
    let res = engine.evaluate(req).await;
    assert!(matches!(res, Err(ThrotError::PolicyNotFound(_))));
}

#[tokio::test]
async fn test_engine_concurrent_evaluation() {
    let app_id = Uuid::new_v4();
    let store = Arc::new(InMemoryPolicyStore::new());
    let policy = create_policy(app_id, "concurrent", 10_000, FailureMode::FailClosed);
    store.insert_policy(policy);

    let engine = Arc::new(RateLimitEngine::new(store, Arc::new(MockEvaluator)));

    let mut handles = Vec::new();
    for i in 0..50 {
        let engine_clone = Arc::clone(&engine);
        handles.push(tokio::spawn(async move {
            let req = EvaluationRequest::for_application(
                app_id,
                format!("user:concurrent_{i}"),
                Some("concurrent"),
                1,
            )
            .expect("Valid request");
            engine_clone.evaluate(req).await
        }));
    }

    for handle in handles {
        let result = handle.await.expect("Task join failed");
        let decision = result.expect("Evaluation should succeed");
        assert!(decision.allowed);
    }
}
