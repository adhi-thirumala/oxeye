# Oxeye Architecture Decisions

## Problem Statement

Currently, all Discord servers share a single SQLite database. When one server's players join/leave, it causes global write contention affecting all other servers.

```
Server A: /join "Steve"  ─┐
Server B: /join "Alex"   ─┼─► Single SQLite = serialized writes
Server C: /leave "Notch" ─┘
```

---

## Options Considered

### Option 1: PostgreSQL Container

| Aspect | Details |
|--------|---------|
| Locking | Row-level MVCC - concurrent writes to different rows |
| Memory | ~100MB base |
| Latency | ~0.3ms via Unix socket |
| Complexity | Medium (container, backups, upgrades) |

**Verdict:** Overkill. Adds operational complexity for a problem solvable in-process.

### Option 2: Per-Server SQLite (Turso-style)

| Aspect | Details |
|--------|---------|
| Isolation | Complete - one DB file per Discord guild |
| Memory | ~10MB |
| Complexity | Medium (N files, N connections) |

**Verdict:** Good isolation, but still managing many DB files.

### Option 3: Hybrid (SQLite + In-Memory) ✓ CHOSEN

| Data | Storage | Why |
|------|---------|-----|
| `servers`, `pending_links` | SQLite | Persistent, rarely written |
| `online_players` | In-memory | Ephemeral, high write rate, resyncs on reconnect |

**Verdict:** Best fit. Matches data durability requirements, minimal complexity.

---

## In-Memory Storage Design

### Why Not a Separate Redis/Valkey Container?

No need. The data is ephemeral and resyncs on reconnect anyway. A simple in-process data structure is:
- Faster (no network/serialization)
- Simpler (no container to manage)
- Sufficient (player data self-heals on reconnect)

### Data Structure Selection

#### Map Implementation: `scc::HashMap`

| Crate | Mechanism | Why not |
|-------|-----------|---------|
| `dashmap` | Sharded RwLock | Not truly lock-free |
| `scc` ✓ | Lock-free | True lock-free reads AND writes |
| `flurry` | Lock-free | More complex API |

#### Collection for Players: `Vec<ArrayString<16>>`

| Choice | Reasoning |
|--------|-----------|
| `Vec` over `HashSet` | Smaller memory, cache-friendly at small N |
| `ArrayString<16>` over `String` | Inline storage, no heap allocation, Minecraft names max 16 chars |

**Memory layout:**
```
Vec<String>:          [ptr|len|cap] → heap (scattered)
Vec<ArrayString<16>>: [bytes inline] [bytes inline] (contiguous)
```

Fully contiguous memory = CPU prefetcher friendly, zero pointer chasing.

### Final Structure

```rust
use scc::HashMap;
use arrayvec::ArrayString;

type PlayerName = ArrayString<16>;

struct ServerState {
    players: Vec<PlayerName>,
    synced_since_boot: bool,
}

struct OnlineCache {
    servers: HashMap<String, ServerState>,  // api_key_hash -> state
}
```

---

## Handling Backend Restarts

### Problem

If backend restarts, in-memory cache is empty. Mod doesn't know it needs to resync.

### Solution: Boot ID

Backend generates a unique ID on startup, returns it in every response header.

```
Normal operation:
  /join "Steve" → 200, X-Boot-ID: abc123
  /leave "Steve" → 200, X-Boot-ID: abc123
  (mod sees same ID, does nothing)

After backend restart:
  /join "Alex" → 200, X-Boot-ID: xyz789  ← different!
  (mod detects change, immediately sends /sync)
```

### Tracking Sync State

Backend knows which servers exist (from SQLite `servers` table) but doesn't know if they've synced since restart.

```rust
// On startup: load servers from SQLite, mark all unsynced
let cache: HashMap<String, ServerState> = db.get_all_servers()
    .map(|s| (s.api_key_hash, ServerState {
        players: Vec::new(),
        synced_since_boot: false,
    }))
    .collect();
```

Discord bot can show:
```
🟢 SMP Server - 5 players online
⏳ Creative Server - awaiting sync
🟢 Minigames - 0 players online
```

---

## Request Ordering with Generations

### Problem

Network can reorder requests. A `/sync` might arrive after a `/join` that happened before it.

```
T=0: /sync [A, B, C] sent (slow network)
T=1: /join D sent (fast)
T=2: /join D arrives → state = [D]
T=3: /sync arrives → state = [A, B, C] ← D is LOST
```

### Solution: Generation Numbers

Mod increments generation BEFORE sending sync. All events carry their generation.

```rust
// Mod side
fn sync(players: Vec<Player>) {
    self.gen += 1;  // increment FIRST
    send("/sync", players, self.gen);
}

fn join(player: Player) {
    send("/join", player, self.gen);  // uses current gen
}
```

Backend logic:
```rust
fn handle_sync(players, gen) {
    current_gen = gen;
    state = players;
}

fn handle_join(player, gen) {
    if gen < current_gen {
        return;  // stale event, drop it
    }
    state.push(player);
}
```

### Why Drops Are Correct

Dropped events are from OLD generations (before a sync). Examples:

1. **MC server restarted:** Old gen events are from dead session
2. **Backend restarted:** Mod detected via boot ID, sent new sync, old events are stale

Events within the SAME generation are never dropped - they're additive to the sync.

---

## Contention Analysis

### SQLite (persistent data)

| Operation | Frequency | Contention |
|-----------|-----------|------------|
| Create link code | Rare (setup) | Low |
| Consume code + register server | Rare (setup) | Low |
| Cleanup expired links | Periodic | Low |

**Verdict:** Negligible. Only happens during server registration.

### In-Memory Cache (online players)

| Scenario | Contention |
|----------|------------|
| Different servers | **Zero** (different keys) |
| Same server, different players | Serialized on same key (unavoidable, correct) |
| Sync vs join on same server | Handled by generations |

**Verdict:** Same-server serialization is unavoidable and fast (~100ns). Different servers never contend.

---

## Summary

| Component | Technology | Reason |
|-----------|------------|--------|
| Persistent storage | SQLite | Simple, sufficient for rare writes |
| Online players cache | `scc::HashMap<String, ServerState>` | Lock-free, in-process |
| Player names | `Vec<ArrayString<16>>` | Contiguous, no heap |
| Restart detection | Boot ID header | Zero extra requests |
| Stale state tracking | `synced_since_boot` flag | Clear UX for Discord bot |
| Request ordering | Generation numbers | Handles network reordering |
