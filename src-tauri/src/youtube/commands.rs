//! Tauri commands for YouTube Music integration — search, browse, and playback.
//!
//! No authentication required — YouTube Music search and browse work anonymously.

use std::sync::Arc;

use tauri::State;

use super::types::*;

// ---------------------------------------------------------------------------
// Playback commands
// ---------------------------------------------------------------------------

/// Play a YouTube track by video ID. Adds it to the playlist and plays it.
#[tauri::command]
pub fn youtube_play_track(
    engine: State<'_, Arc<crate::audio::engine::AudioEngine>>,
    playlist: State<'_, Arc<std::sync::Mutex<crate::playlist::manager::PlaylistManager>>>,
    video_id: String,
    title: String,
    artist: String,
    album: String,
    duration_ms: u64,
) -> Result<(), String> {
    let uri = format!("youtube:{video_id}");
    let mut pl = playlist.lock().map_err(|e| e.to_string())?;
    let id = pl.add_track(&uri);

    pl.update_metadata(
        id,
        &crate::audio::source::TrackMetadata {
            title: Some(title),
            artist: Some(artist),
            album: Some(album),
            duration: Some(std::time::Duration::from_millis(duration_ms)),
            sample_rate: 44100,
            channels: 2,
            bitrate: None,
            genre: None,
            year: None,
            track_number: None,
            cover_art: None,
        },
    );

    pl.play_track(id);
    let track = pl
        .current_track()
        .ok_or("track not found after adding")?;
    let path = track.path.clone();
    let meta = track.to_source_metadata();
    drop(pl);

    crate::commands::play_path(&engine, &path, None, Some(meta))?;
    Ok(())
}

/// Add a YouTube track to the playlist without playing it.
#[tauri::command]
pub fn youtube_add_to_playlist(
    playlist: State<'_, Arc<std::sync::Mutex<crate::playlist::manager::PlaylistManager>>>,
    video_id: String,
    title: String,
    artist: String,
    album: String,
    duration_ms: u64,
) -> Result<(), String> {
    let uri = format!("youtube:{video_id}");
    let mut pl = playlist.lock().map_err(|e| e.to_string())?;
    let id = pl.add_track(&uri);
    pl.update_metadata(
        id,
        &crate::audio::source::TrackMetadata {
            title: Some(title),
            artist: Some(artist),
            album: Some(album),
            duration: Some(std::time::Duration::from_millis(duration_ms)),
            sample_rate: 44100,
            channels: 2,
            bitrate: None,
            genre: None,
            year: None,
            track_number: None,
            cover_art: None,
        },
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Search commands
// ---------------------------------------------------------------------------

/// Search YouTube Music for songs, albums, artists, and playlists.
#[tauri::command]
pub async fn youtube_search(query: String) -> Result<YtSearchResults, String> {
    log::info!("[youtube] search command called: {query:?}");
    let result = crate::youtube::api::search(&query).await;
    match &result {
        Ok(r) => log::info!(
            "[youtube] search returned {} tracks, {} albums, {} artists, {} playlists",
            r.tracks.len(), r.albums.len(), r.artists.len(), r.playlists.len(),
        ),
        Err(e) => log::error!("[youtube] search failed: {e}"),
    }
    result
}

/// Search YouTube Music filtered to songs only.
#[tauri::command]
pub async fn youtube_search_songs(query: String) -> Result<Vec<YtTrack>, String> {
    crate::youtube::api::search_songs(&query).await
}

/// Get search suggestions for autocomplete.
#[tauri::command]
pub async fn youtube_search_suggestions(query: String) -> Result<Vec<String>, String> {
    crate::youtube::api::get_search_suggestions(&query).await
}

// ---------------------------------------------------------------------------
// Browse commands
// ---------------------------------------------------------------------------

/// Get a full album with track listing.
#[tauri::command]
pub async fn youtube_get_album(browse_id: String) -> Result<YtAlbum, String> {
    crate::youtube::api::get_album(&browse_id).await
}

/// Get an artist's page with albums and singles.
#[tauri::command]
pub async fn youtube_get_artist(browse_id: String) -> Result<YtArtist, String> {
    crate::youtube::api::get_artist(&browse_id).await
}

/// Get a playlist with its tracks.
#[tauri::command]
pub async fn youtube_get_playlist(browse_id: String) -> Result<YtPlaylistDetail, String> {
    crate::youtube::api::get_playlist(&browse_id).await
}

// ---------------------------------------------------------------------------
// Settings commands
// ---------------------------------------------------------------------------

/// YouTube settings returned to and accepted from the frontend.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct YouTubeSettings {
    pub quality: String,
    pub has_cookie: bool,
    pub ytdlp_path: Option<String>,
    pub ytdlp_status: String,
}

/// Get the current YouTube settings.
#[tauri::command]
pub fn get_youtube_settings() -> YouTubeSettings {
    let cfg = crate::config::AppConfig::load();
    let ytdlp_path = crate::youtube::ytdlp::find();
    let ytdlp_status = match &ytdlp_path {
        Some(p) if p.to_str() == Some("yt-dlp") => "System PATH".to_string(),
        Some(p) => format!("Installed: {}", p.display()),
        None => "Not installed".to_string(),
    };
    YouTubeSettings {
        quality: cfg.youtube.quality,
        has_cookie: cfg.youtube.cookie.is_some(),
        ytdlp_path: ytdlp_path.map(|p| p.to_string_lossy().to_string()),
        ytdlp_status,
    }
}

/// Update YouTube settings (quality, yt-dlp path override).
#[tauri::command]
pub fn set_youtube_settings(quality: String, ytdlp_path: Option<String>) -> Result<(), String> {
    let mut cfg = crate::config::AppConfig::load();
    cfg.youtube.quality = quality;
    cfg.youtube.ytdlp_path = ytdlp_path;
    cfg.save()
}

/// Save a YouTube Music cookie for authenticated access.
/// Also immediately reinitializes the API client with the new cookie.
#[tauri::command]
pub async fn youtube_save_cookie(cookie: String) -> Result<YouTubeAuthStatus, String> {
    // Try to login first to validate the cookie.
    crate::youtube::api::login_with_cookie(&cookie).await?;

    // If login succeeded, persist the cookie.
    let mut cfg = crate::config::AppConfig::load();
    cfg.youtube.cookie = Some(cookie);
    cfg.save().map_err(|e| format!("Failed to save cookie: {e}"))?;

    log::info!("[youtube] cookie saved to config");
    Ok(YouTubeAuthStatus { authenticated: true })
}

/// Clear the saved YouTube Music cookie and revert to anonymous mode.
#[tauri::command]
pub async fn youtube_clear_cookie() -> Result<YouTubeAuthStatus, String> {
    crate::youtube::api::logout().await?;

    let mut cfg = crate::config::AppConfig::load();
    cfg.youtube.cookie = None;
    cfg.save().map_err(|e| format!("Failed to save config: {e}"))?;

    log::info!("[youtube] cookie cleared from config");
    Ok(YouTubeAuthStatus { authenticated: false })
}

// ---------------------------------------------------------------------------
// Auth commands (optional — for accessing personal library)
// ---------------------------------------------------------------------------

/// YouTube Music auth status returned to the frontend.
#[derive(Debug, Clone, serde::Serialize)]
pub struct YouTubeAuthStatus {
    pub authenticated: bool,
}

/// Check whether the user is logged in to YouTube Music.
#[tauri::command]
pub async fn youtube_auth_status() -> YouTubeAuthStatus {
    YouTubeAuthStatus {
        authenticated: crate::youtube::api::is_authenticated().await,
    }
}

/// Log in to YouTube Music using browser cookies.
/// The cookie string should be the raw `Cookie` header value from a
/// logged-in browser session on music.youtube.com.
#[tauri::command]
pub async fn youtube_login(cookie: String) -> Result<YouTubeAuthStatus, String> {
    crate::youtube::api::login_with_cookie(&cookie).await?;
    Ok(YouTubeAuthStatus { authenticated: true })
}

/// Log out of YouTube Music (revert to unauthenticated mode).
#[tauri::command]
pub async fn youtube_logout() -> Result<YouTubeAuthStatus, String> {
    crate::youtube::api::logout().await?;
    Ok(YouTubeAuthStatus { authenticated: false })
}
