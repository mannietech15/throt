use crate::decision::Decision;
use crate::errors::ThrotError;
use crate::identity::Identity;
use crate::policy::{FailureMode, RateLimitPolicy};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tracing::{debug, warn};
use uuid::Uuid;

/// Contract for persistent or cached rate limit policy lookup.
#[async_trait]
pub trait PolicyStore: Send + Sync {
    /// Retrieve a policy by application ID and explicit policy name.
    async fn get_by_name(
        &self,
        application_id: Uuid,
        name: &str,
    ) -> Result<Option<RateLimitPolicy>, ThrotError>;

    /// Retrieve the default fallback policy for an application.
    async fn get_default(
        &self,
        application_id: Uuid,
    ) -> Result<Option<RateLimitPolicy>, ThrotError>;

    /// Retrieve an identity-specific policy assignment.
    async fn get_by_identity(
        &self,
        application_id: Uuid,
        identity: &Identity,
    ) -> Result<Option<RateLimitPolicy>, ThrotError>;
}

/// An in-memory policy store implementation for unit tests and local caching.
#[derive(Default)]
pub struct InMemoryPolicyStore {
    policies: RwLock<HashMap<(Uuid, String), RateLimitPolicy>>,
    defaults: RwLock<HashMap<Uuid, RateLimitPolicy>>,
    identity_assignments: RwLock<HashMap<(Uuid, String), RateLimitPolicy>>,
}

impl InMemoryPolicyStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert_policy(&self, policy: RateLimitPolicy) {
        let mut policies = self.policies.write().unwrap();
        policies.insert((policy.application_id, policy.name.clone()), policy);
    }

    pub fn set_default_policy(&self, application_id: Uuid, policy: RateLimitPolicy) {
        let mut defaults = self.defaults.write().unwrap();
        defaults.insert(application_id, policy);
    }

    pub fn assign_identity_policy(
        &self,
        application_id: Uuid,
        identity: &Identity,
        policy: RateLimitPolicy,
    ) {
        let mut assignments = self.identity_assignments.write().unwrap();
        assignments.insert((application_id, identity.as_str().to_string()), policy);
    }
}

#[async_trait]
impl PolicyStore for InMemoryPolicyStore {
    async fn get_by_name(
        &self,
        application_id: Uuid,
        name: &str,
    ) -> Result<Option<RateLimitPolicy>, ThrotError> {
        let policies = self.policies.read().unwrap();
        Ok(policies.get(&(application_id, name.to_string())).cloned())
    }

    async fn get_default(
        &self,
        application_id: Uuid,
    ) -> Result<Option<RateLimitPolicy>, ThrotError> {
        let defaults = self.defaults.read().unwrap();
        Ok(defaults.get(&application_id).cloned())
    }

    async fn get_by_identity(
        &self,
        application_id: Uuid,
        identity: &Identity,
    ) -> Result<Option<RateLimitPolicy>, ThrotError> {
        let assignments = self.identity_assignments.read().unwrap();
        Ok(assignments
            .get(&(application_id, identity.as_str().to_string()))
            .cloned())
    }
}

/// Helper trait to convert various string types or an existing `Identity` into `Identity`.
pub trait IntoIdentity {
    fn into_identity(self) -> Result<Identity, ThrotError>;
}

impl IntoIdentity for Identity {
    fn into_identity(self) -> Result<Identity, ThrotError> {
        Ok(self)
    }
}

impl IntoIdentity for &Identity {
    fn into_identity(self) -> Result<Identity, ThrotError> {
        Ok(self.clone())
    }
}

impl IntoIdentity for &str {
    fn into_identity(self) -> Result<Identity, ThrotError> {
        Identity::new(self).map_err(|e| ThrotError::Validation(format!("Invalid identity: {e}")))
    }
}

impl IntoIdentity for String {
    fn into_identity(self) -> Result<Identity, ThrotError> {
        Identity::new(self).map_err(|e| ThrotError::Validation(format!("Invalid identity: {e}")))
    }
}

impl IntoIdentity for &String {
    fn into_identity(self) -> Result<Identity, ThrotError> {
        Identity::new(self.as_str())
            .map_err(|e| ThrotError::Validation(format!("Invalid identity: {e}")))
    }
}

/// Evaluation request input to the rate-limiting engine.
#[derive(Debug, Clone)]
pub struct EvaluationRequest {
    pub application_id: Option<Uuid>,
    pub identity: Identity,
    pub policy_name: Option<String>,
    pub cost: u64,
}

impl EvaluationRequest {
    /// Creates a new evaluation request with validated identity and default cost.
    ///
    /// Note: `application_id` must be assigned via `.with_application_id()` or created
    /// directly with `.for_application()` before evaluating in production.
    pub fn new(
        identity: impl IntoIdentity,
        policy_name: Option<impl Into<String>>,
        cost: u64,
    ) -> Result<Self, ThrotError> {
        let identity = identity.into_identity()?;
        let policy_name = policy_name
            .map(|p| p.into().trim().to_string())
            .filter(|s| !s.is_empty());
        let cost = if cost == 0 { 1 } else { cost };

        Ok(Self {
            application_id: None,
            identity,
            policy_name,
            cost,
        })
    }

    /// Creates a complete, validated evaluation request bound to a specific application ID.
    /// Rejects `Uuid::nil()`.
    pub fn for_application(
        application_id: Uuid,
        identity: impl IntoIdentity,
        policy_name: Option<impl Into<String>>,
        cost: u64,
    ) -> Result<Self, ThrotError> {
        let req = Self::new(identity, policy_name, cost)?;
        req.with_application_id(application_id)
    }

    /// Attaches an application ID. Validates that the application ID is not nil (`Uuid::nil()`).
    pub fn with_application_id(mut self, application_id: Uuid) -> Result<Self, ThrotError> {
        if application_id.is_nil() {
            return Err(ThrotError::Validation(
                "Application ID cannot be nil (00000000-0000-0000-0000-000000000000)".to_string(),
            ));
        }
        self.application_id = Some(application_id);
        Ok(self)
    }

    /// Parses and attaches an application ID from a UUID string. Validates non-nil.
    pub fn with_application_str(self, app_id_str: &str) -> Result<Self, ThrotError> {
        let id = Uuid::parse_str(app_id_str.trim()).map_err(|e| {
            ThrotError::Validation(format!("Invalid UUID format for application ID: {e}"))
        })?;
        self.with_application_id(id)
    }
}

/// Dynamic evaluator trait to decouple core engine from specific storage or algorithm implementations.
#[async_trait]
pub trait RateLimitEvaluator: Send + Sync {
    async fn evaluate_request(
        &self,
        application_id: Uuid,
        policy: &RateLimitPolicy,
        identity: &Identity,
        cost: u64,
    ) -> Result<Decision, ThrotError>;
}

/// Core evaluation engine coordinating policy resolution, identity enforcement, and failure modes.
pub struct RateLimitEngine {
    policy_store: Arc<dyn PolicyStore>,
    evaluator: Arc<dyn RateLimitEvaluator>,
}

impl RateLimitEngine {
    pub fn new(policy_store: Arc<dyn PolicyStore>, evaluator: Arc<dyn RateLimitEvaluator>) -> Self {
        Self {
            policy_store,
            evaluator,
        }
    }

    /// Evaluates an incoming rate-limit request according to policy precedence and failure behavior.
    pub async fn evaluate(&self, req: EvaluationRequest) -> Result<Decision, ThrotError> {
        let application_id = match req.application_id {
            Some(id) if !id.is_nil() => id,
            Some(_) => {
                return Err(ThrotError::Validation(
                    "Application ID cannot be nil (00000000-0000-0000-0000-000000000000) in production evaluation requests".to_string(),
                ));
            }
            None => {
                return Err(ThrotError::Validation(
                    "Application ID is required for rate limit evaluation. Call .with_application_id() before evaluating.".to_string(),
                ));
            }
        };

        let cost = if req.cost == 0 { 1 } else { req.cost };

        // Resolve policy following strict 3-tier precedence:
        // 1. Explicitly requested policy name
        // 2. Identity-specific assignment
        // 3. Application default policy
        let policy = if let Some(ref name) = req.policy_name {
            self.policy_store
                .get_by_name(application_id, name)
                .await?
                .ok_or_else(|| {
                    ThrotError::PolicyNotFound(format!(
                        "Policy '{name}' not found for application {}",
                        application_id
                    ))
                })?
        } else if let Some(assigned) = self
            .policy_store
            .get_by_identity(application_id, &req.identity)
            .await?
        {
            assigned
        } else if let Some(default_policy) = self.policy_store.get_default(application_id).await? {
            default_policy
        } else {
            return Err(ThrotError::PolicyNotFound(format!(
                "No matching rate-limit policy found for application {}",
                application_id
            )));
        };

        debug!(
            application_id = %application_id,
            policy_id = %policy.id,
            policy_name = %policy.name,
            identity = %req.identity,
            cost = cost,
            "Evaluating rate limit decision"
        );

        match self
            .evaluator
            .evaluate_request(application_id, &policy, &req.identity, cost)
            .await
        {
            Ok(decision) => {
                metrics::counter!(
                    "throt_evaluations_total",
                    "algorithm" => format!("{:?}", policy.algorithm),
                    "allowed" => decision.allowed.to_string()
                )
                .increment(1);
                Ok(decision)
            }
            Err(e) => {
                warn!(
                    error = %e,
                    failure_mode = ?policy.failure_mode,
                    "Rate limit evaluation failed; evaluating fallback policy"
                );

                metrics::counter!(
                    "throt_evaluations_fallback_total",
                    "mode" => format!("{:?}", policy.failure_mode)
                )
                .increment(1);

                match policy.failure_mode {
                    FailureMode::FailOpen => {
                        let now = chrono::Utc::now().timestamp();
                        Ok(Decision::allow(
                            policy.request_limit,
                            policy.request_limit,
                            now + policy.window_seconds as i64,
                        ))
                    }
                    FailureMode::FailClosed => Err(e),
                }
            }
        }
    }
}
