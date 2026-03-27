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
