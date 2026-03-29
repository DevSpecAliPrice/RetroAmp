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
// Direct InnerTube API client for library queries
// ---------------------------------------------------------------------------

const YTM_API_URL: &str = "https://music.youtube.com/youtubei/v1/browse";
const YTM_API_KEY: &str = "AIzaSyC9XL3ZjWddXya6X74dJoCTL-WEYFDNX30";
const YTM_ORIGIN: &str = "https://music.youtube.com";

/// Make a direct InnerTube browse request with proper channel/profile support.
///
/// This bypasses ytmapi-rs for library queries because:
/// 1. ytmapi-rs hardcodes `X-Goog-AuthUser: 0`
/// 2. ytmapi-rs doesn't send `X-Goog-PageId` which is required for
///    accounts with Brand Accounts / delegated channels
///
/// We extract the `DATASYNC_ID` from music.youtube.com on login and use
/// the first part as `X-Goog-PageId` to identify the correct channel.
fn innertube_browse(browse_id: &str, cookie: &str) -> Result<serde_json::Value, String> {
    let cfg = crate::config::AppConfig::load();
    let auth_user = cfg.youtube.auth_user.unwrap_or(0).to_string();

    // Extract SAPISID from cookie for auth header.
    let sapisid = cookie
        .split(';')
        .find_map(|part| {
            let part = part.trim();
            if part.starts_with("SAPISID=") {
                Some(part["SAPISID=".len()..].to_string())
            } else {
                None
            }
        })
        .ok_or_else(|| "SAPISID not found in cookie".to_string())?;

    // Generate SAPISIDHASH: SHA1(timestamp SAPISID origin)
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let hash_input = format!("{timestamp} {sapisid} {YTM_ORIGIN}");
    let hash = compute_sha1(&hash_input);
    let auth_header = format!("SAPISIDHASH {timestamp}_{hash}");

    let body = serde_json::json!({
        "context": {
            "client": {
                "clientName": "WEB_REMIX",
                "clientVersion": "1.20260324.01.00",
                "hl": "en",
                "gl": "GB",
            }
        },
        "browseId": browse_id,
    });

    let url = format!("{YTM_API_URL}?alt=json&prettyPrint=false&key={YTM_API_KEY}");

    // Extract X-Goog-PageId from stored DATASYNC_ID.
    // The first part (before ||) identifies the channel for library access.
    let page_id = cfg.youtube.datasync_id.as_deref().and_then(|dsid| {
        dsid.split("||").next().filter(|s| !s.is_empty())
    });

    let body_str = serde_json::to_string(&body)
        .map_err(|e| format!("Failed to serialize request: {e}"))?;

    let mut request = ureq::post(&url)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:88.0) Gecko/20100101 Firefox/88.0")
        .header("Cookie", cookie)
        .header("Authorization", &auth_header)
        .header("X-Goog-AuthUser", &auth_user)
        .header("X-Origin", YTM_ORIGIN)
        .header("Origin", YTM_ORIGIN)
        .header("Referer", YTM_ORIGIN)
        .header("Content-Type", "application/json");

    if let Some(pid) = page_id {
        request = request.header("X-Goog-PageId", pid);
    }

    let response = request
        .send(&body_str)
        .map_err(|e| format!("InnerTube browse request failed: {e}"))?;

    let body_str = response
        .into_body()
        .read_to_string()
        .map_err(|e| format!("Failed to read InnerTube response: {e}"))?;

    serde_json::from_str(&body_str)
        .map_err(|e| format!("Failed to parse InnerTube JSON: {e}"))
}

/// Compute SHA-1 hash hex string (used for SAPISIDHASH auth).
fn compute_sha1(input: &str) -> String {
    use sha1::Digest;
    let mut hasher = sha1::Sha1::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    result.iter().map(|b| format!("{b:02x}")).collect()
}

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

impl YtClient {
    /// Run a library query (requires BrowserToken / LoggedIn).
    /// Returns an error if the client is not authenticated.
    async fn library_query<Q>(&self, q: Q) -> Result<Q::Output, String>
    where
        Q: ytmapi_rs::query::Query<BrowserToken>,
    {
        match self {
            YtClient::Browser(c) => c.query(q).await.map_err(|e| format!("{e}")),
            YtClient::NoAuth(_) => Err("Not logged in — library queries require authentication".into()),
        }
    }

    /// Run a library query and return the raw JSON string (pre-parsing).
    /// Use this when ytmapi-rs's parser fails due to YouTube response changes.
    async fn library_raw_json<Q>(&self, q: Q) -> Result<String, String>
    where
        Q: ytmapi_rs::query::Query<BrowserToken>,
    {
        match self {
            YtClient::Browser(c) => c.raw_json_query(q).await.map_err(|e| format!("{e}")),
            YtClient::NoAuth(_) => Err("Not logged in — library queries require authentication".into()),
        }
    }
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

    let thumbnail_url = best_thumbnail(&album.thumbnails);

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

    let thumbnail_url = best_thumbnail(&artist.thumbnails);

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
///
/// Uses direct InnerTube API when authenticated (for X-Goog-PageId support),
/// falls back to ytmapi-rs for unauthenticated browsing of public playlists.
pub async fn get_playlist(browse_id: &str) -> Result<YtPlaylistDetail, String> {
    let cfg = crate::config::AppConfig::load();

    // For authenticated users, use direct InnerTube (supports Brand Accounts).
    if let Some(ref cookie) = cfg.youtube.cookie {
        // Playlist browse IDs need "VL" prefix.
        let vl_id = if browse_id.starts_with("VL") {
            browse_id.to_string()
        } else {
            format!("VL{browse_id}")
        };

        let json = innertube_browse(&vl_id, cookie)?;
        let tracks = extract_tracks_from_browse_response(&json);

        // Extract playlist title and thumbnail by searching recursively
        // for musicResponsiveHeaderRenderer (YouTube changes the path).
        let (title, thumbnail_url) = extract_playlist_header(&json, browse_id);

        log::info!("[youtube] playlist {browse_id} (direct InnerTube): {title}, {} tracks", tracks.len());

        return Ok(YtPlaylistDetail {
            info: YtPlaylist {
                browse_id: browse_id.to_string(),
                title,
                author: None,
                track_count: Some(format!("{} tracks", tracks.len())),
                thumbnail_url,
            },
            tracks,
        });
    }

    // Unauthenticated fallback: use ytmapi-rs.
    let client = get_client().await?;
    let pid = PlaylistID::from_raw(browse_id);

    let details: ytmapi_rs::parse::GetPlaylistDetails =
        yt_query!(client, GetPlaylistDetailsQuery::new(pid))
            .map_err(|e| format!("Failed to get playlist details: {e}"))?;

    let items: Vec<ytmapi_rs::parse::PlaylistItem> =
        yt_query!(client, GetPlaylistTracksQuery::new(PlaylistID::from_raw(browse_id)))
            .map_err(|e| format!("Failed to get playlist tracks: {e}"))?;

    let thumbnail_url = best_thumbnail(&details.thumbnails);

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
                    thumbnail_url: best_thumbnail(&song.thumbnails),
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
// Library (authenticated only)
// ---------------------------------------------------------------------------

/// Get the user's liked songs.
///
/// YouTube Music stores liked songs in the "Liked Music" auto-playlist.
/// OuterTune accesses this as browse ID "VLLM" (VL prefix + LM playlist ID).
/// The "FEmusic_liked_videos" browse page is a DIFFERENT thing (library songs).
pub async fn get_library_songs() -> Result<Vec<YtTrack>, String> {
    let cfg = crate::config::AppConfig::load();
    let cookie = cfg.youtube.cookie.as_deref()
        .ok_or("Not logged in — liked songs require authentication")?;

    // "VLLM" = VL (playlist prefix) + LM (Liked Music auto-playlist).
    // This is how OuterTune fetches liked songs.
    let json = innertube_browse("VLLM", cookie)?;

    // Dump for debugging.
    if let Some(cache_dir) = dirs::cache_dir() {
        let dump_path = cache_dir.join("retroamp").join("yt_VLLM_debug.json");
        let _ = std::fs::create_dir_all(dump_path.parent().unwrap());
        let _ = std::fs::write(&dump_path, serde_json::to_string(&json).unwrap_or_default());
    }

    let tracks = extract_tracks_from_browse_response(&json);
    log::info!("[youtube] liked songs (VLLM): {} tracks", tracks.len());

    if tracks.is_empty() {
        let mut messages = Vec::new();
        find_messages(&json, &mut messages);
        if !messages.is_empty() {
            log::info!("[youtube] liked songs messages: {messages:?}");
            return Err(messages.join("; "));
        }
    }

    Ok(tracks)
}

/// Fetch tracks from any browse ID using raw JSON.
/// Works for library browse IDs (FEmusic_*), playlist IDs, and auto-playlists.
async fn browse_tracks_raw(browse_id: &str) -> Result<Vec<YtTrack>, String> {
    let client = get_client().await?;

    // Use GetPlaylistDetailsQuery as a generic browse — it sends a browse
    // request with the given ID, which works for any browse endpoint.
    let json_str: String = match &client {
        YtClient::Browser(c) => c
            .raw_json_query(GetPlaylistTracksQuery::new(
                PlaylistID::from_raw(browse_id),
            ))
            .await
            .map_err(|e| format!("{e}"))?,
        YtClient::NoAuth(c) => c
            .raw_json_query(GetPlaylistTracksQuery::new(
                PlaylistID::from_raw(browse_id),
            ))
            .await
            .map_err(|e| format!("{e}"))?,
    };

    let json: serde_json::Value = serde_json::from_str(&json_str)
        .map_err(|e| format!("Failed to parse browse JSON: {e}"))?;

    // Dump for debugging if cache dir available.
    if let Some(cache_dir) = dirs::cache_dir() {
        let safe_id = browse_id.replace('/', "_");
        let dump_path = cache_dir
            .join("retroamp")
            .join(format!("yt_browse_{safe_id}_debug.json"));
        let _ = std::fs::create_dir_all(dump_path.parent().unwrap());
        let _ = std::fs::write(&dump_path, &json_str);
    }

    let tracks = extract_tracks_from_browse_response(&json);
    log::info!(
        "[youtube] browse {browse_id}: extracted {} tracks from raw JSON",
        tracks.len()
    );

    if tracks.is_empty() {
        let mut messages = Vec::new();
        find_messages(&json, &mut messages);
        if !messages.is_empty() {
            log::warn!("[youtube] browse {browse_id} messages: {messages:?}");
            return Err(messages.join("; "));
        }
    }

    Ok(tracks)
}

/// Get the user's playlists via direct InnerTube.
pub async fn get_library_playlists() -> Result<Vec<YtPlaylist>, String> {
    let cfg = crate::config::AppConfig::load();
    let cookie = cfg.youtube.cookie.as_deref()
        .ok_or("Not logged in — playlists require authentication")?;

    let json = innertube_browse("FEmusic_liked_playlists", cookie)?;
    let playlists = extract_playlists_from_browse_response(&json);
    log::info!("[youtube] library playlists (direct InnerTube): {} found", playlists.len());

    if playlists.is_empty() {
        let mut messages = Vec::new();
        find_messages(&json, &mut messages);
        if !messages.is_empty() {
            return Err(messages.join("; "));
        }
    }

    Ok(playlists)
}

/// Get the user's listening history via direct InnerTube.
pub async fn get_history() -> Result<Vec<YtTrack>, String> {
    let cfg = crate::config::AppConfig::load();
    let cookie = cfg.youtube.cookie.as_deref()
        .ok_or("Not logged in — history requires authentication")?;

    let json = innertube_browse("FEmusic_history", cookie)?;
    let tracks = extract_tracks_from_browse_response(&json);
    log::info!("[youtube] history (direct InnerTube): {} tracks", tracks.len());

    if tracks.is_empty() {
        let mut messages = Vec::new();
        find_messages(&json, &mut messages);
        if !messages.is_empty() {
            return Err(messages.join("; "));
        }
    }

    Ok(tracks)
}

// ---------------------------------------------------------------------------
// Raw JSON extraction helpers
// ---------------------------------------------------------------------------

/// Extract text from messageRenderer nodes (for error/empty state messages).
fn find_messages(value: &serde_json::Value, messages: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(renderer) = map.get("messageRenderer") {
                // Extract text from the message.
                if let Some(text) = renderer
                    .pointer("/text/runs")
                    .and_then(|v| v.as_array())
                    .map(|runs| {
                        runs.iter()
                            .filter_map(|r| r.get("text").and_then(|v| v.as_str()))
                            .collect::<Vec<_>>()
                            .join("")
                    })
                    .or_else(|| {
                        renderer
                            .pointer("/text/simpleText")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                    })
                {
                    if !text.is_empty() {
                        messages.push(text);
                    }
                }
                // Also check subtext.
                if let Some(sub) = renderer
                    .pointer("/subtext/runs")
                    .and_then(|v| v.as_array())
                    .map(|runs| {
                        runs.iter()
                            .filter_map(|r| r.get("text").and_then(|v| v.as_str()))
                            .collect::<Vec<_>>()
                            .join("")
                    })
                {
                    if !sub.is_empty() {
                        messages.push(sub);
                    }
                }
            }
            for v in map.values() {
                find_messages(v, messages);
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr {
                find_messages(v, messages);
            }
        }
        _ => {}
    }
}

/// Find all renderer type keys in the JSON (for debugging).
fn find_renderer_types(value: &serde_json::Value, types: &mut Vec<String>) {
    if let serde_json::Value::Object(map) = value {
        for key in map.keys() {
            if key.ends_with("Renderer") && !types.contains(key) {
                types.push(key.clone());
            }
        }
        for v in map.values() {
            find_renderer_types(v, types);
        }
    } else if let serde_json::Value::Array(arr) = value {
        for v in arr {
            find_renderer_types(v, types);
        }
    }
}

/// Extract playlist title and thumbnail from a browse response.
/// Searches recursively for `musicResponsiveHeaderRenderer` or
/// `musicDetailHeaderRenderer` which contain the playlist header.
fn extract_playlist_header(json: &serde_json::Value, fallback_id: &str) -> (String, Option<String>) {
    let mut title = fallback_id.to_string();
    let mut thumbnail_url = None;

    fn find_header(v: &serde_json::Value, title: &mut String, thumb: &mut Option<String>) {
        match v {
            serde_json::Value::Object(map) => {
                for key in ["musicResponsiveHeaderRenderer", "musicDetailHeaderRenderer", "musicImmersiveHeaderRenderer"] {
                    if let Some(header) = map.get(key) {
                        if let Some(t) = header
                            .pointer("/title/runs/0/text")
                            .and_then(|v| v.as_str())
                        {
                            *title = t.to_string();
                        }
                        if let Some(thumbs) = header
                            .pointer("/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails")
                            .and_then(|v| v.as_array())
                        {
                            if let Some(t) = thumbs.last().and_then(|t| t.get("url")?.as_str()) {
                                *thumb = Some(normalize_thumbnail_url(t));
                            }
                        }
                        return;
                    }
                }
                for val in map.values() {
                    find_header(val, title, thumb);
                }
            }
            serde_json::Value::Array(arr) => {
                for val in arr {
                    find_header(val, title, thumb);
                }
            }
            _ => {}
        }
    }

    find_header(json, &mut title, &mut thumbnail_url);
    (title, thumbnail_url)
}

/// Extract tracks from a YouTube Music browse response JSON.
///
/// Walks the JSON tree looking for `musicResponsiveListItemRenderer` nodes
/// which contain song data. This is resilient to structural changes because
/// we search recursively rather than following a fixed path.
fn extract_tracks_from_browse_response(json: &serde_json::Value) -> Vec<YtTrack> {
    let mut tracks = Vec::new();
    find_song_renderers(json, &mut tracks);
    tracks
}

/// Recursively find `musicResponsiveListItemRenderer` objects and extract song data.
fn find_song_renderers(value: &serde_json::Value, tracks: &mut Vec<YtTrack>) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(renderer) = map.get("musicResponsiveListItemRenderer") {
                if let Some(track) = parse_song_renderer(renderer) {
                    tracks.push(track);
                }
            }
            // Recurse into all object values.
            for v in map.values() {
                find_song_renderers(v, tracks);
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr {
                find_song_renderers(v, tracks);
            }
        }
        _ => {}
    }
}

/// Parse a single `musicResponsiveListItemRenderer` into a YtTrack.
fn parse_song_renderer(renderer: &serde_json::Value) -> Option<YtTrack> {
    // Extract video ID from playlistItemData or overlay menu.
    let video_id = renderer
        .pointer("/playlistItemData/videoId")
        .or_else(|| renderer.pointer("/overlay/musicItemThumbnailOverlayRenderer/content/musicPlayButtonRenderer/playNavigationEndpoint/watchEndpoint/videoId"))
        .and_then(|v| v.as_str())?
        .to_string();

    // Extract flex columns — typically [title, artist/album, duration].
    let columns = renderer
        .pointer("/flexColumns")
        .and_then(|v| v.as_array())?;

    let get_column_text = |idx: usize| -> Option<String> {
        columns
            .get(idx)?
            .pointer("/musicResponsiveListItemFlexColumnRenderer/text/runs/0/text")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    };

    let get_column_browse_id = |idx: usize| -> Option<String> {
        columns
            .get(idx)?
            .pointer("/musicResponsiveListItemFlexColumnRenderer/text/runs/0/navigationEndpoint/browseEndpoint/browseId")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    };

    let title = get_column_text(0)?;
    let artist_name = get_column_text(1).unwrap_or_default();
    let artist_browse_id = get_column_browse_id(1);

    // Album info is often in the second column's additional runs.
    let album_name = columns
        .get(1)
        .and_then(|c| c.pointer("/musicResponsiveListItemFlexColumnRenderer/text/runs"))
        .and_then(|runs| runs.as_array())
        .and_then(|runs| {
            // Find the run that navigates to an album (browseEndpoint with page type MUSIC_PAGE_TYPE_ALBUM).
            runs.iter().find_map(|run| {
                let browse_id = run.pointer("/navigationEndpoint/browseEndpoint/browseId")
                    .and_then(|v| v.as_str())?;
                if browse_id.starts_with("MPR") {
                    Some((run.get("text")?.as_str()?.to_string(), browse_id.to_string()))
                } else {
                    None
                }
            })
        });

    // Duration — check fixed columns or the last flex column.
    let duration = renderer
        .pointer("/fixedColumns/0/musicResponsiveListItemFixedColumnRenderer/text/runs/0/text")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| get_column_text(2));

    let duration_ms = duration.as_deref().and_then(|s| parse_duration_str(s));

    // Thumbnail.
    let thumbnail_url = renderer
        .pointer("/thumbnail/musicThumbnailRenderer/thumbnail/thumbnails")
        .and_then(|v| v.as_array())
        .and_then(|arr| {
            arr.iter()
                .filter_map(|t| {
                    let url = t.get("url")?.as_str()?;
                    let w = t.get("width").and_then(|v| v.as_u64()).unwrap_or(0);
                    Some((url.to_string(), w))
                })
                .filter(|(_, w)| *w >= 60)
                .min_by_key(|(_, w)| *w)
                .or_else(|| {
                    arr.last().and_then(|t| {
                        Some((t.get("url")?.as_str()?.to_string(), 0))
                    })
                })
                .map(|(url, _)| normalize_thumbnail_url(&url))
        });

    Some(YtTrack {
        video_id,
        title,
        artists: vec![YtArtistRef {
            browse_id: artist_browse_id,
            name: artist_name,
        }],
        album: album_name.map(|(name, browse_id)| YtAlbumRefSimple { browse_id, name }),
        duration,
        duration_ms,
        thumbnail_url,
        explicit: false,
    })
}

/// Extract playlists from a YouTube Music browse response JSON.
fn extract_playlists_from_browse_response(json: &serde_json::Value) -> Vec<YtPlaylist> {
    let mut playlists = Vec::new();
    find_playlist_renderers(json, &mut playlists);
    playlists
}

fn find_playlist_renderers(value: &serde_json::Value, playlists: &mut Vec<YtPlaylist>) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(renderer) = map.get("musicTwoRowItemRenderer") {
                if let Some(pl) = parse_playlist_renderer(renderer) {
                    playlists.push(pl);
                }
            }
            for v in map.values() {
                find_playlist_renderers(v, playlists);
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr {
                find_playlist_renderers(v, playlists);
            }
        }
        _ => {}
    }
}

fn parse_playlist_renderer(renderer: &serde_json::Value) -> Option<YtPlaylist> {
    let browse_id = renderer
        .pointer("/navigationEndpoint/browseEndpoint/browseId")
        .and_then(|v| v.as_str())?
        .to_string();

    // Exclude non-playlist items (albums start with "MPR", artists with "UC").
    if browse_id.starts_with("MPR") || browse_id.starts_with("UC") {
        return None;
    }

    let title = renderer
        .pointer("/title/runs/0/text")
        .and_then(|v| v.as_str())?
        .to_string();

    let subtitle = renderer
        .pointer("/subtitle/runs")
        .and_then(|v| v.as_array())
        .map(|runs| {
            runs.iter()
                .filter_map(|r| r.get("text").and_then(|v| v.as_str()))
                .collect::<Vec<_>>()
                .join("")
        });

    let thumbnail_url = renderer
        .pointer("/thumbnailRenderer/musicThumbnailRenderer/thumbnail/thumbnails")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.last())
        .and_then(|t| t.get("url")?.as_str().map(|s| normalize_thumbnail_url(s)));

    Some(YtPlaylist {
        browse_id,
        title,
        author: subtitle.clone(),
        track_count: None,
        thumbnail_url,
    })
}

// ---------------------------------------------------------------------------
// Type conversion helpers
// ---------------------------------------------------------------------------

fn convert_table_list_song(s: ytmapi_rs::parse::TableListSong) -> YtTrack {
    let duration_ms = parse_duration_str(&s.duration);
    YtTrack {
        video_id: s.video_id.get_raw().to_string(),
        title: s.title,
        artists: s
            .artists
            .into_iter()
            .map(|a| YtArtistRef {
                browse_id: a.id.map(|id| id.get_raw().to_string()),
                name: a.name,
            })
            .collect(),
        album: Some(YtAlbumRefSimple {
            browse_id: s.album.id.get_raw().to_string(),
            name: s.album.name,
        }),
        duration: Some(s.duration),
        duration_ms,
        thumbnail_url: best_thumbnail(&s.thumbnails),
        explicit: s.explicit == ytmapi_rs::common::Explicit::IsExplicit,
    }
}

fn convert_history_song(s: ytmapi_rs::parse::HistoryItemSong) -> YtTrack {
    let duration_ms = parse_duration_str(&s.duration);
    YtTrack {
        video_id: s.video_id.get_raw().to_string(),
        title: s.title,
        artists: s
            .artists
            .into_iter()
            .map(|a| YtArtistRef {
                browse_id: a.id.map(|id| id.get_raw().to_string()),
                name: a.name,
            })
            .collect(),
        album: Some(YtAlbumRefSimple {
            browse_id: s.album.id.get_raw().to_string(),
            name: s.album.name,
        }),
        duration: Some(s.duration),
        duration_ms,
        thumbnail_url: best_thumbnail(&s.thumbnails),
        explicit: s.explicit == ytmapi_rs::common::Explicit::IsExplicit,
    }
}

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
        thumbnail_url: best_thumbnail(&s.thumbnails),
        explicit: s.explicit == ytmapi_rs::common::Explicit::IsExplicit,
    }
}

fn convert_search_album(a: ytmapi_rs::parse::SearchResultAlbum) -> YtAlbumRef {
    YtAlbumRef {
        browse_id: a.album_id.get_raw().to_string(),
        name: a.title,
        thumbnail_url: best_thumbnail(&a.thumbnails),
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
            thumbnail_url: best_thumbnail(&a.thumbnails),
            year: Some(a.year.clone()),
            artists: Vec::new(),
            album_type: None,
        })
        .collect()
}

/// Pick the best thumbnail from a list and normalize its URL.
///
/// Prefers thumbnails around 226px wide (good for browser display at 2x scale).
/// Normalizes protocol-relative URLs (`//...`) to `https://...`.
fn best_thumbnail(thumbnails: &[ytmapi_rs::common::Thumbnail]) -> Option<String> {
    if thumbnails.is_empty() {
        return None;
    }
    // Pick the smallest thumbnail that's at least 120px wide, or the largest available.
    let thumb = thumbnails
        .iter()
        .filter(|t| t.width >= 120)
        .min_by_key(|t| t.width)
        .or_else(|| thumbnails.iter().max_by_key(|t| t.width))?;
    Some(normalize_thumbnail_url(&thumb.url))
}

/// Normalize a thumbnail URL — fix protocol-relative URLs and resize params.
fn normalize_thumbnail_url(url: &str) -> String {
    let mut url = url.to_string();
    // Fix protocol-relative URLs.
    if url.starts_with("//") {
        url = format!("https:{url}");
    }
    // YouTube thumbnail URLs often have `=w60-h60` or `=s60` size params.
    // Replace with a reasonable size for our UI.
    if let Some(idx) = url.rfind("=w") {
        url.truncate(idx);
        url.push_str("=w226-h226-l90-rj");
    } else if let Some(idx) = url.rfind("=s") {
        url.truncate(idx);
        url.push_str("=w226-h226-l90-rj");
    }
    url
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
