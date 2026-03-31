//! Data types for YouTube Music API responses.
//!
//! These are the types serialised to the frontend via Tauri commands.
//! They're intentionally simple and flat — conversion from ytmapi-rs
//! internal types happens in `api.rs`.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Artist reference (embedded in tracks, albums)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YtArtistRef {
    /// Artist's browse ID (channel ID). None for "Various Artists" etc.
    #[serde(default)]
    pub browse_id: Option<String>,
    pub name: String,
}

// ---------------------------------------------------------------------------
// Album reference (embedded in tracks, search results)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YtAlbumRef {
    pub browse_id: String,
    pub name: String,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
    #[serde(default)]
    pub year: Option<String>,
    #[serde(default)]
    pub artists: Vec<YtArtistRef>,
    #[serde(default)]
    pub album_type: Option<String>,
}

// ---------------------------------------------------------------------------
// Track
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YtTrack {
    pub video_id: String,
    pub title: String,
    #[serde(default)]
    pub artists: Vec<YtArtistRef>,
    #[serde(default)]
    pub album: Option<YtAlbumRefSimple>,
    /// Duration as a human-readable string (e.g. "3:45").
    #[serde(default)]
    pub duration: Option<String>,
    /// Duration in milliseconds (parsed from the string when possible).
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
    #[serde(default)]
    pub explicit: bool,
    /// Playlist-specific entry ID, needed for removing tracks from playlists.
    /// Only present when the track was fetched from a playlist browse response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set_video_id: Option<String>,
}

/// Minimal album reference inside a track (just name + id).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YtAlbumRefSimple {
    pub browse_id: String,
    pub name: String,
}

// ---------------------------------------------------------------------------
// Full album (from GetAlbum)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YtAlbum {
    pub browse_id: String,
    pub title: String,
    #[serde(default)]
    pub artists: Vec<YtArtistRef>,
    #[serde(default)]
    pub year: Option<String>,
    #[serde(default)]
    pub tracks: Vec<YtTrack>,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
    #[serde(default)]
    pub album_type: Option<String>,
    #[serde(default)]
    pub duration: Option<String>,
}

// ---------------------------------------------------------------------------
// Full artist (from GetArtist)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YtArtist {
    pub browse_id: String,
    pub name: String,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub subscribers: Option<String>,
    #[serde(default)]
    pub top_tracks: Vec<YtTrack>,
    #[serde(default)]
    pub albums: Vec<YtAlbumRef>,
    #[serde(default)]
    pub singles: Vec<YtAlbumRef>,
}

// ---------------------------------------------------------------------------
// Playlist
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YtPlaylist {
    pub browse_id: String,
    pub title: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub track_count: Option<String>,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
}

// ---------------------------------------------------------------------------
// Search results
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YtSearchResults {
    #[serde(default)]
    pub tracks: Vec<YtTrack>,
    #[serde(default)]
    pub albums: Vec<YtAlbumRef>,
    #[serde(default)]
    pub artists: Vec<YtArtistRef>,
    #[serde(default)]
    pub playlists: Vec<YtPlaylist>,
}

// ---------------------------------------------------------------------------
// Playlist detail (playlist metadata + tracks)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YtPlaylistDetail {
    pub info: YtPlaylist,
    pub tracks: Vec<YtTrack>,
}
