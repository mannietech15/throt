use throt_core::{AlgorithmType, Decision, FailureMode, Identity, RateLimitPolicy};
use uuid::Uuid;

#[test]
fn test_identity_validation() {
    // Valid identity
    let id = Identity::new("user:12345").expect("Should be valid");
    assert_eq!(id.as_str(), "user:12345");
    assert_eq!(id.to_string(), "user:12345");

    // Whitespace trimming
    let id_trimmed = Identity::new("  app:api:99  ").expect("Should trim whitespace");
    assert_eq!(id_trimmed.as_str(), "app:api:99");

    // Empty identity should fail
    assert!(Identity::new("").is_err());
    assert!(Identity::new("   ").is_err());

    // Exceeding 256 characters should fail
    let long_str = "a".repeat(257);
    assert!(Identity::new(long_str).is_err());
}

#[test]
fn test_decision_constructors() {
    let allowed = Decision::allow(100, 75, 1790000000);
    assert!(allowed.allowed);
    assert_eq!(allowed.limit, 100);
    assert_eq!(allowed.remaining, 75);
    assert_eq!(allowed.reset_at, 1790000000);
    assert_eq!(allowed.retry_after, None);

    let rejected = Decision::reject(100, 1790000000, 15);
    assert!(!rejected.allowed);
    assert_eq!(rejected.limit, 100);
    assert_eq!(rejected.remaining, 0);
    assert_eq!(rejected.reset_at, 1790000000);
    assert_eq!(rejected.retry_after, Some(15));
}

#[test]
fn test_policy_serialization() {
    let policy = RateLimitPolicy {
        id: Uuid::new_v4(),
        application_id: Uuid::new_v4(),
        name: "test-policy".to_string(),
        algorithm: AlgorithmType::TokenBucket,
        request_limit: 100,
        window_seconds: 60,
        burst_capacity: Some(150),
        refill_rate: Some(10.0),
        failure_mode: FailureMode::FailClosed,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    let json = serde_json::to_string(&policy).expect("Serialization failed");
    let deserialized: RateLimitPolicy =
        serde_json::from_str(&json).expect("Deserialization failed");

    assert_eq!(deserialized.name, "test-policy");
    assert_eq!(deserialized.algorithm, AlgorithmType::TokenBucket);
    assert_eq!(deserialized.failure_mode, FailureMode::FailClosed);
}
