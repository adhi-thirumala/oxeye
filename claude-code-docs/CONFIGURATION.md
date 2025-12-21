# Configuration Guide

## Overview

All configuration values can be overridden using environment variables. If no environment variable is set, sensible defaults are used.

## Configuration Options

### Request Body Size Limit

**Environment Variable:** `REQUEST_BODY_LIMIT`
**Default:** `1048576` (1 MB)
**Type:** Integer (bytes)

Controls the maximum size of HTTP request bodies. Prevents DOS attacks via massive payloads.

**Example:**
```bash
# Set to 2 MB
export REQUEST_BODY_LIMIT=2097152

# Set to 512 KB
export REQUEST_BODY_LIMIT=524288
```

**Context:**
- Typical request: 1000 players × ~100 bytes = ~100KB
- Default 1MB provides 10x headroom for legitimate use
- Requests exceeding this limit return 413 Payload Too Large

---

### Request Timeout

**Environment Variable:** `REQUEST_TIMEOUT_SECS`
**Default:** `30` (seconds)
**Type:** Integer (seconds)

Controls how long a request can run before timing out. Prevents hung database operations from causing indefinite hangs.

**Example:**
```bash
# Set to 60 seconds
export REQUEST_TIMEOUT_SECS=60

# Set to 10 seconds (more aggressive)
export REQUEST_TIMEOUT_SECS=10
```

**Context:**
- Most requests complete in <100ms
- Default 30s provides generous buffer
- Requests exceeding this timeout return 408 Request Timeout
- If timeouts occur frequently in production, investigate backend performance

---

### Server Port

**Environment Variable:** `PORT`
**Default:** `3000`
**Type:** Integer (port number)

Controls which port the HTTP server listens on.

**Example:**
```bash
# Use port 8080
export PORT=8080

# Use port 80 (requires root/admin privileges)
export PORT=80
```

**Context:**
- Server binds to `0.0.0.0:<PORT>` (all interfaces)
- Commonly used ports: 3000 (default), 8080, 80, 443

---

### Database Path

**Environment Variable:** `DATABASE_PATH`
**Default:** `"oxeye.db"`
**Type:** String (file path)

Controls where the SQLite database file is stored.

**Example:**
```bash
# Use absolute path
export DATABASE_PATH=/var/lib/oxeye/database.db

# Use relative path in data directory
export DATABASE_PATH=./data/oxeye.db

# Use in-memory database (development only - data lost on restart)
export DATABASE_PATH=:memory:
```

**Context:**
- Relative paths are relative to the working directory
- Ensure the directory exists and is writable
- For production, use absolute paths

---

## Usage Examples

### Development (Defaults)

Just run the server - it will use all defaults:

```bash
cargo run
```

Output:
```
Starting Oxeye backend server...
Configuration: port=3000, db_path=oxeye.db, body_limit=1024KB, timeout=30s
Server listening on 0.0.0.0:3000
```

---

### Production with Custom Config

Set environment variables before running:

```bash
export PORT=8080
export DATABASE_PATH=/var/lib/oxeye/production.db
export REQUEST_TIMEOUT_SECS=15
export REQUEST_BODY_LIMIT=524288  # 512 KB

cargo run --release
```

Output:
```
Starting Oxeye backend server...
Configuration: port=8080, db_path=/var/lib/oxeye/production.db, body_limit=512KB, timeout=15s
Server listening on 0.0.0.0:8080
```

---

### Using .env File (Optional)

You can create a `.env` file (not currently supported - would need dotenv crate):

```bash
# .env
PORT=8080
DATABASE_PATH=/var/lib/oxeye/production.db
REQUEST_TIMEOUT_SECS=15
REQUEST_BODY_LIMIT=524288
```

**Note:** Currently the application does NOT automatically load `.env` files. You would need to:
1. Add `dotenv` crate to dependencies
2. Call `dotenv::dotenv().ok();` at the start of main()

Or use shell to load the file:
```bash
set -a
source .env
set +a
cargo run
```

---

### Docker Environment

In Docker Compose:

```yaml
version: '3.8'
services:
  oxeye-backend:
    image: oxeye-backend:latest
    environment:
      - PORT=3000
      - DATABASE_PATH=/data/oxeye.db
      - REQUEST_TIMEOUT_SECS=30
      - REQUEST_BODY_LIMIT=1048576
    volumes:
      - ./data:/data
    ports:
      - "3000:3000"
```

---

### Kubernetes ConfigMap

In Kubernetes:

```yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: oxeye-config
data:
  PORT: "3000"
  DATABASE_PATH: "/data/oxeye.db"
  REQUEST_TIMEOUT_SECS: "30"
  REQUEST_BODY_LIMIT: "1048576"
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: oxeye-backend
spec:
  template:
    spec:
      containers:
      - name: oxeye-backend
        image: oxeye-backend:latest
        envFrom:
        - configMapRef:
            name: oxeye-config
```

---

## Validation and Limits

### Request Body Limit

- **Minimum:** No enforced minimum (can be 0, but not recommended)
- **Maximum:** Limited by available memory
- **Recommended Range:** 512 KB - 5 MB

### Request Timeout

- **Minimum:** 1 second (lower values may cause spurious timeouts)
- **Maximum:** No enforced maximum
- **Recommended Range:** 10 - 60 seconds

### Port

- **Range:** 1 - 65535
- **Privileged Ports:** 1-1023 (require root/admin)
- **Commonly Used:** 3000, 8000, 8080, 80, 443

### Database Path

- Must be a valid file path or `:memory:` for in-memory database
- Directory must exist and be writable
- For production, use absolute paths

---

## Configuration Precedence

Configuration is loaded in this order (later overrides earlier):

1. **Hardcoded defaults** in `Config::default()`
2. **Environment variables** (if set)

There is NO configuration file support currently. All config must come from environment variables or defaults.

---

## Logging Configuration

When the server starts, it logs the active configuration:

```
Configuration: port=3000, db_path=oxeye.db, body_limit=1024KB, timeout=30s
```

This helps verify that environment variables are being picked up correctly.

---

## Security Considerations

### Don't Expose Internals

- Configuration values are logged at startup (INFO level)
- Ensure logs don't expose sensitive paths in production
- Consider using TRACING_LEVEL env var to control log verbosity

### File Permissions

- Ensure database file has restricted permissions (600 or 640)
- Ensure database directory is not world-writable

### Resource Limits

- **Body Limit:** Too high allows DOS attacks; too low blocks legitimate requests
- **Timeout:** Too high allows resource exhaustion; too low causes spurious failures

---

## Troubleshooting

### "Address already in use"

Another process is using the port. Either:
- Stop the other process
- Change `PORT` environment variable to a different port

```bash
# Check what's using port 3000
lsof -i :3000
# or
netstat -tunlp | grep 3000
```

### "Permission denied" for database

The database file or directory is not writable:

```bash
# Make directory writable
chmod 755 /path/to/database/dir

# Make database file writable
chmod 644 /path/to/database/oxeye.db
```

### Environment Variables Not Taking Effect

Ensure they're exported:

```bash
# Wrong - only sets for that one command
PORT=8080
cargo run

# Right - exports for all subsequent commands
export PORT=8080
cargo run
```

Or set inline:

```bash
PORT=8080 cargo run
```

### Config Values Seem Wrong

Check the startup logs:

```
Configuration: port=3000, db_path=oxeye.db, body_limit=1024KB, timeout=30s
```

If values are wrong, verify:
1. Environment variables are exported
2. Values are valid (integers for numbers, valid paths for DATABASE_PATH)
3. No typos in environment variable names

---

## Implementation Details

Configuration is implemented in `oxeye-backend/src/config.rs`:

```rust
pub struct Config {
    pub request_body_limit: usize,
    pub request_timeout: Duration,
    pub port: u16,
    pub database_path: String,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            request_body_limit: env_or_default("REQUEST_BODY_LIMIT", 1024 * 1024),
            request_timeout: Duration::from_secs(env_or_default("REQUEST_TIMEOUT_SECS", 30)),
            port: env_or_default("PORT", 3000),
            database_path: env_or_default_string("DATABASE_PATH", "oxeye.db"),
        }
    }
}
```

The `env_or_default` function attempts to parse the environment variable, and falls back to the default if:
- The environment variable is not set
- The environment variable cannot be parsed (e.g., "abc" for an integer)
