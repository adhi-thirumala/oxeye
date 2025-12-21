use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use oxeye_backend::{create_app, helpers};
use serde_json::{json, Value};
use tower::ServiceExt;

/// Helper to create test database
async fn setup_test_db() -> oxeye_db::Database {
    oxeye_db::Database::open_in_memory()
        .await
        .expect("Failed to create in-memory database")
}

/// Helper to send a request and get response
async fn send_request(
    app: axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
    auth_token: Option<&str>,
) -> (StatusCode, Value) {
    let mut request_builder = Request::builder().uri(uri).method(method);

    if let Some(token) = auth_token {
        request_builder = request_builder.header("Authorization", format!("Bearer {}", token));
    }

    let request = if let Some(json_body) = body {
        request_builder
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&json_body).unwrap()))
            .unwrap()
    } else {
        request_builder.body(Body::empty()).unwrap()
    };

    let response = app.oneshot(request).await.unwrap();
    let status = response.status();

    let body_bytes = response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes();

    let json = if body_bytes.is_empty() {
        json!({})
    } else {
        serde_json::from_slice(&body_bytes).unwrap_or(json!({}))
    };

    (status, json)
}

#[tokio::test]
async fn test_error_response_format_for_nonexistent_code() {
    // GIVEN: An empty database
    let db = setup_test_db().await;
    let app = create_app(db);

    // WHEN: Requesting with nonexistent code
    let (status, body) = send_request(
        app,
        "POST",
        "/connect",
        Some(json!({ "code": "oxeye-invalid" })),
        None,
    )
    .await;

    // THEN: Should return 404 with JSON error
    assert_eq!(status, StatusCode::NOT_FOUND);

    // AND: Error response should have proper structure
    assert!(body.get("error").is_some(), "Response should have 'error' field");
    let error_msg = body["error"].as_str().unwrap();
    assert!(
        error_msg.contains("not found") || error_msg.contains("expired"),
        "Error message should be user-friendly"
    );
}

#[tokio::test]
async fn test_error_response_for_invalid_api_key() {
    // GIVEN: An empty database
    let db = setup_test_db().await;
    let app = create_app(db);

    // WHEN: Making request with invalid API key
    let (status, body) = send_request(
        app,
        "POST",
        "/join",
        Some(json!({ "player": "Steve" })),
        Some("oxeye-sk-invalid12345678901234567890"),
    )
    .await;

    // THEN: Should return 401 with JSON error
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // AND: Error response should be helpful
    assert!(body.get("error").is_some());
    let error_msg = body["error"].as_str().unwrap();
    assert!(
        error_msg.contains("Invalid") || error_msg.contains("API key"),
        "Error message: {}",
        error_msg
    );
}

#[tokio::test]
async fn test_error_response_for_validation_failure() {
    // GIVEN: A valid server
    let db = setup_test_db().await;
    let api_key = helpers::generate_api_key();
    let api_key_hash = helpers::hash_api_key(&api_key);

    db.create_server(api_key_hash, "TestServer".to_string(), 123456)
        .await
        .expect("Failed to create server");

    let app = create_app(db);

    // WHEN: Sending invalid player name (too long)
    let (status, body) = send_request(
        app,
        "POST",
        "/join",
        Some(json!({ "player": "12345678901234567" })), // 17 chars
        Some(&api_key),
    )
    .await;

    // THEN: Should return 400 with validation error
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // AND: Error message should explain the problem
    assert!(body.get("error").is_some());
    let error_msg = body["error"].as_str().unwrap();
    assert!(
        error_msg.contains("too long") || error_msg.contains("16"),
        "Error should mention length limit: {}",
        error_msg
    );
}

#[tokio::test]
async fn test_error_response_doesnt_expose_internals() {
    // GIVEN: An empty database
    let db = setup_test_db().await;
    let app = create_app(db);

    // WHEN: Making request that would cause DB error
    let (status, body) = send_request(
        app,
        "POST",
        "/connect",
        Some(json!({ "code": "oxeye-nonexistent" })),
        None,
    )
    .await;

    // THEN: Error should not expose internal details
    assert_eq!(status, StatusCode::NOT_FOUND);

    let error_msg = body["error"].as_str().unwrap();

    // Should NOT contain internal error details
    assert!(
        !error_msg.contains("SQL"),
        "Should not expose SQL details"
    );
    assert!(
        !error_msg.contains("rusqlite"),
        "Should not expose library names"
    );
    assert!(
        !error_msg.contains("panic"),
        "Should not expose panic details"
    );
    assert!(
        !error_msg.contains("stack"),
        "Should not expose stack traces"
    );

    // Should contain user-friendly message
    assert!(
        error_msg.contains("not found") || error_msg.contains("expired"),
        "Should have user-friendly message"
    );
}

#[tokio::test]
async fn test_validation_error_has_details() {
    // GIVEN: A valid server
    let db = setup_test_db().await;
    let api_key = helpers::generate_api_key();
    let api_key_hash = helpers::hash_api_key(&api_key);

    db.create_server(api_key_hash, "TestServer".to_string(), 123456)
        .await
        .expect("Failed to create server");

    let app = create_app(db);

    // WHEN: Sending player name with invalid characters
    let (status, body) = send_request(
        app,
        "POST",
        "/join",
        Some(json!({ "player": "Player-123" })),
        Some(&api_key),
    )
    .await;

    // THEN: Error should be specific
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let error_msg = body["error"].as_str().unwrap();
    assert!(
        error_msg.contains("Invalid") || error_msg.contains("character"),
        "Should explain what's invalid: {}",
        error_msg
    );
}
