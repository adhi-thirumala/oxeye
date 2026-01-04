use arrayvec::ArrayString;

/// Minecraft player name - max 16 characters, stored inline (no heap allocation).
pub type PlayerName = ArrayString<16>;

/// A pending connection code waiting for a Minecraft server to claim it.
#[derive(Debug, Clone)]
pub struct PendingLink {
  /// The connection code (e.g., "oxeye-a1b2c3")
  pub code: String,
  /// Discord guild ID
  pub guild_id: u64,
  /// User-provided server name
  pub server_name: String,
  /// Unix timestamp when this was created
  pub created_at: i64,
}

impl PendingLink {
  /// Check if this pending link has expired (10 minute TTL)
  pub fn is_expired(&self, now: i64) -> bool {
    const TTL_SECONDS: i64 = 600; // 10 minutes
    now - self.created_at > TTL_SECONDS
  }

  /// Seconds remaining until expiry
  pub fn expires_in(&self, now: i64) -> i64 {
    const TTL_SECONDS: i64 = 600;
    (self.created_at + TTL_SECONDS - now).max(0)
  }
}

/// A linked Minecraft server.
#[derive(Debug, Clone)]
pub struct Server {
  /// SHA-256 hash of the API key (primary key)
  pub api_key_hash: String,
  /// User-provided server name
  pub name: String,
  /// Discord guild ID this server is linked to
  pub guild_id: u64,
}

/// An online player on a server.
#[derive(Debug, Clone)]
pub struct OnlinePlayer {
  /// SHA-256 hash of the server's API key
  pub api_key_hash: String,
  /// Player's Minecraft username
  pub player_name: PlayerName,
  /// Unix timestamp when they joined
  pub joined_at: i64,
}

/// Summary of a server with player count.
#[derive(Debug, Clone)]
pub struct ServerSummary {
  pub name: String,
  pub player_count: u32,
}

/// Player info without server context (for use in ServerWithPlayers).
#[derive(Debug, Clone)]
pub struct PlayerInfo {
  /// Player's Minecraft username
  pub player_name: PlayerName,
  /// Unix timestamp when they joined
  pub joined_at: i64,
}

/// Server with its online players.
#[derive(Debug, Clone)]
pub struct ServerWithPlayers {
  pub name: String,
  pub players: Vec<PlayerInfo>,
}
