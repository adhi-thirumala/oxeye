# Minecraft Skin Head Images - Design Document

**Date:** 2026-01-02
**Status:** Design Phase
**Goal:** Display Minecraft player head images in Discord embeds while minimizing rendering operations and network transfers

---

## Table of Contents

1. [System Goals](#system-goals)
2. [Design Decisions](#design-decisions)
   - [Decision 1: Skin Change Detection](#decision-1-skin-change-detection)
   - [Decision 2: Raw Skin Storage](#decision-2-raw-skin-storage)
   - [Decision 3: Rendered Head Storage](#decision-3-rendered-head-storage)
   - [Decision 4: Network Protocol](#decision-4-network-protocol)
   - [Decision 5: Rendering Strategy](#decision-5-rendering-strategy)
   - [Decision 6: Image Serving](#decision-6-image-serving)
   - [Decision 7: Database Schema](#decision-7-database-schema)
3. [Final Architecture](#final-architecture)
4. [Data Flow Examples](#data-flow-examples)
5. [Trade-offs and Rationale](#trade-offs-and-rationale)

---

## System Goals

### Primary Objectives
1. **Minimize rendering operations** - Render each unique skin exactly once, cache forever
2. **Minimize network transfers** - Only send skin data when backend doesn't have it
3. **Minimize storage overhead** - Deduplicate skins, efficient storage
4. **Low latency** - Fast Discord embed responses

### Functional Requirements
- Display player head images in Discord `/status` embeds
- Support both online-mode (verified) and offline-mode (cracked) servers
- Handle serverside skin plugins (SkinsRestorer, etc.)
- Detect and handle skin changes automatically
- Graceful fallback for missing/failed renders

---

## Design Decisions

### Decision 1: Skin Change Detection

**Problem:** How do we detect when a player's skin has changed without re-downloading every time?

#### Options Considered

**Option A: Hash the texture URL**
- Hash: `SHA256("http://textures.minecraft.net/texture/abc123...")`
- ✅ Very cheap (just hash a string)
- ❌ Same skin can have different URLs (false positives)
- ❌ Wastes renders on URL rotation

**Option B: Hash the PNG bytes**
- Hash: `SHA256(skin_png_bytes)`
- ✅ 100% accurate
- ✅ Works for any skin source
- ❌ Requires downloading skin first (network cost)

**Option C: Hash the texture value from GameProfile** ⭐ CHOSEN
- Hash: `SHA256(texturesProperty.getValue())`
- ✅ Already in memory (no download needed)
- ✅ Changes if and only if skin changes
- ✅ Works for both online and offline servers
- ✅ Fast to compute

#### Decision: Option C

**Rationale:**
Serverside skin plugins MUST populate GameProfile's texture property in Mojang's exact format for vanilla clients to render the skin. This means we can rely on the texture value as a canonical identifier for the skin, regardless of server mode.

**Implementation:**
```java
Property texturesProperty = profile.getProperties().get("textures").stream().findFirst().orElse(null);
String textureHash = SHA256(texturesProperty.getValue());
```

**Benefits:**
- Zero network overhead
- Works universally (online/offline/plugins)
- Detectable entirely in the Fabric mod

---

### Decision 2: Raw Skin Storage

**Problem:** Where should we store the raw skin texture PNG files (~10KB each)?

#### Options Considered

**Option A: SQLite BLOB** ⭐ CHOSEN
```sql
CREATE TABLE skins (
    texture_hash TEXT PRIMARY KEY,
    skin_data BLOB NOT NULL  -- ~10KB PNG
);
```

**Pros:**
- Everything in one place (simple backup)
- ACID transactions (no orphaned data)
- SQLite handles 10KB BLOBs efficiently
- Page cache keeps hot data in RAM
- Faster serving (no filesystem syscalls)

**Cons:**
- Makes database file larger (~10MB per 1000 skins)

**Option B: Filesystem**
```
/data/skins/{texture_hash}.png
```

**Pros:**
- Easy to inspect files for debugging
- Database stays smaller

**Cons:**
- Two storage systems to manage
- Orphaned files possible
- Backup complexity
- Slower serving (filesystem syscalls)

#### Decision: Option A (SQLite BLOB)

**Rationale:**
Storage is cheap. For 10KB files, SQLite BLOB storage is faster and simpler. Even with 10,000 unique skins, the overhead is only ~100MB, which is trivial on modern systems.

**Why it's faster:**
- SQLite page cache keeps frequently accessed BLOBs in RAM
- No filesystem syscall overhead (open/read/close)
- No path resolution
- Direct memory access for serving

---

### Decision 3: Rendered Head Storage

**Problem:** Where should we store the rendered head images (64x64 PNG, ~2KB each)?

#### Options Considered

**Option A: Same table as skins**
```sql
CREATE TABLE skins (
    skin_data BLOB,      -- 10KB
    rendered_head BLOB   -- 2KB
);
```

**Pros:** One query gets everything

**Cons:** Loading head requires loading 10KB skin too (wasted bandwidth)

**Option B: Separate table** ⭐ CHOSEN
```sql
CREATE TABLE skins (
    skin_data BLOB  -- 10KB
);

CREATE TABLE rendered_heads (
    head_data BLOB  -- 2KB
);
```

**Pros:**
- Only load what you need (2KB vs 12KB)
- Clean separation of concerns
- Can rebuild heads without touching skins

**Cons:** Slightly more complex (but minimal)

**Option C: Don't store raw skins**

Considered discarding raw skins after rendering, but decided against it because:
- Can't re-render if algorithm improves
- Can't render different sizes/styles later
- Can't recover if render fails
- Storage is cheap

#### Decision: Option B (Separate Table)

**Rationale:**
Discord will request heads frequently, but we rarely need the raw skin. Separating them saves 10KB per request. The complexity overhead is minimal (just another table).

---

### Decision 4: Network Protocol

**Problem:** What data should the Fabric mod send to the backend on player join?

**Current Protocol:**
```json
POST /join
{"player": "Steve"}
```

#### Options Considered

**Option A: Send everything every time**
```json
POST /join
{
  "player": "Steve",
  "texture_url": "...",
  "texture_hash": "...",
  "skin_data": "base64..."  // 13KB!
}
```

**Cons:** Wastes 13KB per join even if skin unchanged. With 100 players × 10 joins/hour = 13MB/hour = 2.2GB/week

**Option B: Send hash, backend fetches if needed**
```json
POST /join
{
  "player": "Steve",
  "texture_url": "...",
  "texture_hash": "..."
}
```

**Cons:** Doesn't work for offline servers where URLs are invalid/inaccessible

**Option C: Two-step protocol** ⭐ CHOSEN
```json
// Step 1: Mod sends hash
POST /join
{
  "player": "Steve",
  "texture_hash": "d4f7e8a9..."
}

// Step 2a: Backend already has it
Response: 200 OK

// Step 2b: Backend needs it
Response: 202 Accepted

// Step 3: Mod sends skin (only if 202)
POST /skin
{
  "texture_hash": "d4f7e8a9...",
  "skin_data": "base64_encoded_png"
}

Response: 200 OK
```

#### Decision: Option C (Two-Step Protocol)

**Rationale:**
- **Massive bandwidth savings:** 200 bytes per join (hash only) vs 13KB (full skin)
- For repeat joins: 200 bytes × 1000 joins = 195KB vs 13MB (67x reduction)
- **Works with offline servers:** Mod sends skin data directly, no URL dependency
- **SkinsRestorer compatibility:** URLs may be local/fake, so backend can't fetch
- **Simple enough:** One round-trip on first join, instant on repeat joins

**HTTP Status Codes:**
- `200 OK` = "I have this skin, all good"
- `202 Accepted` = "I got your request, but send me the skin data next"

**Why 202 Accepted?**
It's semantically correct for "I accepted your request but need follow-up action." It's a success code (not an error), which is exactly what we want.

---

### Decision 5: Rendering Strategy

**Problem:** When should we render the head image from the skin?

#### Options Considered

**Option A: Render synchronously on skin arrival**
- Blocks `/skin` request until render completes (~50ms)
- ❌ Slows down player join

**Option B: Fire-and-forget async** ⭐ CHOSEN
```rust
POST /skin arrives
→ Store skin
→ tokio::spawn(render_head())  // Background task
→ Return 200 OK immediately
```

**Option C: Render on-demand (lazy)**
- Only render when Discord requests head
- ❌ First request is slow

**Option D: Hybrid (async + lazy fallback)**
- Complex, unnecessary with good fallback strategy

#### Decision: Option B (Fire-and-forget) + Steve Head Fallback

**Rationale:**
- Doesn't block join request
- Head usually ready before Discord requests it
- If render fails or Discord requests immediately, serve default Steve head
- Simple, robust, performant

**Fallback Strategy:**
```rust
async fn get_head(player: String) -> Vec<u8> {
    db.get_head(&player).await
        .unwrap_or(DEFAULT_STEVE_HEAD)
}
```

Embed Steve head PNG bytes in binary at compile time:
```rust
const DEFAULT_STEVE_HEAD: &[u8] = include_bytes!("assets/steve_head.png");
```

**Why Steve?**
- Universally recognizable
- Classic Minecraft default
- Single fallback to maintain

---

### Decision 6: Image Serving

**Problem:** How should Discord access head images?

#### Options Considered

**Option A: By player name**
```
GET /heads/Steve.png
```

**Pros:** Simple, readable URLs

**Cons:** URL changes meaning when player changes skin (bad for caching)

**Option B: By texture hash** ⭐ CHOSEN
```
GET /heads/d4f7e8a9b2c1.png
```

**Pros:**
- Immutable URLs (hash never changes)
- Perfect for caching
- Discord can cache forever
- Old embeds stay consistent

**Cons:** Requires `/status` command to know each player's hash

#### Decision: Option B (By Texture Hash)

**Rationale:**
Immutable URLs are ideal for CDN caching. Discord's CDN can cache these forever with:
```rust
(
    header::CACHE_CONTROL,
    "public, immutable, max-age=31536000"  // 1 year
)
```

**Implementation in Discord Bot:**
```rust
async fn status_command(server: String) -> Embed {
    let players = db.get_online_players(&server).await?;

    for player in players {
        let hash = db.get_player_texture_hash(&player.name).await?;
        embed.thumbnail(format!("https://backend.com/heads/{}.png", hash));
    }
}
```

This requires the `player_skins` table to map `player_name → texture_hash`.

---

### Decision 7: Database Schema

**Problem:** How should we structure the database?

#### Final Schema

```sql
-- Stores unique skin textures (deduplicated)
CREATE TABLE skins (
    texture_hash TEXT PRIMARY KEY,  -- SHA256 of GameProfile texture value
    texture_url TEXT,                -- Original URL (for reference)
    skin_data BLOB NOT NULL          -- Raw PNG bytes (~10KB)
);

-- Maps players to their current skin
CREATE TABLE player_skins (
    player_name TEXT PRIMARY KEY,
    texture_hash TEXT NOT NULL,
    last_updated INTEGER NOT NULL,   -- Unix timestamp
    FOREIGN KEY (texture_hash) REFERENCES skins(texture_hash)
);

-- Stores rendered head images (one per unique skin)
CREATE TABLE rendered_heads (
    texture_hash TEXT PRIMARY KEY,
    head_data BLOB NOT NULL,         -- Rendered PNG (~2KB)
    rendered_at INTEGER NOT NULL,    -- Unix timestamp
    FOREIGN KEY (texture_hash) REFERENCES skins(texture_hash)
);
```

#### Design Principles

**Deduplication:**
- Multiple players can share the same skin
- One skin entry, one rendered head entry
- Example: 10 players with Steve skin = 1 skin + 1 head render

**Why no indexes?**
All queries use PRIMARY KEYs which are automatically indexed by SQLite:
- `SELECT * FROM skins WHERE texture_hash = ?` ← PRIMARY KEY
- `SELECT * FROM player_skins WHERE player_name = ?` ← PRIMARY KEY
- `SELECT * FROM rendered_heads WHERE texture_hash = ?` ← PRIMARY KEY

No additional indexes needed.

**Why no `first_seen` timestamp?**
Not needed for core functionality. YAGNI (You Ain't Gonna Need It). Can add later if debugging requires it.

#### Integration with In-Memory Architecture

**Note:** Per the architectural redesign (see `docs/architecture-decisions.md`), **active/online players are stored in-memory** using `scc::HashMap`, not in SQLite. This design only concerns persistent skin data.

**Where data lives:**
- **In-Memory** (`scc::HashMap`): Which players are currently online (ephemeral, resyncs on reconnect)
- **SQLite** (`player_skins`): Which skin each player last had (persistent)
- **SQLite** (`skins`, `rendered_heads`): Skin textures and rendered heads (persistent)

**Discord bot flow:**
1. Query in-memory cache → get list of online player names
2. For each player, query `player_skins` table → get their `texture_hash`
3. Build embed with image URL: `https://backend/heads/{texture_hash}.png`

**Concurrency guarantee:**
SQLite in WAL mode supports concurrent reads during writes. Skin data reads (serving Discord embeds) won't be blocked by skin data writes (player joins with new skins).

---

## Final Architecture

### System Components

```
┌─────────────────┐
│  Minecraft      │
│  Server         │
│  (Fabric Mod)   │
└────────┬────────┘
         │
         │ POST /join (hash only)
         │ POST /skin (if 202)
         │ POST /leave
         │ POST /sync
         │
         ▼
┌──────────────────────────────────────────────────────────┐
│  Rust Backend (Axum)                                     │
│  ┌────────────────────────────────────────────────────┐  │
│  │ In-Memory Cache (scc::HashMap)                     │  │
│  │ - Online players (ephemeral, resyncs on reconnect)│  │
│  └────────────────────────────────────────────────────┘  │
│  ┌────────────────────────────────────────────────────┐  │
│  │ SQLite Database (persistent)                       │  │
│  │ - skins (10KB each)                                │  │
│  │ - player_skins (player → texture_hash mapping)     │  │
│  │ - rendered_heads (2KB each)                        │  │
│  └────────────────────────────────────────────────────┘  │
│                                                           │
│  GET /heads/{hash}.png                                    │
└─────────┬─────────────────────────────────────────────────┘
          │
          │ Image data (from SQLite)
          │
          ▼
┌─────────────────┐
│  Discord Bot    │
│  (Poise)        │
│                 │
│  /online cmd    │
│  (queries both  │
│   in-memory +   │
│   SQLite)       │
└─────────────────┘
          │
          │ Embed with image URL
          │
          ▼
┌─────────────────┐
│  Discord CDN    │
│  (caches image) │
└─────────────────┘
```

### API Endpoints

#### Player Events (Fabric Mod → Backend)

**POST /join**
```json
Request:
{
  "player": "Steve",
  "texture_hash": "d4f7e8a9b2c1a5f3..."
}

Response (already have skin):
200 OK

Response (need skin):
202 Accepted
```

**Backend Implementation:**
- Check if skin exists in `skins` table (SQLite)
- Update `player_skins` table (SQLite) with texture_hash
- Add player to in-memory cache for this server
- Return 200 if skin exists, 202 if need skin data

**POST /skin**
```json
Request:
{
  "texture_hash": "d4f7e8a9b2c1a5f3...",
  "skin_data": "iVBORw0KGgoAAAANSUhEUgAA..."  // base64 PNG
}

Response:
200 OK
```

**POST /leave**
```json
{
  "player": "Steve"
}
```

**Backend Implementation:**
- Remove player from in-memory cache for this server
- Note: `player_skins` table persists (we remember their last skin for next join)

**POST /sync**
```json
{
  "players": ["Steve", "Alex", "Notch"]
}
```

**Backend Implementation:**
- Replace entire player list in in-memory cache for this server
- Note: `player_skins` table is not affected (persists historical skin data)

#### Image Serving (Discord Bot → Backend)

**GET /heads/{texture_hash}.png**

Response:
```
200 OK
Content-Type: image/png
Cache-Control: public, immutable, max-age=31536000

[PNG binary data]
```

Fallback if head not found: Returns default Steve head

---

## Data Flow Examples

### Scenario 1: First Join (New Skin)

```
1. Player "Steve" joins Minecraft server with new skin

2. Fabric Mod:
   - Extracts GameProfile texture property
   - Computes: texture_hash = SHA256(texture_value)
   - Sends: POST /join {"player": "Steve", "texture_hash": "abc123"}

3. Backend:
   - Checks: SELECT * FROM skins WHERE texture_hash = 'abc123'
   - Not found → Responds: 202 Accepted

4. Fabric Mod:
   - Receives 202
   - Extracts skin PNG from texture URL in GameProfile
   - Encodes to base64
   - Sends: POST /skin {"texture_hash": "abc123", "skin_data": "..."}

5. Backend:
   - Receives skin
   - Verifies hash matches
   - Inserts into skins table (SQLite)
   - Inserts/updates player_skins table (SQLite)
   - Player "Steve" already in in-memory cache from step 3
   - Spawns async task: render_head("abc123")
   - Responds: 200 OK

6. Background Render Task:
   - Loads skin from skins table
   - Renders 64x64 head image
   - Inserts into rendered_heads table

Later:

7. Discord user runs: /online MyServer

8. Discord Bot:
   - Queries in-memory cache → gets list of online player names (including "Steve")
   - For each player, queries player_skins table → gets texture_hash
   - Builds embed with: thumbnail("https://backend/heads/abc123.png")

9. Discord fetches: GET /heads/abc123.png

10. Backend:
    - Queries rendered_heads table
    - Returns PNG with immutable cache headers

11. Discord caches image on their CDN forever
```

### Scenario 2: Repeat Join (Existing Skin)

```
1. Player "Steve" joins Minecraft server (same skin as before)

2. Fabric Mod:
   - Computes: texture_hash = SHA256(texture_value)  (same hash)
   - Sends: POST /join {"player": "Steve", "texture_hash": "abc123"}

3. Backend:
   - Checks: SELECT * FROM skins WHERE texture_hash = 'abc123'
   - Found! → Updates player_skins.last_updated (SQLite)
   - Adds "Steve" to in-memory cache
   - Responds: 200 OK

4. Done! No network transfer of skin, no rendering.
```

**Bandwidth saved:** 13KB (skin) + 0ms (no render)

### Scenario 3: Player Changes Skin

```
1. Player "Steve" changes skin on Mojang/SkinsRestorer

2. Player joins Minecraft server

3. Fabric Mod:
   - Computes: texture_hash = SHA256(new_texture_value)
   - Hash is DIFFERENT now!
   - Sends: POST /join {"player": "Steve", "texture_hash": "xyz789"}

4. Backend:
   - Checks: SELECT * FROM skins WHERE texture_hash = 'xyz789'
   - Not found → Responds: 202 Accepted

5-6. Same as Scenario 1 (fetch skin, render head)

7. Backend:
   - Updates player_skins table: Steve now points to xyz789 (SQLite)
   - "Steve" already in in-memory cache from step 4
   - Old skin (abc123) remains in database (might be used by other players)
   - Old head (abc123) remains cached
```

**Result:** Old Discord embeds still show old skin (immutable URLs), new embeds show new skin.

---

## Trade-offs and Rationale

### Decisions Made

| Decision | What We Chose | What We Skipped | Why |
|----------|---------------|-----------------|-----|
| **Change Detection** | Hash texture value | Hash PNG bytes | Zero network cost, already in memory |
| **Skin Storage** | SQLite BLOB | Filesystem | Simpler, faster, single backup file |
| **Head Storage** | Separate table | Same table as skin | Don't load 10KB skin when serving 2KB head |
| **Network Protocol** | Two-step (hash → 202 → skin) | Send always / Backend fetch | 67x bandwidth reduction, works offline |
| **Rendering** | Fire-and-forget async | Sync / On-demand | Fast response, Steve fallback handles edge cases |
| **Serving** | By texture hash | By player name | Immutable URLs, perfect caching |
| **Indexes** | None (PKs only) | Additional indexes | All queries use PRIMARY KEYs |
| **Timestamps** | last_updated only | first_seen | YAGNI - not needed for core functionality |

### Performance Characteristics

**Network Bandwidth:**
- Per join (existing skin): **200 bytes** (just hash)
- Per join (new skin): **200 bytes + 13KB** (one-time)
- Per Discord request: **2KB** (once, then Discord CDN caches)
- Bandwidth reduction: **67x** less than "send always" approach

**Rendering Operations:**
- Per unique skin: **1 render** (~10-50ms)
- Per player join: **0 renders** (if skin exists)
- Total renders for 1000 unique skins: **1000 renders**
- Total renders for 10,000 player joins: **~1000 renders** (only unique skins)
- Reduction: **10x** less rendering than "render per player"

**Storage:**
- Per unique skin: 12KB (10KB raw + 2KB head)
- 1,000 unique skins: ~12MB
- 10,000 unique skins: ~120MB
- Assessment: **Trivial** on modern systems

**Latency:**
- Join with existing skin: **<10ms**
- Join with new skin: **~100ms** (hash + 202 + skin upload)
- Serve existing head: **<5ms** (SQLite BLOB read)
- Serve with fallback: **<1ms** (embedded Steve head)

### Edge Cases Handled

1. **Skin fetch fails** → Return Steve head fallback
2. **Render fails** → Return Steve head fallback, can retry later
3. **Hash collision** → Cryptographically improbable (SHA256)
4. **Multiple players, same skin** → Deduplicated, one render
5. **Player changes skin** → New hash, new render, old data persists
6. **Discord requests before render complete** → Steve head fallback
7. **Backend restart** → Heads persisted in DB, no re-render needed
8. **Offline server** → Mod sends skin directly, no URL dependency
9. **SkinsRestorer with fake URLs** → Mod sends skin data, backend doesn't fetch

---

## Implementation Notes

### Fabric Mod Changes Required

1. Extract skin data from GameProfile:
   ```java
   Property texturesProperty = profile.getProperties()
       .get("textures").stream().findFirst().orElse(null);
   ```

2. Compute texture hash:
   ```java
   String textureValue = texturesProperty.getValue();
   String textureHash = sha256(textureValue);
   ```

3. Parse texture JSON to get skin URL:
   ```java
   String json = new String(Base64.decode(textureValue));
   JsonObject obj = JsonParser.parseString(json).getAsJsonObject();
   String skinUrl = obj.getAsJsonObject("textures")
       .getAsJsonObject("SKIN")
       .get("url").getAsString();
   ```

4. Download skin PNG:
   ```java
   byte[] skinPng = downloadBytes(skinUrl);
   String skinBase64 = Base64.encode(skinPng);
   ```

5. Implement two-step protocol:
   ```java
   Response joinResponse = POST("/join", json);
   if (joinResponse.status == 202) {
       POST("/skin", json);
   }
   ```

### Backend Changes Required

1. Add dependencies:
   ```toml
   image = "0.25"  # For rendering
   sha2 = "0.10"   # For SHA256
   base64 = "0.22" # For decoding skin data
   ```

2. Create database migrations for new tables

3. Implement `/join` endpoint logic:
   - Check if skin exists by hash
   - Return 200 if exists, 202 if needed

4. Implement `/skin` endpoint:
   - Decode base64 skin data
   - Verify hash matches
   - Store in database
   - Spawn render task

5. Implement head rendering:
   ```rust
   fn render_head(skin_png: &[u8]) -> Vec<u8> {
       let skin = image::load_from_memory(skin_png)?;

       // Crop face (8x8 at position 8,8)
       let face = imageops::crop(&mut skin, 8, 8, 8, 8).to_image();

       // Crop helmet overlay (8x8 at position 40,8)
       let helmet = imageops::crop(&mut skin, 40, 8, 8, 8).to_image();

       // Composite helmet over face
       let mut head = face;
       imageops::overlay(&mut head, &helmet, 0, 0);

       // Scale to 64x64
       let head = imageops::resize(&head, 64, 64, FilterType::Nearest);

       // Encode to PNG
       let mut buf = Vec::new();
       head.write_to(&mut Cursor::new(&mut buf), ImageFormat::Png)?;
       buf
   }
   ```

6. Implement `/heads/{hash}.png` endpoint

7. Embed default Steve head:
   - Pre-render Steve head PNG
   - Store in `assets/steve_head.png`
   - Include at compile time: `include_bytes!("assets/steve_head.png")`

### Discord Bot Changes Required

1. Modify `/online` command to query both in-memory cache and SQLite:
   ```rust
   // Step 1: Get online players from in-memory cache
   let online_players: Vec<PlayerName> = state.online_cache
       .get_players(api_key_hash)
       .await?;

   // Step 2: For each player, get their texture hash from SQLite
   for player in online_players {
       match db.get_player_texture_hash(&player).await? {
           Some(hash) => {
               embed = embed.thumbnail(format!("{}/heads/{}.png", base_url, hash));
           }
           None => {
               // Player hasn't sent skin data yet, use Steve fallback
               embed = embed.thumbnail(format!("{}/heads/steve.png", base_url));
           }
       }
   }
   ```

2. Add database method for getting texture hash:
   ```rust
   impl Database {
       async fn get_player_texture_hash(&self, player_name: &str) -> Result<Option<String>>;
   }
   ```

---

## Success Criteria

The implementation will be considered successful if:

1. ✅ **Network bandwidth reduced by >50x** for repeat joins
2. ✅ **Rendering happens once per unique skin** (not per player)
3. ✅ **Discord embeds show player heads** with <5s latency
4. ✅ **System works on offline servers** with skin plugins
5. ✅ **Graceful degradation** when render fails (Steve head fallback)
6. ✅ **Storage growth is linear** with unique skins, not total players
7. ✅ **No manual intervention required** for skin updates

---

## Future Enhancements (Out of Scope)

- 3D isometric head renders (would require external service like nmsr-rs)
- Full body renders
- Cape rendering
- Multiple render sizes (32x32, 128x128)
- Admin dashboard for skin management
- Automatic cleanup of unused skins
- WebP format for smaller file sizes
- In-memory LRU cache (if profiling shows SQLite is bottleneck)

---

## Conclusion

This design achieves all primary objectives:
- ✅ Minimal rendering (once per unique skin)
- ✅ Minimal network (67x bandwidth reduction)
- ✅ Efficient storage (deduplicated, SQLite BLOBs)
- ✅ Low latency (fast lookups, immutable CDN caching)
- ✅ Simple implementation (single database, straightforward logic)
- ✅ Robust (fallbacks, self-healing, handles edge cases)

The architecture is production-ready and scales efficiently to thousands of players with minimal resource usage.
