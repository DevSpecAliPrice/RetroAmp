//! Lightweight app configuration persisted as TOML in the platform config dir.
//!
//! Config file location:
//! - Linux:   `~/.config/retroamp/config.toml`
//! - macOS:   `~/Library/Application Support/retroamp/config.toml`
//! - Windows: `C:\Users\<user>\AppData\Roaming\retroamp\config.toml`
//!
//! On first load, if a legacy `config.json` exists it is automatically
//! migrated to TOML and the JSON file is renamed to `config.json.bak`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Top-level application configuration.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub skins: SkinConfig,

    #[serde(default)]
    pub eq: EqConfig,

    #[serde(default)]
    pub playback: PlaybackConfig,

    #[serde(default)]
    pub ui: UiConfig,

    #[serde(default)]
    pub library: LibraryConfig,

    #[serde(default)]
    pub radio: RadioConfig,

    #[serde(default)]
    pub general: GeneralConfig,

    #[serde(default)]
    pub spotify: SpotifyConfig,

    #[serde(default)]
    pub youtube: YouTubeConfig,
}

/// Skin-related preferences.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SkinConfig {
    /// Last-used skin path, restored on next launch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_skin_path: Option<String>,
}

/// Equalizer settings persisted across restarts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EqConfig {
    /// Per-band gains in dB (10 bands). Defaults to all zeros.
    #[serde(default)]
    pub gains: [f32; 10],
    /// Whether the EQ is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Preamp gain in dB.
    #[serde(default)]
    pub preamp: f32,
}

fn default_true() -> bool {
    true
}

impl Default for EqConfig {
    fn default() -> Self {
        Self {
            gains: [0.0; 10],
            enabled: true,
            preamp: 0.0,
        }
    }
}

/// Playback-related preferences.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybackConfig {
    /// What to do when playing from the library: "append", "replace", or "ask".
    #[serde(default = "default_append")]
    pub playlist_add_mode: String,
}

impl Default for PlaybackConfig {
    fn default() -> Self {
        Self {
            playlist_add_mode: "append".to_string(),
        }
    }
}

fn default_append() -> String {
    "append".to_string()
}

/// Library browser preferences.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LibraryConfig {
    /// Which columns are visible in the tracks view.
    /// If empty, defaults are used.
    #[serde(default)]
    pub visible_columns: Vec<String>,

    /// Active tab: "tracks", "artists", "albums", "genres".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_tab: Option<String>,

    /// Sort field for the tracks tab.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_by: Option<String>,

    /// Sort direction: "asc" or "desc".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_dir: Option<String>,

    /// Sort mode for browse tabs (artists/albums/genres): "name" or "count".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browse_sort_by: Option<String>,

    /// Per-column widths (unscaled pixels). Only present for columns the user has resized.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub column_widths: HashMap<String, f64>,
}

/// Radio browser preferences.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RadioConfig {
    /// Active tab: "favorites", "library", "discover".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_tab: Option<String>,

    /// Whether hidden stations are shown.
    #[serde(default)]
    pub show_hidden: bool,

    /// Per-column widths (unscaled pixels). Only present for columns the user has resized.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub column_widths: HashMap<String, f64>,
}

/// General application preferences.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GeneralConfig {
    /// Download folder for saved radio recordings.
    /// Falls back to the OS music directory if absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_dir: Option<String>,
}


/// Spotify integration preferences.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpotifyConfig {
    /// Spotify Developer App Client ID. Required for Web API access.
    /// Get one at https://developer.spotify.com/dashboard
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,

    /// Audio quality: "normal" (96kbps), "high" (160kbps), "very_high" (320kbps).
    #[serde(default = "default_spotify_quality")]
    pub quality: String,

    /// Device name shown in Spotify Connect device list.
    #[serde(default = "default_device_name")]
    pub device_name: String,

    /// Whether Spotify Connect is enabled (advertise as a playback device).
    #[serde(default)]
    pub connect_enabled: bool,

    /// Whether to apply Spotify's volume normalisation (ReplayGain).
    #[serde(default)]
    pub normalize_volume: bool,
}

fn default_spotify_quality() -> String {
    "very_high".to_string()
}

fn default_device_name() -> String {
    "RetroAmp".to_string()
}

impl Default for SpotifyConfig {
    fn default() -> Self {
        Self {
            client_id: None,
            quality: default_spotify_quality(),
            device_name: default_device_name(),
            connect_enabled: false,
            normalize_volume: false,
        }
    }
}

/// YouTube Music integration preferences.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YouTubeConfig {
    /// Audio quality preference. Controls the yt-dlp format selector:
    /// - "low":    worst audio (~48 kbps)
    /// - "medium": ~128 kbps (default)
    /// - "high":   best audio (~256 kbps)
    #[serde(default = "default_youtube_quality")]
    pub quality: String,

    /// Saved YouTube Music browser cookie for authenticated access.
    /// When present, the API client uses this to access personal library,
    /// liked songs, history, etc. Set to None for anonymous mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cookie: Option<String>,

    /// Google account index for multi-account users (X-Goog-AuthUser header).
    /// 0 = first account (default), 1 = second, etc.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_user: Option<u32>,

    /// DATASYNC_ID extracted from music.youtube.com during cookie login.
    /// The first part (before ||) is sent as X-Goog-PageId to identify
    /// the correct YouTube channel/profile for library access.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub datasync_id: Option<String>,

    /// Optional override for the yt-dlp binary path.
    /// If unset, RetroAmp uses the managed binary or system PATH.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ytdlp_path: Option<String>,
}

fn default_youtube_quality() -> String {
    "high".to_string()
}

impl Default for YouTubeConfig {
    fn default() -> Self {
        Self {
            quality: default_youtube_quality(),
            cookie: None,
            auth_user: None,
            datasync_id: None,
            ytdlp_path: None,
        }
    }
}

/// Spotify browser window preferences.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SpotifyBrowserConfig {
    /// Active tab: "home", "search", "library".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_tab: Option<String>,
}

/// UI layout persisted across restarts — window visibility, positions, sizes.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    /// Saved UI scale (1, 2, 3). If absent, auto-detected from screen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale: Option<u32>,

    #[serde(default)]
    pub main: WindowLayoutEntry,

    #[serde(default)]
    pub equalizer: WindowLayoutEntry,

    #[serde(default)]
    pub playlist: WindowLayoutEntry,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub radio_browser: Option<WindowLayoutEntry>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings: Option<WindowLayoutEntry>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library_browser: Option<WindowLayoutEntry>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spotify_browser: Option<WindowLayoutEntry>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub youtube_browser: Option<WindowLayoutEntry>,
}

/// Saved layout for a single window.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct WindowLayoutEntry {
    /// Whether this window was open when the app last closed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<bool>,
    /// Outer X position (logical pixels). Ignored on Wayland.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<i32>,
    /// Outer Y position (logical pixels). Ignored on Wayland.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<i32>,
    /// Inner width (logical pixels) — only meaningful for resizable windows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<f64>,
    /// Inner height (logical pixels) — only meaningful for resizable windows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f64>,
}

impl AppConfig {
    /// Load config from disk, returning defaults if the file doesn't exist yet.
    /// Automatically migrates from legacy JSON if needed.
    pub fn load() -> Self {
        // Migrate legacy JSON config if the TOML file doesn't exist yet.
        migrate_from_json();

        let Some(path) = config_path() else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Persist current config to disk.
    pub fn save(&self) -> Result<(), String> {
        let path = config_path().ok_or("could not determine config directory")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create config directory: {e}"))?;
        }
        let text = toml::to_string_pretty(self)
            .map_err(|e| format!("failed to serialize config: {e}"))?;
        std::fs::write(&path, text)
            .map_err(|e| format!("failed to write config: {e}"))?;
        Ok(())
    }

}

fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|c| c.join("retroamp").join("config.toml"))
}

fn legacy_json_path() -> Option<PathBuf> {
    dirs::config_dir().map(|c| c.join("retroamp").join("config.json"))
}

/// One-time migration: if `config.json` exists but `config.toml` does not,
/// read the JSON, convert to the new TOML structure, and rename the JSON
/// file to `.json.bak`.
fn migrate_from_json() {
    let Some(toml_path) = config_path() else { return };
    let Some(json_path) = legacy_json_path() else { return };

    // Only migrate if TOML is absent and JSON is present.
    if toml_path.exists() || !json_path.exists() {
        return;
    }

    log::info!("migrating config from JSON to TOML");

    // The legacy JSON struct matches the old flat layout.
    #[derive(Deserialize)]
    struct LegacyConfig {
        #[serde(default)]
        last_skin_path: Option<String>,
    }

    let Ok(json_text) = std::fs::read_to_string(&json_path) else { return };
    let Ok(legacy) = serde_json::from_str::<LegacyConfig>(&json_text) else { return };

    let new_config = AppConfig {
        skins: SkinConfig {
            last_skin_path: legacy.last_skin_path,
        },
        ..Default::default()
    };

    if let Err(e) = new_config.save() {
        log::error!("failed to save migrated config: {e}");
        return;
    }

    // Rename the old JSON file so it's not re-migrated.
    let backup = json_path.with_extension("json.bak");
    if let Err(e) = std::fs::rename(&json_path, &backup) {
        log::warn!("could not rename legacy config.json to .bak: {e}");
    }
}
