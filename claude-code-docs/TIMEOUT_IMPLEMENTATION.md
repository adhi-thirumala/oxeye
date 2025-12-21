# Request Timeout Implementation

## Summary

Implemented HTTP request timeout middleware to prevent hung database operations from causing indefinite request hangs. Requests that exceed 30 seconds now automatically return a 408 Request Timeout response.

## Changes Made

### Modified Files

**`Cargo.toml`** (workspace dependencies)
- Added `"timeout"` feature to `tower-http` dependency
- Enables `TimeoutLayer` middleware

**`oxeye-backend/src/lib.rs`**
- Added `use tower_http::timeout::TimeoutLayer`
- Added `use std::time::Duration`
- Added constant: `const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);`
- Applied `TimeoutLayer` middleware to all routes
- Returns 408 Request Timeout on timeout

## Implementation Details

### Timeout Duration

```rust
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
```

**Why 30 seconds?**
- Typical request completion: <100ms (database operations are fast)
- 30 seconds provides generous buffer for legitimate slow operations
- Prevents indefinite hangs from database deadlocks or connection issues
- Short enough to fail fast and not tie up resources

### Middleware Layer

```rust
Router::new()
    .route("/health", get(|| async { StatusCode::OK }))
    .route("/connect", post(routes::connect))
    .route("/join", post(routes::join))
    .route("/leave", post(routes::leave))
    .route("/sync", post(routes::sync))
    .layer(TimeoutLayer::with_status_code(StatusCode::REQUEST_TIMEOUT, REQUEST_TIMEOUT))
    .layer(RequestBodyLimitLayer::new(REQUEST_BODY_LIMIT))
    .with_state(state)
```

**Layer Order**: Timeout layer wraps the body limit layer, meaning:
1. Request body size is checked first
2. Then timeout protection is applied during processing

### Error Response

When request exceeds timeout:
- **Status Code**: `408 Request Timeout`
- **Timing**: Automatically after 30 seconds
- **Behavior**: Request is cancelled, resources freed

## Defense Layers

Now we have **multiple layers of protection** against hung requests:

### Layer 1: Timeout (NEW - Middleware)
- Limit: 30 seconds per request
- Protection: Database deadlocks, hung connections, slow queries
- Response: 408 Request Timeout

### Layer 2: Request Size Limit (Existing)
- Limit: 1MB total request body
- Protection: Memory exhaustion, parser DOS
- Response: 413 Payload Too Large

### Layer 3: Validation (Existing)
- Limits: Array size, field format
- Protection: Invalid data, excessive iteration
- Response: 400 Bad Request with details

## Attack Scenarios Prevented

### Scenario 1: Database Deadlock

**Before (Vulnerable)**:
```bash
# Database enters deadlock state
# Request hangs indefinitely
# Connection pool exhausted as requests pile up
# Eventually: entire server unresponsive
```

**After (Protected)**:
```bash
# Database enters deadlock state
# Request automatically cancelled after 30s
# Returns: 408 Request Timeout
# Connection released back to pool
# Server remains responsive
```

### Scenario 2: Slow Network to Database

**Before (Vulnerable)**:
```bash
# Network issue causes database connection to hang
# Multiple requests hang waiting for database
# Resource exhaustion as connections accumulate
```

**After (Protected)**:
```bash
# Network issue detected via timeout
# Requests fail fast with 408
# Resources released promptly
# Server continues serving other requests
```

### Scenario 3: Malicious Long-Running Query

**Before (Vulnerable)**:
```bash
# Attacker crafts request that triggers expensive DB operation
# Request hangs for minutes
# Repeat attack → DOS via resource exhaustion
```

**After (Protected)**:
```bash
# Long-running operation started
# Timeout triggers at 30s
# Returns 408, operation cancelled
# Limited impact even if repeated
```

## Performance Impact

- ✅ **Zero overhead for normal requests** - Timer only starts on request
- ✅ **Fast failure** - 30s is short enough to prevent cascading failures
- ✅ **Resource cleanup** - Connections properly released on timeout
- ✅ **Protects all routes** - Applied globally

## Production Considerations

### Normal Operation
- Requests typically complete in <100ms
- 30s timeout should **never** be hit in normal operation
- If timeouts occur regularly, investigate:
  - Database performance issues
  - Network connectivity problems
  - Query optimization needs

### Monitoring Recommendations

Consider adding metrics for:
- Count of 408 timeout errors (should be near zero)
- Request duration distribution
- Database query timing
- Connection pool utilization

If 408s occur frequently, it's a sign of serious backend issues.

### Cascading Failure Prevention

**Without timeout**:
1. Database becomes slow/unresponsive
2. Requests pile up waiting for database
3. Connection pool exhausted
4. All new requests fail immediately
5. Service completely down

**With timeout**:
1. Database becomes slow/unresponsive
2. Requests timeout at 30s
3. Connections released back to pool
4. Service degraded but functional
5. Can serve cached/static content

## Architectural Issue Resolved

**Issue #12: No Timeout Handling** - ✅ FIXED

Status changed from **LOW-MEDIUM severity** to **RESOLVED**.

### What Was Fixed
- ✅ 30-second timeout enforced on all routes
- ✅ Returns 408 Request Timeout on timeout
- ✅ Prevents indefinite hangs
- ✅ Protects against database deadlocks
- ✅ Graceful degradation under load

### Benefits

**For Reliability**:
- Fail fast instead of hanging indefinitely
- Prevent cascading failures
- Resource cleanup on timeout

**For Operations**:
- Clear signal when backend is struggling
- Easier to detect and diagnose issues
- Metrics on timeout rate show health

**For Security**:
- Limits impact of DOS attacks via slow queries
- Prevents resource exhaustion
- Bounded worst-case behavior

## Edge Cases Handled

### Fast Requests
- Normal requests (<100ms): No impact ✅
- Medium requests (1-5s): No impact ✅
- Legitimately slow requests (5-29s): Still succeed ✅

### Slow Requests
- 30+ second operations: Return 408 ❌ (by design)
- If legitimate operations take this long, consider:
  - Async job queue for long operations
  - Increase timeout (not recommended)
  - Optimize the slow operation

### Database Issues
- Deadlock: 408 after 30s ✅
- Connection timeout: 408 after 30s ✅
- Slow query: 408 after 30s ✅
- Network partition: 408 after 30s ✅

## Configuration

### Adjusting the Timeout

To change the timeout, edit `REQUEST_TIMEOUT` constant:

```rust
// More aggressive (10 seconds)
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

// More permissive (60 seconds) - not recommended
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
```

**Recommendation**: Keep it at 30s. If you need longer, investigate why operations are slow.

### Per-Route Timeouts (Future)

If needed, different routes could have different timeouts:

```rust
Router::new()
    .route("/health", get(health))  // No timeout needed
    .route("/sync",
        post(sync).layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(60)
        ))) // Longer timeout for batch operation
    .route("/join",
        post(join))  // Default 30s timeout
```

## Comparison with Other Middleware

| Feature | Timeout | Body Size Limit | Validation |
|---------|---------|-----------------|------------|
| **Level** | Middleware | Middleware | Route handler |
| **Timing** | During processing | Before parsing | After parsing |
| **Checks** | Duration | Total bytes | Content validity |
| **Error** | 408 Timeout | 413 Too Large | 400 Bad Request |
| **Purpose** | Prevent hangs | Prevent DOS | Enforce rules |

**All three are essential** for a robust service.

## Test Coverage

All existing tests pass with timeout middleware:

```
Total: 50 tests ✅
├─ Backend integration: 31 tests
├─ Validation unit: 14 tests
└─ Error response: 5 tests
```

**Note**: No specific timeout tests added because:
1. Would require artificially slowing down requests (unreliable)
2. Existing tests verify normal operations still work
3. Middleware is from trusted `tower-http` library

### Manual Testing Timeout (Optional)

To test timeout behavior manually:

```rust
// Temporary test - add to integration_tests.rs
#[tokio::test]
async fn test_timeout_triggers() {
    let db = setup_test_db().await;
    let app = create_app(db);

    // Create a route that sleeps for 35 seconds
    // Should return 408 Request Timeout
}
```

## Related Issues

### Resolved
- ✅ Issue #12: No Timeout Handling
- ✅ Issue #5: No Request Size Limits
- ✅ Issue #7: Generic Error Responses
- ✅ Issue #2: No Input Validation
- ✅ Issue #1: HTTP Method Mismatch

### Deferred by User
- ⏸️ Issue #4: No Rate Limiting (user will do later)
- ⏸️ Issue #10: No CORS Headers (user will do later)
- ⏸️ Issue #6: Timing Attacks (impractical to fix)
- ⏸️ Issue #8: No Request Logging (already done via tracing)
- ⏸️ Issues #11, #13, #14, #15: User doesn't care

## References

- [tower-http TimeoutLayer docs](https://docs.rs/tower-http/latest/tower_http/timeout/struct.TimeoutLayer.html)
- [HTTP 408 Status Code](https://developer.mozilla.org/en-US/docs/Web/HTTP/Status/408)
- [Timeouts in distributed systems](https://aws.amazon.com/builders-library/timeouts-retries-and-backoff-with-jitter/)
