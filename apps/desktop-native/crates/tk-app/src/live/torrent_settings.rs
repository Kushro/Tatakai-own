//! Torrent client settings persistence and the background-download / after-watch policy (docs/redesign/10
//! section 4.3). Owner: wave 3 D.
//!
//! `<data>/torrent-settings.json` holds `TorrentSettingsVm` (serde, `#[serde(default)]`, `sanitized()` on load and
//! save, atomic write like `settings_store`). Missing or corrupt files load as the defaults.
//!
//! Policy when a playback session ends (`playback_ended`), applied after a short grace period so that "next
//! episode" in a batch (the player reports the end, then plays the next file of the same torrent) never loses the
//! torrent:
//!
//! | `persist` | background downloads | action |
//! |---|---|---|
//! | false (after watching = Delete) | any | remove with its files |
//! | true | off | pause (nothing uploads while nothing plays) |
//! | true | on | keep downloading / seeding |
//!
//! The action is skipped when, at the end of the grace period, a stream of the torrent is open again or its label
//! moved to another episode (the player went on in the batch); `persist` is read again then.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use tk_core::release::{CandidateFile, pick_episode_file};
use tk_torrent::{AddOptions, TorrentEngine, TorrentState};
use tk_ui::backend::{TorrentSettingsVm, UiResult};
use tk_ui::model::{ErrorKind, UiError};

use super::LiveBackend;
use super::torrent::engine_err;
use super::torrent_play;
use crate::paths::AppPaths;

/// How long `playback_ended` waits before removing / pausing (covers the player switching to the next episode).
const END_GRACE: Duration = Duration::from_secs(20);
/// Lowest non-zero listen port accepted (no privileged ports), like the Link port.
const MIN_LISTEN_PORT: u16 = 1024;

/// What happens to a torrent once nothing plays it any more.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EndAction {
    /// Remove it and its files (not kept after watching).
    Remove,
    /// Kept, background downloads off: stop transferring.
    Pause,
    /// Kept, background downloads on: keep downloading / seeding.
    Keep,
}

fn end_action(persist: bool, background: bool) -> EndAction {
    match (persist, background) {
        (false, _) => EndAction::Remove,
        (true, false) => EndAction::Pause,
        (true, true) => EndAction::Keep,
    }
}

/// `0` (random) or `MIN_LISTEN_PORT..=65535`.
fn listen_port_ok(port: u16) -> bool {
    port == 0 || port >= MIN_LISTEN_PORT
}

/// What `save_settings` writes: the incoming settings, sanitized, except that the warning acceptance is one-way (a
/// stale form never takes `background_ack` back).
fn merge_for_save(stored: &TorrentSettingsVm, incoming: &TorrentSettingsVm) -> TorrentSettingsVm {
    let mut next = incoming.clone();
    next.background_ack = stored.background_ack || incoming.background_ack;
    next.sanitized()
}

fn read_file(path: &Path) -> TorrentSettingsVm {
    match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<TorrentSettingsVm>(&text) {
            Ok(s) => s.sanitized(),
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "torrent settings file is invalid; using defaults");
                TorrentSettingsVm::default()
            }
        },
        Err(_) => TorrentSettingsVm::default(),
    }
}

fn write_file(path: &Path, settings: &TorrentSettingsVm) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(settings).map_err(std::io::Error::other)? + "\n")?;
    std::fs::rename(&tmp, path)
}

/// Saved settings (defaults when missing or unreadable), sanitized, with `default_download_dir` filled. Also read by
/// `torrent::TorrentService` (A) to start the engine.
pub fn load(paths: &AppPaths) -> TorrentSettingsVm {
    TorrentSettingsVm {
        default_download_dir: paths.default_torrent_download_dir().display().to_string(),
        ..read_file(&paths.torrent_settings_file())
    }
}

/// Persist `settings.clone().sanitized()`.
pub fn save(paths: &AppPaths, settings: &TorrentSettingsVm) -> UiResult<()> {
    let path = paths.torrent_settings_file();
    write_file(&path, &settings.clone().sanitized())
        .map_err(|e| UiError::new(ErrorKind::Other, format!("cannot write {}: {e}", path.display())))
}

/// `Backend::torrent_settings`.
pub async fn settings(b: &LiveBackend) -> UiResult<TorrentSettingsVm> {
    Ok(load(&b.paths()))
}

/// `Backend::save_torrent_settings`: `save` then `b.torrent.apply_settings`. Rejects a privileged listen port.
pub async fn save_settings(b: &LiveBackend, settings: &TorrentSettingsVm) -> UiResult<()> {
    if !listen_port_ok(settings.listen_port) {
        return Err(UiError::new(
            ErrorKind::Other,
            format!("listen port {} is not allowed (0 or {MIN_LISTEN_PORT}..=65535)", settings.listen_port),
        ));
    }
    let paths = b.paths();
    let next = merge_for_save(&load(&paths), settings);
    save(&paths, &next)?;
    b.torrent.apply_settings(&load(&paths)).await
}

/// `Backend::background_downloads_ack`: persist `background_ack = true`.
pub async fn background_ack(b: &LiveBackend) -> UiResult<()> {
    let paths = b.paths();
    let mut s = load(&paths);
    if s.background_ack {
        return Ok(());
    }
    s.background_ack = true;
    save(&paths, &s)
}

/// `Backend::background_downloads_enabled`.
pub async fn background_enabled(b: &LiveBackend) -> UiResult<bool> {
    let s = load(&b.paths());
    Ok(s.background_ack && s.background_downloads)
}

/// Main title + synonyms of `id` (for `pick_episode_file`), plus the label's title (also the fallback when the
/// metadata providers do not answer). Deduplicated, case-insensitively.
async fn series_titles(b: &LiveBackend, id: &str, fallback: &str) -> Vec<String> {
    let mut titles: Vec<String> = Vec::new();
    if let Ok((_, media)) = b.media(id).await {
        titles.push(media.title.clone());
        titles.extend(media.titles.values().cloned());
    }
    if !fallback.is_empty() {
        titles.push(fallback.to_owned());
    }
    let mut seen = HashSet::new();
    titles.retain(|title| !title.trim().is_empty() && seen.insert(title.to_lowercase()));
    titles
}

/// Resume `tid` when the engine reports it paused (a saved torrent added again: the user asked to download it).
async fn resume_if_paused(engine: &TorrentEngine, tid: &str) -> UiResult<()> {
    if engine.stats(tid).is_ok_and(|s| s.state == TorrentState::Paused) {
        engine.resume(tid).await.map_err(engine_err)?;
    }
    Ok(())
}

/// `Backend::torrent_add_download`: refuse unless `background_enabled`; `torrent_play::resolve_source` (B) ->
/// `engine.add(.., AddOptions { persist: true, only_files: the episode file when `episode` is `Some`, .. })`.
///
/// With an episode the torrent is added with an empty file selection, the episode file is picked from its file list
/// (`pick_episode_file`) and selected (`select_file`); a torrent this call added is removed again when no file
/// matches (`ErrorKind::NoSource`, torrent id as detail).
pub async fn add_download(b: &LiveBackend, source: &str, id: &str, episode: Option<u32>) -> UiResult<String> {
    if !background_enabled(b).await? {
        return Err(UiError::new(
            ErrorKind::Other,
            "background downloads are off: accept the warning and turn them on in Torrents > Settings",
        ));
    }
    let (add, label) = torrent_play::resolve_source(b, source, id, episode).await?;
    let engine = b.torrent.engine().await?;
    let label_title = label.title.clone();

    let Some(episode) = episode else {
        let tid = engine
            .add(add, AddOptions { persist: true, label: Some(label), ..AddOptions::default() })
            .await
            .map_err(engine_err)?;
        resume_if_paused(&engine, &tid).await?;
        tracing::info!(torrent = %tid, "background download added (every file)");
        return Ok(tid);
    };

    let known: HashSet<String> = engine.list().into_iter().map(|e| e.id).collect();
    let tid = engine
        .add(add, AddOptions { only_files: Some(Vec::new()), persist: true, label: Some(label), ..AddOptions::default() })
        .await
        .map_err(engine_err)?;
    let files: Vec<CandidateFile> = engine
        .files(&tid)
        .map_err(engine_err)?
        .into_iter()
        .map(|f| CandidateFile { index: f.index, path: f.path, size_bytes: f.size_bytes })
        .collect();
    let titles = series_titles(b, id, &label_title).await;
    let Some(index) = pick_episode_file(&files, episode, &titles) else {
        if !known.contains(&tid)
            && let Err(e) = engine.remove(&tid, true).await
        {
            tracing::warn!(torrent = %tid, error = %e, "removing an unmatched background download failed");
        }
        return Err(UiError::new(ErrorKind::NoSource, tid));
    };
    engine.select_file(&tid, index).await.map_err(engine_err)?;
    resume_if_paused(&engine, &tid).await?;
    tracing::info!(torrent = %tid, episode, file = index, "background download added");
    Ok(tid)
}

/// `Backend::torrent_playback_ended`: torrents not kept (`persist == false`, i.e. after_watch Delete when it was
/// added) are removed with their files unless a background download of the same torrent is running; kept ones
/// pause when background downloads are off (so nothing uploads while nothing plays) and keep seeding otherwise.
///
/// Background downloads are always added kept (`persist: true`, OR-ed by the engine), so a torrent that is still
/// not kept is never one. The action runs after `END_GRACE` in a background task (see the module docs).
pub async fn playback_ended(b: &LiveBackend, torrent_id: &str, completed: bool) -> UiResult<()> {
    // Wave A (worker i): the session-history row of this playback ends here (a batch's next episode opens a new one).
    b.torrent.close_session(torrent_id);
    let Some(engine) = b.torrent.running() else { return Ok(()) };
    let Some(entry) = engine.list().into_iter().find(|e| e.id == torrent_id) else { return Ok(()) };
    let background = background_enabled(b).await.unwrap_or(false);
    let action = end_action(entry.persist, background);
    tracing::info!(torrent = %torrent_id, completed, persist = entry.persist, background, ?action, "torrent playback ended");
    if action == EndAction::Keep {
        return Ok(());
    }
    let episode = entry.label.as_ref().and_then(|l| l.episode);
    let paths = b.paths();
    let id = torrent_id.to_owned();
    match tokio::runtime::Handle::try_current() {
        Ok(rt) => {
            rt.spawn(async move {
                tokio::time::sleep(END_GRACE).await;
                apply_end(&engine, &paths, &id, episode).await;
            });
        }
        // No runtime to defer on (never the case in the app): act now.
        Err(_) => apply_end(&engine, &paths, &id, episode).await,
    }
    Ok(())
}

/// The deferred half of `playback_ended`: re-read the torrent and the settings, then remove / pause unless the
/// torrent is in use again.
async fn apply_end(engine: &Arc<TorrentEngine>, paths: &AppPaths, id: &str, episode: Option<u32>) {
    let Some(entry) = engine.list().into_iter().find(|e| e.id == id) else { return };
    let streaming = engine.stats(id).map(|s| s.streaming).unwrap_or(false);
    let moved_on = entry.label.as_ref().and_then(|l| l.episode) != episode;
    if streaming || moved_on {
        tracing::debug!(torrent = %id, streaming, moved_on, "torrent in use again; after-watch policy skipped");
        return;
    }
    let s = load(paths);
    let result = match end_action(entry.persist, s.background_ack && s.background_downloads) {
        EndAction::Remove => engine.remove(id, true).await,
        EndAction::Pause if entry.state != TorrentState::Paused => engine.pause(id).await,
        EndAction::Pause | EndAction::Keep => Ok(()),
    };
    if let Err(e) = result {
        tracing::warn!(torrent = %id, error = %e, "after-watch policy failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tk_ui::backend::{AfterWatchVm, TORRENT_MAX_LIMIT_KIB, TORRENT_MAX_PEERS};

    #[test]
    fn round_trip_defaults_and_atomic_write() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(tmp.path());
        let fresh = load(&paths);
        assert_eq!(TorrentSettingsVm { default_download_dir: String::new(), ..fresh.clone() }, TorrentSettingsVm::default());
        assert!(!fresh.default_download_dir.is_empty());

        let s = TorrentSettingsVm {
            download_dir: "  /data/anime ".into(),
            download_limit_kib: 2048,
            listen_port: 51413,
            after_watch: AfterWatchVm::Keep,
            background_ack: true,
            background_downloads: true,
            ..fresh.clone()
        };
        save(&paths, &s).unwrap();
        let back = load(&paths);
        assert_eq!(back.download_dir, "/data/anime");
        assert_eq!((back.download_limit_kib, back.listen_port, back.after_watch), (2048, 51413, AfterWatchVm::Keep));
        assert!(back.background_ack && back.background_downloads);
        assert_eq!(back.default_download_dir, fresh.default_download_dir);
        assert!(!paths.torrent_settings_file().with_extension("json.tmp").exists());
        // `default_download_dir` is display-only and never written
        let text = std::fs::read_to_string(paths.torrent_settings_file()).unwrap();
        assert!(!text.contains("default_download_dir"));
    }

    #[test]
    fn corrupt_or_out_of_range_files_are_repaired() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(tmp.path());
        std::fs::write(paths.torrent_settings_file(), "{nope").unwrap();
        assert_eq!(read_file(&paths.torrent_settings_file()), TorrentSettingsVm::default());
        std::fs::write(
            paths.torrent_settings_file(),
            r#"{"max_peers":100000,"upload_limit_kib":99999999,"background_downloads":true}"#,
        )
        .unwrap();
        let s = load(&paths);
        assert_eq!((s.max_peers, s.upload_limit_kib), (TORRENT_MAX_PEERS, TORRENT_MAX_LIMIT_KIB));
        // never acknowledged -> background downloads stay off
        assert!(!s.background_downloads);
    }

    #[test]
    fn warning_acceptance_is_one_way() {
        let stored = TorrentSettingsVm { background_ack: true, ..TorrentSettingsVm::default() };
        let stale = TorrentSettingsVm { background_downloads: true, ..TorrentSettingsVm::default() };
        let next = merge_for_save(&stored, &stale);
        assert!(next.background_ack && next.background_downloads);
        let never = merge_for_save(&TorrentSettingsVm::default(), &stale);
        assert!(!never.background_ack && !never.background_downloads);
    }

    #[test]
    fn listen_port_bounds() {
        assert!(listen_port_ok(0) && listen_port_ok(1024) && listen_port_ok(65535));
        assert!(!listen_port_ok(1) && !listen_port_ok(1023));
    }

    #[test]
    fn after_watch_policy() {
        assert_eq!(end_action(false, false), EndAction::Remove);
        assert_eq!(end_action(false, true), EndAction::Remove);
        assert_eq!(end_action(true, false), EndAction::Pause);
        assert_eq!(end_action(true, true), EndAction::Keep);
    }
}
