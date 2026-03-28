//! Spotify Web API client — HTTP requests for browsing, searching, and
//! fetching user library data.
//!
//! All functions are synchronous (using ureq) and should be called from
//! `tauri::async_runtime::spawn_blocking()` to avoid blocking the Tokio runtime.
//!
//! Tokens are obtained from the librespot Session's token provider and
//! refreshed automatically when they expire.

use std::time::Duration;

use super::types::*;

const API_BASE: &str = "https://api.spotify.com/v1";
const USER_AGENT: &str = "RetroAmp/0.1";
const TIMEOUT: Duration = Duration::from_secs(15);

/// Maximum retries for rate-limited requests.
const MAX_RETRIES: u32 = 3;

/// Make an authenticated GET request to the Spotify Web API.
/// Automatically retries on 429 (rate limited) with the Retry-After delay.
fn api_get<T: serde::de::DeserializeOwned>(token: &str, url: &str) -> Result<T, String> {
    log::debug!("Spotify API GET: {url}");

    for attempt in 0..=MAX_RETRIES {
        let result = ureq::get(url)
            .header("Authorization", &format!("Bearer {token}"))
            .header("User-Agent", USER_AGENT)
            .config()
            .timeout_connect(Some(TIMEOUT))
            .timeout_recv_response(Some(TIMEOUT))
            .build()
            .call();

        match result {
            Ok(response) => {
                let body = response
                    .into_body()
                    .read_to_string()
                    .map_err(|e| format!("Failed to read Spotify API response: {e}"))?;

                return serde_json::from_str(&body).map_err(|e| {
                    log::error!("Spotify API parse error for {url}: {e}");
                    log::error!("Response body (first 500 chars): {}", &body[..body.len().min(500)]);
                    format!("Failed to parse Spotify API response: {e}")
                });
            }
            Err(ureq::Error::StatusCode(429)) => {
                if attempt < MAX_RETRIES {
                    // Spotify rate limit — wait and retry.
                    let wait_secs = 2u64.pow(attempt + 1); // 2, 4, 8 seconds
                    log::warn!("Spotify API rate limited (429), retrying in {wait_secs}s (attempt {}/{})", attempt + 1, MAX_RETRIES);
                    std::thread::sleep(Duration::from_secs(wait_secs));
                } else {
                    return Err("Spotify API rate limited — please try again in a moment".to_string());
                }
            }
            Err(e) => {
                log::error!("Spotify API error for {url}: {e}");
                return Err(format!("Spotify API error: {e}"));
            }
        }
    }

    Err("Spotify API request failed after retries".to_string())
}

/// Simple percent-encoding for query parameters.
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
// Search
// ---------------------------------------------------------------------------

/// Search Spotify for tracks, albums, artists, and/or playlists.
/// `types` is a comma-separated list: "track,album,artist,playlist".
pub fn search(token: &str, query: &str, types: &str, limit: usize) -> Result<SearchResults, String> {
    let url = format!(
        "{API_BASE}/search?q={}&type={types}&limit={limit}",
        urlencoded(query),
    );
    api_get(token, &url)
}

// ---------------------------------------------------------------------------
// User Library
// ---------------------------------------------------------------------------

/// Get the current user's playlists.
pub fn get_user_playlists(token: &str, limit: usize, offset: usize) -> Result<Paged<ApiPlaylist>, String> {
    let url = format!("{API_BASE}/me/playlists?limit={limit}&offset={offset}");
    api_get(token, &url)
}

/// Get tracks from a playlist.
pub fn get_playlist_tracks(token: &str, playlist_id: &str, limit: usize, offset: usize) -> Result<Paged<PlaylistTrackItem>, String> {
    let url = format!(
        "{API_BASE}/playlists/{playlist_id}/tracks?limit={limit}&offset={offset}"
    );
    api_get(token, &url)
}

/// Get the user's saved albums.
pub fn get_saved_albums(token: &str, limit: usize, offset: usize) -> Result<Paged<SavedAlbum>, String> {
    let url = format!("{API_BASE}/me/albums?limit={limit}&offset={offset}");
    api_get(token, &url)
}

/// Get the user's saved (liked) tracks.
pub fn get_saved_tracks(token: &str, limit: usize, offset: usize) -> Result<Paged<SavedTrack>, String> {
    let url = format!("{API_BASE}/me/tracks?limit={limit}&offset={offset}");
    api_get(token, &url)
}

// ---------------------------------------------------------------------------
// Browse / Detail
// ---------------------------------------------------------------------------

/// Get a full album with its track listing.
pub fn get_album(token: &str, album_id: &str) -> Result<ApiAlbum, String> {
    let url = format!("{API_BASE}/albums/{album_id}");
    api_get(token, &url)
}

/// Get a full artist profile.
pub fn get_artist(token: &str, artist_id: &str) -> Result<ApiArtist, String> {
    let url = format!("{API_BASE}/artists/{artist_id}");
    api_get(token, &url)
}

/// Get an artist's top tracks (for the user's market).
pub fn get_artist_top_tracks(token: &str, artist_id: &str) -> Result<Vec<ApiTrack>, String> {
    let url = format!("{API_BASE}/artists/{artist_id}/top-tracks");
    let resp: ArtistTopTracksResponse = api_get(token, &url)?;
    Ok(resp.tracks)
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
