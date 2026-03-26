/**
 * Native OS context menus via Tauri — replaces the HTML ContextMenu component
 * so menus can overflow the window like every other desktop app.
 *
 * On GTK3 (Linux), popup_at() is non-blocking, so we can't read the result
 * synchronously. Instead, the Rust side emits a "context-menu-selected" event
 * when the user clicks an item, and we resolve the Promise here.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface NativeMenuItem {
  type: "item";
  id: string;
  label: string;
  disabled?: boolean;
}

export interface NativeMenuSeparator {
  type: "separator";
}

export interface NativeMenuSubmenu {
  type: "submenu";
  label: string;
  items: NativeMenuEntry[];
}

export type NativeMenuEntry = NativeMenuItem | NativeMenuSeparator | NativeMenuSubmenu;

/** Clean up any listener from the previous context menu call. */
let pendingCleanup: (() => void) | null = null;

/**
 * Show a native OS context menu at `(x, y)` logical pixels (relative to
 * the window). Returns the ID of the selected item, or `null` if dismissed.
 */
export function showContextMenu(
  items: NativeMenuEntry[],
  x = 0,
  y = 0,
): Promise<string | null> {
  // Cancel any leftover listener from a previous menu.
  if (pendingCleanup) {
    pendingCleanup();
    pendingCleanup = null;
  }

  return new Promise<string | null>((resolve) => {
    let settled = false;
    let unlisten: UnlistenFn | null = null;

    const cleanup = () => {
      if (unlisten) unlisten();
      unlisten = null;
      if (pendingCleanup === cleanup) pendingCleanup = null;
    };

    const settle = (value: string | null) => {
      if (settled) return;
      settled = true;
      cleanup();
      resolve(value);
    };

    // 1. Start listening for the selection event BEFORE showing the menu.
    listen<string>("context-menu-selected", (event) => {
      settle(event.payload);
    }).then((fn) => {
      unlisten = fn;
      // If already settled (race), clean up immediately.
      if (settled) cleanup();
    });

    // 2. Show the menu. On X11 this blocks until dismissed; on GTK3/Wayland
    //    it returns immediately.
    invoke("show_context_menu", { items, x, y }).then(() => {
      // popup_at returned. On blocking platforms the event already arrived.
      // On non-blocking platforms the menu may still be open — don't resolve
      // yet; the listener will handle it. Store cleanup so the NEXT menu
      // call can cancel this listener if the user right-clicks elsewhere
      // without selecting anything.
      pendingCleanup = () => settle(null);
    }).catch(() => {
      settle(null);
    });
  });
}
