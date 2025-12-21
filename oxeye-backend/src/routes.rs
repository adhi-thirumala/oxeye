use crate::helpers::now;
use crate::AppState;

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use axum_extra::TypedHeader;
use axum_macros::debug_handler;
use headers::authorization::Bearer;
use headers::Authorization;
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
) -> Result<impl IntoResponse, StatusCode> {
    let pending_link = state
        .db
        .consume_pending_link(payload.code, now())
        .await
        .map_err(|e| match e {
            oxeye_db::DbError::PendingLinkNotFound => StatusCode::NOT_FOUND,
            oxeye_db::DbError::PendingLinkAlreadyUsed => StatusCode::CONFLICT,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        })?;
    let api_key = crate::helpers::generate_api_key();
    let api_key_hash = crate::helpers::hash_api_key(&api_key);
    state
        .db
        .create_server(
            api_key_hash,
            pending_link.server_name,
            pending_link.guild_id,
        )
        .await
        .map_err(|e| match e {
            oxeye_db::DbError::ServerNameConflict => StatusCode::CONFLICT,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        })?;
    Ok((StatusCode::OK, Json(ConnResponse { api_key })))
}

#[debug_handler]
pub(crate) async fn join(
    State(state): State<Arc<AppState>>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
    Json(payload): Json<TransitionRequest>,
) -> StatusCode {
    let api_key = auth.token().to_string();
    let api_key_hash = crate::helpers::hash_api_key(&api_key);
    match state
        .db
        .player_join(api_key_hash, payload.player, now())
        .await
    {
        Ok(_) => StatusCode::OK,
        Err(e) => match e {
            oxeye_db::DbError::InvalidApiKey => StatusCode::UNAUTHORIZED,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        },
    }
}

pub(crate) async fn leave(
    State(state): State<Arc<AppState>>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
    Json(payload): Json<TransitionRequest>,
) -> StatusCode {
    let api_key = auth.token().to_string();
    let api_key_hash = crate::helpers::hash_api_key(&api_key);
    match state
        .db
        .player_leave(api_key_hash, payload.player)
        .await
    {
        Ok(_) => StatusCode::OK,
        Err(e) => match e {
            oxeye_db::DbError::InvalidApiKey => StatusCode::UNAUTHORIZED,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        },
    }
}


pub(crate) async fn sync(
    State(state): State<Arc<AppState>>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
    Json(payload): Json<SyncRequest>,
) -> StatusCode {
    let api_key = auth.token().to_string();
    let api_key_hash = crate::helpers::hash_api_key(&api_key);
    match state
        .db
        .sync_players(api_key_hash, payload.players, now())
        .await
    {
        Ok(_) => StatusCode::OK,
        Err(e) => match e {
            oxeye_db::DbError::InvalidApiKey => StatusCode::UNAUTHORIZED,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        },
    }
}
