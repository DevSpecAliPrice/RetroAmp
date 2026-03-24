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
}

impl WindowId {
    /// The Tauri window label for this panel.
    pub fn label(&self) -> &'static str {
        match self {
            WindowId::Main => "main",
            WindowId::Equalizer => "equalizer",
            WindowId::Playlist => "playlist",
        }
    }

    /// The URL path this window loads (used for routing in the React app).
    pub fn url_path(&self) -> &'static str {
        match self {
            WindowId::Main => "/",
            WindowId::Equalizer => "/?window=equalizer",
            WindowId::Playlist => "/?window=playlist",
        }
    }

    /// Default width in native Winamp pixels (before scaling).
    pub fn native_width(&self) -> u32 {
        275
    }

    /// Default height in native Winamp pixels (before scaling).
    pub fn native_height(&self) -> u32 {
        match self {
            WindowId::Main => 116,
            WindowId::Equalizer => 116,
            WindowId::Playlist => 232, // 116 * 2 — a reasonable default
        }
    }

    /// Whether this window should be resizable.
    pub fn resizable(&self) -> bool {
        match self {
            WindowId::Main => false,
            WindowId::Equalizer => false,
            WindowId::Playlist => true,
        }
    }
}

/// Tracks the state of all managed windows.
#[derive(Debug, Clone, Serialize)]
pub struct WindowStates {
    pub windows: HashMap<String, WindowState>,
    pub scale: u32,
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
}

impl WindowManager {
    pub fn new() -> Self {
        let mut states = HashMap::new();
        states.insert(WindowId::Main, true);
        states.insert(WindowId::Equalizer, false);
        states.insert(WindowId::Playlist, false);
        Self { states, scale: 2 }
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
        }
    }
}
