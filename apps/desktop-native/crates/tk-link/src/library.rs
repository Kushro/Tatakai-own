//! `GET /v1/library`: ONE user-chosen folder, non-recursive. The host app implements [`crate::LinkHost`]; [`scan_folder`]
//! is a ready-made listing for it (ffprobe optional: without it duration/size fall back to 0 / file size).

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;
use sha2::{Digest, Sha256};

pub const MAX_ITEMS: usize = 10_000;
const EXTENSIONS: [&str; 8] = ["mkv", "mp4", "m4v", "avi", "webm", "mov", "ts", "wmv"];

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LibraryItem {
    /// 32 lowercase hex characters (stable hash of the file path).
    pub id: String,
    pub title: String,
    pub duration_secs: f64,
    pub width: u32,
    pub height: u32,
    pub size_bytes: u64,
}

/// Stable id of a path (never the path itself).
pub fn id_for(path: &Path) -> String {
    hex::encode(&Sha256::digest(path.to_string_lossy().to_lowercase().as_bytes())[..16])
}

/// Result of [`scan_folder`].
pub struct Scan {
    pub items: Vec<LibraryItem>,
    pub truncated: bool,
    /// `(id, path)` pairs for `LinkHost::library_path`.
    pub paths: Vec<(String, PathBuf)>,
}

/// List playable files directly inside `dir` (sorted by title). Returns the items, the truncation flag and the id -> path map.
/// Blocking: call from `spawn_blocking`.
pub fn scan_folder(dir: &Path, ffprobe: Option<&Path>) -> io::Result<Scan> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().and_then(|e| e.to_str()).is_some_and(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str())))
        .collect();
    files.sort_by_key(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()));
    let truncated = files.len() > MAX_ITEMS;
    files.truncate(MAX_ITEMS);
    let mut items = Vec::with_capacity(files.len());
    let mut paths = Vec::with_capacity(files.len());
    for p in files {
        let size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
        let (duration_secs, width, height) = ffprobe.and_then(|f| probe(f, &p)).unwrap_or((0.0, 0, 0));
        let id = id_for(&p);
        items.push(LibraryItem {
            id: id.clone(),
            title: p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
            duration_secs,
            width,
            height,
            size_bytes: size,
        });
        paths.push((id, p));
    }
    Ok(Scan { items, truncated, paths })
}

fn probe(ffprobe: &Path, file: &Path) -> Option<(f64, u32, u32)> {
    let out = Command::new(ffprobe)
        .args(["-v", "error", "-print_format", "json", "-show_format", "-show_streams", "-select_streams", "v:0"])
        .arg(file)
        .output()
        .ok()?;
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    let dur = v["format"]["duration"].as_str().and_then(|d| d.parse::<f64>().ok()).unwrap_or(0.0);
    let s = &v["streams"][0];
    Some((dur, s["width"].as_u64().unwrap_or(0) as u32, s["height"].as_u64().unwrap_or(0) as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_one_folder_non_recursively_with_opaque_ids() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("b episode.MKV"), vec![0u8; 10]).unwrap();
        std::fs::write(dir.path().join("a episode.mp4"), vec![0u8; 5]).unwrap();
        std::fs::write(dir.path().join("notes.txt"), b"x").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub").join("deep.mp4"), b"x").unwrap();
        let Scan { items, truncated, paths } = scan_folder(dir.path(), None).unwrap();
        assert!(!truncated);
        let titles: Vec<_> = items.iter().map(|i| i.title.as_str()).collect();
        assert_eq!(titles, ["a episode", "b episode"]);
        assert_eq!(items[0].size_bytes, 5);
        for i in &items {
            assert_eq!(i.id.len(), 32);
            assert!(i.id.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
            assert!(!i.id.contains("episode"), "ids never carry paths");
        }
        assert_eq!(paths.len(), 2);
        assert_eq!(id_for(&paths[0].1), paths[0].0);
    }
}
