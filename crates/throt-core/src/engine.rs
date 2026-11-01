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

/// Evaluation request input to the rate-limiting engine.
#[derive(Debug, Clone)]
pub struct EvaluationRequest {
    pub application_id: Uuid,
    pub identity: Identity,
    pub policy_name: Option<String>,
    pub cost: u64,
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
        let cost = if req.cost == 0 { 1 } else { req.cost };

        // Resolve policy following strict 3-tier precedence:
        // 1. Explicitly requested policy name
        // 2. Identity-specific assignment
        // 3. Application default policy
        let policy = if let Some(ref name) = req.policy_name {
            self.policy_store
                .get_by_name(req.application_id, name)
                .await?
                .ok_or_else(|| {
                    ThrotError::PolicyNotFound(format!(
                        "Policy '{name}' not found for application {}",
                        req.application_id
                    ))
                })?
        } else if let Some(assigned) = self
            .policy_store
            .get_by_identity(req.application_id, &req.identity)
            .await?
        {
            assigned
        } else if let Some(default_policy) =
            self.policy_store.get_default(req.application_id).await?
        {
            default_policy
        } else {
            return Err(ThrotError::PolicyNotFound(format!(
                "No matching rate-limit policy found for application {}",
                req.application_id
            )));
        };

        debug!(
            application_id = %req.application_id,
            policy_id = %policy.id,
            policy_name = %policy.name,
            identity = %req.identity,
            cost = cost,
            "Evaluating rate limit decision"
        );

        match self
            .evaluator
            .evaluate_request(req.application_id, &policy, &req.identity, cost)
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
