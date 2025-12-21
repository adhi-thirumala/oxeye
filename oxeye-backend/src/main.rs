#[tokio::main]
async fn main() {
    let db = oxeye_db::Database::open("oxeye.db").await.unwrap();
    let app = oxeye_backend::create_app(db);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
