//! yt-dlp binary manager — finds, downloads, and updates the yt-dlp binary.
//!
//! Strategy:
//!   1. Check for a managed binary in the app data dir (`~/.config/retroamp/bin/`)
//!   2. Check the system PATH
//!   3. If neither found, download the platform-appropriate standalone binary
//!      from GitHub releases
//!
//! On app startup, a background check compares the installed version tag against
//! the latest GitHub release. If a newer version exists, it downloads it silently.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Duration;

/// Cached path to the yt-dlp binary (resolved once, reused).
static YTDLP_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();

/// The directory inside the app data dir where we store managed binaries.
const BIN_DIR_NAME: &str = "bin";

/// GitHub API endpoint for the latest yt-dlp release.
const GITHUB_LATEST_URL: &str =
    "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest";

/// File that stores the version tag of the managed binary.
const VERSION_FILE: &str = "yt-dlp.version";

/// Get the path to a working yt-dlp binary.
///
/// Checks (in order): managed binary in app dir, system PATH.
/// Returns `None` if yt-dlp is not available anywhere.
/// Does NOT trigger a download — use `ensure_available()` for that.
pub fn find() -> Option<PathBuf> {
    // Check cached result first.
    if let Some(cached) = YTDLP_PATH.get() {
        return cached.clone();
    }

    let result = find_inner();
    let _ = YTDLP_PATH.set(result.clone());
    result
}

fn find_inner() -> Option<PathBuf> {
    // 1. Check managed binary in app data dir.
    if let Some(managed) = managed_binary_path() {
        if managed.exists() {
            log::info!("[ytdlp] using managed binary: {}", managed.display());
            return Some(managed);
        }
    }

    // 2. Check system PATH.
    if let Ok(output) = std::process::Command::new("yt-dlp")
        .arg("--version")
        .output()
    {
        if output.status.success() {
            let version = String::from_utf8_lossy(&output.stdout);
            log::info!("[ytdlp] using system yt-dlp: {}", version.trim());
            return Some(PathBuf::from("yt-dlp"));
        }
    }

    None
}

/// Ensure yt-dlp is available. If not found, downloads it.
///
/// This may block for several seconds on first run (downloading ~20MB).
/// Returns the path to the binary, or an error.
pub fn ensure_available() -> Result<PathBuf, String> {
    if let Some(path) = find() {
        return Ok(path);
    }

    log::info!("[ytdlp] yt-dlp not found — downloading...");
    download_latest()?;

    // Clear the cached path so find() re-checks.
    // OnceLock can't be reset, so we check the managed path directly.
    let managed = managed_binary_path()
        .ok_or_else(|| "could not determine app data directory".to_string())?;
    if managed.exists() {
        Ok(managed)
    } else {
        Err("download completed but binary not found".into())
    }
}

/// Check for updates and download if a newer version is available.
///
/// Designed to be called from a background thread on app startup.
/// Fails silently (logs warnings) if GitHub is unreachable.
pub fn check_for_update() {
    let bin_dir = match app_bin_dir() {
        Some(d) => d,
        None => return,
    };

    let version_file = bin_dir.join(VERSION_FILE);
    let current_version = fs::read_to_string(&version_file).unwrap_or_default();
    let current_version = current_version.trim().to_string();

    // Query GitHub for the latest release tag.
    let latest_tag = match fetch_latest_version_tag() {
        Ok(tag) => tag,
        Err(e) => {
            log::debug!("[ytdlp] update check skipped: {e}");
            return;
        }
    };

    if !current_version.is_empty() && current_version == latest_tag {
        log::debug!("[ytdlp] up to date: {current_version}");
        return;
    }

    // Check if we even have a managed binary (vs system PATH).
    let managed = bin_dir.join(binary_filename());
    if !managed.exists() && find().is_some() {
        // Using system yt-dlp, no managed binary to update.
        log::debug!("[ytdlp] using system binary, skipping managed update");
        return;
    }

    if current_version.is_empty() {
        log::info!("[ytdlp] no managed binary — downloading {latest_tag}...");
    } else {
        log::info!("[ytdlp] updating from {current_version} to {latest_tag}...");
    }

    if let Err(e) = download_release(&latest_tag) {
        log::warn!("[ytdlp] update download failed: {e}");
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Get the app's binary directory (e.g. `~/.config/retroamp/bin/`).
fn app_bin_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("retroamp").join(BIN_DIR_NAME))
}

/// Platform-specific binary filename.
fn binary_filename() -> &'static str {
    if cfg!(target_os = "windows") {
        "yt-dlp.exe"
    } else {
        "yt-dlp"
    }
}

/// Full path to the managed binary.
fn managed_binary_path() -> Option<PathBuf> {
    app_bin_dir().map(|d| d.join(binary_filename()))
}

/// Platform-specific download asset name from GitHub releases.
fn release_asset_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "yt-dlp.exe"
    } else if cfg!(target_os = "macos") {
        "yt-dlp_macos"
    } else {
        "yt-dlp_linux"
    }
}

/// Fetch the latest version tag from GitHub Releases API.
fn fetch_latest_version_tag() -> Result<String, String> {
    let response = ureq::get(GITHUB_LATEST_URL)
        .header("User-Agent", "RetroAmp/0.1")
        .header("Accept", "application/vnd.github+json")
        .config()
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_recv_response(Some(Duration::from_secs(10)))
        .build()
        .call()
        .map_err(|e| format!("GitHub API request failed: {e}"))?;

    let body = response
        .into_body()
        .read_to_string()
        .map_err(|e| format!("Failed to read GitHub API response: {e}"))?;

    let json: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| format!("Failed to parse GitHub JSON: {e}"))?;

    json.get("tag_name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "tag_name not found in GitHub response".to_string())
}

/// Download the latest yt-dlp release.
fn download_latest() -> Result<(), String> {
    let tag = fetch_latest_version_tag()?;
    download_release(&tag)
}

/// Download a specific yt-dlp release by tag.
fn download_release(tag: &str) -> Result<(), String> {
    let bin_dir = app_bin_dir()
        .ok_or_else(|| "could not determine app data directory".to_string())?;
    fs::create_dir_all(&bin_dir)
        .map_err(|e| format!("failed to create bin directory: {e}"))?;

    let asset = release_asset_name();
    let download_url = format!(
        "https://github.com/yt-dlp/yt-dlp/releases/download/{tag}/{asset}"
    );

    log::info!("[ytdlp] downloading from {download_url}");

    let response = ureq::get(&download_url)
        .header("User-Agent", "RetroAmp/0.1")
        .config()
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_body(None)
        .build()
        .call()
        .map_err(|e| format!("download failed: {e}"))?;

    // Write to a temp file first, then rename (atomic-ish).
    let dest = bin_dir.join(binary_filename());
    let tmp = bin_dir.join(format!("{}.tmp", binary_filename()));

    {
        let mut file = fs::File::create(&tmp)
            .map_err(|e| format!("failed to create temp file: {e}"))?;

        let mut reader = response.into_body().into_reader();
        let mut buf = [0u8; 65536];
        loop {
            let n = reader.read(&mut buf)
                .map_err(|e| format!("download read error: {e}"))?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n])
                .map_err(|e| format!("file write error: {e}"))?;
        }
    }

    // Set executable permission on Unix.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("failed to set executable permission: {e}"))?;
    }

    // Rename temp to final.
    fs::rename(&tmp, &dest)
        .map_err(|e| format!("failed to rename binary: {e}"))?;

    // Write version tag.
    let version_file = bin_dir.join(VERSION_FILE);
    let _ = fs::write(&version_file, tag);

    let size_mb = fs::metadata(&dest).map(|m| m.len()).unwrap_or(0) as f64 / 1_048_576.0;
    log::info!("[ytdlp] installed {tag} ({size_mb:.1}MB) at {}", dest.display());

    Ok(())
}

use std::io::Read;
