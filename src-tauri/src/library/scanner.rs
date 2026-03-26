//! Directory walking and audio file discovery.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// Supported audio file extensions.
const EXTENSIONS: &[&str] = &["mp3", "flac", "ogg", "wav", "aac", "m4a", "alac"];

/// A discovered audio file with its filesystem metadata.
pub struct FileEntry {
    pub path: PathBuf,
    pub size: u64,
    /// Unix timestamp (seconds since epoch).
    pub mtime: u64,
}

/// Walk multiple directories recursively and collect all audio files.
pub fn walk_directories(dirs: &[PathBuf]) -> Vec<FileEntry> {
    let mut result = Vec::new();
    for dir in dirs {
        walk_recursive(dir, &mut result);
    }
    result
}

fn walk_recursive(dir: &Path, out: &mut Vec<FileEntry>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            log::warn!("cannot read directory {}: {e}", dir.display());
            return;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_recursive(&path, out);
        } else if is_audio_file(&path) {
            if let Ok(meta) = std::fs::metadata(&path) {
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                out.push(FileEntry {
                    path,
                    size: meta.len(),
                    mtime,
                });
            }
        }
    }
}

fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| EXTENSIONS.contains(&ext.to_lowercase().as_str()))
}
