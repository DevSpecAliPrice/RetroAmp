//! Native OS context menus — replaces HTML-based menus so they can overflow
//! the webview window, like every other desktop application.
//!
//! On Wayland, `popup()` can't determine cursor position, so we always
//! receive `(x, y)` from the frontend and use `popup_at()`.
//!
//! On GTK3 (Linux), `popup_at()` is non-blocking — it returns immediately
//! before the user interacts with the menu. So instead of reading a result
//! after `popup_at()` returns, we emit a Tauri event from the per-window
//! `on_menu_event` handler and let the frontend listen for it.

use serde::Deserialize;
use tauri::menu::{ContextMenu as _, Menu, MenuBuilder, MenuItem, PredefinedMenuItem, Submenu, SubmenuBuilder};
use tauri::{Emitter, LogicalPosition, Position};

/// A single entry in a context menu request from the frontend.
#[derive(Deserialize)]
#[serde(tag = "type")]
pub enum ContextMenuItem {
    #[serde(rename = "item")]
    Item {
        id: String,
        label: String,
        #[serde(default)]
        disabled: bool,
    },
    #[serde(rename = "separator")]
    Separator,
    #[serde(rename = "submenu")]
    Submenu {
        label: String,
        items: Vec<ContextMenuItem>,
    },
}

/// Build a native `Menu` from a list of `ContextMenuItem` entries.
fn build_menu(
    window: &tauri::Window,
    items: &[ContextMenuItem],
) -> tauri::Result<Menu<tauri::Wry>> {
    let mut builder = MenuBuilder::new(window);
    for item in items {
        match item {
            ContextMenuItem::Item { id, label, disabled } => {
                let mi = MenuItem::with_id(window, id, label, !disabled, None::<&str>)?;
                builder = builder.item(&mi);
            }
            ContextMenuItem::Separator => {
                builder = builder.separator();
            }
            ContextMenuItem::Submenu { label, items: children } => {
                let sub = build_submenu(window, label, children)?;
                builder = builder.item(&sub);
            }
        }
    }
    builder.build()
}

/// Build a native `Submenu` from a label and list of entries.
fn build_submenu(
    window: &tauri::Window,
    label: &str,
    items: &[ContextMenuItem],
) -> tauri::Result<Submenu<tauri::Wry>> {
    let mut builder = SubmenuBuilder::new(window, label);
    for item in items {
        match item {
            ContextMenuItem::Item { id, label, disabled } => {
                let mi = MenuItem::with_id(window, id, label, !disabled, None::<&str>)?;
                builder = builder.item(&mi);
            }
            ContextMenuItem::Separator => {
                let sep = PredefinedMenuItem::separator(window)?;
                builder = builder.item(&sep);
            }
            ContextMenuItem::Submenu { label, items: children } => {
                let sub = build_submenu(window, label, children)?;
                builder = builder.item(&sub);
            }
        }
    }
    builder.build()
}

/// Show a native context menu at `(x, y)` (logical pixels, relative to the
/// window). The selected item ID is delivered asynchronously via a
/// `"context-menu-selected"` Tauri event to the calling window.
#[tauri::command]
pub fn show_context_menu(
    window: tauri::Window,
    items: Vec<ContextMenuItem>,
    x: f64,
    y: f64,
) -> Result<(), String> {
    let menu = build_menu(&window, &items).map_err(|e| e.to_string())?;

    // When the user clicks a menu item, emit a Tauri event with the item ID.
    // This works regardless of whether popup_at() blocks (X11) or not (GTK3/Wayland).
    let emitter = window.clone();
    window.on_menu_event(move |_win, event| {
        let _ = emitter.emit("context-menu-selected", event.id().as_ref().to_string());
    });

    let pos = Position::Logical(LogicalPosition::new(x, y));
    menu.popup_at(window, pos).map_err(|e| e.to_string())?;

    Ok(())
}
