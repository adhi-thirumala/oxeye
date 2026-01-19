use crate::AppState;
use crate::error::AppError;
use crate::helpers::now;
use crate::render::{self, CompositeConfig, DEFAULT_STEVE_HEAD, PlayerEntry};
use crate::validation;

use axum::{
    Json,
    body::Body,
    extract::{Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use axum_extra::TypedHeader;
use axum_macros::debug_handler;
use base64::Engine;
use headers::Authorization;
use headers::authorization::Bearer;
use oxeye_db::PlayerName;
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

/// Join request - player name with optional skin texture hash.
/// If texture_hash is provided and we don't have the skin, returns 202.
#[derive(Deserialize)]
pub(crate) struct JoinRequest {
    player: PlayerName,
    /// SHA256 hash of the GameProfile texture value (optional for backward compat)
    #[serde(default)]
    texture_hash: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct LeaveRequest {
    player: PlayerName,
}

#[derive(Deserialize)]
pub(crate) struct SyncRequest {
    players: Vec<PlayerName>,
}

/// Skin upload request - sent when backend returns 202 from /join.
#[derive(Deserialize)]
pub(crate) struct SkinRequest {
    /// Player name who owns this skin
    player: PlayerName,
    /// SHA256 hash of the GameProfile texture value
    texture_hash: String,
    /// Base64-encoded PNG skin data
    skin_data: String,
    /// Optional texture URL for reference
    #[serde(default)]
    texture_url: Option<String>,
}

#[debug_handler]
pub(crate) async fn connect(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ConnRequest>,
) -> Result<impl IntoResponse, AppError> {
    #[cfg(debug_assertions)]
    tracing::debug!(?payload.code, "connect request");

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
    Json(payload): Json<JoinRequest>,
) -> Result<impl IntoResponse, AppError> {
    #[cfg(debug_assertions)]
    tracing::debug!(player = %payload.player, texture_hash = ?payload.texture_hash, "join request");

    // Validate player name
    validation::validate_player_name(payload.player.as_str())?;

    // Validate texture hash if provided
    if let Some(ref hash) = payload.texture_hash {
        validation::validate_texture_hash(hash)?;
    }

    let api_key = auth.token().to_string();
    let api_key_hash = crate::helpers::hash_api_key(&api_key);
    let api_key_hash_clone = api_key_hash.clone();

    // Record the player join
    state
        .db
        .player_join(api_key_hash.clone(), payload.player, now())
        .await?;

    // Check if we need the skin data
    let need_skin = if let Some(ref texture_hash) = payload.texture_hash {
        // Check if we already have this skin
        let exists = state.db.skin_exists(texture_hash).await?;

        if exists {
            // Only update player's skin mapping if the skin already exists in the database
            // (FK constraint: player_skins.texture_hash references skins.texture_hash)
            state
                .db
                .update_player_skin(payload.player.as_str(), texture_hash, now())
                .await?;
        }

        !exists
    } else {
        false
    };

    // Spawn async task to regenerate composite image
    let db = state.db.clone();
    tokio::spawn(async move {
        if let Err(e) = regenerate_status_composite(&db, &api_key_hash_clone).await {
            tracing::error!(?e, "failed to regenerate status composite");
        }
    });

    if need_skin {
        // Return 202 to signal mod should send skin data
        Ok(StatusCode::ACCEPTED)
    } else {
        Ok(StatusCode::OK)
    }
}

pub(crate) async fn leave(
    State(state): State<Arc<AppState>>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
    Json(payload): Json<LeaveRequest>,
) -> Result<impl IntoResponse, AppError> {
    #[cfg(debug_assertions)]
    tracing::debug!(player = %payload.player, "leave request");

    // Validate player name
    validation::validate_player_name(payload.player.as_str())?;

    let api_key = auth.token().to_string();
    let api_key_hash = crate::helpers::hash_api_key(&api_key);
    let api_key_hash_clone = api_key_hash.clone();

    state.db.player_leave(api_key_hash, payload.player).await?;

    // Spawn async task to regenerate composite image
    let db = state.db.clone();
    tokio::spawn(async move {
        if let Err(e) = regenerate_status_composite(&db, &api_key_hash_clone).await {
            tracing::error!(?e, "failed to regenerate status composite");
        }
    });

    Ok(StatusCode::OK)
}

pub(crate) async fn sync(
    State(state): State<Arc<AppState>>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
    Json(payload): Json<SyncRequest>,
) -> Result<impl IntoResponse, AppError> {
    #[cfg(debug_assertions)]
    tracing::debug!(players = ?payload.players, count = payload.players.len(), "sync request");

    // Validate player list (already deserialized into Vec<PlayerName>, check size and content)
    validation::validate_player_list(&payload.players)?;

    let api_key = auth.token().to_string();
    let api_key_hash = crate::helpers::hash_api_key(&api_key);
    let api_key_hash_clone = api_key_hash.clone();

    state
        .db
        .sync_players(api_key_hash, payload.players, now())
        .await?;

    // Spawn async task to regenerate composite image
    let db = state.db.clone();
    tokio::spawn(async move {
        if let Err(e) = regenerate_status_composite(&db, &api_key_hash_clone).await {
            tracing::error!(?e, "failed to regenerate status composite");
        }
    });

    Ok(StatusCode::OK)
}

#[debug_handler]
pub(crate) async fn disconnect(
    State(state): State<Arc<AppState>>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
) -> Result<impl IntoResponse, AppError> {
    #[cfg(debug_assertions)]
    tracing::debug!("disconnect request");

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
    #[cfg(debug_assertions)]
    tracing::debug!("status request");

    let api_key = auth.token().to_string();
    let api_key_hash = crate::helpers::hash_api_key(&api_key);

    // Check if server exists with this API key
    let server = state.db.get_server_by_api_key(api_key_hash).await?;

    match server {
        Some(_) => Ok(StatusCode::OK),
        None => Err(AppError::DatabaseError(oxeye_db::DbError::InvalidApiKey)),
    }
}

// ============================================================================
// Skin Endpoints
// ============================================================================

/// Receive skin data after a 202 response from /join.
#[debug_handler]
pub(crate) async fn upload_skin(
    State(state): State<Arc<AppState>>,
    TypedHeader(auth): TypedHeader<Authorization<Bearer>>,
    Json(payload): Json<SkinRequest>,
) -> Result<impl IntoResponse, AppError> {
    #[cfg(debug_assertions)]
    tracing::debug!(texture_hash = %payload.texture_hash, "skin upload request");

    // Validate texture hash
    validation::validate_texture_hash(&payload.texture_hash)?;

    // Validate skin data (base64)
    validation::validate_skin_data(&payload.skin_data)?;

    // Verify server exists
    let api_key = auth.token().to_string();
    let api_key_hash = crate::helpers::hash_api_key(&api_key);
    let server = state.db.get_server_by_api_key(api_key_hash.clone()).await?;
    if server.is_none() {
        return Err(AppError::DatabaseError(oxeye_db::DbError::InvalidApiKey));
    }

    // Decode skin data
    let skin_data = base64::engine::general_purpose::STANDARD
        .decode(&payload.skin_data)
        .map_err(|e| AppError::ValidationError(format!("invalid base64 skin data: {}", e)))?;

    // Store the skin
    state
        .db
        .store_skin(
            payload.texture_hash.clone(),
            payload.texture_url.clone(),
            skin_data.clone(),
        )
        .await?;

    // Now that the skin exists, update the player's skin mapping
    // (This was deferred from /join because the FK constraint requires the skin to exist first)
    state
        .db
        .update_player_skin(payload.player.as_str(), &payload.texture_hash, now())
        .await?;

    // Spawn async task to render head
    let db = state.db.clone();
    let texture_hash = payload.texture_hash.clone();
    tokio::spawn(async move {
        match render::render_head(&skin_data) {
            Ok(head_data) => {
                if let Err(e) = db
                    .store_rendered_head(texture_hash.clone(), head_data, now())
                    .await
                {
                    tracing::error!(?e, "failed to store rendered head");
                }

                // Also regenerate composite for the server
                if let Err(e) = regenerate_status_composite(&db, &api_key_hash).await {
                    tracing::error!(
                        ?e,
                        "failed to regenerate status composite after skin upload"
                    );
                }
            }
            Err(e) => {
                tracing::error!(?e, texture_hash, "failed to render head from skin");
            }
        }
    });

    Ok(StatusCode::OK)
}

/// Serve a rendered player head image by texture hash.
/// Falls back to Steve head if not found.
pub(crate) async fn get_head(
    State(state): State<Arc<AppState>>,
    Path(hash_with_ext): Path<String>,
) -> Response {
    // Strip .png extension if present
    let texture_hash = hash_with_ext.strip_suffix(".png").unwrap_or(&hash_with_ext);

    // Try to get rendered head from database
    let head_data = match state.db.get_rendered_head(texture_hash).await {
        Ok(Some(data)) => data,
        Ok(None) => {
            // Not found, return Steve head
            tracing::debug!(texture_hash, "head not found, returning Steve fallback");
            DEFAULT_STEVE_HEAD.to_vec()
        }
        Err(e) => {
            tracing::error!(?e, "failed to get rendered head");
            DEFAULT_STEVE_HEAD.to_vec()
        }
    };

    // Return PNG with immutable cache headers (1 year)
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "image/png")
        .header(header::CACHE_CONTROL, "public, immutable, max-age=31536000")
        .body(Body::from(head_data))
        .unwrap()
}

/// Serve a cached status composite image for a server.
pub(crate) async fn get_status_image(
    State(state): State<Arc<AppState>>,
    Path(hash_with_ext): Path<String>,
) -> Response {
    // Strip .png extension if present
    let api_key_hash = hash_with_ext.strip_suffix(".png").unwrap_or(&hash_with_ext);
    tracing::info!(api_key_hash, "status image requested");

    // Try to get cached status image
    let image_data = match state.db.get_status_image(api_key_hash).await {
        Ok(Some(data)) => {
            tracing::info!(
                api_key_hash,
                bytes = data.len(),
                "returning cached status image"
            );
            data
        }
        Ok(None) => {
            // Not cached yet, generate on-demand
            tracing::info!(
                api_key_hash,
                "status image not cached, generating on-demand"
            );
            match generate_status_composite(&state.db, api_key_hash).await {
                Ok(data) => data,
                Err(e) => {
                    tracing::error!(?e, "failed to generate status image");
                    // Return empty state image
                    let config = CompositeConfig::default();
                    render::render_composite(&[], &config).unwrap_or_default()
                }
            }
        }
        Err(e) => {
            tracing::error!(?e, "failed to get status image");
            let config = CompositeConfig::default();
            render::render_composite(&[], &config).unwrap_or_default()
        }
    };

    // Return PNG with short cache (can change when players join/leave)
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "image/png")
        .header(header::CACHE_CONTROL, "public, max-age=10")
        .body(Body::from(image_data))
        .unwrap()
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Generate a status composite image for a server.
async fn generate_status_composite(
    db: &oxeye_db::Database,
    api_key_hash: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    // Get players with their texture hashes
    let players = db.get_players_with_heads(api_key_hash).await?;
    tracing::info!(
        api_key_hash,
        player_count = players.len(),
        players = ?players.iter().map(|(name, hash)| (name.as_str(), hash.as_ref().map(|h| &h[..8]))).collect::<Vec<_>>(),
        "generating status composite"
    );

    // Build player entries with head data
    let mut entries = Vec::with_capacity(players.len());
    for (player_name, texture_hash) in players {
        let head_data = if let Some(ref hash) = texture_hash {
            let data = db.get_rendered_head(hash).await.ok().flatten();
            tracing::debug!(
                player = %player_name,
                texture_hash = &hash[..8],
                has_head = data.is_some(),
                "fetched head data"
            );
            data
        } else {
            tracing::debug!(player = %player_name, "no texture hash, using fallback");
            None
        };

        entries.push(PlayerEntry {
            name: player_name.to_string(),
            head_data,
        });
    }

    // Render composite
    let config = CompositeConfig::default();
    tracing::info!(entry_count = entries.len(), "rendering composite image");
    let image_data = render::render_composite(&entries, &config)?;
    tracing::info!(bytes = image_data.len(), "composite image rendered");

    Ok(image_data)
}

/// Regenerate and cache the status composite image for a server.
async fn regenerate_status_composite(
    db: &oxeye_db::Database,
    api_key_hash: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing::info!(api_key_hash, "regenerating status composite");
    let image_data = generate_status_composite(db, api_key_hash).await?;

    db.store_status_image(api_key_hash.to_string(), image_data, now())
        .await?;

    tracing::info!(api_key_hash, "status composite cached");
    Ok(())
}
