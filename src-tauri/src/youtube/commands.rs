//! Tauri commands for YouTube Music integration — search, browse, and playback.
//!
//! No authentication required — YouTube Music search and browse work anonymously.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Emitter, Manager, State};

use super::types::*;

/// Shared state for the pre-created YouTube helper WebViews
/// (`youtube_login`, `youtube_cookie_refresh`).
///
/// `login_extracted` is reset to `false` before each new login attempt so the
/// page-load handler will extract cookies on the first music.youtube.com load
/// after the user signs in, and ignore subsequent loads on the same session.
pub struct YouTubeWebviewState {
    pub login_extracted: Arc<AtomicBool>,
}

impl Default for YouTubeWebviewState {
    fn default() -> Self {
        Self { login_extracted: Arc::new(AtomicBool::new(false)) }
    }
}

// ---------------------------------------------------------------------------
// Playback commands
// ---------------------------------------------------------------------------

/// Convert an incoming `duration_ms` from the frontend into an `Option<Duration>`.
/// A value of 0 means "unknown" (the frontend uses `?? 0` when it doesn't have
/// a duration), so we map it to None rather than displaying it as "0:00".
fn duration_from_ms(duration_ms: u64) -> Option<std::time::Duration> {
    if duration_ms == 0 {
        None
    } else {
        Some(std::time::Duration::from_millis(duration_ms))
    }
}

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
    thumbnail_url: Option<String>,
) -> Result<(), String> {
    let cover_art = thumbnail_url.as_deref().and_then(download_thumbnail);

    let uri = format!("youtube:{video_id}");
    let mut pl = playlist.lock().map_err(|e| e.to_string())?;
    let id = pl.add_track(&uri);

    pl.update_metadata(
        id,
        &crate::audio::source::TrackMetadata {
            title: Some(title),
            artist: Some(artist),
            album: Some(album),
            duration: duration_from_ms(duration_ms),
            sample_rate: 44100,
            channels: 2,
            bitrate: None,
            genre: None,
            year: None,
            track_number: None,
            cover_art,
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
    thumbnail_url: Option<String>,
) -> Result<(), String> {
    let cover_art = thumbnail_url.as_deref().and_then(download_thumbnail);

    let uri = format!("youtube:{video_id}");
    let mut pl = playlist.lock().map_err(|e| e.to_string())?;
    let id = pl.add_track(&uri);
    pl.update_metadata(
        id,
        &crate::audio::source::TrackMetadata {
            title: Some(title),
            artist: Some(artist),
            album: Some(album),
            duration: duration_from_ms(duration_ms),
            sample_rate: 44100,
            channels: 2,
            bitrate: None,
            genre: None,
            year: None,
            track_number: None,
            cover_art,
        },
    );
    Ok(())
}

/// Add multiple YouTube tracks to the playlist at once.
/// If `play_first` is true, the first track is played immediately.
#[tauri::command]
pub fn youtube_add_tracks(
    engine: State<'_, Arc<crate::audio::engine::AudioEngine>>,
    playlist: State<'_, Arc<std::sync::Mutex<crate::playlist::manager::PlaylistManager>>>,
    tracks: Vec<YtTrackInput>,
    play_first: bool,
) -> Result<(), String> {
    let mut pl = playlist.lock().map_err(|e| e.to_string())?;

    let mut first_path: Option<String> = None;
    let mut first_meta: Option<crate::audio::source::TrackMetadata> = None;

    for (i, t) in tracks.iter().enumerate() {
        let uri = format!("youtube:{}", t.video_id);
        let id = pl.add_track(&uri);
        let meta = crate::audio::source::TrackMetadata {
            title: Some(t.title.clone()),
            artist: Some(t.artist.clone()),
            album: Some(t.album.clone()),
            duration: duration_from_ms(t.duration_ms),
            sample_rate: 44100,
            channels: 2,
            bitrate: None,
            genre: None,
            year: None,
            track_number: None,
            cover_art: if i == 0 { t.thumbnail_url.as_deref().and_then(download_thumbnail) } else { None },
        };
        pl.update_metadata(id, &meta);

        if i == 0 && play_first {
            pl.play_track(id);
            first_path = Some(uri);
            first_meta = Some(meta);
        }
    }

    if let (Some(path), Some(meta)) = (first_path, first_meta) {
        drop(pl);
        crate::commands::play_path(&engine, &path, None, Some(meta))?;
    }

    Ok(())
}

/// Input type for bulk track addition.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct YtTrackInput {
    pub video_id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: u64,
    pub thumbnail_url: Option<String>,
}

// ---------------------------------------------------------------------------
// Save / download commands
// ---------------------------------------------------------------------------

/// Save the currently-playing YouTube track to the user's download directory.
///
/// Reuses the temp file the audio source has already downloaded — no second
/// network round-trip. Returns the saved file path on success.
#[tauri::command]
pub async fn youtube_save_current_track() -> Result<String, String> {
    let active = crate::audio::youtube::current_download()
        .ok_or_else(|| "no YouTube track is currently playing".to_string())?;

    let path = tauri::async_runtime::spawn_blocking(move || {
        crate::youtube::save::save_active(active)
    })
    .await
    .map_err(|e| format!("save task panicked: {e}"))??;

    Ok(path.to_string_lossy().into_owned())
}

/// Download a YouTube track that's already in the playlist (looked up by
/// track id). Reuses the metadata/cover art the playlist already holds.
///
/// If the requested track is the currently-playing one, the caller should
/// prefer `youtube_save_current_track` to avoid the second yt-dlp run — but
/// this command works in either case as a safe fallback.
#[tauri::command]
pub async fn youtube_download_playlist_track(
    playlist: tauri::State<'_, Arc<std::sync::Mutex<crate::playlist::manager::PlaylistManager>>>,
    track_id: u64,
) -> Result<String, String> {
    let (video_id, metadata) = {
        let pl = playlist.lock().map_err(|e| e.to_string())?;
        let track = pl.get_track(track_id).ok_or("track not found")?;
        if !track.path.starts_with("youtube:") {
            return Err("not a YouTube track".into());
        }
        let video_id = track.path.trim_start_matches("youtube:").to_string();
        (video_id, track.to_source_metadata())
    };

    let path = tauri::async_runtime::spawn_blocking(move || {
        crate::youtube::save::download_and_save(&video_id, &metadata, None)
    })
    .await
    .map_err(|e| format!("download task panicked: {e}"))??;

    Ok(path.to_string_lossy().into_owned())
}

/// Download a YouTube track headlessly (no playback) and save it to the
/// download directory with metadata + cover art tagged in.
#[tauri::command]
pub async fn youtube_download_track(
    video_id: String,
    title: String,
    artist: String,
    album: String,
    duration_ms: u64,
    thumbnail_url: Option<String>,
) -> Result<String, String> {
    let metadata = crate::audio::source::TrackMetadata {
        title: Some(title),
        artist: Some(artist),
        album: Some(album),
        duration: duration_from_ms(duration_ms),
        sample_rate: 44100,
        channels: 2,
        bitrate: None,
        genre: None,
        year: None,
        track_number: None,
        cover_art: None,
    };

    let path = tauri::async_runtime::spawn_blocking(move || {
        crate::youtube::save::download_and_save(
            &video_id,
            &metadata,
            thumbnail_url.as_deref(),
        )
    })
    .await
    .map_err(|e| format!("download task panicked: {e}"))??;

    Ok(path.to_string_lossy().into_owned())
}

/// Download a thumbnail image and return its bytes.
/// Returns None on any failure (non-blocking to caller).
pub(crate) fn download_thumbnail(url: &str) -> Option<Vec<u8>> {
    use std::io::Read;
    let response = ureq::get(url)
        .header("User-Agent", "RetroAmp/0.1")
        .config()
        .timeout_connect(Some(std::time::Duration::from_secs(5)))
        .timeout_recv_response(Some(std::time::Duration::from_secs(5)))
        .build()
        .call()
        .ok()?;
    let mut bytes = Vec::new();
    response
        .into_body()
        .into_reader()
        .take(512 * 1024) // Max 512KB
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.is_empty() {
        return None;
    }
    Some(bytes)
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
// Library commands (authenticated only)
// ---------------------------------------------------------------------------

/// Get the user's liked songs.
#[tauri::command]
pub async fn youtube_get_library_songs() -> Result<Vec<YtTrack>, String> {
    crate::youtube::api::get_library_songs().await
}

/// Get the user's playlists.
#[tauri::command]
pub async fn youtube_get_library_playlists() -> Result<Vec<YtPlaylist>, String> {
    crate::youtube::api::get_library_playlists().await
}

/// Get the user's listening history.
#[tauri::command]
pub async fn youtube_get_history() -> Result<Vec<YtTrack>, String> {
    crate::youtube::api::get_history().await
}

// ---------------------------------------------------------------------------
// Write operations (authenticated only)
// ---------------------------------------------------------------------------

/// Like a track on YouTube Music.
#[tauri::command]
pub async fn youtube_like_track(video_id: String) -> Result<(), String> {
    crate::youtube::api::like_track(&video_id).await
}

/// Remove a like from a track on YouTube Music.
#[tauri::command]
pub async fn youtube_unlike_track(video_id: String) -> Result<(), String> {
    crate::youtube::api::unlike_track(&video_id).await
}

/// Create a new YouTube Music playlist. Returns the new playlist ID.
#[tauri::command]
pub async fn youtube_create_playlist(
    title: String,
    video_ids: Vec<String>,
) -> Result<String, String> {
    crate::youtube::api::create_playlist(&title, &video_ids).await
}

/// Delete a YouTube Music playlist.
#[tauri::command]
pub async fn youtube_delete_playlist(playlist_id: String) -> Result<(), String> {
    crate::youtube::api::delete_playlist(&playlist_id).await
}

/// Add a track to a YouTube Music playlist.
#[tauri::command]
pub async fn youtube_add_to_yt_playlist(
    playlist_id: String,
    video_id: String,
) -> Result<(), String> {
    crate::youtube::api::add_to_yt_playlist(&playlist_id, &video_id).await
}

/// Remove a track from a YouTube Music playlist.
#[tauri::command]
pub async fn youtube_remove_from_yt_playlist(
    playlist_id: String,
    video_id: String,
    set_video_id: String,
) -> Result<(), String> {
    crate::youtube::api::remove_from_yt_playlist(&playlist_id, &video_id, &set_video_id).await
}

/// Subscribe to a YouTube Music artist.
#[tauri::command]
pub async fn youtube_subscribe(channel_id: String) -> Result<(), String> {
    crate::youtube::api::subscribe(&channel_id).await
}

/// Unsubscribe from a YouTube Music artist.
#[tauri::command]
pub async fn youtube_unsubscribe(channel_id: String) -> Result<(), String> {
    crate::youtube::api::unsubscribe(&channel_id).await
}

// ---------------------------------------------------------------------------
// Browse: Home feed and Explore
// ---------------------------------------------------------------------------

/// Get the YouTube Music home feed (personalized recommendations).
#[tauri::command]
pub async fn youtube_get_home() -> Result<serde_json::Value, String> {
    crate::youtube::api::get_home_feed().await
}

/// Get moods and genres for the Explore tab.
#[tauri::command]
pub async fn youtube_get_moods_and_genres() -> Result<serde_json::Value, String> {
    crate::youtube::api::get_moods_and_genres().await
}

/// Browse a genre/mood category and return its playlists.
#[tauri::command]
pub async fn youtube_get_genre_playlists(browse_id: String, params: Option<String>) -> Result<Vec<super::types::YtPlaylist>, String> {
    crate::youtube::api::get_genre_playlists(&browse_id, params.as_deref()).await
}

/// Get the user's subscribed/library artists.
#[tauri::command]
pub async fn youtube_get_library_artists() -> Result<Vec<super::types::YtArtistRef>, String> {
    crate::youtube::api::get_library_artists().await
}

// ---------------------------------------------------------------------------
// Settings commands
// ---------------------------------------------------------------------------

/// YouTube settings returned to and accepted from the frontend.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct YouTubeSettings {
    pub quality: String,
    pub has_cookie: bool,
    pub auth_user: u32,
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
        auth_user: cfg.youtube.auth_user.unwrap_or(0),
        ytdlp_path: ytdlp_path.map(|p| p.to_string_lossy().to_string()),
        ytdlp_status,
    }
}

/// Update YouTube settings.
#[tauri::command]
pub fn set_youtube_settings(quality: String, auth_user: u32, ytdlp_path: Option<String>) -> Result<(), String> {
    let mut cfg = crate::config::AppConfig::load();
    cfg.youtube.quality = quality;
    cfg.youtube.auth_user = if auth_user == 0 { None } else { Some(auth_user) };
    cfg.youtube.ytdlp_path = ytdlp_path;
    cfg.save()
}

/// Manually trigger a yt-dlp update check (downloads new binary if available).
#[tauri::command]
pub async fn youtube_update_ytdlp() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(|| {
        crate::youtube::ytdlp::check_for_update();
        // Re-resolve the path/version info after update.
        match crate::youtube::ytdlp::find() {
            Some(p) if p.to_str() == Some("yt-dlp") => "System PATH".to_string(),
            Some(p) => format!("Installed: {}", p.display()),
            None => "Not installed".to_string(),
        }
    })
    .await
    .map_err(|e| format!("update task failed: {e}"))
}

/// Save a YouTube Music cookie for authenticated access.
/// Also fetches the DATASYNC_ID for channel identification and
/// reinitializes the API client.
#[tauri::command]
pub async fn youtube_save_cookie(cookie: String) -> Result<YouTubeAuthStatus, String> {
    // Try to login first to validate the cookie.
    crate::youtube::api::login_with_cookie(&cookie).await?;

    // Fetch DATASYNC_ID from music.youtube.com for channel identification.
    // This is needed for library access on accounts with Brand Accounts.
    let datasync_id = fetch_datasync_id(&cookie);

    // Persist cookie and DATASYNC_ID.
    let mut cfg = crate::config::AppConfig::load();
    cfg.youtube.cookie = Some(cookie);
    cfg.youtube.datasync_id = datasync_id.clone();
    cfg.save().map_err(|e| format!("Failed to save cookie: {e}"))?;

    log::info!("[youtube] cookie saved, datasync_id={datasync_id:?}");
    Ok(YouTubeAuthStatus { authenticated: true })
}

/// Fetch DATASYNC_ID from music.youtube.com page using the user's cookie.
/// Fetch DATASYNC_ID from music.youtube.com page using the user's cookie.
/// Processes it like OuterTune: takes the part BEFORE "||".
fn fetch_datasync_id(cookie: &str) -> Option<String> {
    let response = ureq::get("https://music.youtube.com")
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:88.0) Gecko/20100101 Firefox/88.0")
        .header("Cookie", cookie)
        .call()
        .ok()?;

    let body = response.into_body().read_to_string().ok()?;

    // Extract DATASYNC_ID from ytcfg.
    let raw_dsid = body
        .split_once("\"DATASYNC_ID\":\"")
        .and_then(|(_, rest)| rest.split_once('"'))
        .map(|(val, _)| val.to_string())
        .unwrap_or_default();

    if raw_dsid.is_empty() {
        return None;
    }

    // Process like OuterTune: take the part BEFORE "||".
    // For "107641838249180395925||103534278559884561104" → "107641838249180395925"
    // For "103534278559884561104||" → "103534278559884561104"
    // For "103534278559884561104" → "103534278559884561104"
    let processed = raw_dsid.split("||").next().unwrap_or("").to_string();

    log::info!("[youtube] extracted DATASYNC_ID: {raw_dsid} → processed: {processed}");

    if processed.is_empty() {
        None
    } else {
        Some(processed)
    }
}

/// Pre-create the hidden Google sign-in WebView (`youtube_login`) at
/// app startup.
///
/// On Wayland (GTK3 + WebKitGTK), creating a new WebView at runtime while
/// other WebViews are alive corrupts GTK's internal pointer-event state and
/// permanently breaks dragging, close, and right-click context menus on
/// every existing window.  The login window must therefore be created
/// during `setup` alongside the panel windows, then reused via `navigate()`
/// — never built or destroyed at runtime.
pub fn precreate_helper_windows(app: &AppHandle) {
    use tauri::webview::PageLoadEvent;
    use tauri::WebviewWindowBuilder;

    // Install shared state once.
    if app.try_state::<YouTubeWebviewState>().is_none() {
        app.manage(YouTubeWebviewState::default());
    }
    let extracted = app
        .state::<YouTubeWebviewState>()
        .login_extracted
        .clone();

    if app.get_webview_window("youtube_login").is_none() {
        let app_for_handler = app.clone();
        let extracted_for_handler = extracted.clone();
        let builder = WebviewWindowBuilder::new(
            app,
            "youtube_login",
            tauri::WebviewUrl::External("about:blank".parse().unwrap()),
        )
        .title("YouTube Music — Sign In")
        .inner_size(500.0, 700.0)
        .center()
        .visible(false)
        .skip_taskbar(true)
        .on_page_load(move |webview, payload| {
            if payload.event() != PageLoadEvent::Finished {
                return;
            }
            let url = payload.url().to_string();
            if !url.starts_with("https://music.youtube.com") {
                return;
            }

            // Only extract once per login attempt; reset by `youtube_login_webview`.
            if extracted_for_handler.swap(true, Ordering::Relaxed) {
                return;
            }

            log::info!("[youtube] login WebView reached music.youtube.com, extracting...");

            let app = app_for_handler.clone();
            let webview_clone = webview.clone();
            tauri::async_runtime::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(1000)).await;

                let cookies = match webview_clone.cookies_for_url(
                    "https://music.youtube.com".parse().unwrap(),
                ) {
                    Ok(c) => c,
                    Err(e) => {
                        log::error!("[youtube] failed to get cookies: {e}");
                        let _ = app.emit("youtube-login-result", serde_json::json!({
                            "success": false, "error": format!("{e}")
                        }));
                        return;
                    }
                };

                let cookie_str: String = cookies
                    .iter()
                    .map(|c| format!("{}={}", c.name(), c.value()))
                    .collect::<Vec<_>>()
                    .join("; ");

                if cookie_str.is_empty() {
                    log::warn!("[youtube] no cookies extracted");
                    let _ = app.emit("youtube-login-result", serde_json::json!({
                        "success": false, "error": "No cookies found after login"
                    }));
                    return;
                }

                log::info!("[youtube] extracted {} cookies ({} chars)", cookies.len(), cookie_str.len());

                let datasync_id = fetch_datasync_id(&cookie_str);

                if let Err(e) = crate::youtube::api::login_with_cookie(&cookie_str).await {
                    log::error!("[youtube] API init failed: {e}");
                    let _ = app.emit("youtube-login-result", serde_json::json!({
                        "success": false, "error": format!("API init failed: {e}")
                    }));
                    return;
                }

                let mut cfg = crate::config::AppConfig::load();
                cfg.youtube.cookie = Some(cookie_str);
                cfg.youtube.datasync_id = datasync_id.clone();
                let _ = cfg.save();

                log::info!("[youtube] login complete, datasync_id={datasync_id:?}");

                let _ = app.emit("youtube-login-result", serde_json::json!({
                    "success": true
                }));

                // Hide instead of close — the window is reused across logins.
                if let Some(win) = app.get_webview_window("youtube_login") {
                    let _ = win.hide();
                }
            });
        });

        if let Err(e) = builder.build() {
            log::warn!("[youtube] failed to pre-create login window: {e}");
        }
    }
}

/// Open the WebView-based login window for YouTube Music.
///
/// The window is pre-created at startup (see `precreate_helper_windows`) and
/// reused for every login — we never build or destroy a WebView at runtime
/// because doing so corrupts GTK pointer state on Wayland.
///
/// Each invocation clears stored browsing data (so the user gets a fresh
/// Google login prompt rather than auto-signed-in from a previous session)
/// and navigates to Google's sign-in page.  After the user signs in and
/// lands on music.youtube.com, the page-load handler extracts cookies,
/// saves them, emits `youtube-login-result`, and hides the window.
#[tauri::command]
pub async fn youtube_login_webview(
    app: AppHandle,
    state: State<'_, YouTubeWebviewState>,
) -> Result<(), String> {
    let win = app
        .get_webview_window("youtube_login")
        .ok_or_else(|| "Login window not pre-created at startup".to_string())?;

    // If the user re-clicked while a previous login is still in flight,
    // just refocus rather than tearing down the in-progress page.
    if win.is_visible().unwrap_or(false) {
        let _ = win.set_focus();
        return Ok(());
    }

    // Reset the extraction flag so the page-load handler will pick up
    // cookies on the next music.youtube.com load.
    state.login_extracted.store(false, Ordering::Relaxed);

    // Clear stored browsing data so the Google login prompt isn't
    // auto-filled from a prior session.
    let _ = win.clear_all_browsing_data();

    let login_url = "https://accounts.google.com/ServiceLogin?continue=https%3A%2F%2Fmusic.youtube.com";
    let parsed = login_url
        .parse()
        .map_err(|e| format!("invalid login URL: {e}"))?;
    win.navigate(parsed).map_err(|e| format!("navigate failed: {e}"))?;
    let _ = win.show();
    let _ = win.set_focus();
    Ok(())
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
