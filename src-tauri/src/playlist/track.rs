//! Track — a single item in a playlist.
//!
//! Contains all metadata needed for display, playback, and library management.
//! The same Track struct serves both the classic single-column Winamp view
//! (via `display_name()`) and future multi-column extended views.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Unique track identifier within a playlist session.
pub type TrackId = u64;

/// A single track in a playlist.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub id: TrackId,
    /// The source path or URL.
    pub path: String,
    /// Core metadata — populated from file tags on load.
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub genre: Option<String>,
    pub year: Option<u32>,
    pub track_number: Option<u32>,
    pub duration: Option<Duration>,
    /// Technical info.
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
    pub bitrate: Option<u32>,
    /// Whether metadata has been loaded from tags.
    pub metadata_loaded: bool,
}

impl Track {
    /// Create a new track from a file path. Metadata is not loaded yet —
    /// call `load_metadata` or let the playlist manager handle it.
    pub fn from_path(id: TrackId, path: impl Into<String>) -> Self {
        Self {
            id,
            path: path.into(),
            title: None,
            artist: None,
            album: None,
            genre: None,
            year: None,
            track_number: None,
            duration: None,
            sample_rate: None,
            channels: None,
            bitrate: None,
            metadata_loaded: false,
        }
    }

    /// The display name for the classic Winamp single-column playlist.
    /// Format: "Artist - Title", falling back to the filename.
    pub fn display_name(&self) -> String {
        match (&self.artist, &self.title) {
            (Some(artist), Some(title)) => format!("{artist} - {title}"),
            (None, Some(title)) => title.clone(),
            _ => self.filename(),
        }
    }

    /// Extract the filename from the path (without extension).
    pub fn filename(&self) -> String {
        PathBuf::from(&self.path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("???")
            .to_string()
    }

    /// Duration formatted as "M:SS" for the playlist display.
    pub fn duration_display(&self) -> String {
        match self.duration {
            Some(d) => {
                let total_secs = d.as_secs();
                let mins = total_secs / 60;
                let secs = total_secs % 60;
                format!("{mins}:{secs:02}")
            }
            None => String::new(),
        }
    }
}
