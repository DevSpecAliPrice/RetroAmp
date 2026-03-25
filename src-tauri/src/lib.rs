//! RetroAmp — cross-platform desktop audio player inspired by Winamp 2.x.

pub mod audio;
pub mod commands;
pub mod config;
pub mod db;
pub mod playlist;
pub mod skin;
pub mod window;

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use tauri::{Emitter, Manager};

use serde::Serialize;

use audio::engine::{AudioEngine, EngineEvent};
use audio::eq::EqSettings;
use audio::local::LocalFileSource;
use audio::source::AudioSource;
use db::Database;
use playlist::manager::PlaylistManager;
use window::manager::{WindowId, WindowManager};

pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    // Ensure the built-in default skin is present in the user's skins directory.
    skin::default::ensure_default_skin();

    let engine = match AudioEngine::new() {
        Ok(engine) => engine,
        Err(e) => {
            eprintln!("Fatal: failed to initialise audio engine: {e}");
            std::process::exit(1);
        }
    };

    let engine = Arc::new(engine);
    let playlist_manager = Arc::new(Mutex::new(PlaylistManager::new()));
    // Load persisted EQ settings from config, falling back to defaults.
    let saved_eq = {
        let cfg = config::AppConfig::load();
        EqSettings {
            gains: cfg.eq.gains,
            enabled: cfg.eq.enabled,
            preamp: cfg.eq.preamp,
        }
    };
    engine.set_eq(saved_eq.clone());
    let eq_settings = Arc::new(Mutex::new(saved_eq));
    // Load saved UI layout. Apply saved scale if present.
    let saved_ui = config::AppConfig::load().ui;
    let mut window_manager = WindowManager::new();
    if let Some(scale) = saved_ui.scale {
        window_manager.set_scale(scale);
    }

    let database = match Database::open() {
        Ok(db) => Arc::new(Mutex::new(db)),
        Err(e) => {
            eprintln!("Warning: failed to open database: {e}");
            // Create an in-memory fallback so the app still works.
            Arc::new(Mutex::new(
                Database::open_at(std::path::Path::new(":memory:"))
                    .expect("in-memory database should never fail"),
            ))
        }
    };

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

    // Capture scale before moving window_manager into Tauri state.
    let initial_scale = window_manager.scale() as f64;

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(engine)
        .manage(playlist_manager)
        .manage(eq_settings)
        .manage(Mutex::new(window_manager))
        .manage(Arc::clone(&database))
        .setup(move |app| {
            // Sync the skin catalog in the background so startup isn't blocked.
            {
                let db = Arc::clone(&database);
                let app_handle = app.handle().clone();
                thread::Builder::new()
                    .name("retroamp-catalog-sync".into())
                    .spawn(move || {
                        sync_skin_catalog(db, app_handle);
                    })
                    .expect("failed to spawn catalog sync thread");
            }

            // Create the main window programmatically (same code path as
            // EQ/playlist windows) so Wayland handles it consistently.
            let w = 275.0 * initial_scale;
            let h = 116.0 * initial_scale;
            eprintln!("[retroamp] creating main window: {w}x{h} (scale={initial_scale})");

            tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::App("/".into()),
            )
            .title("RetroAmp")
            .inner_size(w, h)
            .min_inner_size(w, h)
            .max_inner_size(w, h)
            .decorations(false)
            .resizable(true)
            .visible(true)
            .build()?;

            // Apply saved main window position (X11/Windows/macOS; no-op on Wayland).
            if let Some(win) = app.get_webview_window("main") {
                if let (Some(x), Some(y)) = (saved_ui.main.x, saved_ui.main.y) {
                    let _ = win.set_position(tauri::Position::Physical(
                        tauri::PhysicalPosition::new(x, y),
                    ));
                }
                let actual = win.inner_size().unwrap_or_default();
                let sf = win.scale_factor().unwrap_or(1.0);
                eprintln!(
                    "[retroamp] main window ACTUAL: {}x{} physical, scale_factor={sf}, logical={}x{}",
                    actual.width, actual.height, actual.width as f64 / sf, actual.height as f64 / sf
                );
            }

            // Restore previously-open panel windows.
            let panels_to_restore: Vec<(WindowId, &config::WindowLayoutEntry)> = [
                (WindowId::Equalizer, &saved_ui.equalizer),
                (WindowId::Playlist, &saved_ui.playlist),
            ]
            .into_iter()
            .filter(|(_, entry)| entry.visible == Some(true))
            .collect();

            for (id, entry) in panels_to_restore {
                let label = id.label();
                let url = id.url_path();
                let resizable = id.resizable();

                // Mark visible in the window manager.
                if let Ok(mut wm) = app.state::<std::sync::Mutex<WindowManager>>().lock() {
                    wm.set_visible(id, true);
                }

                // Derive default size from main window.
                let (main_w, main_h) = app
                    .get_webview_window("main")
                    .and_then(|win| {
                        let size = win.inner_size().ok()?;
                        let sf = win.scale_factor().ok().unwrap_or(1.0);
                        Some((size.width as f64 / sf, size.height as f64 / sf))
                    })
                    .unwrap_or((w, h));

                let default_w = if id == WindowId::Playlist { main_w * 1.15 } else { main_w };
                let win_w = if resizable { entry.width.unwrap_or(default_w) } else { main_w };
                let win_h = if resizable {
                    entry.height.unwrap_or(main_h * 2.0)
                } else {
                    main_h
                };

                let mut builder = tauri::WebviewWindowBuilder::new(
                    app,
                    label,
                    tauri::WebviewUrl::App(url.into()),
                )
                .title(format!("RetroAmp — {label}"))
                .inner_size(win_w, win_h)
                .decorations(false)
                .resizable(true)
                .visible(true)
                .skip_taskbar(true);

                if !resizable {
                    builder = builder
                        .min_inner_size(win_w, win_h)
                        .max_inner_size(win_w, win_h);
                }

                if let (Some(x), Some(y)) = (entry.x, entry.y) {
                    builder = builder.position(x as f64, y as f64);
                }

                // parent() takes ownership and may fail — handle both paths.
                let build_result = if let Some(main_win) = app.get_webview_window("main") {
                    match builder.parent(&main_win) {
                        Ok(b) => b.build(),
                        Err(_) => { continue; }
                    }
                } else {
                    builder.build()
                };

                match build_result {
                    Ok(_) => eprintln!("[retroamp] restored window: {label} ({win_w}x{win_h})"),
                    Err(e) => eprintln!("[retroamp] failed to restore {label}: {e}"),
                }
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            // Save window layout before the main window closes.
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                if window.label() == "main" {
                    if let Some(wm) = window.try_state::<Mutex<WindowManager>>() {
                        if let Ok(wm) = wm.lock() {
                            commands::save_window_layout(window.app_handle(), &wm);
                        }
                    }
                }
            }

            if let tauri::WindowEvent::Destroyed = event {
                let label = window.label();

                if label == "main" {
                    // Main window closed — exit the entire application.
                    // Child windows are destroyed automatically by the parent
                    // relationship, but we also need to stop the audio engine
                    // and exit the process.
                    std::process::exit(0);
                }

                // Shade window closed — restore main window.
                if label == "shade" {
                    if let Some(main) = window.app_handle().get_webview_window("main") {
                        let _ = main.show();
                        let _ = main.set_focus();
                    }
                }

                // Secondary window closed — update WindowManager state
                // so PL/EQ buttons reflect the correct state.
                let window_id = match label {
                    "playlist" => Some(WindowId::Playlist),
                    "equalizer" => Some(WindowId::Equalizer),
                    "settings" => Some(WindowId::Settings),
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
            commands::get_eq,
            commands::set_eq,
            commands::save_eq_preset,
            commands::get_eq_presets,
            commands::delete_eq_preset,
            commands::set_volume,
            commands::set_balance,
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
            commands::get_skins,
            commands::get_skins_dir,
            commands::set_active_skin,
            commands::get_last_skin_path,
            commands::add_skin_dir,
            commands::remove_skin_dir,
            commands::get_extra_skin_dirs,
            commands::delete_skin,
            commands::reveal_skin_folder,
            // Skin catalog (database-backed)
            commands::get_skin_catalog,
            commands::get_skin_thumbnails,
            commands::toggle_skin_favorite,
            commands::get_recent_skins,
            commands::refresh_skin_catalog,
            // Settings
            commands::open_settings,
            // Windows
            commands::toggle_window,
            commands::get_window_states,
            commands::cycle_scale,
            commands::set_scale,
            commands::enter_shade,
            commands::exit_shade,
        ])
        .run(tauri::generate_context!())
        .expect("error while running RetroAmp");
}

/// Progress event emitted during skin catalog sync.
#[derive(Clone, Serialize)]
struct CatalogSyncProgress {
    current: usize,
    total: usize,
    phase: &'static str,
    skin_name: String,
}

/// Scan the filesystem for skins and sync the results into the SQLite catalog.
/// Runs in a background thread so it doesn't block startup.
///
/// Key design: the DB lock is acquired and released per-skin so that other
/// commands (like `set_active_skin`) are never blocked for long.
fn sync_skin_catalog(db: Arc<Mutex<Database>>, app: tauri::AppHandle) {
    log::info!("starting skin catalog sync");

    let _ = app.emit("catalog-sync-progress", CatalogSyncProgress {
        current: 0, total: 0, phase: "scanning", skin_name: String::new(),
    });

    // Gather scan directories (same logic as commands::get_skins).
    let mut dirs = Vec::new();
    if let Some(dir) = dirs::config_dir().map(|c| c.join("retroamp").join("skins")) {
        let _ = std::fs::create_dir_all(&dir);
        dirs.push(dir);
    }
    for dir in config::AppConfig::load().skins.extra_dirs {
        if dir.is_dir() {
            dirs.push(dir);
        }
    }
    if cfg!(debug_assertions) {
        if let Some(dir) = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(|p| p.join("skins"))
        {
            if dir.is_dir() {
                dirs.push(dir);
            }
        }
    }

    let skins = skin::scanner::scan_all(&dirs);
    let total = skins.len();
    let valid_paths: Vec<String> = skins.iter().map(|s| s.path.clone()).collect();

    // First pass: upsert all skins without thumbnails (fast — just metadata).
    // This uses brief per-batch locks so other commands aren't blocked.
    let _ = app.emit("catalog-sync-progress", CatalogSyncProgress {
        current: 0, total, phase: "indexing", skin_name: String::new(),
    });

    for (i, chunk) in skins.chunks(50).enumerate() {
        if let Ok(db) = db.lock() {
            for skin_info in chunk {
                let _ = db.upsert_skin(skin_info, None);
            }
        }
        let done = ((i + 1) * 50).min(total);
        let _ = app.emit("catalog-sync-progress", CatalogSyncProgress {
            current: done, total, phase: "indexing", skin_name: String::new(),
        });
    }

    // Prune skins that no longer exist on disk (brief lock).
    if let Ok(db) = db.lock() {
        match db.remove_missing(&valid_paths) {
            Ok(0) => {}
            Ok(n) => log::info!("pruned {n} missing skins from catalog"),
            Err(e) => log::warn!("failed to prune missing skins: {e}"),
        }
    }

    // Second pass: generate thumbnails for skins that need them.
    // Get all existing thumbnails in ONE lock acquisition, then filter in Rust.
    let has_thumbs = db.lock().ok()
        .and_then(|d| d.paths_with_thumbnails().ok())
        .unwrap_or_default();

    let needs_thumbs: Vec<&skin::scanner::SkinInfo> = skins.iter()
        .filter(|s| !has_thumbs.contains(&s.path))
        .collect();

    let thumb_total = needs_thumbs.len();
    if thumb_total > 0 {
        log::info!("generating thumbnails for {thumb_total} skins");
    }

    for (i, skin_info) in needs_thumbs.iter().enumerate() {
        // Extract thumbnail WITHOUT holding the lock.
        let thumbnail = skin::thumbnail::extract_thumbnail(&skin_info.path);

        // Brief lock to write the result.
        if let Some(thumb) = thumbnail {
            if let Ok(db) = db.lock() {
                let _ = db.upsert_skin(skin_info, Some(thumb));
            }
        }

        // Emit progress every 10 skins to avoid event spam.
        if i % 10 == 0 || i == thumb_total - 1 {
            let _ = app.emit("catalog-sync-progress", CatalogSyncProgress {
                current: i + 1,
                total: thumb_total,
                phase: "thumbnails",
                skin_name: skin_info.name.clone(),
            });
        }
    }

    let _ = app.emit("catalog-sync-progress", CatalogSyncProgress {
        current: total, total, phase: "done", skin_name: String::new(),
    });

    log::info!("skin catalog sync complete: {total} skins, {thumb_total} thumbnails generated");
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
