/// Input validation functions for all backend routes
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
pub fn validate_player_list(players: &[String]) -> Result<(), ValidationError> {
    const MAX_PLAYERS: usize = 1000;

    if players.len() > MAX_PLAYERS {
        return Err(ValidationError::PlayerListTooLarge {
            max: MAX_PLAYERS,
            actual: players.len(),
        });
    }

    // Validate each player name
    for player in players {
        validate_player_name(player)?;
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
    #[test]
    fn test_valid_player_list() {
        let players = vec!["Steve".to_string(), "Alex".to_string(), "Notch".to_string()];
        assert!(validate_player_list(&players).is_ok());
    }

    #[test]
    fn test_empty_player_list() {
        let players: Vec<String> = vec![];
        assert!(validate_player_list(&players).is_ok()); // Empty list is valid
    }

    #[test]
    fn test_player_list_too_large() {
        let players: Vec<String> = (0..1001).map(|i| format!("Player{}", i)).collect();
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
        let players = vec![
            "Steve".to_string(),
            "".to_string(), // Invalid: empty
            "Alex".to_string(),
        ];
        assert_eq!(
            validate_player_list(&players),
            Err(ValidationError::PlayerNameEmpty)
        );
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
}
