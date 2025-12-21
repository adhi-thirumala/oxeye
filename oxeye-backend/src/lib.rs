pub mod helpers;
mod routes;

use axum::{http::StatusCode, routing::{get, post}, Router};
use std::sync::Arc;

pub struct AppState {
    pub db: oxeye_db::Database,
}

/// Create the application router with the given database
pub fn create_app(db: oxeye_db::Database) -> Router {
    let state = Arc::new(AppState { db });

    Router::new()
        .route("/health", get(|| async { StatusCode::OK }))
        .route("/connect", post(routes::connect))
        .route("/join", post(routes::join))
        .route("/leave", post(routes::leave))
        .route("/sync", post(routes::sync))
        .with_state(state)
}
