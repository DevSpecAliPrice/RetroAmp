//! RetroAmp — cross-platform desktop audio player inspired by Winamp 2.x.

pub mod audio;
pub mod commands;
pub mod window;

use audio::engine::AudioEngine;
use window::manager::WindowManager;

/// Build and configure the Tauri application.
///
/// This is the main entry point called by both the desktop binary and any
/// test harnesses. It sets up managed state (audio engine, window manager)
/// and registers all Tauri commands.
pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // Initialise the audio engine. If this fails (e.g. no audio device),
    // we still want the app to launch — the UI should show an error state
    // rather than crashing.
    let engine = match AudioEngine::new() {
        Ok(engine) => engine,
        Err(e) => {
            log::error!("Failed to initialise audio engine: {e}");
            log::error!("RetroAmp will launch without audio capability.");
            // TODO: Create a dummy/disabled engine so the app can still run
            // For now, exit early — this will be handled properly when we
            // implement graceful degradation.
            eprintln!("Fatal: failed to initialise audio engine: {e}");
            std::process::exit(1);
        }
    };

    let window_manager = WindowManager::new();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(engine)
        .manage(std::sync::Mutex::new(window_manager))
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::get_fft_data,
            commands::play_file,
            commands::pause,
            commands::resume,
            commands::stop,
            commands::seek,
            commands::set_eq,
            commands::set_volume,
        ])
        .run(tauri::generate_context!())
        .expect("error while running RetroAmp");
}
