# Architectural Issues and Code Review

## Critical Issues

### 1. ✅ FIXED - HTTP Method Mismatch (lib.rs:17-20)
**Issue**: All routes except `/health` were registered as GET but accept JSON request bodies.

**Status**: ✅ **FIXED** - All routes now use POST method

**Fixed Code**:
```rust
.route("/connect", post(routes::connect))
.route("/join", post(routes::join))
.route("/leave", post(routes::leave))
.route("/sync", post(routes::sync))
```

**Changes Made**:
- Changed all routes from GET to POST in lib.rs
- Updated all 26 integration tests to use POST
- All tests passing with new HTTP methods

**Severity**: HIGH - Was causing REST compliance issues (now resolved)

---

## Security Issues

### 2. Missing Input Validation
**Locations**: All route handlers (routes.rs)

**Issues**:
- No validation for empty player names
- No validation for player name length (potential DOS via very long names)
- No validation for player name format (could contain special chars, SQL-like strings)
- No validation for code format in `/connect`
- No validation for array size in `/sync` (potential DOS via massive arrays)

**Recommendation**: Add validation functions:
```rust
fn validate_player_name(name: &str) -> Result<(), &'static str> {
    if name.is_empty() {
        return Err("Player name cannot be empty");
    }
    if name.len() > 16 {  // Minecraft usernames are max 16 chars
        return Err("Player name too long");
    }
    if !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Err("Invalid characters in player name");
    }
    Ok(())
}
```

**Severity**: MEDIUM-HIGH - Can cause database issues, DOS attacks

### 3. ✅ FIXED - API Key Type Inefficiency (helpers.rs:14)
**Issue**: `hash_api_key` took `&String` instead of `&str`

**Status**: ✅ **FIXED** - Function now accepts `&str`

**Fixed Code**:
```rust
pub fn hash_api_key(key: &str) -> String {
  format!("{:x}", Sha256::digest(key.as_bytes()))
}
```

**Changes Made**:
- Changed parameter from `&String` to `&str`
- Made function public for testing
- Fixed format! macro spacing

**Severity**: LOW - Code quality improvement (now resolved)

### 4. No Rate Limiting
**Issue**: No protection against abuse

**Problem**:
- Attackers can spam `/connect` to consume pending links
- Attackers can spam `/join`/`/leave` to create database load
- No per-IP or per-API-key rate limiting

**Recommendation**: Add tower-governor or similar rate limiting middleware

**Severity**: MEDIUM - Can enable DOS attacks

### 5. No Request Size Limits
**Issue**: No limits on JSON payload size

**Problem**:
- `/sync` endpoint accepts arbitrary-sized Vec<String>
- Attackers can send massive payloads causing memory exhaustion

**Recommendation**: Add request body size limits in middleware

**Severity**: MEDIUM - DOS vulnerability

### 6. Potential Timing Attacks
**Issue**: API key hash comparison may not be constant-time

**Problem**: Database lookup timing could leak information about valid API keys

**Recommendation**: While SQLite comparison is likely safe, consider using constant-time comparison for hashes if paranoid

**Severity**: LOW - Theoretical attack, requires precise timing measurements

---

## Error Handling Issues

### 7. Generic Error Responses (routes.rs)
**Issue**: All database errors map to 500 Internal Server Error

**Current Code**:
```rust
Err(e) => match e {
    oxeye_db::DbError::InvalidApiKey => StatusCode::UNAUTHORIZED,
    _ => StatusCode::INTERNAL_SERVER_ERROR,  // Too broad
}
```

**Problem**:
- No logging of specific error types for debugging
- Client cannot distinguish between different failure modes
- Makes troubleshooting difficult

**Recommendation**: Add structured logging:
```rust
Err(e) => {
    match e {
        oxeye_db::DbError::InvalidApiKey => StatusCode::UNAUTHORIZED,
        _ => {
            tracing::error!(?e, "Database error in join endpoint");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}
```

**Severity**: LOW - Quality of life issue

### 8. No Request Logging Middleware
**Issue**: No automatic logging of requests

**Problem**: Hard to debug issues, no audit trail

**Recommendation**: Add tower-http TraceLayer

**Severity**: LOW - Operational issue

---

## Code Quality Issues

### 9. Inconsistent Error Handling Patterns
**Issue**: `/connect` uses `Result<impl IntoResponse, StatusCode>` while others use `StatusCode`

**Observation**: This is actually fine - `/connect` needs to return JSON on success, others only return status codes

**Severity**: NONE - Intentional design

### 10. Missing CORS Configuration
**Issue**: tower-http dependency includes CORS support but not configured

**Problem**: If frontend is on different domain, requests will fail

**Recommendation**: Add CorsLayer if cross-origin access is needed:
```rust
use tower_http::cors::CorsLayer;

let app = Router::new()
    .route(...)
    .layer(CorsLayer::permissive())  // or more restrictive config
    .with_state(state);
```

**Severity**: LOW - Only matters if cross-origin access is needed

---

## Design Issues

### 11. Missing Health Check Details
**Issue**: `/health` endpoint only returns 200 OK

**Problem**: Doesn't verify database connectivity

**Recommendation**: Add database health check:
```rust
async fn health(State(state): State<Arc<AppState>>) -> StatusCode {
    match state.db.health_check().await {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}
```

**Severity**: LOW - Operational improvement

### 12. No Timeout Handling
**Issue**: No timeouts on database operations

**Problem**: Hung database operations can cause request to hang indefinitely

**Recommendation**: Add timeout middleware or per-operation timeouts

**Severity**: LOW-MEDIUM - Can cause cascading failures

### 13. Unwrap Usage in Production Code (main.rs)
**Issue**: Multiple `.unwrap()` calls in main function

**Current Code**:
```rust
db: oxeye_db::Database::open("oxeye.db").await.unwrap(),
let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
axum::serve(listener, app).await.unwrap();
```

**Problem**: Panics on error instead of graceful error handling

**Recommendation**: Use proper error handling with `?` operator and return Result from main

**Severity**: MEDIUM - Poor error handling in production

---

## Missing Features

### 14. No Graceful Shutdown
**Issue**: Server doesn't handle SIGTERM/SIGINT gracefully

**Problem**: Database connections may not close cleanly on shutdown

**Recommendation**: Add signal handling with tokio::signal

**Severity**: LOW - Operational improvement

### 15. No Metrics/Observability
**Issue**: No metrics exported (request counts, latency, etc.)

**Recommendation**: Add prometheus metrics or similar

**Severity**: LOW - Operational improvement

---

## Summary

**Fixed**: 2 ✅
- HTTP method mismatch (HIGH severity)
- API key type inefficiency (LOW severity)

**Remaining Issues**:
- **Critical**: 0
- **High**: 0
- **Medium**: 4 (Input validation, rate limiting, request size limits, unwrap usage)
- **Low**: 9 (Various quality and operational improvements)

**Completed Fixes**:
1. ✅ Changed routes to POST methods
2. ✅ Fixed API key parameter type

**Remaining Priority Fix List**:
1. Add input validation for all user inputs
2. Add request size limits
3. Fix .unwrap() calls in main.rs
4. Add rate limiting
5. Add structured error logging
6. Improve health check endpoint
7. Add CORS if needed

**Testing**:
- ✅ 26 comprehensive integration tests created
- ✅ All routes tested (success, error, edge cases)
- ✅ TDD approach used throughout
- ✅ All tests passing
