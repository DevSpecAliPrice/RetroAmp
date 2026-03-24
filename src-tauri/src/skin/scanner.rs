//! Skin scanner — discovers skins in the skins directories.
//!
//! Recursively scans for both .wsz archives and extracted directories.
//! Identifies whether each skin is classic (BMP-based) or modern (XML-based).

use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SkinType {
    Classic,
    Modern,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkinInfo {
    /// Display name (derived from filename/directory name).
    pub name: String,
    /// Full path to the .wsz file or directory.
    pub path: String,
    /// Whether this is an archive (.wsz) or an extracted directory.
    pub is_archive: bool,
    /// Classic (BMP) or Modern (XML).
    pub skin_type: SkinType,
}

/// Scan directories recursively for skins.
pub fn scan_all(dirs: &[PathBuf]) -> Vec<SkinInfo> {
    let mut all = Vec::new();
    for dir in dirs {
        if dir.exists() {
            scan_recursive(dir, &mut all);
        }
    }
    all.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
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
                    skin_type: SkinType::Classic,
                });
            }
        } else if path.is_dir() {
            let skin_type = detect_skin_type(&path);
            if skin_type != SkinType::Unknown {
                // It's a skin directory — add it.
                results.push(SkinInfo {
                    name,
                    path: path.to_string_lossy().to_string(),
                    is_archive: false,
                    skin_type,
                });
            } else {
                // Not a skin — recurse into it to find skins inside.
                scan_recursive(&path, results);
            }
        }
    }
}

/// Detect whether a directory contains a classic or modern skin.
fn detect_skin_type(dir: &Path) -> SkinType {
    if dir.join("skin.xml").exists() || dir.join("Skin.xml").exists() {
        return SkinType::Modern;
    }

    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if name == "main.bmp" {
                return SkinType::Classic;
            }
        }
    }

    SkinType::Unknown
}
