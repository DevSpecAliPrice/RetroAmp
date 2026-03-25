//! Skin scanner — discovers classic Winamp skins in the skins directories.
//!
//! Recursively scans for .wsz archives and extracted directories containing
//! classic BMP-based skins. Modern XML-based skins (.wal) are not supported.

use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct SkinInfo {
    /// Display name (derived from filename/directory name).
    pub name: String,
    /// Full path to the .wsz file or directory.
    pub path: String,
    /// Whether this is an archive (.wsz) or an extracted directory.
    pub is_archive: bool,
}

/// Scan directories recursively for skins.
pub fn scan_all(dirs: &[PathBuf]) -> Vec<SkinInfo> {
    let mut all = Vec::new();
    for dir in dirs {
        if dir.exists() {
            scan_recursive(dir, &mut all);
        }
    }
    all.sort_by(|a, b| {
        let a_default = a.name == super::default::SKIN_NAME;
        let b_default = b.name == super::default::SKIN_NAME;
        b_default.cmp(&a_default)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    all.dedup_by(|a, b| a.name.to_lowercase() == b.name.to_lowercase());
    all
}

fn scan_recursive(dir: &Path, results: &mut Vec<SkinInfo>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        if path.is_file() {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();

            if ext == "wsz" || ext == "zip" {
                let display_name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&name)
                    .to_string();

                results.push(SkinInfo {
                    name: display_name,
                    path: path.to_string_lossy().to_string(),
                    is_archive: true,
                });
            }
        } else if path.is_dir() {
            if is_classic_skin(&path) {
                results.push(SkinInfo {
                    name,
                    path: path.to_string_lossy().to_string(),
                    is_archive: false,
                });
            } else {
                // Not a skin — recurse into it to find skins inside.
                scan_recursive(&path, results);
            }
        }
    }
}

/// Check whether a directory contains a classic (BMP-based) skin.
/// Directories with skin.xml are modern skins and are skipped.
fn is_classic_skin(dir: &Path) -> bool {
    // Modern skins have skin.xml — skip them.
    if dir.join("skin.xml").exists() || dir.join("Skin.xml").exists() {
        return false;
    }

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if name == "main.bmp" {
                return true;
            }
        }
    }

    false
}
