//! Tauri command handlers — the bridge between the frontend and Rust backend.
//!
//! Each #[tauri::command] function is callable from the WebView via
//! `invoke("command_name", { args })`. Commands access the audio engine
//! and window manager through Tauri's managed state.

use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};

use crate::audio::engine::{AudioEngine, EngineStatus};
use crate::audio::eq::EqSettings;
use crate::audio::fft::FftData;
use crate::audio::local::LocalFileSource;
use crate::audio::source::AudioSource;
use crate::playlist::manager::{PlaylistManager, PlaylistState};
use crate::skin::loader::SkinContents;
use crate::skin::scanner::SkinInfo;
use crate::window::manager::{WindowId, WindowManager, WindowStates};

// -- Engine commands --

#[tauri::command]
pub fn get_status(engine: State<'_, Arc<AudioEngine>>) -> EngineStatus {
    engine.status()
}

#[tauri::command]
pub fn get_fft_data(engine: State<'_, Arc<AudioEngine>>) -> FftData {
    engine.fft_data()
}

#[tauri::command]
pub fn pause(engine: State<'_, Arc<AudioEngine>>) {
    engine.pause();
}

#[tauri::command]
pub fn resume(engine: State<'_, Arc<AudioEngine>>) {
    engine.resume();
}

#[tauri::command]
pub fn stop(engine: State<'_, Arc<AudioEngine>>) {
    engine.stop();
}

#[tauri::command]
pub fn seek(engine: State<'_, Arc<AudioEngine>>, position_secs: f64) {
    engine.seek(std::time::Duration::from_secs_f64(position_secs));
}

#[tauri::command]
pub fn set_eq(engine: State<'_, Arc<AudioEngine>>, settings: EqSettings) {
    engine.set_eq(settings);
}

#[tauri::command]
pub fn set_volume(engine: State<'_, Arc<AudioEngine>>, volume: f32) {
    engine.set_volume(volume);
}

// -- Playlist commands --

/// Add files to the playlist and start playing the first one if nothing
/// is currently playing.
#[tauri::command]
pub fn playlist_add_files(
    engine: State<'_, Arc<AudioEngine>>,
    playlist: State<'_, Arc<Mutex<PlaylistManager>>>,
    paths: Vec<String>,
) -> Result<PlaylistState, String> {
    let mut pl = playlist.lock().map_err(|e| e.to_string())?;
    let was_empty = pl.track_count() == 0;
    let ids = pl.add_tracks(paths);

    // Load metadata for each added track by probing the file.
    for &id in &ids {
        if let Some(track) = pl.get_track(id) {
            let path = track.path.clone();
            if let Ok(source) = LocalFileSource::open(&path) {
                if let Ok(meta) = source.metadata() {
                    pl.update_metadata(id, &meta);
                }
            }
        }
    }

    // Auto-play the first added track if the playlist was empty.
    if was_empty && !ids.is_empty() {
        if let Some(track) = pl.play_index(0) {
            let path = track.path.clone();
            play_path(&engine, &path)?;
        }
    }

    Ok(pl.state())
}

/// Add a single file — convenience wrapper that also starts playback.
#[tauri::command]
pub fn play_file(
    engine: State<'_, Arc<AudioEngine>>,
    playlist: State<'_, Arc<Mutex<PlaylistManager>>>,
    path: String,
) -> Result<(), String> {
    let mut pl = playlist.lock().map_err(|e| e.to_string())?;
    let id = pl.add_track(&path);

    // Load metadata.
    if let Ok(source) = LocalFileSource::open(&path) {
        if let Ok(meta) = source.metadata() {
            pl.update_metadata(id, &meta);
        }
    }

    // Play this track.
    pl.play_track(id);
    drop(pl); // Release lock before engine call.
    play_path(&engine, &path)?;
    Ok(())
}

/// Get the current playlist state.
#[tauri::command]
pub fn get_playlist(
    playlist: State<'_, Arc<Mutex<PlaylistManager>>>,
) -> Result<PlaylistState, String> {
    let pl = playlist.lock().map_err(|e| e.to_string())?;
    Ok(pl.state())
}

/// Play a specific track by index (e.g. user double-clicked in the list).
#[tauri::command]
pub fn playlist_play_index(
    engine: State<'_, Arc<AudioEngine>>,
    playlist: State<'_, Arc<Mutex<PlaylistManager>>>,
    index: usize,
) -> Result<(), String> {
    let mut pl = playlist.lock().map_err(|e| e.to_string())?;
    let track = pl.play_index(index).ok_or("invalid index")?;
    let path = track.path.clone();
    drop(pl);
    play_path(&engine, &path)
}

/// Advance to the next track.
#[tauri::command]
pub fn next_track(
    engine: State<'_, Arc<AudioEngine>>,
    playlist: State<'_, Arc<Mutex<PlaylistManager>>>,
) -> Result<(), String> {
    let mut pl = playlist.lock().map_err(|e| e.to_string())?;
    match pl.next_track() {
        Some(track) => {
            let path = track.path.clone();
            drop(pl);
            play_path(&engine, &path)
        }
        None => {
            drop(pl);
            engine.stop();
            Ok(())
        }
    }
}

/// Go to the previous track.
#[tauri::command]
pub fn previous_track(
    engine: State<'_, Arc<AudioEngine>>,
    playlist: State<'_, Arc<Mutex<PlaylistManager>>>,
) -> Result<(), String> {
    let mut pl = playlist.lock().map_err(|e| e.to_string())?;
    match pl.previous_track() {
        Some(track) => {
            let path = track.path.clone();
            drop(pl);
            play_path(&engine, &path)
        }
        None => Ok(()),
    }
}

/// Toggle shuffle mode.
#[tauri::command]
pub fn toggle_shuffle(
    playlist: State<'_, Arc<Mutex<PlaylistManager>>>,
) -> Result<PlaylistState, String> {
    let mut pl = playlist.lock().map_err(|e| e.to_string())?;
    pl.toggle_shuffle();
    Ok(pl.state())
}

/// Cycle repeat mode: Off → Playlist → Track → Off.
#[tauri::command]
pub fn cycle_repeat(
    playlist: State<'_, Arc<Mutex<PlaylistManager>>>,
) -> Result<PlaylistState, String> {
    let mut pl = playlist.lock().map_err(|e| e.to_string())?;
    pl.cycle_repeat();
    Ok(pl.state())
}

/// Remove selected tracks from the playlist.
#[tauri::command]
pub fn playlist_remove_selected(
    playlist: State<'_, Arc<Mutex<PlaylistManager>>>,
) -> Result<PlaylistState, String> {
    let mut pl = playlist.lock().map_err(|e| e.to_string())?;
    pl.remove_selected();
    Ok(pl.state())
}

/// Clear the entire playlist.
#[tauri::command]
pub fn playlist_clear(
    engine: State<'_, Arc<AudioEngine>>,
    playlist: State<'_, Arc<Mutex<PlaylistManager>>>,
) -> Result<PlaylistState, String> {
    let mut pl = playlist.lock().map_err(|e| e.to_string())?;
    pl.clear();
    drop(pl);
    engine.stop();
    Ok(PlaylistState {
        tracks: vec![],
        current_index: None,
        current_track_id: None,
        shuffle: crate::playlist::sequence::ShuffleMode::Off,
        repeat: crate::playlist::sequence::RepeatMode::Off,
        total_duration: None,
        track_count: 0,
    })
}

// -- Window commands --

/// Toggle a panel window (playlist, equalizer). Creates the window if it
/// doesn't exist yet, otherwise shows/hides it.
#[tauri::command]
pub async fn toggle_window(
    app: AppHandle,
    window_manager: State<'_, Mutex<WindowManager>>,
    window_id: WindowId,
) -> Result<WindowStates, String> {
    let (should_show, label, url, width, height, resizable) = {
        let mut wm = window_manager.lock().map_err(|e| e.to_string())?;
        let should_show = wm.toggle(window_id);
        (
            should_show,
            window_id.label().to_string(),
            window_id.url_path().to_string(),
            window_id.native_width(),
            window_id.native_height(),
            window_id.resizable(),
        )
    };

    eprintln!("[retroamp] toggle_window: id={window_id:?} label={label} should_show={should_show}");

    // Try to find an existing window with this label.
    if let Some(existing) = app.get_webview_window(&label) {
        if should_show {
            existing.show().map_err(|e| e.to_string())?;
            existing.set_focus().map_err(|e| e.to_string())?;
        } else {
            existing.hide().map_err(|e| e.to_string())?;
        }
    } else if should_show {
        // Match the main window's actual dimensions so panels align visually.
        let (main_w, main_h) = app
            .get_webview_window("main")
            .and_then(|win| win.inner_size().ok())
            .map(|s| (s.width as f64, s.height as f64))
            .unwrap_or(((width * 2) as f64, (height * 2) as f64));

        let w = main_w;
        // Playlist height: 2x the main window height by default.
        // EQ: same height as main window.
        let h = if resizable { main_h * 2.0 } else { main_h };

        eprintln!("[retroamp] creating window: label={label} size={w}x{h} (main={main_w}x{main_h})");
        WebviewWindowBuilder::new(&app, &label, WebviewUrl::App(url.into()))
            .title(format!("RetroAmp — {}", label))
            .inner_size(w, h)
            .decorations(false)
            .resizable(resizable)
            .visible(true)
            .build()
            .map_err(|e| {
                eprintln!("[retroamp] window creation failed: {e}");
                e.to_string()
            })?;
    }

    let wm = window_manager.lock().map_err(|e| e.to_string())?;
    Ok(wm.get_states())
}

/// Get the current state of all windows.
#[tauri::command]
pub fn get_window_states(
    window_manager: State<'_, Mutex<WindowManager>>,
) -> Result<WindowStates, String> {
    let wm = window_manager.lock().map_err(|e| e.to_string())?;
    Ok(wm.get_states())
}

/// Cycle the global UI scale (1x → 2x → 3x → 1x).
#[tauri::command]
pub async fn cycle_scale(
    window_manager: State<'_, Mutex<WindowManager>>,
) -> Result<WindowStates, String> {
    let mut wm = window_manager.lock().map_err(|e| e.to_string())?;
    wm.cycle_scale();
    Ok(wm.get_states())
}

/// Set a specific scale value.
#[tauri::command]
pub async fn set_scale(
    window_manager: State<'_, Mutex<WindowManager>>,
    scale: u32,
) -> Result<WindowStates, String> {
    let mut wm = window_manager.lock().map_err(|e| e.to_string())?;
    wm.set_scale(scale);
    Ok(wm.get_states())
}

// -- Skin commands --

/// Load a skin from a .wsz archive or extracted directory.
#[tauri::command]
pub fn load_skin(path: String) -> Result<SkinContents, String> {
    crate::skin::loader::load_skin(&path)
}

/// Set the active skin path (so all windows can pick it up).
#[tauri::command]
pub fn set_active_skin(
    window_manager: State<'_, Mutex<WindowManager>>,
    path: String,
) -> Result<(), String> {
    let mut wm = window_manager.lock().map_err(|e| e.to_string())?;
    wm.set_active_skin_path(path);
    Ok(())
}

/// List all available skins from the skins directories.
#[tauri::command]
pub fn get_skins() -> Vec<SkinInfo> {
    use std::path::PathBuf;

    let mut dirs = Vec::new();

    // Project skins directory (for development).
    let project_skins = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|p| p.join("skins"));
    if let Some(dir) = project_skins {
        dirs.push(dir);
    }

    // XDG user skins directory.
    if let Some(config) = dirs::config_dir() {
        dirs.push(config.join("retroamp").join("skins"));
    }

    crate::skin::scanner::scan_all(&dirs)
}

// -- Internal helpers --

fn play_path(engine: &AudioEngine, path: &str) -> Result<(), String> {
    let source = LocalFileSource::open(path).map_err(|e| e.to_string())?;
    engine.play(Box::new(source));
    Ok(())
}
