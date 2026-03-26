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
    /// Whether this track is a stream (URL-based) rather than a local file.
    pub is_stream: bool,
    /// For radio streams, the station name to display in the playlist.
    pub station_name: Option<String>,
}

impl Track {
    /// Create a new track from a file path or URL. Metadata is not loaded yet —
    /// call `load_metadata` or let the playlist manager handle it.
    pub fn from_path(id: TrackId, path: impl Into<String>) -> Self {
        let path = path.into();
        let is_stream = path.starts_with("http://") || path.starts_with("https://");
        Self {
            id,
            path,
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
            is_stream,
            station_name: None,
        }
    }

    /// The display name for the classic Winamp single-column playlist.
    /// For radio streams, always shows the station name.
    /// For local files: "Artist - Title", falling back to the filename.
    pub fn display_name(&self) -> String {
        if let Some(name) = &self.station_name {
            return name.clone();
        }
        match (&self.artist, &self.title) {
            (Some(artist), Some(title)) => format!("{artist} - {title}"),
            (None, Some(title)) => title.clone(),
            _ if self.is_stream => self.hostname().unwrap_or_else(|| self.path.clone()),
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

    /// Extract the hostname from a URL for display purposes.
    fn hostname(&self) -> Option<String> {
        // Simple extraction: find "://" then take until next "/" or end.
        let after_scheme = self.path.find("://").map(|i| &self.path[i + 3..])?;
        let host = after_scheme.split('/').next()?;
        Some(host.to_string())
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
