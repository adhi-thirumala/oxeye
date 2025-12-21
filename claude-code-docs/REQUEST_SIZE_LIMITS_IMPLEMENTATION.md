# Request Body Size Limits Implementation

## Summary

Implemented HTTP request body size limits at the middleware level to prevent DOS attacks via massive payloads. This complements our existing validation and provides defense-in-depth.

## Changes Made

### Modified Files

**`Cargo.toml`** (workspace dependencies)
- Added `"limit"` feature to `tower-http` dependency
- Enables `RequestBodyLimitLayer` middleware

**`oxeye-backend/src/lib.rs`**
- Added `RequestBodyLimitLayer` to the router
- Set limit to 1MB (configurable constant)
- Applied to all routes via middleware layer

**`oxeye-backend/tests/integration_tests.rs`**
- Added 2 new tests for oversized payload handling
- `test_sync_with_oversized_payload` - Tests 1.5MB payload rejection
- `test_join_with_oversized_player_name` - Tests 2MB single field rejection

## Implementation Details

### Request Size Limit

```rust
const REQUEST_BODY_LIMIT: usize = 1024 * 1024; // 1 MB
```

**Why 1MB?**
- Typical request: 1000 players × ~100 bytes = ~100KB
- 1MB provides 10x headroom for legitimate use
- Small enough to prevent memory exhaustion
- Large enough to avoid false positives

### Middleware Layer

```rust
Router::new()
    .route("/health", get(|| async { StatusCode::OK }))
    .route("/connect", post(routes::connect))
    .route("/join", post(routes::join))
    .route("/leave", post(routes::leave))
    .route("/sync", post(routes::sync))
    .layer(RequestBodyLimitLayer::new(REQUEST_BODY_LIMIT))  // Applied to all routes
    .with_state(state)
```

### Error Response

When payload exceeds limit:
- **Status Code**: `413 Payload Too Large`
- **Timing**: Rejected before deserialization (very fast)
- **Memory**: Prevents allocating massive buffers

## Defense Layers

Now we have **three layers of protection** against DOS attacks:

### Layer 1: Request Size Limit (NEW - Middleware)
- Limit: 1MB total request body
- Rejection: Before JSON parsing
- Protection: Memory exhaustion, parser DOS

### Layer 2: Validation - Array Size (Existing)
- Limit: 1000 players per sync request
- Rejection: After parsing, before DB
- Protection: Excessive iteration, DB load

### Layer 3: Validation - Field Format (Existing)
- Limit: 16 chars per player name
- Rejection: After parsing, before DB
- Protection: Invalid data, injection attacks

## Test Coverage

### New Tests (2)

**`test_sync_with_oversized_payload`**
```rust
// Create 150 players × 10KB each = 1.5MB payload
let huge_name = "A".repeat(10 * 1024);
let players: Vec<String> = (0..150).map(|_| huge_name.clone()).collect();
// Expected: 413 Payload Too Large
```

**`test_join_with_oversized_player_name`**
```rust
// Single 2MB player name
let huge_name = "A".repeat(2 * 1024 * 1024);
// Expected: 413 Payload Too Large
```

### All Tests Status
```
Total: 52 tests
├─ Backend integration: 31 tests ✅ (+2 new)
├─ Validation unit: 14 tests ✅
└─ Database unit: 7 tests ✅
```

## Attack Scenarios Prevented

### Before (Vulnerable)
```bash
# Attacker sends 100MB payload
curl -X POST /sync \
  -H "Authorization: Bearer $KEY" \
  -d '{"players": ["'$(python -c 'print("A" * 100000000)')'"]}'
# Result: Server allocates 100MB, potentially crashes
```

### After (Protected)
```bash
# Same attack
curl -X POST /sync \
  -H "Authorization: Bearer $KEY" \
  -d '{"players": ["'$(python -c 'print("A" * 100000000)')'"]}'
# Result: 413 Payload Too Large, no memory allocated
```

## Performance Impact

- ✅ **Minimal overhead** - Simple byte counter
- ✅ **Early rejection** - Before JSON parsing
- ✅ **No allocations** - Rejects before buffering
- ✅ **Protects all routes** - Applied globally

## Security Improvements

### DOS Prevention
1. **Memory Exhaustion**: Can't send GB-sized payloads
2. **Parser DOS**: JSON parser never sees oversized data
3. **Network DOS**: Reduced because connection is dropped early

### Defense in Depth
1. Middleware layer (request size)
2. Validation layer (array size, field format)
3. Database layer (constraints, transactions)

## Comparison with Validation

| Feature | Request Size Limit | Validation |
|---------|-------------------|------------|
| **Level** | Middleware | Route handler |
| **Timing** | Before parsing | After parsing |
| **Checks** | Total bytes | Array length, field format |
| **Error** | 413 Payload Too Large | 400 Bad Request |
| **Memory** | Prevents allocation | Validates allocated data |

**Both are needed!** They protect against different attack vectors.

## Configuration

### Adjusting the Limit

To change the limit, edit `REQUEST_BODY_LIMIT` constant:

```rust
// More restrictive (500KB)
const REQUEST_BODY_LIMIT: usize = 500 * 1024;

// More permissive (5MB)
const REQUEST_BODY_LIMIT: usize = 5 * 1024 * 1024;
```

### Per-Route Limits (Future)

If needed, we can apply different limits per route:

```rust
Router::new()
    .route("/health", get(health))
    .route("/sync",
        post(sync).layer(RequestBodyLimitLayer::new(2 * 1024 * 1024))) // 2MB
    .route("/join",
        post(join).layer(RequestBodyLimitLayer::new(512))) // 512 bytes
```

## Architectural Issue Resolved

**Issue #5: No Request Size Limits** - ✅ FIXED

Status changed from **MEDIUM severity** to **RESOLVED**.

### What Was Fixed
- ✅ Request body size limit enforced (1MB)
- ✅ Applied to all routes via middleware
- ✅ Returns 413 Payload Too Large for oversized requests
- ✅ Prevents memory exhaustion attacks
- ✅ Comprehensive test coverage

### Remaining Security Issues
- ⚠️ No rate limiting (Issue #4)
- ⚠️ No structured error logging (Issue #7)
- ⚠️ .unwrap() usage in main.rs (Issue #13)

## Edge Cases Handled

### Valid Large Requests
- 1000 players × 16 chars = ~16KB ✅ (well under limit)
- Reasonable JSON overhead ✅ (under limit)

### Invalid Oversized Requests
- Megabyte-sized player names ❌ (rejected)
- Thousands of players ❌ (rejected by validation if < 1MB, by size limit if > 1MB)
- Malformed huge payloads ❌ (rejected before parsing)

## Production Considerations

### Monitoring
Consider adding metrics for:
- Count of 413 errors (potential attack detection)
- Request size distribution
- Rejected payload sizes

### Logging
Could add structured logging:
```rust
.layer(RequestBodyLimitLayer::new(REQUEST_BODY_LIMIT))
.layer(TraceLayer::new_for_http())  // Logs rejections
```

### CDN/Proxy
If behind a CDN/proxy, ensure they also have size limits configured for defense-in-depth.

## References

- [tower-http RequestBodyLimitLayer docs](https://docs.rs/tower-http/latest/tower_http/limit/struct.RequestBodyLimitLayer.html)
- [Axum middleware guide](https://docs.rs/axum/latest/axum/middleware/index.html)
- [OWASP - DOS Prevention](https://cheatsheetseries.owasp.org/cheatsheets/Denial_of_Service_Cheat_Sheet.html)
