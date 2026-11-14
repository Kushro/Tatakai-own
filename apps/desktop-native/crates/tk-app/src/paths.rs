//! Where things live. Data: `%APPDATA%\Tatakai-Next\` (06 #6). Resources (fonts, shaders, ffmpeg) sit next
//! to the executable in an installed build, or in the source tree during development.

use std::path::{Path, PathBuf};

pub const APP_DIR_NAME: &str = "Tatakai-Next";
/// Override for development and tests (never used by the installer).
pub const DATA_DIR_ENV: &str = "TATAKAI_NEXT_DATA_DIR";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub root: PathBuf,
}

impl AppPaths {
    pub fn from_root(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// `%APPDATA%\Tatakai-Next` (Roaming), or the `TATAKAI_NEXT_DATA_DIR` override.
    pub fn resolve() -> anyhow::Result<Self> {
        if let Some(dir) = std::env::var_os(DATA_DIR_ENV).filter(|v| !v.is_empty()) {
            return Ok(Self::from_root(dir));
        }
        let base = directories::BaseDirs::new().ok_or_else(|| anyhow::anyhow!("cannot determine the user data directory"))?;
        Ok(Self::from_root(base.config_dir().join(APP_DIR_NAME)))
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }
    pub fn webview_dir(&self) -> PathBuf {
        self.root.join("webview")
    }
    #[cfg_attr(not(feature = "live"), allow(dead_code))]
    pub fn db_file(&self) -> PathBuf {
        self.root.join("data.db")
    }
    pub fn extensions_dir(&self) -> PathBuf {
        self.root.join("extensions")
    }
    pub fn link_dir(&self) -> PathBuf {
        self.root.join("link")
    }
    pub fn ui_settings_file(&self) -> PathBuf {
        self.root.join("ui-settings.json")
    }
    /// Torrent engine state: librqbit session persistence + fastresume, the engine's labels (wave 3).
    #[cfg_attr(not(feature = "live"), allow(dead_code))]
    pub fn torrent_dir(&self) -> PathBuf {
        self.root.join("torrents")
    }
    /// `TorrentSettingsVm` (torrent client settings), kept apart from `ui-settings.json`.
    #[cfg_attr(not(feature = "live"), allow(dead_code))]
    pub fn torrent_settings_file(&self) -> PathBuf {
        self.root.join("torrent-settings.json")
    }
    /// Default torrent download folder: `<user Downloads>/Tatakai`, else `<data>/torrents/downloads`.
    #[cfg_attr(not(feature = "live"), allow(dead_code))]
    pub fn default_torrent_download_dir(&self) -> PathBuf {
        directories::UserDirs::new()
            .and_then(|u| u.download_dir().map(|d| d.join("Tatakai")))
            .unwrap_or_else(|| self.torrent_dir().join("downloads"))
    }

    /// Create the data directories (idempotent).
    pub fn ensure(&self) -> std::io::Result<()> {
        for d in [self.root.clone(), self.logs_dir(), self.webview_dir(), self.extensions_dir(), self.link_dir()] {
            std::fs::create_dir_all(d)?;
        }
        Ok(())
    }
}

/// First existing candidate, else the first one (so the error message names the preferred location).
pub fn first_existing(candidates: &[PathBuf]) -> PathBuf {
    candidates.iter().find(|p| p.exists()).or_else(|| candidates.first()).cloned().unwrap_or_default()
}

fn exe_dir() -> PathBuf {
    std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_default()
}

/// Bundled `.woff2` fonts: `<exe>/assets/fonts`, or the tk-ui source tree in development.
pub fn fonts_dir(exe_dir_override: Option<&Path>) -> PathBuf {
    let exe = exe_dir_override.map(Path::to_path_buf).unwrap_or_else(exe_dir);
    first_existing(&[
        exe.join("assets").join("fonts"),
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../tk-ui/assets/fonts"),
    ])
}

/// Anime4K / FSR shader files, in a directory the app can write (`shaders::fetch` downloads the missing presets'
/// files there on first use). `<exe>/shaders` when the installer shipped them there and it is writable (development
/// build, portable copy); otherwise `<data>/shaders`, seeded with the shipped files. BUG-003: an AppImage (squashfs),
/// `/usr/lib/tatakai-next` or `Program Files` is read-only, so the HQ / Fidelity / ArtCNN / NVScaler / CAS files could
/// never be installed next to the executable, and `<data>/shaders` was never looked at while `<exe>/shaders` existed.
pub fn shaders_dir(paths: &AppPaths, exe_dir_override: Option<&Path>) -> PathBuf {
    let bundled = exe_dir_override.map(Path::to_path_buf).unwrap_or_else(exe_dir).join("shaders");
    if bundled.is_dir() && dir_writable(&bundled) {
        return bundled;
    }
    let user = paths.root.join("shaders");
    if bundled.is_dir() {
        seed_dir(&bundled, &user);
    }
    user
}

/// Whether a file can be created in `dir` (a read-only mount fails here, not on a permission bit check).
fn dir_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".tk-write-probe-{}", std::process::id()));
    match std::fs::OpenOptions::new().write(true).create_new(true).open(&probe) {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// Copies the regular files of `from` that `to` lacks (shader `.glsl` files and their lock). Best effort: a file
/// that fails is fetched later like any missing one.
fn seed_dir(from: &Path, to: &Path) {
    let Ok(entries) = std::fs::read_dir(from) else { return };
    if std::fs::create_dir_all(to).is_err() {
        return;
    }
    for e in entries.flatten() {
        let src = e.path();
        let Some(name) = src.file_name() else { continue };
        let dest = to.join(name);
        if src.is_file() && !dest.exists() && std::fs::copy(&src, &dest).is_err() {
            tracing::warn!(file = %dest.display(), "seeding a shipped shader failed");
        }
    }
}

/// `anime4k.lock.json` shipped next to the shaders (installer) or in the source tree (development).
#[cfg_attr(not(feature = "live"), allow(dead_code))]
pub fn anime4k_lock(exe_dir_override: Option<&Path>) -> Option<PathBuf> {
    let exe = exe_dir_override.map(Path::to_path_buf).unwrap_or_else(exe_dir);
    [
        exe.join("shaders").join("anime4k.lock.json"),
        exe.join("anime4k.lock.json"),
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/anime4k.lock.json"),
    ]
    .into_iter()
    .find(|p| p.is_file())
}

#[cfg_attr(not(feature = "live"), allow(dead_code))]
/// `<exe>/ffmpeg/bin/ffmpeg.exe` if `fetch-deps.ps1` installed it.
pub fn ffmpeg_path(exe_dir_override: Option<&Path>) -> Option<PathBuf> {
    let exe = exe_dir_override.map(Path::to_path_buf).unwrap_or_else(exe_dir);
    let p = exe.join("ffmpeg").join("bin").join(if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" });
    p.is_file().then_some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_under_the_root() {
        let p = AppPaths::from_root("X:/data/Tatakai-Next");
        assert_eq!(p.logs_dir(), Path::new("X:/data/Tatakai-Next/logs"));
        assert_eq!(p.db_file(), Path::new("X:/data/Tatakai-Next/data.db"));
        assert_eq!(p.link_dir(), Path::new("X:/data/Tatakai-Next/link"));
    }

    #[test]
    fn ensure_creates_the_tree_inside_a_temp_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let p = AppPaths::from_root(tmp.path().join("Tatakai-Next"));
        p.ensure().unwrap();
        p.ensure().unwrap();
        for d in [p.logs_dir(), p.webview_dir(), p.extensions_dir(), p.link_dir()] {
            assert!(d.is_dir(), "{d:?}");
        }
    }

    #[test]
    fn first_existing_prefers_what_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let (a, b) = (tmp.path().join("a"), tmp.path().join("b"));
        std::fs::create_dir(&b).unwrap();
        assert_eq!(first_existing(&[a.clone(), b.clone()]), b);
        assert_eq!(first_existing(&[a.clone(), tmp.path().join("c")]), a);
    }

    #[test]
    fn shaders_dir_is_writable_and_seeded_from_a_read_only_install() {
        // BUG-003: a writable `<exe>/shaders` (development) is used as is.
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(tmp.path().join("data"));
        let exe = tmp.path().join("exe");
        std::fs::create_dir_all(exe.join("shaders")).unwrap();
        std::fs::write(exe.join("shaders").join("Anime4K_Clamp_Highlights.glsl"), "x").unwrap();
        assert_eq!(shaders_dir(&paths, Some(&exe)), exe.join("shaders"));
        // A read-only one (AppImage, /usr/lib, Program Files) is copied into `<data>/shaders`, which is returned.
        let user = paths.root.join("shaders");
        std::fs::create_dir_all(&user).unwrap();
        std::fs::write(user.join("CAS.glsl"), "fetched").unwrap();
        seed_dir(&exe.join("shaders"), &user);
        assert_eq!(std::fs::read_to_string(user.join("Anime4K_Clamp_Highlights.glsl")).unwrap(), "x");
        assert_eq!(std::fs::read_to_string(user.join("CAS.glsl")).unwrap(), "fetched", "existing files are kept");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let ro = exe.join("shaders");
            std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();
            // root ignores the mode bits: only check what a normal user gets.
            if !dir_writable(&ro) {
                assert_eq!(shaders_dir(&paths, Some(&exe)), user);
            }
            std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }

    #[test]
    fn shaders_dir_falls_back_to_the_data_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(tmp.path().join("data"));
        std::fs::create_dir_all(paths.root.join("shaders")).unwrap();
        assert_eq!(shaders_dir(&paths, Some(&tmp.path().join("exe"))), paths.root.join("shaders"));
    }

    #[test]
    fn dev_fonts_dir_resolves_to_the_repo_copy() {
        let dir = fonts_dir(Some(Path::new("Z:/definitely/not/here")));
        assert!(dir.join("JetBrainsMono.woff2").is_file(), "{dir:?}");
    }
}
