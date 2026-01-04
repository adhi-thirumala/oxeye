use arrayvec::ArrayString;
use scc::HashMap;

pub type PlayerName = ArrayString<16>;

#[derive(Clone)]
pub struct PlayerEntry {
    pub name: PlayerName,
    pub joined_at: i64,
}

pub struct ServerState {
    pub players: Vec<PlayerEntry>,
    pub synced_since_boot: bool,
}

/// In-memory cache for online players.
pub struct OnlineCache {
    servers: HashMap<String, ServerState>,
}

impl OnlineCache {
    pub fn new() -> Self {
        Self {
            servers: HashMap::new(),
        }
    }

    /// Called when a server connects/registers.
    pub async fn register_server(&self, api_key_hash: &str) -> Result<(), CacheError> {
        self.servers
            .insert_async(
                api_key_hash.to_string(),
                ServerState {
                    players: Vec::new(),
                    synced_since_boot: false,
                },
            )
            .await
            .map_err(|_| CacheError::ServerAlreadyExists)
    }

    /// Called when a server disconnects.
    pub async fn unregister_server(&self, api_key_hash: &str) {
        let _ = self.servers.remove_async(api_key_hash).await;
    }

    /// Record a player joining. Returns error if server not found or name too long.
    pub async fn player_join(
        &self,
        api_key_hash: &str,
        player: &str,
        now: i64,
    ) -> Result<(), CacheError> {
        let name = PlayerName::try_from(player).map_err(|_| CacheError::PlayerNameTooLong)?;

        self.servers
            .update_async(api_key_hash, |_, state| {
                // Don't add duplicates
                if !state.players.iter().any(|p| p.name == name) {
                    state.players.push(PlayerEntry {
                        name,
                        joined_at: now,
                    });
                }
            })
            .await
            .ok_or(CacheError::ServerNotFound)
    }

    /// Record a player leaving. Uses swap_remove for O(1).
    pub async fn player_leave(&self, api_key_hash: &str, player: &str) -> Result<(), CacheError> {
        let name = PlayerName::try_from(player).map_err(|_| CacheError::PlayerNameTooLong)?;

        self.servers
            .update_async(api_key_hash, |_, state| {
                if let Some(idx) = state.players.iter().position(|p| p.name == name) {
                    state.players.swap_remove(idx);
                }
            })
            .await
            .ok_or(CacheError::ServerNotFound)
    }

    /// Replace all players with the given list. Sets synced_since_boot = true.
    pub async fn sync_players(
        &self,
        api_key_hash: &str,
        players: &[String],
        now: i64,
    ) -> Result<(), CacheError> {
        let entries: Vec<PlayerEntry> = players
            .iter()
            .filter_map(|p| {
                PlayerName::try_from(p.as_str())
                    .ok()
                    .map(|name| PlayerEntry {
                        name,
                        joined_at: now,
                    })
            })
            .collect();

        self.servers
            .update_async(api_key_hash, |_, state| {
                state.players = entries.clone();
                state.synced_since_boot = true;
            })
            .await
            .ok_or(CacheError::ServerNotFound)
    }

    /// Get a copy of players for a server.
    pub async fn get_players(&self, api_key_hash: &str) -> Option<Vec<PlayerEntry>> {
        self.servers
            .read_async(api_key_hash, |_, state| state.players.clone())
            .await
    }

    /// Get players and sync status for a server.
    pub async fn get_server_state(&self, api_key_hash: &str) -> Option<(Vec<PlayerEntry>, bool)> {
        self.servers
            .read_async(api_key_hash, |_, state| {
                (state.players.clone(), state.synced_since_boot)
            })
            .await
    }

    /// Get states for multiple servers (for Discord bot status command).
    pub async fn iter_guild_servers(
        &self,
        api_key_hashes: &[String],
    ) -> Vec<(String, Vec<PlayerEntry>, bool)> {
        let mut results = Vec::with_capacity(api_key_hashes.len());
        for hash in api_key_hashes {
            if let Some(state) = self
                .servers
                .read_async(hash, |_, state| {
                    (hash.clone(), state.players.clone(), state.synced_since_boot)
                })
                .await
            {
                results.push(state);
            }
        }
        results
    }
}

impl Default for OnlineCache {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("server not found in cache")]
    ServerNotFound,
    #[error("player name exceeds 16 characters")]
    PlayerNameTooLong,
    #[error("server already exists in cache")]
    ServerAlreadyExists,
}
