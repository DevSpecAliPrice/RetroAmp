//! Embedded default skin — bootstrapped into the user's skins directory on
//! first launch so the app always has at least one skin available.

use std::path::{Path, PathBuf};

pub const SKIN_NAME: &str = "RetroAmp Default";

/// Bump this when the embedded skin assets change, so cached copies get refreshed.
const SKIN_VERSION: &str = "7";

/// Files embedded at compile time from `assets/default-skin/`.
const FILES: &[(&str, &[u8])] = &[
    ("main.bmp", include_bytes!("../../../assets/default-skin/main.bmp")),
    ("titlebar.bmp", include_bytes!("../../../assets/default-skin/titlebar.bmp")),
    ("cbuttons.bmp", include_bytes!("../../../assets/default-skin/cbuttons.bmp")),
    ("numbers.bmp", include_bytes!("../../../assets/default-skin/numbers.bmp")),
    ("nums_ex.bmp", include_bytes!("../../../assets/default-skin/nums_ex.bmp")),
    ("playpaus.bmp", include_bytes!("../../../assets/default-skin/playpaus.bmp")),
    ("posbar.bmp", include_bytes!("../../../assets/default-skin/posbar.bmp")),
    ("volume.bmp", include_bytes!("../../../assets/default-skin/volume.bmp")),
    ("balance.bmp", include_bytes!("../../../assets/default-skin/balance.bmp")),
    ("shufrep.bmp", include_bytes!("../../../assets/default-skin/shufrep.bmp")),
    ("monoster.bmp", include_bytes!("../../../assets/default-skin/monoster.bmp")),
    ("text.bmp", include_bytes!("../../../assets/default-skin/text.bmp")),
    ("eqmain.bmp", include_bytes!("../../../assets/default-skin/eqmain.bmp")),
    ("pledit.bmp", include_bytes!("../../../assets/default-skin/pledit.bmp")),
    ("viscolor.txt", include_bytes!("../../../assets/default-skin/viscolor.txt")),
    ("pledit.txt", include_bytes!("../../../assets/default-skin/pledit.txt")),
];

/// Return the path where the default skin lives inside the config skins dir.
pub fn default_skin_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|c| c.join("retroamp").join("skins").join(SKIN_NAME))
}

/// Ensure the default skin exists on disk and is up-to-date.
/// Called once at startup — cheap no-op if already current.
pub fn ensure_default_skin() {
    let Some(dir) = default_skin_dir() else {
        return;
    };

    let version_file = dir.join(".skin_version");

    // Check if the cached version matches the embedded version.
    let cached_version = std::fs::read_to_string(&version_file).unwrap_or_default();
    if cached_version.trim() == SKIN_VERSION && dir.join("main.bmp").exists() {
        return;
    }

    if let Err(e) = write_default_skin(&dir) {
        log::warn!("failed to bootstrap default skin: {e}");
        return;
    }

    // Write the version marker so we don't rewrite next time.
    let _ = std::fs::write(&version_file, SKIN_VERSION);
}

fn write_default_skin(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("create dir: {e}"))?;

    for (name, data) in FILES {
        std::fs::write(dir.join(name), data)
            .map_err(|e| format!("write {name}: {e}"))?;
    }

    log::info!("bootstrapped default skin at {}", dir.display());
    Ok(())
}
