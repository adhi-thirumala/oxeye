mod error;
mod models;

pub use error::{DbError, Result};
pub use models::{OnlinePlayer, PendingLink, Server, ServerSummary, ServerWithPlayers};

use rusqlite::{params, OptionalExtension};
use std::path::Path;
use tokio_rusqlite::Connection;
use tracing::{debug, info};

/// Database wrapper for all Oxeye operations.
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Open or create a database at the given path.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path).await?;
        let db = Self { conn };
        db.initialize().await?;
        Ok(db)
    }

    /// Create an in-memory database (useful for testing).
    pub async fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory().await.map_err(|e| DbError::Sqlite(e))?;
        let db = Self { conn };
        db.initialize().await?;
        Ok(db)
    }

    /// Initialize the database schema.
    async fn initialize(&self) -> Result<()> {
        self.conn
            .call(|conn| {
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

                    -- Online players
                    CREATE TABLE IF NOT EXISTS online_players (
                        api_key_hash TEXT NOT NULL REFERENCES servers(api_key_hash) ON DELETE CASCADE,
                        player_name TEXT NOT NULL,
                        joined_at INTEGER NOT NULL,
                        PRIMARY KEY (api_key_hash, player_name)
                    );

                    -- Index for fast guild lookups
                    CREATE INDEX IF NOT EXISTS idx_servers_guild ON servers(guild_id);

                    -- Enable foreign keys
                    PRAGMA foreign_keys = ON;
                    "#,
                )?;
                Ok(())
            })
            .await?;

        info!("database initialized");
        Ok(())
    }

    // ========================================================================
    // Pending Links
    // ========================================================================

    /// Create a new pending link.
    /// Returns an error if a server with that name already exists in the guild.
    pub async fn create_pending_link(
        &self,
        code: &str,
        guild_id: u64,
        server_name: &str,
        now: i64,
    ) -> Result<PendingLink> {
        let code = code.to_string();
        let server_name = server_name.to_string();

        let result = self
            .conn
            .call(move |conn| {
                // Check if server name already exists in this guild
                let exists: bool = conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM servers WHERE guild_id = ?1 AND name = ?2)",
                    params![guild_id, &server_name],
                    |row| row.get(0),
                )?;

                if exists {
                    return Ok(Err(DbError::ServerNameConflict));
                }

                conn.execute(
                    "INSERT INTO pending_links (code, guild_id, server_name, created_at) VALUES (?1, ?2, ?3, ?4)",
                    params![&code, guild_id, &server_name, now],
                )?;

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
    pub async fn get_pending_link(&self, code: &str) -> Result<Option<PendingLink>> {
        let code = code.to_string();

        let link = self
            .conn
            .call(move |conn| {
                conn.query_row(
                    "SELECT code, guild_id, server_name, created_at FROM pending_links WHERE code = ?1",
                    params![&code],
                    |row| {
                        Ok(PendingLink {
                            code: row.get(0)?,
                            guild_id: row.get(1)?,
                            server_name: row.get(2)?,
                            created_at: row.get(3)?,
                        })
                    },
                )
                .optional()
            })
            .await?;

        Ok(link)
    }

    /// Consume a pending link (delete it and return it).
    /// Returns an error if not found or expired.
    pub async fn consume_pending_link(&self, code: &str, now: i64) -> Result<PendingLink> {
        let code = code.to_string();

        let result = self
            .conn
            .call(move |conn| {
                let link: Option<PendingLink> = conn
                    .query_row(
                        "SELECT code, guild_id, server_name, created_at FROM pending_links WHERE code = ?1",
                        params![&code],
                        |row| {
                            Ok(PendingLink {
                                code: row.get(0)?,
                                guild_id: row.get(1)?,
                                server_name: row.get(2)?,
                                created_at: row.get(3)?,
                            })
                        },
                    )
                    .optional()?;

                let link = match link {
                    Some(l) => l,
                    None => return Ok(Err(DbError::PendingLinkNotFound)),
                };

                if link.is_expired(now) {
                    conn.execute("DELETE FROM pending_links WHERE code = ?1", params![&code])?;
                    return Ok(Err(DbError::PendingLinkNotFound));
                }

                conn.execute("DELETE FROM pending_links WHERE code = ?1", params![&code])?;
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

                let deleted = conn.execute(
                    "DELETE FROM pending_links WHERE created_at < ?1",
                    params![cutoff],
                )?;
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
        api_key_hash: &str,
        name: &str,
        guild_id: u64,
    ) -> Result<Server> {
        let api_key_hash = api_key_hash.to_string();
        let name = name.to_string();

        let server = self
            .conn
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO servers (api_key_hash, name, guild_id) VALUES (?1, ?2, ?3)",
                    params![&api_key_hash, &name, guild_id],
                )?;

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
    pub async fn get_server_by_api_key(&self, api_key_hash: &str) -> Result<Option<Server>> {
        let api_key_hash = api_key_hash.to_string();

        let server = self
            .conn
            .call(move |conn| {
                conn.query_row(
                    "SELECT api_key_hash, name, guild_id FROM servers WHERE api_key_hash = ?1",
                    params![&api_key_hash],
                    |row| {
                        Ok(Server {
                            api_key_hash: row.get(0)?,
                            name: row.get(1)?,
                            guild_id: row.get(2)?,
                        })
                    },
                )
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
                let mut stmt = conn.prepare(
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
        let summaries = self
            .conn
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    r#"
                    SELECT s.name, COUNT(op.player_name) as player_count
                    FROM servers s
                    LEFT JOIN online_players op ON s.api_key_hash = op.api_key_hash
                    WHERE s.guild_id = ?1
                    GROUP BY s.api_key_hash
                    ORDER BY s.name
                    "#,
                )?;

                let summaries = stmt
                    .query_map(params![guild_id], |row| {
                        Ok(ServerSummary {
                            name: row.get(0)?,
                            player_count: row.get(1)?,
                        })
                    })?
                    .collect::<std::result::Result<Vec<_>, _>>()?;

                Ok(summaries)
            })
            .await?;

        Ok(summaries)
    }

    /// Delete a server by guild and name.
    pub async fn delete_server(&self, guild_id: u64, name: &str) -> Result<()> {
        let name = name.to_string();

        let result = self
            .conn
            .call(move |conn| {
                let deleted = conn.execute(
                    "DELETE FROM servers WHERE guild_id = ?1 AND name = ?2",
                    params![guild_id, &name],
                )?;

                if deleted == 0 {
                    return Ok(Err(DbError::ServerNotFound));
                }

                Ok(Ok(()))
            })
            .await??;

        debug!(guild_id, "deleted server");
        Ok(result)
    }

    /// Check if a server name exists in a guild.
    pub async fn server_name_exists(&self, guild_id: u64, name: &str) -> Result<bool> {
        let name = name.to_string();

        let exists = self
            .conn
            .call(move |conn| {
                let exists: bool = conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM servers WHERE guild_id = ?1 AND name = ?2)",
                    params![guild_id, &name],
                    |row| row.get(0),
                )?;

                Ok(exists)
            })
            .await?;

        Ok(exists)
    }

    // ========================================================================
    // Online Players
    // ========================================================================

    /// Record a player joining.
    pub async fn player_join(&self, api_key_hash: &str, player_name: &str, now: i64) -> Result<()> {
        let api_key_hash = api_key_hash.to_string();
        let player_name = player_name.to_string();
        let player_name_log = player_name.clone();

        self.conn
            .call(move |conn| {
                // Verify the server exists
                let exists: bool = conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM servers WHERE api_key_hash = ?1)",
                    params![&api_key_hash],
                    |row| row.get(0),
                )?;

                if !exists {
                    return Ok(Err(DbError::InvalidApiKey));
                }

                conn.execute(
                    "INSERT OR REPLACE INTO online_players (api_key_hash, player_name, joined_at) VALUES (?1, ?2, ?3)",
                    params![&api_key_hash, &player_name, now],
                )?;

                Ok(Ok(()))
            })
            .await??;

        debug!(player_name = %player_name_log, "player joined");
        Ok(())
    }

    /// Record a player leaving.
    pub async fn player_leave(&self, api_key_hash: &str, player_name: &str) -> Result<()> {
        let api_key_hash = api_key_hash.to_string();
        let player_name = player_name.to_string();
        let player_name_log = player_name.clone();

        self.conn
            .call(move |conn| {
                // Verify the server exists
                let exists: bool = conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM servers WHERE api_key_hash = ?1)",
                    params![&api_key_hash],
                    |row| row.get(0),
                )?;

                if !exists {
                    return Ok(Err(DbError::InvalidApiKey));
                }

                conn.execute(
                    "DELETE FROM online_players WHERE api_key_hash = ?1 AND player_name = ?2",
                    params![&api_key_hash, &player_name],
                )?;

                Ok(Ok(()))
            })
            .await??;

        debug!(player_name = %player_name_log, "player left");
        Ok(())
    }

    /// Sync the player list (replace all players for a server).
    pub async fn sync_players(
        &self,
        api_key_hash: &str,
        players: &[String],
        now: i64,
    ) -> Result<()> {
        let api_key_hash = api_key_hash.to_string();
        let players = players.to_vec();
        let count = players.len();

        self.conn
            .call(move |conn| {
                // Verify the server exists
                let exists: bool = conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM servers WHERE api_key_hash = ?1)",
                    params![&api_key_hash],
                    |row| row.get(0),
                )?;

                if !exists {
                    return Ok(Err(DbError::InvalidApiKey));
                }

                // Delete all existing players for this server
                conn.execute(
                    "DELETE FROM online_players WHERE api_key_hash = ?1",
                    params![&api_key_hash],
                )?;

                // Insert all new players
                for player in &players {
                    conn.execute(
                        "INSERT INTO online_players (api_key_hash, player_name, joined_at) VALUES (?1, ?2, ?3)",
                        params![&api_key_hash, player, now],
                    )?;
                }

                Ok(Ok(()))
            })
            .await??;

        debug!(count, "synced players");
        Ok(())
    }

    /// Get online players for a server.
    pub async fn get_online_players(&self, api_key_hash: &str) -> Result<Vec<String>> {
        let api_key_hash = api_key_hash.to_string();

        let players = self
            .conn
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT player_name FROM online_players WHERE api_key_hash = ?1 ORDER BY player_name",
                )?;

                let players = stmt
                    .query_map(params![&api_key_hash], |row| row.get(0))?
                    .collect::<std::result::Result<Vec<String>, _>>()?;

                Ok(players)
            })
            .await?;

        Ok(players)
    }

    /// Get all servers with their online players for a guild.
    pub async fn get_servers_with_players(&self, guild_id: u64) -> Result<Vec<ServerWithPlayers>> {
        let result = self
            .conn
            .call(move |conn| {
                // First get all servers for the guild
                let mut server_stmt = conn
                    .prepare("SELECT api_key_hash, name FROM servers WHERE guild_id = ?1 ORDER BY name")?;

                let servers: Vec<(String, String)> = server_stmt
                    .query_map(params![guild_id], |row| Ok((row.get(0)?, row.get(1)?)))?
                    .collect::<std::result::Result<Vec<_>, _>>()?;

                // Then get players for each server
                let mut player_stmt = conn.prepare(
                    "SELECT player_name FROM online_players WHERE api_key_hash = ?1 ORDER BY player_name",
                )?;

                let mut result = Vec::new();
                for (api_key_hash, name) in servers {
                    let players: Vec<String> = player_stmt
                        .query_map(params![&api_key_hash], |row| row.get(0))?
                        .collect::<std::result::Result<Vec<_>, _>>()?;

                    result.push(ServerWithPlayers { name, players });
                }

                Ok(result)
            })
            .await?;

        Ok(result)
    }

    /// Get a specific server with its online players.
    pub async fn get_server_with_players(
        &self,
        guild_id: u64,
        server_name: &str,
    ) -> Result<ServerWithPlayers> {
        let server_name = server_name.to_string();

        let result = self
            .conn
            .call(move |conn| {
                // Get the server
                let api_key_hash: Option<String> = conn
                    .query_row(
                        "SELECT api_key_hash FROM servers WHERE guild_id = ?1 AND name = ?2",
                        params![guild_id, &server_name],
                        |row| row.get(0),
                    )
                    .optional()?;

                let api_key_hash = match api_key_hash {
                    Some(h) => h,
                    None => return Ok(Err(DbError::ServerNotFound)),
                };

                // Get players
                let mut stmt = conn.prepare(
                    "SELECT player_name FROM online_players WHERE api_key_hash = ?1 ORDER BY player_name",
                )?;

                let players = stmt
                    .query_map(params![&api_key_hash], |row| row.get(0))?
                    .collect::<std::result::Result<Vec<String>, _>>()?;

                Ok(Ok(ServerWithPlayers {
                    name: server_name,
                    players,
                }))
            })
            .await??;

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> i64 {
        1700000000 // Fixed timestamp for testing
    }

    #[tokio::test]
    async fn test_pending_link_lifecycle() {
        let db = Database::open_in_memory().await.unwrap();

        // Create a pending link
        let link = db
            .create_pending_link("oxeye-abc123", 12345, "Survival SMP", now())
            .await
            .unwrap();
        assert_eq!(link.code, "oxeye-abc123");
        assert_eq!(link.guild_id, 12345);
        assert_eq!(link.server_name, "Survival SMP");

        // Get it
        let link = db.get_pending_link("oxeye-abc123").await.unwrap().unwrap();
        assert_eq!(link.code, "oxeye-abc123");

        // Consume it
        let link = db.consume_pending_link("oxeye-abc123", now()).await.unwrap();
        assert_eq!(link.server_name, "Survival SMP");

        // Should be gone now
        assert!(db.get_pending_link("oxeye-abc123").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_expired_link() {
        let db = Database::open_in_memory().await.unwrap();

        db.create_pending_link("oxeye-expired", 12345, "Test", now())
            .await
            .unwrap();

        // Try to consume after expiry (11 minutes later)
        let result = db.consume_pending_link("oxeye-expired", now() + 660).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_server_lifecycle() {
        let db = Database::open_in_memory().await.unwrap();

        // Create a server
        let server = db
            .create_server("hash123", "Survival SMP", 12345)
            .await
            .unwrap();
        assert_eq!(server.name, "Survival SMP");

        // Get it by API key
        let server = db.get_server_by_api_key("hash123").await.unwrap().unwrap();
        assert_eq!(server.name, "Survival SMP");

        // Get servers by guild
        let servers = db.get_servers_by_guild(12345).await.unwrap();
        assert_eq!(servers.len(), 1);

        // Check name exists
        assert!(db.server_name_exists(12345, "Survival SMP").await.unwrap());
        assert!(!db.server_name_exists(12345, "Creative").await.unwrap());

        // Delete it
        db.delete_server(12345, "Survival SMP").await.unwrap();
        assert!(db.get_server_by_api_key("hash123").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_player_tracking() {
        let db = Database::open_in_memory().await.unwrap();

        // Create a server first
        db.create_server("hash123", "Survival SMP", 12345)
            .await
            .unwrap();

        // Player joins
        db.player_join("hash123", "Steve", now()).await.unwrap();
        db.player_join("hash123", "Alex", now()).await.unwrap();

        // Get online players
        let players = db.get_online_players("hash123").await.unwrap();
        assert_eq!(players, vec!["Alex", "Steve"]);

        // Player leaves
        db.player_leave("hash123", "Steve").await.unwrap();
        let players = db.get_online_players("hash123").await.unwrap();
        assert_eq!(players, vec!["Alex"]);

        // Sync players
        db.sync_players("hash123", &["Notch".to_string(), "jeb_".to_string()], now())
            .await
            .unwrap();
        let players = db.get_online_players("hash123").await.unwrap();
        assert_eq!(players, vec!["Notch", "jeb_"]);
    }

    #[tokio::test]
    async fn test_server_summaries() {
        let db = Database::open_in_memory().await.unwrap();

        db.create_server("hash1", "Survival", 12345).await.unwrap();
        db.create_server("hash2", "Creative", 12345).await.unwrap();

        db.player_join("hash1", "Steve", now()).await.unwrap();
        db.player_join("hash1", "Alex", now()).await.unwrap();

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

        db.create_server("hash1", "Survival", 12345).await.unwrap();
        db.create_server("hash2", "Creative", 12345).await.unwrap();

        db.player_join("hash1", "Steve", now()).await.unwrap();
        db.player_join("hash1", "Alex", now()).await.unwrap();

        let servers = db.get_servers_with_players(12345).await.unwrap();
        assert_eq!(servers.len(), 2);
        assert_eq!(servers[0].name, "Creative");
        assert!(servers[0].players.is_empty());
        assert_eq!(servers[1].name, "Survival");
        assert_eq!(servers[1].players, vec!["Alex", "Steve"]);

        // Get specific server
        let server = db.get_server_with_players(12345, "Survival").await.unwrap();
        assert_eq!(server.players, vec!["Alex", "Steve"]);
    }

    #[tokio::test]
    async fn test_server_name_conflict() {
        let db = Database::open_in_memory().await.unwrap();

        db.create_server("hash1", "Survival", 12345).await.unwrap();

        // Try to create pending link with same name
        let result = db
            .create_pending_link("oxeye-test", 12345, "Survival", now())
            .await;
        assert!(result.is_err());
    }
}
