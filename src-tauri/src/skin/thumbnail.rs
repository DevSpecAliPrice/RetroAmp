//! Thumbnail extraction — pull just `main.bmp` from a skin for preview.
//!
//! Much faster than a full skin load because only one file is read.

use std::io::Read;
use std::path::Path;

use base64::Engine;

/// Extract the `main.bmp` image from a skin as a base64 data URI.
///
/// Works with both .wsz archives and extracted directories.
/// Returns `None` if main.bmp is not present.
pub fn extract_thumbnail(skin_path: &str) -> Option<String> {
    let path = Path::new(skin_path);
    if path.is_dir() {
        extract_from_directory(path)
    } else {
        extract_from_archive(path)
    }
}

fn extract_from_archive(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;

    for i in 0..archive.len() {
        let mut entry = match archive.by_index(i) {
            Ok(e) => e,
            Err(_) => continue,
        };

        if entry.is_dir() {
            continue;
        }

        // Get just the filename (skip directory prefixes).
        let raw_name = entry.name().to_string();
        let filename = raw_name.rsplit('/').next().unwrap_or(&raw_name);

        if filename.eq_ignore_ascii_case("main.bmp") {
            let mut data = Vec::new();
            if entry.read_to_end(&mut data).is_ok() {
                return Some(encode_data_uri(&data, "image/bmp"));
            }
        }
    }

    None
}

fn extract_from_directory(dir: &Path) -> Option<String> {
    // Case-insensitive search for main.bmp.
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        if name.to_string_lossy().eq_ignore_ascii_case("main.bmp") {
            let data = std::fs::read(entry.path()).ok()?;
            return Some(encode_data_uri(&data, "image/bmp"));
        }
    }
    None
}

fn encode_data_uri(data: &[u8], mime: &str) -> String {
    let b64 = base64::engine::general_purpose::STANDARD.encode(data);
    format!("data:{mime};base64,{b64}")
}
