//! Window manager — tracks all application windows, handles position
//! persistence, snap-to-dock behaviour, and layout state restoration.
//!
//! Every window in RetroAmp (main player, EQ, playlist, library browser,
//! tag editor, skin browser, Milkdrop) registers with the window manager.
//! This is the single place where window behaviour is defined.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Identifies a specific window type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WindowId {
    Main,
    Equalizer,
    Playlist,
    LibraryBrowser,
    TagEditor,
    SkinBrowser,
    Milkdrop,
}

/// Stored position and size of a window.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct WindowGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Full state of a managed window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowState {
    pub id: WindowId,
    pub geometry: WindowGeometry,
    pub visible: bool,
    /// Which window this one is snapped to, if any.
    pub snapped_to: Option<WindowId>,
    /// Which edge it's snapped on.
    pub snap_edge: Option<SnapEdge>,
}

/// The edge along which one window snaps to another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SnapEdge {
    Bottom,
    Right,
    Left,
    Top,
}

/// Snap distance in pixels — windows within this distance of each other's
/// edges will magnetise together.
const SNAP_DISTANCE: i32 = 10;

/// The window manager.
///
/// Tracks all window states and provides snap logic. Window positions are
/// persisted to SQLite (not implemented yet — will be added when the SQLite
/// layer lands in Phase 3, but the data model is ready).
pub struct WindowManager {
    windows: HashMap<WindowId, WindowState>,
}

impl WindowManager {
    pub fn new() -> Self {
        Self {
            windows: HashMap::new(),
        }
    }

    /// Register a window with its initial state.
    pub fn register(&mut self, state: WindowState) {
        self.windows.insert(state.id, state);
    }

    /// Update the geometry of a window (e.g. after the user drags it).
    pub fn update_geometry(&mut self, id: WindowId, geometry: WindowGeometry) {
        if let Some(state) = self.windows.get_mut(&id) {
            state.geometry = geometry;
        }
    }

    /// Set visibility of a window.
    pub fn set_visible(&mut self, id: WindowId, visible: bool) {
        if let Some(state) = self.windows.get_mut(&id) {
            state.visible = visible;
        }
    }

    /// Get the state of a specific window.
    pub fn get(&self, id: WindowId) -> Option<&WindowState> {
        self.windows.get(&id)
    }

    /// Get all window states (for layout persistence).
    pub fn all_states(&self) -> Vec<&WindowState> {
        self.windows.values().collect()
    }

    /// Get all visible window states.
    pub fn visible_windows(&self) -> Vec<&WindowState> {
        self.windows.values().filter(|w| w.visible).collect()
    }

    /// Calculate the snap position for a window being dragged to a proposed
    /// position. Returns the snapped position if within snap distance of
    /// another window's edge, otherwise returns the proposed position unchanged.
    pub fn calculate_snap(
        &self,
        id: WindowId,
        proposed: WindowGeometry,
    ) -> (WindowGeometry, Option<WindowId>, Option<SnapEdge>) {
        let mut best_x = proposed.x;
        let mut best_y = proposed.y;
        let mut snapped_to = None;
        let mut snap_edge = None;
        let mut min_distance = SNAP_DISTANCE + 1;

        for (other_id, other) in &self.windows {
            if *other_id == id || !other.visible {
                continue;
            }

            let other_geo = &other.geometry;

            // Check bottom snap: proposed window's top edge near other's bottom edge
            let dy_bottom =
                (proposed.y - (other_geo.y + other_geo.height as i32)).abs();
            if dy_bottom < min_distance
                && horizontal_overlap(&proposed, other_geo)
            {
                best_y = other_geo.y + other_geo.height as i32;
                min_distance = dy_bottom;
                snapped_to = Some(*other_id);
                snap_edge = Some(SnapEdge::Bottom);
            }

            // Check top snap: proposed window's bottom edge near other's top edge
            let dy_top =
                ((proposed.y + proposed.height as i32) - other_geo.y).abs();
            if dy_top < min_distance
                && horizontal_overlap(&proposed, other_geo)
            {
                best_y = other_geo.y - proposed.height as i32;
                if dy_top < min_distance {
                    min_distance = dy_top;
                    snapped_to = Some(*other_id);
                    snap_edge = Some(SnapEdge::Top);
                }
            }

            // Check right snap: proposed window's left edge near other's right edge
            let dx_right =
                (proposed.x - (other_geo.x + other_geo.width as i32)).abs();
            if dx_right < min_distance
                && vertical_overlap(&proposed, other_geo)
            {
                best_x = other_geo.x + other_geo.width as i32;
                min_distance = dx_right;
                snapped_to = Some(*other_id);
                snap_edge = Some(SnapEdge::Right);
            }

            // Check left snap: proposed window's right edge near other's left edge
            let dx_left =
                ((proposed.x + proposed.width as i32) - other_geo.x).abs();
            if dx_left < min_distance
                && vertical_overlap(&proposed, other_geo)
            {
                best_x = other_geo.x - proposed.width as i32;
                min_distance = dx_left;
                snapped_to = Some(*other_id);
                snap_edge = Some(SnapEdge::Left);
            }

            // Also snap left edges to align
            let dx_align_left = (proposed.x - other_geo.x).abs();
            if dx_align_left < SNAP_DISTANCE && dx_align_left < min_distance {
                best_x = other_geo.x;
            }
        }

        let snapped_geo = WindowGeometry {
            x: best_x,
            y: best_y,
            ..proposed
        };

        (snapped_geo, snapped_to, snap_edge)
    }

    /// When the main window is dragged, move all snapped windows with it.
    pub fn drag_group(&mut self, dragged_id: WindowId, dx: i32, dy: i32) {
        // Collect IDs of windows snapped to the dragged window
        let snapped_ids: Vec<WindowId> = self
            .windows
            .values()
            .filter(|w| w.snapped_to == Some(dragged_id))
            .map(|w| w.id)
            .collect();

        // Move the dragged window
        if let Some(state) = self.windows.get_mut(&dragged_id) {
            state.geometry.x += dx;
            state.geometry.y += dy;
        }

        // Move all snapped windows by the same delta (and recurse)
        for snapped_id in snapped_ids {
            self.drag_group(snapped_id, dx, dy);
        }
    }
}

/// Check if two geometries overlap horizontally (for vertical snapping).
fn horizontal_overlap(a: &WindowGeometry, b: &WindowGeometry) -> bool {
    let a_right = a.x + a.width as i32;
    let b_right = b.x + b.width as i32;
    a.x < b_right && a_right > b.x
}

/// Check if two geometries overlap vertically (for horizontal snapping).
fn vertical_overlap(a: &WindowGeometry, b: &WindowGeometry) -> bool {
    let a_bottom = a.y + a.height as i32;
    let b_bottom = b.y + b.height as i32;
    a.y < b_bottom && a_bottom > b.y
}
