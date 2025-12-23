use crate::AppState;
use crate::error::AppError;
use crate::helpers::now;
use crate::validation;

use axum::{Json, extract::State, http::StatusCode, response::IntoResponse};
use axum_extra::TypedHeader;
use axum_macros::debug_handler;
use headers::Authorization;
use headers::authorization::Bearer;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Deserialize)]
pub(crate) struct ConnRequest {
  code: String,
}

#[derive(Serialize)]
pub(crate) struct ConnResponse {
  api_key: String,
}

#[derive(Deserialize)]
pub(crate) struct TransitionRequest {
  player: String,
}

#[derive(Deserialize)]
pub(crate) struct SyncRequest {
  players: Vec<String>,
}

#[debug_handler]
pub(crate) async fn connect(
  State(state): State<Arc<AppState>>,
  Json(payload): Json<ConnRequest>,
) -> Result<impl IntoResponse, AppError> {
  // Validate code format
  validation::validate_code(&payload.code)?;

  let pending_link = state.db.consume_pending_link(payload.code, now()).await?;

  let api_key = crate::helpers::generate_api_key();
  let api_key_hash = crate::helpers::hash_api_key(&api_key);

  state
    .db
    .create_server(
      api_key_hash,
      pending_link.server_name,
      pending_link.guild_id,
    )
    .await?;

  Ok((StatusCode::CREATED, Json(ConnResponse { api_key })))
}

#[debug_handler]
pub(crate) async fn join(
  State(state): State<Arc<AppState>>,
  TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
  Json(payload): Json<TransitionRequest>,
) -> Result<impl IntoResponse, AppError> {
  // Validate player name
  validation::validate_player_name(&payload.player)?;

  let api_key = auth.token().to_string();
  let api_key_hash = crate::helpers::hash_api_key(&api_key);

  state
    .db
    .player_join(api_key_hash, payload.player, now())
    .await?;

  Ok(StatusCode::OK)
}

pub(crate) async fn leave(
  State(state): State<Arc<AppState>>,
  TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
  Json(payload): Json<TransitionRequest>,
) -> Result<impl IntoResponse, AppError> {
  // Validate player name
  validation::validate_player_name(&payload.player)?;

  let api_key = auth.token().to_string();
  let api_key_hash = crate::helpers::hash_api_key(&api_key);

  state.db.player_leave(api_key_hash, payload.player).await?;

  Ok(StatusCode::OK)
}

pub(crate) async fn sync(
  State(state): State<Arc<AppState>>,
  TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
  Json(payload): Json<SyncRequest>,
) -> Result<impl IntoResponse, AppError> {
  // Validate player list (size and individual names)
  validation::validate_player_list(&payload.players)?;

  let api_key = auth.token().to_string();
  let api_key_hash = crate::helpers::hash_api_key(&api_key);

  state
    .db
    .sync_players(api_key_hash, payload.players, now())
    .await?;

  Ok(StatusCode::OK)
}

#[debug_handler]
pub(crate) async fn disconnect(
  State(state): State<Arc<AppState>>,
  TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
) -> Result<impl IntoResponse, AppError> {
  let api_key = auth.token().to_string();
  let api_key_hash = crate::helpers::hash_api_key(&api_key);

  state.db.delete_server_by_api_key(api_key_hash).await?;

  Ok(StatusCode::OK)
}

#[debug_handler]
pub(crate) async fn status(
  State(state): State<Arc<AppState>>,
  TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
) -> Result<impl IntoResponse, AppError> {
  let api_key = auth.token().to_string();
  let api_key_hash = crate::helpers::hash_api_key(&api_key);

  // Check if server exists with this API key
  let server = state.db.get_server_by_api_key(api_key_hash).await?;

  match server {
    Some(_) => Ok(StatusCode::OK),
    None => Err(AppError::DatabaseError(oxeye_db::DbError::InvalidApiKey)),
  }
}
