//! RetroAmp — cross-platform desktop audio player inspired by Winamp 2.x.

pub mod audio;
pub mod commands;
pub mod playlist;
pub mod skin;
pub mod window;

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use tauri::Manager;

use audio::engine::{AudioEngine, EngineEvent};
use audio::local::LocalFileSource;
use audio::source::AudioSource;
use playlist::manager::PlaylistManager;
use window::manager::{WindowId, WindowManager};

pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let engine = match AudioEngine::new() {
        Ok(engine) => engine,
        Err(e) => {
            eprintln!("Fatal: failed to initialise audio engine: {e}");
            std::process::exit(1);
        }
    };

    let engine = Arc::new(engine);
    let playlist_manager = Arc::new(Mutex::new(PlaylistManager::new()));
    let window_manager = WindowManager::new();

    // Spawn the auto-advance listener. When the engine signals that a track
    // has finished, this thread advances the playlist and feeds the next
    // track to the engine.
    {
        let engine = Arc::clone(&engine);
        let playlist = Arc::clone(&playlist_manager);
        thread::Builder::new()
            .name("retroamp-auto-advance".into())
            .spawn(move || {
                auto_advance_loop(engine, playlist);
            })
            .expect("failed to spawn auto-advance thread");
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(engine)
        .manage(playlist_manager)
        .manage(Mutex::new(window_manager))
        .on_window_event(|window, event| {
            // When a secondary window is closed by the compositor (e.g. user
            // clicks the X), update the WindowManager so the PL/EQ buttons
            // reflect the correct state.
            if let tauri::WindowEvent::Destroyed = event {
                let label = window.label();
                let window_id = match label {
                    "playlist" => Some(WindowId::Playlist),
                    "equalizer" => Some(WindowId::Equalizer),
                    _ => None,
                };
                if let Some(id) = window_id {
                    if let Some(wm) = window.try_state::<Mutex<WindowManager>>() {
                        if let Ok(mut wm) = wm.lock() {
                            wm.set_visible(id, false);
                        }
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            // Engine
            commands::get_status,
            commands::get_fft_data,
            commands::pause,
            commands::resume,
            commands::stop,
            commands::seek,
            commands::set_eq,
            commands::set_volume,
            // Playlist
            commands::play_file,
            commands::playlist_add_files,
            commands::get_playlist,
            commands::playlist_play_index,
            commands::next_track,
            commands::previous_track,
            commands::toggle_shuffle,
            commands::cycle_repeat,
            commands::playlist_remove_selected,
            commands::playlist_clear,
            // Skin
            commands::load_skin,
            // Windows
            commands::toggle_window,
            commands::get_window_states,
            commands::cycle_scale,
            commands::set_scale,
        ])
        .run(tauri::generate_context!())
        .expect("error while running RetroAmp");
}

/// Polls the engine for TrackFinished events and advances the playlist.
fn auto_advance_loop(engine: Arc<AudioEngine>, playlist: Arc<Mutex<PlaylistManager>>) {
    loop {
        match engine.try_recv_event() {
            Some(EngineEvent::TrackFinished) => {
                let mut pl = match playlist.lock() {
                    Ok(pl) => pl,
                    Err(_) => continue,
                };

                match pl.next_track() {
                    Some(track) => {
                        let path = track.path.clone();
                        drop(pl); // Release lock before engine call.

                        match LocalFileSource::open(&path) {
                            Ok(source) => {
                                // Update metadata if not already loaded.
                                if let Ok(meta) = source.metadata() {
                                    if let Ok(mut pl) = playlist.lock() {
                                        if let Some(track) = pl.current_track() {
                                            let id = track.id;
                                            pl.update_metadata(id, &meta);
                                        }
                                    }
                                }
                                engine.play(Box::new(source));
                                log::info!("auto-advance: playing {path}");
                            }
                            Err(e) => {
                                log::error!("auto-advance: failed to open {path}: {e}");
                                // Try the next track if this one fails.
                                // (Don't loop infinitely — just try once more.)
                            }
                        }
                    }
                    None => {
                        log::info!("auto-advance: end of playlist");
                    }
                }
            }
            None => {
                // No event — sleep briefly before polling again.
                thread::sleep(Duration::from_millis(50));
            }
        }
    }
}
