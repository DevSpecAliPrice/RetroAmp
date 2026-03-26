//! Window manager — creates and manages Tauri windows for each panel.
//!
//! Each panel (main player, EQ, playlist, etc.) is a separate Tauri window
//! with its own WebView. The window manager tracks which windows are open,
//! creates them on demand, and persists their state.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Identifies a specific window/panel type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WindowId {
    Main,
    Equalizer,
    Playlist,
    Settings,
    RadioBrowser,
    LibraryBrowser,
}

impl WindowId {
    /// The Tauri window label for this panel.
    pub fn label(&self) -> &'static str {
        match self {
            WindowId::Main => "main",
            WindowId::Equalizer => "equalizer",
            WindowId::Playlist => "playlist",
            WindowId::Settings => "settings",
            WindowId::RadioBrowser => "radiobrowser",
            WindowId::LibraryBrowser => "librarybrowser",
        }
    }

    /// The URL path this window loads (used for routing in the React app).
    pub fn url_path(&self) -> &'static str {
        match self {
            WindowId::Main => "/",
            WindowId::Equalizer => "/?window=equalizer",
            WindowId::Playlist => "/?window=playlist",
            WindowId::Settings => "/?window=settings",
            WindowId::RadioBrowser => "/?window=radiobrowser",
            WindowId::LibraryBrowser => "/?window=librarybrowser",
        }
    }

    /// Default width in native Winamp pixels (before scaling).
    /// Settings uses logical pixels directly (not Winamp-scaled).
    pub fn native_width(&self) -> u32 {
        match self {
            WindowId::Settings => 700,
            _ => 275,
        }
    }

    /// Default height in native Winamp pixels (before scaling).
    /// Settings uses logical pixels directly (not Winamp-scaled).
    pub fn native_height(&self) -> u32 {
        match self {
            WindowId::Main => 116,
            WindowId::Equalizer => 116,
            WindowId::Playlist => 232,
            WindowId::Settings => 500,
            WindowId::RadioBrowser => 300,
            WindowId::LibraryBrowser => 350,
        }
    }

    /// Whether this window should be resizable.
    pub fn resizable(&self) -> bool {
        match self {
            WindowId::Main | WindowId::Equalizer => false,
            WindowId::Playlist | WindowId::Settings | WindowId::RadioBrowser
            | WindowId::LibraryBrowser => true,
        }
    }
}

/// Tracks the state of all managed windows.
#[derive(Debug, Clone, Serialize)]
pub struct WindowStates {
    pub windows: HashMap<String, WindowState>,
    pub scale: u32,
    pub active_skin_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WindowState {
    pub id: WindowId,
    pub visible: bool,
}

/// The window manager.
pub struct WindowManager {
    /// Which windows are currently open (visible).
    states: HashMap<WindowId, bool>,
    /// Global UI scale factor (1, 2, 3).
    scale: u32,
    /// Path to the currently active skin.
    active_skin_path: Option<String>,
}

impl WindowManager {
    pub fn new() -> Self {
        let mut states = HashMap::new();
        states.insert(WindowId::Main, true);
        states.insert(WindowId::Equalizer, false);
        states.insert(WindowId::Playlist, false);
        states.insert(WindowId::RadioBrowser, false);
        states.insert(WindowId::LibraryBrowser, false);

        // Determine scale from screen height.
        let scale = Self::detect_scale();
        eprintln!("[retroamp] detected UI scale: {scale}x");

        Self { states, scale, active_skin_path: None }
    }

    /// Pick scale based on primary monitor resolution.
    fn detect_scale() -> u32 {
        // Use the SCREEN_HEIGHT env or fall back to a reasonable default.
        // On Linux we can read the display resolution.
        if let Ok(output) = std::process::Command::new("xdpyinfo")
            .output()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if line.contains("dimensions:") {
                    // Format: "  dimensions:    2560x1440 pixels ..."
                    if let Some(dims) = line.split_whitespace().nth(1) {
                        if let Some(h) = dims.split('x').nth(1) {
                            if let Ok(height) = h.parse::<u32>() {
                                if height >= 2160 { return 3; } // 4K
                                if height >= 1080 { return 2; } // 1080p, 1440p
                                return 1;
                            }
                        }
                    }
                }
            }
        }
        2 // Safe default
    }

    pub fn scale(&self) -> u32 {
        self.scale
    }

    pub fn set_scale(&mut self, scale: u32) {
        self.scale = scale.clamp(1, 3);
    }

    pub fn cycle_scale(&mut self) -> u32 {
        self.scale = match self.scale {
            1 => 2,
            2 => 3,
            _ => 1,
        };
        self.scale
    }

    pub fn active_skin_path(&self) -> Option<&str> {
        self.active_skin_path.as_deref()
    }

    pub fn set_active_skin_path(&mut self, path: String) {
        self.active_skin_path = Some(path);
    }

    /// Check if a window is currently visible.
    pub fn is_visible(&self, id: WindowId) -> bool {
        *self.states.get(&id).unwrap_or(&false)
    }

    /// Mark a window as visible or hidden.
    pub fn set_visible(&mut self, id: WindowId, visible: bool) {
        self.states.insert(id, visible);
    }

    /// Toggle a window's visibility. Returns the new state.
    pub fn toggle(&mut self, id: WindowId) -> bool {
        let current = self.is_visible(id);
        let new_state = !current;
        self.states.insert(id, new_state);
        new_state
    }

    /// Get the state of all windows (for the frontend to render button states).
    pub fn get_states(&self) -> WindowStates {
        let windows = self
            .states
            .iter()
            .map(|(id, visible)| {
                (
                    id.label().to_string(),
                    WindowState {
                        id: *id,
                        visible: *visible,
                    },
                )
            })
            .collect();
        WindowStates {
            windows,
            scale: self.scale,
            active_skin_path: self.active_skin_path.clone(),
        }
    }
}
