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
    RateLimitPolicy {
        id: Uuid::new_v4(),
        application_id: app_id,
        name: name.to_string(),
        algorithm: AlgorithmType::FixedWindow,
        request_limit: limit,
        window_seconds: 60,
        burst_capacity: None,
        refill_rate: None,
        failure_mode,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    }
}

#[tokio::test]
async fn test_engine_explicit_policy_resolution() {
    let app_id = Uuid::new_v4();
    let store = Arc::new(InMemoryPolicyStore::new());
    let policy = create_policy(app_id, "standard", 100, FailureMode::FailClosed);
    store.insert_policy(policy);

    let engine = RateLimitEngine::new(store, Arc::new(MockEvaluator));

    let req = EvaluationRequest {
        application_id: app_id,
        identity: Identity::new("user:123").unwrap(),
        policy_name: Some("standard".to_string()),
        cost: 1,
    };

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
    let req = EvaluationRequest {
        application_id: app_id,
        identity: vip_user,
        policy_name: None,
        cost: 5,
    };

    let decision = engine.evaluate(req).await.unwrap();
    assert_eq!(decision.limit, 1000);
    assert_eq!(decision.remaining, 995);

    // Regular user should fallback to default policy (limit 10)
    let regular_user = Identity::new("user:regular").unwrap();
    let req2 = EvaluationRequest {
        application_id: app_id,
        identity: regular_user,
        policy_name: None,
        cost: 1,
    };

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

    let req = EvaluationRequest {
        application_id: app_id,
        identity: Identity::new("user:fail_open_test").unwrap(),
        policy_name: Some("resilient".to_string()),
        cost: 1,
    };

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

    let req = EvaluationRequest {
        application_id: app_id,
        identity: Identity::new("user:fail_closed_test").unwrap(),
        policy_name: Some("strict".to_string()),
        cost: 1,
    };

    let result = engine.evaluate(req).await;
    assert!(result.is_err());
}
