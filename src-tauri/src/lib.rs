//! RetroAmp — cross-platform desktop audio player inspired by Winamp 2.x.

pub mod audio;
pub mod commands;
pub mod config;
pub mod context_menu;
pub mod db;
pub mod library;
pub mod media_controls;
pub mod playlist;
pub mod tray;
pub mod radio_browser;
pub mod skin;
pub mod window;

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use tauri::{Emitter, Manager};

use serde::Serialize;

use audio::engine::{AudioEngine, EngineEvent};
use audio::eq::EqSettings;
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

    // Restore persisted playlist from database.
    {
        use crate::playlist::sequence::{ShuffleMode, RepeatMode};
        if let Ok(db) = database.lock() {
            match db.restore_playlist() {
                Ok((paths, current_index, shuffle, repeat)) if !paths.is_empty() => {
                    if let Ok(mut pl) = playlist_manager.lock() {
                        // Only add tracks whose files still exist (skip stale entries).
                        // Streams (URLs) are always kept.
                        let mut index_offset: usize = 0;
                        let mut valid_count: usize = 0;
                        for (i, path) in paths.iter().enumerate() {
                            let is_url = path.starts_with("http://") || path.starts_with("https://");
                            if is_url || std::path::Path::new(path).exists() {
                                pl.add_track(path);
                                valid_count += 1;
                            } else {
                                // Track before current_index was skipped — adjust.
                                if current_index.map_or(false, |ci| i < ci) {
                                    index_offset += 1;
                                }
                            }
                        }

                        // Restore current index (adjusted for skipped tracks).
                        if let Some(ci) = current_index {
                            let adjusted = ci.saturating_sub(index_offset);
                            if adjusted < valid_count {
                                pl.play_index(adjusted);
                                // Don't actually start playback — just set position.
                                // The engine stays stopped; user presses play.
                            }
                        }

                        // Restore shuffle/repeat modes.
                        let shuffle_mode = match shuffle.as_str() {
                            "All" => ShuffleMode::All,
                            _ => ShuffleMode::Off,
                        };
                        let repeat_mode = match repeat.as_str() {
                            "Track" => RepeatMode::Track,
                            "Playlist" => RepeatMode::Playlist,
                            _ => RepeatMode::Off,
                        };
                        pl.set_shuffle(shuffle_mode);
                        pl.set_repeat(repeat_mode);

                        log::info!(
                            "restored playlist: {valid_count} tracks, index={:?}, shuffle={shuffle}, repeat={repeat}",
                            current_index.map(|ci| ci.saturating_sub(index_offset))
                        );
                    }
                }
                Ok(_) => {} // Empty playlist, nothing to restore.
                Err(e) => log::warn!("failed to restore playlist: {e}"),
            }
        }

        // Load metadata for restored playlist tracks in the background.
        {
            let pl_clone = Arc::clone(&playlist_manager);
            thread::Builder::new()
                .name("retroamp-playlist-meta".into())
                .spawn(move || {
                    use crate::audio::local::LocalFileSource;
                    use crate::audio::source::AudioSource;
                    let ids_and_paths: Vec<(u64, String)> = {
                        let Ok(pl) = pl_clone.lock() else { return };
                        let state = pl.state();
                        state.tracks.iter()
                            .filter(|t| !t.is_stream)
                            .map(|t| (t.id, t.path.clone()))
                            .collect()
                    };
                    for (id, path) in ids_and_paths {
                        // Bail out if the playlist was cleared by the user.
                        if let Ok(pl) = pl_clone.lock() {
                            if pl.track_count() == 0 { break; }
                        }
                        if let Ok(source) = LocalFileSource::open(&path) {
                            if let Ok(meta) = source.metadata() {
                                if let Ok(mut pl) = pl_clone.lock() {
                                    pl.update_metadata(id, &meta);
                                }
                            }
                        }
                        // Yield to avoid starving other threads waiting for the lock.
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    log::info!("playlist metadata loading complete");
                })
                .ok();
        }
    }

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
        .manage(Mutex::new(commands::SkinCache::new()))
        .manage(Arc::clone(&database))
        .setup(move |app| {
            // Start OS media controls (MPRIS on Linux, SMTC on Windows,
            // MPRemoteCommandCenter on macOS). Non-fatal if it fails.
            {
                let engine_mc: Arc<AudioEngine> =
                    Arc::clone(&*app.state::<Arc<AudioEngine>>());
                let playlist_mc: Arc<Mutex<PlaylistManager>> =
                    Arc::clone(&*app.state::<Arc<Mutex<PlaylistManager>>>());

                // On Windows, souvlaki needs the main window's HWND.
                #[allow(unused_mut)]
                let mut hwnd: Option<*mut std::ffi::c_void> = None;
                #[cfg(target_os = "windows")]
                {
                    if let Some(win) = app.get_webview_window("main") {
                        use raw_window_handle::HasWindowHandle;
                        if let Ok(handle) = win.window_handle() {
                            if let raw_window_handle::RawWindowHandle::Win32(h) = handle.as_raw() {
                                hwnd = Some(h.hwnd.get() as *mut std::ffi::c_void);
                            }
                        }
                    }
                }

                match media_controls::MediaService::new(engine_mc, playlist_mc, hwnd) {
                    Ok(service) => {
                        app.manage(service);
                        log::info!("OS media controls registered");
                    }
                    Err(e) => {
                        log::warn!("Failed to register OS media controls: {e}");
                    }
                }
            }

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

            // Seed default radio stations (best-effort, non-blocking).
            {
                let db = Arc::clone(&database);
                thread::Builder::new()
                    .name("retroamp-radio-seed".into())
                    .spawn(move || {
                        if let Ok(db) = db.lock() {
                            match db.seed_default_stations() {
                                Ok(n) if n > 0 => log::info!("seeded {n} default radio stations"),
                                Ok(_) => {}
                                Err(e) => log::warn!("failed to seed radio stations: {e}"),
                            }
                        }
                    })
                    .ok();
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

            // Pre-create ALL panel windows at startup.  On Wayland,
            // creating a new WebView while existing WebViews are active
            // corrupts GTK's internal pointer-event state, permanently
            // breaking startDragging/close/corner-resize for every window.
            // By creating all windows during setup (when the GTK main loop
            // is in a clean state with no active WebViews) we avoid this.
            // Windows that were not visible at last close are created hidden.
            let default_layout = config::WindowLayoutEntry::default();
            let radio_layout = saved_ui.radio_browser.as_ref().unwrap_or(&default_layout);
            let library_layout = saved_ui.library_browser.as_ref().unwrap_or(&default_layout);
            let settings_layout = saved_ui.settings.as_ref().unwrap_or(&default_layout);

            let all_panels: &[(WindowId, &config::WindowLayoutEntry)] = &[
                (WindowId::Equalizer, &saved_ui.equalizer),
                (WindowId::Playlist, &saved_ui.playlist),
                (WindowId::RadioBrowser, radio_layout),
                (WindowId::LibraryBrowser, library_layout),
                (WindowId::Settings, settings_layout),
            ];

            // Derive default size from main window (computed once).
            let (main_w, main_h) = app
                .get_webview_window("main")
                .and_then(|win| {
                    let size = win.inner_size().ok()?;
                    let sf = win.scale_factor().ok().unwrap_or(1.0);
                    Some((size.width as f64 / sf, size.height as f64 / sf))
                })
                .unwrap_or((w, h));

            for &(id, entry) in all_panels {
                let label = id.label();
                let url = id.url_path();
                let resizable = id.resizable();
                let was_visible = entry.visible == Some(true);

                if was_visible {
                    if let Ok(mut wm) = app.state::<std::sync::Mutex<WindowManager>>().lock() {
                        wm.set_visible(id, true);
                    }
                }

                let default_w = match id {
                    WindowId::RadioBrowser => main_w * 1.5,
                    WindowId::Settings => 700.0,
                    _ => main_w,
                };
                let win_w = if resizable { entry.width.unwrap_or(default_w) } else { main_w };
                let win_h = if resizable {
                    match id {
                        WindowId::Settings => entry.height.unwrap_or(500.0),
                        _ => entry.height.unwrap_or(main_h * 2.0),
                    }
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
                .visible(was_visible)
                .skip_taskbar(true);

                if !resizable {
                    builder = builder
                        .min_inner_size(win_w, win_h)
                        .max_inner_size(win_w, win_h);
                }
                if id == WindowId::Settings {
                    builder = builder.min_inner_size(500.0, 400.0);
                }

                if let (Some(x), Some(y)) = (entry.x, entry.y) {
                    builder = builder.position(x as f64, y as f64);
                }

                if let Some(main_win) = app.get_webview_window("main") {
                    builder = match builder.parent(&main_win) {
                        Ok(b) => b,
                        Err(e) => {
                            eprintln!("[retroamp] failed to set parent for {label}: {e}");
                            continue;
                        }
                    };
                }

                match builder.build() {
                    Ok(_) => eprintln!("[retroamp] pre-created window: {label} ({win_w}x{win_h}) visible={was_visible}"),
                    Err(e) => eprintln!("[retroamp] failed to create {label}: {e}"),
                }
            }

            // Pre-create the tag editor window (hidden, no file loaded).
            {
                let mut builder = tauri::WebviewWindowBuilder::new(
                    app,
                    "tageditor",
                    tauri::WebviewUrl::App("/?window=tageditor".into()),
                )
                .title("RetroAmp \u{2014} Tag Editor")
                .inner_size(550.0, 500.0)
                .min_inner_size(450.0, 400.0)
                .decorations(false)
                .resizable(true)
                .visible(false)
                .skip_taskbar(true);

                if let Some(main_win) = app.get_webview_window("main") {
                    builder = match builder.parent(&main_win) {
                        Ok(b) => b,
                        Err(e) => {
                            eprintln!("[retroamp] tag editor parent failed: {e}");
                            // parent() consumed builder — rebuild without parent
                            tauri::WebviewWindowBuilder::new(
                                app, "tageditor",
                                tauri::WebviewUrl::App("/?window=tageditor".into()),
                            )
                            .title("RetroAmp \u{2014} Tag Editor")
                            .inner_size(550.0, 500.0)
                            .min_inner_size(450.0, 400.0)
                            .decorations(false)
                            .resizable(true)
                            .visible(false)
                            .skip_taskbar(true)
                        }
                    };
                }
                match builder.build() {
                    Ok(_) => eprintln!("[retroamp] pre-created tag editor (hidden)"),
                    Err(e) => eprintln!("[retroamp] failed to create tag editor: {e}"),
                }
            }

            // Set up system tray (non-fatal).
            match tray::setup(app.handle()) {
                Ok(()) => log::info!("system tray initialized"),
                Err(e) => log::warn!("failed to set up system tray: {e}"),
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            // For secondary windows, intercept close and hide instead —
            // windows are pre-created at startup and must not be destroyed.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let label = window.label();
                if label != "main" && label != "shade" {
                    api.prevent_close();
                    let _ = window.hide();
                    // Update WindowManager state.
                    let window_id = match label {
                        "playlist" => Some(WindowId::Playlist),
                        "equalizer" => Some(WindowId::Equalizer),
                        "settings" => Some(WindowId::Settings),
                        "radiobrowser" => Some(WindowId::RadioBrowser),
                        "librarybrowser" => Some(WindowId::LibraryBrowser),
                        _ => None,
                    };
                    if let Some(id) = window_id {
                        if let Some(wm) = window.try_state::<Mutex<WindowManager>>() {
                            if let Ok(mut wm) = wm.lock() {
                                wm.set_visible(id, false);
                            }
                        }
                    }
                    return;
                }

                if label == "main" {
                    if let Some(wm) = window.try_state::<Mutex<WindowManager>>() {
                        if let Ok(wm) = wm.lock() {
                            let states = wm.get_states();
                            drop(wm);
                            commands::save_window_layout(window.app_handle(), &states);
                        }
                    }
                    let db = Arc::clone(&*window.state::<Arc<Mutex<Database>>>());
                    let pl = Arc::clone(&*window.state::<Arc<Mutex<PlaylistManager>>>());
                    commands::save_playlist_state(&db, &pl);
                }
            }

            if let tauri::WindowEvent::Destroyed = event {
                let label = window.label();

                if label == "main" {
                    // Main window closed — exit the entire application.
                    // Child windows are destroyed by the parent relationship.
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
                    "radiobrowser" => Some(WindowId::RadioBrowser),
                    "librarybrowser" => Some(WindowId::LibraryBrowser),
                    _ => None,
                };
                if let Some(id) = window_id {
                    if let Some(wm) = window.try_state::<Mutex<WindowManager>>() {
                        // Use try_lock to avoid deadlock if toggle_window
                        // holds the lock on the same thread.  If we can't
                        // acquire it, that's OK — toggle_window or the
                        // reconcile pass in get_window_states will fix it.
                        if let Ok(mut wm) = wm.try_lock() {
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
            commands::playlist_select_track,
            commands::playlist_toggle_select,
            commands::playlist_remove_selected,
            commands::playlist_remove_tracks,
            commands::playlist_clear,
            commands::playlist_save,
            commands::playlist_load,
            commands::play_url,
            commands::playlist_add_url,
            // Radio browser
            commands::get_radio_stations,
            commands::get_favorite_stations,
            commands::search_radio_stations_local,
            commands::toggle_station_favorite,
            commands::hide_radio_station,
            commands::unhide_radio_station,
            commands::delete_radio_station,
            commands::save_radio_station,
            commands::radio_browser_search,
            commands::radio_browser_top,
            commands::radio_browser_by_tag,
            // Skin
            commands::load_skin,
            commands::get_skins,
            commands::get_skins_dir,
            commands::set_active_skin,
            commands::get_last_skin_path,
            commands::import_skins,
            commands::open_skins_folder,
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
            // Library
            commands::scan_library,
            commands::get_scan_status,
            commands::get_library_dirs,
            commands::add_library_dir,
            commands::remove_library_dir,
            commands::get_library_tracks,
            commands::search_library,
            commands::get_library_artists,
            commands::get_library_albums,
            commands::get_library_genres,
            commands::get_library_cover,
            commands::get_library_track_count,
            commands::set_track_rating,
            commands::get_tracks_by_artist,
            commands::get_tracks_by_album,
            commands::get_tracks_by_genre,
            commands::reveal_in_file_manager,
            commands::get_playlist_add_mode,
            commands::set_playlist_add_mode,
            commands::get_library_columns,
            commands::set_library_columns,
            // Browser view state persistence
            commands::get_library_view_state,
            commands::set_library_view_state,
            commands::get_radio_view_state,
            commands::set_radio_view_state,
            // Column width persistence
            commands::get_library_column_widths,
            commands::set_library_column_widths,
            commands::get_radio_column_widths,
            commands::set_radio_column_widths,
            // Tag editor
            commands::read_track_tags,
            commands::write_track_tags,
            commands::open_tag_editor,
            // Context menu
            context_menu::show_context_menu,
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

    // Scan the skins directory.
    let Some(dir) = dirs::config_dir().map(|c| c.join("retroamp").join("skins")) else {
        log::warn!("could not determine skins directory, skipping catalog sync");
        return;
    };
    let _ = std::fs::create_dir_all(&dir);
    let skins = skin::scanner::scan_all(&[dir]);
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

                        match commands::create_source(&path) {
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
                                engine.play(source);
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
