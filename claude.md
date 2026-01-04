# Claude Code Guide for Oxeye

## Project Overview

Oxeye is a Discord bot + Minecraft Mod + backend service that tracks who's online on Minecraft servers and displays that
information in Discord. It consists of:

- **Rust Backend**: HTTP API server + Discord bot (single binary)
- **Minecraft Fabric Mod**: Java mod that reports player join/leave events
- **SQLite Database**: Persistent storage for server links and player status

## Enabled Tools

- **ast-grep**: Available for structural code search and refactoring using AST patterns. Use for finding/modifying Rust
  and Java code structures.

## Quick Reference

### Build Commands

```bash
# Build the project
cargo build --release

# Run tests
cargo test

# Run with Discord token
Happens by default because of .env file

# Check without building
cargo check
```

### Project Structure

```
oxeye/
├── oxeye-backend/        # Main Rust HTTP server + Discord bot
│   ├── src/
│   │   ├── main.rs       # Entry point, runtime setup
│   │   ├── lib.rs        # Router creation, middleware setup
│   │   ├── config.rs     # Environment configuration
│   │   ├── routes.rs     # HTTP endpoint handlers
│   │   ├── error.rs      # Error types and HTTP mapping
│   │   ├── helpers.rs    # Utility functions
│   │   ├── validation.rs # Input validation
│   │   └── discord_commands.rs  # Discord slash commands
│   └── tests/            # Integration tests
├── oxeye-db/             # Database abstraction crate
│   └── src/
│       ├── lib.rs        # Database operations (~60 methods)
│       ├── models.rs     # Data types
│       └── error.rs      # Database errors
├── oxeye-mod/            # Minecraft Fabric Mod (Java/Gradle)
├── docs/                 # Architecture documentation
├── Cargo.toml            # Workspace root
└── DESIGN.md             # API specification
```

## Technology Stack

| Component      | Technology         | Notes                         |
|----------------|--------------------|-------------------------------|
| Web Framework  | Axum 0.8           | HTTP server with middleware   |
| Async Runtime  | Tokio              | Full features enabled         |
| Discord Bot    | Poise 0.6          | Command framework on Serenity |
| Database       | tokio-rusqlite     | Async SQLite with WAL mode    |
| Serialization  | Serde + serde_json | JSON encoding/decoding        |
| Error Handling | thiserror          | Ergonomic error types         |
| Logging        | tracing            | Structured logging            |
| Rate Limiting  | tower-governor     | IP-based rate limiting        |
| Lock-free Map  | scc 3.x            | In-memory player cache        |

**Rust Edition**: 2024

### scc::HashMap API Reference

Lock-free concurrent HashMap. Async methods (use these in async code):

| Method | Purpose |
|--------|---------|
| `insert_async(key, val).await` | Insert, error if key exists |
| `upsert_async(key, val).await` | Insert or replace, returns old value |
| `remove_async(key).await` | Delete, returns `None` if missing |
| `read_async(key, \|k, v\| ...).await` | Read-only closure access |
| `update_async(key, \|k, v\| ...).await` | Mutating closure access |
| `contains_async(key).await` | Check key exists |
| `get_async(key).await` | Get `OccupiedEntry` for in-place ops |
| `entry_async(key).await` | Get `Entry` for insert-or-update |

Sync counterparts have `_sync` suffix (e.g., `insert_sync`).

## Architecture Patterns

### Layered Design

```
HTTP Request → Validation (validation.rs) → Routes (routes.rs) → Database (oxeye-db)
Discord Command → discord_commands.rs → Database (oxeye-db)
```

### Error Handling

- All errors flow through `AppError` enum in `error.rs`
- Database errors wrapped and mapped to HTTP status codes
- Internal errors logged server-side, user-friendly messages returned
- Never expose SQL, stack traces, or library names to clients

### Authentication

- API keys generated as `oxeye-sk-{32 alphanumeric}`
- Keys stored as SHA-256 hashes (never plaintext)
- Bearer token authentication on protected endpoints
- Connection codes format: `oxeye-{6 alphanumeric}` (10-minute TTL)

### Concurrency

- SQLite with WAL mode for concurrent reads
- Tokio async/await throughout
- Rate limiting prevents abuse

## API Endpoints

| Method | Path        | Auth   | Purpose                    |
|--------|-------------|--------|----------------------------|
| POST   | /connect    | None   | Redeem connection code     |
| POST   | /join       | Bearer | Report player joining      |
| POST   | /leave      | Bearer | Report player leaving      |
| POST   | /sync       | Bearer | Replace entire player list |
| GET    | /status     | Bearer | Check server connection    |
| POST   | /disconnect | Bearer | Unlink server              |

## Discord Commands

- `/oxeye connect <name>` - Admin-only, generates linking code
- `/oxeye list` - Shows all servers in guild
- `/oxeye status <name>` - Shows online players with join times

## Database Schema

Three main tables:

- `pending_links` - Connection codes with TTL
- `servers` - Linked Minecraft servers (api_key_hash, name, guild_id)
- `online_players` - Players currently online with join timestamps

## Testing

```bash
# Run all tests
cargo test

# Run specific test
cargo test test_name

# Run with output
cargo test -- --nocapture
```

Tests use in-memory SQLite for speed. Coverage includes:

- All HTTP endpoints (success and error cases)
- Database operations
- Input validation
- Rate limiting behavior
- Error response formats

## Environment Variables

| Variable             | Default  | Required | Description              |
|----------------------|----------|----------|--------------------------|
| DISCORD_TOKEN        | -        | Yes      | Discord bot token        |
| PORT                 | 3000     | No       | HTTP server port         |
| DATABASE_PATH        | oxeye.db | No       | SQLite database file     |
| REQUEST_BODY_LIMIT   | 1048576  | No       | Max request size (bytes) |
| REQUEST_TIMEOUT_SECS | 30       | No       | Request timeout          |

Rate limit variables also available (see `config.rs`).

## Code Style

- Use `#[debug_handler]` on Axum handlers for better error messages
- Validation happens before any database operations
- Helper functions in `helpers.rs` for common operations
- Comprehensive error messages that help users fix issues
- Tests alongside implementation in oxeye-db, separate test files in oxeye-backend

## Common Tasks

### Adding a new HTTP endpoint

1. Add route in `lib.rs` under appropriate rate-limit tier
2. Add handler in `routes.rs`
3. Add validation if needed in `validation.rs`
4. Add database methods if needed in `oxeye-db/src/lib.rs`
5. Add integration tests in `tests/integration_tests.rs`

### Adding a new Discord command

1. Add command function in `discord_commands.rs`
2. Register in the commands vector in `main.rs`
3. Database access via `ctx.data().db`

### Adding a new database operation

1. Add method in `oxeye-db/src/lib.rs`
2. Add types if needed in `models.rs`
3. Add tests in the `#[cfg(test)]` module

## Key Files to Understand

- `oxeye-backend/src/lib.rs:1-119` - Router setup, middleware, rate limiting tiers
- `oxeye-backend/src/routes.rs:1-143` - All HTTP handlers
- `oxeye-db/src/lib.rs:1-400` - Database schema and core operations
- `DESIGN.md` - Complete API specification
- `docs/architecture-decisions.md` - Performance decisions and caching strategy

## Docker

Multi-architecture builds available:

- `Dockerfile.amd64` - x86-64
- `Dockerfile.arm64` - ARM64 (Raspberry Pi, Apple Silicon)

Final images are minimal scratch-based containers.

## Overall Preferences

Make sure to use the AskUserQuestion tool to clarify any ambiguities before proceeding with tasks. Be very liberal with
usage of this tool.

No need to clone the scc hashmap - its already a shared resource
