use throt_core::{
    AlgorithmType, Decision, EvaluationRequest, FailureMode, Identity, RateLimitPolicy,
    RateLimitPolicyBuilder,
};
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

    // Boundary: exactly 256 characters is valid
    let max_len_str = "a".repeat(256);
    assert!(Identity::new(&max_len_str).is_ok());

    // Boundary: exceeding 256 characters should fail
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
fn test_policy_builder_sensible_defaults() {
    let app_id = Uuid::new_v4();
    let policy = RateLimitPolicy::builder("standard", AlgorithmType::FixedWindow)
        .application_id(app_id)
        .request_limit(100)
        .build()
        .expect("Builder with defaults should succeed");

    assert_eq!(policy.name, "standard");
    assert_eq!(policy.application_id, app_id);
    assert_eq!(policy.request_limit, 100);
    assert_eq!(policy.window_seconds, 60); // Sensible default: 60s
    assert_eq!(policy.failure_mode, FailureMode::FailClosed); // Sensible default: FailClosed
    assert!(!policy.id.is_nil());
    assert_eq!(policy.burst_capacity, None);
    assert_eq!(policy.refill_rate, None);
}

#[test]
fn test_policy_builder_rejects_nil_application_id() {
    let res = RateLimitPolicy::builder("test", AlgorithmType::TokenBucket)
        .application_id(Uuid::nil())
        .request_limit(50)
        .build();

    assert!(res.is_err());
    let err_msg = res.unwrap_err().to_string();
    assert!(err_msg.contains("cannot be nil"));
}

#[test]
fn test_policy_builder_rejects_missing_application_id() {
    let res = RateLimitPolicy::builder("test", AlgorithmType::TokenBucket)
        .request_limit(50)
        .build();

    assert!(res.is_err());
    let err_msg = res.unwrap_err().to_string();
    assert!(err_msg.contains("Application ID is required"));
}

#[test]
fn test_policy_builder_rejects_empty_name() {
    let app_id = Uuid::new_v4();
    let res_empty = RateLimitPolicy::builder("", AlgorithmType::FixedWindow)
        .application_id(app_id)
        .request_limit(100)
        .build();
    assert!(res_empty.is_err());

    let res_spaces = RateLimitPolicy::builder("   ", AlgorithmType::FixedWindow)
        .application_id(app_id)
        .request_limit(100)
        .build();
    assert!(res_spaces.is_err());
}

#[test]
fn test_policy_builder_rejects_zero_limit_and_window() {
    let app_id = Uuid::new_v4();

    // 0 request limit
    let res_zero_limit = RateLimitPolicy::builder("test", AlgorithmType::FixedWindow)
        .application_id(app_id)
        .request_limit(0)
        .build();
    assert!(res_zero_limit.is_err());

    // 0 window seconds
    let res_zero_window = RateLimitPolicy::builder("test", AlgorithmType::FixedWindow)
        .application_id(app_id)
        .request_limit(100)
        .window_seconds(0)
        .build();
    assert!(res_zero_window.is_err());
}

#[test]
fn test_policy_builder_boundary_burst_and_refill() {
    let app_id = Uuid::new_v4();

    // Burst < request limit is invalid
    let res_low_burst = RateLimitPolicy::builder("token", AlgorithmType::TokenBucket)
        .application_id(app_id)
        .request_limit(100)
        .burst_capacity(99)
        .build();
    assert!(res_low_burst.is_err());

    // Burst == request limit is valid boundary condition
    let res_eq_burst = RateLimitPolicy::builder("token", AlgorithmType::TokenBucket)
        .application_id(app_id)
        .request_limit(100)
        .burst_capacity(100)
        .build();
    assert!(res_eq_burst.is_ok());

    // Refill <= 0.0 is invalid
    let res_zero_refill = RateLimitPolicy::builder("token", AlgorithmType::TokenBucket)
        .application_id(app_id)
        .request_limit(100)
        .refill_rate(0.0)
        .build();
    assert!(res_zero_refill.is_err());

    let res_neg_refill = RateLimitPolicy::builder("token", AlgorithmType::TokenBucket)
        .application_id(app_id)
        .request_limit(100)
        .refill_rate(-5.0)
        .build();
    assert!(res_neg_refill.is_err());

    // Smallest positive refill rate is valid
    let res_min_refill = RateLimitPolicy::builder("token", AlgorithmType::TokenBucket)
        .application_id(app_id)
        .request_limit(100)
        .refill_rate(0.0001)
        .build();
    assert!(res_min_refill.is_ok());
}

#[test]
fn test_policy_builder_application_str_parsing() {
    let valid_uuid = Uuid::new_v4();
    let builder = RateLimitPolicyBuilder::new("web-tier", AlgorithmType::FixedWindow)
        .application_str(&valid_uuid.to_string())
        .expect("Valid UUID string");
    let policy = builder.request_limit(50).build().expect("Build policy");
    assert_eq!(policy.application_id, valid_uuid);

    // Invalid string
    let res_bad = RateLimitPolicyBuilder::new("web-tier", AlgorithmType::FixedWindow)
        .application_str("not-a-uuid");
    assert!(res_bad.is_err());
}

#[test]
fn test_evaluation_request_constructors_and_defaults() {
    // EvaluationRequest::new: defaults cost 0 to 1, application_id to None
    let req = EvaluationRequest::new("user:42", None::<String>, 0).expect("Valid request");
    assert_eq!(req.identity.as_str(), "user:42");
    assert_eq!(req.policy_name, None);
    assert_eq!(req.cost, 1);
    assert_eq!(req.application_id, None);

    // Policy name with whitespace is trimmed/filtered
    let req2 = EvaluationRequest::new("user:42", Some("  premium  "), 10).expect("Valid request");
    assert_eq!(req2.policy_name, Some("premium".to_string()));
    assert_eq!(req2.cost, 10);

    let req_blank_policy =
        EvaluationRequest::new("user:42", Some("   "), 5).expect("Valid request");
    assert_eq!(req_blank_policy.policy_name, None);

    // EvaluationRequest::for_application
    let app_id = Uuid::new_v4();
    let bound_req = EvaluationRequest::for_application(app_id, "user:99", Some("standard"), 3)
        .expect("Valid bound request");
    assert_eq!(bound_req.application_id, Some(app_id));
    assert_eq!(bound_req.identity.as_str(), "user:99");
    assert_eq!(bound_req.cost, 3);
}

#[test]
fn test_evaluation_request_rejects_nil_application_id() {
    // Immediate rejection in for_application
    let res = EvaluationRequest::for_application(Uuid::nil(), "user:1", None::<String>, 1);
    assert!(res.is_err());
    assert!(res.unwrap_err().to_string().contains("cannot be nil"));

    // Rejection in with_application_id
    let req = EvaluationRequest::new("user:1", None::<String>, 1).unwrap();
    let res2 = req.with_application_id(Uuid::nil());
    assert!(res2.is_err());
    assert!(res2.unwrap_err().to_string().contains("cannot be nil"));

    // Rejection in with_application_str with nil UUID
    let req3 = EvaluationRequest::new("user:1", None::<String>, 1).unwrap();
    let res3 = req3.with_application_str("00000000-0000-0000-0000-000000000000");
    assert!(res3.is_err());
    assert!(res3.unwrap_err().to_string().contains("cannot be nil"));
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
