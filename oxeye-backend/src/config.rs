use std::env::var;
use std::time::Duration;

use dotenvy::dotenv;

/// Application configuration with environment variable overrides
#[derive(Debug, Clone)]
pub struct Config {
    /// Request body size limit in bytes
    /// Env: REQUEST_BODY_LIMIT (default: 1048576 = 1MB)
    pub request_body_limit: usize,

    /// Request timeout in seconds
    /// Env: REQUEST_TIMEOUT_SECS (default: 30)
    pub request_timeout: Duration,

    /// Server port
    /// Env: PORT (default: 3000)
    pub port: u16,

    /// Database file path
    /// Env: DATABASE_PATH (default: "oxeye.db")
    pub database_path: String,

    /// Discord API Token
    /// Env: DISCORD_TOKEN (optional, check at runtime, if doesn't exist, panic)
    pub discord_token: Option<String>,

    /// Discord Command Prefix
    /// Env: DISCORD_COMMAND_PREFIX (default: "!")
    pub discord_command_prefix: String,

    /// Rate limit for /connect endpoint (requests per minute)
    /// Env: RATE_LIMIT_CONNECT_PER_MIN (default: 5)
    /// This is stricter since connect is only used once per server setup
    pub rate_limit_connect_per_min: u64,

    /// Burst size for /connect endpoint
    /// Env: RATE_LIMIT_CONNECT_BURST (default: 2)
    pub rate_limit_connect_burst: u32,

    /// Rate limit for player endpoints like /join, /leave, /sync (requests per second)
    /// Env: RATE_LIMIT_PLAYER_PER_SEC (default: 50)
    /// This is lenient to handle many players joining/leaving at once
    pub rate_limit_player_per_sec: u64,

    /// Burst size for player endpoints
    /// Env: RATE_LIMIT_PLAYER_BURST (default: 100)
    pub rate_limit_player_burst: u32,

    /// Rate limit for general endpoints (requests per second)
    /// Env: RATE_LIMIT_GENERAL_PER_SEC (default: 10)
    pub rate_limit_general_per_sec: u64,

    /// Burst size for general endpoints
    /// Env: RATE_LIMIT_GENERAL_BURST (default: 20)
    pub rate_limit_general_burst: u32,
}

impl Config {
    /// Load configuration from environment variables with defaults
    pub fn from_env() -> Self {
        let _ = dotenv(); //for debugging mostly
        Self {
            request_body_limit: env_or_default("REQUEST_BODY_LIMIT", 1024 * 1024),
            request_timeout: Duration::from_secs(env_or_default("REQUEST_TIMEOUT_SECS", 30)),
            port: env_or_default("PORT", 3000),
            database_path: env_or_default_string("DATABASE_PATH", "oxeye.db"),
            discord_token: var("DISCORD_TOKEN")
                .expect("DISCORD_TOKEN environment variable is required")
                .into(),
            discord_command_prefix: env_or_default_string("DISCORD_COMMAND_PREFIX", "!"),
            rate_limit_connect_per_min: env_or_default("RATE_LIMIT_CONNECT_PER_MIN", 5),
            rate_limit_connect_burst: env_or_default("RATE_LIMIT_CONNECT_BURST", 2),
            rate_limit_player_per_sec: env_or_default("RATE_LIMIT_PLAYER_PER_SEC", 50),
            rate_limit_player_burst: env_or_default("RATE_LIMIT_PLAYER_BURST", 100),
            rate_limit_general_per_sec: env_or_default("RATE_LIMIT_GENERAL_PER_SEC", 10),
            rate_limit_general_burst: env_or_default("RATE_LIMIT_GENERAL_BURST", 20),
        }
    }

    /// Create configuration with all default values
    pub fn default() -> Self {
        Self {
            request_body_limit: 1024 * 1024, // 1 MB
            request_timeout: Duration::from_secs(30),
            port: 3000,
            database_path: "oxeye.db".to_string(),
            discord_token: None,
            discord_command_prefix: "!oxeye".to_string(),
            rate_limit_connect_per_min: 5,
            rate_limit_connect_burst: 2,
            rate_limit_player_per_sec: 50,
            rate_limit_player_burst: 100,
            rate_limit_general_per_sec: 10,
            rate_limit_general_burst: 20,
        }
    }
}

/// Parse environment variable or return default value
fn env_or_default<T: std::str::FromStr>(key: &str, default: T) -> T {
    var(key)
        .ok()
        .and_then(|val| val.parse().ok())
        .unwrap_or(default)
}

/// Parse environment variable string or return default value
fn env_or_default_string(key: &str, default: &str) -> String {
    var(key).unwrap_or_else(|_| default.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.request_body_limit, 1024 * 1024);
        assert_eq!(config.request_timeout, Duration::from_secs(30));
        assert_eq!(config.port, 3000);
        assert_eq!(config.database_path, "oxeye.db");
        assert_eq!(config.rate_limit_connect_per_min, 5);
        assert_eq!(config.rate_limit_connect_burst, 2);
        assert_eq!(config.rate_limit_player_per_sec, 50);
        assert_eq!(config.rate_limit_player_burst, 100);
        assert_eq!(config.rate_limit_general_per_sec, 10);
        assert_eq!(config.rate_limit_general_burst, 20);
    }
}
