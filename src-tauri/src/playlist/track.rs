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

/// The type of audio source backing a track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourceType {
    /// A local file on disk.
    Local,
    /// An internet radio stream (HTTP/HTTPS URL).
    Stream,
    /// A Spotify track (spotify:track:<id> URI).
    #[cfg(feature = "spotify")]
    Spotify,
    /// A YouTube track (youtube:<video_id>).
    YouTube,
}

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
    /// The type of audio source (local file, stream, or Spotify).
    pub source_type: SourceType,
    /// For radio streams, the station name to display in the playlist.
    pub station_name: Option<String>,
    /// Cached cover art bytes. Skipped from persistence — local sources
    /// re-extract on open, and YouTube tracks re-download on play.
    #[serde(skip)]
    pub cover_art: Option<Vec<u8>>,
}

impl Track {
    /// Create a new track from a file path or URL. Metadata is not loaded yet —
    /// call `load_metadata` or let the playlist manager handle it.
    pub fn from_path(id: TrackId, path: impl Into<String>) -> Self {
        let path = path.into();
        let source_type = {
            #[cfg(feature = "spotify")]
            {
                if path.starts_with("spotify:track:") {
                    SourceType::Spotify
                } else if path.starts_with("youtube:") {
                    SourceType::YouTube
                } else if path.starts_with("http://") || path.starts_with("https://") {
                    SourceType::Stream
                } else {
                    SourceType::Local
                }
            }
            #[cfg(not(feature = "spotify"))]
            {
                if path.starts_with("youtube:") {
                    SourceType::YouTube
                } else if path.starts_with("http://") || path.starts_with("https://") {
                    SourceType::Stream
                } else {
                    SourceType::Local
                }
            }
        };
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
            source_type,
            station_name: None,
            cover_art: None,
        }
    }

    /// Whether this track is an internet radio stream.
    pub fn is_stream(&self) -> bool {
        self.source_type == SourceType::Stream
    }

    /// Whether this track is a Spotify track.
    #[cfg(feature = "spotify")]
    pub fn is_spotify(&self) -> bool {
        self.source_type == SourceType::Spotify
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
            _ if self.is_stream() => self.hostname().unwrap_or_else(|| self.path.clone()),
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

    /// Convert playlist track metadata into an `AudioSource`-level `TrackMetadata`.
    /// Used to pre-populate metadata when creating sources for YouTube/Spotify
    /// tracks that already have metadata from the browser UI.
    pub fn to_source_metadata(&self) -> crate::audio::source::TrackMetadata {
        crate::audio::source::TrackMetadata {
            title: self.title.clone(),
            artist: self.artist.clone(),
            album: self.album.clone(),
            duration: self.duration,
            sample_rate: self.sample_rate.unwrap_or(44100),
            channels: self.channels.unwrap_or(2),
            bitrate: self.bitrate,
            genre: self.genre.clone(),
            year: self.year,
            track_number: self.track_number,
            cover_art: self.cover_art.clone(),
        }
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
