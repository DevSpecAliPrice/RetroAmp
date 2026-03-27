//! Tag reading and writing via lofty.
//!
//! File tags are the source of truth. The database is a cache. When the user
//! edits metadata in RetroAmp, we write to the file first, then update the DB.

use std::path::Path;

use base64::Engine;
use lofty::file::TaggedFileExt;
use lofty::prelude::*;
use lofty::tag::{ItemKey, ItemValue, Tag, TagItem};
use serde::{Deserialize, Serialize};
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
///
/// Lofty may normalize TXXX frame descriptions to mixed-case (e.g.
/// "FMPS_Rating" instead of "FMPS_RATING"), so we iterate all items
/// and compare case-insensitively for Unknown keys.
fn read_rating(tag: &Tag) -> u8 {
    for item in tag.items() {
        match item.key() {
            // ID3v2 POPM: rating byte is 0-255.
            ItemKey::Popularimeter => {
                if let ItemValue::Binary(data) = item.value() {
                    if let Some(null_pos) = data.iter().position(|&b| b == 0) {
                        if data.len() > null_pos + 1 {
                            return popm_to_stars(data[null_pos + 1]);
                        }
                    }
                }
            }
            // FMPS_RATING (float 0.0-1.0) — case-insensitive match.
            // Also matches MP4 freeform "----:com.apple.iTunes:FMPS_Rating".
            ItemKey::Unknown(key)
                if key.eq_ignore_ascii_case("FMPS_RATING")
                    || key.ends_with(":FMPS_Rating")
                    || key.ends_with(":FMPS_RATING") =>
            {
                if let ItemValue::Text(val) = item.value() {
                    if let Ok(f) = val.parse::<f32>() {
                        return (f * 5.0).round().clamp(0.0, 5.0) as u8;
                    }
                }
            }
            // Some players use a RATING tag (integer 0-100).
            ItemKey::Unknown(key) if key.eq_ignore_ascii_case("RATING") => {
                if let ItemValue::Text(val) = item.value() {
                    if let Ok(n) = val.parse::<u32>() {
                        return ((n as f32 / 100.0) * 5.0).round().clamp(0.0, 5.0) as u8;
                    }
                }
            }
            _ => {}
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

/// The freeform atom key used for FMPS_Rating in MP4/M4A files.
const MP4_FMPS_RATING: &str = "----:com.apple.iTunes:FMPS_Rating";

/// Remove all known rating items from a tag (POPM, FMPS_Rating, RATING).
fn remove_rating_keys(tag: &mut Tag) {
    tag.remove_key(&ItemKey::Popularimeter);
    // Remove both exact and case variants that lofty might store.
    tag.remove_key(&ItemKey::Unknown("FMPS_RATING".into()));
    tag.remove_key(&ItemKey::Unknown("FMPS_Rating".into()));
    tag.remove_key(&ItemKey::Unknown("RATING".into()));
    // MP4 freeform key.
    tag.remove_key(&ItemKey::Unknown(MP4_FMPS_RATING.into()));
}

/// Push a rating item appropriate for the tag format.
/// Uses `push_unchecked` because lofty 0.22's `push()` silently rejects
/// Unknown items that don't pass its internal validation for the tag type.
fn push_rating(tag: &mut Tag, stars: u8) {
    let fmps_value = format!("{:.2}", stars as f32 / 5.0);

    match tag.tag_type() {
        lofty::tag::TagType::Id3v2 => {
            // TXXX FMPS_Rating — lofty converts Unknown keys to TXXX frames
            // for ID3v2 and normalizes the case.
            tag.push_unchecked(TagItem::new(
                ItemKey::Unknown("FMPS_Rating".into()),
                ItemValue::Text(fmps_value),
            ));
        }
        lofty::tag::TagType::Mp4Ilst => {
            // MP4 freeform atom: ----:com.apple.iTunes:FMPS_Rating
            tag.push_unchecked(TagItem::new(
                ItemKey::Unknown(MP4_FMPS_RATING.into()),
                ItemValue::Text(fmps_value),
            ));
        }
        _ => {
            // Vorbis Comments and others: plain FMPS_RATING field.
            tag.push_unchecked(TagItem::new(
                ItemKey::Unknown("FMPS_RATING".into()),
                ItemValue::Text(fmps_value),
            ));
        }
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
    remove_rating_keys(tag);

    if stars > 0 {
        push_rating(tag, stars);
    }

    tag.save_to_path(path, lofty::config::WriteOptions::default())
        .map_err(|e| format!("failed to write tags: {e}"))?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Tag editor support
// ---------------------------------------------------------------------------

/// Full tag information for the tag editor UI. Read directly from the file.
#[derive(Debug, Clone, Serialize)]
pub struct TrackTagInfo {
    pub path: String,
    // Editable fields
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album_artist: Option<String>,
    pub album: Option<String>,
    pub genre: Option<String>,
    pub year: Option<i32>,
    pub track_number: Option<i32>,
    pub disc_number: Option<i32>,
    pub comment: Option<String>,
    pub rating: u8,
    // Read-only file info
    pub duration_ms: Option<i64>,
    pub bitrate: Option<i32>,
    pub sample_rate: Option<i32>,
    pub channels: Option<i32>,
    pub file_size: i64,
    pub format: String,
    /// Base64 data URI of embedded cover art, or null.
    pub cover_art_data_uri: Option<String>,
}

/// Edits to apply to a track's tags. `None` = unchanged, `Some("")` = clear.
#[derive(Debug, Clone, Deserialize)]
pub struct TagEdits {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album_artist: Option<String>,
    pub album: Option<String>,
    pub genre: Option<String>,
    pub year: Option<String>,
    pub track_number: Option<String>,
    pub disc_number: Option<String>,
    pub comment: Option<String>,
    /// Rating 0-5. `None` = unchanged, `Some(0)` = clear.
    pub rating: Option<u8>,
}

/// Read all tag information from a file for the tag editor.
pub fn read_track_tags(path: &Path) -> Result<TrackTagInfo, String> {
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

    let file_size = std::fs::metadata(path)
        .map(|m| m.len() as i64)
        .unwrap_or(0);

    let tag = tagged_file
        .primary_tag()
        .or_else(|| tagged_file.first_tag());

    let (title, artist, album_artist, album, genre, year, track_number, disc_number, comment, rating, cover_art_data_uri) =
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
                let comment = tag.comment().map(|s| s.to_string());
                let rating = read_rating(tag);

                // Build inline data URI for cover art.
                let cover_art_data_uri = extract_cover_art(tag).map(|ca| {
                    let b64 = base64::engine::general_purpose::STANDARD.encode(&ca.data);
                    format!("data:{};base64,{}", ca.mime_type, b64)
                });

                (title, artist, album_artist, album, genre, year, track_number, disc_number, comment, rating, cover_art_data_uri)
            }
            None => (None, None, None, None, None, None, None, None, None, 0, None),
        };

    Ok(TrackTagInfo {
        path: path.to_string_lossy().to_string(),
        title,
        artist,
        album_artist,
        album,
        genre,
        year,
        track_number,
        disc_number,
        comment,
        rating,
        duration_ms: if duration_ms > 0 { Some(duration_ms) } else { None },
        bitrate,
        sample_rate,
        channels,
        file_size,
        format,
        cover_art_data_uri,
    })
}

/// Write tag edits to a file. Only fields with `Some` values are changed.
/// An empty string clears the field.
pub fn write_tags(path: &str, edits: &TagEdits) -> Result<(), String> {
    let mut tagged_file = lofty::read_from_path(path).map_err(|e| format!("{e}"))?;

    let tag = match tagged_file.primary_tag_mut() {
        Some(t) => t,
        None => {
            let tag_type = tagged_file.primary_tag_type();
            tagged_file.insert_tag(Tag::new(tag_type));
            tagged_file
                .primary_tag_mut()
                .ok_or("failed to create tag")?
        }
    };

    // Helper: set a text field, or clear it if empty.
    fn set_text(tag: &mut Tag, key: &ItemKey, value: &str) {
        tag.remove_key(key);
        if !value.is_empty() {
            tag.push(TagItem::new(
                key.clone(),
                ItemValue::Text(value.to_string()),
            ));
        }
    }

    if let Some(ref v) = edits.title {
        tag.remove_key(&ItemKey::TrackTitle);
        if !v.is_empty() {
            tag.set_title(v.clone());
        }
    }

    if let Some(ref v) = edits.artist {
        tag.remove_key(&ItemKey::TrackArtist);
        if !v.is_empty() {
            tag.set_artist(v.clone());
        }
    }

    if let Some(ref v) = edits.album_artist {
        set_text(tag, &ItemKey::AlbumArtist, v);
    }

    if let Some(ref v) = edits.album {
        tag.remove_key(&ItemKey::AlbumTitle);
        if !v.is_empty() {
            tag.set_album(v.clone());
        }
    }

    if let Some(ref v) = edits.genre {
        tag.remove_key(&ItemKey::Genre);
        if !v.is_empty() {
            tag.set_genre(v.clone());
        }
    }

    if let Some(ref v) = edits.year {
        tag.remove_key(&ItemKey::Year);
        if !v.is_empty() {
            if let Ok(y) = v.parse::<u32>() {
                tag.set_year(y);
            }
        }
    }

    if let Some(ref v) = edits.track_number {
        tag.remove_key(&ItemKey::TrackNumber);
        if !v.is_empty() {
            if let Ok(n) = v.parse::<u32>() {
                tag.set_track(n);
            }
        }
    }

    if let Some(ref v) = edits.disc_number {
        tag.remove_key(&ItemKey::DiscNumber);
        if !v.is_empty() {
            if let Ok(n) = v.parse::<u32>() {
                tag.set_disk(n);
            }
        }
    }

    if let Some(ref v) = edits.comment {
        tag.remove_key(&ItemKey::Comment);
        if !v.is_empty() {
            tag.set_comment(v.clone());
        }
    }

    if let Some(stars) = edits.rating {
        remove_rating_keys(tag);
        if stars > 0 {
            push_rating(tag, stars);
        }
    }

    tag.save_to_path(path, lofty::config::WriteOptions::default())
        .map_err(|e| format!("failed to write tags: {e}"))?;

    Ok(())
}