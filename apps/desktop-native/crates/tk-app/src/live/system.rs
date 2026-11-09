//! `Backend` system calls (wave A, worker **f**; docs/redesign/15 section 4): data & storage, about, OS
//! integration, first-run wizard support, "reset app" and the start-up jobs. Classic: `desktop/ipc/ipc-system.cjs`
//! (`system:get-storage-info`, `system:clear-storage-category`, `export-logs`, `set-auto-launch`, `reset-app`) and
//! `desktop/services/update-manager.cjs` (notice only).
//!
//! * `storage_usage` / `clear_cache` walk `b.paths()`: the metadata TTL cache rows of the store (`cache` table),
//!   cached translations, `<data>/images` and the web view caches (`webview/**/{Cache,cache,...}`).
//! * `open_*` = `open::that_detached`; `export_history` writes JSON (camelCase `tk_core::HistoryEntry`, the local
//!   Next format) or CSV from `b.store.history(..)` into the user's Downloads folder.
//! * `licenses_text` = `THIRD_PARTY_NOTICES.md` of this build.
//! * `check_updates` = `crate::update_check::latest` (GitHub `releases/latest` of `Kushro/Tatakai-own`) + `semver`;
//!   the answer is kept for [`UPDATE_CACHE`] so the start-up notice and Settings > About share one request.
//! * autostart = `crate::autostart`.
//! * `reset_app` revokes the paired Link devices, removes the torrents (with their files unless `keep_downloads`),
//!   deletes the keyring entries (`b.secrets.wipe` over every `tk_core::secrets::keys`), empties the store tables
//!   it can reach, then removes the contents of `b.paths().root` (`crate::reset::wipe`) except the logs and, with
//!   `keep_downloads`, the download folder. Nothing outside the data directory and the download folder is touched.
//!   The UI then shows the first-run wizard with default settings (`ui-settings.json` is gone).
//! * `on_start` runs once per launch from `Services::on_start`: the update check when
//!   `Settings::system.check_updates` (the shell's snackbar in `tk_ui::tray` reads the cached answer).

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use time::OffsetDateTime;
use tk_core::HistoryEntry;
use tk_ui::backend::{DebridServiceVm, ExportFormatVm, StorageUsageVm, SystemCapsVm, UiResult, UpdateCheckVm};
use tk_ui::model::{ErrorKind, PlatformVm, TranslationProviderVm, UiError};

use crate::live::LiveBackend;

/// How long one GitHub answer is reused (start-up notice, Settings > About, the wizard).
pub const UPDATE_CACHE: Duration = Duration::from_secs(10 * 60);
/// History rows an export writes at most (the whole local history in practice).
const EXPORT_LIMIT: u32 = 1_000_000;
/// Directory names (lower case) under `webview/` that hold caches (WebKitGTK and WebView2 layouts).
const WEBVIEW_CACHE_DIRS: &[&str] = &["cache", "code cache", "gpucache", "shadercache", "grshadercache", "cachestorage", "dawncache", "service worker"];

static LAST_UPDATE: Mutex<Option<(Instant, UpdateCheckVm)>> = Mutex::new(None);

fn other(e: impl std::fmt::Display) -> UiError {
    UiError::new(ErrorKind::Other, e.to_string())
}

fn now_unix() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

// ---- reset -------------------------------------------------------------------------------------------------------

/// Every secret key the app writes (keyring entries are per key, so a reset names them all).
pub fn all_secret_keys() -> Vec<String> {
    let mut keys = vec![tk_core::secrets::keys::ANILIST_TOKEN.to_owned(), tk_core::secrets::keys::MAL_TOKEN.to_owned()];
    keys.extend(DebridServiceVm::ALL.iter().map(|s| tk_core::secrets::keys::debrid(s.id())));
    keys.extend(
        TranslationProviderVm::ALL
            .iter()
            .filter(|p| **p != TranslationProviderVm::Off)
            .map(|p| tk_core::secrets::keys::translation(p.id())),
    );
    keys
}

/// The torrent download folder in use (`TorrentSettingsVm::download_dir`, else the default) and whether it is the
/// app's own default folder (then a reset may remove it whole).
fn download_dir(b: &LiveBackend) -> (PathBuf, bool) {
    let paths = b.paths();
    let s = super::torrent_settings::load(&paths);
    let default = paths.default_torrent_download_dir();
    let chosen = s.download_dir.trim();
    if chosen.is_empty() {
        (default, true)
    } else {
        let p = PathBuf::from(chosen);
        let is_default = p == default;
        (p, is_default)
    }
}

/// `Backend::reset_app`.
pub async fn reset_app(b: &LiveBackend, keep_downloads: bool) -> UiResult<()> {
    let root = b.paths().root;
    tracing::warn!(keep_downloads, data = %root.display(), "resetting the app");

    // 1. Link: revoke every paired device (the server keeps running until the next start, with no device left).
    if let Ok(status) = b.link.status().await {
        for d in status.devices {
            if let Err(e) = b.link.revoke(&d.id).await {
                tracing::warn!(device = %d.id, detail = %e.detail, "reset: revoking a Link device failed");
            }
        }
    }

    // 2. Torrents: without "keep downloads", remove every torrent with its files (this reaches custom folders
    //    outside the data directory without deleting anything else that lives there).
    let (downloads, downloads_is_default) = download_dir(b);
    if !keep_downloads && (b.torrent.running().is_some() || b.paths().torrent_dir().exists()) {
        match b.torrent.library().await {
            Ok(items) => {
                for t in items {
                    if let Err(e) = b.torrent.remove(&t.id, true).await {
                        tracing::warn!(torrent = %t.id, detail = %e.detail, "reset: removing a torrent failed");
                    }
                }
            }
            Err(e) => tracing::warn!(detail = %e.detail, "reset: the torrent engine did not start; only folders are removed"),
        }
    }

    // 3. Secrets: keyring entries + fallback file.
    if let Err(e) = b.secrets.wipe(&all_secret_keys()) {
        tracing::warn!(error = %e, "reset: wiping secrets failed");
    }

    // 4. Store: empty what this session would otherwise keep showing (the file itself goes in step 5; on Windows
    //    the open database cannot be deleted until the next start, see `crate::reset`).
    empty_store(&b.store);
    b.invalidate_sources();

    // 5. Files of the data directory.
    let keep = if keep_downloads { vec![downloads.clone()] } else { Vec::new() };
    let root2 = root.clone();
    let failed = tokio::task::spawn_blocking(move || crate::reset::wipe(&root2, &keep)).await.map_err(other)?;
    if !failed.is_empty() {
        tracing::info!(count = failed.len(), "reset: some files are in use; they are removed on the next start");
    }

    // 6. The app's own default download folder (`<Downloads>/Tatakai`), outside the data directory.
    if !keep_downloads && downloads_is_default && !downloads.starts_with(&root) && downloads.is_dir() {
        let d = downloads.clone();
        let res = tokio::task::spawn_blocking(move || std::fs::remove_dir_all(&d)).await.map_err(other)?;
        if let Err(e) = res {
            tracing::warn!(path = %downloads.display(), error = %e, "reset: removing the download folder failed");
        }
    }
    *LAST_UPDATE.lock().unwrap_or_else(|e| e.into_inner()) = None;
    super::history_sync::forget_last_sync();
    Ok(())
}

/// Best-effort: every table the store exposes a way to empty.
fn empty_store(store: &tk_core::LocalStore) {
    let _ = store.clear_history();
    let _ = store.cache_purge(i64::MAX);
    let _ = store.translation_clear();
    let _ = store.clear_torrent_sessions();
    for service in ["anilist", "mal"] {
        for row in store.list_entries(service, None).unwrap_or_default() {
            let _ = store.delete_list_entry(service, &row.media_id);
        }
    }
    for op in store.list_queue(None).unwrap_or_default() {
        let _ = store.dequeue_list_op(op.id);
    }
    for c in store.list_conflicts().unwrap_or_default() {
        let _ = store.delete_list_conflict(c.id);
    }
    for f in store.favorites().unwrap_or_default() {
        let _ = store.remove_favorite(&f.media_id);
    }
}

// ---- storage -----------------------------------------------------------------------------------------------------

/// Recursive size of a file or directory (symlinks are not followed). Missing = 0.
pub fn dir_size(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return 0 };
    if meta.file_type().is_symlink() {
        return 0;
    }
    if meta.is_file() {
        return meta.len();
    }
    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_symlink() {
                continue;
            }
            if ft.is_dir() {
                stack.push(e.path());
            } else if let Ok(m) = e.metadata() {
                total = total.saturating_add(m.len());
            }
        }
    }
    total
}

/// Cache directories inside the web view profile (WebKitGTK and WebView2 layouts), at most 4 levels deep.
pub fn webview_cache_dirs(webview: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![(webview.to_path_buf(), 0u8)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            if !e.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let name = e.file_name().to_string_lossy().to_lowercase();
            if WEBVIEW_CACHE_DIRS.contains(&name.as_str()) {
                out.push(e.path());
            } else if depth < 4 {
                stack.push((e.path(), depth + 1));
            }
        }
    }
    out
}

fn disk_caches(root: &Path) -> Vec<PathBuf> {
    let mut v = webview_cache_dirs(&root.join("webview"));
    v.push(root.join("images"));
    v.push(root.join("cache"));
    v
}

/// Free bytes on the volume that holds `path` (`df` on Unix, PowerShell on Windows); `None` when unknown.
pub fn free_bytes(path: &Path) -> Option<u64> {
    let existing = path.ancestors().find(|p| p.exists())?.to_path_buf();
    #[cfg(unix)]
    {
        let out = std::process::Command::new("df").arg("-Pk").arg(&existing).output().ok()?;
        if !out.status.success() {
            return None;
        }
        parse_df(&String::from_utf8_lossy(&out.stdout))
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let drive: String = existing.to_string_lossy().chars().take_while(|c| *c != ':').collect();
        if drive.len() != 1 || !drive.chars().all(|c| c.is_ascii_alphabetic()) {
            return None;
        }
        let out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &format!("(Get-PSDrive -Name {drive}).Free")])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;
        String::from_utf8_lossy(&out.stdout).trim().parse().ok()
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = existing;
        None
    }
}

/// `df -Pk` output: the "Available" column (KiB) of the last line.
pub fn parse_df(out: &str) -> Option<u64> {
    let line = out.lines().rfind(|l| !l.trim().is_empty())?;
    let cols: Vec<&str> = line.split_whitespace().collect();
    if cols.first().is_some_and(|c| *c == "Filesystem") {
        return None;
    }
    cols.get(3)?.parse::<u64>().ok().map(|kib| kib.saturating_mul(1024))
}

/// `Backend::storage_usage`.
pub async fn storage_usage(b: &LiveBackend) -> UiResult<StorageUsageVm> {
    let paths = b.paths();
    let (downloads, _) = download_dir(b);
    let store = b.store.clone();
    let root = paths.root.clone();
    let vm = tokio::task::spawn_blocking(move || {
        let (_, cache_rows_bytes) = store.cache_stats().unwrap_or((0, 0));
        let cache_bytes = cache_rows_bytes.saturating_add(disk_caches(&root).iter().map(|p| dir_size(p)).sum::<u64>());
        let torrent_state = dir_size(&paths.torrent_dir());
        // the download folder may live inside `torrents/` (fallback layout): never count it twice
        let torrents_bytes = if downloads.starts_with(paths.torrent_dir()) {
            torrent_state
        } else {
            torrent_state.saturating_add(dir_size(&downloads))
        };
        StorageUsageVm {
            data_dir: root.display().to_string(),
            cache_bytes,
            torrents_bytes,
            logs_bytes: dir_size(&paths.logs_dir()),
            extensions_bytes: dir_size(&paths.extensions_dir()),
            free_bytes: free_bytes(&root),
        }
    })
    .await
    .map_err(other)?;
    Ok(vm)
}

/// `Backend::clear_cache` (bytes freed): metadata cache rows, cached translations, images and web view caches.
pub async fn clear_cache(b: &LiveBackend) -> UiResult<u64> {
    let store = b.store.clone();
    let root = b.paths().root;
    let freed = tokio::task::spawn_blocking(move || {
        let (_, rows_bytes) = store.cache_stats().unwrap_or((0, 0));
        let mut freed = 0u64;
        if store.cache_purge(i64::MAX).is_ok() {
            freed = freed.saturating_add(rows_bytes);
        }
        let _ = store.translation_clear();
        for dir in disk_caches(&root) {
            if !dir.exists() {
                continue;
            }
            // empty the directory but keep it (the web view expects its layout); files in use stay
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for e in entries.flatten() {
                    let p = e.path();
                    let before = dir_size(&p);
                    let gone = if e.file_type().is_ok_and(|t| t.is_dir()) { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
                    if gone.is_ok() {
                        freed = freed.saturating_add(before);
                    }
                }
            }
        }
        freed
    })
    .await
    .map_err(other)?;
    b.invalidate_sources();
    Ok(freed)
}

// ---- open --------------------------------------------------------------------------------------------------------

fn open_path(path: &Path) -> UiResult<()> {
    open::that_detached(path).map_err(|e| UiError::new(ErrorKind::Other, format!("cannot open {}: {e}", path.display())))
}

/// `Backend::open_data_folder`.
pub async fn open_data_folder(b: &LiveBackend) -> UiResult<()> {
    let root = b.paths().root;
    std::fs::create_dir_all(&root).map_err(other)?;
    open_path(&root)
}

/// `Backend::open_logs`: the current log file in the system viewer, else the logs folder.
pub async fn open_logs(b: &LiveBackend) -> UiResult<()> {
    let dir = b.paths().logs_dir();
    let file = dir.join(crate::logging::LOG_FILE);
    if file.is_file() {
        return open_path(&file);
    }
    std::fs::create_dir_all(&dir).map_err(other)?;
    open_path(&dir)
}

/// `Backend::open_url` (`http(s)` only; anything else is `ErrorKind::Other`).
pub async fn open_url(_b: &LiveBackend, url: &str) -> UiResult<()> {
    let url = url.trim();
    if !is_web_url(url) {
        return Err(UiError::new(ErrorKind::Other, format!("refusing to open a non-web URL: {url}")));
    }
    open::that_detached(url).map_err(|e| UiError::new(ErrorKind::Other, format!("cannot open the browser: {e}")))
}

/// `http://` or `https://` with a host. Pure.
pub fn is_web_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    let rest = lower.strip_prefix("https://").or_else(|| lower.strip_prefix("http://"));
    rest.is_some_and(|r| !r.is_empty() && !r.starts_with('/') && !r.chars().any(char::is_whitespace))
}

// ---- export ------------------------------------------------------------------------------------------------------

/// `tatakai-history-<YYYY-MM-DD>.<ext>`.
pub fn export_file_name(format: ExportFormatVm, now: OffsetDateTime) -> String {
    let d = now.date();
    format!("tatakai-history-{:04}-{:02}-{:02}.{}", d.year(), u8::from(d.month()), d.day(), format.id())
}

/// JSON array of `HistoryEntry` (camelCase, the local Next format), newest first.
pub fn history_json(rows: &[HistoryEntry]) -> Result<String, String> {
    serde_json::to_string_pretty(rows).map_err(|e| e.to_string())
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_owned() }
}

/// CSV with a header row; same columns as the JSON keys.
pub fn history_csv(rows: &[HistoryEntry]) -> String {
    let mut out = String::from("animeId,title,episodeId,episode,progressSec,durationSec,completed,watchedAt,anilistId,malId\n");
    for r in rows {
        let watched = r.watched_at.format(&time::format_description::well_known::Rfc3339).unwrap_or_default();
        let opt = |v: Option<u32>| v.map(|n| n.to_string()).unwrap_or_default();
        out.push_str(&[
            csv_field(&r.anime_id),
            csv_field(&r.title),
            csv_field(&r.episode_id),
            r.episode.to_string(),
            r.progress_sec.to_string(),
            opt(r.duration_sec),
            r.completed.to_string(),
            watched,
            opt(r.anilist_id),
            opt(r.mal_id),
        ]
        .join(","));
        out.push('\n');
    }
    out
}

/// A free file name in `dir` (`name`, then `name (2)`, ...).
fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = name.rsplit_once('.').unwrap_or((name, ""));
    (2..1000).map(|i| dir.join(format!("{stem} ({i}).{ext}"))).find(|p| !p.exists()).unwrap_or(first)
}

/// `Backend::export_history`.
pub async fn export_history(b: &LiveBackend, format: ExportFormatVm) -> UiResult<PathBuf> {
    let rows = b.store.history(EXPORT_LIMIT).map_err(other)?;
    let body = match format {
        ExportFormatVm::Json => history_json(&rows).map_err(other)?,
        ExportFormatVm::Csv => history_csv(&rows),
    };
    let dir = directories::UserDirs::new()
        .and_then(|u| u.download_dir().map(Path::to_path_buf))
        .unwrap_or_else(|| b.paths().root.join("exports"));
    std::fs::create_dir_all(&dir).map_err(|e| other(format!("cannot create {}: {e}", dir.display())))?;
    let path = unique_path(&dir, &export_file_name(format, OffsetDateTime::now_utc()));
    std::fs::write(&path, body).map_err(|e| other(format!("cannot write {}: {e}", path.display())))?;
    tracing::info!(path = %path.display(), rows = rows.len(), "history exported");
    Ok(path)
}

// ---- about -------------------------------------------------------------------------------------------------------

/// `THIRD_PARTY_NOTICES.md` of this build.
pub const LICENSES: &str = include_str!("../../../../THIRD_PARTY_NOTICES.md");

/// `Backend::licenses_text`.
pub async fn licenses_text(_b: &LiveBackend) -> UiResult<String> {
    Ok(LICENSES.to_owned())
}

/// The view-model of a GitHub answer. Pure.
pub fn update_vm(current: &str, latest: Option<crate::update_check::Latest>, checked_unix: i64) -> UpdateCheckVm {
    match latest {
        Some(l) => UpdateCheckVm {
            current: current.to_owned(),
            newer: crate::update_check::is_newer(current, &l.tag),
            latest: Some(l.tag),
            url: Some(l.url),
            notes: l.notes,
            checked_unix,
        },
        None => UpdateCheckVm { current: current.to_owned(), latest: None, newer: false, url: None, notes: None, checked_unix },
    }
}

async fn fetch_update() -> UiResult<UpdateCheckVm> {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(15)).build().map_err(other)?;
    let latest = crate::update_check::latest(&client).await.map_err(|e| UiError::new(ErrorKind::Other, e))?;
    Ok(update_vm(env!("CARGO_PKG_VERSION"), latest, now_unix()))
}

/// `Backend::check_updates`: one request per [`UPDATE_CACHE`].
pub async fn check_updates(_b: &LiveBackend) -> UiResult<UpdateCheckVm> {
    if let Some((at, vm)) = LAST_UPDATE.lock().unwrap_or_else(|e| e.into_inner()).clone()
        && at.elapsed() < UPDATE_CACHE
    {
        return Ok(vm);
    }
    let vm = fetch_update().await?;
    *LAST_UPDATE.lock().unwrap_or_else(|e| e.into_inner()) = Some((Instant::now(), vm.clone()));
    Ok(vm)
}

// ---- OS integration ----------------------------------------------------------------------------------------------

/// `Backend::autostart_enabled` (`crate::autostart::is_enabled`).
pub async fn autostart_enabled(_b: &LiveBackend) -> UiResult<bool> {
    tokio::task::spawn_blocking(crate::autostart::is_enabled).await.map_err(other)?.map_err(other)
}

/// `Backend::set_autostart` (`crate::autostart::set_enabled`).
pub async fn set_autostart(_b: &LiveBackend, enabled: bool) -> UiResult<()> {
    tokio::task::spawn_blocking(move || crate::autostart::set_enabled(enabled)).await.map_err(other)?.map_err(other)
}

/// `Backend::system_caps` (sync): autostart where `crate::autostart` supports the platform, the tray on the three
/// desktop platforms (`tk_ui::tray`), the `magnet:` handler as worker i's `crate::magnet::supported()` says.
pub fn system_caps(_b: &LiveBackend) -> SystemCapsVm {
    SystemCapsVm {
        platform: PlatformVm::current(),
        autostart: crate::autostart::supported(),
        tray: cfg!(any(target_os = "windows", target_os = "linux", target_os = "macos")),
        magnet_handler: crate::magnet::supported(),
    }
}

/// Seconds of the metadata TTL cache chosen in Settings (`Settings::system.metadata_cache_ttl_hours`), for the
/// `b.store.cache_put` calls of the metadata paths (details, characters, watch order).
#[allow(dead_code)] // used by the metadata cache writers (live/details.rs, live/search.rs) once the integrator wires it
pub fn metadata_ttl_secs(b: &LiveBackend) -> i64 {
    i64::from(crate::settings_store::load(&b.settings_file).system.metadata_cache_ttl_hours) * 3600
}

// ---- start-up ----------------------------------------------------------------------------------------------------

/// Start-up job (called once by `LiveBackend::on_start`, spawned by tk-app's `main.rs` through `Services::on_start`):
/// the new-version check when `Settings::system.check_updates` (the answer is cached for the shell's notice).
pub async fn on_start(b: &LiveBackend) {
    let settings = crate::settings_store::load(&b.settings_file);
    if settings.system.check_updates {
        match check_updates(b).await {
            Ok(vm) if vm.newer => tracing::info!(latest = ?vm.latest, "a newer release is available"),
            Ok(_) => {}
            Err(e) => tracing::debug!(detail = %e.detail, "update check failed"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tk_core::MediaRef;

    fn entry(title: &str, ep: u32) -> HistoryEntry {
        HistoryEntry::for_playback(&MediaRef::anilist(21), title, None, ep, 383, Some(1440), OffsetDateTime::from_unix_timestamp(1_793_135_051).unwrap()).unwrap()
    }

    #[test]
    fn exports_use_the_local_camel_case_shape_and_escape_csv() {
        let rows = vec![entry("One Piece", 3), entry("Say \"hi\", ok", 4)];
        let json: serde_json::Value = serde_json::from_str(&history_json(&rows).unwrap()).unwrap();
        assert_eq!(json[0]["animeId"], "21");
        assert_eq!(json[0]["progressSec"], 383);
        assert_eq!(json[0]["watchedAt"], "2026-10-27T21:04:11Z");
        let csv = history_csv(&rows);
        let mut lines = csv.lines();
        assert!(lines.next().unwrap().starts_with("animeId,title,episodeId,episode,progressSec"));
        assert_eq!(lines.next().unwrap(), "21,One Piece,21?ep=3,3,383,1440,false,2026-10-27T21:04:11Z,21,");
        assert!(lines.next().unwrap().starts_with("21,\"Say \"\"hi\"\", ok\",21?ep=4"));
        let name = export_file_name(ExportFormatVm::Csv, OffsetDateTime::from_unix_timestamp(1_793_135_051).unwrap());
        assert_eq!(name, "tatakai-history-2026-10-27.csv");
    }

    #[test]
    fn unique_path_does_not_overwrite() {
        let tmp = tempfile::tempdir().unwrap();
        let a = unique_path(tmp.path(), "h.json");
        std::fs::write(&a, b"x").unwrap();
        assert_eq!(unique_path(tmp.path(), "h.json"), tmp.path().join("h (2).json"));
    }

    #[test]
    fn sizes_and_web_view_caches() {
        let tmp = tempfile::tempdir().unwrap();
        let wv = tmp.path().join("webview");
        std::fs::create_dir_all(wv.join("EBWebView/Default/Cache/Cache_Data")).unwrap();
        std::fs::create_dir_all(wv.join("EBWebView/Default/Local Storage")).unwrap();
        std::fs::write(wv.join("EBWebView/Default/Cache/Cache_Data/f_1"), vec![0u8; 100]).unwrap();
        std::fs::write(wv.join("EBWebView/Default/Local Storage/keep"), vec![0u8; 7]).unwrap();
        let dirs = webview_cache_dirs(&wv);
        assert_eq!(dirs, vec![wv.join("EBWebView/Default/Cache")]);
        assert_eq!(dir_size(&dirs[0]), 100);
        assert_eq!(dir_size(&wv), 107);
        assert_eq!(dir_size(&tmp.path().join("missing")), 0);
    }

    #[test]
    fn df_output_is_parsed() {
        let out = "Filesystem     1024-blocks      Used Available Capacity Mounted on\n/dev/nvme0n1p2   960000000 700000000 260000000      73% /\n";
        assert_eq!(parse_df(out), Some(260_000_000 * 1024));
        assert_eq!(parse_df("Filesystem 1024-blocks Used Available Capacity Mounted on\n"), None);
        assert_eq!(parse_df(""), None);
    }

    #[test]
    fn only_web_urls_open() {
        assert!(is_web_url("https://github.com/Kushro/Tatakai-own/releases"));
        assert!(is_web_url("HTTP://example.org"));
        assert!(!is_web_url("file:///etc/passwd"));
        assert!(!is_web_url("https://"));
        assert!(!is_web_url("javascript:alert(1)"));
        assert!(!is_web_url("https://a b"));
    }

    #[test]
    fn update_view_model_compares_versions() {
        let l = crate::update_check::Latest { tag: "v99.0.0".into(), url: "https://github.com/x".into(), notes: Some("n".into()) };
        let vm = update_vm("0.1.0", Some(l), 5);
        assert!(vm.newer);
        assert_eq!(vm.latest.as_deref(), Some("v99.0.0"));
        let none = update_vm("0.1.0", None, 5);
        assert!(!none.newer && none.latest.is_none() && none.checked_unix == 5);
    }

    #[test]
    fn every_secret_key_is_named_for_the_reset() {
        let keys = all_secret_keys();
        assert!(keys.contains(&"lists.anilist.token".to_owned()));
        assert!(keys.contains(&"debrid.real_debrid.key".to_owned()) || keys.iter().any(|k| k.starts_with("debrid.")));
        assert!(keys.contains(&"translate.deepl.key".to_owned()));
        assert!(!keys.contains(&"translate.off.key".to_owned()));
    }

    #[test]
    fn the_licenses_text_is_the_notices_file() {
        assert!(LICENSES.contains("Third-party notices"));
    }
}
