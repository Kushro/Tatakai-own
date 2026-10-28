//! Anime4K 4.0 as a libplacebo custom shader. The 14 pass files (`*.glsl`, MIT / Unlicense) are NOT embedded: the user
//! downloads them with `packaging/fetch-deps.ps1` into a directory the app passes in `LinkConfig::anime4k_dir`. For each
//! mode/variant the passes are concatenated into one `.hook` written to the FFmpeg working directory and referenced
//! RELATIVELY (no Windows path in the filter string).
//!
//! The pass chains per mode follow Enigmata-Temple `media/anime4k.rs` (AGPL-3.0, same author), which follows the
//! upstream Anime4K guide for mpv.

use std::fs;
use std::io;
use std::path::Path;

use sha2::{Digest, Sha256};

pub const ANIME4K_VERSION: &str = "4.0";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Anime4kMode {
    A,
    B,
    C,
}

impl Anime4kMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::A => "a",
            Self::B => "b",
            Self::C => "c",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Anime4kVariant {
    S,
    M,
    L,
}

impl Anime4kVariant {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::S => "s",
            Self::M => "m",
            Self::L => "l",
        }
    }
}

/// `anime4k-v4.0-a-m.hook`: ASCII, no characters that need escaping inside an FFmpeg filter string.
pub fn shader_file_name(mode: Anime4kMode, variant: Anime4kVariant) -> String {
    format!("anime4k-v{ANIME4K_VERSION}-{}-{}.hook", mode.as_str(), variant.as_str())
}

fn passes(mode: Anime4kMode, q: Anime4kVariant) -> Vec<String> {
    let v = q.as_str().to_ascii_uppercase();
    let clamp = "Anime4K_Clamp_Highlights".to_owned();
    let down = "Anime4K_AutoDownscalePre_x2".to_owned();
    let up = format!("Anime4K_Upscale_CNN_x2_{v}");
    match mode {
        Anime4kMode::A => vec![clamp, format!("Anime4K_Restore_CNN_{v}"), up.clone(), down, up],
        Anime4kMode::B => vec![clamp, format!("Anime4K_Restore_CNN_Soft_{v}"), up.clone(), down, up],
        Anime4kMode::C => vec![clamp, format!("Anime4K_Upscale_Denoise_CNN_x2_{v}"), down, up],
    }
}

/// All 14 pass names (without extension) the shaders directory must contain.
pub fn required_files() -> Vec<String> {
    let mut v = vec!["Anime4K_Clamp_Highlights".to_owned(), "Anime4K_AutoDownscalePre_x2".to_owned()];
    for q in ["S", "M", "L"] {
        v.push(format!("Anime4K_Restore_CNN_{q}"));
        v.push(format!("Anime4K_Restore_CNN_Soft_{q}"));
        v.push(format!("Anime4K_Upscale_CNN_x2_{q}"));
        v.push(format!("Anime4K_Upscale_Denoise_CNN_x2_{q}"));
    }
    v
}

/// True when every pass file exists in `dir`.
pub fn shaders_present(dir: &Path) -> bool {
    required_files().iter().all(|n| dir.join(format!("{n}.glsl")).is_file())
}

/// Verify the directory against `anime4k.lock.json` (`files[].path`, `size`, `sha256`).
pub fn verify_against_lock(dir: &Path, lock_json: &str) -> Result<(), String> {
    let lock: serde_json::Value = serde_json::from_str(lock_json).map_err(|e| format!("lock is not JSON: {e}"))?;
    let files = lock["files"].as_array().ok_or("lock has no files")?;
    for f in files {
        let name = f["path"].as_str().ok_or("lock entry without path")?;
        if name.contains(['/', '\\']) || name.contains("..") {
            return Err(format!("lock entry '{name}' escapes the shaders directory"));
        }
        let bytes = fs::read(dir.join(name)).map_err(|_| format!("missing shader {name}"))?;
        let want = f["sha256"].as_str().ok_or("lock entry without sha256")?;
        if hex::encode(Sha256::digest(&bytes)) != want {
            return Err(format!("shader {name} does not match the lock"));
        }
    }
    Ok(())
}

/// The `.hook` text of a mode/variant: passes in order, LF line endings, separated by a blank line.
pub fn hook_source(dir: &Path, mode: Anime4kMode, variant: Anime4kVariant) -> io::Result<String> {
    let mut hook = String::new();
    for p in passes(mode, variant) {
        hook.push_str(&fs::read_to_string(dir.join(format!("{p}.glsl")))?.replace("\r\n", "\n"));
        if !hook.ends_with('\n') {
            hook.push('\n');
        }
        hook.push('\n');
    }
    Ok(hook)
}

/// Write the `.hook` into `workdir` (the FFmpeg cwd) if absent or different; returns its relative name.
pub fn write_shader(shaders: &Path, workdir: &Path, mode: Anime4kMode, variant: Anime4kVariant) -> io::Result<String> {
    let name = shader_file_name(mode, variant);
    let path = workdir.join(&name);
    let hook = hook_source(shaders, mode, variant)?;
    if fs::metadata(&path).ok().map(|m| m.len()) != Some(hook.len() as u64) {
        fs::write(&path, hook)?;
    }
    Ok(name)
}

#[cfg(test)]
pub(crate) fn write_fake_shaders(dir: &Path) {
    for n in required_files() {
        fs::write(dir.join(format!("{n}.glsl")), format!("//!DESC {n}\r\n//!HOOK MAIN\r\nvec4 hook() {{ return HOOKED_tex(HOOKED_pos); }}\r\n")).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_filter_safe() {
        for m in [Anime4kMode::A, Anime4kMode::B, Anime4kMode::C] {
            for v in [Anime4kVariant::S, Anime4kVariant::M, Anime4kVariant::L] {
                let n = shader_file_name(m, v);
                assert!(n.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.')), "{n}");
            }
        }
        assert_eq!(shader_file_name(Anime4kMode::A, Anime4kVariant::M), "anime4k-v4.0-a-m.hook");
        assert_eq!(required_files().len(), 14);
    }

    #[test]
    fn composes_each_mode_from_its_passes_and_writes_once() {
        let shaders = tempfile::tempdir().unwrap();
        let work = tempfile::tempdir().unwrap();
        assert!(!shaders_present(shaders.path()));
        write_fake_shaders(shaders.path());
        assert!(shaders_present(shaders.path()));
        let a = hook_source(shaders.path(), Anime4kMode::A, Anime4kVariant::M).unwrap();
        let b = hook_source(shaders.path(), Anime4kMode::B, Anime4kVariant::M).unwrap();
        let c = hook_source(shaders.path(), Anime4kMode::C, Anime4kVariant::S).unwrap();
        assert!(!a.contains('\r'));
        assert!(a.contains("Anime4K_Restore_CNN_M\n") && !a.contains("Restore_CNN_Soft"));
        assert!(b.contains("Anime4K_Restore_CNN_Soft_M"));
        assert!(c.contains("Anime4K_Upscale_Denoise_CNN_x2_S") && !c.contains("Restore_CNN_S"));
        assert_eq!(a.matches("Anime4K_Upscale_CNN_x2_M").count(), 2, "mode A upscales twice around AutoDownscale");
        assert_eq!(a.matches("Anime4K_AutoDownscalePre_x2").count(), 1);
        let name = write_shader(shaders.path(), work.path(), Anime4kMode::A, Anime4kVariant::M).unwrap();
        assert_eq!(fs::read_to_string(work.path().join(&name)).unwrap(), a);
        write_shader(shaders.path(), work.path(), Anime4kMode::A, Anime4kVariant::M).unwrap();
        assert_eq!(fs::read_dir(work.path()).unwrap().count(), 1);
    }

    #[test]
    fn lock_verification_detects_tampering_and_traversal() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.glsl"), b"abc").unwrap();
        let good = format!(r#"{{"files":[{{"path":"a.glsl","sha256":"{}"}}]}}"#, hex::encode(Sha256::digest(b"abc")));
        verify_against_lock(dir.path(), &good).unwrap();
        fs::write(dir.path().join("a.glsl"), b"abd").unwrap();
        assert!(verify_against_lock(dir.path(), &good).unwrap_err().contains("does not match"));
        assert!(verify_against_lock(dir.path(), r#"{"files":[{"path":"../x","sha256":"00"}]}"#).unwrap_err().contains("escapes"));
        assert!(verify_against_lock(dir.path(), r#"{"files":[{"path":"missing.glsl","sha256":"00"}]}"#).is_err());
    }
}
