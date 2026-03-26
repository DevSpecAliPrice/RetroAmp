//! Media library — scans user directories, reads file tags via lofty, and
//! indexes metadata into SQLite. File tags are always the source of truth;
//! the database is a cache/index.

pub mod db;
pub mod scanner;
pub mod tags;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::Emitter;

use crate::db::Database;

/// Global flag to prevent concurrent scans.
static SCAN_RUNNING: AtomicBool = AtomicBool::new(false);

/// Progress event emitted during a library scan.
#[derive(Clone, Serialize)]
pub struct LibraryScanProgress {
    pub current: usize,
    pub total: usize,
    pub phase: &'static str,
    pub file_name: String,
    pub new_tracks: usize,
    pub updated_tracks: usize,
}

/// Returns true if a scan is currently in progress.
pub fn is_scanning() -> bool {
    SCAN_RUNNING.load(Ordering::Relaxed)
}

/// Run a full library scan. Designed to be called from a background thread.
///
/// Algorithm:
/// 1. Walk configured directories and collect audio files
/// 2. Bulk-load existing file stats from DB (path → mtime/size)
/// 3. Filter to files that are new or changed
/// 4. Read tags with lofty, extract cover art, upsert into DB
/// 5. Prune tracks whose files no longer exist on disk
pub fn scan_library(database: Arc<Mutex<Database>>, app: tauri::AppHandle) {
    if SCAN_RUNNING.swap(true, Ordering::SeqCst) {
        log::warn!("library scan already in progress, skipping");
        return;
    }

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        do_scan(&database, &app);
    }));

    SCAN_RUNNING.store(false, Ordering::SeqCst);

    if let Err(e) = result {
        log::error!("library scan panicked: {e:?}");
    }
}

fn do_scan(database: &Arc<Mutex<Database>>, app: &tauri::AppHandle) {
    log::info!("starting library scan");

    let _ = app.emit(
        "library-scan-progress",
        LibraryScanProgress {
            current: 0,
            total: 0,
            phase: "scanning",
            file_name: String::new(),
            new_tracks: 0,
            updated_tracks: 0,
        },
    );

    // Phase 1: Load configured library directories.
    let dirs = match database.lock() {
        Ok(db) => db::get_library_dirs(db.conn()),
        Err(_) => Vec::new(),
    };

    if dirs.is_empty() {
        log::info!("no library directories configured, skipping scan");
        let _ = app.emit(
            "library-scan-progress",
            LibraryScanProgress {
                current: 0,
                total: 0,
                phase: "done",
                file_name: String::new(),
                new_tracks: 0,
                updated_tracks: 0,
            },
        );
        return;
    }

    // Phase 2: Walk directories and collect audio files.
    let files = scanner::walk_directories(&dirs);
    let total = files.len();
    log::info!("found {total} audio files across {} directories", dirs.len());

    // Phase 3: Bulk-load existing file stats for change detection.
    let existing_stats: HashMap<String, (i64, i64)> = match database.lock() {
        Ok(db) => db::get_file_stats(db.conn()),
        Err(_) => HashMap::new(),
    };

    // Phase 4: Determine which files need scanning.
    let mut to_scan = Vec::new();
    let mut valid_paths = Vec::with_capacity(total);

    for entry in &files {
        let path_str = entry.path.to_string_lossy().to_string();
        valid_paths.push(path_str.clone());

        match existing_stats.get(&path_str) {
            Some(&(db_size, db_mtime)) => {
                if entry.size as i64 != db_size || entry.mtime as i64 != db_mtime {
                    to_scan.push(entry);
                }
            }
            None => {
                to_scan.push(entry);
            }
        }
    }

    let scan_total = to_scan.len();
    log::info!("{scan_total} files need scanning ({} unchanged)", total - scan_total);

    // Phase 5: Read tags and upsert into DB.
    let mut new_tracks = 0usize;
    let mut updated_tracks = 0usize;

    for (i, entry) in to_scan.iter().enumerate() {
        let path_str = entry.path.to_string_lossy().to_string();
        let is_new = !existing_stats.contains_key(&path_str);

        // Read tags WITHOUT holding the DB lock.
        let scanned = tags::read_tags(&entry.path, entry.size, entry.mtime);

        // Brief lock to write.
        if let Ok(db) = database.lock() {
            match &scanned {
                Ok(track) => {
                    // Store cover art if present.
                    if let Some(ref cover) = track.cover_art {
                        let _ = db::upsert_cover(
                            db.conn(),
                            &cover.hash,
                            &cover.data,
                            &cover.mime_type,
                        );
                    }
                    let _ = db::upsert_track(db.conn(), track);
                }
                Err(e) => {
                    // Record the file with an error so we don't retry every scan.
                    let _ = db::upsert_error(db.conn(), &path_str, entry.size, entry.mtime, e);
                }
            }
        }

        if is_new {
            new_tracks += 1;
        } else {
            updated_tracks += 1;
        }

        // Emit progress every 25 files.
        if i % 25 == 0 || i == scan_total - 1 {
            let file_name = entry
                .path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let _ = app.emit(
                "library-scan-progress",
                LibraryScanProgress {
                    current: i + 1,
                    total: scan_total,
                    phase: "reading-tags",
                    file_name,
                    new_tracks,
                    updated_tracks,
                },
            );
        }
    }

    // Phase 6: Prune tracks that no longer exist on disk.
    if let Ok(db) = database.lock() {
        match db::remove_missing_tracks(db.conn(), &valid_paths) {
            Ok(0) => {}
            Ok(n) => log::info!("pruned {n} missing tracks from library"),
            Err(e) => log::warn!("failed to prune missing tracks: {e}"),
        }
        // Clean up orphaned covers.
        let _ = db::remove_orphaned_covers(db.conn());
    }

    let _ = app.emit(
        "library-scan-progress",
        LibraryScanProgress {
            current: scan_total,
            total: scan_total,
            phase: "done",
            file_name: String::new(),
            new_tracks,
            updated_tracks,
        },
    );

    log::info!(
        "library scan complete: {new_tracks} new, {updated_tracks} updated, {total} total files"
    );
}
