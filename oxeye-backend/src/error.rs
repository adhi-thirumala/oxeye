use axum::{
  Json,
  http::StatusCode,
  response::{IntoResponse, Response},
};
use serde::Serialize;

/// API error response structure
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
  pub error: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub details: Option<String>,
}

impl ErrorResponse {
  pub fn new(error: impl Into<String>) -> Self {
    Self {
      error: error.into(),
      details: None,
    }
  }

  pub fn with_details(error: impl Into<String>, details: impl Into<String>) -> Self {
    Self {
      error: error.into(),
      details: Some(details.into()),
    }
  }
}

/// Application error type
#[derive(Debug)]
pub enum AppError {
  DatabaseError(oxeye_db::DbError),
  ValidationError(String),
}

impl IntoResponse for AppError {
  fn into_response(self) -> Response {
    match self {
      AppError::DatabaseError(db_err) => {
        // Log the detailed error server-side
        tracing::error!(?db_err, "Database error occurred");

        // Return user-friendly error to client
        let (status, message) = match db_err {
          oxeye_db::DbError::PendingLinkNotFound => (
            StatusCode::NOT_FOUND,
            "Connection code not found or expired",
          ),
          oxeye_db::DbError::PendingLinkAlreadyUsed => (
            StatusCode::CONFLICT,
            "Connection code has already been used",
          ),
          oxeye_db::DbError::ServerNameConflict => (
            StatusCode::CONFLICT,
            "A server with this name already exists",
          ),
          oxeye_db::DbError::InvalidApiKey => {
            (StatusCode::UNAUTHORIZED, "Invalid or expired API key")
          }
          oxeye_db::DbError::ServerNotFound => (StatusCode::NOT_FOUND, "Server not found"),
          oxeye_db::DbError::Sqlite(_) | oxeye_db::DbError::Connection(_) => {
            // Don't expose internal database errors
            tracing::error!("Internal database error: {:?}", db_err);
            (
              StatusCode::INTERNAL_SERVER_ERROR,
              "An internal error occurred. Please try again later.",
            )
          }
        };

        let error_response = ErrorResponse::new(message);
        (status, Json(error_response)).into_response()
      }
      AppError::ValidationError(msg) => {
        tracing::warn!(validation_error = %msg, "Validation failed");
        let error_response = ErrorResponse::new(msg);
        (StatusCode::BAD_REQUEST, Json(error_response)).into_response()
      }
    }
  }
}

impl From<oxeye_db::DbError> for AppError {
  fn from(err: oxeye_db::DbError) -> Self {
    AppError::DatabaseError(err)
  }
}

impl From<crate::validation::ValidationError> for AppError {
  fn from(err: crate::validation::ValidationError) -> Self {
    AppError::ValidationError(err.to_string())
  }
}
