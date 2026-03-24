//! Lightweight app configuration persisted as JSON in the platform config dir.
//!
//! Config file location:
//! - Linux:   `~/.config/retroamp/config.json`
//! - macOS:   `~/Library/Application Support/retroamp/config.json`
//! - Windows: `C:\Users\<user>\AppData\Roaming\retroamp\config.json`

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct AppConfig {
    /// Additional directories to scan for skins (beyond the built-in skins dir).
    #[serde(default)]
    pub extra_skin_dirs: Vec<PathBuf>,

    /// Last-used skin path, restored on next launch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_skin_path: Option<String>,
}

impl AppConfig {
    /// Load config from disk, returning defaults if the file doesn't exist yet.
    pub fn load() -> Self {
        let Some(path) = config_path() else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
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
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("failed to serialize config: {e}"))?;
        std::fs::write(&path, json)
            .map_err(|e| format!("failed to write config: {e}"))?;
        Ok(())
    }

    /// Add a skin directory if it isn't already present.
    pub fn add_skin_dir(&mut self, dir: PathBuf) -> bool {
        if self.extra_skin_dirs.contains(&dir) {
            return false;
        }
        self.extra_skin_dirs.push(dir);
        true
    }

    /// Remove a skin directory. Returns true if it was present.
    pub fn remove_skin_dir(&mut self, dir: &PathBuf) -> bool {
        let len = self.extra_skin_dirs.len();
        self.extra_skin_dirs.retain(|d| d != dir);
        self.extra_skin_dirs.len() != len
    }
}

fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|c| c.join("retroamp").join("config.json"))
}
