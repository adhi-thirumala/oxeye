use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use oxeye_backend::{create_app, helpers};
use serde_json::{json, Value};
use tower::ServiceExt;
// for `oneshot` method

/// Helper to create test database with in-memory SQLite
async fn setup_test_db() -> oxeye_db::Database {
    oxeye_db::Database::open_in_memory()
        .await
        .expect("Failed to create in-memory database")
}

/// Helper to create app with default test configuration
fn create_test_app(db: oxeye_db::Database) -> axum::Router {
    let config = oxeye_backend::config::Config::default();
    create_app(db, config.request_body_limit, config.request_timeout)
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

    // Add Authorization header if provided
    if let Some(token) = auth_token {
        request_builder = request_builder.header("Authorization", format!("Bearer {}", token));
    }

    // Build request with body
    let request = if let Some(json_body) = body {
        request_builder
            .header("Content-Type", "application/json")
            .body(Body::from(serde_json::to_vec(&json_body).unwrap()))
            .unwrap()
    } else {
        request_builder.body(Body::empty()).unwrap()
    };

    // Send request
    let response = app.oneshot(request).await.unwrap();

    // Extract status
    let status = response.status();

    // Extract body
    let body_bytes = response.into_body().collect().await.unwrap().to_bytes();

    // Try to parse as JSON, or return empty object
    let json = if body_bytes.is_empty() {
        json!({})
    } else {
        serde_json::from_slice(&body_bytes).unwrap_or(json!({}))
    };

    (status, json)
}

// =============================================================================
// HEALTH ENDPOINT TESTS
// =============================================================================

#[tokio::test]
async fn test_health_endpoint_returns_ok() {
    // GIVEN: A running application
    let db = setup_test_db().await;
    let app = create_test_app(db);

    // WHEN: Making a GET request to /health
    let (status, _body) = send_request(app, "GET", "/health", None, None).await;

    // THEN: Should return 200 OK
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn test_health_endpoint_with_post_method() {
    // GIVEN: A running application
    let db = setup_test_db().await;
    let app = create_test_app(db);

    // WHEN: Making a POST request to /health (wrong method)
    let (status, _body) = send_request(app, "POST", "/health", None, None).await;

    // THEN: Should return 405 Method Not Allowed
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
}

// =============================================================================
// CONNECT ENDPOINT TESTS
// =============================================================================

#[tokio::test]
async fn test_connect_success() {
    // GIVEN: A pending link exists in the database
    let db = setup_test_db().await;
    let code = helpers::generate_code();
    let guild_id = 123456789u64;
    let server_name = "TestServer".to_string();
    let now = helpers::now();

    db.create_pending_link(code.clone(), guild_id, server_name.clone(), now)
        .await
        .expect("Failed to create pending link");

    let app = create_test_app(db);

    // WHEN: Making a POST request to /connect with valid code
    let (status, body) =
        send_request(app, "POST", "/connect", Some(json!({ "code": code })), None).await;

    // THEN: Should return 201 OK with an API key
    assert_eq!(status, StatusCode::CREATED);
    assert!(body.get("api_key").is_some());
    let api_key = body["api_key"].as_str().unwrap();
    assert!(api_key.starts_with("oxeye-sk-"));
    assert_eq!(api_key.len(), "oxeye-sk-".len() + 32);
}

#[tokio::test]
async fn test_connect_with_nonexistent_code() {
    // GIVEN: An empty database
    let db = setup_test_db().await;
    let app = create_test_app(db);

    // WHEN: Making a POST request to /connect with nonexistent code
    let (status, _body) = send_request(
        app,
        "POST",
        "/connect",
        Some(json!({ "code": "oxeye-invalid" })),
        None,
    )
        .await;

    // THEN: Should return 404 Not Found
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_connect_with_expired_code() {
    // GIVEN: An expired pending link (created 11 minutes ago)
    let db = setup_test_db().await;
    let code = helpers::generate_code();
    let guild_id = 123456789u64;
    let server_name = "TestServer".to_string();
    let now = helpers::now();
    let eleven_minutes_ago = now - 11 * 60; // 11 minutes = 660 seconds

    db.create_pending_link(code.clone(), guild_id, server_name, eleven_minutes_ago)
        .await
        .expect("Failed to create pending link");

    let app = create_test_app(db);

    // WHEN: Making a POST request to /connect with expired code
    let (status, _body) =
        send_request(app, "POST", "/connect", Some(json!({ "code": code })), None).await;

    // THEN: Should return 404 Not Found
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_connect_with_already_used_code() {
    // GIVEN: A pending link that has been consumed
    let db = setup_test_db().await;
    let code = helpers::generate_code();
    let guild_id = 123456789u64;
    let server_name = "TestServer".to_string();
    let now = helpers::now();

    db.create_pending_link(code.clone(), guild_id, server_name.clone(), now)
        .await
        .expect("Failed to create pending link");

    // Consume the link
    db.consume_pending_link(code.clone(), now)
        .await
        .expect("Failed to consume pending link");

    let app = create_test_app(db);

    // WHEN: Making a POST request to /connect with already used code
    let (status, _body) =
        send_request(app, "POST", "/connect", Some(json!({ "code": code })), None).await;

    // THEN: Should return 404 Not Found (code no longer exists)
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_connect_with_server_name_conflict() {
    // GIVEN: A server already exists with the same name in the guild
    let db = setup_test_db().await;
    let guild_id = 123456789u64;
    let server_name = "TestServer".to_string();

    // Create existing server
    let existing_api_key = helpers::generate_api_key();
    let existing_hash = helpers::hash_api_key(&existing_api_key);
    db.create_server(existing_hash, server_name.clone(), guild_id)
        .await
        .expect("Failed to create existing server");

    // Create pending link with same server name
    let code = helpers::generate_code();
    let now = helpers::now();

    // This should fail at creation time
    let result = db
        .create_pending_link(code.clone(), guild_id, server_name, now)
        .await;

    // THEN: Should return error for name conflict
    assert!(result.is_err());
    assert!(matches!(
    result.unwrap_err(),
    oxeye_db::DbError::ServerNameConflict
  ));
}

#[tokio::test]
async fn test_connect_with_invalid_code_format() {
    // GIVEN: A running application
    let db = setup_test_db().await;
    let app = create_test_app(db);

    // WHEN: Making a POST request to /connect with invalid code format
    let (status, _body) = send_request(
        app,
        "POST",
        "/connect",
        Some(json!({ "code": "invalid-format" })),
        None,
    )
        .await;

    // THEN: Should return 400 Bad Request
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_connect_without_body() {
    // GIVEN: A running application
    let db = setup_test_db().await;
    let app = create_test_app(db);

    // WHEN: Making a POST request to /connect without body
    let request = Request::builder()
        .uri("/connect")
        .method("POST")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    let status = response.status();

    // THEN: Should return 400 Bad Request or similar error
    // Axum returns 422 Unprocessable Entity for missing required fields
    assert!(
        status.is_client_error(),
        "Expected client error, got {}",
        status
    );
}

// =============================================================================
// JOIN ENDPOINT TESTS
// =============================================================================

#[tokio::test]
async fn test_join_success() {
    // GIVEN: A valid server exists
    let db = setup_test_db().await;
    let api_key = helpers::generate_api_key();
    let api_key_hash = helpers::hash_api_key(&api_key);
    let guild_id = 123456789u64;
    let server_name = "TestServer".to_string();

    db.create_server(api_key_hash, server_name, guild_id)
        .await
        .expect("Failed to create server");

    let app = create_test_app(db);

    // WHEN: Making a POST request to /join with valid API key
    let (status, _body) = send_request(
        app,
        "POST",
        "/join",
        Some(json!({ "player": "Steve" })),
        Some(&api_key),
    )
        .await;

    // THEN: Should return 200 OK
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn test_join_with_invalid_api_key() {
    // GIVEN: An empty database
    let db = setup_test_db().await;
    let app = create_test_app(db);

    // WHEN: Making a POST request to /join with invalid API key
    let (status, _body) = send_request(
        app,
        "POST",
        "/join",
        Some(json!({ "player": "Steve" })),
        Some("oxeye-sk-invalid12345678901234567890"),
    )
        .await;

    // THEN: Should return 401 Unauthorized
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_join_without_authorization() {
    // GIVEN: A running application
    let db = setup_test_db().await;
    let app = create_test_app(db);

    // WHEN: Making a POST request to /join without Authorization header
    let request = Request::builder()
        .uri("/join")
        .method("POST")
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&json!({ "player": "Steve" })).unwrap(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    let status = response.status();

    // THEN: Should return 400 Bad Request (missing required header)
    assert!(
        status.is_client_error(),
        "Expected client error, got {}",
        status
    );
}

#[tokio::test]
async fn test_join_same_player_twice() {
    // GIVEN: A player has already joined
    let db = setup_test_db().await;
    let api_key = helpers::generate_api_key();
    let api_key_hash = helpers::hash_api_key(&api_key);
    let guild_id = 123456789u64;
    let server_name = "TestServer".to_string();

    db.create_server(api_key_hash.clone(), server_name, guild_id)
        .await
        .expect("Failed to create server");

    let now = helpers::now();
    db.player_join(api_key_hash, "Steve".to_string(), now)
        .await
        .expect("Failed to add player");

    let app = create_test_app(db);

    // WHEN: Same player joins again
    let (status, _body) = send_request(
        app,
        "POST",
        "/join",
        Some(json!({ "player": "Steve" })),
        Some(&api_key),
    )
        .await;

    // THEN: Should return 200 OK (upsert behavior - replaces old record)
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn test_join_multiple_players() {
    // GIVEN: A valid server exists
    let db = setup_test_db().await;
    let api_key = helpers::generate_api_key();
    let api_key_hash = helpers::hash_api_key(&api_key);
    let guild_id = 123456789u64;
    let server_name = "TestServer".to_string();

    db.create_server(api_key_hash, server_name, guild_id)
        .await
        .expect("Failed to create server");

    // WHEN: Multiple players join
    for player in &["Steve", "Alex", "Notch"] {
        let app = create_test_app(db.clone());
        let (status, _body) = send_request(
            app,
            "POST",
            "/join",
            Some(json!({ "player": player })),
            Some(&api_key),
        )
            .await;

        // THEN: Each should return 200 OK
        assert_eq!(status, StatusCode::OK);
    }
}

#[tokio::test]
async fn test_join_with_empty_player_name() {
    // GIVEN: A valid server exists
    let db = setup_test_db().await;
    let api_key = helpers::generate_api_key();
    let api_key_hash = helpers::hash_api_key(&api_key);
    let guild_id = 123456789u64;
    let server_name = "TestServer".to_string();

    db.create_server(api_key_hash, server_name, guild_id)
        .await
        .expect("Failed to create server");

    let app = create_test_app(db);

    // WHEN: Making a request with empty player name
    let (status, _body) = send_request(
        app,
        "POST",
        "/join",
        Some(json!({ "player": "" })),
        Some(&api_key),
    )
        .await;

    // THEN: Should return 400 Bad Request (validation now enforced)
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_join_with_invalid_player_name_chars() {
    // GIVEN: A valid server exists
    let db = setup_test_db().await;
    let api_key = helpers::generate_api_key();
    let api_key_hash = helpers::hash_api_key(&api_key);
    let guild_id = 123456789u64;
    let server_name = "TestServer".to_string();

    db.create_server(api_key_hash, server_name, guild_id)
        .await
        .expect("Failed to create server");

    let app = create_test_app(db);

    // WHEN: Making a request with invalid player name (contains special chars)
    let (status, _body) = send_request(
        app,
        "POST",
        "/join",
        Some(json!({ "player": "Player-123" })),
        Some(&api_key),
    )
        .await;

    // THEN: Should return 400 Bad Request
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_join_with_too_long_player_name() {
    // GIVEN: A valid server exists
    let db = setup_test_db().await;
    let api_key = helpers::generate_api_key();
    let api_key_hash = helpers::hash_api_key(&api_key);
    let guild_id = 123456789u64;
    let server_name = "TestServer".to_string();

    db.create_server(api_key_hash, server_name, guild_id)
        .await
        .expect("Failed to create server");

    let app = create_test_app(db);

    // WHEN: Making a request with player name too long (17 chars)
    let (status, _body) = send_request(
        app,
        "POST",
        "/join",
        Some(json!({ "player": "12345678901234567" })),
        Some(&api_key),
    )
        .await;

    // THEN: Should return 400 Bad Request
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

// =============================================================================
// LEAVE ENDPOINT TESTS
// =============================================================================

#[tokio::test]
async fn test_leave_success() {
    // GIVEN: A player is online
    let db = setup_test_db().await;
    let api_key = helpers::generate_api_key();
    let api_key_hash = helpers::hash_api_key(&api_key);
    let guild_id = 123456789u64;
    let server_name = "TestServer".to_string();

    db.create_server(api_key_hash.clone(), server_name, guild_id)
        .await
        .expect("Failed to create server");

    let now = helpers::now();
    db.player_join(api_key_hash, "Steve".to_string(), now)
        .await
        .expect("Failed to add player");

    let app = create_test_app(db);

    // WHEN: Player leaves
    let (status, _body) = send_request(
        app,
        "POST",
        "/leave",
        Some(json!({ "player": "Steve" })),
        Some(&api_key),
    )
        .await;

    // THEN: Should return 200 OK
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn test_leave_player_not_online() {
    // GIVEN: A server exists but player is not online
    let db = setup_test_db().await;
    let api_key = helpers::generate_api_key();
    let api_key_hash = helpers::hash_api_key(&api_key);
    let guild_id = 123456789u64;
    let server_name = "TestServer".to_string();

    db.create_server(api_key_hash, server_name, guild_id)
        .await
        .expect("Failed to create server");

    let app = create_test_app(db);

    // WHEN: Player leaves without being online
    let (status, _body) = send_request(
        app,
        "POST",
        "/leave",
        Some(json!({ "player": "Steve" })),
        Some(&api_key),
    )
        .await;

    // THEN: Should return 200 OK (idempotent operation)
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn test_leave_with_invalid_api_key() {
    // GIVEN: An empty database
    let db = setup_test_db().await;
    let app = create_test_app(db);

    // WHEN: Making a request with invalid API key
    let (status, _body) = send_request(
        app,
        "POST",
        "/leave",
        Some(json!({ "player": "Steve" })),
        Some("oxeye-sk-invalid12345678901234567890"),
    )
        .await;

    // THEN: Should return 401 Unauthorized
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_leave_without_authorization() {
    // GIVEN: A running application
    let db = setup_test_db().await;
    let app = create_test_app(db);

    // WHEN: Making a request without Authorization header
    let request = Request::builder()
        .uri("/leave")
        .method("POST")
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&json!({ "player": "Steve" })).unwrap(),
        ))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    let status = response.status();

    // THEN: Should return 400 Bad Request
    assert!(
        status.is_client_error(),
        "Expected client error, got {}",
        status
    );
}

// =============================================================================
// SYNC ENDPOINT TESTS
// =============================================================================

#[tokio::test]
async fn test_sync_success() {
    // GIVEN: A server exists with some players
    let db = setup_test_db().await;
    let api_key = helpers::generate_api_key();
    let api_key_hash = helpers::hash_api_key(&api_key);
    let guild_id = 123456789u64;
    let server_name = "TestServer".to_string();

    db.create_server(api_key_hash.clone(), server_name, guild_id)
        .await
        .expect("Failed to create server");

    // Add some initial players
    let now = helpers::now();
    db.player_join(api_key_hash.clone(), "Steve".to_string(), now)
        .await
        .expect("Failed to add player");
    db.player_join(api_key_hash, "Alex".to_string(), now)
        .await
        .expect("Failed to add player");

    let app = create_test_app(db);

    // WHEN: Syncing with new player list
    let (status, _body) = send_request(
        app,
        "POST",
        "/sync",
        Some(json!({ "players": ["Notch", "Jeb"] })),
        Some(&api_key),
    )
        .await;

    // THEN: Should return 200 OK
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn test_sync_empty_list() {
    // GIVEN: A server exists with players
    let db = setup_test_db().await;
    let api_key = helpers::generate_api_key();
    let api_key_hash = helpers::hash_api_key(&api_key);
    let guild_id = 123456789u64;
    let server_name = "TestServer".to_string();

    db.create_server(api_key_hash.clone(), server_name, guild_id)
        .await
        .expect("Failed to create server");

    let now = helpers::now();
    db.player_join(api_key_hash, "Steve".to_string(), now)
        .await
        .expect("Failed to add player");

    let app = create_test_app(db);

    // WHEN: Syncing with empty player list (all players left)
    let (status, _body) = send_request(
        app,
        "POST",
        "/sync",
        Some(json!({ "players": [] })),
        Some(&api_key),
    )
        .await;

    // THEN: Should return 200 OK
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn test_sync_with_invalid_api_key() {
    // GIVEN: An empty database
    let db = setup_test_db().await;
    let app = create_test_app(db);

    // WHEN: Making a request with invalid API key
    let (status, _body) = send_request(
        app,
        "POST",
        "/sync",
        Some(json!({ "players": ["Steve"] })),
        Some("oxeye-sk-invalid12345678901234567890"),
    )
        .await;

    // THEN: Should return 401 Unauthorized
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_sync_replaces_entire_list() {
    // GIVEN: A server with existing players
    let db = setup_test_db().await;
    let api_key = helpers::generate_api_key();
    let api_key_hash = helpers::hash_api_key(&api_key);
    let guild_id = 123456789u64;
    let server_name = "TestServer".to_string();

    db.create_server(api_key_hash.clone(), server_name, guild_id)
        .await
        .expect("Failed to create server");

    let now = helpers::now();
    db.player_join(api_key_hash.clone(), "Steve".to_string(), now)
        .await
        .expect("Failed to add player");
    db.player_join(api_key_hash.clone(), "Alex".to_string(), now)
        .await
        .expect("Failed to add player");

    // WHEN: Syncing with completely different list
    let app = create_test_app(db.clone());
    let (status, _body) = send_request(
        app,
        "POST",
        "/sync",
        Some(json!({ "players": ["Notch", "Jeb", "Dinnerbone"] })),
        Some(&api_key),
    )
        .await;

    // THEN: Should return 200 OK
    assert_eq!(status, StatusCode::OK);

    // AND: The player list should be replaced
    let players = db
        .get_online_players(api_key_hash)
        .await
        .expect("Failed to get players");
    assert_eq!(players.len(), 3);
    assert!(players.contains(&"Notch".to_string()));
    assert!(players.contains(&"Jeb".to_string()));
    assert!(players.contains(&"Dinnerbone".to_string()));
    assert!(!players.contains(&"Steve".to_string()));
    assert!(!players.contains(&"Alex".to_string()));
}

#[tokio::test]
async fn test_sync_with_large_player_list() {
    // GIVEN: A valid server exists
    let db = setup_test_db().await;
    let api_key = helpers::generate_api_key();
    let api_key_hash = helpers::hash_api_key(&api_key);
    let guild_id = 123456789u64;
    let server_name = "TestServer".to_string();

    db.create_server(api_key_hash, server_name, guild_id)
        .await
        .expect("Failed to create server");

    let app = create_test_app(db);

    // WHEN: Syncing with large player list (1001 players - exceeds limit)
    let players: Vec<String> = (0..1001).map(|i| format!("Player{}", i)).collect();
    let (status, _body) = send_request(
        app,
        "POST",
        "/sync",
        Some(json!({ "players": players })),
        Some(&api_key),
    )
        .await;

    // THEN: Should return 400 Bad Request (list too large - validation enforced)
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_sync_with_oversized_payload() {
    // GIVEN: A valid server exists
    let db = setup_test_db().await;
    let api_key = helpers::generate_api_key();
    let api_key_hash = helpers::hash_api_key(&api_key);
    let guild_id = 123456789u64;
    let server_name = "TestServer".to_string();

    db.create_server(api_key_hash, server_name, guild_id)
        .await
        .expect("Failed to create server");

    let app = create_test_app(db);

    // WHEN: Sending a request with massive player names (> 1MB total payload)
    // Create player names that are each 10KB, with 150 of them = 1.5MB total
    let huge_name = "A".repeat(10 * 1024); // 10KB per name
    let players: Vec<String> = (0..150).map(|_| huge_name.clone()).collect(); // 1.5MB total

    let (status, _body) = send_request(
        app,
        "POST",
        "/sync",
        Some(json!({ "players": players })),
        Some(&api_key),
    )
        .await;

    // THEN: Should return 413 Payload Too Large
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn test_join_with_oversized_player_name() {
    // GIVEN: A valid server exists
    let db = setup_test_db().await;
    let api_key = helpers::generate_api_key();
    let api_key_hash = helpers::hash_api_key(&api_key);
    let guild_id = 123456789u64;
    let server_name = "TestServer".to_string();

    db.create_server(api_key_hash, server_name, guild_id)
        .await
        .expect("Failed to create server");

    let app = create_test_app(db);

    // WHEN: Making a request with a massive player name (> 1MB)
    let huge_name = "A".repeat(2 * 1024 * 1024); // 2MB player name
    let (status, _body) = send_request(
        app,
        "POST",
        "/join",
        Some(json!({ "player": huge_name })),
        Some(&api_key),
    )
        .await;

    // THEN: Should return 413 Payload Too Large
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}

// =============================================================================
// STATUS ENDPOINT TESTS
// =============================================================================

#[tokio::test]
async fn test_status_authenticated() {
    // GIVEN: A connected server
    let db = setup_test_db().await;
    let api_key = helpers::generate_api_key();
    let api_key_hash = helpers::hash_api_key(&api_key);
    let guild_id = 123456789u64;
    let server_name = "TestServer".to_string();

    db.create_server(api_key_hash, server_name, guild_id)
        .await
        .expect("Failed to create server");

    let app = create_test_app(db);

    // WHEN: Making a GET request to /status with valid API key
    let (status, _body) = send_request(app, "GET", "/status", None, Some(&api_key)).await;

    // THEN: Should return 200 OK
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn test_status_invalid_api_key() {
    // GIVEN: An empty database
    let db = setup_test_db().await;
    let app = create_test_app(db);

    // WHEN: Making a GET request to /status with invalid API key
    let (status, body) = send_request(
        app,
        "GET",
        "/status",
        None,
        Some("oxeye-sk-invalid12345678901234567890"),
    )
        .await;

    // THEN: Should return 401 Unauthorized
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(body.get("error").is_some());
}

#[tokio::test]
async fn test_status_without_authorization() {
    // GIVEN: A running application
    let db = setup_test_db().await;
    let app = create_test_app(db);

    // WHEN: Making a GET request to /status without Authorization header
    let request = Request::builder()
        .uri("/status")
        .method("GET")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    let status = response.status();

    // THEN: Should return 400 Bad Request (missing required header)
    assert!(
        status.is_client_error(),
        "Expected client error, got {}",
        status
    );
}

// =============================================================================
// DISCONNECT ENDPOINT TESTS
// =============================================================================

#[tokio::test]
async fn test_disconnect_success() {
    // GIVEN: A connected server with players
    let db = setup_test_db().await;
    let api_key = helpers::generate_api_key();
    let api_key_hash = helpers::hash_api_key(&api_key);
    let guild_id = 123456789u64;
    let server_name = "TestServer".to_string();

    db.create_server(api_key_hash.clone(), server_name, guild_id)
        .await
        .expect("Failed to create server");

    // Add some players
    let now = helpers::now();
    db.player_join(api_key_hash.clone(), "Steve".to_string(), now)
        .await
        .expect("Failed to add player");

    let app = create_test_app(db.clone());

    // WHEN: Making a POST request to /disconnect with valid API key
    let (status, _body) = send_request(app, "POST", "/disconnect", None, Some(&api_key)).await;

    // THEN: Should return 200 OK
    assert_eq!(status, StatusCode::OK);

    // AND: Server should be deleted
    let server = db
        .get_server_by_api_key(api_key_hash.clone())
        .await
        .expect("Query failed");
    assert!(server.is_none());

    // AND: Players should be deleted (cascade)
    let players = db
        .get_online_players(api_key_hash)
        .await
        .expect("Query failed");
    assert!(players.is_empty());
}

#[tokio::test]
async fn test_disconnect_with_invalid_api_key() {
    // GIVEN: An empty database
    let db = setup_test_db().await;
    let app = create_test_app(db);

    // WHEN: Making a request with invalid API key
    let (status, body) = send_request(
        app,
        "POST",
        "/disconnect",
        None,
        Some("oxeye-sk-invalid12345678901234567890"),
    )
        .await;

    // THEN: Should return 401 Unauthorized
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(body.get("error").is_some());
}

#[tokio::test]
async fn test_disconnect_without_authorization() {
    // GIVEN: A running application
    let db = setup_test_db().await;
    let app = create_test_app(db);

    // WHEN: Making a request without Authorization header
    let request = Request::builder()
        .uri("/disconnect")
        .method("POST")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    let status = response.status();

    // THEN: Should return 400 Bad Request (missing required header)
    assert!(
        status.is_client_error(),
        "Expected client error, got {}",
        status
    );
}

#[tokio::test]
async fn test_disconnect_twice() {
    // GIVEN: A connected server
    let db = setup_test_db().await;
    let api_key = helpers::generate_api_key();
    let api_key_hash = helpers::hash_api_key(&api_key);
    let guild_id = 123456789u64;
    let server_name = "TestServer".to_string();

    db.create_server(api_key_hash, server_name, guild_id)
        .await
        .expect("Failed to create server");

    // First disconnect
    let app = create_test_app(db.clone());
    let (status, _) = send_request(app, "POST", "/disconnect", None, Some(&api_key)).await;
    assert_eq!(status, StatusCode::OK);

    // WHEN: Trying to disconnect again with same API key
    let app = create_test_app(db);
    let (status, _) = send_request(app, "POST", "/disconnect", None, Some(&api_key)).await;

    // THEN: Should return 401 Unauthorized (key no longer valid)
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

// =============================================================================
// INTEGRATION TESTS - COMPLETE USER FLOWS
// =============================================================================

#[tokio::test]
async fn test_complete_server_lifecycle() {
    // GIVEN: A fresh database
    let db = setup_test_db().await;
    let code = helpers::generate_code();
    let guild_id = 123456789u64;
    let server_name = "MyServer".to_string();
    let now = helpers::now();

    // Step 1: Create pending link
    db.create_pending_link(code.clone(), guild_id, server_name, now)
        .await
        .expect("Failed to create pending link");

    // Step 2: Connect and get API key
    let app = create_test_app(db.clone());
    let (status, body) =
        send_request(app, "POST", "/connect", Some(json!({ "code": code })), None).await;
    assert_eq!(status, StatusCode::CREATED);
    let api_key = body["api_key"].as_str().unwrap().to_string();

    // Step 3: Player joins
    let app = create_test_app(db.clone());
    let (status, _) = send_request(
        app,
        "POST",
        "/join",
        Some(json!({ "player": "Steve" })),
        Some(&api_key),
    )
        .await;
    assert_eq!(status, StatusCode::OK);

    // Step 4: Another player joins
    let app = create_test_app(db.clone());
    let (status, _) = send_request(
        app,
        "POST",
        "/join",
        Some(json!({ "player": "Alex" })),
        Some(&api_key),
    )
        .await;
    assert_eq!(status, StatusCode::OK);

    // Step 5: One player leaves
    let app = create_test_app(db.clone());
    let (status, _) = send_request(
        app,
        "POST",
        "/leave",
        Some(json!({ "player": "Steve" })),
        Some(&api_key),
    )
        .await;
    assert_eq!(status, StatusCode::OK);

    // Step 6: Sync with new player list
    let app = create_test_app(db.clone());
    let (status, _) = send_request(
        app,
        "POST",
        "/sync",
        Some(json!({ "players": ["Alex", "Notch", "Jeb"] })),
        Some(&api_key),
    )
        .await;
    assert_eq!(status, StatusCode::OK);

    // Verify final state
    let api_key_hash = helpers::hash_api_key(&api_key);
    let players = db
        .get_online_players(api_key_hash)
        .await
        .expect("Failed to get players");
    assert_eq!(players.len(), 3);
}

#[tokio::test]
async fn test_multiple_servers_in_same_guild() {
    // GIVEN: A guild with two servers
    let db = setup_test_db().await;
    let guild_id = 123456789u64;
    let now = helpers::now();

    // Create first server
    let code1 = helpers::generate_code();
    db.create_pending_link(code1.clone(), guild_id, "Server1".to_string(), now)
        .await
        .expect("Failed to create pending link 1");

    let app = create_test_app(db.clone());
    let (status, body) = send_request(
        app,
        "POST",
        "/connect",
        Some(json!({ "code": code1 })),
        None,
    )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let api_key1 = body["api_key"].as_str().unwrap().to_string();

    // Create second server
    let code2 = helpers::generate_code();
    db.create_pending_link(code2.clone(), guild_id, "Server2".to_string(), now)
        .await
        .expect("Failed to create pending link 2");

    let app = create_test_app(db.clone());
    let (status, body) = send_request(
        app,
        "POST",
        "/connect",
        Some(json!({ "code": code2 })),
        None,
    )
        .await;
    assert_eq!(status, StatusCode::CREATED);
    let api_key2 = body["api_key"].as_str().unwrap().to_string();

    // WHEN: Different players join each server
    let app = create_test_app(db.clone());
    let (status, _) = send_request(
        app,
        "POST",
        "/join",
        Some(json!({ "player": "Steve" })),
        Some(&api_key1),
    )
        .await;
    assert_eq!(status, StatusCode::OK);

    let app = create_test_app(db.clone());
    let (status, _) = send_request(
        app,
        "POST",
        "/join",
        Some(json!({ "player": "Alex" })),
        Some(&api_key2),
    )
        .await;
    assert_eq!(status, StatusCode::OK);

    // THEN: Each server should have its own player list
    let hash1 = helpers::hash_api_key(&api_key1);
    let players1 = db
        .get_online_players(hash1)
        .await
        .expect("Failed to get players");
    assert_eq!(players1.len(), 1);
    assert_eq!(players1[0], "Steve");

    let hash2 = helpers::hash_api_key(&api_key2);
    let players2 = db
        .get_online_players(hash2)
        .await
        .expect("Failed to get players");
    assert_eq!(players2.len(), 1);
    assert_eq!(players2[0], "Alex");
}

#[tokio::test]
async fn test_api_key_isolation() {
    // GIVEN: Two different servers
    let db = setup_test_db().await;
    let now = helpers::now();

    // Server 1
    let code1 = helpers::generate_code();
    db.create_pending_link(code1.clone(), 111, "Server1".to_string(), now)
        .await
        .expect("Failed to create pending link 1");
    let app = create_test_app(db.clone());
    let (_, body) = send_request(
        app,
        "POST",
        "/connect",
        Some(json!({ "code": code1 })),
        None,
    )
        .await;
    let api_key1 = body["api_key"].as_str().unwrap().to_string();

    // Server 2
    let code2 = helpers::generate_code();
    db.create_pending_link(code2.clone(), 222, "Server2".to_string(), now)
        .await
        .expect("Failed to create pending link 2");
    let app = create_test_app(db.clone());
    let (_, body) = send_request(
        app,
        "POST",
        "/connect",
        Some(json!({ "code": code2 })),
        None,
    )
        .await;
    let _api_key2 = body["api_key"].as_str().unwrap().to_string();

    // WHEN: Server 1 tries to use Server 2's endpoint with wrong API key
    let app = create_test_app(db.clone());
    let (status, _) = send_request(
        app,
        "POST",
        "/join",
        Some(json!({ "player": "Hacker" })),
        Some(&api_key1), // Using Server 1's key for Server 2's player
    )
        .await;

    // THEN: Should succeed (each server has its own player list)
    // This is expected behavior - API keys are for server identification, not authorization
    assert_eq!(status, StatusCode::OK);
}
