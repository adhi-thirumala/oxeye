# Recent Activity & Offline Duration Feature Design

## Overview

This feature extends Oxeye to track **recent player activity** and **offline duration**, enabling users to see who's been on their Minecraft servers recently and how long they've been offline.

## Problem Statement

Currently, Oxeye only tracks players who are **currently online**. Once a player disconnects, all information about their session is lost. Users want to:

1. See which players have been on recently (even if they're currently offline)
2. Know how long a player has been offline
3. Understand recent server activity patterns

## Design Decisions

### 1. Data Storage Strategy

**Choice: Persistent SQLite table for player sessions**

We'll create a new `player_sessions` table to track historical join/leave events:

```sql
CREATE TABLE player_sessions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    api_key_hash TEXT NOT NULL,
    player_name TEXT NOT NULL,
    joined_at INTEGER NOT NULL,    -- Unix timestamp
    left_at INTEGER,                -- NULL if still online
    FOREIGN KEY (api_key_hash) REFERENCES servers(api_key_hash) ON DELETE CASCADE
);

CREATE INDEX idx_sessions_server_time ON player_sessions(api_key_hash, left_at DESC);
CREATE INDEX idx_sessions_player ON player_sessions(player_name);
```

**Why persistent storage?**
- Recent activity is valuable historical data that should survive backend restarts
- Users expect to query "who was on yesterday" even after server maintenance
- Session history enables future analytics features

### 2. Session Tracking

**When to create/update sessions:**

1. **Player joins** (`POST /join`)
   - Create a new session record with `joined_at` timestamp
   - `left_at` is NULL (still online)

2. **Player leaves** (`POST /leave`)
   - Update the session record, setting `left_at` to current timestamp

3. **Server sync** (`POST /sync`)
   - For players in the sync list who aren't in cache: create new sessions
   - For players in cache who aren't in sync list: close their sessions (set `left_at`)

4. **Server disconnection**
   - Close all open sessions for that server (set `left_at` to current time)

### 3. Data Retention

**Retention policy: 30 days**

Sessions older than 30 days should be automatically pruned to prevent unbounded growth:

```sql
DELETE FROM player_sessions
WHERE left_at < ?1
  AND left_at IS NOT NULL;
```

This cleanup can run:
- On database initialization
- Periodically (e.g., daily via a background task)
- Before querying recent activity

**Why 30 days?**
- Balances historical insight with database size
- Configurable via environment variable if users want more/less
- Most "recent activity" queries care about the last few days, not months

### 4. Query Interface

**New database methods:**

```rust
impl Database {
    /// Get recent sessions for a server (active or recently ended).
    /// Returns sessions that ended within `window_secs` or are still active.
    pub async fn get_recent_sessions(
        &self,
        api_key_hash: &str,
        window_secs: i64,
        now: i64
    ) -> Result<Vec<PlayerSession>>;

    /// Get all recent sessions across all servers in a guild.
    pub async fn get_recent_sessions_for_guild(
        &self,
        guild_id: u64,
        window_secs: i64,
        now: i64
    ) -> Result<Vec<ServerRecentSessions>>;

    /// Cleanup old sessions (called periodically).
    pub async fn cleanup_old_sessions(&self, before: i64) -> Result<u64>;
}

pub struct PlayerSession {
    pub player_name: PlayerName,
    pub joined_at: i64,
    pub left_at: Option<i64>,  // None if still online
}

pub struct ServerRecentSessions {
    pub server_name: String,
    pub sessions: Vec<PlayerSession>,
}
```

### 5. Discord Commands

**New command: `/oxeye recent [server] [timeframe]`**

Shows players who have been online recently, including offline duration.

**Parameters:**
- `server` (optional): Server name (defaults to all servers)
- `timeframe` (optional): How far back to look (default: 24h)
  - Options: `1h`, `6h`, `12h`, `24h`, `7d`, `30d`

**Example output:**

```
┌──────────────────────────────────────────────────┐
│ 📊 Recent Activity - survival                    │
├──────────────────────────────────────────────────┤
│ 🟢 Currently Online (3)                          │
│ • Steve (joined 2h ago)                          │
│ • Alex (joined 45m ago)                          │
│ • Notch (joined 5m ago)                          │
│                                                  │
│ 🔴 Recently Offline (5)                          │
│ • jeb_ (offline 30m)                             │
│ • Dinnerbone (offline 2h)                        │
│ • Herobrine (offline 5h)                         │
│ • Dream (offline 18h)                            │
│ • Technoblade (offline 23h)                      │
│                                                  │
│                                           Oxeye  │
└──────────────────────────────────────────────────┘
```

**Enhanced `/oxeye status` command:**

Update the existing status command to show last seen time for offline players:

```
┌──────────────────────────────────────────────────┐
│ 🟢 survival — 3 online                           │
├──────────────────────────────────────────────────┤
│ Steve (joined 2h ago)                            │
│ Alex (joined 45m ago)                            │
│ Notch (joined 5m ago)                            │
│                                                  │
│ Last seen:                                       │
│ • jeb_ (30m ago)                                 │
│ • Dinnerbone (2h ago)                            │
│                                                  │
│                                           Oxeye  │
└──────────────────────────────────────────────────┘
```

### 6. API Changes

No new HTTP endpoints needed. The existing endpoints will be enhanced to track sessions:

**`POST /join`** - Create new session record
**`POST /leave`** - Close session record
**`POST /sync`** - Reconcile session records with current player list
**`POST /disconnect`** - Close all open sessions for the server

### 7. Performance Considerations

**Indexes:**
- `idx_sessions_server_time` - Enables fast queries for recent sessions by server
- `idx_sessions_player` - Enables fast lookups of a specific player's history

**Query efficiency:**
```sql
-- Get recent sessions (last 24h) for a server
SELECT player_name, joined_at, left_at
FROM player_sessions
WHERE api_key_hash = ?1
  AND (left_at IS NULL OR left_at > ?2)
ORDER BY
  CASE WHEN left_at IS NULL THEN 0 ELSE 1 END,  -- Online first
  COALESCE(left_at, joined_at) DESC
LIMIT 100;
```

**Expected overhead:**
- INSERT on player join: ~1ms
- UPDATE on player leave: ~1ms
- Storage: ~100 bytes per session record
- For 100 players with 10 sessions/day each: ~100KB/day, ~3MB/month

This is negligible compared to the skin/image caching overhead already in the system.

### 8. Time Formatting

**Human-readable durations:**

```rust
fn format_duration(seconds: i64) -> String {
    match seconds {
        s if s < 60 => format!("{}s", s),
        s if s < 3600 => format!("{}m", s / 60),
        s if s < 86400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86400),
    }
}
```

Examples:
- `30s` - 30 seconds
- `45m` - 45 minutes
- `2h` - 2 hours
- `5d` - 5 days

### 9. Edge Cases

1. **Server restart without sync**
   - Open sessions remain open until next sync
   - Status command shows "⚠️ Server not synced since backend restart"

2. **Player rejoins before session timeout**
   - Close previous session (set `left_at`)
   - Create new session record
   - Prevents inflated "online time" from single mega-session

3. **Backend restart**
   - All open sessions in DB show `left_at = NULL`
   - On first sync per server, close old sessions
   - Cache is rebuilt from server sync

4. **Time zones**
   - All timestamps are UTC (Unix timestamps)
   - Duration calculations are timezone-agnostic
   - Discord renders relative times ("2h ago") automatically

## Implementation Plan

### Phase 1: Database Layer (Core)
1. Add `player_sessions` table to schema migration
2. Implement session tracking methods:
   - `create_session()`
   - `close_session()`
   - `get_recent_sessions()`
   - `cleanup_old_sessions()`
3. Add database tests for session tracking

### Phase 2: API Integration
1. Modify `POST /join` to create session records
2. Modify `POST /leave` to close session records
3. Modify `POST /sync` to reconcile sessions
4. Add cleanup task on database initialization

### Phase 3: Discord Commands
1. Add `/oxeye recent` command
2. Enhance `/oxeye status` to show last seen
3. Add time formatting helper functions

### Phase 4: Testing & Polish
1. Integration tests for session tracking
2. Test edge cases (disconnects, restarts, etc.)
3. Documentation updates

## Configuration

**New environment variables:**

```bash
# Session retention period (days)
SESSION_RETENTION_DAYS=30  # default: 30

# Default recent activity window (hours)
RECENT_ACTIVITY_WINDOW_HOURS=24  # default: 24
```

## Future Enhancements (Out of Scope)

1. **Player activity graphs**
   - Visualize peak hours
   - Track daily/weekly active users

2. **Player statistics**
   - Total playtime per player
   - Average session duration
   - First seen / last seen timestamps

3. **Activity webhooks**
   - Notify when specific players join/leave
   - Daily activity summary reports

4. **Playtime leaderboard**
   - Rank players by total time online
   - Show most active players per week/month

## Migration Strategy

This is a **backward-compatible** addition:
- Existing functionality continues to work unchanged
- New table is created on next backend startup
- No breaking changes to existing commands or APIs
- Session tracking starts from deployment forward (no backfill)

## Summary

This design provides a robust foundation for tracking recent player activity:

✅ **Persistent session tracking** with SQLite storage
✅ **Human-readable durations** in Discord embeds
✅ **Efficient queries** with proper indexing
✅ **Automatic cleanup** to prevent unbounded growth
✅ **Backward compatible** with existing functionality
✅ **Extensible** for future analytics features

The feature enhances user visibility into server activity while maintaining the simplicity and performance of the current system.
