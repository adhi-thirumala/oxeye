mod cache;
mod error;
mod models;

pub use cache::{OnlineCache, ServerState, new_cache};
pub use error::{DbError, Result};
pub use models::{
    OnlinePlayer, PendingLink, PlayerInfo, PlayerName, Server, ServerSummary, ServerWithPlayers,
};

use std::path::Path;
use std::sync::Arc;
use tokio_rusqlite::Connection;
use tokio_rusqlite::rusqlite::{OptionalExtension, params};
use tracing::{debug, info};

/// Database wrapper for all Oxeye operations.
///
/// Persistent data (servers, pending_links) is stored in SQLite.
/// Ephemeral data (online_players) is stored in an in-memory cache.
#[derive(Clone)]
pub struct Database {
    conn: Connection,
    cache: Arc<OnlineCache>,
}

impl Database {
    /// Open or create a database at the given path.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path).await.map_err(DbError::Sqlite)?;
        let cache = Arc::new(new_cache());
        let db = Self { conn, cache };
        db.initialize().await?;
        db.populate_cache().await?;
        Ok(db)
    }

    /// Create an in-memory database (useful for testing).
    pub async fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()
            .await
            .map_err(DbError::Sqlite)?;
        let cache = Arc::new(new_cache());
        let db = Self { conn, cache };
        db.initialize().await?;
        db.populate_cache().await?;
        Ok(db)
    }

    /// Initialize the database schema.
    async fn initialize(&self) -> Result<()> {
        self.conn
            .call(|conn| {
                // Enable WAL mode for better concurrent read/write performance
                conn.pragma_update(None, "journal_mode", "WAL")?;

                // Enable foreign key constraints (must be set per-connection)
                conn.pragma_update(None, "foreign_keys", "ON")?;

                conn.execute_batch(
                    r#"
                    -- Pending connection codes (expire after 10 minutes)
                    CREATE TABLE IF NOT EXISTS pending_links (
                        code TEXT PRIMARY KEY,
                        guild_id INTEGER NOT NULL,
                        server_name TEXT NOT NULL,
                        created_at INTEGER NOT NULL
                    );

                    -- Linked servers (API key hash is primary key)
                    CREATE TABLE IF NOT EXISTS servers (
                        api_key_hash TEXT PRIMARY KEY,
                        name TEXT NOT NULL,
                        guild_id INTEGER NOT NULL,
                        UNIQUE(guild_id, name)
                    );

                    -- Index for fast guild lookups
                    CREATE INDEX IF NOT EXISTS idx_servers_guild ON servers(guild_id);

                    -- Stores unique skin textures (deduplicated by texture hash)
                    CREATE TABLE IF NOT EXISTS skins (
                        texture_hash TEXT PRIMARY KEY,
                        texture_url TEXT,
                        skin_data BLOB NOT NULL
                    );

                    -- Maps players to their current skin
                    CREATE TABLE IF NOT EXISTS player_skins (
                        player_name TEXT PRIMARY KEY,
                        texture_hash TEXT NOT NULL,
                        last_updated INTEGER NOT NULL,
                        FOREIGN KEY (texture_hash) REFERENCES skins(texture_hash)
                    );

                    -- Stores rendered head images (one per unique skin)
                    CREATE TABLE IF NOT EXISTS rendered_heads (
                        texture_hash TEXT PRIMARY KEY,
                        head_data BLOB NOT NULL,
                        rendered_at INTEGER NOT NULL,
                        FOREIGN KEY (texture_hash) REFERENCES skins(texture_hash)
                    );

                    -- Caches rendered status composite images (one per server)
                    CREATE TABLE IF NOT EXISTS status_images (
                        api_key_hash TEXT PRIMARY KEY,
                        image_data BLOB NOT NULL,
                        updated_at INTEGER NOT NULL,
                        FOREIGN KEY (api_key_hash) REFERENCES servers(api_key_hash) ON DELETE CASCADE
                    );
                    "#,
                )?;
                Ok(())
            })
            .await?;

        info!("database initialized");
        Ok(())
    }

    /// Pre-populate the cache with all existing servers.
    /// All servers start with synced_since_boot = false.
    async fn populate_cache(&self) -> Result<()> {
        let api_key_hashes: Vec<String> = self
            .conn
            .call(|conn| {
                let mut stmt = conn.prepare_cached("SELECT api_key_hash FROM servers")?;
                let hashes = stmt
                    .query_map([], |row| row.get(0))?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                Ok(hashes)
            })
            .await?;

        let count = api_key_hashes.len();
        for hash in api_key_hashes {
            let _ = self.cache.insert_async(hash, ServerState::new()).await;
        }

        info!(count, "pre-populated cache with servers (awaiting sync)");
        Ok(())
    }

    /// Check if a server has synced since backend restart.
    pub async fn is_server_synced(&self, api_key_hash: &str) -> bool {
        match self.cache.get_async(api_key_hash).await {
            Some(entry) => entry.get().synced_since_boot,
            None => false,
        }
    }

    /// Check if a server has synced since backend restart (by guild and name).
    pub async fn is_server_synced_by_name(&self, guild_id: u64, server_name: &str) -> Result<bool> {
        let name = server_name.to_string();
        let api_key_hash: Option<String> = self
            .conn
            .call(move |conn| {
                conn.prepare_cached(
                    "SELECT api_key_hash FROM servers WHERE guild_id = ?1 AND name = ?2",
                )?
                .query_row(params![guild_id, &name], |row| row.get(0))
                .optional()
            })
            .await?;

        match api_key_hash {
            Some(hash) => Ok(self.is_server_synced(&hash).await),
            None => Err(DbError::ServerNotFound),
        }
    }

    // ========================================================================
    // Pending Links
    // ========================================================================

    /// Create a new pending link.
    /// Returns an error if a server with that name already exists in the guild.
    pub async fn create_pending_link(
        &self,
        code: String,
        guild_id: u64,
        server_name: String,
        now: i64,
    ) -> Result<PendingLink> {
        let result = self
            .conn
            .call(move |conn| {
                let tx = conn.transaction()?;

                // Check if server name already exists in this guild
                let exists: bool = tx
                    .prepare_cached("SELECT EXISTS(SELECT 1 FROM servers WHERE guild_id = ?1 AND name = ?2)")?
                    .query_row(params![guild_id, &server_name], |row| row.get(0))?;

                if exists {
                    return Ok(Err(DbError::ServerNameConflict));
                }

                tx.prepare_cached(
                    "INSERT INTO pending_links (code, guild_id, server_name, created_at) VALUES (?1, ?2, ?3, ?4)",
                )?
                    .execute(params![&code, guild_id, &server_name, now])?;

                tx.commit()?;
                Ok(Ok(PendingLink {
                    code,
                    guild_id,
                    server_name,
                    created_at: now,
                }))
            })
            .await??;

        debug!(%result.code, result.guild_id, %result.server_name, "created pending link");
        Ok(result)
    }

    /// Get a pending link by code.
    /// Returns None if not found.
    pub async fn get_pending_link(&self, code: String) -> Result<Option<PendingLink>> {
        let link = self
            .conn
            .call(move |conn| {
                conn
          .prepare_cached(
            "SELECT code, guild_id, server_name, created_at FROM pending_links WHERE code = ?1",
          )?
          .query_row(params![&code], |row| {
            Ok(PendingLink {
              code: row.get(0)?,
              guild_id: row.get(1)?,
              server_name: row.get(2)?,
              created_at: row.get(3)?,
            })
          })
          .optional()
            })
            .await?;

        Ok(link)
    }

    /// Consume a pending link (delete it and return it).
    /// Returns an error if not found or expired.
    pub async fn consume_pending_link(&self, code: String, now: i64) -> Result<PendingLink> {
        let result = self
            .conn
            .call(move |conn| {
                let tx = conn.transaction()?;

                let link: Option<PendingLink> = tx
          .prepare_cached(
            "SELECT code, guild_id, server_name, created_at FROM pending_links WHERE code = ?1",
          )?
          .query_row(params![&code], |row| {
            Ok(PendingLink {
              code: row.get(0)?,
              guild_id: row.get(1)?,
              server_name: row.get(2)?,
              created_at: row.get(3)?,
            })
          })
          .optional()?;

                let link = match link {
                    Some(l) => l,
                    None => return Ok(Err(DbError::PendingLinkNotFound)),
                };

                if link.is_expired(now) {
                    tx.prepare_cached("DELETE FROM pending_links WHERE code = ?1")?
                        .execute(params![&code])?;
                    tx.commit()?;
                    return Ok(Err(DbError::PendingLinkNotFound));
                }

                tx.prepare_cached("DELETE FROM pending_links WHERE code = ?1")?
                    .execute(params![&code])?;
                tx.commit()?;
                Ok(Ok(link))
            })
            .await??;

        debug!(%result.code, "consumed pending link");
        Ok(result)
    }

    /// Clean up expired pending links.
    pub async fn cleanup_expired_links(&self, now: i64) -> Result<u64> {
        let deleted = self
            .conn
            .call(move |conn| {
                const TTL_SECONDS: i64 = 600;
                let cutoff = now - TTL_SECONDS;

                let deleted = conn
                    .prepare_cached("DELETE FROM pending_links WHERE created_at < ?1")?
                    .execute(params![cutoff])?;
                Ok(deleted as u64)
            })
            .await?;

        if deleted > 0 {
            debug!(deleted, "cleaned up expired pending links");
        }

        Ok(deleted)
    }

    // ========================================================================
    // Servers
    // ========================================================================

    /// Create a new server.
    pub async fn create_server(
        &self,
        api_key_hash: String,
        name: String,
        guild_id: u64,
    ) -> Result<Server> {
        let server = self
            .conn
            .call(move |conn| {
                conn.prepare_cached(
                    "INSERT INTO servers (api_key_hash, name, guild_id) VALUES (?1, ?2, ?3)",
                )?
                .execute(params![&api_key_hash, &name, guild_id])?;

                Ok(Server {
                    api_key_hash,
                    name,
                    guild_id,
                })
            })
            .await?;

        debug!(%server.name, server.guild_id, "created server");
        Ok(server)
    }

    /// Get a server by API key hash.
    pub async fn get_server_by_api_key(&self, api_key_hash: String) -> Result<Option<Server>> {
        let server = self
            .conn
            .call(move |conn| {
                conn.prepare_cached(
                    "SELECT api_key_hash, name, guild_id FROM servers WHERE api_key_hash = ?1",
                )?
                .query_row(params![&api_key_hash], |row| {
                    Ok(Server {
                        api_key_hash: row.get(0)?,
                        name: row.get(1)?,
                        guild_id: row.get(2)?,
                    })
                })
                .optional()
            })
            .await?;

        Ok(server)
    }

    /// Get all servers for a guild.
    pub async fn get_servers_by_guild(&self, guild_id: u64) -> Result<Vec<Server>> {
        let servers = self
            .conn
            .call(move |conn| {
                let mut stmt = conn.prepare_cached(
                    "SELECT api_key_hash, name, guild_id FROM servers WHERE guild_id = ?1",
                )?;

                let servers = stmt
                    .query_map(params![guild_id], |row| {
                        Ok(Server {
                            api_key_hash: row.get(0)?,
                            name: row.get(1)?,
                            guild_id: row.get(2)?,
                        })
                    })?
                    .collect::<std::result::Result<Vec<_>, _>>()?;

                Ok(servers)
            })
            .await?;

        Ok(servers)
    }

    /// Get server summaries for a guild (with player counts).
    pub async fn get_server_summaries(&self, guild_id: u64) -> Result<Vec<ServerSummary>> {
        // Get servers from SQLite
        let servers: Vec<(String, String)> = self
            .conn
            .call(move |conn| {
                let mut stmt = conn.prepare_cached(
                    "SELECT api_key_hash, name FROM servers WHERE guild_id = ?1 ORDER BY name",
                )?;
                let servers = stmt
                    .query_map(params![guild_id], |row| Ok((row.get(0)?, row.get(1)?)))?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                Ok(servers)
            })
            .await?;

        // Get player counts from in-memory cache
        let mut summaries = Vec::with_capacity(servers.len());
        for (api_key_hash, name) in servers {
            let player_count = match self.cache.get_async(&api_key_hash).await {
                Some(entry) => entry.get().player_count() as u32,
                None => 0,
            };
            summaries.push(ServerSummary { name, player_count });
        }

        Ok(summaries)
    }

    /// Delete a server by guild and name.
    pub async fn delete_server(&self, guild_id: u64, name: String) -> Result<()> {
        // First get the api_key_hash so we can clean up the cache
        let name_clone = name.clone();
        let api_key_hash: Option<String> = self
            .conn
            .call(move |conn| {
                conn.prepare_cached(
                    "SELECT api_key_hash FROM servers WHERE guild_id = ?1 AND name = ?2",
                )?
                .query_row(params![guild_id, &name_clone], |row| row.get(0))
                .optional()
            })
            .await?;

        let result = self
            .conn
            .call(move |conn| {
                let deleted = conn
                    .prepare_cached("DELETE FROM servers WHERE guild_id = ?1 AND name = ?2")?
                    .execute(params![guild_id, &name])?;

                if deleted == 0 {
                    return Ok(Err(DbError::ServerNotFound));
                }

                Ok(Ok(()))
            })
            .await??;

        // Clean up cache
        if let Some(hash) = api_key_hash {
            let _ = self.cache.remove_async(&hash).await;
        }

        debug!(guild_id, "deleted server");
        Ok(result)
    }

    /// Delete a server by API key hash (for self-disconnect).
    pub async fn delete_server_by_api_key(&self, api_key_hash: String) -> Result<()> {
        let hash_clone = api_key_hash.clone();
        let result = self
            .conn
            .call(move |conn| {
                let deleted = conn
                    .prepare_cached("DELETE FROM servers WHERE api_key_hash = ?1")?
                    .execute(params![&hash_clone])?;

                if deleted == 0 {
                    return Ok(Err(DbError::InvalidApiKey));
                }

                Ok(Ok(()))
            })
            .await??;

        // Clean up cache
        let _ = self.cache.remove_async(&api_key_hash).await;

        debug!("deleted server by api key");
        Ok(result)
    }

    /// Check if a server name exists in a guild.
    pub async fn server_name_exists(&self, guild_id: u64, name: String) -> Result<bool> {
        let exists =
            self.conn
                .call(move |conn| {
                    let exists: bool = conn
          .prepare_cached("SELECT EXISTS(SELECT 1 FROM servers WHERE guild_id = ?1 AND name = ?2)")?
          .query_row(params![guild_id, &name], |row| row.get(0))?;

                    Ok(exists)
                })
                .await?;

        Ok(exists)
    }

    /// Get a server's API key hash by guild and name.
    pub async fn get_api_key_hash_by_name(
        &self,
        guild_id: u64,
        name: &str,
    ) -> Result<Option<String>> {
        let name = name.to_string();
        let hash = self
            .conn
            .call(move |conn| {
                conn.prepare_cached(
                    "SELECT api_key_hash FROM servers WHERE guild_id = ?1 AND name = ?2",
                )?
                .query_row(params![guild_id, &name], |row| row.get(0))
                .optional()
            })
            .await?;
        Ok(hash)
    }

    // ========================================================================
    // Online Players (in-memory cache)
    // ========================================================================

    /// Record a player joining.
    pub async fn player_join(
        &self,
        api_key_hash: String,
        player_name: PlayerName,
        now: i64,
    ) -> Result<()> {
        // Verify the server exists in SQLite
        let exists = self.server_exists(&api_key_hash).await?;
        if !exists {
            return Err(DbError::InvalidApiKey);
        }

        // Update in-memory cache
        self.cache
            .entry_async(api_key_hash)
            .await
            .or_insert_with(|| ServerState::new())
            .get_mut()
            .add_player(player_name, now);

        debug!(player_name = %player_name, "player joined");
        Ok(())
    }

    /// Record a player leaving.
    pub async fn player_leave(&self, api_key_hash: String, player_name: PlayerName) -> Result<()> {
        // Verify the server exists in SQLite
        let exists = self.server_exists(&api_key_hash).await?;
        if !exists {
            return Err(DbError::InvalidApiKey);
        }

        // Update in-memory cache
        if let Some(mut entry) = self.cache.get_async(&api_key_hash).await {
            entry.get_mut().remove_player(&player_name);
        }

        debug!(player_name = %player_name, "player left");
        Ok(())
    }

    /// Sync the player list (replace all players for a server).
    pub async fn sync_players(
        &self,
        api_key_hash: String,
        players: Vec<PlayerName>,
        now: i64,
    ) -> Result<()> {
        // Verify the server exists in SQLite
        let exists = self.server_exists(&api_key_hash).await?;
        if !exists {
            return Err(DbError::InvalidApiKey);
        }

        let count = players.len();

        // Convert to (PlayerName, i64) pairs
        let players_with_time: Vec<(PlayerName, i64)> =
            players.into_iter().map(|p| (p, now)).collect();

        // Update in-memory cache
        self.cache
            .entry_async(api_key_hash)
            .await
            .or_insert_with(|| ServerState::new())
            .get_mut()
            .sync_players(players_with_time);

        debug!(count, "synced players");
        Ok(())
    }

    /// Get online players for a server (sorted by name).
    pub async fn get_online_players(&self, api_key_hash: String) -> Result<Vec<PlayerName>> {
        let mut players: Vec<PlayerName> = match self.cache.get_async(&api_key_hash).await {
            Some(entry) => entry.get().players.iter().map(|(name, _)| *name).collect(),
            None => Vec::new(),
        };
        players.sort();
        Ok(players)
    }

    /// Helper to check if a server exists in SQLite.
    async fn server_exists(&self, api_key_hash: &str) -> Result<bool> {
        let hash = api_key_hash.to_string();
        let exists = self
            .conn
            .call(move |conn| {
                let exists: bool = conn
                    .prepare_cached("SELECT EXISTS(SELECT 1 FROM servers WHERE api_key_hash = ?1)")?
                    .query_row(params![&hash], |row| row.get(0))?;
                Ok(exists)
            })
            .await?;
        Ok(exists)
    }

    /// Get all servers with their online players for a guild.
    pub async fn get_servers_with_players(&self, guild_id: u64) -> Result<Vec<ServerWithPlayers>> {
        // Get servers from SQLite
        let servers: Vec<(String, String)> = self
            .conn
            .call(move |conn| {
                let mut stmt = conn.prepare_cached(
                    "SELECT api_key_hash, name FROM servers WHERE guild_id = ?1 ORDER BY name",
                )?;
                let servers = stmt
                    .query_map(params![guild_id], |row| Ok((row.get(0)?, row.get(1)?)))?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                Ok(servers)
            })
            .await?;

        // Get players from in-memory cache
        let mut result = Vec::with_capacity(servers.len());
        for (api_key_hash, name) in servers {
            let mut players: Vec<PlayerInfo> = match self.cache.get_async(&api_key_hash).await {
                Some(entry) => entry
                    .get()
                    .players
                    .iter()
                    .map(|(player_name, joined_at)| PlayerInfo {
                        player_name: *player_name,
                        joined_at: *joined_at,
                    })
                    .collect(),
                None => Vec::new(),
            };
            // Sort by player name for consistent ordering
            players.sort_by(|a, b| a.player_name.cmp(&b.player_name));
            result.push(ServerWithPlayers { name, players });
        }

        Ok(result)
    }

    /// Get a specific server with its online players.
    pub async fn get_server_with_players(
        &self,
        guild_id: u64,
        server_name: String,
    ) -> Result<ServerWithPlayers> {
        // Get server from SQLite
        let server_name_clone = server_name.clone();
        let api_key_hash: Option<String> = self
            .conn
            .call(move |conn| {
                conn.prepare_cached(
                    "SELECT api_key_hash FROM servers WHERE guild_id = ?1 AND name = ?2",
                )?
                .query_row(params![guild_id, &server_name_clone], |row| row.get(0))
                .optional()
            })
            .await?;

        let api_key_hash = match api_key_hash {
            Some(h) => h,
            None => return Err(DbError::ServerNotFound),
        };

        // Get players from in-memory cache
        let mut players: Vec<PlayerInfo> = match self.cache.get_async(&api_key_hash).await {
            Some(entry) => entry
                .get()
                .players
                .iter()
                .map(|(player_name, joined_at)| PlayerInfo {
                    player_name: *player_name,
                    joined_at: *joined_at,
                })
                .collect(),
            None => Vec::new(),
        };
        // Sort by player name for consistent ordering
        players.sort_by(|a, b| a.player_name.cmp(&b.player_name));

        Ok(ServerWithPlayers {
            name: server_name,
            players,
        })
    }

    // ========================================================================
    // Skins and Rendered Heads
    // ========================================================================

    /// Check if a skin exists by texture hash.
    pub async fn skin_exists(&self, texture_hash: &str) -> Result<bool> {
        let hash = texture_hash.to_string();
        let exists = self
            .conn
            .call(move |conn| {
                let exists: bool = conn
                    .prepare_cached("SELECT EXISTS(SELECT 1 FROM skins WHERE texture_hash = ?1)")?
                    .query_row(params![&hash], |row| row.get(0))?;
                Ok(exists)
            })
            .await?;
        Ok(exists)
    }

    /// Store a skin (raw PNG data).
    pub async fn store_skin(
        &self,
        texture_hash: String,
        texture_url: Option<String>,
        skin_data: Vec<u8>,
    ) -> Result<()> {
        let hash_for_log = texture_hash.clone();
        self.conn
            .call(move |conn| {
                conn.prepare_cached(
                    "INSERT OR REPLACE INTO skins (texture_hash, texture_url, skin_data) VALUES (?1, ?2, ?3)",
                )?
                .execute(params![&texture_hash, &texture_url, &skin_data])?;
                Ok(())
            })
            .await?;

        debug!(texture_hash = %hash_for_log, "stored skin");
        Ok(())
    }

    /// Get skin data by texture hash.
    pub async fn get_skin_data(&self, texture_hash: &str) -> Result<Option<Vec<u8>>> {
        let hash = texture_hash.to_string();
        let skin_data = self
            .conn
            .call(move |conn| {
                conn.prepare_cached("SELECT skin_data FROM skins WHERE texture_hash = ?1")?
                    .query_row(params![&hash], |row| row.get(0))
                    .optional()
            })
            .await?;
        Ok(skin_data)
    }

    /// Update player's current skin mapping.
    pub async fn update_player_skin(
        &self,
        player_name: &str,
        texture_hash: &str,
        now: i64,
    ) -> Result<()> {
        let name = player_name.to_string();
        let hash = texture_hash.to_string();
        self.conn
            .call(move |conn| {
                conn.prepare_cached(
                    "INSERT OR REPLACE INTO player_skins (player_name, texture_hash, last_updated) VALUES (?1, ?2, ?3)",
                )?
                .execute(params![&name, &hash, now])?;
                Ok(())
            })
            .await?;

        debug!(player_name, texture_hash, "updated player skin");
        Ok(())
    }

    /// Get a player's current texture hash.
    pub async fn get_player_texture_hash(&self, player_name: &str) -> Result<Option<String>> {
        let name = player_name.to_string();
        let hash = self
            .conn
            .call(move |conn| {
                conn.prepare_cached("SELECT texture_hash FROM player_skins WHERE player_name = ?1")?
                    .query_row(params![&name], |row| row.get(0))
                    .optional()
            })
            .await?;
        Ok(hash)
    }

    /// Store a rendered head image.
    pub async fn store_rendered_head(
        &self,
        texture_hash: String,
        head_data: Vec<u8>,
        now: i64,
    ) -> Result<()> {
        let hash_for_log = texture_hash.clone();
        self.conn
            .call(move |conn| {
                conn.prepare_cached(
                    "INSERT OR REPLACE INTO rendered_heads (texture_hash, head_data, rendered_at) VALUES (?1, ?2, ?3)",
                )?
                .execute(params![&texture_hash, &head_data, now])?;
                Ok(())
            })
            .await?;

        debug!(texture_hash = %hash_for_log, "stored rendered head");
        Ok(())
    }

    /// Get a rendered head by texture hash.
    pub async fn get_rendered_head(&self, texture_hash: &str) -> Result<Option<Vec<u8>>> {
        let hash = texture_hash.to_string();
        let head_data = self
            .conn
            .call(move |conn| {
                conn.prepare_cached("SELECT head_data FROM rendered_heads WHERE texture_hash = ?1")?
                    .query_row(params![&hash], |row| row.get(0))
                    .optional()
            })
            .await?;
        Ok(head_data)
    }

    // ========================================================================
    // Status Composite Images (Cache)
    // ========================================================================

    /// Store a cached status composite image for a server.
    pub async fn store_status_image(
        &self,
        api_key_hash: String,
        image_data: Vec<u8>,
        now: i64,
    ) -> Result<()> {
        self.conn
            .call(move |conn| {
                conn.prepare_cached(
                    "INSERT OR REPLACE INTO status_images (api_key_hash, image_data, updated_at) VALUES (?1, ?2, ?3)",
                )?
                .execute(params![&api_key_hash, &image_data, now])?;
                Ok(())
            })
            .await?;

        debug!("stored status image");
        Ok(())
    }

    /// Get a cached status composite image.
    pub async fn get_status_image(&self, api_key_hash: &str) -> Result<Option<Vec<u8>>> {
        let hash = api_key_hash.to_string();
        let image_data = self
            .conn
            .call(move |conn| {
                conn.prepare_cached("SELECT image_data FROM status_images WHERE api_key_hash = ?1")?
                    .query_row(params![&hash], |row| row.get(0))
                    .optional()
            })
            .await?;
        Ok(image_data)
    }

    /// Get players with their texture hashes for a server (for composite rendering).
    pub async fn get_players_with_heads(
        &self,
        api_key_hash: &str,
    ) -> Result<Vec<(PlayerName, Option<String>)>> {
        // Get online players from cache
        let players: Vec<PlayerName> = match self.cache.get_async(api_key_hash).await {
            Some(entry) => entry.get().players.iter().map(|(name, _)| *name).collect(),
            None => Vec::new(),
        };

        // Look up texture hashes for each player
        let mut result = Vec::with_capacity(players.len());
        for player_name in players {
            let hash = self.get_player_texture_hash(player_name.as_str()).await?;
            result.push((player_name, hash));
        }

        // Sort by player name for consistent ordering
        result.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> i64 {
        1700000000 // Fixed timestamp for testing
    }

    /// Helper to create PlayerName from a string literal.
    fn pn(s: &str) -> PlayerName {
        PlayerName::from(s).unwrap()
    }

    #[tokio::test]
    async fn test_pending_link_lifecycle() {
        let db = Database::open_in_memory().await.unwrap();

        // Create a pending link
        let link = db
            .create_pending_link(
                "oxeye-abc123".to_string(),
                12345,
                "Survival SMP".to_string(),
                now(),
            )
            .await
            .unwrap();
        assert_eq!(link.code, "oxeye-abc123");
        assert_eq!(link.guild_id, 12345);
        assert_eq!(link.server_name, "Survival SMP");

        // Get it
        let link = db
            .get_pending_link("oxeye-abc123".to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(link.code, "oxeye-abc123");

        // Consume it
        let link = db
            .consume_pending_link("oxeye-abc123".to_string(), now())
            .await
            .unwrap();
        assert_eq!(link.server_name, "Survival SMP");

        // Should be gone now
        assert!(
            db.get_pending_link("oxeye-abc123".to_string())
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn test_expired_link() {
        let db = Database::open_in_memory().await.unwrap();

        db.create_pending_link(
            "oxeye-expired".to_string(),
            12345,
            "Test".to_string(),
            now(),
        )
        .await
        .unwrap();

        // Try to consume after expiry (11 minutes later)
        let result = db
            .consume_pending_link("oxeye-expired".to_string(), now() + 660)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_server_lifecycle() {
        let db = Database::open_in_memory().await.unwrap();

        // Create a server
        let server = db
            .create_server("hash123".to_string(), "Survival SMP".to_string(), 12345)
            .await
            .unwrap();
        assert_eq!(server.name, "Survival SMP");

        // Get it by API key
        let server = db
            .get_server_by_api_key("hash123".to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(server.name, "Survival SMP");

        // Get servers by guild
        let servers = db.get_servers_by_guild(12345).await.unwrap();
        assert_eq!(servers.len(), 1);

        // Check name exists
        assert!(
            db.server_name_exists(12345, "Survival SMP".to_string())
                .await
                .unwrap()
        );
        assert!(
            !db.server_name_exists(12345, "Creative".to_string())
                .await
                .unwrap()
        );

        // Delete it
        db.delete_server(12345, "Survival SMP".to_string())
            .await
            .unwrap();
        assert!(
            db.get_server_by_api_key("hash123".to_string())
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn test_player_tracking() {
        let db = Database::open_in_memory().await.unwrap();

        // Create a server first
        db.create_server("hash123".to_string(), "Survival SMP".to_string(), 12345)
            .await
            .unwrap();

        // Player joins
        db.player_join("hash123".to_string(), pn("Steve"), now())
            .await
            .unwrap();
        db.player_join("hash123".to_string(), pn("Alex"), now())
            .await
            .unwrap();

        // Get online players
        let players = db.get_online_players("hash123".to_string()).await.unwrap();
        assert_eq!(players, vec![pn("Alex"), pn("Steve")]);

        // Player leaves
        db.player_leave("hash123".to_string(), pn("Steve"))
            .await
            .unwrap();
        let players = db.get_online_players("hash123".to_string()).await.unwrap();
        assert_eq!(players, vec![pn("Alex")]);

        // Sync players
        db.sync_players("hash123".to_string(), vec![pn("Notch"), pn("jeb_")], now())
            .await
            .unwrap();
        let players = db.get_online_players("hash123".to_string()).await.unwrap();
        assert_eq!(players, vec![pn("Notch"), pn("jeb_")]);
    }

    #[tokio::test]
    async fn test_server_summaries() {
        let db = Database::open_in_memory().await.unwrap();

        db.create_server("hash1".to_string(), "Survival".to_string(), 12345)
            .await
            .unwrap();
        db.create_server("hash2".to_string(), "Creative".to_string(), 12345)
            .await
            .unwrap();

        db.player_join("hash1".to_string(), pn("Steve"), now())
            .await
            .unwrap();
        db.player_join("hash1".to_string(), pn("Alex"), now())
            .await
            .unwrap();

        let summaries = db.get_server_summaries(12345).await.unwrap();
        assert_eq!(summaries.len(), 2);
        assert_eq!(summaries[0].name, "Creative");
        assert_eq!(summaries[0].player_count, 0);
        assert_eq!(summaries[1].name, "Survival");
        assert_eq!(summaries[1].player_count, 2);
    }

    #[tokio::test]
    async fn test_servers_with_players() {
        let db = Database::open_in_memory().await.unwrap();

        db.create_server("hash1".to_string(), "Survival".to_string(), 12345)
            .await
            .unwrap();
        db.create_server("hash2".to_string(), "Creative".to_string(), 12345)
            .await
            .unwrap();

        db.player_join("hash1".to_string(), pn("Steve"), now())
            .await
            .unwrap();
        db.player_join("hash1".to_string(), pn("Alex"), now())
            .await
            .unwrap();

        let servers = db.get_servers_with_players(12345).await.unwrap();
        assert_eq!(servers.len(), 2);
        assert_eq!(servers[0].name, "Creative");
        assert!(servers[0].players.is_empty());
        assert_eq!(servers[1].name, "Survival");
        let player_names: Vec<&str> = servers[1]
            .players
            .iter()
            .map(|p| p.player_name.as_str())
            .collect();
        assert_eq!(player_names, vec!["Alex", "Steve"]);

        // Get specific server
        let server = db
            .get_server_with_players(12345, "Survival".to_string())
            .await
            .unwrap();
        let player_names: Vec<&str> = server
            .players
            .iter()
            .map(|p| p.player_name.as_str())
            .collect();
        assert_eq!(player_names, vec!["Alex", "Steve"]);
    }

    #[tokio::test]
    async fn test_server_name_conflict() {
        let db = Database::open_in_memory().await.unwrap();

        db.create_server("hash1".to_string(), "Survival".to_string(), 12345)
            .await
            .unwrap();

        // Try to create pending link with same name
        let result = db
            .create_pending_link(
                "oxeye-test".to_string(),
                12345,
                "Survival".to_string(),
                now(),
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_player_join_times_and_time_online_calculation() {
        let db = Database::open_in_memory().await.unwrap();

        db.create_server("hash1".to_string(), "Survival".to_string(), 12345)
            .await
            .unwrap();

        // Simulate different join times
        let base_time = 1700000000; // Fixed timestamp
        let player1_join_time = base_time;
        let player2_join_time = base_time + 300; // 5 minutes later
        let player3_join_time = base_time + 3600; // 1 hour later

        // Players join at different times
        db.player_join("hash1".to_string(), pn("Alice"), player1_join_time)
            .await
            .unwrap();
        db.player_join("hash1".to_string(), pn("Bob"), player2_join_time)
            .await
            .unwrap();
        db.player_join("hash1".to_string(), pn("Charlie"), player3_join_time)
            .await
            .unwrap();

        // Retrieve players
        let server = db
            .get_server_with_players(12345, "Survival".to_string())
            .await
            .unwrap();

        assert_eq!(server.players.len(), 3);

        // Verify join times are stored correctly
        let alice = server
            .players
            .iter()
            .find(|p| p.player_name == pn("Alice"))
            .unwrap();
        let bob = server
            .players
            .iter()
            .find(|p| p.player_name == pn("Bob"))
            .unwrap();
        let charlie = server
            .players
            .iter()
            .find(|p| p.player_name == pn("Charlie"))
            .unwrap();

        assert_eq!(alice.joined_at, player1_join_time);
        assert_eq!(bob.joined_at, player2_join_time);
        assert_eq!(charlie.joined_at, player3_join_time);

        // Simulate current time being 2 hours after base_time
        let current_time = base_time + 7200;

        // Calculate time online for each player
        let alice_time_online = current_time - alice.joined_at;
        let bob_time_online = current_time - bob.joined_at;
        let charlie_time_online = current_time - charlie.joined_at;

        // Verify time calculations
        assert_eq!(alice_time_online, 7200); // 2 hours = 7200 seconds
        assert_eq!(bob_time_online, 6900); // 1 hour 55 minutes = 6900 seconds
        assert_eq!(charlie_time_online, 3600); // 1 hour = 3600 seconds

        // Verify time formatting would work correctly
        // Alice: 7200s = 2h
        // Bob: 6900s = 1h 55m = 1h (truncated)
        // Charlie: 3600s = 1h
        assert!(alice_time_online >= 3600); // Should show hours
        assert!(bob_time_online >= 3600); // Should show hours
        assert!(charlie_time_online >= 3600); // Should show hours
    }

    #[tokio::test]
    async fn test_player_time_online_with_join_leave() {
        let db = Database::open_in_memory().await.unwrap();

        db.create_server("hash1".to_string(), "Survival".to_string(), 12345)
            .await
            .unwrap();

        let base_time = 1700000000;

        // Alice joins at base_time
        db.player_join("hash1".to_string(), pn("Alice"), base_time)
            .await
            .unwrap();

        // Bob joins 5 minutes later
        let bob_join_time = base_time + 300;
        db.player_join("hash1".to_string(), pn("Bob"), bob_join_time)
            .await
            .unwrap();

        // Charlie joins 30 minutes later
        let charlie_join_time = base_time + 1800;
        db.player_join("hash1".to_string(), pn("Charlie"), charlie_join_time)
            .await
            .unwrap();

        // Bob leaves after 10 minutes (doesn't affect others' join times)
        db.player_leave("hash1".to_string(), pn("Bob"))
            .await
            .unwrap();

        // Retrieve remaining players
        let server = db
            .get_server_with_players(12345, "Survival".to_string())
            .await
            .unwrap();

        assert_eq!(server.players.len(), 2);

        // Verify Alice and Charlie's join times are preserved
        let alice = server
            .players
            .iter()
            .find(|p| p.player_name == pn("Alice"))
            .unwrap();
        let charlie = server
            .players
            .iter()
            .find(|p| p.player_name == pn("Charlie"))
            .unwrap();

        assert_eq!(alice.joined_at, base_time);
        assert_eq!(charlie.joined_at, charlie_join_time);

        // Simulate current time being 1 hour after base_time
        let current_time = base_time + 3600;

        let alice_time_online = current_time - alice.joined_at;
        let charlie_time_online = current_time - charlie.joined_at;

        assert_eq!(alice_time_online, 3600); // 1 hour
        assert_eq!(charlie_time_online, 1800); // 30 minutes (joined 30 min after Alice)
    }
}
