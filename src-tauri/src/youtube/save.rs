//! Save (download) a YouTube track to the user's download directory.
//!
//! Two flows:
//! 1. `save_active(...)` — copies the currently-playing source's already-
//!    downloaded temp file. No re-download.
//! 2. `download_and_save(...)` — runs yt-dlp headless, then writes to disk.
//!
//! Both flows go through `process_for_save`, which:
//! - Tags m4a / opus / mp3 / flac / ogg directly with lofty.
//! - Remuxes webm/Matroska to .opus (no re-encode, no quality loss) via
//!   ffmpeg when available, then tags the .opus with lofty (Vorbis comments
//!   support embedded pictures).
//! - Falls back to copying the raw file + a sidecar `.jpg` when ffmpeg
//!   isn't on PATH.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use crate::audio::recorder::{get_download_dir, tag_file};
use crate::audio::source::TrackMetadata;
use crate::audio::youtube::{begin_save, ActiveDownload};

/// How long to wait for the streaming temp file to finish before falling back
/// to a full-speed yt-dlp download. YouTube throttles stream URLs to roughly
/// playback bitrate, so a partway-through track can take minutes to fully
/// download — yt-dlp can fetch the same file in a few seconds instead.
const TEMP_READY_GRACE: Duration = Duration::from_secs(5);

/// Sanitize a string for use as a filename (mirrors the recorder's rules).
fn sanitize_filename(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(200)
        .collect()
}

fn unique_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let base = dir.join(format!("{stem}.{ext}"));
    if !base.exists() {
        return base;
    }
    for i in 2..1000 {
        let candidate = dir.join(format!("{stem} ({i}).{ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    dir.join(format!("{stem}_{ts}.{ext}"))
}

fn build_stem(metadata: &TrackMetadata, fallback: &str) -> String {
    let title = metadata.title.as_deref().filter(|s| !s.is_empty());
    let artist = metadata.artist.as_deref().filter(|s| !s.is_empty());
    let stem = match (artist, title) {
        (Some(a), Some(t)) => format!("{a} - {t}"),
        (None, Some(t)) => t.to_string(),
        (Some(a), None) => a.to_string(),
        (None, None) => fallback.to_string(),
    };
    sanitize_filename(&stem)
}

/// Returns true if ffmpeg is available on PATH. Result is cached.
fn ffmpeg_available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        std::process::Command::new("ffmpeg")
            .arg("-version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    })
}

/// Lofty 0.22 supports these container/extension combos for tag writes.
fn lofty_supports_ext(ext: &str) -> bool {
    matches!(
        ext.to_ascii_lowercase().as_str(),
        "m4a" | "m4b" | "mp4" | "mp3" | "flac" | "ogg" | "opus" | "wav"
    )
}

fn write_sidecar_thumbnail(audio_path: &Path, cover_art: &[u8]) {
    let mime_ext = match cover_art.get(..4) {
        Some(b) if b.starts_with(&[0x89, b'P', b'N', b'G']) => "png",
        _ => "jpg", // YouTube thumbnails are JPEG by default.
    };
    let sidecar = audio_path.with_extension(mime_ext);
    if let Err(e) = std::fs::write(&sidecar, cover_art) {
        log::warn!("[yt-save] sidecar write failed for {}: {e}", sidecar.display());
    } else {
        log::info!("[yt-save] wrote sidecar thumbnail: {}", sidecar.display());
    }
}

/// Take a downloaded source file and produce the tagged final file in
/// `download_dir`. Handles webm → opus remux when ffmpeg is available.
fn process_for_save(
    src: &Path,
    src_ext: &str,
    download_dir: &Path,
    stem: &str,
    metadata: &TrackMetadata,
    cover_art: Option<&[u8]>,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(download_dir)
        .map_err(|e| format!("create download dir: {e}"))?;

    // Path 1: source format is taggable by lofty — simple copy + tag.
    if lofty_supports_ext(src_ext) {
        let final_path = unique_path(download_dir, stem, src_ext);
        std::fs::copy(src, &final_path)
            .map_err(|e| format!("copy: {e}"))?;
        if let Err(e) = tag_file(
            &final_path,
            metadata.title.as_deref(),
            metadata.artist.as_deref(),
            metadata.album.as_deref(),
            cover_art,
        ) {
            log::warn!("[yt-save] tagging failed for {}: {e}", final_path.display());
        }
        log::info!("[yt-save] saved {}", final_path.display());
        return Ok(final_path);
    }

    // Path 2: webm/Matroska — remux to .opus via ffmpeg if available.
    // Stream copy: same audio bytes, new container. No quality loss.
    if ffmpeg_available() {
        let final_path = unique_path(download_dir, stem, "opus");
        let output = std::process::Command::new("ffmpeg")
            .arg("-hide_banner")
            .arg("-loglevel").arg("error")
            .arg("-y")
            .arg("-i").arg(src)
            .arg("-vn") // drop any video/cover stream
            .arg("-c:a").arg("copy")
            .arg(&final_path)
            .output()
            .map_err(|e| format!("ffmpeg spawn: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            log::warn!("[yt-save] ffmpeg remux failed: {}", stderr.trim());
            // Fall through to the no-ffmpeg path.
        } else {
            if let Err(e) = tag_file(
                &final_path,
                metadata.title.as_deref(),
                metadata.artist.as_deref(),
                metadata.album.as_deref(),
                cover_art,
            ) {
                log::warn!("[yt-save] tagging failed for {}: {e}", final_path.display());
            }
            log::info!("[yt-save] saved (remuxed to opus) {}", final_path.display());
            return Ok(final_path);
        }
    }

    // Path 3: no ffmpeg (or remux failed). Save raw + sidecar JPG for cover.
    let final_path = unique_path(download_dir, stem, src_ext);
    std::fs::copy(src, &final_path)
        .map_err(|e| format!("copy: {e}"))?;
    if let Some(art) = cover_art {
        write_sidecar_thumbnail(&final_path, art);
    }
    log::info!(
        "[yt-save] saved (no embedded tags — install ffmpeg for opus remux) {}",
        final_path.display(),
    );
    Ok(final_path)
}

/// Save the currently-playing YouTube track to the user's download directory.
///
/// Fast path: when the streaming temp file is already (or almost) complete,
/// copy + tag it without re-downloading. Otherwise fall back to a full-speed
/// yt-dlp download, which is much faster than waiting for the throttled
/// stream URL to finish.
pub fn save_active(active: Arc<ActiveDownload>) -> Result<PathBuf, String> {
    let guard = begin_save(&active);

    let deadline = Instant::now() + TEMP_READY_GRACE;
    while !active.temp_ready() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(200));
    }

    if active.temp_ready() {
        let download_dir = get_download_dir();
        let stem = build_stem(&active.metadata, &format!("YouTube {}", active.video_id));
        return process_for_save(
            &active.temp_path,
            &active.ext_hint,
            &download_dir,
            &stem,
            &active.metadata,
            active.metadata.cover_art.as_deref(),
        );
    }

    // Stream still throttled. Drop the save guard before yt-dlp runs so the
    // streaming source's Drop can proceed normally if the user changes track.
    drop(guard);
    log::info!(
        "[yt-save] temp file not ready (streaming throttled) — using yt-dlp for full-speed download"
    );
    download_and_save(&active.video_id, &active.metadata, None)
}

/// Run yt-dlp to download the audio for `video_id` (no playback), then process
/// it for save. Re-fetches the cover art from `thumbnail_url` if `metadata`
/// doesn't already have it.
pub fn download_and_save(
    video_id: &str,
    metadata: &TrackMetadata,
    thumbnail_url: Option<&str>,
) -> Result<PathBuf, String> {
    let ytdlp = crate::youtube::ytdlp::ensure_available()
        .map_err(|e| format!("yt-dlp unavailable: {e}"))?;

    let temp_dir = std::env::temp_dir().join(format!(
        "retroamp_yt_dl_{}_{}",
        video_id,
        std::process::id(),
    ));
    std::fs::create_dir_all(&temp_dir)
        .map_err(|e| format!("create temp dir: {e}"))?;

    let url = format!("https://www.youtube.com/watch?v={video_id}");
    let output_template = temp_dir.join("%(id)s.%(ext)s");

    let output = std::process::Command::new(&ytdlp)
        .arg("--no-playlist")
        .arg("-f").arg("bestaudio")
        .arg("-o").arg(&output_template)
        .arg(&url)
        .output()
        .map_err(|e| format!("spawn yt-dlp: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let _ = std::fs::remove_dir_all(&temp_dir);
        return Err(format!("yt-dlp failed: {stderr}"));
    }

    let downloaded = std::fs::read_dir(&temp_dir)
        .map_err(|e| format!("read temp dir: {e}"))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.is_file());

    let downloaded = match downloaded {
        Some(p) => p,
        None => {
            let _ = std::fs::remove_dir_all(&temp_dir);
            return Err("yt-dlp produced no output file".into());
        }
    };

    let ext = downloaded
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("webm")
        .to_string();

    let cover_owned: Option<Vec<u8>> = match metadata.cover_art.clone() {
        Some(bytes) => Some(bytes),
        None => thumbnail_url.and_then(super::commands::download_thumbnail),
    };

    let download_dir = get_download_dir();
    let stem = build_stem(metadata, &format!("YouTube {video_id}"));

    let result = process_for_save(
        &downloaded,
        &ext,
        &download_dir,
        &stem,
        metadata,
        cover_owned.as_deref(),
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
    result
}
