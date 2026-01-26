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
pub struct ServerState {
    /// Online players with their join timestamps.
    pub players: Vec<(PlayerName, i64)>,

    /// NEW: Recently offline players with their last_seen timestamps.
    /// Entries are lazily removed when older than the configured window.
    pub last_seen: HashMap<PlayerName, i64>,  // Unix timestamp

    /// Whether this server has synced since backend restart.
    pub synced_since_boot: bool,
}
```

The existing cache structure remains:
```rust
pub type OnlineCache = scc::HashMap<String, ServerState>;  // api_key_hash -> ServerState
```

**Why use the existing SCC cache?**
- Already thread-safe: `scc::HashMap` provides lock-free concurrent access
- Per-server isolation: `last_seen` is scoped to each server's `ServerState`
- No additional data structures: Extends existing architecture naturally
- Performance: Instant lookups with no database queries or additional locks
- Simplicity: No separate cache management logic needed
- Lightweight: Only stores timestamps (~8 bytes per player)
- No persistence needed: Data resets on restart (acceptable for "recent activity")

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

### 3. Data Retention & TTL Cleanup

**Retention policy: Configurable time window (default 24 hours)**

**Lazy cleanup strategy (O(1) amortized, not O(n)):**

Instead of scanning all entries periodically, we remove stale entries **as we access them**:

1. **During queries** (`get_recently_offline()`):
   - Remove `last_seen` entries older than threshold **while iterating**
   - Only touches entries we're reading anyway
   - O(k) where k = entries checked, not O(n) for all players

2. **On player events** (join/leave/sync):
   - When updating a player's `last_seen`, remove it if it's too old
   - Natural cleanup during normal operation

This approach:
- ✅ No O(n) scans of all cached players
- ✅ No background cleanup tasks needed
- ✅ Memory naturally bounded by activity rate
- ✅ Each operation removes stale data for players it touches

**Why 24 hours default?**
- Covers typical "who was on today" use case
- Lightweight memory footprint (~8 bytes per player)
- Configurable via environment variable for users who want longer history
- Lazy cleanup keeps memory usage minimal

### 4. Query Interface

**Extended ServerState methods:**

```rust
impl ServerState {
    /// Update last_seen timestamp for a player.
    /// Also removes the entry if it's older than the cutoff (lazy cleanup).
    pub fn update_last_seen(&mut self, player_name: PlayerName, now: i64, cutoff: i64) {
        // Remove if too old (lazy cleanup)
        if now < cutoff {
            self.last_seen.remove(&player_name);
            return;
        }
        self.last_seen.insert(player_name, now);
    }

    /// Get recently offline players (not currently online).
    /// Removes stale entries older than cutoff during iteration (lazy cleanup).
    pub fn get_recently_offline(&mut self, cutoff: i64) -> Vec<OfflinePlayerInfo> {
        let online_players: HashSet<_> = self.players.iter().map(|(n, _)| n).collect();

        // Collect recent offline players and remove stale entries
        let mut result = Vec::new();
        self.last_seen.retain(|name, &mut last_seen| {
            if last_seen < cutoff {
                false  // Remove stale entry (lazy cleanup)
            } else if !online_players.contains(name) {
                result.push(OfflinePlayerInfo { player_name: *name, last_seen });
                true  // Keep entry
            } else {
                true  // Keep entry (player is online)
            }
        });

        result
    }
}

pub struct OfflinePlayerInfo {
    pub player_name: PlayerName,
    pub last_seen: i64,  // Unix timestamp
}
```

**Key optimization:**
- `update_last_seen()` performs lazy cleanup on single entries
- `get_recently_offline()` uses `HashMap::retain()` to remove stale entries during iteration
- No separate O(n) cleanup needed

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
- `last_seen` HashMap entry: ~(16 bytes PlayerName + 8 bytes timestamp) = ~24 bytes per player
- For 100 unique players in 24h: ~2.4KB per server
- Stored within existing `ServerState` in SCC cache
- Lazy cleanup prevents unbounded growth
- Negligible compared to player skin cache

**Query efficiency:**
- Timestamp update: O(1) HashMap insert
- Recent activity query: O(k) where k = `last_seen` entries (bounded by lazy cleanup)
- No database I/O required
- No global locks (SCC HashMap is lock-free)
- For typical server (100 players): < 1ms query time

**Expected overhead:**
- Timestamp update on join/leave: < 0.1ms (HashMap insert)
- Recent activity query with lazy cleanup: < 1ms (in-memory filtering + retain)
- Memory: ~24 bytes per unique player tracked

**Lazy cleanup efficiency:**
- No O(n) scans across all servers
- Stale entries removed during normal reads/writes
- Memory bounded by: `(activity_rate) × (window_duration)`
- Example: 100 players/day × 24h window = ~2.4KB per server

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
1. **Extend `ServerState` struct** (oxeye-db/src/cache.rs):
   - Add `last_seen: HashMap<PlayerName, i64>` field
   - Implement `update_last_seen()` with lazy cleanup
   - Implement `get_recently_offline()` with lazy cleanup via `retain()`
2. **Update `ServerState::new()`** to initialize empty `last_seen` HashMap
3. **Add unit tests** for:
   - Last seen tracking
   - Lazy cleanup during queries
   - Stale entry removal

### Phase 2: Database Layer Integration
1. **Add helper method** to `Database` (oxeye-db/src/lib.rs):
   - `get_recently_offline(api_key_hash, window_secs) -> Vec<OfflinePlayerInfo>`
   - Calls `ServerState::get_recently_offline()` on the cached state
2. **Modify existing methods** to call `update_last_seen()`:
   - `player_join()` - update last_seen before adding to online list
   - `player_leave()` - update last_seen after removing from online list
   - `sync_players()` - update last_seen for all players in sync
   - `delete_server_by_api_key()` - last_seen is cleared when server cache entry is removed

### Phase 3: API Integration
1. **Configuration**: Add `RECENT_ACTIVITY_WINDOW_HOURS` env var to backend
2. **Modify routes** (oxeye-backend/src/routes.rs):
   - `POST /join`, `/leave`, `/sync` already call DB methods, which now update last_seen
   - No route changes needed (logic is in DB layer)

### Phase 4: Discord Command Enhancement
1. **Enhance `/oxeye status`** (oxeye-bot):
   - Call `db.get_recently_offline()` after getting online players
   - Format as text list: "Last seen (within 24h): • player (Xm ago)"
2. **Add time formatting helper** for human-readable durations
3. **Ensure no picture changes** for offline players (text only)

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
✅ **Lock-free concurrency** - leverages existing `scc::HashMap` thread safety
✅ **Lazy cleanup (not O(n))** - removes stale entries during reads/writes, not periodic scans
✅ **Per-server isolation** - `last_seen` scoped to each server's state
✅ **Minimal memory overhead** (~24 bytes per player, bounded by lazy cleanup)
✅ **Simple last seen timestamps** for each player
✅ **Human-readable durations** in Discord embeds
✅ **No database changes** required
✅ **Backward compatible** with existing functionality
✅ **Instant queries** with no I/O overhead

The feature enhances user visibility into recent server activity by naturally extending the existing cache architecture with efficient lazy cleanup.
