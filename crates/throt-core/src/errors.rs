use thiserror::Error;

/// Core domain errors for THROT.
#[derive(Debug, Error)]
pub enum ThrotError {
    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Policy not found: {0}")]
    PolicyNotFound(String),

    #[error("Application not found: {0}")]
    ApplicationNotFound(String),

    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    #[error("Storage failure: {0}")]
    Storage(String),

    #[error("Internal error: {0}")]
    Internal(String),
}
