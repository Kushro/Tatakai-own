//! The vendored mpv-anime-build release (docs/redesign/09 section 3.3, owner B): find the pinned zip, verify its
//! SHA-256 against `vendor/mpv-anime-build/SHA256SUMS` (compiled in, so a swapped file on disk is rejected) and
//! extract it into `<data>/mpv-anime-build/<VERSION>/` (marker file [`MARKER`] written last, holding the digest).
//! The extracted directory is the `config_dir` of `tk_player::EngineMode::Enhanced`.
//!
//! Zip lookup: `<exe>/mpv-anime-build/<ZIP_NAME>` (installers / AppImage / PKGBUILD put it there: the Linux
//! packages install the app under `/usr/lib/tatakai-next/` with a `/usr/bin` symlink, and `current_exe` resolves
//! it), else the source tree `apps/desktop-native/vendor/mpv-anime-build/` in development.

#![cfg_attr(not(feature = "live"), allow(dead_code))]

use std::path::{Path, PathBuf};

use tk_ui::backend::AnimeBuildVm;
use tk_ui::model::{ErrorKind, UiError};

use crate::paths::AppPaths;

pub const VERSION: &str = "5.6";
pub const ZIP_NAME: &str = "mpv-anime-build-v5.6.zip";
/// Written inside the extracted directory once extraction finished; contains the zip's hex SHA-256.
pub const MARKER: &str = ".tatakai-extracted";
/// `<hex>  <file>` lines, compiled in.
pub const SHA256SUMS: &str = include_str!("../../../vendor/mpv-anime-build/SHA256SUMS");

/// The pinned zip shipped with this installation, if any.
pub fn zip_path(exe_dir_override: Option<&Path>) -> Option<PathBuf> {
    let exe = exe_dir_override
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)))
        .unwrap_or_default();
    [
        exe.join("mpv-anime-build").join(ZIP_NAME),
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/mpv-anime-build").join(ZIP_NAME),
    ]
    .into_iter()
    .find(|p| p.is_file())
}

/// `<data>/mpv-anime-build/<VERSION>`.
pub fn config_dir(paths: &AppPaths) -> PathBuf {
    paths.root.join("mpv-anime-build").join(VERSION)
}

/// Cheap status for Settings (no hashing).
pub fn status(paths: &AppPaths) -> AnimeBuildVm {
    let available = zip_path(None).is_some();
    let dir = config_dir(paths);
    let extracted = dir.join(MARKER).is_file();
    AnimeBuildVm {
        available,
        version: if available { VERSION.to_owned() } else { String::new() },
        extracted,
        path: extracted.then(|| dir.display().to_string()),
    }
}

/// Verify + extract if needed; returns the config directory. Blocking: run it in `spawn_blocking`.
pub fn prepare(paths: &AppPaths) -> Result<PathBuf, UiError> {
    // Contract stub (owner B): verify SHA256SUMS, extract safely (no paths outside the target), write MARKER.
    let dir = config_dir(paths);
    if dir.join(MARKER).is_file() {
        return Ok(dir);
    }
    Err(UiError::new(ErrorKind::NotWired, "mpv-anime-build extraction is not implemented yet"))
}
