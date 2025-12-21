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

// Request body size limit: 1MB
// This prevents DOS attacks via massive payloads while allowing reasonable requests
// Context: 1000 players * ~100 bytes per player in JSON = ~100KB, so 1MB is generous
const REQUEST_BODY_LIMIT: usize = 1024 * 1024; // 1 MB

// Request timeout: 30 seconds
// Prevents hung requests from database deadlocks or slow queries
// Most requests complete in <100ms, 30s is generous buffer
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Create the application router with the given database
pub fn create_app(db: oxeye_db::Database) -> Router {
    let state = Arc::new(AppState { db });

    Router::new()
        .route("/health", get(|| async { StatusCode::OK }))
        .route("/connect", post(routes::connect))
        .route("/join", post(routes::join))
        .route("/leave", post(routes::leave))
        .route("/sync", post(routes::sync))
        .layer(TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, REQUEST_TIMEOUT))
        .layer(RequestBodyLimitLayer::new(REQUEST_BODY_LIMIT))
        .with_state(state)
}
