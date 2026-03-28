//! Tauri command handlers — the bridge between the frontend and Rust backend.
//!
//! Each #[tauri::command] function is callable from the WebView via
//! `invoke("command_name", { args })`. Commands access the audio engine
//! and window manager through Tauri's managed state.

use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};

use crate::audio::engine::{AudioEngine, EngineStatus};
use crate::audio::eq::EqSettings;
use crate::audio::fft::FftData;
use crate::audio::local::LocalFileSource;
use crate::audio::radio::RadioSource;
use crate::audio::source::AudioSource;
use crate::db::{Database, EqPresetEntry, SkinCatalogEntry};
use crate::library;
use crate::playlist::manager::{PlaylistManager, PlaylistState};
use crate::skin::loader::SkinContents;
use crate::skin::scanner::SkinInfo;
use crate::window::manager::{WindowId, WindowManager, WindowStates};

// -- Skin cache --

/// Caches the most recently loaded skin to avoid redundant ZIP extractions
/// when multiple windows load the same skin.
pub struct SkinCache {
    cached: Option<(String, SkinContents)>,
}

impl SkinCache {
    pub fn new() -> Self {
        Self { cached: None }
    }

    pub fn get(&self, path: &str) -> Option<&SkinContents> {
        self.cached.as_ref().filter(|(p, _)| p == path).map(|(_, c)| c)
    }

    pub fn put(&mut self, path: String, contents: SkinContents) {
        self.cached = Some((path, contents));
    }
}

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

// -- Playlist persistence --

/// Save the current playlist state to the database. Called on app exit.
pub fn save_playlist_state(
    database: &Arc<Mutex<Database>>,
    playlist: &Arc<Mutex<PlaylistManager>>,
) {
    let Ok(pl) = playlist.lock() else { return };
    let paths = pl.track_paths();
    let current_index = pl.current_index();
    let shuffle = format!("{:?}", pl.shuffle_mode());
    let repeat = format!("{:?}", pl.repeat_mode());
    drop(pl);

    if let Ok(db) = database.lock() {
        if let Err(e) = db.save_playlist(&paths, current_index, &shuffle, &repeat) {
            log::warn!("failed to save playlist state: {e}");
        }
    }
}

// -- Window layout persistence --

/// Capture the current window layout (visibility, positions, sizes) and save
/// to config. Called on toggle, scale change, and app exit. Position reads
/// are best-effort — on Wayland `outer_position()` may return (0,0).
///
/// Accepts a `WindowStates` snapshot so the caller can drop the lock before
/// calling this (avoiding lock contention with the 50ms poller).
pub fn save_window_layout(app: &AppHandle, states: &WindowStates) {
    use crate::config::WindowLayoutEntry;

    let mut cfg = crate::config::AppConfig::load();
    cfg.ui.scale = Some(states.scale);

    let is_visible = |label: &str| -> bool {
        states.windows.get(label).map_or(false, |w| w.visible)
    };

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
    cfg.ui.equalizer = capture("equalizer", is_visible("equalizer"), false);
    cfg.ui.playlist = capture("playlist", is_visible("playlist"), true);
    if is_visible("radiobrowser") || app.get_webview_window("radiobrowser").is_some() {
        cfg.ui.radio_browser = Some(capture("radiobrowser", is_visible("radiobrowser"), true));
    }
    if is_visible("settings") || app.get_webview_window("settings").is_some() {
        cfg.ui.settings = Some(capture("settings", is_visible("settings"), true));
    }
    if is_visible("librarybrowser") || app.get_webview_window("librarybrowser").is_some() {
        cfg.ui.library_browser = Some(capture("librarybrowser", is_visible("librarybrowser"), true));
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
    let label = window_id.label().to_string();

    // All windows are pre-created at startup (hidden).  We only show/hide
    // them here — never create or destroy.  On Wayland, creating a WebView
    // while existing WebViews are active corrupts GTK's pointer-event state
    // and permanently breaks dragging/close/corner-resize.
    let win = app
        .get_webview_window(&label)
        .ok_or_else(|| format!("window '{label}' not found — was it pre-created at startup?"))?;

    let currently_visible = win.is_visible().unwrap_or(false);
    let should_show = !currently_visible;

    eprintln!("[retroamp] toggle_window: id={window_id:?} label={label} should_show={should_show}");

    {
        let mut wm = window_manager.lock().map_err(|e| e.to_string())?;
        wm.set_visible(window_id, should_show);
    }

    if should_show {
        win.show().map_err(|e| e.to_string())?;
    } else {
        win.hide().map_err(|e| e.to_string())?;
    }

    let states = {
        let wm = window_manager.lock().map_err(|e| e.to_string())?;
        wm.get_states()
    };
    save_window_layout(&app, &states);
    Ok(states)
}

/// Get the current state of all windows.
/// Cross-checks internal state against actual window existence so that
/// indicators (PL/EQ buttons) stay accurate even after compositor-initiated
/// window destruction or failed creation.
#[tauri::command]
pub fn get_window_states(
    app: AppHandle,
    window_manager: State<'_, Mutex<WindowManager>>,
) -> Result<WindowStates, String> {
    let mut wm = window_manager.lock().map_err(|e| e.to_string())?;
    wm.reconcile(|id| app.get_webview_window(id.label()).is_some());
    Ok(wm.get_states())
}

/// Cycle the global UI scale (1x → 2x → 3x → 1x).
#[tauri::command]
pub async fn cycle_scale(
    app: AppHandle,
    window_manager: State<'_, Mutex<WindowManager>>,
) -> Result<WindowStates, String> {
    let states = {
        let mut wm = window_manager.lock().map_err(|e| e.to_string())?;
        wm.cycle_scale();
        wm.get_states()
    };
    save_window_layout(&app, &states);
    Ok(states)
}

/// Set a specific scale value.
#[tauri::command]
pub async fn set_scale(
    app: AppHandle,
    window_manager: State<'_, Mutex<WindowManager>>,
    scale: u32,
) -> Result<WindowStates, String> {
    let states = {
        let mut wm = window_manager.lock().map_err(|e| e.to_string())?;
        wm.set_scale(scale);
        wm.get_states()
    };
    save_window_layout(&app, &states);
    Ok(states)
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

/// Open the settings/preferences window.  The window is pre-created at
/// startup — this just shows it.
#[tauri::command]
pub fn open_settings(
    app: AppHandle,
    window_manager: State<'_, Mutex<WindowManager>>,
) -> Result<(), String> {
    if let Ok(mut wm) = window_manager.lock() {
        wm.set_visible(WindowId::Settings, true);
    }
    if let Some(win) = app.get_webview_window("settings") {
        win.show().map_err(|e| e.to_string())?;
    }
    Ok(())
}

// -- Skin commands --

/// Load a skin from a .wsz archive or extracted directory.
/// Results are cached so that opening additional windows doesn't re-extract
/// the same ZIP file.
#[tauri::command]
pub fn load_skin(
    skin_cache: State<'_, Mutex<SkinCache>>,
    path: String,
) -> Result<SkinContents, String> {
    let mut cache = skin_cache.lock().map_err(|e| e.to_string())?;
    if let Some(cached) = cache.get(&path) {
        return Ok(cached.clone());
    }
    let contents = crate::skin::loader::load_skin(&path)?;
    cache.put(path, contents.clone());
    Ok(contents)
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

/// List all available skins from the skins directory.
#[tauri::command]
pub fn get_skins() -> Vec<SkinInfo> {
    let Some(dir) = skins_dir() else {
        return Vec::new();
    };
    let _ = std::fs::create_dir_all(&dir);
    crate::skin::scanner::scan_all(&[dir])
}

/// Import skin files (`.wsz` / `.zip`) into the skins directory.
/// Copies each file, adds it to the catalog, and returns the imported paths.
#[tauri::command]
pub fn import_skins(
    database: State<'_, Arc<Mutex<Database>>>,
    paths: Vec<String>,
) -> Result<Vec<String>, String> {
    let dir = skins_dir().ok_or("could not determine skins directory")?;
    let _ = std::fs::create_dir_all(&dir);

    let mut imported = Vec::new();
    for src in &paths {
        let src_path = std::path::Path::new(src);
        if !src_path.is_file() {
            log::warn!("import_skins: skipping non-file {src}");
            continue;
        }

        let Some(filename) = src_path.file_name() else { continue };
        let dest = dir.join(filename);

        // Don't overwrite if it already exists in the skins folder.
        if dest.exists() {
            log::info!("import_skins: {src} already exists, skipping copy");
            imported.push(dest.to_string_lossy().into_owned());
            continue;
        }

        if let Err(e) = std::fs::copy(src_path, &dest) {
            log::warn!("import_skins: failed to copy {src}: {e}");
            continue;
        }

        let dest_str = dest.to_string_lossy().into_owned();
        log::info!("imported skin: {dest_str}");

        // Add to the catalog immediately.
        let name = dest.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let skin_info = crate::skin::scanner::SkinInfo {
            name,
            path: dest_str.clone(),
            is_archive: true,
        };
        if let Ok(db) = database.lock() {
            let _ = db.upsert_skin(&skin_info, None);
        }

        imported.push(dest_str);
    }

    Ok(imported)
}

/// Open the skins directory in the system file manager.
#[tauri::command]
pub async fn open_skins_folder() -> Result<(), String> {
    let dir = skins_dir().ok_or("could not determine skins directory")?;
    let _ = std::fs::create_dir_all(&dir);
    let folder = dir.to_string_lossy().into_owned();

    #[cfg(target_os = "linux")]
    { let _ = std::process::Command::new("xdg-open").arg(&folder).spawn(); }
    #[cfg(target_os = "macos")]
    { let _ = std::process::Command::new("open").arg(&folder).spawn(); }
    #[cfg(target_os = "windows")]
    { let _ = std::process::Command::new("explorer").arg(&folder).spawn(); }

    Ok(())
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

    // Scan the skins directory.
    let Some(dir) = skins_dir() else {
        return db.get_all_skins();
    };
    let _ = std::fs::create_dir_all(&dir);
    let skins = crate::skin::scanner::scan_all(&[dir]);
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

pub fn play_path(engine: &AudioEngine, path: &str) -> Result<(), String> {
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

// -- Library --

/// Trigger a library scan. Returns immediately; the scan runs in the background.
#[tauri::command]
pub fn scan_library(
    database: State<'_, Arc<Mutex<Database>>>,
    app: AppHandle,
) -> Result<(), String> {
    if library::is_scanning() {
        return Err("scan already in progress".to_string());
    }
    let db = Arc::clone(&*database);
    std::thread::Builder::new()
        .name("retroamp-library-scan".into())
        .spawn(move || {
            library::scan_library(db, app);
        })
        .map_err(|e| format!("{e}"))?;
    Ok(())
}

#[tauri::command]
pub fn get_scan_status() -> bool {
    library::is_scanning()
}

#[tauri::command]
pub fn get_library_dirs(
    database: State<'_, Arc<Mutex<Database>>>,
) -> Result<Vec<String>, String> {
    let db = database.lock().map_err(|e| e.to_string())?;
    Ok(library::db::get_library_dirs(db.conn())
        .into_iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect())
}

#[tauri::command]
pub fn add_library_dir(
    database: State<'_, Arc<Mutex<Database>>>,
    path: String,
) -> Result<(), String> {
    let db = database.lock().map_err(|e| e.to_string())?;
    library::db::add_library_dir(db.conn(), &path)
}

#[tauri::command]
pub fn remove_library_dir(
    database: State<'_, Arc<Mutex<Database>>>,
    path: String,
) -> Result<(), String> {
    let db = database.lock().map_err(|e| e.to_string())?;
    library::db::remove_library_dir(db.conn(), &path)
}

#[tauri::command]
pub fn get_library_tracks(
    database: State<'_, Arc<Mutex<Database>>>,
    search: Option<String>,
    sort_by: Option<String>,
    sort_dir: Option<String>,
    offset: Option<i64>,
    limit: Option<i64>,
) -> Result<Vec<library::db::LibraryTrack>, String> {
    let db = database.lock().map_err(|e| e.to_string())?;
    library::db::get_tracks(
        db.conn(),
        search.as_deref(),
        sort_by.as_deref().unwrap_or("title"),
        sort_dir.as_deref().unwrap_or("asc"),
        offset.unwrap_or(0),
        limit.unwrap_or(-1),
    )
}

#[tauri::command]
pub fn search_library(
    database: State<'_, Arc<Mutex<Database>>>,
    query: String,
) -> Result<Vec<library::db::LibraryTrack>, String> {
    let db = database.lock().map_err(|e| e.to_string())?;
    library::db::get_tracks(db.conn(), Some(&query), "title", "asc", 0, 200)
}

#[tauri::command]
pub fn get_library_artists(
    database: State<'_, Arc<Mutex<Database>>>,
) -> Result<Vec<String>, String> {
    let db = database.lock().map_err(|e| e.to_string())?;
    library::db::get_artists(db.conn())
}

#[tauri::command]
pub fn get_library_albums(
    database: State<'_, Arc<Mutex<Database>>>,
) -> Result<Vec<library::db::AlbumEntry>, String> {
    let db = database.lock().map_err(|e| e.to_string())?;
    library::db::get_albums(db.conn())
}

#[tauri::command]
pub fn get_library_genres(
    database: State<'_, Arc<Mutex<Database>>>,
) -> Result<Vec<String>, String> {
    let db = database.lock().map_err(|e| e.to_string())?;
    library::db::get_genres(db.conn())
}

#[tauri::command]
pub fn get_library_cover(
    database: State<'_, Arc<Mutex<Database>>>,
    hash: String,
) -> Result<Option<String>, String> {
    let db = database.lock().map_err(|e| e.to_string())?;
    match library::db::get_cover(db.conn(), &hash)? {
        Some((data, mime)) => {
            let b64 = base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                &data,
            );
            Ok(Some(format!("data:{mime};base64,{b64}")))
        }
        None => Ok(None),
    }
}

#[tauri::command]
pub fn get_library_track_count(
    database: State<'_, Arc<Mutex<Database>>>,
) -> Result<i64, String> {
    let db = database.lock().map_err(|e| e.to_string())?;
    Ok(library::db::get_track_count(db.conn()))
}

#[tauri::command]
pub fn get_tracks_by_artist(
    database: State<'_, Arc<Mutex<Database>>>,
    artist: String,
) -> Result<Vec<library::db::LibraryTrack>, String> {
    let db = database.lock().map_err(|e| e.to_string())?;
    library::db::get_tracks_by_artist(db.conn(), &artist)
}

#[tauri::command]
pub fn get_tracks_by_album(
    database: State<'_, Arc<Mutex<Database>>>,
    album: String,
) -> Result<Vec<library::db::LibraryTrack>, String> {
    let db = database.lock().map_err(|e| e.to_string())?;
    library::db::get_tracks_by_album(db.conn(), &album)
}

#[tauri::command]
pub fn get_tracks_by_genre(
    database: State<'_, Arc<Mutex<Database>>>,
    genre: String,
) -> Result<Vec<library::db::LibraryTrack>, String> {
    let db = database.lock().map_err(|e| e.to_string())?;
    library::db::get_tracks_by_genre(db.conn(), &genre)
}

/// Set a track's star rating. Always updates the DB (authoritative store).
/// File tag write is best-effort — some formats may not support it.
#[tauri::command]
pub fn set_track_rating(
    database: State<'_, Arc<Mutex<Database>>>,
    app: AppHandle,
    path: String,
    rating: u8,
) -> Result<(), String> {
    if rating > 5 {
        return Err("rating must be 0-5".to_string());
    }
    // Best-effort write to file tags.
    let _ = library::tags::write_rating(&path, rating);
    // Always update the DB (authoritative for ratings).
    let db = database.lock().map_err(|e| e.to_string())?;
    library::db::update_track_rating(db.conn(), &path, rating)?;
    // Notify other windows.
    let _ = app.emit("tags-updated", &path);
    Ok(())
}

/// Open the parent folder of a file in the system file manager.
#[tauri::command]
pub async fn reveal_in_file_manager(path: String) -> Result<(), String> {
    let p = std::path::Path::new(&path);
    let folder = if p.is_file() {
        p.parent().map(|pp| pp.to_string_lossy().into_owned())
    } else {
        Some(path.clone())
    };
    if let Some(folder) = folder {
        #[cfg(target_os = "linux")]
        { let _ = std::process::Command::new("xdg-open").arg(&folder).spawn(); }
        #[cfg(target_os = "macos")]
        { let _ = std::process::Command::new("open").arg(&folder).spawn(); }
        #[cfg(target_os = "windows")]
        { let _ = std::process::Command::new("explorer").arg(&folder).spawn(); }
    }
    Ok(())
}

/// Get the playlist add mode preference ("append", "replace", or "ask").
#[tauri::command]
pub fn get_playlist_add_mode() -> String {
    crate::config::AppConfig::load().playback.playlist_add_mode
}

/// Set the playlist add mode preference.
#[tauri::command]
pub fn set_playlist_add_mode(mode: String) -> Result<(), String> {
    if !["append", "replace", "ask"].contains(&mode.as_str()) {
        return Err("mode must be 'append', 'replace', or 'ask'".to_string());
    }
    let mut cfg = crate::config::AppConfig::load();
    cfg.playback.playlist_add_mode = mode;
    cfg.save()
}

/// Get visible library columns from config.
#[tauri::command]
pub fn get_library_columns() -> Vec<String> {
    let cols = crate::config::AppConfig::load().library.visible_columns;
    if cols.is_empty() {
        // Sensible defaults
        vec!["title", "artist", "album", "duration"]
            .into_iter().map(String::from).collect()
    } else {
        cols
    }
}

/// Save visible library columns to config.
#[tauri::command]
pub fn set_library_columns(columns: Vec<String>) -> Result<(), String> {
    let mut cfg = crate::config::AppConfig::load();
    cfg.library.visible_columns = columns;
    cfg.save()
}

// -- Browser view state persistence --

/// Get saved library browser view state.
#[tauri::command]
pub fn get_library_view_state() -> serde_json::Value {
    let cfg = crate::config::AppConfig::load();
    serde_json::json!({
        "active_tab": cfg.library.active_tab,
        "sort_by": cfg.library.sort_by,
        "sort_dir": cfg.library.sort_dir,
        "browse_sort_by": cfg.library.browse_sort_by,
    })
}

/// Save library browser view state.
#[tauri::command]
pub fn set_library_view_state(
    active_tab: Option<String>,
    sort_by: Option<String>,
    sort_dir: Option<String>,
    browse_sort_by: Option<String>,
) -> Result<(), String> {
    let mut cfg = crate::config::AppConfig::load();
    cfg.library.active_tab = active_tab;
    cfg.library.sort_by = sort_by;
    cfg.library.sort_dir = sort_dir;
    cfg.library.browse_sort_by = browse_sort_by;
    cfg.save()
}

/// Get saved radio browser view state.
#[tauri::command]
pub fn get_radio_view_state() -> serde_json::Value {
    let cfg = crate::config::AppConfig::load();
    serde_json::json!({
        "active_tab": cfg.radio.active_tab,
        "show_hidden": cfg.radio.show_hidden,
    })
}

/// Save radio browser view state.
#[tauri::command]
pub fn set_radio_view_state(
    active_tab: Option<String>,
    show_hidden: bool,
) -> Result<(), String> {
    let mut cfg = crate::config::AppConfig::load();
    cfg.radio.active_tab = active_tab;
    cfg.radio.show_hidden = show_hidden;
    cfg.save()
}

// -- Column width persistence --

/// Get saved column widths for the library browser.
#[tauri::command]
pub fn get_library_column_widths() -> std::collections::HashMap<String, f64> {
    crate::config::AppConfig::load().library.column_widths
}

/// Save column widths for the library browser.
#[tauri::command]
pub fn set_library_column_widths(widths: std::collections::HashMap<String, f64>) -> Result<(), String> {
    let mut cfg = crate::config::AppConfig::load();
    cfg.library.column_widths = widths;
    cfg.save()
}

/// Get saved column widths for the radio browser.
#[tauri::command]
pub fn get_radio_column_widths() -> std::collections::HashMap<String, f64> {
    crate::config::AppConfig::load().radio.column_widths
}

/// Save column widths for the radio browser.
#[tauri::command]
pub fn set_radio_column_widths(widths: std::collections::HashMap<String, f64>) -> Result<(), String> {
    let mut cfg = crate::config::AppConfig::load();
    cfg.radio.column_widths = widths;
    cfg.save()
}

// -- Tag editor commands --

/// Read all tag information from a file for the tag editor.
/// The file is the source of truth for all tags. If the file's embedded
/// rating is 0, we check the DB as a fallback — this covers formats where
/// the rating couldn't be written to the file, or where a previous write
/// was best-effort.
#[tauri::command]
pub fn read_track_tags(
    database: State<'_, Arc<Mutex<Database>>>,
    path: String,
) -> Result<library::tags::TrackTagInfo, String> {
    let mut info = library::tags::read_track_tags(std::path::Path::new(&path))?;

    if info.rating == 0 {
        if let Ok(db) = database.lock() {
            let db_rating = library::db::get_track_rating(db.conn(), &path);
            if db_rating > 0 {
                info.rating = db_rating as u8;
            }
        }
    }

    Ok(info)
}

/// Write tag edits to a file, then update the library DB cache.
/// File tag writes may partially fail for formats with limited tag support;
/// the DB is always updated as the authoritative store.
#[tauri::command]
pub fn write_track_tags(
    database: State<'_, Arc<Mutex<Database>>>,
    app: AppHandle,
    path: String,
    edits: library::tags::TagEdits,
) -> Result<(), String> {
    // Write to file (best-effort for rating, hard fail for text tags).
    let _file_write_err = library::tags::write_tags(&path, &edits).err();

    // Always update the DB cache, regardless of file write success.
    // Re-read the file to get canonical values if the write succeeded.
    let db = database.lock().map_err(|e| e.to_string())?;
    match library::tags::read_track_tags(std::path::Path::new(&path)) {
        Ok(info) => {
            let _ = library::db::update_track_metadata(
                db.conn(),
                &path,
                info.title.as_deref(),
                info.artist.as_deref(),
                info.album_artist.as_deref(),
                info.album.as_deref(),
                info.genre.as_deref(),
                info.year,
                info.track_number,
                info.disc_number,
            );
        }
        Err(_) => {
            // File re-read failed — update DB from the edits directly.
            let _ = library::db::update_track_metadata(
                db.conn(),
                &path,
                edits.title.as_deref(),
                edits.artist.as_deref(),
                edits.album_artist.as_deref(),
                edits.album.as_deref(),
                edits.genre.as_deref(),
                edits.year.as_deref().and_then(|v| v.parse::<i32>().ok()),
                edits.track_number.as_deref().and_then(|v| v.parse::<i32>().ok()),
                edits.disc_number.as_deref().and_then(|v| v.parse::<i32>().ok()),
            );
        }
    }
    // Rating always goes to DB (authoritative).
    if let Some(stars) = edits.rating {
        let _ = library::db::update_track_rating(db.conn(), &path, stars);
    }

    // Notify other windows that tags changed.
    let _ = app.emit("tags-updated", &path);

    Ok(())
}

/// Open the tag editor window for a specific track.
/// The window is pre-created at startup — this emits a "load-tags" event
/// with the file path and shows the window.
#[tauri::command]
pub fn open_tag_editor(app: AppHandle, path: String) -> Result<(), String> {
    // Emit event so the already-mounted TagEditorWindow loads the new file.
    app.emit("load-tags", path).map_err(|e| e.to_string())?;

    if let Some(win) = app.get_webview_window("tageditor") {
        win.show().map_err(|e| e.to_string())?;
    }
    Ok(())
}
