//! HTTP API layer for THROT.

pub mod middleware;
pub mod response;
pub mod routes;

pub use middleware::auth::{require_api_key, AuthState};
pub use response::{ApiErrorDetail, ApiErrorResponse, ApiResponse};
pub use routes::create_router;
pub use routes::health::{health_router, AppState};
