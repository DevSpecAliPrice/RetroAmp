//! Tauri commands for Spotify integration — authentication, settings, and
//! (in later phases) browsing and playback.

use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::audio::spotify::SpotifyPlayer;
use crate::config;

// ---------------------------------------------------------------------------
// Authentication commands
// ---------------------------------------------------------------------------

/// The connection status returned to the frontend.
#[derive(Debug, Clone, Serialize)]
pub struct SpotifyConnectionStatus {
    pub connected: bool,
    pub username: Option<String>,
    pub account_type: Option<String>,
}

/// Log into Spotify via OAuth2 PKCE. Opens the user's browser.
///
/// This is a blocking operation — it waits for the user to complete the
/// browser flow. Tauri runs this on a background thread automatically
/// since it's an async command.
#[tauri::command]
pub async fn spotify_login(
    spotify: State<'_, Arc<SpotifyPlayer>>,
) -> Result<SpotifyConnectionStatus, String> {
    // Use the user's registered client_id, falling back to librespot's default.
    let cfg = config::AppConfig::load();
    let client_id = cfg.spotify.client_id
        .filter(|s| !s.is_empty())
        .unwrap_or_else(crate::spotify::auth::default_client_id);

    // Run the blocking OAuth browser flow on a blocking thread.
    let cid = client_id.clone();
    let login_result = tauri::async_runtime::spawn_blocking(move || {
        crate::spotify::auth::get_oauth_token(&cid)
    })
    .await
    .map_err(|e| format!("OAuth flow failed: {e}"))??;

    // Create a connected librespot Session for audio playback.
    // This must happen in spawn_blocking since Session::new needs Tokio.
    let token_for_session = login_result.access_token.clone();
    let cache_dir = spotify.cache_dir().map(|p| p.to_path_buf());
    let session = tauri::async_runtime::spawn_blocking(move || {
        let cache = cache_dir.as_ref().and_then(|dir| {
            crate::spotify::auth::create_cache(dir)
        });
        let session = librespot::core::session::Session::new(
            librespot::core::config::SessionConfig::default(), cache,
        );
        let credentials = librespot::core::authentication::Credentials::with_access_token(token_for_session);
        let handle = tokio::runtime::Handle::current();
        handle.block_on(async {
            session.connect(credentials, true).await
                .map_err(|e| format!("Playback session connect failed: {e}"))
        })?;
        log::info!("Spotify playback session connected as: {}", session.username());
        Ok::<_, String>(session)
    })
    .await
    .map_err(|e| format!("Session creation failed: {e}"))??;

    spotify.set_playback_session(session);

    // Store the OAuth token — this makes is_connected() return true.
    spotify.set_api_token(crate::audio::spotify::StoredToken {
        access_token: login_result.access_token,
        refresh_token: login_result.refresh_token.clone(),
        expires_at: login_result.expires_at,
    });
    // Save refresh token to disk for auto-reconnect on next launch.
    if let Some(dir) = spotify.cache_dir() {
        crate::spotify::auth::save_refresh_token(dir, &login_result.refresh_token);
    }
    spotify.set_user_info("Spotify User".into(), Some("premium".into()));
    log::info!("Spotify login complete, API token + playback session stored");
    Ok(get_connection_status(&spotify))
}

/// Log out of Spotify. Clears cached credentials.
#[tauri::command]
pub fn spotify_logout(
    spotify: State<'_, Arc<SpotifyPlayer>>,
) -> Result<SpotifyConnectionStatus, String> {
    spotify.disconnect()?;
    Ok(get_connection_status(&spotify))
}

/// Get the current Spotify connection status.
#[tauri::command]
pub fn spotify_status(
    spotify: State<'_, Arc<SpotifyPlayer>>,
) -> SpotifyConnectionStatus {
    get_connection_status(&spotify)
}

fn get_connection_status(player: &SpotifyPlayer) -> SpotifyConnectionStatus {
    SpotifyConnectionStatus {
        connected: player.is_connected(),
        username: player.username(),
        account_type: player.account_type(),
    }
}

// ---------------------------------------------------------------------------
// Settings commands
// ---------------------------------------------------------------------------

/// The Spotify settings returned to and accepted from the frontend.
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct SpotifySettings {
    pub client_id: Option<String>,
    pub quality: String,
    pub device_name: String,
    pub connect_enabled: bool,
    pub normalize_volume: bool,
}

/// Get the current Spotify settings.
#[tauri::command]
pub fn get_spotify_settings() -> SpotifySettings {
    let cfg = config::AppConfig::load();
    SpotifySettings {
        client_id: cfg.spotify.client_id,
        quality: cfg.spotify.quality,
        device_name: cfg.spotify.device_name,
        connect_enabled: cfg.spotify.connect_enabled,
        normalize_volume: cfg.spotify.normalize_volume,
    }
}

/// Update the Spotify settings.
#[tauri::command]
pub fn set_spotify_settings(settings: SpotifySettings) -> Result<(), String> {
    let mut cfg = config::AppConfig::load();
    cfg.spotify.client_id = settings.client_id;
    cfg.spotify.quality = settings.quality;
    cfg.spotify.device_name = settings.device_name;
    cfg.spotify.connect_enabled = settings.connect_enabled;
    cfg.spotify.normalize_volume = settings.normalize_volume;
    cfg.save()
}

// ---------------------------------------------------------------------------
// Playback commands
// ---------------------------------------------------------------------------

/// Play a Spotify track by URI. Adds it to the playlist and plays it.
#[tauri::command]
pub fn spotify_play_track(
    engine: State<'_, Arc<crate::audio::engine::AudioEngine>>,
    playlist: State<'_, Arc<std::sync::Mutex<crate::playlist::manager::PlaylistManager>>>,
    spotify: State<'_, Arc<SpotifyPlayer>>,
    uri: String,
    name: String,
    artist: String,
    album: String,
    duration_ms: u64,
) -> Result<(), String> {
    let mut pl = playlist.lock().map_err(|e| e.to_string())?;
    let id = pl.add_track(&uri);

    // Pre-load metadata from the Web API data we already have.
    pl.update_metadata(id, &crate::audio::source::TrackMetadata {
        title: Some(name),
        artist: Some(artist),
        album: Some(album),
        duration: Some(std::time::Duration::from_millis(duration_ms)),
        sample_rate: 44100,
        channels: 2,
        bitrate: Some(320),
        genre: None,
        year: None,
        track_number: None,
        cover_art: None,
    });

    pl.play_track(id);
    let path = pl.current_track().map(|t| t.path.clone())
        .ok_or("track not found after adding")?;
    drop(pl);

    crate::commands::play_path(&engine, &path, Some(&spotify))?;
    Ok(())
}

/// Add a Spotify track to the playlist without playing it.
#[tauri::command]
pub fn spotify_add_to_playlist(
    playlist: State<'_, Arc<std::sync::Mutex<crate::playlist::manager::PlaylistManager>>>,
    uri: String,
    name: String,
    artist: String,
    album: String,
    duration_ms: u64,
) -> Result<(), String> {
    let mut pl = playlist.lock().map_err(|e| e.to_string())?;
    let id = pl.add_track(&uri);
    pl.update_metadata(id, &crate::audio::source::TrackMetadata {
        title: Some(name),
        artist: Some(artist),
        album: Some(album),
        duration: Some(std::time::Duration::from_millis(duration_ms)),
        sample_rate: 44100,
        channels: 2,
        bitrate: Some(320),
        genre: None,
        year: None,
        track_number: None,
        cover_art: None,
    });
    Ok(())
}

// ---------------------------------------------------------------------------
// Browsing commands — all use spawn_blocking since ureq is synchronous
// ---------------------------------------------------------------------------

use crate::spotify::types::*;

/// Search Spotify for tracks, albums, artists, and/or playlists.
#[tauri::command]
pub async fn spotify_search(
    spotify: State<'_, Arc<SpotifyPlayer>>,
    query: String,
    types: String,
    limit: usize,
) -> Result<SearchResults, String> {
    let token = spotify.api_access_token().ok_or("Spotify not connected")?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::spotify::api::search(&token, &query, &types, limit)
    })
    .await
    .map_err(|e| format!("Search task failed: {e}"))?
}

/// Get the current user's playlists.
#[tauri::command]
pub async fn spotify_get_playlists(
    spotify: State<'_, Arc<SpotifyPlayer>>,
    limit: usize,
    offset: usize,
) -> Result<Paged<ApiPlaylist>, String> {
    let token = spotify.api_access_token().ok_or("Spotify not connected")?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::spotify::api::get_user_playlists(&token, limit, offset)
    })
    .await
    .map_err(|e| format!("Playlists task failed: {e}"))?
}

/// Get tracks from a playlist.
#[tauri::command]
pub async fn spotify_get_playlist_items(
    spotify: State<'_, Arc<SpotifyPlayer>>,
    playlist_id: String,
    limit: usize,
    offset: usize,
) -> Result<Paged<PlaylistTrackItem>, String> {
    let token = spotify.api_access_token().ok_or("Spotify not connected")?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::spotify::api::get_playlist_items(&token, &playlist_id, limit, offset)
    })
    .await
    .map_err(|e| format!("Playlist tracks task failed: {e}"))?
}

/// Get the user's saved albums.
#[tauri::command]
pub async fn spotify_get_saved_albums(
    spotify: State<'_, Arc<SpotifyPlayer>>,
    limit: usize,
    offset: usize,
) -> Result<Paged<SavedAlbum>, String> {
    let token = spotify.api_access_token().ok_or("Spotify not connected")?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::spotify::api::get_saved_albums(&token, limit, offset)
    })
    .await
    .map_err(|e| format!("Saved albums task failed: {e}"))?
}

/// Get the user's saved (liked) tracks.
#[tauri::command]
pub async fn spotify_get_saved_tracks(
    spotify: State<'_, Arc<SpotifyPlayer>>,
    limit: usize,
    offset: usize,
) -> Result<Paged<SavedTrack>, String> {
    let token = spotify.api_access_token().ok_or("Spotify not connected")?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::spotify::api::get_saved_tracks(&token, limit, offset)
    })
    .await
    .map_err(|e| format!("Saved tracks task failed: {e}"))?
}

/// Get a full album with its track listing.
#[tauri::command]
pub async fn spotify_get_album(
    spotify: State<'_, Arc<SpotifyPlayer>>,
    album_id: String,
) -> Result<ApiAlbum, String> {
    let token = spotify.api_access_token().ok_or("Spotify not connected")?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::spotify::api::get_album(&token, &album_id)
    })
    .await
    .map_err(|e| format!("Album task failed: {e}"))?
}

/// Get a full artist profile.
#[tauri::command]
pub async fn spotify_get_artist(
    spotify: State<'_, Arc<SpotifyPlayer>>,
    artist_id: String,
) -> Result<ApiArtist, String> {
    let token = spotify.api_access_token().ok_or("Spotify not connected")?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::spotify::api::get_artist(&token, &artist_id)
    })
    .await
    .map_err(|e| format!("Artist task failed: {e}"))?
}

/// Get an artist's albums.
#[tauri::command]
pub async fn spotify_get_artist_albums(
    spotify: State<'_, Arc<SpotifyPlayer>>,
    artist_id: String,
    limit: usize,
    offset: usize,
) -> Result<Paged<ApiAlbumRef>, String> {
    let token = spotify.api_access_token().ok_or("Spotify not connected")?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::spotify::api::get_artist_albums(&token, &artist_id, limit, offset)
    })
    .await
    .map_err(|e| format!("Artist albums task failed: {e}"))?
}

/// Get the user's recently played tracks.
#[tauri::command]
pub async fn spotify_get_recently_played(
    spotify: State<'_, Arc<SpotifyPlayer>>,
    limit: usize,
) -> Result<CursorPaged<RecentlyPlayedItem>, String> {
    let token = spotify.api_access_token().ok_or("Spotify not connected")?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::spotify::api::get_recently_played(&token, limit)
    })
    .await
    .map_err(|e| format!("Recently played task failed: {e}"))?
}
