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
use crate::audio::radio::RadioSource;
use crate::audio::source::AudioSource;
use crate::db::{Database, EqPresetEntry, SkinCatalogEntry};
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
pub fn get_eq(
    eq_settings: State<'_, Arc<Mutex<EqSettings>>>,
) -> Result<EqSettings, String> {
    let s = eq_settings.lock().map_err(|e| e.to_string())?;
    Ok(s.clone())
}

#[tauri::command]
pub fn set_eq(
    engine: State<'_, Arc<AudioEngine>>,
    eq_settings: State<'_, Arc<Mutex<EqSettings>>>,
    settings: EqSettings,
) {
    engine.set_eq(settings.clone());
    if let Ok(mut s) = eq_settings.lock() {
        *s = settings.clone();
    }

    // Persist to config (best-effort).
    let mut cfg = crate::config::AppConfig::load();
    cfg.eq = crate::config::EqConfig {
        gains: settings.gains,
        enabled: settings.enabled,
        preamp: settings.preamp,
    };
    let _ = cfg.save();
}

// -- EQ preset commands --

#[tauri::command]
pub fn save_eq_preset(
    database: State<'_, Arc<Mutex<Database>>>,
    name: String,
    gains: [f32; 10],
    preamp: f32,
) -> Result<EqPresetEntry, String> {
    let db = database.lock().map_err(|e| e.to_string())?;
    db.save_eq_preset(&name, &gains, preamp)
}

#[tauri::command]
pub fn get_eq_presets(
    database: State<'_, Arc<Mutex<Database>>>,
) -> Result<Vec<EqPresetEntry>, String> {
    let db = database.lock().map_err(|e| e.to_string())?;
    db.get_eq_presets()
}

#[tauri::command]
pub fn delete_eq_preset(
    database: State<'_, Arc<Mutex<Database>>>,
    name: String,
) -> Result<(), String> {
    let db = database.lock().map_err(|e| e.to_string())?;
    db.delete_eq_preset(&name)
}

#[tauri::command]
pub fn set_volume(engine: State<'_, Arc<AudioEngine>>, volume: f32) {
    engine.set_volume(volume);
}

#[tauri::command]
pub fn set_balance(engine: State<'_, Arc<AudioEngine>>, balance: f32) {
    engine.set_balance(balance);
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
    use crate::audio::playlist_parser;

    let mut pl = playlist.lock().map_err(|e| e.to_string())?;
    let was_empty = pl.track_count() == 0;

    let mut ids = Vec::new();

    for path in &paths {
        if playlist_parser::is_playlist_path(path) {
            // Parse M3U/PLS file and add extracted URLs.
            if let Ok(content) = std::fs::read_to_string(path) {
                let entries = playlist_parser::parse_playlist(&content);
                for entry in entries {
                    let id = pl.add_track(&entry.url);
                    if let Some(title) = entry.title {
                        pl.update_display_name(id, &title);
                    }
                    ids.push(id);
                }
            }
        } else {
            let id = pl.add_track(path);
            ids.push(id);
        }
    }

    // Load metadata for local files (skip streams — they load metadata on play).
    for &id in &ids {
        if let Some(track) = pl.get_track(id) {
            if !track.is_stream {
                let path = track.path.clone();
                if let Ok(source) = LocalFileSource::open(&path) {
                    if let Ok(meta) = source.metadata() {
                        pl.update_metadata(id, &meta);
                    }
                }
            }
        }
    }

    // Auto-play the first added track if the playlist was empty.
    if was_empty && !ids.is_empty() {
        if let Some(track) = pl.play_index(0) {
            let path = track.path.clone();
            drop(pl);
            play_path(&engine, &path)?;
            return Ok(playlist.lock().map_err(|e| e.to_string())?.state());
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

// -- Window layout persistence --

/// Capture the current window layout (visibility, positions, sizes) and save
/// to config. Called on toggle, scale change, and app exit. Position reads
/// are best-effort — on Wayland `outer_position()` may return (0,0).
pub fn save_window_layout(app: &AppHandle, wm: &WindowManager) {
    use crate::config::WindowLayoutEntry;

    let mut cfg = crate::config::AppConfig::load();
    cfg.ui.scale = Some(wm.scale());

    let capture = |label: &str, visible: bool, resizable: bool| -> WindowLayoutEntry {
        let mut entry = WindowLayoutEntry {
            visible: Some(visible),
            ..Default::default()
        };
        if let Some(win) = app.get_webview_window(label) {
            // Position (logical). On Wayland this may silently return (0,0).
            if let Ok(pos) = win.outer_position() {
                entry.x = Some(pos.x);
                entry.y = Some(pos.y);
            }
            // Size (logical, from physical / scale_factor).
            if resizable {
                if let Ok(size) = win.inner_size() {
                    let sf = win.scale_factor().unwrap_or(1.0);
                    entry.width = Some(size.width as f64 / sf);
                    entry.height = Some(size.height as f64 / sf);
                }
            }
        }
        entry
    };

    cfg.ui.main = capture("main", true, false);
    cfg.ui.equalizer = capture("equalizer", wm.is_visible(WindowId::Equalizer), false);
    cfg.ui.playlist = capture("playlist", wm.is_visible(WindowId::Playlist), true);
    if wm.is_visible(WindowId::RadioBrowser) || app.get_webview_window("radiobrowser").is_some() {
        cfg.ui.radio_browser = Some(capture("radiobrowser", wm.is_visible(WindowId::RadioBrowser), true));
    }
    if wm.is_visible(WindowId::Settings) || app.get_webview_window("settings").is_some() {
        cfg.ui.settings = Some(capture("settings", wm.is_visible(WindowId::Settings), true));
    }

    let _ = cfg.save();
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
        // Check for saved layout for this window.
        let saved = {
            let cfg = crate::config::AppConfig::load();
            match window_id {
                WindowId::Equalizer => cfg.ui.equalizer,
                WindowId::Playlist => cfg.ui.playlist,
                WindowId::RadioBrowser => cfg.ui.radio_browser.unwrap_or_default(),
                WindowId::Settings => cfg.ui.settings.unwrap_or_default(),
                _ => Default::default(),
            }
        };

        // Derive panel size from the main window's actual logical dimensions
        // so all panels share exactly the same width. We convert the main
        // window's physical inner_size back to logical using its scale_factor.
        let (main_w, main_h) = app
            .get_webview_window("main")
            .and_then(|win| {
                let size = win.inner_size().ok()?;
                let sf = win.scale_factor().ok().unwrap_or(1.0);
                Some((size.width as f64 / sf, size.height as f64 / sf))
            })
            .unwrap_or_else(|| {
                let wm = window_manager.lock().unwrap();
                let s = wm.scale() as f64;
                (width as f64 * s, height as f64 * s)
            });

        // Use saved size for resizable windows, otherwise derive from main.
        // Playlist needs ~15% extra width so its graphics aren't clipped.
        let default_w = match window_id {
            WindowId::Playlist => main_w * 1.15,
            WindowId::RadioBrowser => main_w * 1.5,
            WindowId::Settings => 700.0,
            _ => main_w,
        };
        let w = if resizable { saved.width.unwrap_or(default_w) } else { main_w };
        let h = if resizable {
            saved.height.unwrap_or(main_h * 2.0)
        } else {
            main_h
        };

        eprintln!("[retroamp] creating window: label={label} size={w}x{h} (main={main_w}x{main_h})");

        // On Wayland, non-resizable toplevel windows get a compositor-enforced
        // minimum size. Work around this by always creating resizable windows
        // and clamping with min/max for ones that shouldn't resize.
        let mut builder = WebviewWindowBuilder::new(&app, &label, WebviewUrl::App(url.into()))
            .title(format!("RetroAmp — {}", label))
            .inner_size(w, h)
            .decorations(false)
            .resizable(true)
            .visible(true)
            .skip_taskbar(true); // Don't show separate taskbar entry.

        // Clamp non-resizable panels so they can't actually be resized.
        if !resizable {
            builder = builder.min_inner_size(w, h).max_inner_size(w, h);
        }

        // Apply saved position (works on X11/Windows/macOS, ignored on Wayland).
        if let (Some(x), Some(y)) = (saved.x, saved.y) {
            builder = builder.position(x as f64, y as f64);
        }

        // Set the main window as parent so closing main closes everything.
        if let Some(main_win) = app.get_webview_window("main") {
            builder = builder.parent(&main_win)
                .map_err(|e| format!("failed to set parent window: {e}"))?;
        }

        builder.build()
            .map_err(|e| {
                eprintln!("[retroamp] window creation failed: {e}");
                e.to_string()
            })?;
    }

    // Persist window layout after every toggle.
    let wm = window_manager.lock().map_err(|e| e.to_string())?;
    save_window_layout(&app, &wm);
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
    app: AppHandle,
    window_manager: State<'_, Mutex<WindowManager>>,
) -> Result<WindowStates, String> {
    let mut wm = window_manager.lock().map_err(|e| e.to_string())?;
    wm.cycle_scale();
    save_window_layout(&app, &wm);
    Ok(wm.get_states())
}

/// Set a specific scale value.
#[tauri::command]
pub async fn set_scale(
    app: AppHandle,
    window_manager: State<'_, Mutex<WindowManager>>,
    scale: u32,
) -> Result<WindowStates, String> {
    let mut wm = window_manager.lock().map_err(|e| e.to_string())?;
    wm.set_scale(scale);
    save_window_layout(&app, &wm);
    Ok(wm.get_states())
}

// -- Shade mode commands --

/// Enter shade mode: create a compact 275x14 shade window and hide the main window.
/// The main window is hidden (not closed) so child windows remain valid.
#[tauri::command]
pub async fn enter_shade(
    app: AppHandle,
    window_manager: State<'_, Mutex<WindowManager>>,
) -> Result<(), String> {
    let scale = {
        let wm = window_manager.lock().map_err(|e| e.to_string())?;
        wm.scale()
    };

    // Request the ideal shade size. Wayland compositors may enforce a minimum
    // height — that's OK, the canvas uses height:auto to maintain aspect ratio
    // and any extra space below is black.
    let w = 275.0 * scale as f64;
    let h = 14.0 * scale as f64;
    eprintln!("[retroamp] shade window requested: {w}x{h}");

    // Only create if it doesn't already exist.
    if app.get_webview_window("shade").is_none() {
        WebviewWindowBuilder::new(&app, "shade", WebviewUrl::App("/?window=shade".into()))
            .title("RetroAmp")
            .inner_size(w, h)
            .decorations(false)
            .resizable(false)
            .visible(true)
            .build()
            .map_err(|e| e.to_string())?;
    }

    // Hide main window.
    if let Some(main) = app.get_webview_window("main") {
        main.hide().map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Exit shade mode: show the main window and close the shade window.
#[tauri::command]
pub async fn exit_shade(app: AppHandle) -> Result<(), String> {
    if let Some(main) = app.get_webview_window("main") {
        main.show().map_err(|e| e.to_string())?;
        main.set_focus().map_err(|e| e.to_string())?;
    }
    if let Some(shade) = app.get_webview_window("shade") {
        shade.close().map_err(|e| e.to_string())?;
    }
    Ok(())
}

// -- Settings command --

/// Open the settings/preferences window (or focus it if already open).
#[tauri::command]
pub async fn open_settings(
    app: AppHandle,
    window_manager: State<'_, Mutex<WindowManager>>,
) -> Result<(), String> {
    // Mark visible in the WindowManager so toggle_window works correctly.
    if let Ok(mut wm) = window_manager.lock() {
        wm.set_visible(WindowId::Settings, true);
    }

    // If already open, just focus it.
    if let Some(existing) = app.get_webview_window("settings") {
        existing.show().map_err(|e| e.to_string())?;
        existing.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }

    // Load saved layout for position/size.
    let saved = {
        let cfg = crate::config::AppConfig::load();
        cfg.ui.settings.unwrap_or_default()
    };

    let w = saved.width.unwrap_or(700.0);
    let h = saved.height.unwrap_or(500.0);

    let mut builder = WebviewWindowBuilder::new(&app, "settings", WebviewUrl::App("/?window=settings".into()))
        .title("RetroAmp Preferences")
        .inner_size(w, h)
        .min_inner_size(500.0, 400.0)
        .decorations(false)
        .resizable(true)
        .visible(true)
        .skip_taskbar(true);

    // Apply saved position.
    if let (Some(x), Some(y)) = (saved.x, saved.y) {
        builder = builder.position(x as f64, y as f64);
    }

    // Set the main window as parent so closing main closes everything.
    if let Some(main_win) = app.get_webview_window("main") {
        builder = builder.parent(&main_win)
            .map_err(|e| format!("failed to set parent window: {e}"))?;
    }

    builder.build()
        .map_err(|e| e.to_string())?;

    Ok(())
}

// -- Skin commands --

/// Load a skin from a .wsz archive or extracted directory.
#[tauri::command]
pub fn load_skin(path: String) -> Result<SkinContents, String> {
    crate::skin::loader::load_skin(&path)
}

/// Set the active skin path (so all windows can pick it up).
/// Also persists the choice to config so it survives restarts.
#[tauri::command]
pub fn set_active_skin(
    window_manager: State<'_, Mutex<WindowManager>>,
    database: State<'_, Arc<Mutex<Database>>>,
    path: String,
) -> Result<(), String> {
    let mut wm = window_manager.lock().map_err(|e| e.to_string())?;
    wm.set_active_skin_path(path.clone());

    // Persist to config (best-effort — don't fail the command if this errors).
    let mut cfg = crate::config::AppConfig::load();
    cfg.skins.last_skin_path = Some(path.clone());
    let _ = cfg.save();

    // Record usage in the database (best-effort, non-blocking).
    // Use try_lock so we never block skin loading if the catalog sync is running.
    if let Ok(db) = database.try_lock() {
        let _ = db.record_skin_use(&path);
    }

    Ok(())
}

/// Return the last-used skin path from config (if any and still exists on disk).
#[tauri::command]
pub fn get_last_skin_path() -> Option<String> {
    let cfg = crate::config::AppConfig::load();
    cfg.skins.last_skin_path.filter(|p| std::path::Path::new(p).exists())
}

/// Return the platform-appropriate skins directory, creating it if needed.
///
/// - Linux:   `~/.config/retroamp/skins/`
/// - macOS:   `~/Library/Application Support/retroamp/skins/`
/// - Windows: `C:\Users\<user>\AppData\Roaming\retroamp\skins\`
#[tauri::command]
pub fn get_skins_dir() -> Result<String, String> {
    let dir = skins_dir().ok_or("could not determine config directory")?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("failed to create skins directory: {e}"))?;
    Ok(dir.to_string_lossy().into_owned())
}

/// List all available skins from the skins directories.
#[tauri::command]
pub fn get_skins() -> Vec<SkinInfo> {
    let mut dirs = Vec::new();

    // Platform skins directory (primary).
    if let Some(dir) = skins_dir() {
        // Ensure it exists so users always have a place to drop skins.
        let _ = std::fs::create_dir_all(&dir);
        dirs.push(dir);
    }

    // User-configured extra skin directories.
    for dir in crate::config::AppConfig::load().skins.extra_dirs {
        if dir.is_dir() {
            dirs.push(dir);
        }
    }

    // Project skins directory (development convenience).
    if cfg!(debug_assertions) {
        let project_skins = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(|p| p.join("skins"));
        if let Some(dir) = project_skins {
            if dir.is_dir() {
                dirs.push(dir);
            }
        }
    }

    crate::skin::scanner::scan_all(&dirs)
}

/// Add a user-chosen directory to the skin scan list.
#[tauri::command]
pub fn add_skin_dir(path: String) -> Result<Vec<String>, String> {
    let mut cfg = crate::config::AppConfig::load();
    cfg.add_skin_dir(path.into());
    cfg.save()?;
    Ok(cfg.skins.extra_dirs.iter().map(|p| p.to_string_lossy().into_owned()).collect())
}

/// Remove a directory from the skin scan list.
#[tauri::command]
pub fn remove_skin_dir(path: String) -> Result<Vec<String>, String> {
    let mut cfg = crate::config::AppConfig::load();
    cfg.remove_skin_dir(&path.into());
    cfg.save()?;
    Ok(cfg.skins.extra_dirs.iter().map(|p| p.to_string_lossy().into_owned()).collect())
}

/// Get the list of extra skin directories.
#[tauri::command]
pub fn get_extra_skin_dirs() -> Vec<String> {
    crate::config::AppConfig::load()
        .skins
        .extra_dirs
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}

/// Delete a skin file from disk and remove it from the catalog.
#[tauri::command]
pub fn delete_skin(
    database: State<'_, Arc<Mutex<Database>>>,
    path: String,
) -> Result<(), String> {
    let p = std::path::Path::new(&path);

    if p.is_file() {
        std::fs::remove_file(p)
            .map_err(|e| format!("failed to delete skin file: {e}"))?;
    } else if p.is_dir() {
        std::fs::remove_dir_all(p)
            .map_err(|e| format!("failed to delete skin directory: {e}"))?;
    } else {
        return Err("skin path does not exist".to_string());
    }

    // Remove from the database catalog.
    if let Ok(db) = database.lock() {
        let _ = db.remove_by_path(&path);
    }

    log::info!("deleted skin: {path}");
    Ok(())
}

/// Reveal a skin's location in the system file manager.
#[tauri::command]
pub async fn reveal_skin_folder(_app: AppHandle, path: String) -> Result<(), String> {
    let p = std::path::Path::new(&path);
    let folder = if p.is_file() {
        p.parent().map(|p| p.to_string_lossy().into_owned())
    } else {
        Some(path.clone())
    };

    if let Some(folder) = folder {
        // Use xdg-open on Linux, open on macOS, explorer on Windows.
        #[cfg(target_os = "linux")]
        {
            let _ = std::process::Command::new("xdg-open").arg(&folder).spawn();
        }
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("open").arg(&folder).spawn();
        }
        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("explorer").arg(&folder).spawn();
        }
    }

    Ok(())
}

// -- Skin catalog commands (database-backed) --

/// Get the skin catalog — metadata only, no thumbnail blobs.
/// Thumbnails are loaded lazily via `get_skin_thumbnails`.
#[tauri::command]
pub fn get_skin_catalog(
    database: State<'_, Arc<Mutex<Database>>>,
) -> Result<Vec<SkinCatalogEntry>, String> {
    let db = database.lock().map_err(|e| e.to_string())?;
    db.get_all_skins()
}

/// Get thumbnails for a batch of skins by path.
/// Returns a list of { path, thumbnail } pairs.
#[tauri::command]
pub fn get_skin_thumbnails(
    database: State<'_, Arc<Mutex<Database>>>,
    paths: Vec<String>,
) -> Result<Vec<(String, String)>, String> {
    let db = database.lock().map_err(|e| e.to_string())?;
    db.get_thumbnails_batch(&paths)
}

/// Toggle a skin's favorite status. Returns the new value.
#[tauri::command]
pub fn toggle_skin_favorite(
    database: State<'_, Arc<Mutex<Database>>>,
    path: String,
) -> Result<bool, String> {
    let db = database.lock().map_err(|e| e.to_string())?;
    db.toggle_favorite(&path)
}

/// Get the N most recently used skins — metadata only.
#[tauri::command]
pub fn get_recent_skins(
    database: State<'_, Arc<Mutex<Database>>>,
    limit: Option<usize>,
) -> Result<Vec<SkinCatalogEntry>, String> {
    let db = database.lock().map_err(|e| e.to_string())?;
    db.get_recently_used(limit.unwrap_or(10))
}

/// Re-scan the filesystem and sync the catalog (metadata only — thumbnails
/// are generated in the background).
#[tauri::command]
pub fn refresh_skin_catalog(
    database: State<'_, Arc<Mutex<Database>>>,
) -> Result<Vec<SkinCatalogEntry>, String> {
    let db = database.lock().map_err(|e| e.to_string())?;

    // Gather scan directories.
    let mut dirs = Vec::new();
    if let Some(dir) = skins_dir() {
        let _ = std::fs::create_dir_all(&dir);
        dirs.push(dir);
    }
    for dir in crate::config::AppConfig::load().skins.extra_dirs {
        if dir.is_dir() {
            dirs.push(dir);
        }
    }
    if cfg!(debug_assertions) {
        let project_skins = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(|p| p.join("skins"));
        if let Some(dir) = project_skins {
            if dir.is_dir() {
                dirs.push(dir);
            }
        }
    }

    let skins = crate::skin::scanner::scan_all(&dirs);
    let valid_paths: Vec<String> = skins.iter().map(|s| s.path.clone()).collect();

    // Only upsert metadata — no thumbnail extraction here.
    for skin in &skins {
        if let Err(e) = db.upsert_skin(skin, None) {
            log::warn!("failed to upsert skin {}: {e}", skin.name);
        }
    }

    let _ = db.remove_missing(&valid_paths);
    db.get_all_skins()
}

fn skins_dir() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|c| c.join("retroamp").join("skins"))
}

// -- Internal helpers --

fn play_path(engine: &AudioEngine, path: &str) -> Result<(), String> {
    let source = create_source(path)?;
    engine.play(source);
    Ok(())
}

/// Create an AudioSource from a path — dispatches to RadioSource for URLs,
/// LocalFileSource for local files.
pub fn create_source(path: &str) -> Result<Box<dyn AudioSource>, String> {
    if is_url(path) {
        RadioSource::connect(path)
            .map(|s| Box::new(s) as Box<dyn AudioSource>)
            .map_err(|e| e.to_string())
    } else {
        LocalFileSource::open(path)
            .map(|s| Box::new(s) as Box<dyn AudioSource>)
            .map_err(|e| e.to_string())
    }
}

/// Check if a string looks like an HTTP URL.
pub fn is_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://")
}

/// Play a radio stream URL.
#[tauri::command]
pub fn play_url(
    engine: State<'_, Arc<AudioEngine>>,
    playlist: State<'_, Arc<Mutex<PlaylistManager>>>,
    url: String,
    name: Option<String>,
) -> Result<(), String> {
    let mut pl = playlist.lock().map_err(|e| e.to_string())?;
    let id = pl.add_track(&url);

    if let Some(name) = &name {
        pl.update_display_name(id, name);
    }

    pl.play_track(id);
    drop(pl);

    let source = RadioSource::connect_with_name(&url, name.as_deref())
        .map_err(|e| e.to_string())?;

    // Update playlist metadata with stream info.
    if let Ok(meta) = source.metadata() {
        if let Ok(mut pl) = playlist.lock() {
            pl.update_metadata(id, &meta);
        }
    }

    engine.play(Box::new(source));
    Ok(())
}

/// Add a radio stream URL to the playlist without playing it.
#[tauri::command]
pub fn playlist_add_url(
    playlist: State<'_, Arc<Mutex<PlaylistManager>>>,
    url: String,
    name: Option<String>,
) -> Result<PlaylistState, String> {
    let mut pl = playlist.lock().map_err(|e| e.to_string())?;
    let id = pl.add_track(&url);
    if let Some(name) = name {
        pl.update_display_name(id, &name);
    }
    Ok(pl.state())
}

// -- Radio browser commands --

#[tauri::command]
pub fn get_radio_stations(
    database: State<'_, Arc<Mutex<Database>>>,
    include_hidden: Option<bool>,
) -> Result<Vec<crate::db::RadioStation>, String> {
    let db = database.lock().map_err(|e| e.to_string())?;
    db.get_all_stations(include_hidden.unwrap_or(false))
}

#[tauri::command]
pub fn get_favorite_stations(
    database: State<'_, Arc<Mutex<Database>>>,
) -> Result<Vec<crate::db::RadioStation>, String> {
    let db = database.lock().map_err(|e| e.to_string())?;
    db.get_favorite_stations()
}

#[tauri::command]
pub fn search_radio_stations_local(
    database: State<'_, Arc<Mutex<Database>>>,
    query: String,
) -> Result<Vec<crate::db::RadioStation>, String> {
    let db = database.lock().map_err(|e| e.to_string())?;
    db.search_stations(&query)
}

#[tauri::command]
pub fn toggle_station_favorite(
    database: State<'_, Arc<Mutex<Database>>>,
    url: String,
) -> Result<bool, String> {
    let db = database.lock().map_err(|e| e.to_string())?;
    db.toggle_station_favorite(&url)
}

#[tauri::command]
pub fn hide_radio_station(
    database: State<'_, Arc<Mutex<Database>>>,
    url: String,
) -> Result<(), String> {
    let db = database.lock().map_err(|e| e.to_string())?;
    db.hide_station(&url)
}

#[tauri::command]
pub fn unhide_radio_station(
    database: State<'_, Arc<Mutex<Database>>>,
    url: String,
) -> Result<(), String> {
    let db = database.lock().map_err(|e| e.to_string())?;
    db.unhide_station(&url)
}

#[tauri::command]
pub fn delete_radio_station(
    database: State<'_, Arc<Mutex<Database>>>,
    url: String,
) -> Result<(), String> {
    let db = database.lock().map_err(|e| e.to_string())?;
    db.delete_station(&url)
}

#[tauri::command]
pub fn save_radio_station(
    database: State<'_, Arc<Mutex<Database>>>,
    name: String,
    url: String,
    genre: Option<String>,
    bitrate: Option<u32>,
    codec: Option<String>,
    country: Option<String>,
) -> Result<(), String> {
    let db = database.lock().map_err(|e| e.to_string())?;
    db.save_station(
        &name,
        &url,
        genre.as_deref(),
        bitrate,
        codec.as_deref(),
        country.as_deref(),
    )
}

#[tauri::command]
pub async fn radio_browser_search(
    query: String,
    limit: Option<usize>,
) -> Result<Vec<crate::radio_browser::ApiStation>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::radio_browser::search(&query, limit.unwrap_or(50))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn radio_browser_top(
    limit: Option<usize>,
) -> Result<Vec<crate::radio_browser::ApiStation>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::radio_browser::top_stations(limit.unwrap_or(100))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn radio_browser_by_tag(
    tag: String,
    limit: Option<usize>,
) -> Result<Vec<crate::radio_browser::ApiStation>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::radio_browser::by_tag(&tag, limit.unwrap_or(50))
    })
    .await
    .map_err(|e| e.to_string())?
}
