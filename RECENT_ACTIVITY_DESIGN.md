# Recent Activity & Offline Duration Feature Design

## Overview

This feature extends Oxeye to track **last seen timestamps** for recently offline players, enabling users to see when players were last on their Minecraft servers within a configurable time window (default: last 24 hours). This information is displayed as text additions to the existing `/oxeye status` command.

## Problem Statement

Currently, Oxeye only tracks players who are **currently online**. Once a player disconnects, all information about them is lost. Users want to see when players were last online within the existing `/oxeye status` command, without needing a separate command or persistent history.

## Design Decisions

### 1. Data Storage Strategy

**Choice: Extend existing SCC cache with last_seen tracking**

We'll track recent player activity by extending the existing `ServerState` struct within the current `scc::HashMap`-based online cache. No new data structures needed:

```rust
/// Tracks player activity state including online status and timestamps.
#[derive(Clone, Debug)]
pub struct PlayerActivity {
    pub player_name: PlayerName,
    pub last_seen: i64,      // Unix timestamp of last activity
    pub joined_at: i64,      // Unix timestamp when player joined (0 if offline)
    pub is_online: bool,     // Current online status
}

pub struct ServerState {
    /// All tracked players with their activity state.
    /// Includes both online and recently offline players.
    /// Entries are lazily removed when older than the configured window.
    pub player_activity: Vec<PlayerActivity>,

    /// Whether this server has synced since backend restart.
    pub synced_since_boot: bool,
}
```

The existing cache structure remains:
```rust
pub type OnlineCache = scc::HashMap<String, ServerState>;  // api_key_hash -> ServerState
```

**Why use the existing SCC cache with a proper struct?**
- Already thread-safe: `scc::HashMap` provides lock-free concurrent access
- Per-server isolation: `player_activity` is scoped to each server's `ServerState`
- Clean data model: Single `PlayerActivity` struct instead of separate collections
- Easy filtering: Vec allows simple iteration for online/offline queries
- No redundant data: One source of truth for each player's state
- Performance: Linear scan through Vec is fast for typical server sizes (<100 players)
- Simplicity: No HashMap lookups or synchronization between separate structures
- Lightweight: ~33 bytes per player (PlayerName + 3×i64 + bool)
- No persistence needed: Data resets on restart (acceptable for "recent activity")

### 2. Activity Tracking

**When to update player activity:**

1. **Player joins** (`POST /join`)
   - Find existing `PlayerActivity` or create new one
   - Set `is_online = true`, `joined_at = now`, `last_seen = now`

2. **Player leaves** (`POST /leave`)
   - Find player's `PlayerActivity` entry
   - Set `is_online = false`, `joined_at = 0`, `last_seen = now`
   - Entry stays in Vec for configured window (lazy cleanup removes later)

3. **Server sync** (`POST /sync`)
   - For all players in sync list: update their activity (mark online if not already)
   - For tracked players not in sync: mark as offline (`is_online = false`, `last_seen = now`)

4. **Server disconnection**
   - Mark all tracked players as offline
   - Set `last_seen = now` for all
   - Entries stay in Vec until lazy cleanup removes them

### 3. Data Retention & TTL Cleanup

**Retention policy: Configurable time window (default 24 hours)**

**Lazy cleanup strategy:**

Instead of scanning all entries periodically, we remove stale entries **as we access them**:

1. **During queries** (`get_online_players()`, `get_recently_offline()`):
   - Filter out `PlayerActivity` entries where `last_seen < threshold`
   - Use `Vec::retain()` to remove stale entries during iteration
   - O(n) where n = entries in this server's Vec (typically <100 players)

2. **On player updates** (join/leave/sync):
   - When updating a player, check if `last_seen` is too old
   - Remove stale entry before adding updated one

This approach:
- ✅ No background cleanup tasks needed
- ✅ Memory naturally bounded by activity rate × window duration
- ✅ Cleanup happens during normal operations
- ✅ Linear scan is fast for typical server sizes (<100 players)

**Why 24 hours default?**
- Covers typical "who was on today" use case
- Lightweight memory footprint (~33 bytes per player)
- Configurable via environment variable for users who want longer history
- Lazy cleanup keeps memory usage minimal

### 4. Query Interface

**Extended ServerState methods:**

```rust
impl ServerState {
    /// Update or create player activity entry.
    /// Performs lazy cleanup by removing stale entries during update.
    pub fn update_player_activity(
        &mut self,
        player_name: PlayerName,
        is_online: bool,
        now: i64,
        cutoff: i64,
    ) {
        // Lazy cleanup: remove stale entries while searching
        self.player_activity.retain(|p| p.last_seen >= cutoff);

        // Find existing player or add new entry
        if let Some(activity) = self.player_activity.iter_mut().find(|p| p.player_name == player_name) {
            activity.last_seen = now;
            activity.is_online = is_online;
            activity.joined_at = if is_online { now } else { 0 };
        } else {
            self.player_activity.push(PlayerActivity {
                player_name,
                last_seen: now,
                joined_at: if is_online { now } else { 0 },
                is_online,
            });
        }
    }

    /// Get currently online players.
    /// Performs lazy cleanup by removing stale entries.
    pub fn get_online_players(&mut self, cutoff: i64) -> Vec<PlayerActivity> {
        // Lazy cleanup while collecting
        self.player_activity.retain(|p| p.last_seen >= cutoff);
        self.player_activity.iter()
            .filter(|p| p.is_online)
            .cloned()
            .collect()
    }

    /// Get recently offline players (not currently online).
    /// Performs lazy cleanup by removing stale entries.
    pub fn get_recently_offline(&mut self, cutoff: i64) -> Vec<PlayerActivity> {
        // Lazy cleanup while collecting
        self.player_activity.retain(|p| p.last_seen >= cutoff);
        self.player_activity.iter()
            .filter(|p| !p.is_online)
            .cloned()
            .collect()
    }

    /// Get all recent activity (online + offline).
    pub fn get_all_activity(&mut self, cutoff: i64) -> Vec<PlayerActivity> {
        // Lazy cleanup
        self.player_activity.retain(|p| p.last_seen >= cutoff);
        self.player_activity.clone()
    }
}
```

**Key benefits:**
- Single source of truth: `PlayerActivity` struct contains all player state
- Simple Vec operations: No HashMap lookups or separate data structures
- Lazy cleanup: `Vec::retain()` removes stale entries during queries
- Easy filtering: `is_online` flag makes online/offline queries straightforward
- Clone-friendly: Small struct (~33 bytes) is cheap to copy

### 5. Discord Commands

**Enhanced `/oxeye status` command:**

Update the existing status command to show last seen time for recently offline players. The existing embedded image/picture showing online players remains unchanged - offline players are only shown as text additions below.

**Example output:**

```
[Existing server status embed with player head images - unchanged]

Last seen (within 24h):
• jeb_ (30m ago)
• Dinnerbone (2h ago)
• Herobrine (5h ago)
```

**Key points:**
- **No new commands** - all functionality integrated into existing `/oxeye status`
- **No picture/image changes** - player head images only shown for currently online players (existing behavior)
- **Text-only additions** - offline players shown as simple text list below the online players
- **Configurable window** - default 24h, configurable via environment variable

### 6. API Changes

No new HTTP endpoints needed. The existing endpoints will be enhanced to update last seen timestamps:

**`POST /join`** - Update player's last seen timestamp
**`POST /leave`** - Update player's last seen timestamp
**`POST /sync`** - Update last seen timestamps for all players in sync
**`POST /disconnect`** - Update last seen timestamps for all active players on server

### 7. Performance Considerations

**Memory overhead:**
- `PlayerActivity` struct: ~33 bytes (PlayerName:16 + last_seen:8 + joined_at:8 + is_online:1)
- For 100 unique players in 24h: ~3.3KB per server
- Stored within existing `ServerState` in SCC cache
- Lazy cleanup prevents unbounded growth
- Negligible compared to player skin cache

**Query efficiency:**
- Update player activity: O(n) linear search through Vec (n = players on this server)
- Recent activity query: O(n) where n = tracked players (typically <100)
- No database I/O required
- No global locks (SCC HashMap is lock-free)
- For typical server (100 players): < 1ms query time

**Expected overhead:**
- Player update on join/leave: < 0.5ms (linear search + lazy cleanup)
- Recent activity query with lazy cleanup: < 1ms (Vec filter + retain)
- Memory: ~33 bytes per unique player tracked

**Why Vec over HashMap:**
- Simpler: No key management, single data structure
- Faster for small n: Linear scan of <100 entries is ~microseconds
- Better cache locality: Contiguous memory layout
- Less memory: No hash table overhead
- Lazy cleanup efficiency: `Vec::retain()` is highly optimized

**Lazy cleanup efficiency:**
- No O(n) scans across all servers
- Stale entries removed during normal reads/writes per-server
- Memory bounded by: `(activity_rate) × (window_duration)`
- Example: 100 players/day × 24h window = ~3.3KB per server

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
1. **Define `PlayerActivity` struct** (oxeye-db/src/cache.rs):
   - Fields: `player_name`, `last_seen`, `joined_at`, `is_online`
   - Derive `Clone`, `Debug` traits
2. **Update `ServerState` struct** (oxeye-db/src/cache.rs):
   - Replace separate player tracking with `player_activity: Vec<PlayerActivity>`
   - Implement `update_player_activity()` with lazy cleanup
   - Implement `get_online_players()`, `get_recently_offline()`, `get_all_activity()`
3. **Update `ServerState::new()`** to initialize empty `player_activity` Vec
4. **Add unit tests** for:
   - Player activity tracking (join/leave transitions)
   - Online/offline filtering
   - Lazy cleanup during queries
   - Stale entry removal

### Phase 2: Database Layer Integration
1. **Add helper methods** to `Database` (oxeye-db/src/lib.rs):
   - `get_online_players(api_key_hash, window_secs) -> Vec<PlayerActivity>`
   - `get_recently_offline(api_key_hash, window_secs) -> Vec<PlayerActivity>`
   - Both call corresponding `ServerState` methods
2. **Refactor existing player tracking methods** to use `PlayerActivity`:
   - `player_join()` - call `update_player_activity(name, true, now, cutoff)`
   - `player_leave()` - call `update_player_activity(name, false, now, cutoff)`
   - `sync_players()` - update all synced players as online, mark missing as offline
   - `get_server_state()` - return online players from `player_activity` Vec
3. **Migration strategy**:
   - Existing `players: Vec<(PlayerName, i64)>` is replaced by filtering `player_activity`
   - Backward compatible: Online player queries filter `is_online == true`

### Phase 3: API Integration
1. **Configuration**: Add `RECENT_ACTIVITY_WINDOW_HOURS` env var to backend
2. **Modify routes** (oxeye-backend/src/routes.rs):
   - `POST /join`, `/leave`, `/sync` already call DB methods, which now update last_seen
   - No route changes needed (logic is in DB layer)

### Phase 4: Discord Command Enhancement
1. **Enhance `/oxeye status`** (oxeye-bot):
   - Call `db.get_online_players()` for online list (replaces existing call)
   - Call `db.get_recently_offline()` for offline list
   - Format offline players as text list: "Last seen (within 24h): • player (Xm ago)"
2. **Add time formatting helper** for human-readable durations
3. **Ensure no picture changes** for offline players (text only)
4. **Update existing status display** to work with `PlayerActivity` struct

### Phase 5: Testing & Polish
1. Integration tests for activity tracking
2. Test lazy cleanup efficiency
3. Test edge cases (restarts, disconnects, etc.)
4. Documentation updates

## Configuration

**New environment variable:**

```bash
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
- No database schema changes required
- No breaking changes to existing commands or APIs
- Activity tracking starts immediately upon deployment
- After backend restart, tracking resumes from first sync/join/leave events

## Summary

This design provides a lightweight foundation for tracking recent player activity:

✅ **Uses existing SCC cache** - extends `ServerState` struct, no new data structures
✅ **Clean data model** - single `PlayerActivity` struct with all player state
✅ **Lock-free concurrency** - leverages existing `scc::HashMap` thread safety
✅ **Lazy cleanup** - removes stale entries during reads/writes, not periodic scans
✅ **Per-server isolation** - `player_activity` scoped to each server's state
✅ **Minimal memory overhead** (~33 bytes per player, bounded by lazy cleanup)
✅ **Simple Vec operations** - no HashMap overhead, better cache locality
✅ **Complete player state** - timestamps, online status, join time in one struct
✅ **Human-readable durations** in Discord embeds
✅ **No database changes** required
✅ **Backward compatible** with existing functionality
✅ **Instant queries** with no I/O overhead

The feature enhances user visibility into recent server activity by naturally extending the existing cache architecture with a clean struct-based design and efficient lazy cleanup.
