
# Oxeye Design Document

> A system for displaying Minecraft server player status in Discord.

## Overview

Oxeye consists of three components:

1. **Backend** (Rust + Axum + SQLite) — Central API server
2. **Discord Bot** (Rust + Poise) — Slash commands for Discord users
3. **Fabric Mod** (Java) — Reports player join/leave events from MC servers

```
┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
│  Fabric Mod A   │     │  Fabric Mod B   │     │  Fabric Mod C   │
└────────┬────────┘     └────────┬────────┘     └────────┬────────┘
         │                       │                       │
         │ POST /join            │                       │
         │ POST /leave           │                       │
         │ Authorization: Bearer │                       │
         ▼                       ▼                       ▼
┌──────────────────────────────────────────────────────────────────┐
│                        Oxeye Backend                             │
│                                                                  │
│  Endpoints:                                                      │
│    POST /setup              (from bot)                           │
│    POST /connect            (from mod)                           │
│    POST /join               (from mod)                           │
│    POST /leave              (from mod)                           │
│    POST /sync               (from mod)                           │
│    GET  /guilds/:id/servers (from bot)                           │
│    GET  /guilds/:id/online  (from bot)                           │
│    DELETE /guilds/:id/servers/:name (from bot)                   │
│                                                                  │
│  Database: SQLite                                                │
└──────────────────────────────────────────────────────────────────┘
                                 ▲
                                 │
┌────────────────────────────────┴─────────────────────────────────┐
│                       Oxeye Discord Bot                          │
│                                                                  │
│  Commands:                                                       │
│    /setup <name>       → Generate connection code                │
│    /servers            → List linked servers                     │
│    /online [name]      → Show online players                     │
│    /remove <name>      → Unlink a server                         │
└──────────────────────────────────────────────────────────────────┘
```

---

## Connection Flow

### Linking a Minecraft Server to a Discord Guild

```
Discord User                Discord Bot                 Backend                    MC Server Admin
     │                           │                          │                             │
     │  /setup "Survival SMP"    │                          │                             │
     │ ─────────────────────────►│                          │                             │
     │                           │  POST /setup             │                             │
     │                           │  { guild_id, name }      │                             │
     │                           │ ────────────────────────►│                             │
     │                           │                          │  Create PendingLink         │
     │                           │                          │  { code, guild_id, name,    │
     │                           │     { code: "oxeye-..." }│    expires_at }             │
     │                           │ ◄────────────────────────│                             │
     │   "Run /oxeye connect     │                          │                             │
     │    oxeye-a1b2c3 on your   │                          │                             │
     │    MC server console"     │                          │                             │
     │ ◄─────────────────────────│                          │                             │
     │                           │                          │                             │
     │                           │                          │   /oxeye connect oxeye-...  │
     │                           │                          │ ◄────────────────────────────│
     │                           │                          │                             │
     │                           │                          │  POST /connect              │
     │                           │                          │  { code: "oxeye-a1b2c3" }   │
     │                           │                          │ ◄────────────────────────────│
     │                           │                          │                             │
     │                           │                          │  Validate code              │
     │                           │                          │  Create Server              │
     │                           │                          │  Generate API key           │
     │                           │                          │                             │
     │                           │                          │  { api_key: "sk_live_..." } │
     │                           │                          │ ────────────────────────────►│
     │                           │                          │                             │
     │                           │                          │         Mod saves API key   │
     │                           │                          │         to config/oxeye.json│
```

### Player Join/Leave Events

```
MC Server                       Backend
     │                              │
     │  Player "Steve" joins        │
     │                              │
     │  POST /join                  │
     │  Authorization: Bearer sk_.. │
     │  { "player": "Steve" }       │
     │ ────────────────────────────►│
     │                              │
     │                              │  Lookup server by API key hash
     │                              │  Add player to online_players table
     │                              │
     │              200 OK          │
     │ ◄────────────────────────────│
```

---

## Database Schema

```sql
-- Pending connection codes (expire after 10 minutes)
CREATE TABLE pending_links (
    code TEXT PRIMARY KEY,
    guild_id INTEGER NOT NULL,
    server_name TEXT NOT NULL,
    created_at INTEGER NOT NULL  -- Unix timestamp
);

-- Linked servers (API key hash is primary key)
CREATE TABLE servers (
    api_key_hash TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    guild_id INTEGER NOT NULL,
    UNIQUE(guild_id, name)
);

-- Online players
CREATE TABLE online_players (
    api_key_hash TEXT NOT NULL REFERENCES servers(api_key_hash) ON DELETE CASCADE,
    player_name TEXT NOT NULL,
    joined_at INTEGER NOT NULL,  -- Unix timestamp
    PRIMARY KEY (api_key_hash, player_name)
);

-- Index for fast guild lookups
CREATE INDEX idx_servers_guild ON servers(guild_id);
```

---

## API Specification

### Backend Endpoints

#### `POST /setup`
Called by Discord bot when user runs `/setup`.

**Request:**
```json
{
    "guild_id": 123456789,
    "server_name": "Survival SMP"
}
```

**Response (201 Created):**
```json
{
    "code": "oxeye-a1b2c3",
    "expires_in": 600
}
```

**Errors:**
- `409 Conflict` — Server name already exists in this guild

---

#### `POST /connect`
Called by Fabric mod when admin runs `/oxeye connect <code>`.

**Request:**
```json
{
    "code": "oxeye-a1b2c3"
}
```

**Response (201 Created):**
```json
{
    "api_key": "sk_live_abc123def456..."
}
```

**Errors:**
- `404 Not Found` — Invalid or expired code
- `410 Gone` — Code already used

---

#### `POST /join`
Called by Fabric mod when a player joins.

**Headers:**
```
Authorization: Bearer sk_live_abc123def456...
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
- `401 Unauthorized` — Invalid API key

---

#### `POST /leave`
Called by Fabric mod when a player leaves.

**Headers:**
```
Authorization: Bearer sk_live_abc123def456...
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
- `401 Unauthorized` — Invalid API key

---

#### `POST /sync`
Called by Fabric mod on server startup to sync current player list.
Clears existing players and replaces with provided list.

**Headers:**
```
Authorization: Bearer sk_live_abc123def456...
```

**Request:**
```json
{
    "players": ["Steve", "Alex"]
}
```

**Response (200 OK):**
```json
{
    "ok": true
}
```

**Errors:**
- `401 Unauthorized` — Invalid API key

---

#### `GET /guilds/:guild_id/servers`
Called by Discord bot for `/servers` command.

**Response (200 OK):**
```json
{
    "servers": [
        {
            "name": "Survival SMP",
            "player_count": 3
        },
        {
            "name": "Creative",
            "player_count": 0
        }
    ]
}
```

---

#### `GET /guilds/:guild_id/online`
Called by Discord bot for `/online` command.

**Query params:**
- `server` (optional) — Filter by server name

**Response (no filter, 200 OK):**
```json
{
    "servers": [
        {
            "name": "Survival SMP",
            "players": ["Steve", "Alex", "Notch"]
        },
        {
            "name": "Creative",
            "players": []
        }
    ]
}
```

**Response (with `?server=Survival%20SMP`, 200 OK):**
```json
{
    "name": "Survival SMP",
    "players": ["Steve", "Alex", "Notch"]
}
```

**Errors:**
- `404 Not Found` — Server name not found (when filter specified)

---

#### `DELETE /guilds/:guild_id/servers/:name`
Called by Discord bot for `/remove` command.

**Response (200 OK):**
```json
{
    "ok": true
}
```

**Errors:**
- `404 Not Found` — Server not found

---

## Discord Bot Commands

| Command | Description | Permissions |
|---------|-------------|-------------|
| `/setup <name>` | Generate a connection code for a new MC server | Manage Server |
| `/servers` | List all linked MC servers | Everyone |
| `/online [name]` | Show online players (all servers or specific) | Everyone |
| `/remove <name>` | Unlink a MC server | Manage Server |

### Embed Designs

#### `/servers`
```
┌────────────────────────────────────────┐
│ 📋 Linked Servers                      │
├────────────────────────────────────────┤
│                                        │
│ Survival SMP          3 online         │
│ Creative              0 online         │
│ Minigames             12 online        │
│                                        │
├────────────────────────────────────────┤
│ 3 servers linked                Oxeye  │
└────────────────────────────────────────┘
```

#### `/online` (all servers)
```
┌────────────────────────────────────────┐
│ 🟢 Online Players                      │
├────────────────────────────────────────┤
│                                        │
│ Survival SMP (3)                       │
│ Steve, Alex, Notch                     │
│                                        │
│ Creative (0)                           │
│ No players online                      │
│                                        │
│ Minigames (12)                         │
│ Player1, Player2, Player3, +9 more     │
│                                        │
├────────────────────────────────────────┤
│ 15 players online               Oxeye  │
└────────────────────────────────────────┘
```

#### `/online Survival SMP`
```
┌────────────────────────────────────────┐
│ 🟢 Survival SMP                        │
├────────────────────────────────────────┤
│                                        │
│ Steve                                  │
│ Alex                                   │
│ Notch                                  │
│                                        │
├────────────────────────────────────────┤
│ 3 players online                Oxeye  │
└────────────────────────────────────────┘
```

#### `/setup Survival SMP`
```
┌────────────────────────────────────────┐
│ 🔗 Connect Your Server                 │
├────────────────────────────────────────┤
│                                        │
│ Run this command in your Minecraft     │
│ server console:                        │
│                                        │
│ /oxeye connect oxeye-a1b2c3            │
│                                        │
│ ⏰ This code expires in 10 minutes     │
│                                        │
├────────────────────────────────────────┤
│                                 Oxeye  │
└────────────────────────────────────────┘
```

---

## Fabric Mod

### Commands

| Command | Description | Permissions |
|---------|-------------|-------------|
| `/oxeye connect <code>` | Link this server to a Discord guild | OP level 4 |
| `/oxeye disconnect` | Unlink this server | OP level 4 |
| `/oxeye status` | Show connection status | OP level 2 |

### Config File

`config/oxeye.json`:
```json
{
    "backend_url": "https://oxeye.yourdomain.com",
    "api_key": null
}
```

After connecting:
```json
{
    "backend_url": "https://oxeye.yourdomain.com",
    "api_key": "sk_live_abc123def456..."
}
```

### Events Hooked

| Event | Action |
|-------|--------|
| `ServerPlayConnectionEvents.JOIN` | POST /join with player name |
| `ServerPlayConnectionEvents.DISCONNECT` | POST /leave with player name |
| `ServerLifecycleEvents.SERVER_STARTED` | POST /sync with current player list |
| `ServerLifecycleEvents.SERVER_STOPPING` | POST /sync with empty list |

---

## Security

1. **TLS Required** — All communication over HTTPS
2. **API Key Hashing** — Keys stored as SHA-256 hashes in database
3. **Code Expiry** — Connection codes expire after 10 minutes
4. **Permission Checks** — `/setup` and `/remove` require Manage Server permission
5. **OP Required** — `/oxeye connect` requires OP level 4 on MC server

---

## Tech Stack

| Component | Technology |
|-----------|------------|
| Backend | Rust, Axum, rusqlite |
| Discord Bot | Rust, Poise |
| Fabric Mod | Java 21, Fabric API |

---

## File Structure

```
oxeye/
├── backend/
│   ├── Cargo.toml
│   └── src/
│       └── main.rs
├── bot/
│   ├── Cargo.toml
│   └── src/
│       └── main.rs
├── mod/
│   ├── build.gradle
│   ├── settings.gradle
│   ├── gradle.properties
│   └── src/main/
│       ├── java/com/oxeye/
│       │   ├── Oxeye.java
│       │   ├── OxeyeConfig.java
│       │   ├── OxeyeCommands.java
│       │   └── OxeyeHttp.java
│       └── resources/
│           └── fabric.mod.json
└── DESIGN.md
```

---

## Future Ideas

- [ ] Webhook notifications on player join/leave
- [ ] Bot status showing total players ("Watching 47 players")
- [ ] Player head avatars in embeds via Crafatar
- [ ] `/stats` command showing peak player counts
- [ ] Web dashboard
