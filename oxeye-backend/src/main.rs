#[tokio::main]
async fn main() {
    // Initialize tracing for structured logging
    tracing_subscriber::fmt()
        .with_target(false)
        .compact()
        .init();

    tracing::info!("Starting Oxeye backend server...");

    let db = oxeye_db::Database::open("oxeye.db").await.unwrap();
    let app = oxeye_backend::create_app(db);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    tracing::info!("Server listening on 0.0.0.0:3000");

    axum::serve(listener, app).await.unwrap();
}
