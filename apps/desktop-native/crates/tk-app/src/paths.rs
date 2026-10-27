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

/// Anime4K / FSR shader files: `<exe>/shaders`, else `<data>/shaders`.
pub fn shaders_dir(paths: &AppPaths, exe_dir_override: Option<&Path>) -> PathBuf {
    let exe = exe_dir_override.map(Path::to_path_buf).unwrap_or_else(exe_dir);
    first_existing(&[exe.join("shaders"), paths.root.join("shaders")])
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
