//! File side of "Reset app" (wave A, worker **f**; docs/redesign/15 section 0.12, checklist `system.reset-app`;
//! classic `desktop/ipc/ipc-system.cjs` `reset-app`). Pure file-system code over an injected data root so the
//! tests run in a tempdir; `live/system.rs::reset_app` calls it after stopping what it can.
//!
//! What goes: every entry of the data directory (`ui-settings.json`, `data.db` + its journal, `secrets.json`,
//! `config.json`, `extensions/`, `link/` = Link identity and paired devices, `torrents/` = engine state,
//! `webview/` = web view profile and cache, `torrent-settings.json`, caches...) EXCEPT `logs/` (the open log file;
//! logs hold no personal data and help diagnose the reset itself) and, with "keep downloaded torrents", the
//! download folder when it lives inside the data directory. Nothing outside the data directory is touched here
//! (the torrent download folder outside it is handled by `live/system.rs` through the torrent engine).
//!
//! Windows cannot delete a file another handle has open (SQLite keeps `data.db` open, WebView2 its profile): what
//! cannot be removed now is listed in `<data>/reset.pending` and [`finish_pending`] removes it on the next start,
//! before anything opens those files (tk-app `main.rs` calls it right after the single-instance check).

use std::path::{Component, Path, PathBuf};

/// Marker with the entries a reset could not remove yet (JSON array of names relative to the data root).
pub const PENDING_FILE: &str = "reset.pending";
/// Kept by a reset: the log directory (`AppPaths::logs_dir`).
pub const KEEP_ALWAYS: &[&str] = &["logs"];

/// Remove the contents of `root` except [`KEEP_ALWAYS`] and the top-level entries in `keep` (absolute paths inside
/// `root`; anything else in `keep` is ignored). Returns the entries that could not be removed (also written to
/// [`PENDING_FILE`] so [`finish_pending`] retries them). A missing `root` is not an error.
pub fn wipe(root: &Path, keep: &[PathBuf]) -> Vec<PathBuf> {
    let keep_names: Vec<std::ffi::OsString> = keep
        .iter()
        .filter_map(|k| k.strip_prefix(root).ok())
        .filter_map(|rel| rel.components().next())
        .filter_map(|c| match c {
            Component::Normal(n) => Some(n.to_os_string()),
            _ => None,
        })
        .collect();
    let Ok(entries) = std::fs::read_dir(root) else { return Vec::new() };
    let mut failed = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        if KEEP_ALWAYS.iter().any(|k| name == *k) || keep_names.contains(&name) || name == PENDING_FILE {
            continue;
        }
        let path = entry.path();
        if let Err(e) = remove_any(&path) {
            tracing::warn!(path = %path.display(), error = %e, "reset: could not remove now; retried on the next start");
            failed.push(path);
        }
    }
    write_pending(root, &failed);
    failed
}

/// Finish a reset whose files were locked: remove what [`PENDING_FILE`] lists (names relative to `root`, never
/// outside it), then the marker. Call before anything opens the data files. No marker = nothing to do.
pub fn finish_pending(root: &Path) {
    let marker = root.join(PENDING_FILE);
    let Ok(text) = std::fs::read_to_string(&marker) else { return };
    let names: Vec<String> = serde_json::from_str(&text).unwrap_or_default();
    for name in names {
        let Some(path) = safe_child(root, &name) else { continue };
        if let Err(e) = remove_any(&path)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            tracing::warn!(path = %path.display(), error = %e, "reset: pending entry still cannot be removed");
        }
    }
    let _ = std::fs::remove_file(&marker);
}

/// `root/<name>` when `name` is one plain path component (no separators, no `..`).
fn safe_child(root: &Path, name: &str) -> Option<PathBuf> {
    let rel = Path::new(name);
    let mut comps = rel.components();
    match (comps.next(), comps.next()) {
        (Some(Component::Normal(n)), None) if !KEEP_ALWAYS.iter().any(|k| n == *k) => Some(root.join(n)),
        _ => None,
    }
}

fn write_pending(root: &Path, failed: &[PathBuf]) {
    let marker = root.join(PENDING_FILE);
    if failed.is_empty() {
        let _ = std::fs::remove_file(&marker);
        return;
    }
    let names: Vec<String> =
        failed.iter().filter_map(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).collect();
    if let Ok(json) = serde_json::to_string(&names) {
        let _ = std::fs::write(&marker, json);
    }
}

/// Remove a file, a symlink (never its target) or a directory tree.
fn remove_any(path: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.is_dir() { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(root: &Path) {
        for d in ["logs", "extensions/x", "link", "webview/Cache", "torrents/downloads/Show"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        for f in ["ui-settings.json", "data.db", "secrets.json", "config.json", "logs/tatakai-next.log", "torrents/downloads/Show/ep1.mkv"] {
            std::fs::write(root.join(f), b"x").unwrap();
        }
    }

    #[test]
    fn wipe_removes_everything_but_logs_and_the_kept_download_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("Tatakai-Next");
        tree(&root);
        let outside = tmp.path().join("Downloads");
        std::fs::create_dir_all(&outside).unwrap();
        // the download folder lives inside `torrents/`: keeping it keeps that top-level entry
        let failed = wipe(&root, &[root.join("torrents/downloads"), outside.clone()]);
        assert!(failed.is_empty(), "{failed:?}");
        for gone in ["ui-settings.json", "data.db", "secrets.json", "config.json", "extensions", "link", "webview"] {
            assert!(!root.join(gone).exists(), "{gone} should be gone");
        }
        assert!(root.join("logs/tatakai-next.log").exists());
        assert!(root.join("torrents/downloads/Show/ep1.mkv").exists());
        assert!(outside.exists(), "nothing outside the data root is touched");
        assert!(!root.join(PENDING_FILE).exists());
    }

    #[test]
    fn wipe_without_keep_removes_the_torrent_folder_and_tolerates_a_missing_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("data");
        tree(&root);
        assert!(wipe(&root, &[]).is_empty());
        assert!(!root.join("torrents").exists());
        assert!(wipe(&tmp.path().join("missing"), &[]).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_removed_without_following_them() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("data");
        std::fs::create_dir_all(&root).unwrap();
        let target = tmp.path().join("precious");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("keep.txt"), b"x").unwrap();
        std::os::unix::fs::symlink(&target, root.join("link-to-precious")).unwrap();
        assert!(wipe(&root, &[]).is_empty());
        assert!(target.join("keep.txt").exists());
        assert!(!root.join("link-to-precious").exists());
    }

    #[test]
    fn pending_entries_are_finished_and_never_escape_the_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("data");
        std::fs::create_dir_all(root.join("webview")).unwrap();
        std::fs::write(root.join("data.db"), b"x").unwrap();
        std::fs::write(tmp.path().join("outside.txt"), b"x").unwrap();
        std::fs::create_dir_all(root.join("logs")).unwrap();
        std::fs::write(root.join(PENDING_FILE), r#"["data.db","webview","../outside.txt","/etc","logs","a/b"]"#).unwrap();
        finish_pending(&root);
        assert!(!root.join("data.db").exists() && !root.join("webview").exists());
        assert!(tmp.path().join("outside.txt").exists());
        assert!(root.join("logs").exists());
        assert!(!root.join(PENDING_FILE).exists());
        // no marker: nothing happens
        finish_pending(&root);
    }

    #[test]
    fn safe_child_accepts_one_plain_component() {
        let root = Path::new("/r");
        assert_eq!(safe_child(root, "data.db"), Some(PathBuf::from("/r/data.db")));
        assert_eq!(safe_child(root, ".."), None);
        assert_eq!(safe_child(root, "a/b"), None);
        assert_eq!(safe_child(root, "logs"), None);
        assert_eq!(safe_child(root, ""), None);
    }
}
