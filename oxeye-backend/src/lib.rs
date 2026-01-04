pub mod config;
mod error;
pub mod helpers;
mod routes;
mod validation;

use axum::{
    Router,
    http::StatusCode,
    routing::{get, post},
};
use std::sync::Arc;
use std::time::Duration;
use tower_governor::{
    GovernorLayer, governor::GovernorConfigBuilder, key_extractor::SmartIpKeyExtractor,
};
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::timeout::TimeoutLayer;

pub struct AppState {
    pub db: oxeye_db::Database,
}

/// Rate limiting configuration
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Requests per minute for /connect endpoint
    pub connect_per_min: u64,
    /// Burst size for /connect endpoint
    pub connect_burst: u32,
    /// Requests per second for player endpoints (/join, /leave, /sync)
    pub player_per_sec: u64,
    /// Burst size for player endpoints
    pub player_burst: u32,
    /// Requests per second for general endpoints
    pub general_per_sec: u64,
    /// Burst size for general endpoints
    pub general_burst: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            connect_per_min: 5,
            connect_burst: 2,
            player_per_sec: 50,
            player_burst: 100,
            general_per_sec: 10,
            general_burst: 20,
        }
    }
}

/// Create the application router with the given database and configuration
pub fn create_app(
    db: oxeye_db::Database,
    request_body_limit: usize,
    request_timeout: Duration,
    rate_limit: RateLimitConfig,
) -> Router {
    let state = Arc::new(AppState { db });

    // Strict rate limit for /connect - only needed once per server setup
    let connect_governor = GovernorConfigBuilder::default()
        .per_second(rate_limit.connect_per_min / 60 + 1) // Convert per-min to per-sec, min 1
        .burst_size(rate_limit.connect_burst)
        .key_extractor(SmartIpKeyExtractor)
        .finish()
        .unwrap();

    // Lenient rate limit for player endpoints - many players join/leave at once
    let player_governor = GovernorConfigBuilder::default()
        .per_second(rate_limit.player_per_sec)
        .burst_size(rate_limit.player_burst)
        .key_extractor(SmartIpKeyExtractor)
        .finish()
        .unwrap();

    // General rate limit for other endpoints
    let general_governor = GovernorConfigBuilder::default()
        .per_second(rate_limit.general_per_sec)
        .burst_size(rate_limit.general_burst)
        .key_extractor(SmartIpKeyExtractor)
        .finish()
        .unwrap();

    // Routes with strict rate limiting (connect is sensitive)
    let connect_routes = Router::new()
        .route("/connect", post(routes::connect))
        .layer(GovernorLayer::new(connect_governor));

    // Routes with lenient rate limiting (high traffic from players)
    let player_routes = Router::new()
        .route("/join", post(routes::join))
        .route("/leave", post(routes::leave))
        .route("/sync", post(routes::sync))
        .layer(GovernorLayer::new(player_governor));

    // Routes with general rate limiting
    let general_routes = Router::new()
        .route("/status", get(routes::status))
        .route("/disconnect", post(routes::disconnect))
        .layer(GovernorLayer::new(general_governor));

    Router::new()
        .route("/health", get(|| async { StatusCode::OK }))
        .merge(connect_routes)
        .merge(player_routes)
        .merge(general_routes)
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            request_timeout,
        ))
        .layer(RequestBodyLimitLayer::new(request_body_limit))
        .with_state(state)
}
