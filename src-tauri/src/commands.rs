//! Tauri command handlers — the bridge between the frontend and Rust backend.
//!
//! Each #[tauri::command] function is callable from the WebView via
//! `invoke("command_name", { args })`. Commands access the audio engine
//! and window manager through Tauri's managed state.

use std::sync::{Arc, Mutex};

use tauri::State;

use crate::audio::engine::{AudioEngine, EngineStatus};
use crate::audio::eq::EqSettings;
use crate::audio::fft::FftData;
use crate::audio::local::LocalFileSource;
use crate::audio::source::AudioSource;
use crate::playlist::manager::{PlaylistManager, PlaylistState};
use crate::skin::loader::SkinContents;

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

// -- Skin commands --

/// Load a .wsz skin file and return its contents for the frontend to render.
#[tauri::command]
pub fn load_skin(path: String) -> Result<SkinContents, String> {
    crate::skin::loader::load_wsz(&path)
}

// -- Internal helpers --

fn play_path(engine: &AudioEngine, path: &str) -> Result<(), String> {
    let source = LocalFileSource::open(path).map_err(|e| e.to_string())?;
    engine.play(Box::new(source));
    Ok(())
}
