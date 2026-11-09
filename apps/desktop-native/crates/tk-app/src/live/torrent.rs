//! Torrent engine wiring (docs/redesign/10 section 4.1). Owner: wave 3 A.
//!
//! `TorrentService` owns the `tk_torrent::TorrentEngine` of the app: started on demand (first `engine()` call: playing
//! a torrent, opening the torrent client, or at start-up when background downloads are enabled), configured from
//! `TorrentSettingsVm` (`torrent_settings::load`) and shut down cleanly (Drop). It also maps the engine types to the
//! UI view models.
//!
//! The engine runs on its own small multi-thread tokio runtime (built on first use): the backend is created before
//! Dioxus starts its runtime, so this is what lets background downloads start with the app, and librqbit's torrent
//! tasks do not depend on the UI runtime. Engine calls hop onto that runtime by themselves.
//!
//! Polled reads of one torrent (`stats`, `files`, `peers`, `trackers`, `seek_marks`) never start the engine (a
//! failing start is not retried every second); the Library and every action do.
//!
//! Wave A (worker i; docs/redesign/15 section 4):
//! * **Session history** (`torrent_sessions`, checklist `torrent.session-history`): a watcher task per engine listens
//!   to its events. A row opens when a torrent enters the engine (`Added`) or a stream of it opens (`StreamOpened`,
//!   restored torrents played again); it closes when the torrent leaves (`Removed`), when the player ends its
//!   session (`close_session`, from `torrent_settings::playback_ended`), when no stream has been open for
//!   `SESSION_IDLE_CLOSE`, or when the engine stops. Rows a crashed run left open close at the next engine start.
//! * **Custom trackers** (`TorrentSettingsVm::trackers`) are announced for every torrent on top of the public ones.
//! * **Bandwidth schedule**: every minute the watcher applies `effective_limits` (the schedule's limits inside its
//!   daily window, the base limits outside) with `set_limits` when they change.
//! * **Cache limit**: every minute, when the torrents' data passes `cache_limit_gib`, the oldest torrents that are
//!   not kept and not streaming are removed with their files until it fits again.
//! * **Trackers**: the status column is the real tracker answer (`tk_torrent::TorrentEngine::trackers` probes every
//!   tracker with an announce / scrape); `reannounce` asks them again now.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};
use std::time::{Duration, Instant};

use tk_core::LocalStore;
use tk_core::store::torrents::TorrentSessionRow;
use tk_torrent::{
    EngineConfig, LibraryEntry, Limits, PeerInfo, TorrentEngine, TorrentError, TorrentEvent, TorrentFile, TorrentState,
    TorrentStats, TrackerInfo, TrackerStatus,
};
use tokio::sync::broadcast::error::RecvError;
use tk_ui::backend::{
    TORRENT_MAX_PEERS, TorrentFileVm, TorrentLibraryItemVm, TorrentPeerVm, TorrentProtocolVm, TorrentSettingsVm,
    TorrentStateVm, TorrentStatsVm, TorrentTrackerVm, TrackerStatusVm, UiResult,
};
use tk_ui::model::{ErrorKind, UiError};

use super::torrent_settings;
use crate::paths::AppPaths;

/// Public trackers announced for every torrent on top of its own (anime releases are public; bare info-hashes and
/// tracker-less magnets find peers faster with them).
const DEFAULT_TRACKERS: [&str; 5] = [
    "http://nyaa.tracker.wf:7777/announce",
    "udp://tracker.opentrackr.org:1337/announce",
    "udp://open.stealth.si:80/announce",
    "udp://exodus.desync.com:6969/announce",
    "udp://tracker.torrent.eu.org:451/announce",
];

/// Longest the app waits for the engine to stop when the service goes away.
const SHUTDOWN_WAIT: Duration = Duration::from_secs(2);
/// Period of the watcher's housekeeping (bandwidth schedule, cache limit, idle sessions).
const TICK: Duration = Duration::from_secs(60);
/// A session whose torrent has had no open stream for this long is closed (mpv reconnects between range requests,
/// so a stream closing is not the end of the playback).
const SESSION_IDLE_CLOSE: Duration = Duration::from_secs(90);
const GIB: u64 = 1024 * 1024 * 1024;

pub struct TorrentService {
    state: Arc<ServiceState>,
}

struct ServiceState {
    paths: AppPaths,
    /// The engine's runtime, built on first use.
    runtime: Mutex<Option<tokio::runtime::Runtime>>,
    engine: Mutex<Option<Arc<TorrentEngine>>>,
    /// Serialises starts / restarts.
    starting: tokio::sync::Mutex<()>,
    /// Second connection to `data.db` for the session history (opened on first use; `None` when it cannot open).
    store: OnceLock<Option<Arc<LocalStore>>>,
    /// Limits last given to the running engine (the schedule only calls `set_limits` on a change).
    applied: Mutex<Option<Limits>>,
    /// Last `(downloaded, uploaded)` seen per torrent, for rows closed after the torrent left the engine.
    bytes: Mutex<HashMap<String, (u64, u64)>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn background_on(settings: &TorrentSettingsVm) -> bool {
    settings.background_ack && settings.background_downloads
}

fn not_running() -> UiError {
    UiError::new(ErrorKind::NoSource, "the torrent engine is not running")
}

impl TorrentService {
    /// Cheap: nothing starts until `engine()`, unless background downloads are enabled (then the engine starts
    /// right away, in the background, to continue the saved downloads).
    pub fn new(paths: AppPaths) -> Self {
        let settings = torrent_settings::load(&paths);
        let state = Arc::new(ServiceState {
            paths,
            runtime: Mutex::new(None),
            engine: Mutex::new(None),
            starting: tokio::sync::Mutex::new(()),
            store: OnceLock::new(),
            applied: Mutex::new(None),
            bytes: Mutex::new(HashMap::new()),
        });
        if background_on(&settings) {
            state.start_in_background();
        }
        Self { state }
    }

    /// The running engine, starting it on first use with `engine_config(&torrent_settings::load(..), ..)`. A failed
    /// start is returned as a `UiError`; the next call tries again.
    pub async fn engine(&self) -> UiResult<Arc<TorrentEngine>> {
        self.state.engine().await
    }

    /// The engine if it is already running (never starts it): stats polls of a closed client cost nothing.
    pub fn running(&self) -> Option<Arc<TorrentEngine>> {
        self.state.running()
    }

    /// Apply saved settings: limits live (`set_limits`); port / DHT / LSD / UPnP / folder / peer changes restart the
    /// engine when no stream is open, else on the next start. Turning background downloads on starts the engine.
    pub async fn apply_settings(&self, settings: &TorrentSettingsVm) -> UiResult<()> {
        let settings = settings.clone().sanitized();
        let Some(engine) = self.running() else {
            if background_on(&settings) {
                self.state.start_in_background();
            }
            return Ok(());
        };
        let next = engine_config(&settings, &self.state.paths);
        self.state.apply_limits(&engine, &settings);
        if !needs_restart(engine.config(), &next) {
            return Ok(());
        }
        let streaming = engine.list().iter().any(|e| engine.stats(&e.id).is_ok_and(|s| s.streaming));
        if streaming {
            tracing::info!("torrent settings changed while streaming; they apply on the next engine start");
            return Ok(());
        }
        {
            let _starting = self.state.starting.lock().await;
            let old = lock(&self.state.engine).take();
            if let Some(old) = old {
                tracing::info!("restarting the torrent engine with the new settings");
                self.state.close_all_sessions(&old);
                old.shutdown().await;
            }
        }
        if background_on(&settings) {
            self.state.start_in_background();
        }
        Ok(())
    }

    pub async fn stats(&self, id: &str) -> UiResult<TorrentStatsVm> {
        let engine = self.running().ok_or_else(not_running)?;
        engine.stats(id).map(stats_vm).map_err(engine_err)
    }

    pub async fn files(&self, id: &str) -> UiResult<Vec<TorrentFileVm>> {
        let engine = self.running().ok_or_else(not_running)?;
        Ok(engine.files(id).map_err(engine_err)?.into_iter().map(file_vm).collect())
    }

    pub async fn peers(&self, id: &str) -> UiResult<Vec<TorrentPeerVm>> {
        let engine = self.running().ok_or_else(not_running)?;
        Ok(engine.peers(id).map_err(engine_err)?.into_iter().map(peer_vm).collect())
    }

    pub async fn trackers(&self, id: &str) -> UiResult<Vec<TorrentTrackerVm>> {
        let engine = self.running().ok_or_else(not_running)?;
        Ok(engine.trackers(id).map_err(engine_err)?.into_iter().map(tracker_vm).collect())
    }

    /// Newest first.
    pub async fn library(&self) -> UiResult<Vec<TorrentLibraryItemVm>> {
        let engine = self.engine().await?;
        Ok(engine.list().into_iter().map(library_vm).collect())
    }

    pub async fn remove(&self, id: &str, delete_files: bool) -> UiResult<()> {
        self.engine().await?.remove(id, delete_files).await.map_err(engine_err)
    }

    pub async fn pause(&self, id: &str) -> UiResult<()> {
        self.engine().await?.pause(id).await.map_err(engine_err)
    }

    pub async fn resume(&self, id: &str) -> UiResult<()> {
        self.engine().await?.resume(id).await.map_err(engine_err)
    }

    pub async fn set_persist(&self, id: &str, persist: bool) -> UiResult<()> {
        self.engine().await?.set_persist(id, persist).map_err(engine_err)
    }

    /// `xdg-open <save_path>` on Linux, `explorer <save_path>` on Windows (no shell, no console window).
    pub async fn open_folder(&self, id: &str) -> UiResult<()> {
        let dir = self.engine().await?.save_path(id).map_err(engine_err)?;
        // Nothing written yet: open the (empty) folder rather than fail.
        let _ = std::fs::create_dir_all(&dir);
        open_in_file_manager(&dir)
    }

    /// Ask every tracker of the torrent again (Trackers "Reannounce"); waits for the answers (at most 15 s).
    pub async fn reannounce(&self, id: &str) -> UiResult<()> {
        let engine = self.engine().await?;
        engine.reannounce(id).await.map_err(engine_err)
    }

    /// Close the open history row of `id` now (the player left it). Nothing happens without an open row.
    pub fn close_session(&self, id: &str) {
        if let Some(engine) = self.running() {
            self.state.close_session(&engine, id);
        }
    }

    /// Downloaded ranges of the file for the seek bar; empty while the engine is not running.
    pub async fn seek_marks(&self, id: &str, file_index: usize) -> UiResult<Vec<(f64, f64)>> {
        match self.running() {
            Some(engine) => engine.piece_bitfield(id, file_index).map_err(engine_err),
            None => Ok(Vec::new()),
        }
    }
}

impl ServiceState {
    fn running(&self) -> Option<Arc<TorrentEngine>> {
        lock(&self.engine).clone()
    }

    /// The engine runtime's handle (building the runtime on first use).
    fn runtime(&self) -> UiResult<tokio::runtime::Handle> {
        let mut slot = lock(&self.runtime);
        if let Some(rt) = slot.as_ref() {
            return Ok(rt.handle().clone());
        }
        let workers = std::thread::available_parallelism().map(|n| n.get().clamp(2, 4)).unwrap_or(2);
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(workers)
            .thread_name("tk-torrent")
            .enable_all()
            .build()
            .map_err(|e| UiError::new(ErrorKind::Other, format!("torrent runtime: {e}")))?;
        let handle = rt.handle().clone();
        *slot = Some(rt);
        Ok(handle)
    }

    async fn engine(self: &Arc<Self>) -> UiResult<Arc<TorrentEngine>> {
        if let Some(engine) = self.running() {
            return Ok(engine);
        }
        let _starting = self.starting.lock().await;
        if let Some(engine) = self.running() {
            return Ok(engine);
        }
        let settings = torrent_settings::load(&self.paths);
        let config = engine_config(&settings, &self.paths);
        let rt = self.runtime()?;
        let engine = rt
            .spawn(TorrentEngine::start(config))
            .await
            .map_err(|e| UiError::new(ErrorKind::Other, format!("torrent engine start: {e}")))?
            .map_err(|e| {
                tracing::warn!(error = %e, "torrent engine did not start");
                engine_err(e)
            })?;
        let engine = Arc::new(engine);
        *lock(&self.engine) = Some(engine.clone());
        *lock(&self.applied) = Some(engine.config().limits);
        rt.spawn(watch(Arc::downgrade(self), engine.clone()));
        Ok(engine)
    }

    /// The history store (`data.db`), opened once.
    fn store(&self) -> Option<Arc<LocalStore>> {
        self.store
            .get_or_init(|| match LocalStore::open(&self.paths.db_file()) {
                Ok(store) => Some(Arc::new(store)),
                Err(e) => {
                    tracing::warn!(error = %e, "torrent session history disabled: cannot open data.db");
                    None
                }
            })
            .clone()
    }

    fn is_current(&self, engine: &Arc<TorrentEngine>) -> bool {
        self.running().is_some_and(|e| Arc::ptr_eq(&e, engine))
    }

    /// `set_limits` with the limits that apply now (schedule), when they differ from the last ones applied.
    fn apply_limits(&self, engine: &TorrentEngine, settings: &TorrentSettingsVm) {
        let next = effective_limits(settings, local_hour());
        let mut applied = lock(&self.applied);
        if *applied == Some(next) {
            return;
        }
        match engine.set_limits(next) {
            Ok(()) => {
                tracing::info!(down = ?next.download_bps, up = ?next.upload_bps, "torrent speed limits applied");
                *applied = Some(next);
            }
            Err(e) => tracing::warn!(error = %e, "applying the torrent speed limits failed"),
        }
    }

    /// Open (or refresh) the history row of a torrent that entered the engine or started streaming.
    fn open_session(&self, engine: &TorrentEngine, id: &str) {
        let Some(store) = self.store() else { return };
        let Some(entry) = engine.list().into_iter().find(|e| e.id == id) else { return };
        let (downloaded, uploaded) = engine.stats(id).map(|s| (s.downloaded_bytes, s.uploaded_bytes)).unwrap_or((0, 0));
        let row = session_row(&entry, downloaded, uploaded, now_unix());
        if let Err(e) = store.open_torrent_session(&row) {
            tracing::warn!(torrent = %id, error = %e, "opening a torrent session row failed");
        }
    }

    /// Close the open history row of `id` with its counters (the last ones seen when it already left the engine).
    fn close_session(&self, engine: &TorrentEngine, id: &str) {
        let Some(store) = self.store() else { return };
        let (downloaded, uploaded, completed) = match engine.stats(id) {
            Ok(s) => (s.downloaded_bytes, s.uploaded_bytes, s.progress >= 1.0),
            Err(_) => {
                let (d, u) = lock(&self.bytes).get(id).copied().unwrap_or((0, 0));
                (d, u, false)
            }
        };
        if let Err(e) = store.close_torrent_session(id, now_unix(), downloaded, uploaded, completed) {
            tracing::warn!(torrent = %id, error = %e, "closing a torrent session row failed");
        }
    }

    /// Engine stopping: close every row still open.
    fn close_all_sessions(&self, engine: &TorrentEngine) {
        let Some(store) = self.store() else { return };
        for id in store.open_torrent_session_ids().unwrap_or_default() {
            self.close_session(engine, &id);
        }
    }

    fn on_event(&self, engine: &TorrentEngine, event: TorrentEvent, idle: &mut HashMap<String, Instant>) {
        match event {
            TorrentEvent::Added { id } => self.open_session(engine, &id),
            TorrentEvent::StreamOpened { id, .. } => {
                idle.remove(&id);
                self.open_session(engine, &id);
            }
            TorrentEvent::StreamClosed { id, .. } => {
                if !engine.stats(&id).is_ok_and(|s| s.streaming) {
                    idle.entry(id).or_insert_with(Instant::now);
                }
            }
            TorrentEvent::Removed { id } => {
                idle.remove(&id);
                self.close_session(engine, &id);
                lock(&self.bytes).remove(&id);
            }
            _ => {}
        }
    }

    /// Housekeeping every `TICK`: counters, idle sessions, bandwidth schedule, cache limit.
    async fn on_tick(&self, engine: &TorrentEngine, idle: &mut HashMap<String, Instant>) {
        let list = engine.list();
        {
            let mut bytes = lock(&self.bytes);
            for e in &list {
                if let Ok(s) = engine.stats(&e.id) {
                    bytes.insert(e.id.clone(), (s.downloaded_bytes, s.uploaded_bytes));
                }
            }
        }
        let expired: Vec<String> = idle.iter().filter(|(_, at)| at.elapsed() >= SESSION_IDLE_CLOSE).map(|(id, _)| id.clone()).collect();
        for id in expired {
            idle.remove(&id);
            if !engine.stats(&id).is_ok_and(|s| s.streaming) {
                self.close_session(engine, &id);
            }
        }
        let settings = torrent_settings::load(&self.paths);
        self.apply_limits(engine, &settings);
        let usage: Vec<CacheUsage> = list
            .iter()
            .map(|e| CacheUsage {
                id: e.id.clone(),
                bytes: engine.stats(&e.id).map(|s| s.downloaded_bytes).unwrap_or((e.size_bytes as f64 * e.progress) as u64),
                added_unix: e.added_unix,
                persist: e.persist,
                streaming: engine.stats(&e.id).is_ok_and(|s| s.streaming),
            })
            .collect();
        for id in over_cache_limit(&usage, u64::from(settings.cache_limit_gib) * GIB) {
            tracing::info!(torrent = %id, limit_gib = settings.cache_limit_gib, "torrent cache over its limit: removing");
            if let Err(e) = engine.remove(&id, true).await {
                tracing::warn!(torrent = %id, error = %e, "removing a torrent over the cache limit failed");
            }
        }
    }

    fn start_in_background(self: &Arc<Self>) {
        let rt = match self.runtime() {
            Ok(rt) => rt,
            Err(e) => {
                tracing::warn!(error = %e.detail, "cannot start the torrent engine for background downloads");
                return;
            }
        };
        let state = self.clone();
        rt.spawn(async move {
            if let Err(e) = state.engine().await {
                tracing::warn!(error = %e.detail, "torrent engine did not start for background downloads");
            }
        });
    }
}

impl Drop for ServiceState {
    fn drop(&mut self) {
        let engine = self.engine.get_mut().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(engine) = &engine {
            self.close_all_sessions(engine);
        }
        let Some(rt) = self.runtime.get_mut().unwrap_or_else(|e| e.into_inner()).take() else {
            return;
        };
        if let Some(engine) = engine {
            let (tx, rx) = std::sync::mpsc::channel();
            rt.spawn(async move {
                engine.shutdown().await;
                let _ = tx.send(());
            });
            // Never hold the caller (possibly the UI thread) for long.
            let _ = rx.recv_timeout(SHUTDOWN_WAIT);
        }
        rt.shutdown_background();
    }
}

/// The engine's watcher (one per engine run; ends when the engine is replaced or the service goes away). Holds only
/// a `Weak` so the service can still drop and shut the engine down.
async fn watch(state: Weak<ServiceState>, engine: Arc<TorrentEngine>) {
    let mut events = engine.events();
    if let Some(s) = state.upgrade()
        && let Some(store) = s.store()
    {
        match store.close_stale_torrent_sessions(&[], now_unix()) {
            Ok(0) => {}
            Ok(n) => tracing::info!(rows = n, "closed torrent session rows left open by the previous run"),
            Err(e) => tracing::warn!(error = %e, "closing stale torrent session rows failed"),
        }
    }
    let mut idle: HashMap<String, Instant> = HashMap::new();
    let mut tick = tokio::time::interval(TICK);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            event = events.recv() => match event {
                Ok(event) => {
                    let Some(s) = state.upgrade() else { return };
                    s.on_event(&engine, event, &mut idle);
                }
                Err(RecvError::Lagged(n)) => tracing::debug!(skipped = n, "torrent watcher lagged behind the events"),
                Err(RecvError::Closed) => return,
            },
            _ = tick.tick() => {
                let Some(s) = state.upgrade() else { return };
                if !s.is_current(&engine) {
                    return;
                }
                s.on_tick(&engine, &mut idle).await;
            }
        }
    }
}

fn now_unix() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

/// History row for a torrent of the engine (label = series / episode when it was added from a title).
fn session_row(entry: &LibraryEntry, downloaded: u64, uploaded: u64, now: i64) -> TorrentSessionRow {
    let label = entry.label.clone().unwrap_or_default();
    let non_empty = |s: String| (!s.trim().is_empty()).then_some(s);
    TorrentSessionRow {
        id: 0,
        torrent_id: entry.id.clone(),
        info_hash: entry.id.clone(),
        name: entry.name.clone(),
        media_id: non_empty(label.media_id),
        series: non_empty(label.title),
        episode: label.episode,
        started_at: now,
        ended_at: None,
        downloaded_bytes: downloaded,
        uploaded_bytes: uploaded,
        completed: entry.progress >= 1.0,
    }
}

/// What the cache limit looks at for one torrent.
#[derive(Debug, Clone)]
struct CacheUsage {
    id: String,
    bytes: u64,
    added_unix: i64,
    persist: bool,
    streaming: bool,
}

/// Torrents to remove so the data fits in `limit` bytes (0 = no limit): the oldest first, never a kept or a streaming
/// one. When only kept / streaming torrents are left the cache stays over the limit.
fn over_cache_limit(usage: &[CacheUsage], limit: u64) -> Vec<String> {
    if limit == 0 {
        return Vec::new();
    }
    let mut total: u64 = usage.iter().map(|u| u.bytes).sum();
    if total <= limit {
        return Vec::new();
    }
    let mut removable: Vec<&CacheUsage> = usage.iter().filter(|u| !u.persist && !u.streaming).collect();
    removable.sort_by_key(|u| u.added_unix);
    let mut out = Vec::new();
    for u in removable {
        if total <= limit {
            break;
        }
        total = total.saturating_sub(u.bytes);
        out.push(u.id.clone());
    }
    out
}

/// Base limits, or the schedule's limits when `hour` (local) is inside its window. Unknown hour = base limits.
fn effective_limits(settings: &TorrentSettingsVm, hour: Option<u8>) -> Limits {
    let bps = |kib: u32| (kib > 0).then(|| kib.saturating_mul(1024));
    if hour.is_some_and(|h| settings.schedule.active_at(h)) {
        Limits { download_bps: bps(settings.schedule.download_limit_kib), upload_bps: bps(settings.schedule.upload_limit_kib) }
    } else {
        Limits { download_bps: bps(settings.download_limit_kib), upload_bps: bps(settings.upload_limit_kib) }
    }
}

/// The local hour (0..=23). Windows: `GetLocalTime`; elsewhere `date +%H` (the `time` crate cannot read the local
/// offset in a multi-threaded process on Unix).
fn local_hour() -> Option<u8> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::SYSTEMTIME;
        use windows_sys::Win32::System::SystemInformation::GetLocalTime;
        // SAFETY: GetLocalTime only writes the struct it is given.
        let mut st: SYSTEMTIME = unsafe { std::mem::zeroed() };
        unsafe { GetLocalTime(&mut st) };
        u8::try_from(st.wHour).ok().filter(|h| *h < 24)
    }
    #[cfg(not(windows))]
    {
        let out = std::process::Command::new("date").arg("+%H").output().ok()?;
        parse_hour(&String::from_utf8_lossy(&out.stdout))
    }
}

#[cfg_attr(windows, allow(dead_code))]
fn parse_hour(text: &str) -> Option<u8> {
    text.trim().parse::<u8>().ok().filter(|h| *h < 24)
}

/// Public trackers plus the user's own, without repeats (order kept: the user's last).
fn merged_trackers(custom: &[String]) -> Vec<String> {
    let mut out: Vec<String> = DEFAULT_TRACKERS.iter().map(|t| (*t).to_owned()).collect();
    for t in custom {
        let t = t.trim();
        if !t.is_empty() && !out.iter().any(|o| o.eq_ignore_ascii_case(t)) {
            out.push(t.to_owned());
        }
    }
    out
}

/// Settings that need a new session (everything but the live limits; PEX cannot be switched in librqbit).
fn needs_restart(current: &EngineConfig, next: &EngineConfig) -> bool {
    let comparable = EngineConfig { limits: next.limits, pex: next.pex, ..current.clone() };
    comparable != *next
}

/// `TorrentSettingsVm` -> engine config (empty `download_dir` = `AppPaths::default_torrent_download_dir`, KiB/s ->
/// B/s, 0 = unlimited, `persistence_dir` = `AppPaths::torrent_dir`).
pub fn engine_config(settings: &TorrentSettingsVm, paths: &AppPaths) -> EngineConfig {
    let folder = settings.download_dir.trim();
    let download_dir = if folder.is_empty() { paths.default_torrent_download_dir() } else { PathBuf::from(folder) };
    let bps = |kib: u32| (kib > 0).then(|| kib.saturating_mul(1024));
    EngineConfig {
        download_dir,
        persistence_dir: paths.torrent_dir(),
        listen_port: settings.listen_port,
        upnp: settings.upnp,
        http_port: 0,
        dht: settings.dht,
        pex: settings.pex,
        lsd: settings.lsd,
        persist_session: true,
        max_peers: settings.max_peers.clamp(1, TORRENT_MAX_PEERS),
        limits: Limits { download_bps: bps(settings.download_limit_kib), upload_bps: bps(settings.upload_limit_kib) },
        trackers: merged_trackers(&settings.trackers),
    }
}

/// Engine errors -> UI errors (`NotFound` / `NoSuchFile` -> `NoSource`, `Timeout` -> `Offline` (no peers), the rest
/// `Other`), keeping the message as detail.
pub fn engine_err(e: TorrentError) -> UiError {
    let kind = match &e {
        TorrentError::NotFound(_) | TorrentError::NoSuchFile { .. } => ErrorKind::NoSource,
        TorrentError::Timeout(_) => ErrorKind::Offline,
        _ => ErrorKind::Other,
    };
    UiError::new(kind, e.to_string())
}

fn open_in_file_manager(dir: &Path) -> UiResult<()> {
    #[cfg(windows)]
    let mut cmd = {
        use std::os::windows::process::CommandExt;
        let mut cmd = std::process::Command::new("explorer");
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        cmd
    };
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("open");
    #[cfg(not(any(windows, target_os = "macos")))]
    let mut cmd = std::process::Command::new("xdg-open");
    cmd.arg(dir);
    match cmd.spawn() {
        Ok(mut child) => {
            // Reap it (xdg-open exits once the file manager is up).
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            Ok(())
        }
        Err(e) => Err(UiError::new(ErrorKind::Other, format!("cannot open {}: {e}", dir.display()))),
    }
}

fn state_vm(s: TorrentState) -> TorrentStateVm {
    match s {
        TorrentState::Initializing => TorrentStateVm::Initializing,
        TorrentState::Downloading => TorrentStateVm::Downloading,
        TorrentState::Seeding => TorrentStateVm::Seeding,
        TorrentState::Paused => TorrentStateVm::Paused,
        TorrentState::Error => TorrentStateVm::Error,
    }
}

fn stats_vm(s: TorrentStats) -> TorrentStatsVm {
    TorrentStatsVm {
        info_hash: s.id.clone(),
        id: s.id,
        name: s.name,
        state: state_vm(s.state),
        error: s.error,
        progress: s.progress,
        downloaded_bytes: s.downloaded_bytes,
        uploaded_bytes: s.uploaded_bytes,
        total_bytes: s.selected_bytes,
        size_bytes: s.size_bytes,
        pieces: s.pieces,
        piece_bytes: s.piece_bytes,
        pieces_have: s.pieces_have,
        down_bps: s.down_bps,
        up_bps: s.up_bps,
        seeders: s.seeders,
        leechers: s.leechers,
        peers: s.peers_connected,
        eta_secs: s.eta_secs,
        streaming: s.streaming,
        elapsed_secs: s.elapsed_secs,
        protocol: TorrentProtocolVm {
            dht: s.protocol.dht,
            lsd: s.protocol.lsd,
            pex: s.protocol.pex,
            nat: s.protocol.nat,
            forwarding: s.protocol.forwarding,
            persisting: s.protocol.persisting,
            streaming: s.protocol.streaming,
        },
    }
}

pub(super) fn file_vm(f: TorrentFile) -> TorrentFileVm {
    let episode = tk_core::release::parse_release(&f.name).episodes.map(|e| e.0);
    TorrentFileVm {
        index: f.index,
        path: f.path,
        name: f.name,
        size_bytes: f.size_bytes,
        downloaded_bytes: f.downloaded_bytes,
        progress: f.progress,
        selected: f.selected,
        streams: f.streams,
        episode,
    }
}

fn peer_vm(p: PeerInfo) -> TorrentPeerVm {
    TorrentPeerVm {
        addr: p.addr,
        client: p.client,
        progress: p.progress,
        down_bps: p.down_bps,
        up_bps: p.up_bps,
        downloaded_bytes: p.downloaded_bytes,
        uploaded_bytes: p.uploaded_bytes,
        country: p.country,
        flags: p.flags,
    }
}

fn tracker_vm(t: TrackerInfo) -> TorrentTrackerVm {
    TorrentTrackerVm {
        url: t.url,
        status: match t.status {
            TrackerStatus::Working => TrackerStatusVm::Working,
            TrackerStatus::Updating => TrackerStatusVm::Updating,
            TrackerStatus::Failed => TrackerStatusVm::Failed,
            TrackerStatus::Unknown => TrackerStatusVm::Unknown,
        },
        message: t.message,
        downloaded: t.downloaded,
        seeders: t.seeders,
        leechers: t.leechers,
    }
}

fn library_vm(e: LibraryEntry) -> TorrentLibraryItemVm {
    let label = e.label.unwrap_or_default();
    let non_empty = |s: String| (!s.trim().is_empty()).then_some(s);
    TorrentLibraryItemVm {
        id: e.id,
        name: e.name,
        media_id: non_empty(label.media_id),
        series: non_empty(label.title),
        episode: label.episode,
        poster: label.poster,
        files: e.files,
        size_bytes: e.size_bytes,
        progress: e.progress,
        state: state_vm(e.state),
        added_unix: e.added_unix,
        persist: e.persist,
        path: e.save_path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_from_settings() {
        let paths = AppPaths::from_root("/data");
        let settings = TorrentSettingsVm { download_limit_kib: 512, max_peers: 9999, ..TorrentSettingsVm::default() };
        let cfg = engine_config(&settings, &paths);
        assert_eq!(cfg.download_dir, paths.default_torrent_download_dir());
        assert_eq!(cfg.persistence_dir, paths.torrent_dir());
        assert_eq!(cfg.limits.download_bps, Some(512 * 1024));
        assert_eq!(cfg.limits.upload_bps, None);
        assert_eq!(cfg.max_peers, TORRENT_MAX_PEERS);
        assert_eq!(cfg.http_port, 0);

        let custom = TorrentSettingsVm { download_dir: " /media/anime ".into(), ..TorrentSettingsVm::default() };
        assert_eq!(engine_config(&custom, &paths).download_dir, PathBuf::from("/media/anime"));
    }

    #[test]
    fn restart_only_for_session_settings() {
        let paths = AppPaths::from_root("/data");
        let base = engine_config(&TorrentSettingsVm::default(), &paths);
        let limits = engine_config(&TorrentSettingsVm { upload_limit_kib: 10, ..TorrentSettingsVm::default() }, &paths);
        assert!(!needs_restart(&base, &limits));
        let port = engine_config(&TorrentSettingsVm { listen_port: 51413, ..TorrentSettingsVm::default() }, &paths);
        assert!(needs_restart(&base, &port));
    }

    #[test]
    fn errors_map_to_kinds() {
        assert_eq!(engine_err(TorrentError::NotFound("x".into())).kind, ErrorKind::NoSource);
        assert_eq!(engine_err(TorrentError::NoSuchFile { id: "x".into(), index: 3 }).kind, ErrorKind::NoSource);
        assert_eq!(engine_err(TorrentError::Timeout("x".into())).kind, ErrorKind::Offline);
        assert_eq!(engine_err(TorrentError::Io("x".into())).kind, ErrorKind::Other);
    }

    #[test]
    fn custom_trackers_join_the_public_ones() {
        let paths = AppPaths::from_root("/data");
        let custom = TorrentSettingsVm {
            trackers: vec!["udp://my.tracker:1337/announce".into(), DEFAULT_TRACKERS[0].to_uppercase()],
            ..TorrentSettingsVm::default()
        };
        let cfg = engine_config(&custom, &paths);
        assert_eq!(cfg.trackers.len(), DEFAULT_TRACKERS.len() + 1);
        assert_eq!(cfg.trackers.last().map(String::as_str), Some("udp://my.tracker:1337/announce"));
        let base = engine_config(&TorrentSettingsVm::default(), &paths);
        assert!(needs_restart(&base, &cfg), "new trackers apply with a new session");
    }

    #[test]
    fn schedule_switches_limits_inside_its_window() {
        let mut s = TorrentSettingsVm { download_limit_kib: 1024, ..TorrentSettingsVm::default() };
        s.schedule = tk_ui::backend::BandwidthScheduleVm {
            enabled: true,
            start_hour: 22,
            end_hour: 7,
            download_limit_kib: 0,
            upload_limit_kib: 50,
        };
        let night = effective_limits(&s, Some(23));
        assert_eq!((night.download_bps, night.upload_bps), (None, Some(50 * 1024)));
        assert_eq!(effective_limits(&s, Some(3)), night);
        let day = effective_limits(&s, Some(12));
        assert_eq!((day.download_bps, day.upload_bps), (Some(1024 * 1024), None));
        assert_eq!(effective_limits(&s, None), day, "unknown hour keeps the base limits");
        s.schedule.enabled = false;
        assert_eq!(effective_limits(&s, Some(23)), day);
        assert_eq!(parse_hour(" 07\n"), Some(7));
        assert_eq!(parse_hour("24"), None);
    }

    #[test]
    fn cache_limit_removes_the_oldest_unkept_torrents() {
        let u = |id: &str, gib: u64, added: i64, persist: bool, streaming: bool| CacheUsage {
            id: id.into(),
            bytes: gib * GIB,
            added_unix: added,
            persist,
            streaming,
        };
        let usage = vec![
            u("kept", 10, 1, true, false),
            u("old", 3, 2, false, false),
            u("playing", 4, 3, false, true),
            u("new", 3, 4, false, false),
        ];
        assert!(over_cache_limit(&usage, 0).is_empty(), "0 = no limit");
        assert!(over_cache_limit(&usage, 30 * GIB).is_empty());
        assert_eq!(over_cache_limit(&usage, 18 * GIB), ["old"]);
        assert_eq!(over_cache_limit(&usage, 15 * GIB), ["old", "new"]);
        // kept and streaming torrents never go, even when still over
        assert_eq!(over_cache_limit(&usage, GIB), ["old", "new"]);
    }

    #[test]
    fn session_rows_carry_the_label() {
        let entry = LibraryEntry {
            id: "b".repeat(40),
            name: "[Group] Show - 03.mkv".into(),
            size_bytes: 10,
            files: 1,
            progress: 1.0,
            state: TorrentState::Seeding,
            added_unix: 5,
            persist: false,
            save_path: PathBuf::from("/dl"),
            label: Some(tk_torrent::TorrentLabel { media_id: "21".into(), title: "Show".into(), episode: Some(3), poster: None }),
        };
        let row = session_row(&entry, 7, 2, 100);
        assert_eq!((row.media_id.as_deref(), row.series.as_deref(), row.episode), (Some("21"), Some("Show"), Some(3)));
        assert_eq!((row.downloaded_bytes, row.uploaded_bytes, row.started_at, row.ended_at), (7, 2, 100, None));
        assert!(row.completed);
        let bare = session_row(&LibraryEntry { label: None, ..entry }, 0, 0, 1);
        assert_eq!((bare.media_id, bare.series), (None, None));
    }

    #[test]
    fn library_label_maps_to_series() {
        let entry = LibraryEntry {
            id: "a".repeat(40),
            name: "[Group] Show (Batch)".into(),
            size_bytes: 10,
            files: 12,
            progress: 0.5,
            state: TorrentState::Paused,
            added_unix: 5,
            persist: true,
            save_path: PathBuf::from("/dl/Show"),
            label: Some(tk_torrent::TorrentLabel { media_id: "21".into(), title: "Show".into(), episode: Some(4), poster: None }),
        };
        let vm = library_vm(entry);
        assert_eq!(vm.media_id.as_deref(), Some("21"));
        assert_eq!(vm.series.as_deref(), Some("Show"));
        assert_eq!(vm.episode, Some(4));
        assert_eq!(vm.state, TorrentStateVm::Paused);
        assert_eq!(vm.files, 12);
    }
}
