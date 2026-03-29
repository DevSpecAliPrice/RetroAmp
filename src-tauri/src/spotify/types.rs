//! Serde types for the Spotify Web API JSON responses.
//!
//! These types are intentionally minimal — they only include the fields that
//! RetroAmp actually uses, and use `#[serde(default)]` to handle fields that
//! may be absent. This makes the deserialisation robust against API changes.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Pagination
// ---------------------------------------------------------------------------

/// Spotify's offset-based paging object.
/// Uses a custom deserializer for `items` that filters out null entries
/// (Spotify returns nulls for unavailable content).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(deserialize = "T: serde::Deserialize<'de>"))]
pub struct Paged<T> {
    #[serde(deserialize_with = "deserialize_filter_nulls")]
    pub items: Vec<T>,
    #[serde(default)]
    pub total: usize,
    #[serde(default)]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
    #[serde(default)]
    pub next: Option<String>,
}

/// Deserialise a JSON array, silently dropping null entries.
fn deserialize_filter_nulls<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    let items: Vec<Option<T>> = Vec::deserialize(deserializer)?;
    Ok(items.into_iter().flatten().collect())
}


/// Cursor-based paging object (used by recently-played).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorPaged<T> {
    pub items: Vec<T>,
    #[serde(default)]
    pub next: Option<String>,
    #[serde(default)]
    pub cursors: Option<Cursors>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cursors {
    pub after: Option<String>,
    pub before: Option<String>,
}

// ---------------------------------------------------------------------------
// Image
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiImage {
    pub url: String,
    #[serde(default)]
    pub height: Option<u32>,
    #[serde(default)]
    pub width: Option<u32>,
}

// ---------------------------------------------------------------------------
// Artist
// ---------------------------------------------------------------------------

/// Simplified artist reference (embedded in tracks, albums).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiArtistRef {
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub uri: Option<String>,
}

/// Full artist object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiArtist {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub uri: String,
    #[serde(default)]
    pub popularity: u32,
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default)]
    pub images: Vec<ApiImage>,
    #[serde(default)]
    pub followers: Option<Followers>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Followers {
    pub total: u32,
}

// ---------------------------------------------------------------------------
// Album
// ---------------------------------------------------------------------------

/// Simplified album reference (embedded in tracks).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiAlbumRef {
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub album_type: Option<String>,
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default)]
    pub images: Vec<ApiImage>,
    #[serde(default)]
    pub uri: Option<String>,
    #[serde(default)]
    pub artists: Vec<ApiArtistRef>,
    #[serde(default)]
    pub total_tracks: Option<u32>,
}

/// Full album object (from GET /v1/albums/{id}).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiAlbum {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub album_type: Option<String>,
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default)]
    pub release_date_precision: Option<String>,
    #[serde(default)]
    pub total_tracks: u32,
    #[serde(default)]
    pub uri: String,
    #[serde(default)]
    pub popularity: u32,
    #[serde(default)]
    pub images: Vec<ApiImage>,
    #[serde(default)]
    pub artists: Vec<ApiArtistRef>,
    #[serde(default)]
    pub genres: Vec<String>,
    /// Full tracklist (paged). Only present in full album responses, not search.
    #[serde(default)]
    pub tracks: Option<Paged<ApiTrackSimple>>,
}

/// Simplified track (in album tracklist — no album field, to avoid recursion).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiTrackSimple {
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub uri: Option<String>,
    #[serde(default)]
    pub duration_ms: u64,
    #[serde(default)]
    pub track_number: u32,
    #[serde(default)]
    pub disc_number: u32,
    #[serde(default)]
    pub explicit: bool,
    #[serde(default)]
    pub artists: Vec<ApiArtistRef>,
}

// ---------------------------------------------------------------------------
// Track
// ---------------------------------------------------------------------------

/// Full track object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiTrack {
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub uri: Option<String>,
    #[serde(default)]
    pub duration_ms: u64,
    #[serde(default)]
    pub track_number: u32,
    #[serde(default)]
    pub disc_number: u32,
    #[serde(default)]
    pub explicit: bool,
    #[serde(default)]
    pub popularity: u32,
    #[serde(default)]
    pub artists: Vec<ApiArtistRef>,
    #[serde(default)]
    pub album: Option<ApiAlbumRef>,
    #[serde(default)]
    pub is_local: bool,
}

// ---------------------------------------------------------------------------
// Playlist
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiPlaylist {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub uri: Option<String>,
    #[serde(default)]
    pub images: Vec<ApiImage>,
    #[serde(default)]
    pub owner: Option<ApiUser>,
    #[serde(default)]
    pub tracks: Option<PlaylistTrackRef>,
    #[serde(rename = "public")]
    #[serde(default)]
    pub is_public: Option<bool>,
    #[serde(default)]
    pub collaborative: bool,
}

/// Minimal track info in playlist listing (just total count).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistTrackRef {
    pub total: u32,
}

/// Playlist item (wraps a track with added_at info).
/// Feb 2026: field renamed from "track" to "item" in API responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistTrackItem {
    #[serde(default)]
    pub added_at: Option<String>,
    /// The track. Can be null for deleted/unavailable tracks.
    /// Accepts both "track" (old API) and "item" (new API) field names.
    #[serde(alias = "track", alias = "item")]
    pub track: Option<ApiTrack>,
}

// ---------------------------------------------------------------------------
// User
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiUser {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
}

// ---------------------------------------------------------------------------
// Search results
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResults {
    #[serde(default)]
    pub tracks: Option<Paged<ApiTrack>>,
    #[serde(default)]
    pub albums: Option<Paged<ApiAlbumRef>>,
    #[serde(default)]
    pub artists: Option<Paged<ApiArtist>>,
    #[serde(default)]
    pub playlists: Option<Paged<ApiPlaylist>>,
}

// ---------------------------------------------------------------------------
// Saved items (from /me/tracks, /me/albums)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedTrack {
    #[serde(default)]
    pub added_at: Option<String>,
    pub track: ApiTrack,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedAlbum {
    #[serde(default)]
    pub added_at: Option<String>,
    pub album: ApiAlbum,
}

// ---------------------------------------------------------------------------
// Recently played
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentlyPlayedItem {
    pub track: ApiTrack,
    #[serde(default)]
    pub played_at: Option<String>,
}

// ---------------------------------------------------------------------------
// Wrapper responses (for endpoints that wrap the paging object)
// ---------------------------------------------------------------------------

// Note: GET /artists/{id}/top-tracks was removed in the Feb 2026 API changes.
