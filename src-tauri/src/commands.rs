//! Tauri command handlers — the bridge between the frontend and Rust backend.
//!
//! Each #[tauri::command] function is callable from the WebView via
//! `invoke("command_name", { args })`. Commands access the audio engine
//! and window manager through Tauri's managed state.

use tauri::State;

use crate::audio::engine::{AudioEngine, EngineStatus};
use crate::audio::eq::EqSettings;
use crate::audio::fft::FftData;
use crate::audio::local::LocalFileSource;

/// Get the current engine status (playback state, position, metadata).
#[tauri::command]
pub fn get_status(engine: State<'_, AudioEngine>) -> EngineStatus {
    engine.status()
}

/// Get the current FFT data for spectrum visualisation.
#[tauri::command]
pub fn get_fft_data(engine: State<'_, AudioEngine>) -> FftData {
    engine.fft_data()
}

/// Pause playback.
#[tauri::command]
pub fn pause(engine: State<'_, AudioEngine>) {
    engine.pause();
}

/// Resume playback after pause.
#[tauri::command]
pub fn resume(engine: State<'_, AudioEngine>) {
    engine.resume();
}

/// Stop playback and unload the current source.
#[tauri::command]
pub fn stop(engine: State<'_, AudioEngine>) {
    engine.stop();
}

/// Seek to a position in seconds.
#[tauri::command]
pub fn seek(engine: State<'_, AudioEngine>, position_secs: f64) {
    engine.seek(std::time::Duration::from_secs_f64(position_secs));
}

/// Update the EQ settings.
#[tauri::command]
pub fn set_eq(engine: State<'_, AudioEngine>, settings: EqSettings) {
    engine.set_eq(settings);
}

/// Set the volume (0.0 to 1.0).
#[tauri::command]
pub fn set_volume(engine: State<'_, AudioEngine>, volume: f32) {
    engine.set_volume(volume);
}

/// Open and play a local audio file.
#[tauri::command]
pub fn play_file(engine: State<'_, AudioEngine>, path: String) -> Result<(), String> {
    let source = LocalFileSource::open(&path).map_err(|e| e.to_string())?;
    engine.play(Box::new(source));
    Ok(())
}
