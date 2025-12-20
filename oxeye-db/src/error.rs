use thiserror::Error;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("database connection error: {0}")]
    Connection(#[from] tokio_rusqlite::Error),

    #[error("pending link not found or expired")]
    PendingLinkNotFound,

    #[error("pending link already used")]
    PendingLinkAlreadyUsed,

    #[error("server not found")]
    ServerNotFound,

    #[error("server name already exists in this guild")]
    ServerNameConflict,

    #[error("invalid api key")]
    InvalidApiKey,
}

pub type Result<T> = std::result::Result<T, DbError>;
