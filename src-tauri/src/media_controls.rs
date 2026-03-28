//! OS-level media player integration.
//!
//! Registers RetroAmp with the operating system's media layer so that:
//! - Hardware media keys (play/pause, next, previous, stop) work
//! - Track metadata appears in OS media widgets (GNOME/KDE panel, Windows
//!   taskbar, macOS Control Center / Now Playing)
//!
//! Uses the `souvlaki` crate which provides a single cross-platform API over:
//! - Linux: MPRIS2 (D-Bus)
//! - Windows: SystemMediaTransportControls (SMTC)
//! - macOS: MPRemoteCommandCenter + MPNowPlayingInfoCenter

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
};

use crate::audio::engine::{AudioEngine, PlaybackState};
use crate::commands;
use crate::playlist::manager::PlaylistManager;

/// Manages the OS media controls integration. Created once at startup and
/// stored as Tauri managed state. The actual `MediaControls` handle lives
/// inside a dedicated polling thread.
pub struct MediaService {
    _thread: thread::JoinHandle<()>,
}

// souvlaki::MediaControls is not Send on all platforms, so we cannot store it
// directly in Tauri state. The polling thread owns it exclusively. We just
// need MediaService itself to be Send for Tauri's manage().
unsafe impl Send for MediaService {}
unsafe impl Sync for MediaService {}

impl MediaService {
    /// Start the media controls service. This spawns a background thread that:
    /// 1. Registers with the OS media layer
    /// 2. Polls engine status and pushes metadata/playback updates
    /// 3. Receives media key events and dispatches them to the engine
    pub fn new(
        engine: Arc<AudioEngine>,
        playlist: Arc<Mutex<PlaylistManager>>,
        #[allow(unused_variables)] hwnd: Option<*mut std::ffi::c_void>,
    ) -> Result<Self, String> {
        // Wrap the raw pointer so it can cross the thread boundary.
        // SAFETY: the HWND is valid for the lifetime of the main window,
        // and the media controls thread will not outlive the process.
        #[allow(unused_variables)]
        let hwnd_raw = hwnd.map(|p| p as usize);

        // We need to create MediaControls on the thread that will own it,
        // but we want to report initialisation errors to the caller. Use a
        // channel to pass back the result.
        let (init_tx, init_rx) = std::sync::mpsc::channel::<Result<(), String>>();

        let handle = thread::Builder::new()
            .name("retroamp-media-controls".into())
            .spawn(move || {
                // Build platform config inside the thread to avoid Send issues
                // with the raw pointer in PlatformConfig.
                #[allow(unused_mut)]
                let mut config = PlatformConfig {
                    dbus_name: "retroamp",
                    display_name: "RetroAmp",
                    hwnd: None,
                };
                #[cfg(target_os = "windows")]
                {
                    config.hwnd = hwnd_raw.map(|p| p as *mut std::ffi::c_void);
                }

                let mut controls = match MediaControls::new(config) {
                    Ok(c) => {
                        let _ = init_tx.send(Ok(()));
                        c
                    }
                    Err(e) => {
                        let _ = init_tx.send(Err(format!("{e}")));
                        return;
                    }
                };

                // Attach the event handler for media key presses.
                let engine_handler = Arc::clone(&engine);
                let playlist_handler = Arc::clone(&playlist);

                if let Err(e) = controls.attach(move |event: MediaControlEvent| {
                    handle_media_event(&event, &engine_handler, &playlist_handler);
                }) {
                    log::warn!("failed to attach media control handler: {e}");
                    return;
                }

                // Enter the polling loop.
                polling_loop(&mut controls, &engine);
            })
            .map_err(|e| format!("failed to spawn media controls thread: {e}"))?;

        // Wait for initialisation result from the thread.
        init_rx
            .recv()
            .map_err(|_| "media controls thread exited during init".to_string())?
            .map_err(|e| format!("failed to create media controls: {e}"))?;

        Ok(Self { _thread: handle })
    }
}

/// Handle an incoming media key event by dispatching to the engine/playlist.
fn handle_media_event(
    event: &MediaControlEvent,
    engine: &AudioEngine,
    playlist: &Mutex<PlaylistManager>,
) {
    match event {
        MediaControlEvent::Play => {
            engine.resume();
        }
        MediaControlEvent::Pause => {
            engine.pause();
        }
        MediaControlEvent::Toggle => {
            let status = engine.status();
            match status.state {
                PlaybackState::Playing => engine.pause(),
                _ => engine.resume(),
            }
        }
        MediaControlEvent::Next => {
            if let Ok(mut pl) = playlist.lock() {
                if let Some(track) = pl.next_track() {
                    let path = track.path.clone();
                    drop(pl);
                    if let Err(e) = commands::play_path(engine, &path, None) {
                        log::error!("media controls: next_track failed: {e}");
                    }
                }
            }
        }
        MediaControlEvent::Previous => {
            if let Ok(mut pl) = playlist.lock() {
                if let Some(track) = pl.previous_track() {
                    let path = track.path.clone();
                    drop(pl);
                    if let Err(e) = commands::play_path(engine, &path, None) {
                        log::error!("media controls: previous_track failed: {e}");
                    }
                }
            }
        }
        MediaControlEvent::Stop => {
            engine.stop();
        }
        MediaControlEvent::Seek(direction) => {
            let status = engine.status();
            if let Some(pos) = status.position {
                let delta = match direction {
                    souvlaki::SeekDirection::Forward => 5.0,
                    souvlaki::SeekDirection::Backward => -5.0,
                };
                let new_pos = (pos + delta).max(0.0);
                engine.seek(Duration::from_secs_f64(new_pos));
            }
        }
        MediaControlEvent::SetPosition(pos) => {
            engine.seek(pos.0);
        }
        MediaControlEvent::SetVolume(v) => {
            engine.set_volume(*v as f32);
        }
        _ => {
            // Raise, Quit, OpenUri — ignore for now.
        }
    }
}

/// Snapshot of the metadata we last reported to the OS, used to avoid
/// redundant updates.
#[derive(Default)]
struct LastReported {
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    state: Option<PlaybackState>,
    duration_secs: Option<f64>,
}

/// Continuously poll the engine status and push updates to the OS media layer.
fn polling_loop(controls: &mut MediaControls, engine: &AudioEngine) {
    let mut last = LastReported::default();
    // Path for writing cover art so the OS can read it via file:// URL.
    let cover_art_path = dirs::cache_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join("retroamp")
        .join("cover_art_cache.jpg");

    loop {
        let status = engine.status();

        // Check if metadata changed.
        let (title, artist, album, duration_secs, cover_art) = match &status.metadata {
            Some(meta) => (
                meta.title.clone(),
                meta.artist.clone(),
                meta.album.clone(),
                meta.duration.map(|d| d.as_secs_f64()),
                meta.cover_art.as_ref(),
            ),
            None => (None, None, None, None, None),
        };

        let metadata_changed = title != last.title
            || artist != last.artist
            || album != last.album
            || duration_secs != last.duration_secs;

        if metadata_changed {
            // Write cover art to a temp file if available.
            let cover_url = if let Some(art_bytes) = cover_art {
                if let Some(parent) = cover_art_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                match std::fs::write(&cover_art_path, art_bytes) {
                    Ok(()) => {
                        Some(format!("file://{}", cover_art_path.to_string_lossy()))
                    }
                    Err(e) => {
                        log::warn!("failed to write cover art cache: {e}");
                        None
                    }
                }
            } else {
                None
            };

            let _ = controls.set_metadata(MediaMetadata {
                title: title.as_deref(),
                artist: artist.as_deref(),
                album: album.as_deref(),
                cover_url: cover_url.as_deref(),
                duration: duration_secs.map(|s| Duration::from_secs_f64(s)),
            });

            last.title = title;
            last.artist = artist;
            last.album = album;
            last.duration_secs = duration_secs;
        }

        // Always update playback state + position (so OS seek bars track).
        let playback_changed = last.state != Some(status.state);
        if playback_changed || status.state == PlaybackState::Playing {
            let playback = match status.state {
                PlaybackState::Playing => MediaPlayback::Playing {
                    progress: status
                        .position
                        .map(|p| MediaPosition(Duration::from_secs_f64(p))),
                },
                PlaybackState::Paused => MediaPlayback::Paused {
                    progress: status
                        .position
                        .map(|p| MediaPosition(Duration::from_secs_f64(p))),
                },
                PlaybackState::Stopped => MediaPlayback::Stopped,
            };
            let _ = controls.set_playback(playback);
            last.state = Some(status.state);
        }

        thread::sleep(Duration::from_millis(200));
    }
}
