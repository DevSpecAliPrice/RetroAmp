//! Spotify authentication — OAuth2 PKCE flow and token management.
//!
//! Architecture: The OAuth token is the single source of truth for "connected"
//! state. We don't maintain a persistent librespot Session — sessions are
//! created on-demand when playing a track.
//!
//! Flow:
//! 1. Login: OAuth PKCE → access_token + refresh_token → stored in SpotifyPlayer
//! 2. Auto-reconnect on launch: load refresh_token from disk → refresh → stored
//! 3. Playing a track: fresh Session created with access_token, used for that track

use std::path::Path;

use librespot::core::cache::Cache;
use librespot::oauth::OAuthClientBuilder;

/// The OAuth scopes RetroAmp needs.
const SCOPES: &[&str] = &[
    "streaming",
    "user-read-email",
    "user-read-private",
    "user-library-read",
    "user-library-modify",
    "playlist-read-private",
    "playlist-read-collaborative",
    "user-read-recently-played",
];

/// Local callback URI for the OAuth PKCE flow.
const REDIRECT_URI: &str = "http://127.0.0.1:8898/login";

/// Result of a successful OAuth flow.
pub struct LoginResult {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: std::time::Instant,
}

/// RetroAmp's registered Spotify Developer App client ID.
/// This is a public identifier (not a secret) — safe to embed in source code.
/// OAuth2 PKCE doesn't use the client_secret, so there's no security concern.
const RETROAMP_CLIENT_ID: &str = "f0ec7821b0e14d138902b31c8acdf832";

/// Get the default Spotify client ID for RetroAmp.
pub fn default_client_id() -> String {
    RETROAMP_CLIENT_ID.to_string()
}

/// Run the OAuth2 PKCE browser flow. Blocking — waits for the user to
/// complete authorization in their browser.
pub fn get_oauth_token(client_id: &str) -> Result<LoginResult, String> {
    log::info!("starting Spotify OAuth login flow");

    let oauth_client = OAuthClientBuilder::new(
        client_id,
        REDIRECT_URI,
        SCOPES.to_vec(),
    )
    .open_in_browser()
    .with_custom_message("You can close this tab and return to RetroAmp.")
    .build()
    .map_err(|e| format!("Failed to build OAuth client: {e}"))?;

    log::info!("OAuth client built, waiting for browser callback...");

    let token = oauth_client
        .get_access_token()
        .map_err(|e| format!("OAuth login failed: {e}"))?;

    log::info!("OAuth token received successfully");

    Ok(LoginResult {
        access_token: token.access_token,
        refresh_token: token.refresh_token,
        expires_at: token.expires_at,
    })
}

/// Refresh an expired access token using a saved refresh token.
pub fn refresh_access_token(
    client_id: &str,
    refresh_token: &str,
) -> Result<LoginResult, String> {
    log::info!("refreshing Spotify OAuth token...");

    let oauth_client = OAuthClientBuilder::new(
        client_id,
        REDIRECT_URI,
        SCOPES.to_vec(),
    )
    .build()
    .map_err(|e| format!("Failed to build OAuth client for refresh: {e}"))?;

    let token = oauth_client
        .refresh_token(refresh_token)
        .map_err(|e| format!("Token refresh failed: {e}"))?;

    log::info!("OAuth token refreshed successfully");

    Ok(LoginResult {
        access_token: token.access_token,
        refresh_token: token.refresh_token,
        expires_at: token.expires_at,
    })
}

/// Create a librespot Cache instance for audio file caching.
pub fn create_cache(cache_dir: &Path) -> Option<Cache> {
    Cache::new(
        Some(cache_dir.to_path_buf()),
        Some(cache_dir.to_path_buf()),
        Some(cache_dir.join("audio")),
        Some(512 * 1024 * 1024),
    )
    .map_err(|e| log::warn!("failed to create Spotify cache at {}: {e}", cache_dir.display()))
    .ok()
}

/// Save the OAuth refresh token to disk.
pub fn save_refresh_token(cache_dir: &Path, refresh_token: &str) {
    let path = cache_dir.join("refresh_token");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(&path, refresh_token) {
        log::warn!("failed to save refresh token: {e}");
    }
}

/// Load the saved OAuth refresh token.
pub fn load_refresh_token(cache_dir: &Path) -> Option<String> {
    let path = cache_dir.join("refresh_token");
    std::fs::read_to_string(&path).ok().filter(|s| !s.is_empty())
}

/// Clear all cached credentials (for logout).
pub fn clear_cached_credentials(cache_dir: &Path) {
    for name in &["credentials.json", "refresh_token"] {
        let path = cache_dir.join(name);
        if path.exists() {
            let _ = std::fs::remove_file(&path);
        }
    }
}
