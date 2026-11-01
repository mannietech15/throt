//! Core domain models and rate limiting engine abstractions for THROT.

pub mod api_key;
pub mod app;
pub mod decision;
pub mod engine;
pub mod errors;
pub mod identity;
pub mod policy;

pub use api_key::{ApiKeyMetadata, GeneratedApiKey};
pub use app::Application;

pub use decision::Decision;
pub use engine::{
    EvaluationRequest, InMemoryPolicyStore, PolicyStore, RateLimitEngine, RateLimitEvaluator,
};
pub use errors::ThrotError;
pub use identity::Identity;
pub use policy::{AlgorithmType, FailureMode, RateLimitPolicy};
