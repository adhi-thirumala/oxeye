#[tokio::main]
async fn main() {
    // Initialize tracing for structured logging
    tracing_subscriber::fmt()
        .with_target(false)
        .compact()
        .init();

    tracing::info!("Starting Oxeye backend server...");

    // Load configuration from environment variables or use defaults
    let config = oxeye_backend::config::Config::from_env();
    tracing::info!(
        "Configuration: port={}, db_path={}, body_limit={}KB, timeout={}s",
        config.port,
        config.database_path,
        config.request_body_limit / 1024,
        config.request_timeout.as_secs()
    );

    let db = oxeye_db::Database::open(&config.database_path).await.unwrap();
    let app = oxeye_backend::create_app(db, config.request_body_limit, config.request_timeout);

    let addr = format!("0.0.0.0:{}", config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    tracing::info!("Server listening on {}", addr);

    axum::serve(listener, app).await.unwrap();
}
