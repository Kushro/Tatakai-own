//! On-demand download of the Anime4K / FSR shader files the player presets use (`tk_player::ShaderPreset`).
//!
//! Installers ship them in `<exe>/shaders` (`packaging/fetch-deps.{ps1,sh}`); a development build or a fresh Linux
//! install has none, and the presets used to fail with "run fetch-deps.ps1". The first time a preset finds them
//! missing, the player asks `Backend::fetch_shaders`, which downloads every file of `packaging/anime4k.lock.json`
//! and `packaging/fsr.lock.json` into `<data>/shaders`, each verified against its pinned SHA-256 (an entry without a
//! hash is never installed). Files already present with the right hash are kept.

use std::path::Path;
use std::time::Duration;

use serde::Deserialize;
use sha2::{Digest, Sha256};
use tk_ui::model::{ErrorKind, UiError};

const LOCKS: [&str; 2] =
    [include_str!("../../../packaging/anime4k.lock.json"), include_str!("../../../packaging/fsr.lock.json")];

#[derive(Debug, Deserialize)]
struct Lock {
    files: Vec<LockFile>,
}

#[derive(Debug, Deserialize)]
struct LockFile {
    path: String,
    url: String,
    #[serde(default)]
    sha256: Option<String>,
}

fn entries() -> Vec<LockFile> {
    LOCKS
        .iter()
        .filter_map(|l| serde_json::from_str::<Lock>(l).ok())
        .flat_map(|l| l.files)
        .filter(|f| f.sha256.as_deref().is_some_and(|h| h.len() == 64) && is_plain_name(&f.path))
        .collect()
}

/// A bare file name (the lock is ours, but never write outside the shader folder).
fn is_plain_name(p: &str) -> bool {
    !p.is_empty() && !p.contains(['/', '\\']) && p != "." && p != ".."
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn valid_on_disk(path: &Path, sha: &str) -> bool {
    std::fs::read(path).map(|b| sha256_hex(&b).eq_ignore_ascii_case(sha)).unwrap_or(false)
}

/// Downloads the missing or corrupt shader files into `dir`; returns how many were written.
pub async fn fetch(dir: &Path) -> Result<u32, UiError> {
    std::fs::create_dir_all(dir).map_err(|e| UiError::new(ErrorKind::Other, format!("{}: {e}", dir.display())))?;
    let client = reqwest::Client::builder()
        .user_agent("TatakaiNext/0.1")
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| UiError::new(ErrorKind::Offline, e.to_string()))?;
    let mut written = 0;
    for f in entries() {
        let sha = f.sha256.as_deref().unwrap_or_default();
        let dest = dir.join(&f.path);
        if valid_on_disk(&dest, sha) {
            continue;
        }
        let bytes = client
            .get(&f.url)
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| UiError::new(ErrorKind::Offline, format!("{}: {e}", f.path)))?
            .bytes()
            .await
            .map_err(|e| UiError::new(ErrorKind::Offline, format!("{}: {e}", f.path)))?;
        if !sha256_hex(&bytes).eq_ignore_ascii_case(sha) {
            return Err(UiError::new(ErrorKind::Offline, format!("{}: SHA-256 does not match the pinned lock", f.path)));
        }
        let tmp = dest.with_extension("part");
        std::fs::write(&tmp, &bytes).map_err(|e| UiError::new(ErrorKind::Other, format!("{}: {e}", tmp.display())))?;
        std::fs::rename(&tmp, &dest).map_err(|e| UiError::new(ErrorKind::Other, format!("{}: {e}", dest.display())))?;
        written += 1;
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locks_list_every_preset_file_with_a_pinned_hash() {
        let e = entries();
        let names: Vec<&str> = e.iter().map(|f| f.path.as_str()).collect();
        for want in ["Anime4K_Upscale_CNN_x2_M.glsl", "Anime4K_Restore_CNN_M.glsl", "FSR.glsl"] {
            assert!(names.contains(&want), "{want} missing from the locks");
        }
        assert!(e.iter().all(|f| f.url.starts_with("https://")));
    }

    #[test]
    fn only_bare_file_names_are_written() {
        assert!(is_plain_name("FSR.glsl"));
        assert!(!is_plain_name("../x.glsl") && !is_plain_name("a/b.glsl") && !is_plain_name(""));
    }

    #[test]
    fn a_file_with_the_pinned_hash_is_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("x.glsl");
        std::fs::write(&p, b"abc").unwrap();
        assert!(valid_on_disk(&p, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"));
        assert!(!valid_on_disk(&p, &"0".repeat(64)));
    }
}
