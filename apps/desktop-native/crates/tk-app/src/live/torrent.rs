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

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tk_torrent::{
    EngineConfig, LibraryEntry, Limits, PeerInfo, TorrentEngine, TorrentError, TorrentFile, TorrentState, TorrentStats,
    TrackerInfo, TrackerStatus,
};
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
        engine.set_limits(next.limits).map_err(engine_err)?;
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

    async fn engine(&self) -> UiResult<Arc<TorrentEngine>> {
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
        Ok(engine)
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
        trackers: DEFAULT_TRACKERS.iter().map(|t| (*t).to_owned()).collect(),
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

fn file_vm(f: TorrentFile) -> TorrentFileVm {
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
