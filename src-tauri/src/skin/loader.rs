//! WSZ skin loader — unzips .wsz files and returns the contents to the frontend.
//!
//! WSZ files are ZIP archives containing BMP images, text config files,
//! and optionally cursors and region definitions. The loader extracts
//! everything and returns it as a structured map that the frontend
//! skin parser can consume.

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use base64::Engine;
use serde::Serialize;

/// The complete contents of a loaded .wsz skin, ready for the frontend.
#[derive(Debug, Clone, Serialize)]
pub struct SkinContents {
    /// BMP/PNG image files as base64-encoded data URIs.
    /// Keys are normalised lowercase filenames without extension (e.g. "main", "cbuttons").
    pub images: HashMap<String, String>,
    /// Text files as UTF-8 strings.
    /// Keys are normalised lowercase filenames without extension (e.g. "pledit", "viscolor").
    pub texts: HashMap<String, String>,
}

/// Load a .wsz skin file and return its contents.
pub fn load_wsz(path: impl AsRef<Path>) -> Result<SkinContents, String> {
    let file = File::open(path.as_ref())
        .map_err(|e| format!("failed to open skin: {e}"))?;

    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("failed to read skin archive: {e}"))?;

    let mut images: HashMap<String, String> = HashMap::new();
    let mut texts: HashMap<String, String> = HashMap::new();

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)
            .map_err(|e| format!("failed to read archive entry: {e}"))?;

        let raw_name = entry.name().to_string();

        // Skip directories.
        if entry.is_dir() {
            continue;
        }

        // Get just the filename (skip any directory prefix some skins include).
        let filename = raw_name
            .rsplit('/')
            .next()
            .unwrap_or(&raw_name)
            .to_string();

        let ext = filename
            .rsplit('.')
            .next()
            .unwrap_or("")
            .to_lowercase();

        // Normalise the key: lowercase filename without extension.
        let key = filename
            .rsplit('.')
            .skip(1)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join(".")
            .to_lowercase();

        if key.is_empty() {
            continue;
        }

        let mut data = Vec::new();
        entry.read_to_end(&mut data)
            .map_err(|e| format!("failed to read {filename}: {e}"))?;

        match ext.as_str() {
            "bmp" | "png" | "jpg" | "jpeg" | "gif" => {
                let mime = match ext.as_str() {
                    "bmp" => "image/bmp",
                    "png" => "image/png",
                    "jpg" | "jpeg" => "image/jpeg",
                    "gif" => "image/gif",
                    _ => "application/octet-stream",
                };
                let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
                let data_uri = format!("data:{mime};base64,{b64}");
                images.insert(key, data_uri);
            }
            "txt" | "ini" => {
                // Text files — try UTF-8, fall back to latin-1.
                let text = String::from_utf8(data.clone()).unwrap_or_else(|_| {
                    data.iter().map(|&b| b as char).collect()
                });
                texts.insert(key, text);
            }
            _ => {
                // Skip unknown file types (cursors, etc. — can add later).
            }
        }
    }

    Ok(SkinContents { images, texts })
}
