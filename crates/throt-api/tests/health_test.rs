use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use std::sync::Arc;
use throt_api::{health_router, AppState};
use tower::ServiceExt;

#[tokio::test]
async fn test_health_liveness_endpoint() {
    let state = Arc::new(AppState {
        redis: None,
        postgres: None,
    });

    let app = health_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body_str = String::from_utf8(body.to_vec()).unwrap();
    assert!(body_str.contains("\"status\":\"pass\""));
}

#[tokio::test]
async fn test_ready_unconfigured_endpoint() {
    let state = Arc::new(AppState {
        redis: None,
        postgres: None,
    });

    let app = health_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let body_str = String::from_utf8(body.to_vec()).unwrap();
    assert!(body_str.contains("\"ready\":true"));
}
