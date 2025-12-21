mod helpers;
mod routes;

use axum::{http::StatusCode, routing::get, Router};
use std::sync::Arc;

pub(crate) struct AppState {
    db: oxeye_db::Database,
}

#[tokio::main]
async fn main() {
    let state = Arc::new(AppState {
        db: oxeye_db::Database::open("oxeye.db").await.unwrap(),
    });

    let app = Router::new()
        .route("/health", get(|| async { StatusCode::OK }))
        .route("/connect", get(routes::connect))
        .route("/join", get(routes::join))
        .route("/leave", get(routes::leave))
        .route("/sync", get(routes::sync))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
