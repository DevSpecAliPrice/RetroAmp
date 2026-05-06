//! Spotify Web API client — updated for the February 2026 API changes.
//!
//! Key changes from Feb 2026:
//! - Search limit max is 10 (was 50) for Development Mode apps
//! - Playlist tracks endpoint renamed from /tracks to /items
//! - /artists/{id}/top-tracks removed
//! - /browse/new-releases removed
//! - Several response fields removed (popularity, followers, etc.)
//! - Only own/collaborative playlists return items
//!
//! All functions are synchronous (using ureq) and should be called from
//! `tauri::async_runtime::spawn_blocking()`.

use std::time::Duration;

use super::types::*;

const API_BASE: &str = "https://api.spotify.com/v1";
const USER_AGENT: &str = "RetroAmp/0.1";
const TIMEOUT: Duration = Duration::from_secs(15);
const MAX_RETRIES: u32 = 3;

/// Dev Mode search limit maximum (Feb 2026 change).
const SEARCH_LIMIT_MAX: usize = 10;

/// Make an authenticated GET request to the Spotify Web API.
fn api_get<T: serde::de::DeserializeOwned>(token: &str, url: &str) -> Result<T, String> {
    log::debug!("Spotify API GET: {url}");

    for attempt in 0..=MAX_RETRIES {
        let response = ureq::get(url)
            .header("Authorization", &format!("Bearer {token}"))
            .header("User-Agent", USER_AGENT)
            .config()
            .timeout_connect(Some(TIMEOUT))
            .timeout_recv_response(Some(TIMEOUT))
            .http_status_as_error(false)
            .build()
            .call()
            .map_err(|e| {
                log::error!("Spotify API transport error for {url}: {e}");
                format!("Spotify API error: {e}")
            })?;

        let status = response.status().as_u16();

        if status == 429 && attempt < MAX_RETRIES {
            let wait_secs = 2u64.pow(attempt + 1);
            log::warn!("Spotify API rate limited (429), retrying in {wait_secs}s (attempt {}/{})", attempt + 1, MAX_RETRIES);
            std::thread::sleep(Duration::from_secs(wait_secs));
            continue;
        }

        let body = response
            .into_body()
            .read_to_string()
            .map_err(|e| format!("Failed to read Spotify API response: {e}"))?;

        if status >= 400 {
            log::error!("Spotify API HTTP {status} for {url}: {}", &body[..body.len().min(500)]);
            return Err(format!("Spotify API error (HTTP {status}): {}", &body[..body.len().min(200)]));
        }

        return serde_json::from_str(&body).map_err(|e| {
            log::error!("Spotify API parse error for {url}: {e}");
            log::error!("Response body (first 500 chars): {}", &body[..body.len().min(500)]);
            format!("Failed to parse Spotify API response: {e}")
        });
    }

    Err("Spotify API rate limited — please try again in a moment".to_string())
}

fn urlencoded(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            b' ' => out.push_str("%20"),
            _ => {
                out.push_str(&format!("%{b:02X}"));
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Search (max 10 results per page in Dev Mode)
// ---------------------------------------------------------------------------

pub fn search(token: &str, query: &str, types: &str, limit: usize) -> Result<SearchResults, String> {
    let clamped = limit.min(SEARCH_LIMIT_MAX);
    let url = format!(
        "{API_BASE}/search?q={}&type={types}&limit={clamped}",
        urlencoded(query),
    );
    api_get(token, &url)
}

// ---------------------------------------------------------------------------
// User Library
// ---------------------------------------------------------------------------

pub fn get_user_playlists(token: &str, limit: usize, offset: usize) -> Result<Paged<ApiPlaylist>, String> {
    let url = format!("{API_BASE}/me/playlists?limit={limit}&offset={offset}");
    api_get(token, &url)
}

/// Get items from a playlist (Feb 2026: /tracks renamed to /items).
/// Only returns content for playlists the user owns or collaborates on.
pub fn get_playlist_items(token: &str, playlist_id: &str, limit: usize, offset: usize) -> Result<Paged<PlaylistTrackItem>, String> {
    let url = format!(
        "{API_BASE}/playlists/{playlist_id}/items?limit={limit}&offset={offset}"
    );
    api_get(token, &url)
}

pub fn get_saved_albums(token: &str, limit: usize, offset: usize) -> Result<Paged<SavedAlbum>, String> {
    let url = format!("{API_BASE}/me/albums?limit={limit}&offset={offset}");
    api_get(token, &url)
}

pub fn get_saved_tracks(token: &str, limit: usize, offset: usize) -> Result<Paged<SavedTrack>, String> {
    let url = format!("{API_BASE}/me/tracks?limit={limit}&offset={offset}");
    api_get(token, &url)
}

// ---------------------------------------------------------------------------
// Browse / Detail
// ---------------------------------------------------------------------------

pub fn get_album(token: &str, album_id: &str) -> Result<ApiAlbum, String> {
    let url = format!("{API_BASE}/albums/{album_id}");
    api_get(token, &url)
}

pub fn get_artist(token: &str, artist_id: &str) -> Result<ApiArtist, String> {
    let url = format!("{API_BASE}/artists/{artist_id}");
    api_get(token, &url)
}

/// Get an artist's albums.
pub fn get_artist_albums(token: &str, artist_id: &str, limit: usize, offset: usize) -> Result<Paged<ApiAlbumRef>, String> {
    let url = format!(
        "{API_BASE}/artists/{artist_id}/albums?include_groups=album,single,compilation&limit={limit}&offset={offset}"
    );
    api_get(token, &url)
}

/// Get the user's recently played tracks.
pub fn get_recently_played(token: &str, limit: usize) -> Result<CursorPaged<RecentlyPlayedItem>, String> {
    let url = format!("{API_BASE}/me/player/recently-played?limit={limit}");
    api_get(token, &url)
}

// ---------------------------------------------------------------------------
// Mutations (PUT/POST/DELETE)
// ---------------------------------------------------------------------------

fn handle_status<T>(status: u16, body: String, on_ok: impl FnOnce() -> T, url: &str) -> Result<T, String> {
    if status >= 400 {
        log::error!("Spotify API HTTP {status} for {url}: {}", &body[..body.len().min(500)]);
        return Err(format!("Spotify API error (HTTP {status}): {}", &body[..body.len().min(200)]));
    }
    Ok(on_ok())
}

/// Save a track to the user's library (Like).
pub fn save_track(token: &str, track_id: &str) -> Result<(), String> {
    let url = format!("{API_BASE}/me/tracks?ids={track_id}");
    let response = ureq::put(&url)
        .header("Authorization", &format!("Bearer {token}"))
        .header("User-Agent", USER_AGENT)
        .config()
        .timeout_connect(Some(TIMEOUT))
        .timeout_recv_response(Some(TIMEOUT))
        .http_status_as_error(false)
        .build()
        .send_empty()
        .map_err(|e| format!("Spotify API error: {e}"))?;
    let status = response.status().as_u16();
    let body = response.into_body().read_to_string().unwrap_or_default();
    handle_status(status, body, || (), &url)
}

/// Remove a track from the user's library (Unlike).
pub fn unsave_track(token: &str, track_id: &str) -> Result<(), String> {
    let url = format!("{API_BASE}/me/tracks?ids={track_id}");
    let response = ureq::delete(&url)
        .header("Authorization", &format!("Bearer {token}"))
        .header("User-Agent", USER_AGENT)
        .config()
        .timeout_connect(Some(TIMEOUT))
        .timeout_recv_response(Some(TIMEOUT))
        .http_status_as_error(false)
        .build()
        .call()
        .map_err(|e| format!("Spotify API error: {e}"))?;
    let status = response.status().as_u16();
    let body = response.into_body().read_to_string().unwrap_or_default();
    handle_status(status, body, || (), &url)
}

/// Check whether tracks are saved in the user's library.
pub fn check_saved_tracks(token: &str, track_ids: &[String]) -> Result<Vec<bool>, String> {
    let ids = track_ids.join(",");
    let url = format!("{API_BASE}/me/tracks/contains?ids={ids}");
    api_get(token, &url)
}

/// Add a track to one of the user's playlists.
pub fn add_to_user_playlist(token: &str, playlist_id: &str, track_uri: &str) -> Result<(), String> {
    let url = format!("{API_BASE}/playlists/{playlist_id}/tracks");
    let body = serde_json::json!({ "uris": [track_uri] }).to_string();
    let response = ureq::post(&url)
        .header("Authorization", &format!("Bearer {token}"))
        .header("User-Agent", USER_AGENT)
        .header("Content-Type", "application/json")
        .config()
        .timeout_connect(Some(TIMEOUT))
        .timeout_recv_response(Some(TIMEOUT))
        .http_status_as_error(false)
        .build()
        .send(body.as_bytes())
        .map_err(|e| format!("Spotify API error: {e}"))?;
    let status = response.status().as_u16();
    let body = response.into_body().read_to_string().unwrap_or_default();
    handle_status(status, body, || (), &url)
}
