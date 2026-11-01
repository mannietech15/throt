use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use metrics_exporter_prometheus::PrometheusHandle;
use std::sync::Arc;

#[derive(Clone)]
pub struct MetricsState {
    pub handle: PrometheusHandle,
}

/// GET /metrics - exposes Prometheus metrics.
pub async fn metrics_handler(State(state): State<Arc<MetricsState>>) -> Response {
    (StatusCode::OK, state.handle.render()).into_response()
}
