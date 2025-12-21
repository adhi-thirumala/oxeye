# Oxeye Implementation Specification

A Minecraft-to-Discord player status bridge. This document contains everything needed to implement the Rust backend +
Discord bot.

## Overview

Oxeye lets Discord servers see who's online on their linked Minecraft servers.

**Components:**

- **Single Rust binary** running both an HTTP server (for Fabric mods) and a Discord bot (for users)
- **Fabric mod** (Java) that reports player joins/leaves to the backend

```
┌─────────────────────────────────────────────────────────────────────┐
│                         oxeye (single binary)                       │
│                                                                     │
│  ┌─────────────────────────┐    ┌─────────────────────────────────┐ │
│  │     HTTP Server         │    │        Discord Bot              │ │
│  │     (Axum)              │    │        (Poise)                  │ │
│  │                         │    │                                 │ │
│  │  POST /connect          │    │  /setup <name>                  │ │
│  │  POST /join             │    │  /servers                       │ │
│  │  POST /leave            │    │  /online [server]               │ │
│  │  POST /sync             │    │  /remove <name>                 │ │
│  │                         │    │                                 │ │
│  └───────────┬─────────────┘    └───────────────┬─────────────────┘ │
│              │                                  │                   │
│              │         ┌────────────┐           │                   │
│              └────────►│  Database  │◄──────────┘                   │
│                        │  (SQLite)  │                               │
│                        └────────────┘                               │
└─────────────────────────────────────────────────────────────────────┘
         ▲
         │ HTTPS
         │
┌────────┴────────┐
│   Fabric Mod    │
│   (Java)        │
└─────────────────┘
```

## Connection Flow

1. Discord user runs `/setup survival` in their server
2. Bot creates a pending link in DB, returns code like `oxeye-a1b2c3`
3. MC admin runs `/oxeye connect oxeye-a1b2c3` in server console
4. Mod calls `POST /connect` with the code
5. Backend validates code, creates server entry, returns API key `sk_live_...`
6. Mod stores API key in `config/oxeye.json`
7. Now mod sends player events to `/join`, `/leave`, `/sync` with Bearer token

```
Discord User          Discord Bot              Database              Fabric Mod
     │                     │                       │                      │
     │  /setup survival    │                       │                      │
     │────────────────────►│                       │                      │
     │                     │  create_pending_link  │                      │
     │                     │──────────────────────►│                      │
     │                     │      oxeye-a1b2c3     │                      │
     │                     │◄──────────────────────│                      │
     │  "Run /oxeye        │                       │                      │
     │   connect           │                       │                      │
     │   oxeye-a1b2c3"     │                       │                      │
     │◄────────────────────│                       │                      │
     │                     │                       │                      │
     │                     │                       │   /oxeye connect     │
     │                     │                       │   oxeye-a1b2c3       │
     │                     │                       │◄─────────────────────│
     │                     │                       │                      │
     │                     │                       │  POST /connect       │
     │                     │                       │  { code }            │
     │                     │                       │◄─────────────────────│
     │                     │                       │                      │
     │                     │                       │  { api_key }         │
     │                     │                       │─────────────────────►│
     │                     │                       │                      │
     │                     │                       │  (stores in config)  │
```

---

## Database Schema

The database crate (`oxeye-db`) should already exist. Here's the schema for reference:

```sql
-- Pending connection codes (expire after 10 min)
CREATE TABLE pending_links
(
    code        TEXT PRIMARY KEY,
    guild_id    INTEGER NOT NULL,
    server_name TEXT    NOT NULL,
    created_at  INTEGER NOT NULL -- Unix timestamp
);

-- Linked Minecraft servers
CREATE TABLE servers
(
    api_key_hash TEXT PRIMARY KEY, -- SHA-256 hash of API key
    name         TEXT    NOT NULL,
    guild_id     INTEGER NOT NULL,
    UNIQUE (guild_id, name)
);

-- Currently online players
CREATE TABLE online_players
(
    api_key_hash TEXT    NOT NULL REFERENCES servers (api_key_hash) ON DELETE CASCADE,
    player_name  TEXT    NOT NULL,
    joined_at    INTEGER NOT NULL, -- Unix timestamp
    PRIMARY KEY (api_key_hash, player_name)
);

CREATE INDEX idx_servers_guild ON servers (guild_id);
```

### Required Database Methods

```rust
impl Database {
    // Pending links
    async fn create_pending_link(&self, code: &str, guild_id: u64, server_name: &str, created_at: i64) -> Result<()>;
    async fn consume_pending_link(&self, code: &str, now: i64) -> Result<PendingLink>;  // Also deletes expired
    async fn cleanup_expired_links(&self, now: i64) -> Result<u64>;  // Returns count deleted

    // Servers
    async fn create_server(&self, api_key_hash: &str, name: &str, guild_id: u64) -> Result<()>;
    async fn delete_server(&self, guild_id: u64, name: &str) -> Result<bool>;  // Returns true if deleted
    async fn server_exists(&self, api_key_hash: &str) -> Result<bool>;
    async fn get_servers_for_guild(&self, guild_id: u64) -> Result<Vec<ServerSummary>>;
    async fn get_server_with_players(&self, guild_id: u64, name: &str) -> Result<Option<ServerWithPlayers>>;
    async fn get_all_servers_with_players(&self, guild_id: u64) -> Result<Vec<ServerWithPlayers>>;

    // Players
    async fn player_join(&self, api_key_hash: &str, player: &str, joined_at: i64) -> Result<()>;
    async fn player_leave(&self, api_key_hash: &str, player: &str) -> Result<()>;
    async fn sync_players(&self, api_key_hash: &str, players: &[String], joined_at: i64) -> Result<()>;  // Replace all
}

// Types
struct PendingLink {
    code: String,
    guild_id: u64,
    server_name: String,
    created_at: i64,
}

struct ServerSummary {
    name: String,
    player_count: u32,
}

struct ServerWithPlayers {
    name: String,
    players: Vec<OnlinePlayer>,
}

struct OnlinePlayer {
    name: String,
    joined_at: i64,
}
```

---

## HTTP API (for Fabric Mod)

All routes are for the Fabric mod to call. The Discord bot does NOT use HTTP — it calls the database directly.

### POST /connect

Redeem a connection code, get an API key.

**Request:**

```json
{
  "code": "oxeye-a1b2c3"
}
```

**Response (201 Created):**

```json
{
  "api_key": "sk_live_aBcDeFgHiJkLmNoPqRsTuVwXyZ123456"
}
```

**Errors:**

- `404 Not Found` — Code doesn't exist or expired
- `409 Conflict` — Server with that name already linked to this guild

**Implementation:**

1. Call `db.consume_pending_link(code, now)` — returns `PendingLink` or error
2. Generate API key: `format!("sk_live_{}", random_alphanumeric(32))`
3. Hash it: `sha256(api_key)` → hex string
4. Call `db.create_server(hash, link.server_name, link.guild_id)`
5. Return the plaintext API key (only time it's ever visible)

---

### POST /join

Report a player joining. Requires Bearer auth.

**Headers:**

```
Authorization: Bearer sk_live_...
```

**Request:**

```json
{
  "player": "Steve"
}
```

**Response (200 OK):**

```json
{
  "ok": true
}
```

**Errors:**

- `401 Unauthorized` — Missing or invalid API key

**Implementation:**

1. Extract Bearer token from `Authorization` header
2. Hash it
3. Verify server exists: `db.server_exists(hash)`
4. Call `db.player_join(hash, player, now)`

---

### POST /leave

Report a player leaving. Requires Bearer auth.

**Headers:**

```
Authorization: Bearer sk_live_...
```

**Request:**

```json
{
  "player": "Steve"
}
```

**Response (200 OK):**

```json
{
  "ok": true
}
```

---

### POST /sync

Replace the entire player list. Called on server start (with current players) and stop (with empty list).

**Headers:**

```
Authorization: Bearer sk_live_...
```

**Request:**

```json
{
  "players": [
    "Steve",
    "Alex"
  ]
}
```

**Response (200 OK):**

```json
{
  "ok": true
}
```

**Implementation:**

1. Delete all players for this server
2. Insert all players from the list

---

## Discord Bot Commands

All commands use Poise framework. The bot has direct database access (same binary).

### /setup \<name\>

Link a Minecraft server to this Discord guild.

**Parameters:**

- `name` (String, required) — Display name for the server (e.g., "survival", "creative")

**Permissions:** Requires "Manage Server"

**Implementation:**

1. Check user has Manage Server permission
2. Generate code: `format!("oxeye-{}", random_alphanumeric(6).to_lowercase())`
3. Call `db.create_pending_link(code, guild_id, name, now)`
4. Reply with embed:

```
┌──────────────────────────────────────────┐
│ 🔗 Link Minecraft Server                 │
├──────────────────────────────────────────┤
│ Run this command in your MC console:     │
│                                          │
│ /oxeye connect oxeye-a1b2c3              │
│                                          │
│ ⏱️ Code expires in 10 minutes            │
│                                          │
│                                   Oxeye  │
└──────────────────────────────────────────┘
```

---

### /servers

List all linked servers with player counts.

**Parameters:** None

**Permissions:** Everyone

**Implementation:**

1. Call `db.get_servers_for_guild(guild_id)`
2. Reply with embed:

```
┌──────────────────────────────────────────┐
│ 📋 Linked Servers                        │
├──────────────────────────────────────────┤
│ survival — 3 online                      │
│ creative — 0 online                      │
│ modded — 1 online                        │
│                                          │
│                                   Oxeye  │
└──────────────────────────────────────────┘
```

Or if no servers:

```
┌──────────────────────────────────────────┐
│ 📋 Linked Servers                        │
├──────────────────────────────────────────┤
│ No servers linked yet.                   │
│ Use /setup to link one!                  │
│                                          │
│                                   Oxeye  │
└──────────────────────────────────────────┘
```

---

### /online \[server\]

Show online players.

**Parameters:**

- `server` (String, optional) — Server name. If omitted, shows all servers.

**Permissions:** Everyone

**Implementation:**

If `server` is provided:

1. Call `db.get_server_with_players(guild_id, server)`
2. If not found, reply "Server not found"
3. Reply with embed showing players

If `server` is omitted:

1. Call `db.get_all_servers_with_players(guild_id)`
2. Reply with embed showing all servers and their players

**Embed (single server):**

```
┌──────────────────────────────────────────┐
│ 🟢 survival — 3 online                   │
├──────────────────────────────────────────┤
│ Steve                                    │
│ Alex                                     │
│ Notch                                    │
│                                          │
│                                   Oxeye  │
└──────────────────────────────────────────┘
```

**Embed (all servers):**

```
┌──────────────────────────────────────────┐
│ 🟢 Online Players                        │
├──────────────────────────────────────────┤
│ **survival** (3)                         │
│ Steve, Alex, Notch                       │
│                                          │
│ **creative** (0)                         │
│ No one online                            │
│                                          │
│                                   Oxeye  │
└──────────────────────────────────────────┘
```

---

### /remove \<name\>

Unlink a server.

**Parameters:**

- `name` (String, required) — Server name to remove

**Permissions:** Requires "Manage Server"

**Implementation:**

1. Check user has Manage Server permission
2. Call `db.delete_server(guild_id, name)`
3. If returned false, reply "Server not found"
4. Reply with embed:

```
┌──────────────────────────────────────────┐
│ 🗑️ Server Removed                        │
├──────────────────────────────────────────┤
│ "survival" has been unlinked.            │
│                                          │
│                                   Oxeye  │
└──────────────────────────────────────────┘
```

---

## Implementation Details

### Dependencies (Cargo.toml)

```toml
[package]
name = "oxeye"
version = "0.1.0"
edition = "2021"

[dependencies]
# Async runtime
tokio = { version = "1", features = ["full"] }

# HTTP server
axum = "0.7"
tower = "0.5"
tower-http = { version = "0.6", features = ["cors", "trace"] }

# Discord bot
poise = "0.6"
serenity = { version = "0.12", default-features = false, features = ["client", "gateway", "model", "cache"] }

# Database (your crate)
oxeye-db = { path = "../oxeye-db" }

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# Auth utilities
sha2 = "0.10"
rand = "0.8"
hex = "0.4"

# Bearer token extraction
axum-auth = "0.8"

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

# Error handling
thiserror = "2"
anyhow = "1"
```

---

### Main Structure

```rust
// src/main.rs

use std::sync::Arc;
use tokio::net::TcpListener;

mod http;
mod bot;
mod auth;
mod error;

#[derive(Clone)]
pub struct AppState {
    pub db: oxeye_db::Database,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter("oxeye=debug,tower_http=debug")
        .init();

    // Load config
    let discord_token = std::env::var("DISCORD_TOKEN")?;
    let db_path = std::env::var("DATABASE_PATH").unwrap_or_else(|_| "oxeye.db".into());
    let http_port: u16 = std::env::var("HTTP_PORT")
        .unwrap_or_else(|_| "8080".into())
        .parse()?;

    // Initialize database
    let db = oxeye_db::Database::open(&db_path).await?;
    let state = AppState { db };

    // Spawn HTTP server
    let http_state = state.clone();
    tokio::spawn(async move {
        let app = http::router(http_state);
        let listener = TcpListener::bind(("0.0.0.0", http_port)).await.unwrap();
        tracing::info!("HTTP server listening on port {}", http_port);
        axum::serve(listener, app).await.unwrap();
    });

    // Run Discord bot (blocks)
    tracing::info!("Starting Discord bot...");
    bot::run(discord_token, state).await?;

    Ok(())
}
```

---

### HTTP Module

```rust
// src/http.rs

use axum::{
    extract::{State, Json},
    http::StatusCode,
    routing::post,
    Router,
};
use axum_auth::AuthBearer;
use serde::{Deserialize, Serialize};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use crate::{auth, error::AppError, AppState};

// ─────────────────────────────────────────────────────────────
// Request/Response Types
// ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ConnectRequest {
    pub code: String,
}

#[derive(Serialize)]
pub struct ConnectResponse {
    pub api_key: String,
}

#[derive(Deserialize)]
pub struct PlayerRequest {
    pub player: String,
}

#[derive(Deserialize)]
pub struct SyncRequest {
    pub players: Vec<String>,
}

#[derive(Serialize)]
pub struct OkResponse {
    pub ok: bool,
}

// ─────────────────────────────────────────────────────────────
// Router
// ─────────────────────────────────────────────────────────────

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/connect", post(connect))
        .route("/join", post(join))
        .route("/leave", post(leave))
        .route("/sync", post(sync))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}

// ─────────────────────────────────────────────────────────────
// Handlers
// ─────────────────────────────────────────────────────────────

async fn connect(
    State(state): State<AppState>,
    Json(req): Json<ConnectRequest>,
) -> Result<(StatusCode, Json<ConnectResponse>), AppError> {
    let now = auth::now();
    
    // Consume the pending link (also validates expiry)
    let link = state.db.consume_pending_link(&req.code, now).await?;
    
    // Generate API key
    let api_key = auth::generate_api_key();
    let hash = auth::hash_api_key(&api_key);
    
    // Create server
    state.db.create_server(&hash, &link.server_name, link.guild_id).await?;
    
    tracing::info!(
        guild_id = link.guild_id,
        server = link.server_name,
        "Server connected"
    );
    
    Ok((StatusCode::CREATED, Json(ConnectResponse { api_key })))
}

async fn join(
    State(state): State<AppState>,
    AuthBearer(token): AuthBearer,
    Json(req): Json<PlayerRequest>,
) -> Result<Json<OkResponse>, AppError> {
    let hash = auth::hash_api_key(&token);
    
    // Verify server exists
    if !state.db.server_exists(&hash).await? {
        return Err(AppError::InvalidApiKey);
    }
    
    state.db.player_join(&hash, &req.player, auth::now()).await?;
    
    tracing::debug!(player = req.player, "Player joined");
    
    Ok(Json(OkResponse { ok: true }))
}

async fn leave(
    State(state): State<AppState>,
    AuthBearer(token): AuthBearer,
    Json(req): Json<PlayerRequest>,
) -> Result<Json<OkResponse>, AppError> {
    let hash = auth::hash_api_key(&token);
    
    if !state.db.server_exists(&hash).await? {
        return Err(AppError::InvalidApiKey);
    }
    
    state.db.player_leave(&hash, &req.player).await?;
    
    tracing::debug!(player = req.player, "Player left");
    
    Ok(Json(OkResponse { ok: true }))
}

async fn sync(
    State(state): State<AppState>,
    AuthBearer(token): AuthBearer,
    Json(req): Json<SyncRequest>,
) -> Result<Json<OkResponse>, AppError> {
    let hash = auth::hash_api_key(&token);
    
    if !state.db.server_exists(&hash).await? {
        return Err(AppError::InvalidApiKey);
    }
    
    state.db.sync_players(&hash, &req.players, auth::now()).await?;
    
    tracing::debug!(count = req.players.len(), "Players synced");
    
    Ok(Json(OkResponse { ok: true }))
}
```

---

### Auth Module

```rust
// src/auth.rs

use rand::Rng;
use sha2::{Sha256, Digest};
use std::time::{SystemTime, UNIX_EPOCH};

/// Generate a connection code like "oxeye-a1b2c3"
pub fn generate_code() -> String {
    let suffix: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(6)
        .map(|c| c.to_ascii_lowercase() as char)
        .collect();
    
    format!("oxeye-{}", suffix)
}

/// Generate an API key like "sk_live_aBcDeFgHiJkLmNoPqRsTuVwXyZ123456"
pub fn generate_api_key() -> String {
    let random: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(32)
        .map(char::from)
        .collect();
    
    format!("sk_live_{}", random)
}

/// Hash an API key using SHA-256, return hex string
pub fn hash_api_key(key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(key.as_bytes());
    hex::encode(hasher.finalize())
}

/// Current Unix timestamp in seconds
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

/// Check if a pending link has expired (10 minute lifetime)
pub fn is_expired(created_at: i64, now: i64) -> bool {
    now - created_at > 600 // 10 minutes
}
```

---

### Error Module

```rust
// src/error.rs

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Not found")]
    NotFound,
    
    #[error("Invalid API key")]
    InvalidApiKey,
    
    #[error("Server already exists")]
    Conflict,
    
    #[error("Code expired")]
    CodeExpired,
    
    #[error("Database error: {0}")]
    Database(#[from] oxeye_db::Error),
    
    #[error("Internal error: {0}")]
    Internal(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            AppError::NotFound => (StatusCode::NOT_FOUND, "Not found"),
            AppError::InvalidApiKey => (StatusCode::UNAUTHORIZED, "Invalid API key"),
            AppError::Conflict => (StatusCode::CONFLICT, "Server already exists"),
            AppError::CodeExpired => (StatusCode::GONE, "Code expired"),
            AppError::Database(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Database error"),
            AppError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Internal error"),
        };
        
        tracing::error!(error = %self, "Request failed");
        
        (status, Json(json!({ "error": message }))).into_response()
    }
}
```

---

### Bot Module

```rust
// src/bot.rs

use poise::serenity_prelude as serenity;
use serenity::all::{CreateEmbed, CreateEmbedFooter};

use crate::{auth, AppState};

type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, AppState, Error>;

const EMBED_COLOR: u32 = 0x55FF55;

// ─────────────────────────────────────────────────────────────
// Bot Entry Point
// ─────────────────────────────────────────────────────────────

pub async fn run(token: String, state: AppState) -> anyhow::Result<()> {
    let intents = serenity::GatewayIntents::non_privileged();
    
    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: vec![setup(), servers(), online(), remove()],
            ..Default::default()
        })
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                tracing::info!("Bot ready!");
                Ok(state)
            })
        })
        .build();
    
    let client = serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .await?;
    
    client.start().await?;
    
    Ok(())
}

// ─────────────────────────────────────────────────────────────
// Helper: Check Manage Server permission
// ─────────────────────────────────────────────────────────────

async fn check_manage_server(ctx: Context<'_>) -> Result<bool, Error> {
    let guild_id = ctx.guild_id().ok_or("Must be used in a server")?;
    let member = ctx.author_member().await.ok_or("Could not get member")?;
    
    let permissions = member.permissions(ctx.cache())?;
    
    if !permissions.manage_guild() {
        ctx.send(
            poise::CreateReply::default()
                .content("❌ You need the **Manage Server** permission to use this command.")
                .ephemeral(true)
        ).await?;
        return Ok(false);
    }
    
    Ok(true)
}

// ─────────────────────────────────────────────────────────────
// Commands
// ─────────────────────────────────────────────────────────────

/// Link a Minecraft server to this Discord
#[poise::command(slash_command, guild_only)]
async fn setup(
    ctx: Context<'_>,
    #[description = "Name for this server (e.g., 'survival')"] name: String,
) -> Result<(), Error> {
    if !check_manage_server(ctx).await? {
        return Ok(());
    }
    
    let guild_id = ctx.guild_id().unwrap().get();
    let code = auth::generate_code();
    
    ctx.data().db.create_pending_link(&code, guild_id, &name, auth::now()).await?;
    
    let embed = CreateEmbed::new()
        .title("🔗 Link Minecraft Server")
        .description(format!(
            "Run this command in your Minecraft server console:\n\n\
            ```\n/oxeye connect {}\n```\n\n\
            ⏱️ Code expires in **10 minutes**",
            code
        ))
        .color(EMBED_COLOR)
        .footer(CreateEmbedFooter::new("Oxeye"));
    
    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    
    Ok(())
}

/// List all linked servers
#[poise::command(slash_command, guild_only)]
async fn servers(ctx: Context<'_>) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap().get();
    
    let servers = ctx.data().db.get_servers_for_guild(guild_id).await?;
    
    let description = if servers.is_empty() {
        "No servers linked yet.\nUse `/setup` to link one!".to_string()
    } else {
        servers
            .iter()
            .map(|s| format!("**{}** — {} online", s.name, s.player_count))
            .collect::<Vec<_>>()
            .join("\n")
    };
    
    let embed = CreateEmbed::new()
        .title("📋 Linked Servers")
        .description(description)
        .color(EMBED_COLOR)
        .footer(CreateEmbedFooter::new("Oxeye"));
    
    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    
    Ok(())
}

/// See who's online
#[poise::command(slash_command, guild_only)]
async fn online(
    ctx: Context<'_>,
    #[description = "Server name (leave empty for all)"] server: Option<String>,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().unwrap().get();
    
    let embed = if let Some(name) = server {
        // Single server
        let server = ctx.data().db.get_server_with_players(guild_id, &name).await?;
        
        match server {
            Some(s) => {
                let player_list = if s.players.is_empty() {
                    "No one online".to_string()
                } else {
                    s.players.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join("\n")
                };
                
                CreateEmbed::new()
                    .title(format!("🟢 {} — {} online", s.name, s.players.len()))
                    .description(player_list)
                    .color(EMBED_COLOR)
                    .footer(CreateEmbedFooter::new("Oxeye"))
            }
            None => {
                CreateEmbed::new()
                    .title("❌ Server Not Found")
                    .description(format!("No server named \"{}\" is linked.\nUse `/servers` to see linked servers.", name))
                    .color(0xFF5555)
                    .footer(CreateEmbedFooter::new("Oxeye"))
            }
        }
    } else {
        // All servers
        let servers = ctx.data().db.get_all_servers_with_players(guild_id).await?;
        
        if servers.is_empty() {
            CreateEmbed::new()
                .title("🟢 Online Players")
                .description("No servers linked yet.\nUse `/setup` to link one!")
                .color(EMBED_COLOR)
                .footer(CreateEmbedFooter::new("Oxeye"))
        } else {
            let description = servers
                .iter()
                .map(|s| {
                    let players = if s.players.is_empty() {
                        "No one online".to_string()
                    } else {
                        s.players.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ")
                    };
                    format!("**{}** ({})\n{}", s.name, s.players.len(), players)
                })
                .collect::<Vec<_>>()
                .join("\n\n");
            
            CreateEmbed::new()
                .title("🟢 Online Players")
                .description(description)
                .color(EMBED_COLOR)
                .footer(CreateEmbedFooter::new("Oxeye"))
        }
    };
    
    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    
    Ok(())
}

/// Unlink a server
#[poise::command(slash_command, guild_only)]
async fn remove(
    ctx: Context<'_>,
    #[description = "Server name to remove"] name: String,
) -> Result<(), Error> {
    if !check_manage_server(ctx).await? {
        return Ok(());
    }
    
    let guild_id = ctx.guild_id().unwrap().get();
    
    let deleted = ctx.data().db.delete_server(guild_id, &name).await?;
    
    let embed = if deleted {
        CreateEmbed::new()
            .title("🗑️ Server Removed")
            .description(format!("\"{}\" has been unlinked.", name))
            .color(EMBED_COLOR)
            .footer(CreateEmbedFooter::new("Oxeye"))
    } else {
        CreateEmbed::new()
            .title("❌ Server Not Found")
            .description(format!("No server named \"{}\" is linked.", name))
            .color(0xFF5555)
            .footer(CreateEmbedFooter::new("Oxeye"))
    };
    
    ctx.send(poise::CreateReply::default().embed(embed)).await?;
    
    Ok(())
}
```

---

## File Structure

```
oxeye/
├── Cargo.toml
├── src/
│   ├── main.rs      # Entry point, spawns HTTP + bot
│   ├── http.rs      # Axum router and handlers
│   ├── bot.rs       # Poise commands
│   ├── auth.rs      # Code/key generation, hashing
│   └── error.rs     # AppError type
└── oxeye-db/        # Your existing database crate
    ├── Cargo.toml
    └── src/
        └── lib.rs
```

---

## Environment Variables

```bash
# Required
DISCORD_TOKEN=your_discord_bot_token

# Optional (defaults shown)
DATABASE_PATH=./oxeye.db
HTTP_PORT=8080
RUST_LOG=oxeye=debug,tower_http=debug
```

---

## Security Notes

1. **TLS Required** — Deploy behind a reverse proxy (nginx, Caddy) with HTTPS
2. **API Keys** — Only visible once at connection time, stored as SHA-256 hash
3. **Connection Codes** — Expire after 10 minutes, single-use
4. **Permission Checks** — `/setup` and `/remove` require Manage Server

---

## Future Enhancements (Not in Scope)

- Webhook notifications on player join/leave
- Bot status showing total players across all servers
- Player head avatars via Crafatar API
- Historical player count graphs
- Web dashboard