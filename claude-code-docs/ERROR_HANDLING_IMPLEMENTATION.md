# Structured Error Handling Implementation

## Summary

Implemented comprehensive error handling with JSON error responses and structured logging. Errors now return user-friendly messages while logging detailed information server-side for debugging.

## Changes Made

### New Files

**`oxeye-backend/src/error.rs`** (98 lines)
- `ErrorResponse` struct for JSON error responses
- `AppError` enum for application-level errors
- `IntoResponse` implementation for automatic error conversion
- Structured logging with `tracing`
- User-friendly error messages
- Security: No internal details exposed to clients

**`oxeye-backend/tests/error_response_tests.rs`** (5 tests)
- Tests error response format and content
- Verifies no internal details are leaked
- Validates user-friendly messages

### Modified Files

**`oxeye-backend/src/lib.rs`**
- Added `mod error;`

**`oxeye-backend/src/routes.rs`** (Complete rewrite)
- Changed return types from `StatusCode` to `Result<impl IntoResponse, AppError>`
- Simplified error handling with `?` operator
- Removed manual `match` statements
- Much cleaner, more idiomatic Rust code

**`oxeye-backend/src/main.rs`**
- Added `tracing_subscriber` initialization
- Added startup/server info logging

## Before vs After

### Before (Generic Errors)

```bash
# Client sees
$ curl -X POST /join -d '{"player":""}' -H "Authorization: Bearer $KEY"
< HTTP/1.1 400 Bad Request
<
(empty body)
```

```rust
// Server logs
(nothing - no logging)
```

### After (Structured Errors)

```bash
# Client sees
$ curl -X POST /join -d '{"player":""}' -H "Authorization: Bearer $KEY"
< HTTP/1.1 400 Bad Request
< Content-Type: application/json
<
{
  "error": "Player name cannot be empty"
}
```

```rust
// Server logs
2024-01-15T10:30:45.123Z WARN validation_error="Player name cannot be empty"
```

## Error Response Format

All errors now return JSON with this structure:

```json
{
  "error": "User-friendly error message",
  "details": "Optional additional context"
}
```

### HTTP Status Codes Mapped

| Error Type | Status Code | Example Message |
|------------|-------------|-----------------|
| **Validation Errors** | 400 Bad Request | "Player name too long (max 16 characters, got 17)" |
| **Not Found** | 404 Not Found | "Connection code not found or expired" |
| **Unauthorized** | 401 Unauthorized | "Invalid or expired API key" |
| **Conflict** | 409 Conflict | "A server with this name already exists" |
| **Internal Errors** | 500 Internal Server Error | "An internal error occurred. Please try again later." |

## Structured Logging

### What Gets Logged

**Validation Errors** (WARN level):
```
validation_error="Player name too long (max 16 characters, got 17)"
```

**Database Errors** (ERROR level):
```
db_err=DbError::InvalidApiKey "Database error occurred"
```

**Internal Errors** (ERROR level):
```
"Internal database error: Connection(Error { ... })"
```

### What Gets Logged vs Sent to Client

| Error Type | Logged Server-Side | Sent to Client |
|------------|-------------------|----------------|
| Validation | Full error details | Same as logged |
| DB - Known | Error type | User-friendly message |
| DB - Internal | Full SQL error | Generic "internal error" |

## Security Improvements

### What We DON'T Expose

- ❌ SQL errors
- ❌ Stack traces
- ❌ Internal library names (rusqlite, etc.)
- ❌ Database schema details
- ❌ File paths
- ❌ Panic messages

### What We DO Return

- ✅ User-friendly error messages
- ✅ Validation rule violations
- ✅ Actionable information
- ✅ HTTP status codes
- ✅ Consistent JSON format

## Code Quality Improvements

### Simplified Route Handlers

**Before:**
```rust
pub(crate) async fn join(
    State(state): State<Arc<AppState>>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
    Json(payload): Json<TransitionRequest>,
) -> StatusCode {
    if let Err(_) = validation::validate_player_name(&payload.player) {
        return StatusCode::BAD_REQUEST;
    }

    let api_key = auth.token().to_string();
    let api_key_hash = crate::helpers::hash_api_key(&api_key);

    match state.db.player_join(api_key_hash, payload.player, now()).await {
        Ok(_) => StatusCode::OK,
        Err(e) => match e {
            oxeye_db::DbError::InvalidApiKey => StatusCode::UNAUTHORIZED,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        },
    }
}
```

**After:**
```rust
pub(crate) async fn join(
    State(state): State<Arc<AppState>>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
    Json(payload): Json<TransitionRequest>,
) -> Result<impl IntoResponse, AppError> {
    validation::validate_player_name(&payload.player)?;

    let api_key = auth.token().to_string();
    let api_key_hash = crate::helpers::hash_api_key(&api_key);

    state.db.player_join(api_key_hash, payload.player, now()).await?;

    Ok(StatusCode::OK)
}
```

**Benefits:**
- 50% less code
- No manual error matching
- Automatic logging
- Consistent error format
- Idiomatic Rust with `?` operator

## Test Coverage

### New Tests (5)

1. **`test_error_response_format_for_nonexistent_code`**
   - Verifies JSON error structure
   - Checks user-friendly messages

2. **`test_error_response_for_invalid_api_key`**
   - Tests 401 error responses
   - Validates helpful auth error messages

3. **`test_error_response_for_validation_failure`**
   - Tests 400 error responses
   - Verifies validation errors are clear

4. **`test_error_response_doesnt_expose_internals`**
   - Security test: ensures no SQL/internal details leaked
   - Critical for preventing information disclosure

5. **`test_validation_error_has_details`**
   - Verifies specific validation errors
   - Tests character validation messages

### Total Tests: 50 ✅
```
├─ Error response: 5 tests (NEW)
├─ Integration: 31 tests
└─ Validation unit: 14 tests
```

## Error Examples

### Validation Error
```http
POST /join
Authorization: Bearer oxeye-sk-abc...

{
  "player": "ThisNameIsWayTooLong17"
}
```

**Response:**
```http
HTTP/1.1 400 Bad Request
Content-Type: application/json

{
  "error": "Player name too long (max 16 characters, got 24)"
}
```

### Authentication Error
```http
POST /join
Authorization: Bearer invalid-key

{
  "player": "Steve"
}
```

**Response:**
```http
HTTP/1.1 401 Unauthorized
Content-Type: application/json

{
  "error": "Invalid or expired API key"
}
```

### Not Found Error
```http
POST /connect

{
  "code": "oxeye-doesnotexist"
}
```

**Response:**
```http
HTTP/1.1 404 Not Found
Content-Type: application/json

{
  "error": "Connection code not found or expired"
}
```

### Internal Error (Safe)
```http
POST /join
(causes database connection error)
```

**Response:**
```http
HTTP/1.1 500 Internal Server Error
Content-Type: application/json

{
  "error": "An internal error occurred. Please try again later."
}
```

**Server Log:**
```
ERROR Internal database error: Connection(Error { kind: ConnectionFailed, ... })
```

## Logging Format

Logs use structured format for easy parsing:

```
2024-01-15T10:30:45.123Z INFO  Starting Oxeye backend server...
2024-01-15T10:30:45.456Z INFO  Server listening on 0.0.0.0:3000
2024-01-15T10:31:15.789Z WARN  validation_error="Player name too long (max 16 characters, got 17)"
2024-01-15T10:31:20.012Z ERROR db_err=InvalidApiKey "Database error occurred"
```

Benefits:
- Easy to grep/search
- Machine-parseable
- Includes timestamps
- Appropriate log levels

## Architectural Issue Resolved

**Issue #7: Generic Error Responses** - ✅ FIXED

Status changed from **LOW severity** to **RESOLVED**.

### What Was Fixed
- ✅ JSON error responses with user-friendly messages
- ✅ Structured logging for server-side debugging
- ✅ Security: No internal details exposed
- ✅ Consistent error format across all endpoints
- ✅ Simplified code with `?` operator
- ✅ Comprehensive error response tests

### Benefits

**For Developers:**
- Structured logs for debugging
- Clear error types in logs
- Easy to trace issues
- Consistent patterns

**For API Clients:**
- Clear, actionable error messages
- Consistent JSON format
- Appropriate HTTP status codes
- No confusing internal errors

**For Security:**
- No information leakage
- Safe error messages
- Internal details stay server-side

## Future Enhancements

### Error Codes (Optional)
Could add error codes for programmatic handling:

```json
{
  "error": "Player name too long",
  "code": "VALIDATION_PLAYER_NAME_TOO_LONG",
  "details": "Maximum 16 characters allowed, got 24"
}
```

### Request IDs (Optional)
For distributed tracing:

```json
{
  "error": "An internal error occurred",
  "request_id": "req_abc123xyz"
}
```

### Internationalization (Optional)
Error messages in multiple languages:

```json
{
  "error": "Player name too long",
  "error_i18n": {
    "en": "Player name too long",
    "es": "Nombre de jugador demasiado largo"
  }
}
```

## References

- [Axum error handling](https://docs.rs/axum/latest/axum/error_handling/index.html)
- [Tracing documentation](https://docs.rs/tracing/latest/tracing/)
- [OWASP - Error Handling](https://cheatsheetseries.owasp.org/cheatsheets/Error_Handling_Cheat_Sheet.html)
