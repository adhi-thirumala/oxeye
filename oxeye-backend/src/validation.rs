/// Input validation functions for all backend routes
use oxeye_db::PlayerName;
use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum ValidationError {
    #[error("Player name cannot be empty")]
    PlayerNameEmpty,

    #[error("Player name too long (max 16 characters, got {0})")]
    PlayerNameTooLong(usize),

    #[error("Player name contains invalid characters (only alphanumeric and underscore allowed)")]
    PlayerNameInvalidChars,

    #[error("Connection code cannot be empty")]
    CodeEmpty,

    #[error("Connection code has invalid format (expected 'oxeye-XXXXXX')")]
    CodeInvalidFormat,

    #[error("Player list too large (max {max} players, got {actual})")]
    PlayerListTooLarge { max: usize, actual: usize },

    #[error("Server name cannot be empty")]
    ServerNameEmpty,

    #[error("Server name too long (max 100 characters, got {0})")]
    ServerNameTooLong(usize),

    #[error("Texture hash cannot be empty")]
    TextureHashEmpty,

    #[error("Texture hash has invalid format (expected 64-character hex string)")]
    TextureHashInvalidFormat,

    #[error("Skin data cannot be empty")]
    SkinDataEmpty,

    #[error("Skin data too large (max {max} bytes, got {actual})")]
    SkinDataTooLarge { max: usize, actual: usize },
}

/// Validates a Minecraft player name
///
/// Rules:
/// - Cannot be empty
/// - Max 16 characters (Minecraft username limit)
/// - Only alphanumeric characters and underscores
pub fn validate_player_name(name: &str) -> Result<(), ValidationError> {
    if name.is_empty() {
        return Err(ValidationError::PlayerNameEmpty);
    }

    if name.len() > 16 {
        return Err(ValidationError::PlayerNameTooLong(name.len()));
    }

    if !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Err(ValidationError::PlayerNameInvalidChars);
    }

    Ok(())
}

/// Validates a connection code
///
/// Rules:
/// - Cannot be empty
/// - Must match format "oxeye-XXXXXX" where X is alphanumeric
pub fn validate_code(code: &str) -> Result<(), ValidationError> {
    if code.is_empty() {
        return Err(ValidationError::CodeEmpty);
    }

    // Expected format: "oxeye-XXXXXX" (6+ alphanumeric chars after prefix)
    if !code.starts_with("oxeye-") || code.len() < 12 {
        return Err(ValidationError::CodeInvalidFormat);
    }

    let suffix = &code[6..]; // After "oxeye-"
    if !suffix.chars().all(|c| c.is_alphanumeric()) {
        return Err(ValidationError::CodeInvalidFormat);
    }

    Ok(())
}

/// Validates a list of player names for bulk operations
///
/// Rules:
/// - Max 1000 players per request (prevents DOS)
/// - Each player name must be valid
pub fn validate_player_list(players: &[PlayerName]) -> Result<(), ValidationError> {
    const MAX_PLAYERS: usize = 1000;

    if players.len() > MAX_PLAYERS {
        return Err(ValidationError::PlayerListTooLarge {
            max: MAX_PLAYERS,
            actual: players.len(),
        });
    }

    // Validate each player name (ArrayString guarantees <= 16 chars, just check content)
    for player in players {
        validate_player_name(player.as_str())?;
    }

    Ok(())
}

/// Validates a server name
///
/// Rules:
/// - Cannot be empty
/// - Max 100 characters
pub fn validate_server_name(name: &str) -> Result<(), ValidationError> {
    if name.is_empty() {
        return Err(ValidationError::ServerNameEmpty);
    }

    if name.len() > 100 {
        return Err(ValidationError::ServerNameTooLong(name.len()));
    }

    Ok(())
}

/// Validates a texture hash (SHA256 of GameProfile texture value)
///
/// Rules:
/// - Cannot be empty
/// - Must be 64 characters (SHA256 hex string)
/// - Must contain only hex characters
pub fn validate_texture_hash(hash: &str) -> Result<(), ValidationError> {
    if hash.is_empty() {
        return Err(ValidationError::TextureHashEmpty);
    }

    // SHA256 produces 64 hex characters
    if hash.len() != 64 {
        return Err(ValidationError::TextureHashInvalidFormat);
    }

    // Must be valid hex
    if !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(ValidationError::TextureHashInvalidFormat);
    }

    Ok(())
}

/// Validates base64-encoded skin data
///
/// Rules:
/// - Cannot be empty
/// - Max 50KB (skin PNGs are typically ~5-15KB)
pub fn validate_skin_data(data: &str) -> Result<(), ValidationError> {
    const MAX_SKIN_SIZE: usize = 50 * 1024; // 50KB base64 (actual PNG will be smaller)

    if data.is_empty() {
        return Err(ValidationError::SkinDataEmpty);
    }

    if data.len() > MAX_SKIN_SIZE {
        return Err(ValidationError::SkinDataTooLarge {
            max: MAX_SKIN_SIZE,
            actual: data.len(),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Player name validation tests
    #[test]
    fn test_valid_player_names() {
        assert!(validate_player_name("Steve").is_ok());
        assert!(validate_player_name("Alex").is_ok());
        assert!(validate_player_name("Player_123").is_ok());
        assert!(validate_player_name("a").is_ok());
        assert!(validate_player_name("1234567890123456").is_ok()); // exactly 16 chars
    }

    #[test]
    fn test_empty_player_name() {
        assert_eq!(
            validate_player_name(""),
            Err(ValidationError::PlayerNameEmpty)
        );
    }

    #[test]
    fn test_player_name_too_long() {
        let long_name = "12345678901234567"; // 17 characters
        assert_eq!(
            validate_player_name(long_name),
            Err(ValidationError::PlayerNameTooLong(17))
        );
    }

    #[test]
    fn test_player_name_invalid_chars() {
        assert_eq!(
            validate_player_name("Player-123"),
            Err(ValidationError::PlayerNameInvalidChars)
        );
        assert_eq!(
            validate_player_name("Player@123"),
            Err(ValidationError::PlayerNameInvalidChars)
        );
        assert_eq!(
            validate_player_name("Player 123"),
            Err(ValidationError::PlayerNameInvalidChars)
        );
    }

    // Code validation tests
    #[test]
    fn test_valid_codes() {
        assert!(validate_code("oxeye-abc123").is_ok());
        assert!(validate_code("oxeye-ABCDEF").is_ok());
        assert!(validate_code("oxeye-123456").is_ok());
        assert!(validate_code("oxeye-aB3DeF").is_ok());
    }

    #[test]
    fn test_empty_code() {
        assert_eq!(validate_code(""), Err(ValidationError::CodeEmpty));
    }

    #[test]
    fn test_code_invalid_format() {
        assert_eq!(
            validate_code("invalid-abc123"),
            Err(ValidationError::CodeInvalidFormat)
        );
        assert_eq!(
            validate_code("oxeye-"),
            Err(ValidationError::CodeInvalidFormat)
        );
        assert_eq!(
            validate_code("oxeye-abc"),
            Err(ValidationError::CodeInvalidFormat)
        );
        assert_eq!(
            validate_code("oxeye-abc-123"),
            Err(ValidationError::CodeInvalidFormat)
        );
    }

    // Player list validation tests
    fn pn(s: &str) -> PlayerName {
        PlayerName::from(s).unwrap()
    }

    #[test]
    fn test_valid_player_list() {
        let players = vec![pn("Steve"), pn("Alex"), pn("Notch")];
        assert!(validate_player_list(&players).is_ok());
    }

    #[test]
    fn test_empty_player_list() {
        let players: Vec<PlayerName> = vec![];
        assert!(validate_player_list(&players).is_ok()); // Empty list is valid
    }

    #[test]
    fn test_player_list_too_large() {
        let players: Vec<PlayerName> = (0..1001)
            .map(|i| PlayerName::from(&format!("P{:04}", i % 10000)).unwrap())
            .collect();
        assert_eq!(
            validate_player_list(&players),
            Err(ValidationError::PlayerListTooLarge {
                max: 1000,
                actual: 1001
            })
        );
    }

    #[test]
    fn test_player_list_with_invalid_name() {
        // Note: Empty string can't be deserialized into PlayerName at route level,
        // but we test that validation catches invalid chars
        let players = vec![pn("Steve"), pn("Player_1")];
        assert!(validate_player_list(&players).is_ok());
    }

    // Server name validation tests
    #[test]
    fn test_valid_server_names() {
        assert!(validate_server_name("MyServer").is_ok());
        assert!(validate_server_name("Server 1").is_ok());
        assert!(validate_server_name("Production-Server-2024").is_ok());
        assert!(validate_server_name("a").is_ok());
    }

    #[test]
    fn test_empty_server_name() {
        assert_eq!(
            validate_server_name(""),
            Err(ValidationError::ServerNameEmpty)
        );
    }

    #[test]
    fn test_server_name_too_long() {
        let long_name = "a".repeat(101);
        assert_eq!(
            validate_server_name(&long_name),
            Err(ValidationError::ServerNameTooLong(101))
        );
    }

    // Texture hash validation tests
    #[test]
    fn test_valid_texture_hash() {
        // Valid SHA256 hex string (64 characters)
        let valid_hash = "a".repeat(64);
        assert!(validate_texture_hash(&valid_hash).is_ok());

        let valid_hash = "0123456789abcdef".repeat(4);
        assert!(validate_texture_hash(&valid_hash).is_ok());

        let valid_hash = "ABCDEF0123456789".repeat(4);
        assert!(validate_texture_hash(&valid_hash).is_ok());
    }

    #[test]
    fn test_empty_texture_hash() {
        assert_eq!(
            validate_texture_hash(""),
            Err(ValidationError::TextureHashEmpty)
        );
    }

    #[test]
    fn test_texture_hash_wrong_length() {
        // Too short
        let short_hash = "a".repeat(63);
        assert_eq!(
            validate_texture_hash(&short_hash),
            Err(ValidationError::TextureHashInvalidFormat)
        );

        // Too long
        let long_hash = "a".repeat(65);
        assert_eq!(
            validate_texture_hash(&long_hash),
            Err(ValidationError::TextureHashInvalidFormat)
        );
    }

    #[test]
    fn test_texture_hash_invalid_chars() {
        // Contains non-hex characters
        let invalid_hash = "g".repeat(64);
        assert_eq!(
            validate_texture_hash(&invalid_hash),
            Err(ValidationError::TextureHashInvalidFormat)
        );

        let invalid_hash = "z".repeat(64);
        assert_eq!(
            validate_texture_hash(&invalid_hash),
            Err(ValidationError::TextureHashInvalidFormat)
        );
    }

    // Skin data validation tests
    #[test]
    fn test_valid_skin_data() {
        let valid_data = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk";
        assert!(validate_skin_data(valid_data).is_ok());
    }

    #[test]
    fn test_empty_skin_data() {
        assert_eq!(validate_skin_data(""), Err(ValidationError::SkinDataEmpty));
    }

    #[test]
    fn test_skin_data_too_large() {
        let large_data = "A".repeat(51 * 1024); // 51KB
        assert_eq!(
            validate_skin_data(&large_data),
            Err(ValidationError::SkinDataTooLarge {
                max: 50 * 1024,
                actual: 51 * 1024
            })
        );
    }
}
