//! Tag reading and writing via lofty.
//!
//! File tags are the source of truth. The database is a cache. When the user
//! edits metadata in RetroAmp, we write to the file first, then update the DB.

use std::path::Path;

use lofty::file::TaggedFileExt;
use lofty::prelude::*;
use lofty::tag::{ItemKey, ItemValue, Tag};
use sha2::{Digest, Sha256};

/// A fully scanned track ready for DB insertion.
pub struct ScannedTrack {
    pub path: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album_artist: Option<String>,
    pub album: Option<String>,
    pub genre: Option<String>,
    pub year: Option<i32>,
    pub track_number: Option<i32>,
    pub disc_number: Option<i32>,
    pub duration_ms: Option<i64>,
    pub bitrate: Option<i32>,
    pub sample_rate: Option<i32>,
    pub channels: Option<i32>,
    pub rating: u8,
    pub file_size: i64,
    pub file_mtime: i64,
    pub cover_art: Option<CoverArt>,
    pub format: String,
    pub has_tags: bool,
}

/// Extracted cover art with content-addressed hash.
pub struct CoverArt {
    pub hash: String,
    pub data: Vec<u8>,
    pub mime_type: String,
}

/// Read all metadata from an audio file.
pub fn read_tags(path: &Path, file_size: u64, file_mtime: u64) -> Result<ScannedTrack, String> {
    let tagged_file = lofty::read_from_path(path).map_err(|e| format!("{e}"))?;

    let properties = tagged_file.properties();
    let duration_ms = properties.duration().as_millis() as i64;
    let bitrate = properties.audio_bitrate().map(|b| b as i32);
    let sample_rate = properties.sample_rate().map(|s| s as i32);
    let channels = properties.channels().map(|c| c as i32);

    let format = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("unknown")
        .to_lowercase();

    // Try to get a tag — prefer primary, fall back to any available.
    let tag = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag());

    let (title, artist, album_artist, album, genre, year, track_number, disc_number, rating, cover_art, has_tags) =
        match tag {
            Some(tag) => {
                let title = tag.title().map(|s| s.to_string());
                let artist = tag.artist().map(|s| s.to_string());
                let album_artist = tag
                    .get_string(&ItemKey::AlbumArtist)
                    .map(|s| s.to_string());
                let album = tag.album().map(|s| s.to_string());
                let genre = tag.genre().map(|s| s.to_string());
                let year = tag.year().map(|y| y as i32);
                let track_number = tag.track().map(|t| t as i32);
                let disc_number = tag.disk().map(|d| d as i32);
                let rating = read_rating(tag);
                let cover_art = extract_cover_art(tag);
                let has_tags = true;

                (
                    title,
                    artist,
                    album_artist,
                    album,
                    genre,
                    year,
                    track_number,
                    disc_number,
                    rating,
                    cover_art,
                    has_tags,
                )
            }
            None => {
                // No tags at all (e.g. raw WAV).
                (None, None, None, None, None, None, None, None, 0, None, false)
            }
        };

    // Fall back to filename as title if no tag title.
    let title = title.or_else(|| {
        path.file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
    });

    Ok(ScannedTrack {
        path: path.to_string_lossy().to_string(),
        title,
        artist,
        album_artist,
        album,
        genre,
        year,
        track_number,
        disc_number,
        duration_ms: if duration_ms > 0 {
            Some(duration_ms)
        } else {
            None
        },
        bitrate,
        sample_rate,
        channels,
        rating,
        file_size: file_size as i64,
        file_mtime: file_mtime as i64,
        cover_art,
        format,
        has_tags,
    })
}

/// Read star rating from a tag, handling format-specific conventions.
/// Returns 0-5 (0 = unrated).
fn read_rating(tag: &Tag) -> u8 {
    // ID3v2 POPM: rating byte is 0-255.
    for item in tag.items() {
        if item.key() == &ItemKey::Popularimeter {
            if let ItemValue::Binary(data) = item.value() {
                // POPM binary layout: null-terminated email, then rating byte, then play count.
                if let Some(null_pos) = data.iter().position(|&b| b == 0) {
                    if data.len() > null_pos + 1 {
                        return popm_to_stars(data[null_pos + 1]);
                    }
                }
            }
        }
    }

    // Vorbis Comments / generic: FMPS_RATING (float 0.0-1.0).
    if let Some(val) = tag.get_string(&ItemKey::Unknown("FMPS_RATING".into())) {
        if let Ok(f) = val.parse::<f32>() {
            return (f * 5.0).round().clamp(0.0, 5.0) as u8;
        }
    }

    // Some players use a RATING tag (integer 0-100).
    if let Some(val) = tag.get_string(&ItemKey::Unknown("RATING".into())) {
        if let Ok(n) = val.parse::<u32>() {
            return ((n as f32 / 100.0) * 5.0).round().clamp(0.0, 5.0) as u8;
        }
    }

    0
}

/// Convert POPM rating byte (0-255) to 0-5 stars (Winamp-compatible mapping).
fn popm_to_stars(rating: u8) -> u8 {
    match rating {
        0 => 0,
        1..=31 => 1,
        32..=95 => 2,
        96..=159 => 3,
        160..=223 => 4,
        224..=255 => 5,
    }
}

/// Convert 0-5 stars back to a POPM rating byte.
pub fn stars_to_popm(stars: u8) -> u8 {
    match stars {
        0 => 0,
        1 => 1,
        2 => 64,
        3 => 128,
        4 => 196,
        _ => 255,
    }
}

/// Extract the first embedded picture and compute its content hash.
fn extract_cover_art(tag: &Tag) -> Option<CoverArt> {
    let picture = tag.pictures().first()?;
    let data = picture.data();
    if data.is_empty() {
        return None;
    }

    let mut hasher = Sha256::new();
    hasher.update(data);
    let hash = format!("{:x}", hasher.finalize());

    let mime_type = picture.mime_type().map_or_else(
        || "image/jpeg".to_string(),
        |m| m.to_string(),
    );

    Some(CoverArt {
        hash,
        data: data.to_vec(),
        mime_type,
    })
}

/// Write a star rating to the file tags. Writes to the file first (source of
/// truth), using the format-appropriate convention.
pub fn write_rating(path: &str, stars: u8) -> Result<(), String> {
    let mut tagged_file = lofty::read_from_path(path).map_err(|e| format!("{e}"))?;

    let tag = match tagged_file.primary_tag_mut() {
        Some(t) => t,
        None => {
            // Create a tag appropriate for the format.
            let tag_type = tagged_file.primary_tag_type();
            tagged_file.insert_tag(Tag::new(tag_type));
            tagged_file
                .primary_tag_mut()
                .ok_or("failed to create tag")?
        }
    };

    // Remove existing rating items before writing.
    tag.remove_key(&ItemKey::Popularimeter);
    tag.remove_key(&ItemKey::Unknown("FMPS_RATING".into()));
    tag.remove_key(&ItemKey::Unknown("RATING".into()));

    if stars > 0 {
        let popm_byte = stars_to_popm(stars);

        // Write format-appropriate rating.
        // For ID3v2, write a POPM frame. For others, use FMPS_RATING.
        match tag.tag_type() {
            lofty::tag::TagType::Id3v2 => {
                // POPM: email (null-terminated) + rating byte + 4-byte play count.
                let mut popm_data = Vec::new();
                popm_data.extend_from_slice(b"RetroAmp\0");
                popm_data.push(popm_byte);
                popm_data.extend_from_slice(&0u32.to_be_bytes());
                tag.push(lofty::tag::TagItem::new(
                    ItemKey::Popularimeter,
                    ItemValue::Binary(popm_data),
                ));
            }
            _ => {
                // Vorbis Comments, MP4: use FMPS_RATING (float 0.0-1.0).
                let fmps = format!("{:.2}", stars as f32 / 5.0);
                tag.push(lofty::tag::TagItem::new(
                    ItemKey::Unknown("FMPS_RATING".into()),
                    ItemValue::Text(fmps),
                ));
            }
        }
    }

    tag.save_to_path(path, lofty::config::WriteOptions::default())
        .map_err(|e| format!("failed to write tags: {e}"))?;

    Ok(())
}
