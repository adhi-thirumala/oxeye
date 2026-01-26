# Recent Activity & Offline Duration Feature Design

## Overview

This feature extends Oxeye to track **recent player activity** and **last seen timestamps**, enabling users to see who's been on their Minecraft servers within a configurable time window (default: last 24 hours) and when they were last seen.

## Problem Statement

Currently, Oxeye only tracks players who are **currently online**. Once a player disconnects, all information about them is lost. Users want to:

1. See which players have been on recently (within the last day by default)
2. Know when each player was last seen
3. Understand recent server activity patterns without needing persistent history

## Design Decisions

### 1. Data Storage Strategy

**Choice: In-memory tracking with configurable time window**

We'll track recent player activity using an in-memory data structure within the existing player cache. Each player entry will include a `last_seen` timestamp:

```rust
struct PlayerActivityCache {
    // Existing player cache
    players: HashMap<ApiKeyHash, HashSet<PlayerName>>,

    // New: Track when each player was last seen
    last_seen: HashMap<(ApiKeyHash, PlayerName), i64>,  // Unix timestamp
}
```

**Why in-memory storage?**
- Simplicity: No database schema changes, migrations, or cleanup tasks
- Performance: Instant lookups with no database queries
- Sufficient for use case: Recent activity within a configurable window (default 1 day)
- No persistence needed: If backend restarts, we'll rebuild tracking from incoming server syncs
- Lightweight: Only stores timestamps, not full session records

### 2. Activity Tracking

**When to update last seen timestamps:**

1. **Player joins** (`POST /join`)
   - Update `last_seen` timestamp to current time
   - Add player to active players cache

2. **Player leaves** (`POST /leave`)
   - Update `last_seen` timestamp to current time
   - Remove player from active players cache
   - Keep `last_seen` timestamp in memory for configured window

3. **Server sync** (`POST /sync`)
   - For all players in sync list: update their `last_seen` timestamps
   - For players in cache but not in sync list: update `last_seen` and mark as offline

4. **Server disconnection**
   - Update `last_seen` for all active players on that server
   - Clear from active players cache

### 3. Data Retention

**Retention policy: Configurable time window (default 24 hours)**

Last seen entries are automatically filtered when querying:
- Only return players whose `last_seen` timestamp is within the configured window
- No explicit cleanup needed - entries naturally age out when queried
- Optional: Periodic cleanup to remove entries older than 2x the window to prevent memory growth

**Why 24 hours default?**
- Covers typical "who was on today" use case
- Lightweight memory footprint
- Configurable via environment variable for users who want longer history
- Short enough that in-memory storage is practical

### 4. Query Interface

**New cache methods:**

```rust
impl PlayerCache {
    /// Get recent activity for a server (active or recently seen).
    /// Returns players seen within `window_secs` from now.
    pub fn get_recent_activity(
        &self,
        api_key_hash: &str,
        window_secs: i64,
        now: i64
    ) -> Vec<PlayerActivity>;

    /// Get all recent activity across all servers in a guild.
    pub fn get_recent_activity_for_guild(
        &self,
        guild_id: u64,
        window_secs: i64,
        now: i64
    ) -> Vec<ServerActivity>;

    /// Optional: Cleanup entries older than threshold to prevent memory growth.
    pub fn cleanup_old_activity(&mut self, before: i64);
}

pub struct PlayerActivity {
    pub player_name: PlayerName,
    pub last_seen: i64,
    pub is_online: bool,
}

pub struct ServerActivity {
    pub server_name: String,
    pub players: Vec<PlayerActivity>,
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
│ 📊 Recent Activity - survival (last 24h)         │
├──────────────────────────────────────────────────┤
│ 🟢 Currently Online (3)                          │
│ • Steve (last seen: online now)                  │
│ • Alex (last seen: online now)                   │
│ • Notch (last seen: online now)                  │
│                                                  │
│ 🔴 Recently Offline (5)                          │
│ • jeb_ (last seen: 30m ago)                      │
│ • Dinnerbone (last seen: 2h ago)                 │
│ • Herobrine (last seen: 5h ago)                  │
│ • Dream (last seen: 18h ago)                     │
│ • Technoblade (last seen: 23h ago)               │
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

No new HTTP endpoints needed. The existing endpoints will be enhanced to update last seen timestamps:

**`POST /join`** - Update player's last seen timestamp
**`POST /leave`** - Update player's last seen timestamp
**`POST /sync`** - Update last seen timestamps for all players in sync
**`POST /disconnect`** - Update last seen timestamps for all active players on server

### 7. Performance Considerations

**Memory overhead:**
- HashMap entry: ~(32 bytes key + 8 bytes timestamp) = ~40 bytes per player
- For 100 unique players in 24h: ~4KB total
- Negligible compared to player skin cache

**Query efficiency:**
- O(1) timestamp lookup per player
- O(n) filtering for recent activity (where n = total tracked players)
- For typical server (100 players): < 1ms query time
- No database I/O required

**Expected overhead:**
- Timestamp update on join/leave: < 0.1ms (HashMap insert)
- Recent activity query: < 1ms (in-memory filtering)
- Memory: ~40 bytes per unique player tracked

This is significantly lighter than the previous database approach.

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

1. **Backend restart**
   - In-memory cache is empty
   - Last seen timestamps are lost (acceptable trade-off)
   - Tracking resumes as servers sync and players join/leave
   - Recent activity will be accurate within configured window after restart

2. **Server disconnection**
   - Update last seen for all active players before clearing
   - Players show in recent activity list until they age out of window

3. **Player rejoins multiple times**
   - Last seen timestamp is simply updated each time
   - No duplicate entries since we only store one timestamp per player

4. **Time zones**
   - All timestamps are UTC (Unix timestamps)
   - Duration calculations are timezone-agnostic
   - Discord renders relative times ("30m ago") automatically

## Implementation Plan

### Phase 1: Cache Layer (Core)
1. Add `last_seen` HashMap to player cache structure
2. Implement activity tracking methods:
   - `update_last_seen()`
   - `get_recent_activity()`
   - `cleanup_old_activity()` (optional)
3. Add unit tests for activity tracking

### Phase 2: API Integration
1. Modify `POST /join` to update last seen timestamps
2. Modify `POST /leave` to update last seen timestamps
3. Modify `POST /sync` to update last seen timestamps
4. Modify `POST /disconnect` to update last seen timestamps

### Phase 3: Discord Commands
1. Add `/oxeye recent` command
2. Enhance `/oxeye status` to show last seen info
3. Add time formatting helper functions

### Phase 4: Testing & Polish
1. Integration tests for activity tracking
2. Test edge cases (restarts, disconnects, etc.)
3. Documentation updates

## Configuration

**New environment variable:**

```bash
# Default recent activity window (hours)
RECENT_ACTIVITY_WINDOW_HOURS=24  # default: 24
```

Users can also override this per-command using the `timeframe` parameter in `/oxeye recent`.

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
- No database schema changes required
- No breaking changes to existing commands or APIs
- Activity tracking starts immediately upon deployment
- After backend restart, tracking resumes from first sync/join/leave events

## Summary

This design provides a lightweight foundation for tracking recent player activity:

✅ **In-memory tracking** with configurable time window (default 24h)
✅ **Simple last seen timestamps** for each player
✅ **Human-readable durations** in Discord embeds
✅ **No database changes** required
✅ **Minimal memory overhead** (~40 bytes per player)
✅ **Backward compatible** with existing functionality
✅ **Instant queries** with no I/O overhead

The feature enhances user visibility into recent server activity with a simple, performant approach that doesn't require persistent storage.
