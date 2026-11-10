//! On-demand install of the shader files the player presets use (`tk_player::ShaderPreset`: Anime4K fast / HQ,
//! FSR, Fidelity, ArtCNN, NVScaler, CAS).
//!
//! Installers ship some in `<exe>/shaders`; a development build or a fresh Linux install has none. The first time a
//! preset finds files missing, the player asks `Backend::fetch_shaders`, which fills `<data>/shaders`:
//! 1. from the pinned mpv-anime-build zip shipped with the app (`vendor/mpv-anime-build`, SHA-256 checked against
//!    its `SHA256SUMS` like `anime_build::prepare`): it carries every shader of every preset (`shaders/*.glsl`);
//! 2. what is still missing (no zip in this installation) is downloaded from `packaging/anime4k.lock.json` /
//!    `packaging/fsr.lock.json`, each file verified against its pinned SHA-256 (an entry without a hash is never
//!    installed). Files already present are kept.

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

/// Copies every preset shader missing from `dir` out of the verified mpv-anime-build zip; returns how many were
/// written (0 when this installation has no zip). Blocking: hashes ~56 MB.
fn install_from_anime_build(dir: &Path) -> Result<u32, UiError> {
    let wanted: Vec<String> = tk_player::all_preset_files().into_iter().filter(|f| !dir.join(f).is_file()).collect();
    if wanted.is_empty() {
        return Ok(0);
    }
    let (Some(zip), Some(expected)) = (crate::anime_build::zip_path(None), crate::anime_build::expected_sha256()) else {
        return Ok(0);
    };
    let bytes = std::fs::read(&zip).map_err(|e| UiError::new(ErrorKind::Other, format!("{}: {e}", zip.display())))?;
    if !sha256_hex(&bytes).eq_ignore_ascii_case(&expected) {
        return Err(UiError::new(ErrorKind::Other, format!("{}: SHA-256 does not match SHA256SUMS", zip.display())));
    }
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| UiError::new(ErrorKind::Other, e.to_string()))?;
    let mut written = 0;
    for name in wanted.iter().filter(|n| is_plain_name(n)) {
        let Ok(mut entry) = archive.by_name(&format!("shaders/{name}")) else { continue };
        let mut data = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut data).map_err(|e| UiError::new(ErrorKind::Other, e.to_string()))?;
        let dest = dir.join(name);
        let tmp = dest.with_extension("part");
        std::fs::write(&tmp, &data).map_err(|e| UiError::new(ErrorKind::Other, format!("{}: {e}", tmp.display())))?;
        std::fs::rename(&tmp, &dest).map_err(|e| UiError::new(ErrorKind::Other, format!("{}: {e}", dest.display())))?;
        written += 1;
    }
    Ok(written)
}

/// Installs the missing preset shaders into `dir` (anime-build zip first, then the pinned URLs); returns how many
/// files were written.
pub async fn fetch(dir: &Path) -> Result<u32, UiError> {
    std::fs::create_dir_all(dir).map_err(|e| UiError::new(ErrorKind::Other, format!("{}: {e}", dir.display())))?;
    let from_zip = {
        let dir = dir.to_path_buf();
        tokio::task::spawn_blocking(move || install_from_anime_build(&dir))
            .await
            .map_err(|e| UiError::new(ErrorKind::Other, e.to_string()))??
    };
    let client = reqwest::Client::builder()
        .user_agent("TatakaiNext/0.1")
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| UiError::new(ErrorKind::Offline, e.to_string()))?;
    let mut written = from_zip;
    for f in entries() {
        let sha = f.sha256.as_deref().unwrap_or_default();
        let dest = dir.join(&f.path);
        // Present (from the zip or an earlier run): keep it. The zip's copies may differ byte-wise from the lock's.
        if dest.is_file() {
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
