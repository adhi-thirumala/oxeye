//! In-memory cache for online players.
//!
//! This module provides a lock-free in-memory storage for ephemeral player data.
//! Player data resyncs on reconnect, so durability isn't needed.

use crate::models::PlayerName;

/// State of a single Minecraft server's online players.
#[derive(Debug, Default)]
pub struct ServerState {
    /// Online players with their join timestamps.
    /// Using Vec for cache-friendly iteration at small N.
    /// Each entry is (player_name, joined_at).
    pub players: Vec<(PlayerName, i64)>,
    /// Whether this server has synced since backend restart.
    pub synced_since_boot: bool,
}

impl ServerState {
    /// Create a new empty server state.
    pub fn new() -> Self {
        Self {
            players: Vec::new(),
            synced_since_boot: false,
        }
    }

    /// Add a player to the server.
    /// If player already exists, updates their join time.
    pub fn add_player(&mut self, name: PlayerName, joined_at: i64) {
        self.synced_since_boot = true;
        // Check if player already exists
        if let Some(idx) = self.players.iter().position(|(n, _)| *n == name) {
            self.players[idx].1 = joined_at;
        } else {
            self.players.push((name, joined_at));
        }
    }

    /// Remove a player from the server.
    /// Uses swap_remove for O(1) removal (order doesn't matter for players).
    pub fn remove_player(&mut self, name: &PlayerName) {
        self.synced_since_boot = true;
        if let Some(idx) = self.players.iter().position(|(n, _)| n == name) {
            self.players.swap_remove(idx);
        }
    }

    /// Replace all players (for sync operation).
    pub fn sync_players(&mut self, players: Vec<(PlayerName, i64)>) {
        self.players = players;
        self.synced_since_boot = true;
    }

    /// Get player count.
    pub fn player_count(&self) -> usize {
        self.players.len()
    }
}

/// Thread-safe cache for all online players across all servers.
/// Uses scc::HashMap for lock-free concurrent access.
pub type OnlineCache = scc::HashMap<String, ServerState>;

/// Create a new empty online cache.
pub fn new_cache() -> OnlineCache {
    OnlineCache::new()
}
