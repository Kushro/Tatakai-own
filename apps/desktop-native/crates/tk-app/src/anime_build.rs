//! The vendored mpv-anime-build release (docs/redesign/09 section 3.3, owner B): find the pinned zip, verify its
//! SHA-256 against `vendor/mpv-anime-build/SHA256SUMS` (compiled in, so a swapped file on disk is rejected) and
//! extract it into `<data>/mpv-anime-build/<VERSION>/` (marker file [`MARKER`] written last, holding the digest).
//! The extracted directory is the `config_dir` of `tk_player::EngineMode::Enhanced`.
//!
//! Zip lookup ([`zip_path`], first hit wins):
//! 1. `$TATAKAI_ANIME_BUILD_ZIP` ([`ZIP_ENV`], a file path; development and packaging tests).
//! 2. `<exe dir>/mpv-anime-build/<ZIP_NAME>`: the shipped location. Installers / AppImage / PKGBUILD put it there:
//!    the Linux packages install the app under `/usr/lib/tatakai-next/` with a `/usr/bin` symlink, and
//!    `current_exe` resolves it; the Windows installer next to `tatakai-next.exe`.
//! 3. `<exe dir>/../share/tatakai-next/mpv-anime-build/<ZIP_NAME>` (FHS layout, if a distro package prefers it).
//! 4. The source tree `apps/desktop-native/vendor/mpv-anime-build/` (development, `cargo run`).
//!
//! The same bytes are hashed and extracted (read once into memory), so the file cannot change in between. Nothing in
//! the release is ever executed by the app; mpv loads the Lua scripts itself, and uosc's `ziggy` helpers keep the
//! executable bit the archive gives them. After extraction `mpv.conf` is replaced by its sanitized form
//! (`tk_player::enhanced_conf::sanitize_mpv_conf`); the original stays as `mpv.conf.upstream`.

#![cfg_attr(not(feature = "live"), allow(dead_code))]

use std::io::Cursor;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use sha2::{Digest, Sha256};
use tk_player::enhanced_conf::{SANITIZE_REV, UPSTREAM_CONF, sanitize_mpv_conf};
use tk_ui::backend::AnimeBuildVm;
use tk_ui::model::{ErrorKind, UiError};

use crate::paths::AppPaths;

pub const VERSION: &str = "5.6";
pub const ZIP_NAME: &str = "mpv-anime-build-v5.6.zip";
/// Written inside the extracted directory once extraction finished; contains the zip's hex SHA-256.
pub const MARKER: &str = ".tatakai-extracted";
/// `<hex>  <file>` lines, compiled in.
pub const SHA256SUMS: &str = include_str!("../../../vendor/mpv-anime-build/SHA256SUMS");
/// Path of the zip to use instead of the shipped one (development and packaging tests).
pub const ZIP_ENV: &str = "TATAKAI_ANIME_BUILD_ZIP";

/// Serializes `prepare` (Settings and the player may both ask on first use).
static PREPARE: Mutex<()> = Mutex::new(());

/// The pinned zip shipped with this installation, if any.
pub fn zip_path(exe_dir_override: Option<&Path>) -> Option<PathBuf> {
    let env = exe_dir_override
        .is_none()
        .then(|| std::env::var_os(ZIP_ENV).filter(|v| !v.is_empty()).map(PathBuf::from))
        .flatten();
    let exe = exe_dir_override
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)))
        .unwrap_or_default();
    env.into_iter()
        .chain([
            exe.join("mpv-anime-build").join(ZIP_NAME),
            exe.join("..").join("share").join("tatakai-next").join("mpv-anime-build").join(ZIP_NAME),
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/mpv-anime-build").join(ZIP_NAME),
        ])
        .find(|p| p.is_file())
}

/// `<data>/mpv-anime-build/<VERSION>`.
pub fn config_dir(paths: &AppPaths) -> PathBuf {
    paths.root.join("mpv-anime-build").join(VERSION)
}

/// The pinned hex digest of [`ZIP_NAME`] in [`SHA256SUMS`] (`sha256sum` format, text or `*binary` mode).
pub fn expected_sha256() -> Option<String> {
    sums_entry(SHA256SUMS, ZIP_NAME)
}

fn sums_entry(sums: &str, file: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let hex = parts.next()?;
        let name = parts.next()?.trim_start_matches('*');
        (name == file && hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| hex.to_ascii_lowercase())
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Marker body: the digest and the sanitizer revision (a newer app re-extracts when either changed).
fn marker_body(digest: &str) -> String {
    format!("{digest}\nrev={SANITIZE_REV}\n")
}

fn is_current(dir: &Path, digest: &str) -> bool {
    std::fs::read_to_string(dir.join(MARKER)).is_ok_and(|m| m == marker_body(digest))
}

/// Cheap status for Settings (no hashing).
pub fn status(paths: &AppPaths) -> AnimeBuildVm {
    let available = zip_path(None).is_some();
    let dir = config_dir(paths);
    let extracted = expected_sha256().is_some_and(|d| is_current(&dir, &d));
    AnimeBuildVm {
        available,
        version: if available { VERSION.to_owned() } else { String::new() },
        extracted,
        path: extracted.then(|| dir.display().to_string()),
    }
}

/// Verify + extract if needed; returns the config directory. Blocking: run it in `spawn_blocking`.
pub fn prepare(paths: &AppPaths) -> Result<PathBuf, UiError> {
    let _guard = PREPARE.lock().unwrap_or_else(|e| e.into_inner());
    let dir = config_dir(paths);
    let expected = expected_sha256()
        .ok_or_else(|| UiError::new(ErrorKind::Other, format!("SHA256SUMS has no valid entry for {ZIP_NAME}")))?;
    if is_current(&dir, &expected) {
        return Ok(dir);
    }
    let zip = zip_path(None).ok_or_else(|| {
        UiError::new(
            ErrorKind::Other,
            format!("{ZIP_NAME} is not installed (expected in <app folder>/mpv-anime-build/)"),
        )
    })?;
    prepare_with(&dir, &zip, &expected)?;
    Ok(dir)
}

/// Verify `zip` against `expected` and extract it into `dir` unless `dir` already holds that exact release.
fn prepare_with(dir: &Path, zip: &Path, expected: &str) -> Result<(), UiError> {
    if is_current(dir, expected) {
        return Ok(());
    }
    let bytes = std::fs::read(zip).map_err(|e| other(&format!("reading {}", zip.display()), e))?;
    let digest = sha256_hex(&bytes);
    if digest != expected {
        return Err(UiError::new(
            ErrorKind::Other,
            format!("{} does not match SHA256SUMS (got {digest}, expected {expected})", zip.display()),
        ));
    }
    std::fs::create_dir_all(dir).map_err(|e| other(&format!("creating {}", dir.display()), e))?;
    // A stale marker (older release or sanitizer) must not survive a half-finished re-extraction.
    let marker = dir.join(MARKER);
    if marker.exists() {
        std::fs::remove_file(&marker).map_err(|e| other("removing the old marker", e))?;
    }
    let files = extract(&bytes, dir)?;
    sanitize_conf(dir).map_err(|e| other("preparing mpv.conf", e))?;
    let tmp = dir.join(format!("{MARKER}.tmp"));
    std::fs::write(&tmp, marker_body(&digest))
        .and_then(|()| std::fs::rename(&tmp, &marker))
        .map_err(|e| other("writing the marker", e))?;
    tracing::info!(dir = %dir.display(), files, "mpv-anime-build extracted");
    Ok(())
}

fn other(what: &str, e: impl std::fmt::Display) -> UiError {
    UiError::new(ErrorKind::Other, format!("{what}: {e}"))
}

fn bad(e: impl std::fmt::Display) -> UiError {
    UiError::new(ErrorKind::Other, format!("mpv-anime-build archive: {e}"))
}

/// The entry's path relative to the target, or `None` if it is absolute, climbs out (`..`) or is otherwise odd.
fn safe_relative(name: &str) -> Option<PathBuf> {
    if name.contains('\\') {
        // The release is built with `/` separators; a backslash would mean different paths per OS.
        return None;
    }
    let rel = PathBuf::from(name);
    let mut out = PathBuf::new();
    for c in rel.components() {
        match c {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

/// Extract every regular file and directory of the archive under `dir` (overwriting); returns the file count.
/// Rejects the whole archive on any unsafe path; symlinks are skipped.
fn extract(bytes: &[u8], dir: &Path) -> Result<usize, UiError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(bad)?;
    let mut files = 0;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(bad)?;
        let name = entry.name().to_owned();
        let rel = safe_relative(&name)
            .filter(|_| entry.enclosed_name().is_some())
            .ok_or_else(|| bad(format!("unsafe path {name:?}")))?;
        if entry.is_symlink() {
            tracing::warn!(entry = %name, "mpv-anime-build: skipping a symlink entry");
            continue;
        }
        let out = dir.join(&rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out).map_err(bad)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(bad)?;
        }
        let mut file = std::fs::File::create(&out).map_err(|e| bad(format!("{}: {e}", out.display())))?;
        std::io::copy(&mut entry, &mut file).map_err(|e| bad(format!("{name}: {e}")))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // Keep only the executable bit the archive gives (uosc's ziggy helpers); never setuid / world-writable.
            let exec = entry.unix_mode().is_some_and(|m| m & 0o111 != 0);
            let mode = if exec { 0o755 } else { 0o644 };
            std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode)).map_err(bad)?;
        }
        files += 1;
    }
    Ok(files)
}

/// Keep the build's `mpv.conf` as `mpv.conf.upstream` and write the sanitized one in its place.
fn sanitize_conf(dir: &Path) -> std::io::Result<()> {
    let conf = dir.join("mpv.conf");
    let Ok(upstream) = std::fs::read_to_string(&conf) else {
        tracing::warn!(dir = %dir.display(), "mpv-anime-build: no mpv.conf in the release");
        return Ok(());
    };
    std::fs::write(dir.join(UPSTREAM_CONF), &upstream)?;
    std::fs::write(&conf, sanitize_mpv_conf(&upstream))
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn make_zip(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, body) in entries {
            w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(body.as_bytes()).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    #[test]
    fn sums_parse_the_pinned_entry() {
        let d = expected_sha256().expect("SHA256SUMS lists the zip");
        assert_eq!(d.len(), 64);
        let sums = "ABCDEF0123456789abcdef0123456789abcdef0123456789abcdef0123456789 *a.zip\nzz  b.zip\n";
        assert_eq!(
            sums_entry(sums, "a.zip").as_deref(),
            Some("abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789")
        );
        assert_eq!(sums_entry(sums, "b.zip"), None);
        assert_eq!(sums_entry(sums, "c.zip"), None);
    }

    #[test]
    fn the_vendored_zip_matches_its_pin() {
        let zip = zip_path(Some(Path::new("/nonexistent"))).expect("source tree zip");
        let bytes = std::fs::read(zip).unwrap();
        assert_eq!(Some(sha256_hex(&bytes)), expected_sha256());
    }

    #[test]
    fn unsafe_paths_are_rejected() {
        assert_eq!(safe_relative("scripts/uosc/main.lua"), Some(PathBuf::from("scripts/uosc/main.lua")));
        assert_eq!(safe_relative("./mpv.conf"), Some(PathBuf::from("mpv.conf")));
        for bad in ["../evil", "a/../../evil", "/etc/passwd", "a\\..\\b", "", "."] {
            assert_eq!(safe_relative(bad), None, "{bad}");
        }
        let tmp = tempfile::tempdir().unwrap();
        let bytes = make_zip(&[("ok.txt", "x"), ("../evil.txt", "x")]);
        assert!(extract(&bytes, tmp.path()).is_err());
        assert!(!tmp.path().parent().unwrap().join("evil.txt").exists());
    }

    #[test]
    fn prepare_verifies_extracts_sanitizes_and_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let bytes = make_zip(&[
            ("mpv.conf", "[default]\nvo=gpu-next\nsub-font=\"Hey Comic\"\n"),
            ("scripts/uosc/main.lua", "-- uosc"),
            ("script-opts/uosc.conf", "autohide=yes\n"),
        ]);
        let zip = tmp.path().join("release.zip");
        std::fs::write(&zip, &bytes).unwrap();
        let dir = tmp.path().join("data").join("mpv-anime-build").join(VERSION);

        let wrong = "0".repeat(64);
        assert!(prepare_with(&dir, &zip, &wrong).is_err());
        assert!(!dir.join(MARKER).exists());

        let digest = sha256_hex(&bytes);
        prepare_with(&dir, &zip, &digest).unwrap();
        assert!(dir.join("scripts/uosc/main.lua").is_file());
        assert_eq!(std::fs::read_to_string(dir.join(MARKER)).unwrap(), marker_body(&digest));
        let conf = std::fs::read_to_string(dir.join("mpv.conf")).unwrap();
        assert!(conf.contains("# tatakai (embedded): vo=gpu-next"));
        assert!(conf.contains("sub-font=\"Hey Comic\""));
        assert!(std::fs::read_to_string(dir.join(UPSTREAM_CONF)).unwrap().contains("\nvo=gpu-next\n"));

        // Already current: nothing is rewritten (user edits made by the build's own scripts survive).
        std::fs::write(dir.join("script-opts/uosc.conf"), "autohide=no\n").unwrap();
        prepare_with(&dir, &zip, &digest).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("script-opts/uosc.conf")).unwrap(), "autohide=no\n");

        // A stale marker (older sanitizer) triggers a fresh extraction.
        std::fs::write(dir.join(MARKER), "old").unwrap();
        prepare_with(&dir, &zip, &digest).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("script-opts/uosc.conf")).unwrap(), "autohide=yes\n");
    }

    #[test]
    fn status_reports_extraction_without_hashing() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(tmp.path());
        let s = status(&paths);
        assert!(!s.extracted);
        assert_eq!(s.path, None);
        let dir = config_dir(&paths);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(MARKER), marker_body(&expected_sha256().unwrap())).unwrap();
        let s = status(&paths);
        assert!(s.extracted);
        assert_eq!(s.path, Some(dir.display().to_string()));
    }
}
