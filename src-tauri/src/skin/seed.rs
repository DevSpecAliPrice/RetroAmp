//! Seed library: bundled .wsz skins shipped with the installer.
//!
//! On startup, copies each bundled skin into the user's skins directory the
//! first time it's seen (tracked per-skin in the `seeded_skins` table). Once
//! a skin is recorded as seeded, the user owns it — deleting it from their
//! skins folder doesn't bring it back, and updated bundled bytes never
//! overwrite the user's copy.
//!
//! The bundled set lives under `<repo>/skins/` and is mapped into the app
//! resource dir at `skins/` by `tauri.conf.json -> bundle.resources`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Manager};

use crate::db::Database;

/// Copy every bundled seed skin that hasn't been seeded yet into the user's
/// skins directory, recording each in the `seeded_skins` table.
///
/// Best-effort: individual failures are logged but never bubble up — the app
/// must still start even if the resource dir is unreadable or the user's
/// config dir isn't writable.
pub fn ensure_seed_skins(app: &AppHandle, database: &Arc<Mutex<Database>>) {
    let Some(bundled_dir) = bundled_skins_dir(app) else {
        log::debug!("no bundled skins dir in resources — skipping seed");
        return;
    };
    let Some(user_dir) = user_skins_dir() else {
        log::warn!("cannot determine user config dir — skipping seed");
        return;
    };

    if let Err(e) = std::fs::create_dir_all(&user_dir) {
        log::warn!("could not create user skins dir {}: {e}", user_dir.display());
        return;
    }

    let entries = match std::fs::read_dir(&bundled_dir) {
        Ok(e) => e,
        Err(e) => {
            log::warn!("cannot read bundled skins dir {}: {e}", bundled_dir.display());
            return;
        }
    };

    let mut seeded = 0usize;
    let mut skipped = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("wsz") {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        let already = match database.lock() {
            Ok(db) => db.is_skin_seeded(name).unwrap_or(false),
            Err(_) => {
                log::warn!("seed DB lock poisoned — aborting seed");
                return;
            }
        };
        if already {
            skipped += 1;
            continue;
        }

        let dst = user_dir.join(name);
        if let Err(e) = std::fs::copy(&path, &dst) {
            log::warn!("failed to seed {name}: {e}");
            continue;
        }
        if let Ok(db) = database.lock() {
            if let Err(e) = db.mark_skin_seeded(name) {
                log::warn!("seeded {name} on disk but failed to record in db: {e}");
            }
        }
        seeded += 1;
    }

    if seeded > 0 || skipped > 0 {
        log::info!(
            "skin seeder: copied {seeded} new, skipped {skipped} already-seeded into {}",
            user_dir.display()
        );
    }
}

fn bundled_skins_dir(app: &AppHandle) -> Option<PathBuf> {
    let resource_dir = app.path().resource_dir().ok()?;
    let dir = resource_dir.join("skins");
    dir.is_dir().then_some(dir)
}

fn user_skins_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|c| c.join("retroamp").join("skins"))
}
