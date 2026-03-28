//! Radio stream recorder — silently buffers the current track and maintains
//! a rolling history of recently completed tracks.
//!
//! The recorder sits in the reader thread's hot loop (`stream_reader.rs`),
//! receiving raw compressed audio bytes after ICY metadata has been stripped.
//! Track boundaries are detected by watching for `StreamTitle` changes.
//!
//! Key design constraints:
//! - `push_bytes()` uses `try_lock()` so it never blocks the reader thread.
//! - File writing is offloaded to spawned threads.
//! - Memory is bounded: max 10 tracks, 80 MB total.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use serde::Serialize;

use crate::audio::icy;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum number of completed tracks to keep in memory.
const MAX_HISTORY: usize = 10;

/// Maximum total memory for the history buffer (~600 MB).
const MAX_MEMORY_BYTES: usize = 600 * 1024 * 1024;

/// Maximum size for a single track buffer (~600 MB).
/// Allows ~4 hours at 320 kbps. Prevents unbounded growth if a stream
/// runs indefinitely with no metadata changes and no manual stop.
const MAX_SINGLE_TRACK_BYTES: usize = 600 * 1024 * 1024;

/// Minimum track duration (seconds) to include in history.
/// Filters out station IDs, jingles, and blank metadata transitions.
const MIN_TRACK_DURATION_SECS: f64 = 15.0;

// ---------------------------------------------------------------------------
// Types exposed to the frontend
// ---------------------------------------------------------------------------

/// Recording state visible to the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordingState {
    /// No active save request — buffering silently.
    Idle,
    /// User requested save — waiting for track boundary.
    Saving,
    /// Track was just saved.
    Saved,
    /// Manual recording in progress (no-metadata streams).
    Recording,
}

/// Lightweight info about the current track (no raw data).
#[derive(Debug, Clone, Serialize)]
pub struct CurrentTrackInfo {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub station_name: Option<String>,
    pub buffered_secs: f64,
    pub buffered_bytes: usize,
}

/// Lightweight info about a completed track (no raw data).
#[derive(Debug, Clone, Serialize)]
pub struct CompletedTrackInfo {
    pub id: u64,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub station_name: Option<String>,
    pub duration_secs: f64,
    pub size_bytes: usize,
    pub saved: bool,
}

/// Full recorder status for the frontend.
#[derive(Debug, Clone, Serialize)]
pub struct RecorderStatus {
    /// Whether a recorder is active (stream is playing).
    pub active: bool,
    /// Whether the stream provides track metadata.
    pub has_metadata: bool,
    /// Current recording state.
    pub state: RecordingState,
    /// Info about the track currently being buffered.
    pub current_track: Option<CurrentTrackInfo>,
    /// History of completed tracks (newest first).
    pub history: Vec<CompletedTrackInfo>,
}

// ---------------------------------------------------------------------------
// Events sent to the Tauri listener thread
// ---------------------------------------------------------------------------

/// Events emitted by the recorder for the frontend.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum RecorderEvent {
    /// A track boundary was detected; history updated.
    TrackChanged {
        history_count: usize,
        /// If the track that just completed was saved, its ID.
        saved_track_id: Option<u64>,
    },
    /// A track was saved to disk.
    TrackSaved {
        track_id: u64,
        filename: String,
    },
}

// ---------------------------------------------------------------------------
// Internal types
// ---------------------------------------------------------------------------

/// Accumulates raw compressed bytes for the track currently playing.
struct RecordingBuffer {
    data: Vec<u8>,
    title: Option<String>,
    station_name: Option<String>,
    content_type: String,
    is_adts: bool,
    bitrate: Option<u32>,
    started_at: Instant,
}

/// A finished track stored in the history ring buffer.
pub struct CompletedTrack {
    pub id: u64,
    pub data: Vec<u8>,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub station_name: Option<String>,
    pub content_type: String,
    pub is_adts: bool,
    pub bitrate: Option<u32>,
    pub duration_estimate: Duration,
    pub completed_at: SystemTime,
    pub saved: bool,
}

// ---------------------------------------------------------------------------
// RadioRecorder
// ---------------------------------------------------------------------------

/// The main recorder state machine, shared via `Arc<RadioRecorder>`.
///
/// Accessed from:
/// - Reader thread: `push_bytes()`, `check_track_boundary()`
/// - Tauri commands: `request_save()`, `save_from_history()`, etc.
/// - Frontend polling: `status()`
pub struct RadioRecorder {
    current: Mutex<Option<RecordingBuffer>>,
    history: Mutex<VecDeque<CompletedTrack>>,
    last_title: Mutex<Option<String>>,
    save_wanted: AtomicBool,
    state: Mutex<RecordingState>,
    has_metadata: AtomicBool,
    manual_recording: AtomicBool,
    next_track_id: AtomicU64,
    event_tx: Mutex<Option<mpsc::Sender<RecorderEvent>>>,

    // Stream info (set once during connect).
    content_type: Mutex<String>,
    is_adts: Mutex<bool>,
    bitrate: Mutex<Option<u32>>,
    station_name: Mutex<Option<String>>,
}

impl RadioRecorder {
    /// Create a new recorder. Call `set_stream_info()` after ADTS detection.
    pub fn new() -> Self {
        Self {
            current: Mutex::new(None),
            history: Mutex::new(VecDeque::new()),
            last_title: Mutex::new(None),
            save_wanted: AtomicBool::new(false),
            state: Mutex::new(RecordingState::Idle),
            has_metadata: AtomicBool::new(false),
            manual_recording: AtomicBool::new(false),
            next_track_id: AtomicU64::new(1),
            event_tx: Mutex::new(None),
            content_type: Mutex::new("audio/mpeg".to_string()),
            is_adts: Mutex::new(false),
            bitrate: Mutex::new(None),
            station_name: Mutex::new(None),
        }
    }

    /// Set stream format info. Called from `radio.rs` after ADTS detection.
    pub fn set_stream_info(
        &self,
        content_type: &str,
        is_adts: bool,
        bitrate: Option<u32>,
        station_name: Option<&str>,
        has_metaint: bool,
    ) {
        if let Ok(mut ct) = self.content_type.lock() {
            *ct = content_type.to_string();
        }
        if let Ok(mut a) = self.is_adts.lock() {
            *a = is_adts;
        }
        if let Ok(mut b) = self.bitrate.lock() {
            *b = bitrate;
        }
        if let Ok(mut sn) = self.station_name.lock() {
            *sn = station_name.map(|s| s.to_string());
        }

        // If there's no metaint at all, we know upfront there's no metadata.
        if !has_metaint {
            self.has_metadata.store(false, Ordering::Relaxed);
        }

        // Start the initial recording buffer (for streams with metadata).
        if has_metaint {
            if let Ok(mut current) = self.current.lock() {
                *current = Some(RecordingBuffer {
                    data: Vec::with_capacity(512 * 1024),
                    title: None,
                    station_name: station_name.map(|s| s.to_string()),
                    content_type: content_type.to_string(),
                    is_adts,
                    bitrate,
                    started_at: Instant::now(),
                });
            }
        }
    }

    /// Set the event channel for notifying the frontend.
    pub fn set_event_tx(&self, tx: mpsc::Sender<RecorderEvent>) {
        if let Ok(mut etx) = self.event_tx.lock() {
            *etx = Some(tx);
        }
    }

    // -- Reader thread methods (hot path) --

    /// Append raw compressed bytes to the current track buffer.
    ///
    /// Called from the reader thread. Uses `try_lock()` to never block.
    pub fn push_bytes(&self, bytes: &[u8]) {
        if let Ok(mut current) = self.current.try_lock() {
            if let Some(ref mut buf) = *current {
                // Cap single track buffer to avoid unbounded growth.
                if buf.data.len() + bytes.len() <= MAX_SINGLE_TRACK_BYTES {
                    buf.data.extend_from_slice(bytes);
                }
            }
        }
    }

    /// Check if the stream title has changed and handle the track boundary.
    ///
    /// Called from the reader thread after each read. Uses `try_lock()`.
    /// Returns `true` if a boundary was detected (for logging).
    pub fn check_track_boundary(&self, new_title: &str) -> bool {
        // Never interfere with manual recording — the user controls start/stop.
        if self.manual_recording.load(Ordering::Relaxed) {
            return false;
        }

        let title_changed = {
            if let Ok(last) = self.last_title.try_lock() {
                match &*last {
                    Some(prev) => prev != new_title,
                    None => true, // First title seen.
                }
            } else {
                return false; // Lock contended, skip.
            }
        };

        if !title_changed {
            return false;
        }

        // Mark that we've seen at least one title change.
        self.has_metadata.store(true, Ordering::Relaxed);

        // Update last known title.
        if let Ok(mut last) = self.last_title.try_lock() {
            *last = Some(new_title.to_string());
        }

        // Finalize the current buffer and start a new one.
        self.finalize_current(Some(new_title));

        true
    }

    /// Finalize the current recording buffer, push to history.
    ///
    /// If `save_wanted` is set, the track will be written to disk.
    /// `next_title` is the title of the incoming track (used to start
    /// a new buffer); pass `None` when stopping.
    fn finalize_current(&self, next_title: Option<&str>) {
        let completed = {
            let mut current = match self.current.lock() {
                Ok(c) => c,
                Err(_) => return,
            };

            let buf = match current.take() {
                Some(b) if !b.data.is_empty() => b,
                _ => {
                    // No data to finalize. Start a new buffer if needed.
                    if next_title.is_some() {
                        *current = Some(self.make_buffer(next_title));
                    }
                    return;
                }
            };

            let id = self.next_track_id.fetch_add(1, Ordering::Relaxed);
            let (title, artist) = match &buf.title {
                Some(raw) => icy::split_stream_title(raw),
                None => (buf.station_name.clone(), None),
            };

            let duration_estimate = buf.started_at.elapsed();
            let track = CompletedTrack {
                id,
                data: buf.data,
                title,
                artist,
                station_name: buf.station_name,
                content_type: buf.content_type,
                is_adts: buf.is_adts,
                bitrate: buf.bitrate,
                duration_estimate,
                completed_at: SystemTime::now(),
                saved: false,
            };

            // Start a new buffer for the next track.
            *current = next_title.map(|_| self.make_buffer(next_title));

            track
        };

        // Check if user wanted to save this track.
        let should_save = self.save_wanted.swap(false, Ordering::Relaxed);

        let track_id = completed.id;
        let track_duration = completed.duration_estimate.as_secs_f64();

        // If save was requested, mark the track as saved regardless of duration.
        if should_save {
            let mut completed = completed;
            completed.saved = true;

            if let Ok(mut history) = self.history.lock() {
                Self::evict_for_new(&mut history, completed.data.len());
                let id = completed.id;
                history.push_back(completed);
                drop(history);
                self.save_track_by_id(id);
            }
        } else if track_duration >= MIN_TRACK_DURATION_SECS {
            // Only add to history if the track exceeds the minimum duration.
            // Filters out station IDs, jingles, and blank metadata transitions.
            if let Ok(mut history) = self.history.lock() {
                Self::evict_for_new(&mut history, completed.data.len());
                history.push_back(completed);
            }
        } else {
            log::debug!(
                "[recorder] skipping short track ({:.1}s < {MIN_TRACK_DURATION_SECS}s)",
                track_duration,
            );
        }

        // Always reset state to Idle for the new track.
        // The saved track appears in history with saved=true as confirmation.
        if let Ok(mut state) = self.state.lock() {
            *state = RecordingState::Idle;
        }

        // Emit event so frontend refreshes.
        self.emit(RecorderEvent::TrackChanged {
            history_count: self
                .history
                .lock()
                .map(|h| h.len())
                .unwrap_or(0),
            saved_track_id: if should_save { Some(track_id) } else { None },
        });
    }

    /// Evict oldest tracks to make room for a new one.
    fn evict_for_new(history: &mut VecDeque<CompletedTrack>, new_size: usize) {
        while history.len() >= MAX_HISTORY {
            history.pop_front();
        }
        let mut total: usize = history.iter().map(|t| t.data.len()).sum();
        while total + new_size > MAX_MEMORY_BYTES && !history.is_empty() {
            if let Some(evicted) = history.pop_front() {
                total -= evicted.data.len();
            }
        }
    }

    fn make_buffer(&self, title: Option<&str>) -> RecordingBuffer {
        RecordingBuffer {
            data: Vec::with_capacity(512 * 1024),
            title: title.map(|s| s.to_string()),
            station_name: self.station_name.lock().ok().and_then(|s| s.clone()),
            content_type: self
                .content_type
                .lock()
                .map(|c| c.clone())
                .unwrap_or_else(|_| "audio/mpeg".to_string()),
            is_adts: self.is_adts.lock().map(|g| *g).unwrap_or(false),
            bitrate: self.bitrate.lock().ok().and_then(|b| *b),
            started_at: Instant::now(),
        }
    }

    // -- Command methods (called from Tauri commands) --

    /// User wants to save the current track when it finishes.
    pub fn request_save(&self) {
        self.save_wanted.store(true, Ordering::Relaxed);
        if let Ok(mut state) = self.state.lock() {
            *state = RecordingState::Saving;
        }
    }

    /// Cancel a pending save request.
    pub fn cancel_save(&self) {
        self.save_wanted.store(false, Ordering::Relaxed);
        if let Ok(mut state) = self.state.lock() {
            *state = RecordingState::Idle;
        }
    }

    /// Whether a save has been requested and we're waiting for a track boundary.
    pub fn is_save_pending(&self) -> bool {
        self.save_wanted.load(Ordering::Relaxed)
    }

    /// Whether manual recording is active.
    pub fn is_manual_recording(&self) -> bool {
        self.manual_recording.load(Ordering::Relaxed)
    }

    /// Finalize the current buffer immediately and save it.
    /// Called when the stream is about to be replaced (station switch, etc.)
    /// and the user had requested a save.
    pub fn finalize_for_save(&self) {
        // save_wanted is already true, so finalize_current will trigger the save.
        self.finalize_current(None);
        if let Ok(mut state) = self.state.lock() {
            *state = RecordingState::Idle;
        }
    }

    /// Start manual recording (no-metadata streams).
    pub fn start_manual_recording(&self) {
        self.manual_recording.store(true, Ordering::Relaxed);
        if let Ok(mut state) = self.state.lock() {
            *state = RecordingState::Recording;
        }
        if let Ok(mut current) = self.current.lock() {
            *current = Some(RecordingBuffer {
                data: Vec::with_capacity(512 * 1024),
                title: None,
                station_name: self.station_name.lock().ok().and_then(|s| s.clone()),
                content_type: self
                    .content_type
                    .lock()
                    .map(|c| c.clone())
                    .unwrap_or_else(|_| "audio/mpeg".to_string()),
                is_adts: self.is_adts.lock().map(|g| *g).unwrap_or(false),
                bitrate: self.bitrate.lock().ok().and_then(|b| *b),
                started_at: Instant::now(),
            });
        }
    }

    /// Stop manual recording and push to history.
    pub fn stop_manual_recording(&self) {
        self.manual_recording.store(false, Ordering::Relaxed);
        // Auto-save: pressing Stop is an explicit "save this" action.
        self.save_wanted.store(true, Ordering::Relaxed);
        self.finalize_current(None);
        if let Ok(mut state) = self.state.lock() {
            *state = RecordingState::Idle;
        }
    }

    /// Save a specific track from history by ID.
    pub fn save_from_history(&self, track_id: u64) {
        self.save_track_by_id(track_id);
    }

    /// Get a clone of a track's data + format info for BufferSource playback.
    pub fn get_track_data(
        &self,
        track_id: u64,
    ) -> Option<(Vec<u8>, String, bool, Option<u32>, TrackMeta)> {
        let history = self.history.lock().ok()?;
        let track = history.iter().find(|t| t.id == track_id)?;
        Some((
            track.data.clone(),
            track.content_type.clone(),
            track.is_adts,
            track.bitrate,
            TrackMeta {
                title: track.title.clone(),
                artist: track.artist.clone(),
                station_name: track.station_name.clone(),
                duration_estimate: track.duration_estimate,
            },
        ))
    }

    /// Build the full status for the frontend.
    pub fn status(&self) -> RecorderStatus {
        let state = self
            .state
            .lock()
            .map(|s| *s)
            .unwrap_or(RecordingState::Idle);

        let current_track = self.current.lock().ok().and_then(|c| {
            c.as_ref().map(|buf| {
                let (title, artist) = match &buf.title {
                    Some(raw) => icy::split_stream_title(raw),
                    None => (buf.station_name.clone(), None),
                };
                let buffered_secs = buf.started_at.elapsed().as_secs_f64();
                CurrentTrackInfo {
                    title,
                    artist,
                    station_name: buf.station_name.clone(),
                    buffered_secs,
                    buffered_bytes: buf.data.len(),
                }
            })
        });

        let history = self
            .history
            .lock()
            .map(|h| {
                h.iter()
                    .rev() // Newest first for the UI.
                    .map(|t| CompletedTrackInfo {
                        id: t.id,
                        title: t.title.clone(),
                        artist: t.artist.clone(),
                        station_name: t.station_name.clone(),
                        duration_secs: t.duration_estimate.as_secs_f64(),
                        size_bytes: t.data.len(),
                        saved: t.saved,
                    })
                    .collect()
            })
            .unwrap_or_default();

        RecorderStatus {
            active: true,
            has_metadata: self.has_metadata.load(Ordering::Relaxed),
            state,
            current_track,
            history,
        }
    }

    /// Reset the recorder (called when stream stops).
    pub fn reset(&self) {
        if let Ok(mut current) = self.current.lock() {
            *current = None;
        }
        if let Ok(mut history) = self.history.lock() {
            history.clear();
        }
        if let Ok(mut last) = self.last_title.lock() {
            *last = None;
        }
        self.save_wanted.store(false, Ordering::Relaxed);
        self.has_metadata.store(false, Ordering::Relaxed);
        self.manual_recording.store(false, Ordering::Relaxed);
        if let Ok(mut state) = self.state.lock() {
            *state = RecordingState::Idle;
        }
    }

    // -- Internal helpers --

    fn save_track_by_id(&self, track_id: u64) {
        let (track_data, content_type, is_adts, title, artist, station_name) = {
            let mut history = match self.history.lock() {
                Ok(h) => h,
                Err(_) => return,
            };
            let track = match history.iter_mut().find(|t| t.id == track_id) {
                Some(t) => t,
                None => return,
            };
            track.saved = true;
            (
                track.data.clone(),
                track.content_type.clone(),
                track.is_adts,
                track.title.clone(),
                track.artist.clone(),
                track.station_name.clone(),
            )
        };

        let download_dir = get_download_dir();
        let event_tx = self.event_tx.lock().ok().and_then(|tx| tx.clone());

        std::thread::Builder::new()
            .name("radio-save".into())
            .spawn(move || {
                match write_track_to_disk(
                    &track_data,
                    &content_type,
                    is_adts,
                    title.as_deref(),
                    artist.as_deref(),
                    station_name.as_deref(),
                    &download_dir,
                ) {
                    Ok(path) => {
                        let filename = path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();
                        log::info!("[recorder] saved: {filename}");
                        if let Some(tx) = event_tx {
                            let _ = tx.send(RecorderEvent::TrackSaved {
                                track_id,
                                filename,
                            });
                        }
                    }
                    Err(e) => {
                        log::error!("[recorder] save failed: {e}");
                    }
                }
            })
            .ok();
    }

    fn emit(&self, event: RecorderEvent) {
        if let Ok(tx) = self.event_tx.lock() {
            if let Some(tx) = tx.as_ref() {
                let _ = tx.send(event);
            }
        }
    }
}

/// Metadata extracted for BufferSource playback.
pub struct TrackMeta {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub station_name: Option<String>,
    pub duration_estimate: Duration,
}

// ---------------------------------------------------------------------------
// File writing
// ---------------------------------------------------------------------------

/// Get the configured download directory, falling back to OS music dir.
pub fn get_download_dir() -> PathBuf {
    let cfg = crate::config::AppConfig::load();
    cfg.general
        .download_dir
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::audio_dir().unwrap_or_else(|| {
                dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
            })
        })
}

/// Determine the file extension from stream format.
fn file_extension(content_type: &str, is_adts: bool) -> &'static str {
    if is_adts {
        return "aac";
    }
    let ct = content_type.to_lowercase();
    if ct.contains("ogg") {
        "ogg"
    } else if ct.contains("flac") {
        "flac"
    } else {
        "mp3"
    }
}

/// Sanitize a string for use as a filename.
fn sanitize_filename(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(200)
        .collect()
}

/// Build a unique file path, adding numeric suffix if needed.
fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let base = dir.join(format!("{stem}.{ext}"));
    if !base.exists() {
        return base;
    }
    for i in 2..1000 {
        let candidate = dir.join(format!("{stem} ({i}).{ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    // Fallback with timestamp.
    let ts = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    dir.join(format!("{stem}_{ts}.{ext}"))
}

/// Write a completed track's raw bytes to disk and attempt to tag it.
fn write_track_to_disk(
    data: &[u8],
    content_type: &str,
    is_adts: bool,
    title: Option<&str>,
    artist: Option<&str>,
    station_name: Option<&str>,
    download_dir: &Path,
) -> Result<PathBuf, String> {
    let ext = file_extension(content_type, is_adts);

    // Build filename.
    let stem = match (artist, title) {
        (Some(a), Some(t)) => sanitize_filename(&format!("{a} - {t}")),
        (None, Some(t)) => sanitize_filename(t),
        _ => {
            // No metadata — use station name + timestamp.
            let now = chrono_timestamp();
            let name = station_name.unwrap_or("Radio");
            sanitize_filename(&format!("{name} - {now}"))
        }
    };

    // Ensure download directory exists.
    std::fs::create_dir_all(download_dir)
        .map_err(|e| format!("create dir: {e}"))?;

    let path = unique_path(download_dir, &stem, ext);

    // Write raw bytes.
    std::fs::write(&path, data).map_err(|e| format!("write: {e}"))?;

    // Attempt to tag the file.
    if let Err(e) = tag_file(&path, title, artist, station_name) {
        log::warn!("[recorder] tagging failed for {}: {e}", path.display());
    }

    Ok(path)
}

/// Attempt to write ID3/Vorbis tags to the file.
fn tag_file(
    path: &Path,
    title: Option<&str>,
    artist: Option<&str>,
    album: Option<&str>,
) -> Result<(), String> {
    use lofty::prelude::*;
    use lofty::tag::ItemKey;

    let mut tagged_file =
        lofty::read_from_path(path).map_err(|e| format!("{e}"))?;

    let tag = match tagged_file.primary_tag_mut() {
        Some(t) => t,
        None => {
            // Create a tag if none exists.
            let tag_type = tagged_file.primary_tag_type();
            tagged_file.insert_tag(lofty::tag::Tag::new(tag_type));
            tagged_file.primary_tag_mut().ok_or("no tag")?
        }
    };

    if let Some(title) = title {
        tag.insert_text(ItemKey::TrackTitle, title.to_string());
    }
    if let Some(artist) = artist {
        tag.insert_text(ItemKey::TrackArtist, artist.to_string());
    }
    if let Some(album) = album {
        tag.insert_text(ItemKey::AlbumTitle, album.to_string());
    }

    tag.save_to_path(path, lofty::config::WriteOptions::default())
        .map_err(|e| format!("{e}"))
}

/// Generate a human-readable timestamp for filenames.
fn chrono_timestamp() -> String {
    use std::time::UNIX_EPOCH;
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Convert to a simple date-time string without external crate.
    // Format: YYYY-MM-DD HH-MM-SS (approximate, using seconds since epoch).
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Simple year/month/day calculation (approximate, ignoring leap seconds).
    let (year, month, day) = days_to_date(days);
    format!("{year:04}-{month:02}-{day:02} {hours:02}-{minutes:02}-{seconds:02}")
}

/// Convert days since Unix epoch to (year, month, day).
fn days_to_date(days_since_epoch: u64) -> (u64, u64, u64) {
    // Simplified Gregorian calendar conversion.
    let mut y = 1970;
    let mut remaining = days_since_epoch;

    loop {
        let days_in_year = if is_leap(y) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        y += 1;
    }

    let leap = is_leap(y);
    let month_days: [u64; 12] = [
        31,
        if leap { 29 } else { 28 },
        31, 30, 31, 30, 31, 31, 30, 31, 30, 31,
    ];
    let mut m = 0;
    for &md in &month_days {
        if remaining < md {
            break;
        }
        remaining -= md;
        m += 1;
    }

    (y, m + 1, remaining + 1)
}

fn is_leap(y: u64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}
