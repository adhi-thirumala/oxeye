pub mod config;
pub mod helpers;
mod error;
mod routes;
mod validation;

use axum::{http::StatusCode, routing::{get, post}, Router};
use std::sync::Arc;
use std::time::Duration;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::timeout::TimeoutLayer;

pub struct AppState {
    pub db: oxeye_db::Database,
}

/// Create the application router with the given database and configuration
pub fn create_app(db: oxeye_db::Database, request_body_limit: usize, request_timeout: Duration) -> Router {
    let state = Arc::new(AppState { db });

    Router::new()
        .route("/health", get(|| async { StatusCode::OK }))
        .route("/connect", post(routes::connect))
        .route("/join", post(routes::join))
        .route("/leave", post(routes::leave))
        .route("/sync", post(routes::sync))
        .layer(TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, request_timeout))
        .layer(RequestBodyLimitLayer::new(request_body_limit))
        .with_state(state)
}
