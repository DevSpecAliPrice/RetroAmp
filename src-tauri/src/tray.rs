//! System tray integration.
//!
//! Provides a persistent tray icon with a context menu for quick transport
//! controls, current track info, and show/quit actions.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use tauri::menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

use crate::audio::engine::{AudioEngine, PlaybackState};
use crate::commands;
use crate::playlist::manager::PlaylistManager;

/// Build and register the system tray icon. Call this from `.setup()`.
pub fn setup(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.clone();

    // Menu items — we'll update the track info and play/pause label dynamically.
    let track_info = MenuItemBuilder::with_id("track_info", "RetroAmp")
        .enabled(false)
        .build(app)?;

    let play_pause = MenuItemBuilder::with_id("play_pause", "Play").build(app)?;
    let stop = MenuItemBuilder::with_id("stop", "Stop").build(app)?;
    let previous = MenuItemBuilder::with_id("previous", "Previous").build(app)?;
    let next = MenuItemBuilder::with_id("next", "Next").build(app)?;
    let show = MenuItemBuilder::with_id("show", "Show RetroAmp").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;

    let menu = MenuBuilder::new(app)
        .item(&track_info)
        .item(&PredefinedMenuItem::separator(app)?)
        .item(&previous)
        .item(&play_pause)
        .item(&stop)
        .item(&next)
        .item(&PredefinedMenuItem::separator(app)?)
        .item(&show)
        .item(&quit)
        .build()?;

    // 32x32 RGBA icon embedded at compile time.
    let icon_rgba = include_bytes!("../icons/tray-icon.rgba");
    let icon = tauri::image::Image::new_owned(icon_rgba.to_vec(), 32, 32);

    let _tray = TrayIconBuilder::new()
        .icon(icon)
        .tooltip("RetroAmp")
        .menu(&menu)
        .on_menu_event(move |app, event| {
            let engine: &Arc<AudioEngine> = &*app.state::<Arc<AudioEngine>>();
            let playlist: &Arc<Mutex<PlaylistManager>> =
                &*app.state::<Arc<Mutex<PlaylistManager>>>();

            match event.id().as_ref() {
                "play_pause" => {
                    let status = engine.status();
                    match status.state {
                        PlaybackState::Playing => engine.pause(),
                        _ => engine.resume(),
                    }
                }
                "stop" => {
                    engine.stop();
                }
                "previous" => {
                    if let Ok(mut pl) = playlist.lock() {
                        if let Some(track) = pl.previous_track() {
                            let path = track.path.clone();
                            drop(pl);
                            let _ = commands::play_path(engine, &path);
                        }
                    }
                }
                "next" => {
                    if let Ok(mut pl) = playlist.lock() {
                        if let Some(track) = pl.next_track() {
                            let path = track.path.clone();
                            drop(pl);
                            let _ = commands::play_path(engine, &path);
                        }
                    }
                }
                "show" => {
                    if let Some(win) = app.get_webview_window("main") {
                        let _ = win.show();
                        let _ = win.set_focus();
                    }
                }
                "quit" => {
                    std::process::exit(0);
                }
                _ => {}
            }
        })
        .build(app)?;

    // Spawn a thread to keep the menu updated with current track info and
    // play/pause state.
    let track_info_item = track_info;
    let play_pause_item = play_pause;

    thread::Builder::new()
        .name("retroamp-tray-updater".into())
        .spawn(move || {
            tray_update_loop(&handle, &track_info_item, &play_pause_item);
        })?;

    Ok(())
}

/// Periodically update the tray menu items to reflect current playback state.
fn tray_update_loop(
    app: &AppHandle,
    track_info: &tauri::menu::MenuItem<tauri::Wry>,
    play_pause: &tauri::menu::MenuItem<tauri::Wry>,
) {
    let mut last_label = String::new();
    let mut last_state = PlaybackState::Stopped;

    loop {
        let engine: &Arc<AudioEngine> = &*app.state::<Arc<AudioEngine>>();
        let status = engine.status();

        // Update track info label.
        let label = match &status.metadata {
            Some(meta) => {
                let artist = meta.artist.as_deref().unwrap_or("");
                let title = meta.title.as_deref().unwrap_or("Unknown");
                if artist.is_empty() {
                    title.to_string()
                } else {
                    format!("{artist} — {title}")
                }
            }
            None => "RetroAmp".to_string(),
        };

        if label != last_label {
            // Truncate long titles for the menu.
            let display = if label.len() > 50 {
                format!("{}...", &label[..47])
            } else {
                label.clone()
            };
            let _ = track_info.set_text(&display);
            last_label = label;
        }

        // Update play/pause label.
        if status.state != last_state {
            let pp_label = match status.state {
                PlaybackState::Playing => "Pause",
                _ => "Play",
            };
            let _ = play_pause.set_text(pp_label);
            last_state = status.state;
        }

        thread::sleep(Duration::from_millis(500));
    }
}
