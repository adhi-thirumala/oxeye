mod error;
mod models;

pub use error::{DbError, Result};
pub use models::{OnlinePlayer, PendingLink, PlayerInfo, Server, ServerSummary, ServerWithPlayers};

use std::path::Path;
use tokio_rusqlite::Connection;
use tokio_rusqlite::rusqlite::{OptionalExtension, params};
use tracing::{debug, info};

/// Database wrapper for all Oxeye operations.
#[derive(Clone)]
pub struct Database {
  conn: Connection,
}

impl Database {
  /// Open or create a database at the given path.
  pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
    let conn = Connection::open(path).await.map_err(DbError::Sqlite)?;
    let db = Self { conn };
    db.initialize().await?;
    Ok(db)
  }

  /// Create an in-memory database (useful for testing).
  pub async fn open_in_memory() -> Result<Self> {
    let conn = Connection::open_in_memory()
      .await
      .map_err(DbError::Sqlite)?;
    let db = Self { conn };
    db.initialize().await?;
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

                    -- Online players
                    CREATE TABLE IF NOT EXISTS online_players (
                        api_key_hash TEXT NOT NULL REFERENCES servers(api_key_hash) ON DELETE CASCADE,
                        player_name TEXT NOT NULL,
                        joined_at INTEGER NOT NULL,
                        PRIMARY KEY (api_key_hash, player_name)
                    );

                    -- Index for fast guild lookups
                    CREATE INDEX IF NOT EXISTS idx_servers_guild ON servers(guild_id);
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
        conn
          .prepare_cached("INSERT INTO servers (api_key_hash, name, guild_id) VALUES (?1, ?2, ?3)")?
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
        conn
          .prepare_cached(
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
        let mut stmt = conn
          .prepare_cached("SELECT api_key_hash, name, guild_id FROM servers WHERE guild_id = ?1")?;

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
        let mut stmt = conn.prepare_cached(
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
  pub async fn delete_server(&self, guild_id: u64, name: String) -> Result<()> {
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

    debug!(guild_id, "deleted server");
    Ok(result)
  }

  /// Delete a server by API key hash (for self-disconnect).
  pub async fn delete_server_by_api_key(&self, api_key_hash: String) -> Result<()> {
    let result = self
      .conn
      .call(move |conn| {
        let deleted = conn
          .prepare_cached("DELETE FROM servers WHERE api_key_hash = ?1")?
          .execute(params![&api_key_hash])?;

        if deleted == 0 {
          return Ok(Err(DbError::InvalidApiKey));
        }

        Ok(Ok(()))
      })
      .await??;

    debug!("deleted server by api key");
    Ok(result)
  }

  /// Check if a server name exists in a guild.
  pub async fn server_name_exists(&self, guild_id: u64, name: String) -> Result<bool> {
    let exists = self
      .conn
      .call(move |conn| {
        let exists: bool = conn
          .prepare_cached("SELECT EXISTS(SELECT 1 FROM servers WHERE guild_id = ?1 AND name = ?2)")?
          .query_row(params![guild_id, &name], |row| row.get(0))?;

        Ok(exists)
      })
      .await?;

    Ok(exists)
  }

  // ========================================================================
  // Online Players
  // ========================================================================

  /// Record a player joining.
  pub async fn player_join(
    &self,
    api_key_hash: String,
    player_name: String,
    now: i64,
  ) -> Result<()> {
    let player_name_log = player_name.clone();

    self.conn
            .call(move |conn| {
                let tx = conn.transaction()?;

                // Verify the server exists
                let exists: bool = tx
                    .prepare_cached("SELECT EXISTS(SELECT 1 FROM servers WHERE api_key_hash = ?1)")?
                    .query_row(params![&api_key_hash], |row| row.get(0))?;

                if !exists {
                    return Ok(Err(DbError::InvalidApiKey));
                }

                tx.prepare_cached(
                    "INSERT OR REPLACE INTO online_players (api_key_hash, player_name, joined_at) VALUES (?1, ?2, ?3)",
                )?
                    .execute(params![&api_key_hash, &player_name, now])?;

                tx.commit()?;
                Ok(Ok(()))
            })
            .await??;

    debug!(player_name = %player_name_log, "player joined");
    Ok(())
  }

  /// Record a player leaving.
  pub async fn player_leave(&self, api_key_hash: String, player_name: String) -> Result<()> {
    let player_name_log = player_name.clone();

    self
      .conn
      .call(move |conn| {
        let tx = conn.transaction()?;

        // Verify the server exists
        let exists: bool = tx
          .prepare_cached("SELECT EXISTS(SELECT 1 FROM servers WHERE api_key_hash = ?1)")?
          .query_row(params![&api_key_hash], |row| row.get(0))?;

        if !exists {
          return Ok(Err(DbError::InvalidApiKey));
        }

        tx.prepare_cached(
          "DELETE FROM online_players WHERE api_key_hash = ?1 AND player_name = ?2",
        )?
        .execute(params![&api_key_hash, &player_name])?;

        tx.commit()?;
        Ok(Ok(()))
      })
      .await??;

    debug!(player_name = %player_name_log, "player left");
    Ok(())
  }

  /// Sync the player list (replace all players for a server).
  pub async fn sync_players(
    &self,
    api_key_hash: String,
    players: Vec<String>,
    now: i64,
  ) -> Result<()> {
    let count = players.len();

    self
      .conn
      .call(move |conn| {
        let tx = conn.transaction()?;

        // Verify the server exists
        let exists: bool = tx
          .prepare_cached("SELECT EXISTS(SELECT 1 FROM servers WHERE api_key_hash = ?1)")?
          .query_row(params![&api_key_hash], |row| row.get(0))?;

        if !exists {
          return Ok(Err(DbError::InvalidApiKey));
        }

        // Delete all existing players for this server
        tx.prepare_cached("DELETE FROM online_players WHERE api_key_hash = ?1")?
          .execute(params![&api_key_hash])?;

        // Insert all new players
        {
          let mut insert_stmt = tx.prepare_cached(
            "INSERT INTO online_players (api_key_hash, player_name, joined_at) VALUES (?1, ?2, ?3)",
          )?;
          for player in &players {
            insert_stmt.execute(params![&api_key_hash, player, now])?;
          }
        }

        tx.commit()?;
        Ok(Ok(()))
      })
      .await??;

    debug!(count, "synced players");
    Ok(())
  }

  /// Get online players for a server.
  pub async fn get_online_players(&self, api_key_hash: String) -> Result<Vec<String>> {
    let players = self
      .conn
      .call(move |conn| {
        let mut stmt = conn.prepare_cached(
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
        let mut server_stmt = conn.prepare_cached(
          "SELECT api_key_hash, name FROM servers WHERE guild_id = ?1 ORDER BY name",
        )?;

        let servers: Vec<(String, String)> = server_stmt
          .query_map(params![guild_id], |row| Ok((row.get(0)?, row.get(1)?)))?
          .collect::<std::result::Result<Vec<_>, _>>()?;

        // Then get players for each server
        let mut player_stmt = conn.prepare_cached(
          "SELECT player_name, joined_at FROM online_players WHERE api_key_hash = ?1 ORDER BY player_name",
        )?;

        let mut result = Vec::new();
        for (api_key_hash, name) in servers {
          let players: Vec<PlayerInfo> = player_stmt
            .query_map(params![&api_key_hash], |row| {
              Ok(PlayerInfo {
                player_name: row.get(0)?,
                joined_at: row.get(1)?,
              })
            })?
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
    server_name: String,
  ) -> Result<ServerWithPlayers> {
    let result = self
      .conn
      .call(move |conn| {
        // Get the server
        let api_key_hash: Option<String> = conn
          .prepare_cached("SELECT api_key_hash FROM servers WHERE guild_id = ?1 AND name = ?2")?
          .query_row(params![guild_id, &server_name], |row| row.get(0))
          .optional()?;

        let api_key_hash = match api_key_hash {
          Some(h) => h,
          None => return Ok(Err(DbError::ServerNotFound)),
        };

        // Get players
        let mut stmt = conn.prepare_cached(
          "SELECT player_name, joined_at FROM online_players WHERE api_key_hash = ?1 ORDER BY player_name",
        )?;

        let players = stmt
          .query_map(params![&api_key_hash], |row| {
            Ok(PlayerInfo {
              player_name: row.get(0)?,
              joined_at: row.get(1)?,
            })
          })?
          .collect::<std::result::Result<Vec<PlayerInfo>, _>>()?;

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
      !db
        .server_name_exists(12345, "Creative".to_string())
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
    db.player_join("hash123".to_string(), "Steve".to_string(), now())
      .await
      .unwrap();
    db.player_join("hash123".to_string(), "Alex".to_string(), now())
      .await
      .unwrap();

    // Get online players
    let players = db.get_online_players("hash123".to_string()).await.unwrap();
    assert_eq!(players, vec!["Alex", "Steve"]);

    // Player leaves
    db.player_leave("hash123".to_string(), "Steve".to_string())
      .await
      .unwrap();
    let players = db.get_online_players("hash123".to_string()).await.unwrap();
    assert_eq!(players, vec!["Alex"]);

    // Sync players
    db.sync_players(
      "hash123".to_string(),
      vec!["Notch".to_string(), "jeb_".to_string()],
      now(),
    )
    .await
    .unwrap();
    let players = db.get_online_players("hash123".to_string()).await.unwrap();
    assert_eq!(players, vec!["Notch", "jeb_"]);
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

    db.player_join("hash1".to_string(), "Steve".to_string(), now())
      .await
      .unwrap();
    db.player_join("hash1".to_string(), "Alex".to_string(), now())
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

    db.player_join("hash1".to_string(), "Steve".to_string(), now())
      .await
      .unwrap();
    db.player_join("hash1".to_string(), "Alex".to_string(), now())
      .await
      .unwrap();

    let servers = db.get_servers_with_players(12345).await.unwrap();
    assert_eq!(servers.len(), 2);
    assert_eq!(servers[0].name, "Creative");
    assert!(servers[0].players.is_empty());
    assert_eq!(servers[1].name, "Survival");
    let player_names: Vec<&str> = servers[1].players.iter().map(|p| p.player_name.as_str()).collect();
    assert_eq!(player_names, vec!["Alex", "Steve"]);

    // Get specific server
    let server = db
      .get_server_with_players(12345, "Survival".to_string())
      .await
      .unwrap();
    let player_names: Vec<&str> = server.players.iter().map(|p| p.player_name.as_str()).collect();
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
    db.player_join("hash1".to_string(), "Alice".to_string(), player1_join_time)
      .await
      .unwrap();
    db.player_join("hash1".to_string(), "Bob".to_string(), player2_join_time)
      .await
      .unwrap();
    db.player_join("hash1".to_string(), "Charlie".to_string(), player3_join_time)
      .await
      .unwrap();

    // Retrieve players
    let server = db
      .get_server_with_players(12345, "Survival".to_string())
      .await
      .unwrap();

    assert_eq!(server.players.len(), 3);

    // Verify join times are stored correctly
    let alice = server.players.iter().find(|p| p.player_name == "Alice").unwrap();
    let bob = server.players.iter().find(|p| p.player_name == "Bob").unwrap();
    let charlie = server.players.iter().find(|p| p.player_name == "Charlie").unwrap();

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
    db.player_join("hash1".to_string(), "Alice".to_string(), base_time)
      .await
      .unwrap();

    // Bob joins 5 minutes later
    let bob_join_time = base_time + 300;
    db.player_join("hash1".to_string(), "Bob".to_string(), bob_join_time)
      .await
      .unwrap();

    // Charlie joins 30 minutes later
    let charlie_join_time = base_time + 1800;
    db.player_join("hash1".to_string(), "Charlie".to_string(), charlie_join_time)
      .await
      .unwrap();

    // Bob leaves after 10 minutes (doesn't affect others' join times)
    db.player_leave("hash1".to_string(), "Bob".to_string())
      .await
      .unwrap();

    // Retrieve remaining players
    let server = db
      .get_server_with_players(12345, "Survival".to_string())
      .await
      .unwrap();

    assert_eq!(server.players.len(), 2);

    // Verify Alice and Charlie's join times are preserved
    let alice = server.players.iter().find(|p| p.player_name == "Alice").unwrap();
    let charlie = server.players.iter().find(|p| p.player_name == "Charlie").unwrap();

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
