//! YouTube Music API wrapper using ytmapi-rs.
//!
//! Supports two modes:
//! - **Unauthenticated** (default): search and browse work immediately, no setup.
//! - **Cookie-authenticated**: user pastes YouTube Music cookies to unlock
//!   personal library, liked songs, history, and recommendations.
//!
//! All public functions are async and should be called from Tauri async commands.

use std::sync::OnceLock;

use tokio::sync::Mutex;
use ytmapi_rs::auth::BrowserToken;
use ytmapi_rs::auth::noauth::NoAuthToken;
use ytmapi_rs::common::{AlbumID, ArtistChannelID, PlaylistID, YoutubeID};
use ytmapi_rs::query::{
    GetAlbumQuery, GetArtistQuery, GetPlaylistDetailsQuery, GetPlaylistTracksQuery,
    GetSearchSuggestionsQuery, SearchQuery,
};
use ytmapi_rs::YtMusic;

use super::types::*;

// ---------------------------------------------------------------------------
// Dual-mode client (unauthenticated or cookie-authenticated)
// ---------------------------------------------------------------------------

/// Wraps either an unauthenticated or authenticated YtMusic client.
/// All queries are generic over `AuthToken`, so both variants support
/// the same operations — but library/history queries only succeed when
/// authenticated.
enum YtClient {
    NoAuth(YtMusic<NoAuthToken>),
    Browser(YtMusic<BrowserToken>),
}

impl Clone for YtClient {
    fn clone(&self) -> Self {
        match self {
            YtClient::NoAuth(c) => YtClient::NoAuth(c.clone()),
            YtClient::Browser(c) => YtClient::Browser(c.clone()),
        }
    }
}

/// Dispatch a query to whichever client variant is active.
///
/// Both `Query<NoAuthToken>` and `Query<BrowserToken>` produce the same
/// `Output` type for every public query, but Rust's type system treats them
/// as different associated types. We work around this by cloning the query
/// and running it through exactly one branch, discarding the other.
macro_rules! yt_query {
    ($client:expr, $query:expr) => {{
        let q = $query;
        match $client {
            YtClient::NoAuth(ref c) => c.query(q).await,
            YtClient::Browser(ref c) => c.query(q).await,
        }
    }};
}

static YT_CLIENT: OnceLock<Mutex<Option<YtClient>>> = OnceLock::new();

/// SOCS consent cookie — bypasses GDPR consent wall on music.youtube.com.
const SOCS_COOKIE: &str = "SOCS=CAESEwgDEgk2ODE5MTkxMjAaAmVuIAEaBgiA_LyaBg";

/// User-Agent matching what ytmapi-rs uses internally.
const USER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:88.0) Gecko/20100101 Firefox/88.0";

/// Fetch VISITOR_DATA from music.youtube.com ourselves using ureq.
///
/// ytmapi-rs's built-in init fails when YouTube returns a consent wall.
/// We bypass this by fetching the page with the SOCS consent cookie using
/// ureq (synchronous, proven to work in our radio stream code).
fn fetch_visitor_data() -> Result<String, String> {
    let response = ureq::get("https://music.youtube.com")
        .header("User-Agent", USER_AGENT)
        .header("Cookie", SOCS_COOKIE)
        .call()
        .map_err(|e| format!("Failed to fetch music.youtube.com: {e}"))?;

    let body = response
        .into_body()
        .read_to_string()
        .map_err(|e| format!("Failed to read response body: {e}"))?;

    // Search the full page body for visitor data.
    // YouTube uses either "VISITOR_DATA" or "EOM_VISITOR_DATA" depending
    // on page version. We try both, searching the raw HTML.
    let visitor_data = extract_json_string_value(&body, "VISITOR_DATA")
        .or_else(|| extract_json_string_value(&body, "EOM_VISITOR_DATA"))
        .ok_or_else(|| "VISITOR_DATA not found in page".to_string())?;

    // URL-decode if needed (YouTube sometimes percent-encodes the value).
    let visitor_data = percent_encoding::percent_decode_str(&visitor_data)
        .decode_utf8_lossy()
        .to_string();

    log::info!(
        "[youtube] fetched visitor data ({} chars): {}...",
        visitor_data.len(),
        &visitor_data[..visitor_data.len().min(30)],
    );
    Ok(visitor_data)
}

/// Extract a JSON string value from raw text: finds `"KEY":"VALUE"` and returns VALUE.
fn extract_json_string_value(text: &str, key: &str) -> Option<String> {
    let pattern = format!("\"{key}\":\"");
    let after = text.split_once(&pattern)?.1;
    let value = after.split_once('"')?.0;
    if value.is_empty() {
        return None;
    }
    Some(value.to_string())
}

/// Get or initialise the YouTube Music client (unauthenticated by default).
async fn get_client() -> Result<YtClient, String> {
    let cell = YT_CLIENT.get_or_init(|| Mutex::new(None));
    let mut guard = cell.lock().await;
    if let Some(ref client) = *guard {
        return Ok(client.clone());
    }
    log::info!("[youtube] initializing YouTube Music API client (unauthenticated)...");

    // Fetch visitor data ourselves (bypasses consent wall issues).
    let visitor_data = fetch_visitor_data()?;

    // Construct a NoAuthToken by deserializing from JSON.
    // NoAuthToken has: create_time (chrono DateTime<Utc>) and visitor_id (String).
    let token_json = serde_json::json!({
        "create_time": chrono::Utc::now(),
        "visitor_id": visitor_data,
    });
    let token: NoAuthToken = serde_json::from_value(token_json)
        .map_err(|e| format!("Failed to construct auth token: {e}"))?;

    // Build the YtMusic client with our manually-fetched token.
    let client = ytmapi_rs::YtMusicBuilder::new()
        .with_auth_token(token)
        .build()
        .map_err(|e| format!("Failed to build YouTube Music client: {e}"))?;

    log::info!("[youtube] API client initialized successfully");
    let wrapped = YtClient::NoAuth(client);
    *guard = Some(wrapped.clone());
    Ok(wrapped)
}

/// Login with YouTube Music cookies. Replaces the current client with an
/// authenticated one. The cookie string should be the raw cookie header
/// value from a logged-in browser session on music.youtube.com.
pub async fn login_with_cookie(cookie: &str) -> Result<(), String> {
    log::info!("[youtube] logging in with browser cookie...");

    // Build reqwest client with SOCS consent cookie as default header.
    let mut default_headers = reqwest::header::HeaderMap::new();
    default_headers.insert(
        reqwest::header::COOKIE,
        reqwest::header::HeaderValue::from_static(SOCS_COOKIE),
    );
    let reqwest_client = reqwest::Client::builder()
        .default_headers(default_headers)
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))?;
    let yt_client =
        ytmapi_rs::client::Client::new_from_reqwest_client(reqwest_client);

    let client = ytmapi_rs::YtMusicBuilder::new_with_client(yt_client)
        .with_browser_token_cookie(cookie.to_string())
        .build()
        .await
        .map_err(|e| {
            log::error!("[youtube] cookie login failed: {e}");
            format!("YouTube Music login failed: {e}")
        })?;

    log::info!("[youtube] logged in successfully");

    let cell = YT_CLIENT.get_or_init(|| Mutex::new(None));
    let mut guard = cell.lock().await;
    *guard = Some(YtClient::Browser(client));
    Ok(())
}

/// Log out — replace the authenticated client with an unauthenticated one.
pub async fn logout() -> Result<(), String> {
    log::info!("[youtube] logging out...");
    let cell = YT_CLIENT.get_or_init(|| Mutex::new(None));
    let mut guard = cell.lock().await;
    *guard = None; // Will be re-initialized as NoAuth on next query.
    Ok(())
}

/// Whether the current client is authenticated (cookie login).
pub async fn is_authenticated() -> bool {
    let cell = YT_CLIENT.get_or_init(|| Mutex::new(None));
    let guard = cell.lock().await;
    matches!(&*guard, Some(YtClient::Browser(_)))
}

// ---------------------------------------------------------------------------
// Search
// ---------------------------------------------------------------------------

/// Search YouTube Music. Returns combined results (songs, albums, artists).
///
/// Uses separate filtered searches instead of unfiltered search because
/// YouTube's mixed search returns section types (e.g. "More results") that
/// ytmapi-rs's parser doesn't handle.
pub async fn search(query: &str) -> Result<YtSearchResults, String> {
    use ytmapi_rs::query::search::{SongsFilter, AlbumsFilter, ArtistsFilter};

    let client = get_client().await?;

    // Run filtered searches in parallel.
    let q = query.to_string();
    let (songs_res, albums_res, artists_res) = tokio::join!(
        async {
            let r: Result<Vec<ytmapi_rs::parse::SearchResultSong>, _> =
                yt_query!(client, SearchQuery::new(&q).with_filter(SongsFilter))
                    .map_err(|e| format!("Song search failed: {e}"));
            r
        },
        async {
            let r: Result<Vec<ytmapi_rs::parse::SearchResultAlbum>, _> =
                yt_query!(client, SearchQuery::new(&q).with_filter(AlbumsFilter))
                    .map_err(|e| format!("Album search failed: {e}"));
            r
        },
        async {
            let r: Result<Vec<ytmapi_rs::parse::SearchResultArtist>, _> =
                yt_query!(client, SearchQuery::new(&q).with_filter(ArtistsFilter))
                    .map_err(|e| format!("Artist search failed: {e}"));
            r
        },
    );

    // Collect results, tolerating individual failures.
    let tracks: Vec<YtTrack> = songs_res
        .unwrap_or_else(|e| { log::warn!("[youtube] {e}"); Vec::new() })
        .into_iter()
        .map(convert_search_song)
        .collect();

    let albums: Vec<YtAlbumRef> = albums_res
        .unwrap_or_else(|e| { log::warn!("[youtube] {e}"); Vec::new() })
        .into_iter()
        .map(convert_search_album)
        .collect();

    let artists: Vec<YtArtistRef> = artists_res
        .unwrap_or_else(|e| { log::warn!("[youtube] {e}"); Vec::new() })
        .into_iter()
        .map(|a| YtArtistRef {
            browse_id: Some(a.browse_id.get_raw().to_string()),
            name: a.artist,
        })
        .collect();

    Ok(YtSearchResults {
        tracks,
        albums,
        artists,
        playlists: Vec::new(), // Playlist search can be added later if needed.
    })
}

/// Search YouTube Music filtered to songs only.
pub async fn search_songs(query: &str) -> Result<Vec<YtTrack>, String> {
    use ytmapi_rs::query::search::SongsFilter;

    let client = get_client().await?;
    let results: Vec<ytmapi_rs::parse::SearchResultSong> =
        yt_query!(client, SearchQuery::new(query).with_filter(SongsFilter))
            .map_err(|e| format!("YouTube Music song search failed: {e}"))?;

    Ok(results.into_iter().map(convert_search_song).collect())
}

// ---------------------------------------------------------------------------
// Album
// ---------------------------------------------------------------------------

/// Get a full album with track listing.
pub async fn get_album(browse_id: &str) -> Result<YtAlbum, String> {
    let client = get_client().await?;
    let album: ytmapi_rs::parse::GetAlbum =
        yt_query!(client, GetAlbumQuery::new(AlbumID::from_raw(browse_id)))
            .map_err(|e| format!("Failed to get album: {e}"))?;

    let thumbnail_url = album.thumbnails.first().map(|t| t.url.clone());

    let tracks: Vec<YtTrack> = album
        .tracks
        .into_iter()
        .map(|t| {
            let duration_ms = parse_duration_str(&t.duration);
            YtTrack {
                video_id: t.video_id.get_raw().to_string(),
                title: t.title,
                artists: album
                    .artists
                    .iter()
                    .map(|a| YtArtistRef {
                        browse_id: a.id.as_ref().map(|id| id.get_raw().to_string()),
                        name: a.name.clone(),
                    })
                    .collect(),
                album: Some(YtAlbumRefSimple {
                    browse_id: browse_id.to_string(),
                    name: album.title.clone(),
                }),
                duration: Some(t.duration.clone()),
                duration_ms,
                thumbnail_url: thumbnail_url.clone(),
                explicit: t.explicit == ytmapi_rs::common::Explicit::IsExplicit,
            }
        })
        .collect();

    Ok(YtAlbum {
        browse_id: browse_id.to_string(),
        title: album.title,
        artists: album
            .artists
            .into_iter()
            .map(|a| YtArtistRef {
                browse_id: a.id.map(|id| id.get_raw().to_string()),
                name: a.name,
            })
            .collect(),
        year: Some(album.year),
        tracks,
        thumbnail_url,
        album_type: Some(format!("{:?}", album.category)),
        duration: Some(album.duration),
    })
}

// ---------------------------------------------------------------------------
// Artist
// ---------------------------------------------------------------------------

/// Get an artist's page.
pub async fn get_artist(browse_id: &str) -> Result<YtArtist, String> {
    let client = get_client().await?;
    let artist: ytmapi_rs::parse::GetArtist =
        yt_query!(client, GetArtistQuery::new(ArtistChannelID::from_raw(browse_id)))
            .map_err(|e| format!("Failed to get artist: {e}"))?;

    let thumbnail_url = artist.thumbnails.first().map(|t| t.url.clone());

    let albums = convert_artist_albums(&artist.top_releases.albums);
    let singles = convert_artist_albums(&artist.top_releases.singles);

    Ok(YtArtist {
        browse_id: browse_id.to_string(),
        name: artist.name,
        thumbnail_url,
        description: artist.description,
        subscribers: artist.subscribers,
        albums,
        singles,
    })
}

// ---------------------------------------------------------------------------
// Playlist
// ---------------------------------------------------------------------------

/// Get a playlist's tracks.
pub async fn get_playlist(browse_id: &str) -> Result<YtPlaylistDetail, String> {
    let client = get_client().await?;
    let pid = PlaylistID::from_raw(browse_id);

    let details: ytmapi_rs::parse::GetPlaylistDetails =
        yt_query!(client, GetPlaylistDetailsQuery::new(pid))
            .map_err(|e| format!("Failed to get playlist details: {e}"))?;

    let items: Vec<ytmapi_rs::parse::PlaylistItem> =
        yt_query!(client, GetPlaylistTracksQuery::new(PlaylistID::from_raw(browse_id)))
            .map_err(|e| format!("Failed to get playlist tracks: {e}"))?;

    let thumbnail_url = details.thumbnails.first().map(|t| t.url.clone());

    let tracks: Vec<YtTrack> = items
        .into_iter()
        .filter_map(|item| match item {
            ytmapi_rs::parse::PlaylistItem::Song(song) => {
                let duration_ms = parse_duration_str(&song.duration);
                Some(YtTrack {
                    video_id: song.video_id.get_raw().to_string(),
                    title: song.title,
                    artists: song
                        .artists
                        .into_iter()
                        .map(|a| YtArtistRef {
                            browse_id: a.id.map(|id| id.get_raw().to_string()),
                            name: a.name,
                        })
                        .collect(),
                    album: Some(YtAlbumRefSimple {
                        browse_id: song.album.id.get_raw().to_string(),
                        name: song.album.name,
                    }),
                    duration: Some(song.duration),
                    duration_ms,
                    thumbnail_url: song.thumbnails.first().map(|t| t.url.clone()),
                    explicit: song.explicit == ytmapi_rs::common::Explicit::IsExplicit,
                })
            }
            _ => None,
        })
        .collect();

    Ok(YtPlaylistDetail {
        info: YtPlaylist {
            browse_id: browse_id.to_string(),
            title: details.title,
            author: Some(details.author),
            track_count: Some(details.track_count_text),
            thumbnail_url,
        },
        tracks,
    })
}

// ---------------------------------------------------------------------------
// Search suggestions
// ---------------------------------------------------------------------------

/// Get search suggestions for autocomplete.
pub async fn get_search_suggestions(query: &str) -> Result<Vec<String>, String> {
    let client = get_client().await?;
    let suggestions =
        yt_query!(client, GetSearchSuggestionsQuery::new(query))
            .map_err(|e| format!("Failed to get search suggestions: {e}"))?;

    Ok(suggestions.into_iter().map(|s| s.get_text()).collect())
}

// ---------------------------------------------------------------------------
// Type conversion helpers
// ---------------------------------------------------------------------------

fn convert_search_song(s: ytmapi_rs::parse::SearchResultSong) -> YtTrack {
    let duration_ms = parse_duration_str(&s.duration);
    YtTrack {
        video_id: s.video_id.get_raw().to_string(),
        title: s.title,
        artists: vec![YtArtistRef {
            browse_id: None,
            name: s.artist,
        }],
        album: s.album.map(|a| YtAlbumRefSimple {
            browse_id: a.id.get_raw().to_string(),
            name: a.name,
        }),
        duration: Some(s.duration),
        duration_ms,
        thumbnail_url: s.thumbnails.first().map(|t| t.url.clone()),
        explicit: s.explicit == ytmapi_rs::common::Explicit::IsExplicit,
    }
}

fn convert_search_album(a: ytmapi_rs::parse::SearchResultAlbum) -> YtAlbumRef {
    YtAlbumRef {
        browse_id: a.album_id.get_raw().to_string(),
        name: a.title,
        thumbnail_url: a.thumbnails.first().map(|t| t.url.clone()),
        year: Some(a.year),
        artists: vec![YtArtistRef {
            browse_id: None,
            name: a.artist,
        }],
        album_type: Some(format!("{:?}", a.album_type)),
    }
}

fn convert_artist_albums(
    section: &Option<ytmapi_rs::parse::GetArtistAlbums>,
) -> Vec<YtAlbumRef> {
    let Some(section) = section else {
        return Vec::new();
    };
    section
        .results
        .iter()
        .map(|a| YtAlbumRef {
            browse_id: a.album_id.get_raw().to_string(),
            name: a.title.clone(),
            thumbnail_url: a.thumbnails.first().map(|t| t.url.clone()),
            year: Some(a.year.clone()),
            artists: Vec::new(),
            album_type: None,
        })
        .collect()
}

fn parse_duration_str(s: &str) -> Option<u64> {
    let parts: Vec<&str> = s.split(':').collect();
    match parts.len() {
        2 => {
            let mins: u64 = parts[0].trim().parse().ok()?;
            let secs: u64 = parts[1].trim().parse().ok()?;
            Some((mins * 60 + secs) * 1000)
        }
        3 => {
            let hours: u64 = parts[0].trim().parse().ok()?;
            let mins: u64 = parts[1].trim().parse().ok()?;
            let secs: u64 = parts[2].trim().parse().ok()?;
            Some((hours * 3600 + mins * 60 + secs) * 1000)
        }
        _ => None,
    }
}
