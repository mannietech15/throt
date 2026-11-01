//! HTTP API layer for THROT.

pub mod response;
pub mod routes;

pub use response::{ApiErrorDetail, ApiErrorResponse, ApiResponse};
pub use routes::health::{health_router, AppState};
