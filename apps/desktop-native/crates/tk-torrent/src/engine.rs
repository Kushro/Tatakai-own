//! [`TorrentEngine`]: one librqbit session + the loopback streaming server (docs/redesign/10 section 2).
//!
//! The engine is `Send + Sync` and used behind an `Arc` by tk-app (`live/torrent.rs`). Read methods are cheap
//! snapshots (no I/O, safe to call every second from the UI poll); methods that change the session are async and
//! run on the runtime the engine was started on (librqbit spawns its torrent tasks there), whatever runtime the
//! caller is on.
//!
//! Ids are lowercase hex v1 info-hashes; librqbit's own numeric ids never leave this module.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use librqbit::api::TorrentIdOrHash;
use librqbit::dht::DhtPersistenceConfig;
use librqbit::file_info::FileInfo;
use librqbit::limits::LimitsConfig;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, Api, DhtSessionConfig, ListenerMode, ListenerOptions,
    ManagedTorrent, ManagedTorrentState, Session, SessionOptions, SessionPersistenceConfig, TorrentStatsState,
};
use librqbit_core::Id20;
use librqbit_core::magnet::Magnet;
use tokio::runtime::Handle;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use crate::error::TorrentError;
use crate::labels::{LabelRecord, LabelStore, now_unix};
use crate::stream;
use crate::types::{
    AddOptions, AddSource, EngineConfig, LibraryEntry, Limits, PeerInfo, ProtocolStatus, TorrentEvent, TorrentFile,
    TorrentId, TorrentLabel, TorrentState, TorrentStats, TrackerInfo, TrackerStatus,
};

/// Capacity of the events channel; slow receivers get `RecvError::Lagged` and keep going.
pub const EVENTS_CAPACITY: usize = 256;
/// Longest `add` waits for a magnet's metadata before failing with `TorrentError::Timeout`.
pub const METADATA_TIMEOUT_SECS: u64 = 90;

/// librqbit's limit on concurrent blocking sections (disk reads / writes, hashing). Every open HTTP stream also
/// holds one permit for its lifetime, so this is well above what a player opens at once.
const BLOCKING_THREADS: usize = 16;
/// How often the event monitor compares torrent states.
const MONITOR_INTERVAL: Duration = Duration::from_secs(1);
/// Peer speed samples closer than this reuse the previous rate (the UI polls about once a second).
const PEER_RATE_MIN_SAMPLE: Duration = Duration::from_millis(500);

pub struct TorrentEngine {
    config: EngineConfig,
    inner: Arc<Inner>,
}

/// Per-peer byte counters of the previous `peers()` call, to turn librqbit's totals into speeds.
#[derive(Debug, Clone, Copy)]
struct PeerRate {
    at: Instant,
    fetched: u64,
    uploaded: u64,
    down_bps: u64,
    up_bps: u64,
}

/// What the event monitor saw last time for one torrent.
#[derive(Debug, Clone)]
struct Seen {
    state: TorrentState,
    finished: bool,
    files_done: Vec<bool>,
}

/// Shared state of the engine (session, labels, stream counters), also the state of the HTTP server.
pub(crate) struct Inner {
    pub(crate) session: Arc<Session>,
    api: Api,
    /// The runtime the engine was started on: every session-changing call runs there.
    pub(crate) handle: Handle,
    pub(crate) events: broadcast::Sender<TorrentEvent>,
    config: EngineConfig,
    labels: LabelStore,
    /// Random per start; the first path segment of every stream URL.
    pub(crate) token: String,
    pub(crate) http_addr: SocketAddr,
    /// `(id, file index) -> open HTTP streams`.
    streams: Mutex<HashMap<(String, usize), u32>>,
    /// When each torrent was added (or restored) in this run.
    added: Mutex<HashMap<String, Instant>>,
    peer_rates: Mutex<HashMap<String, HashMap<String, PeerRate>>>,
    /// Torrents added with every file wanted only because the caller did not know the files yet (a magnet before
    /// its metadata) and not kept: their first `select_file` narrows the selection to that file.
    narrow_on_select: Mutex<HashSet<String>>,
    monitor: Mutex<HashMap<String, Seen>>,
    pub(crate) cancel: CancellationToken,
    stopped: AtomicBool,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn engine_err(e: anyhow::Error) -> TorrentError {
    TorrentError::Engine(format!("{e:#}"))
}

fn parse_hash(id: &str) -> Option<Id20> {
    if id.len() != 40 {
        return None;
    }
    Id20::from_str(id).ok()
}

/// `/`-separated path of a file inside the torrent.
pub(crate) fn file_path(fi: &FileInfo) -> String {
    fi.relative_filename.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")
}

/// Last component of [`file_path`].
pub(crate) fn file_name(fi: &FileInfo) -> String {
    fi.relative_filename
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| file_path(fi))
}

fn map_state(state: TorrentStatsState, finished: bool) -> TorrentState {
    match state {
        TorrentStatsState::Initializing { .. } => TorrentState::Initializing,
        TorrentStatsState::Live if finished => TorrentState::Seeding,
        TorrentStatsState::Live => TorrentState::Downloading,
        TorrentStatsState::Paused => TorrentState::Paused,
        TorrentStatsState::Error => TorrentState::Error,
    }
}

fn limits_config(limits: Limits) -> LimitsConfig {
    LimitsConfig {
        download_bps: limits.download_bps.and_then(NonZeroU32::new),
        upload_bps: limits.upload_bps.and_then(NonZeroU32::new),
    }
}

fn peer_limit(config: &EngineConfig) -> usize {
    usize::try_from(config.max_peers.max(1)).unwrap_or(usize::MAX)
}

/// librqbit session options for `config`, listening on `listen` (an IPv4 address = IPv4 only, the fallback).
fn session_options(config: &EngineConfig, listen: SocketAddr) -> SessionOptions {
    let ipv4_only = listen.is_ipv4();
    let dht = config.dht.then(|| DhtSessionConfig {
        bootstrap_addrs: None,
        port: None,
        persistence: config.persist_session.then(|| DhtPersistenceConfig {
            dump_interval: None,
            config_filename: Some(config.persistence_dir.join("dht.json")),
        }),
    });
    SessionOptions {
        dht,
        fastresume: config.persist_session,
        persistence: config
            .persist_session
            .then(|| SessionPersistenceConfig::Json { folder: Some(config.persistence_dir.clone()) }),
        listen: Some(ListenerOptions {
            mode: ListenerMode::TcpAndUtp,
            listen_addr: listen,
            enable_upnp_port_forwarding: config.upnp,
            ipv4_only,
            ..Default::default()
        }),
        ratelimits: limits_config(config.limits),
        trackers: config.trackers.iter().filter_map(|t| url::Url::parse(t.trim()).ok()).collect(),
        peer_limit: Some(peer_limit(config)),
        disable_local_service_discovery: !config.lsd,
        ipv4_only,
        runtime_worker_threads: Some(BLOCKING_THREADS),
        client_name_and_version: Some(format!("Tatakai/{} (rqbit {})", env!("CARGO_PKG_VERSION"), crate::LIBRQBIT_VERSION)),
        ..Default::default()
    }
}

impl TorrentEngine {
    /// Start the session (DHT, listeners, UPnP, persistence per `config`) and the HTTP streaming server on
    /// `127.0.0.1:<http_port>` with a fresh random token. Restores persisted torrents; torrents that were not kept
    /// (`persist == false`, left over from a run that ended mid-playback) are removed with their files.
    ///
    /// Needs a multi-thread tokio runtime (librqbit uses `block_in_place`); the engine's tasks live on it.
    pub async fn start(config: EngineConfig) -> Result<Self, TorrentError> {
        let handle = Handle::try_current().map_err(|_| TorrentError::Unavailable("no tokio runtime".into()))?;
        std::fs::create_dir_all(&config.download_dir)
            .map_err(|e| TorrentError::Io(format!("{}: {e}", config.download_dir.display())))?;
        std::fs::create_dir_all(&config.persistence_dir)
            .map_err(|e| TorrentError::Io(format!("{}: {e}", config.persistence_dir.display())))?;

        // The configured port first; a taken port or a host without IPv6 falls back to a random IPv4 port.
        let preferred = SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), config.listen_port);
        let session = match Session::new_with_opts(config.download_dir.clone(), session_options(&config, preferred)).await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), port = config.listen_port, "torrent session did not start; retrying on a random IPv4 port");
                let fallback = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0);
                Session::new_with_opts(config.download_dir.clone(), session_options(&config, fallback))
                    .await
                    .map_err(|e| TorrentError::Unavailable(format!("{e:#}")))?
            }
        };

        let listener = match stream::bind(config.http_port).await {
            Ok(l) => l,
            Err(e) => {
                session.stop().await;
                return Err(TorrentError::Io(format!("streaming server: {e}")));
            }
        };
        let http_addr = match listener.local_addr() {
            Ok(a) => a,
            Err(e) => {
                session.stop().await;
                return Err(TorrentError::Io(format!("streaming server: {e}")));
            }
        };

        let (events, _) = broadcast::channel(EVENTS_CAPACITY);
        let inner = Arc::new(Inner {
            api: Api::new(session.clone(), None),
            session,
            handle,
            events,
            labels: LabelStore::load(config.persistence_dir.join("labels.json")),
            config: config.clone(),
            token: uuid::Uuid::new_v4().simple().to_string(),
            http_addr,
            streams: Mutex::new(HashMap::new()),
            added: Mutex::new(HashMap::new()),
            peer_rates: Mutex::new(HashMap::new()),
            narrow_on_select: Mutex::new(HashSet::new()),
            monitor: Mutex::new(HashMap::new()),
            cancel: CancellationToken::new(),
            stopped: AtomicBool::new(false),
        });

        inner.reconcile_restored().await;
        stream::spawn_server(inner.clone(), listener);
        inner.spawn_monitor();
        tracing::info!(
            listen = ?inner.session.listen_addr(),
            http = %http_addr,
            dht = inner.session.get_dht().is_some(),
            torrents = inner.handles().len(),
            "torrent engine started"
        );
        Ok(Self { config, inner })
    }

    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// Run a session-changing operation on the engine's runtime (cancel-safe: dropping the caller's future lets the
    /// operation finish).
    async fn on_engine<F, T>(&self, f: impl FnOnce(Arc<Inner>) -> F) -> Result<T, TorrentError>
    where
        F: Future<Output = Result<T, TorrentError>> + Send + 'static,
        T: Send + 'static,
    {
        self.inner.ensure_running()?;
        self.inner
            .handle
            .spawn(f(self.inner.clone()))
            .await
            .map_err(|e| TorrentError::Engine(format!("engine task failed: {e}")))?
    }

    /// Add a torrent and wait for its metadata (bounded by [`METADATA_TIMEOUT_SECS`]). Adding a torrent that is
    /// already in the session returns its id (options are merged: `only_files` extended, `persist` OR-ed, `label`
    /// replaced when `Some`; a paused torrent resumes unless `paused`). Cancel-safe.
    ///
    /// A new torrent added with `only_files: None` and `persist: false` (a stream whose file is not known yet) is
    /// narrowed to the first file passed to [`Self::select_file`], so a season pack only downloads what is watched.
    pub async fn add(&self, source: AddSource, opts: AddOptions) -> Result<TorrentId, TorrentError> {
        self.on_engine(move |inner| async move { inner.add(source, opts).await }).await
    }

    /// Files of a torrent in index order (padding files of BEP 47 are left out; indices are librqbit's).
    pub fn files(&self, id: &str) -> Result<Vec<TorrentFile>, TorrentError> {
        self.inner.files(id)
    }

    /// Make `index` wanted (added to `only_files`) so it downloads completely; the pieces at the read position of
    /// an open stream come first anyway (librqbit stream priority). Other selected files stay selected. Used for
    /// "next episode" in a batch.
    pub async fn select_file(&self, id: &str, index: usize) -> Result<(), TorrentError> {
        let id = id.to_owned();
        self.on_engine(move |inner| async move {
            let handle = inner.get(&id)?;
            inner.select_files(&handle, &[index]).await
        })
        .await
    }

    /// `http://127.0.0.1:<port>/<token>/<id>/<index>/<url-encoded file name>`. The server answers `Range`
    /// requests; a read selects the file and prioritises the pieces from the read position onwards (librqbit
    /// `FileStream`), so mpv plays while downloading. Works for every player mode (embedded, enhanced, external mpv).
    pub fn stream_url(&self, id: &str, file_index: usize) -> Result<String, TorrentError> {
        let handle = self.inner.get(id)?;
        let meta = handle.metadata.load_full().ok_or_else(|| TorrentError::Unavailable("metadata not known yet".into()))?;
        let fi = meta
            .file_infos
            .get(file_index)
            .filter(|fi| !fi.attrs.padding)
            .ok_or_else(|| TorrentError::NoSuchFile { id: id.to_owned(), index: file_index })?;
        let mut url = url::Url::parse(&format!("http://{}/", self.inner.http_addr))
            .map_err(|e| TorrentError::Engine(format!("stream url: {e}")))?;
        {
            let mut segments = url.path_segments_mut().map_err(|()| TorrentError::Engine("stream url has no path".into()))?;
            segments.pop_if_empty();
            segments.push(&self.inner.token);
            segments.push(&handle.info_hash().as_string());
            segments.push(&file_index.to_string());
            segments.push(&file_name(fi));
        }
        Ok(url.to_string())
    }

    pub fn stats(&self, id: &str) -> Result<TorrentStats, TorrentError> {
        self.inner.stats(id)
    }

    /// Downloaded ranges of one file as sorted, merged `(start, end)` fractions of the file (0.0..=1.0) for the
    /// seek bar. A piece shared with the neighbour file counts for both.
    pub fn piece_bitfield(&self, id: &str, file_index: usize) -> Result<Vec<(f64, f64)>, TorrentError> {
        self.inner.piece_bitfield(id, file_index)
    }

    /// Connected peers, fastest download first.
    pub fn peers(&self, id: &str) -> Result<Vec<PeerInfo>, TorrentError> {
        self.inner.peers(id)
    }

    pub fn trackers(&self, id: &str) -> Result<Vec<TrackerInfo>, TorrentError> {
        self.inner.trackers(id)
    }

    /// Every torrent of the session, newest first.
    pub fn list(&self) -> Vec<LibraryEntry> {
        self.inner.list()
    }

    /// Remove from the session (and the label file); `delete_files` also deletes the downloaded data.
    pub async fn remove(&self, id: &str, delete_files: bool) -> Result<(), TorrentError> {
        let id = id.to_owned();
        self.on_engine(move |inner| async move { inner.remove(&id, delete_files).await }).await
    }

    pub async fn pause(&self, id: &str) -> Result<(), TorrentError> {
        let id = id.to_owned();
        self.on_engine(move |inner| async move {
            let handle = inner.get(&id)?;
            if handle.is_paused() {
                return Ok(());
            }
            inner.session.pause(&handle).await.map_err(engine_err)?;
            inner.emit(TorrentEvent::StateChanged { id: handle.info_hash().as_string(), state: TorrentState::Paused });
            Ok(())
        })
        .await
    }

    pub async fn resume(&self, id: &str) -> Result<(), TorrentError> {
        let id = id.to_owned();
        self.on_engine(move |inner| async move { inner.resume(&id).await }).await
    }

    /// Session-wide limits, applied live.
    pub fn set_limits(&self, limits: Limits) -> Result<(), TorrentError> {
        self.inner.ensure_running()?;
        let cfg = limits_config(limits);
        self.inner.session.ratelimits.set_download_bps(cfg.download_bps);
        self.inner.session.ratelimits.set_upload_bps(cfg.upload_bps);
        Ok(())
    }

    /// Replace the app label of a torrent (persisted).
    pub fn set_label(&self, id: &str, label: TorrentLabel) -> Result<(), TorrentError> {
        let handle = self.inner.get(id)?;
        self.inner.labels.update(&handle.info_hash().as_string(), |r| r.label = Some(label));
        Ok(())
    }

    /// Change `AddOptions::persist` of a torrent (Library "keep").
    pub fn set_persist(&self, id: &str, persist: bool) -> Result<(), TorrentError> {
        let handle = self.inner.get(id)?;
        let id = handle.info_hash().as_string();
        self.inner.labels.update(&id, |r| r.persist = persist);
        if persist {
            lock(&self.inner.narrow_on_select).remove(&id);
        }
        Ok(())
    }

    /// Folder that holds the torrent's data (`LibraryEntry::save_path`).
    pub fn save_path(&self, id: &str) -> Result<PathBuf, TorrentError> {
        Ok(self.inner.get(id)?.output_folder().to_path_buf())
    }

    pub fn events(&self) -> broadcast::Receiver<TorrentEvent> {
        self.inner.events.subscribe()
    }

    /// Stop streams, flush persistence, cancel the session. Idempotent; the engine is unusable afterwards.
    pub async fn shutdown(&self) {
        if self.inner.stopped.swap(true, Ordering::SeqCst) {
            return;
        }
        self.inner.cancel.cancel();
        let session = self.inner.session.clone();
        // On the engine runtime: librqbit's `stop` pauses every torrent, then waits a second for its tasks.
        let stop = self.inner.handle.spawn(async move { session.stop().await });
        if tokio::time::timeout(Duration::from_secs(3), stop).await.is_err() {
            tracing::warn!("torrent session did not stop in time");
        }
        tracing::info!("torrent engine stopped");
    }
}

impl Drop for TorrentEngine {
    fn drop(&mut self) {
        // Stops the streaming server and the monitor; the session cancels its own tasks when its last Arc goes.
        self.inner.cancel.cancel();
    }
}

impl Inner {
    fn ensure_running(&self) -> Result<(), TorrentError> {
        if self.stopped.load(Ordering::SeqCst) {
            return Err(TorrentError::Unavailable("the torrent engine was shut down".into()));
        }
        Ok(())
    }

    pub(crate) fn emit(&self, event: TorrentEvent) {
        // Nobody listening is fine.
        let _ = self.events.send(event);
    }

    pub(crate) fn get(&self, id: &str) -> Result<Arc<ManagedTorrent>, TorrentError> {
        self.ensure_running()?;
        let hash = parse_hash(id).ok_or_else(|| TorrentError::NotFound(id.to_owned()))?;
        self.session.get(TorrentIdOrHash::Hash(hash)).ok_or_else(|| TorrentError::NotFound(id.to_owned()))
    }

    fn handles(&self) -> Vec<Arc<ManagedTorrent>> {
        self.session.with_torrents(|it| it.map(|(_, h)| h.clone()).collect::<Vec<_>>())
    }

    /// After a start: forget labels of torrents that are gone, label torrents that have none (kept), and remove
    /// torrents that were not kept (with their files): they belonged to a playback the previous run did not end.
    async fn reconcile_restored(&self) {
        let mut present = HashSet::new();
        for handle in self.handles() {
            let id = handle.info_hash().as_string();
            match self.labels.get(&id) {
                None => self.labels.set(&id, LabelRecord { persist: true, ..LabelRecord::new() }),
                Some(rec) if !rec.persist => {
                    tracing::info!(%id, "removing a torrent that was not kept after watching");
                    if let Err(e) = self.session.delete(TorrentIdOrHash::Hash(handle.info_hash()), true).await {
                        tracing::warn!(%id, error = %format!("{e:#}"), "cannot remove a leftover torrent");
                    }
                    self.labels.remove(&id);
                    continue;
                }
                Some(_) => {}
            }
            lock(&self.added).insert(id.clone(), Instant::now());
            present.insert(id);
        }
        for id in self.labels.ids() {
            if !present.contains(&id) {
                self.labels.remove(&id);
            }
        }
    }

    async fn add(self: &Arc<Self>, source: AddSource, opts: AddOptions) -> Result<TorrentId, TorrentError> {
        self.ensure_running()?;
        // A known hash already in the session: merge without resolving the magnet again.
        let known = match &source {
            AddSource::Magnet(m) => Some(
                Magnet::parse(m)
                    .map_err(|e| TorrentError::InvalidSource(format!("{e:#}")))?
                    .as_id20()
                    .ok_or_else(|| TorrentError::InvalidSource("magnet link without a v1 (btih) info-hash".into()))?,
            ),
            AddSource::InfoHash(h) => {
                Some(parse_hash(h).ok_or_else(|| TorrentError::InvalidSource(format!("not an info-hash: {h}")))?)
            }
            AddSource::TorrentBytes(_) | AddSource::Url(_) => None,
        };
        if let Some(hash) = known
            && let Some(handle) = self.session.get(TorrentIdOrHash::Hash(hash))
        {
            return self.merge_existing(&handle, opts).await;
        }

        let add = match source {
            AddSource::Magnet(m) => AddTorrent::from_url(m),
            AddSource::InfoHash(h) => AddTorrent::from_url(format!("magnet:?xt=urn:btih:{h}")),
            AddSource::TorrentBytes(b) => AddTorrent::from_bytes(b),
            AddSource::Url(u) => AddTorrent::from_url(u),
        };
        let rq_opts = AddTorrentOptions {
            paused: opts.paused,
            only_files: opts.only_files.clone(),
            // Resume / re-add over data already on disk (the same episode again, a kept batch).
            overwrite: true,
            peer_limit: Some(peer_limit(&self.config)),
            ..Default::default()
        };
        let session = self.session.clone();
        let response =
            match tokio::time::timeout(Duration::from_secs(METADATA_TIMEOUT_SECS), session.add_torrent(add, Some(rq_opts))).await {
                Ok(r) => r.map_err(engine_err)?,
                Err(_) => {
                    return Err(TorrentError::Timeout(format!(
                        "no torrent metadata after {METADATA_TIMEOUT_SECS} s (no peers?)"
                    )));
                }
            };
        let handle = match response {
            AddTorrentResponse::Added(_, h) => h,
            AddTorrentResponse::AlreadyManaged(_, h) => return self.merge_existing(&h, opts).await,
            AddTorrentResponse::ListOnly(_) => return Err(TorrentError::Engine("unexpected list-only answer".into())),
        };

        let id = handle.info_hash().as_string();
        let narrow = opts.only_files.is_none() && !opts.persist;
        let persist = opts.persist;
        self.labels.set(&id, LabelRecord { label: opts.label, persist, added_unix: now_unix() });
        lock(&self.added).insert(id.clone(), Instant::now());
        if narrow {
            lock(&self.narrow_on_select).insert(id.clone());
        }
        tracing::info!(%id, name = ?handle.name(), persist, "torrent added");
        self.emit(TorrentEvent::Added { id: id.clone() });
        self.emit(TorrentEvent::MetadataReady { id: id.clone() });
        Ok(id)
    }

    async fn merge_existing(self: &Arc<Self>, handle: &Arc<ManagedTorrent>, opts: AddOptions) -> Result<TorrentId, TorrentError> {
        let id = handle.info_hash().as_string();
        let AddOptions { only_files, paused, persist, label } = opts;
        self.labels.update(&id, |r| {
            r.persist |= persist;
            if let Some(label) = label {
                r.label = Some(label);
            }
        });
        if persist {
            lock(&self.narrow_on_select).remove(&id);
        }
        if let Some(files) = only_files.as_deref() {
            self.select_files(handle, files).await?;
        }
        if !paused && (handle.is_paused() || is_error(handle)) {
            self.resume(&id).await?;
        }
        Ok(id)
    }

    /// Add `files` to the torrent's selection (see `TorrentEngine::select_file`). Waits for the initial check.
    pub(crate) async fn select_files(&self, handle: &Arc<ManagedTorrent>, files: &[usize]) -> Result<(), TorrentError> {
        let id = handle.info_hash().as_string();
        let count = handle.metadata.load_full().map(|m| m.file_infos.len()).unwrap_or(0);
        if let Some(&bad) = files.iter().find(|&&f| f >= count) {
            return Err(TorrentError::NoSuchFile { id, index: bad });
        }
        let narrow = lock(&self.narrow_on_select).remove(&id);
        let wanted: HashSet<usize> = if narrow {
            files.iter().copied().collect()
        } else {
            match handle.only_files() {
                // Every file is wanted already.
                None => return Ok(()),
                Some(current) => {
                    let mut set: HashSet<usize> = current.into_iter().collect();
                    if files.iter().all(|f| set.contains(f)) {
                        return Ok(());
                    }
                    set.extend(files.iter().copied());
                    set
                }
            }
        };
        handle.wait_until_initialized().await.map_err(engine_err)?;
        self.session.update_only_files(handle, &wanted).await.map_err(engine_err)
    }

    async fn resume(&self, id: &str) -> Result<(), TorrentError> {
        let handle = self.get(id)?;
        if !handle.is_paused() && !is_error(&handle) {
            return Ok(());
        }
        self.session.unpause(&handle).await.map_err(engine_err)?;
        self.emit(TorrentEvent::StateChanged { id: handle.info_hash().as_string(), state: TorrentState::Initializing });
        Ok(())
    }

    async fn remove(&self, id: &str, delete_files: bool) -> Result<(), TorrentError> {
        let handle = self.get(id)?;
        let id = handle.info_hash().as_string();
        self.session.delete(TorrentIdOrHash::Hash(handle.info_hash()), delete_files).await.map_err(engine_err)?;
        self.labels.remove(&id);
        lock(&self.added).remove(&id);
        lock(&self.peer_rates).remove(&id);
        lock(&self.monitor).remove(&id);
        lock(&self.narrow_on_select).remove(&id);
        tracing::info!(%id, delete_files, "torrent removed");
        self.emit(TorrentEvent::Removed { id });
        Ok(())
    }

    fn open_streams(&self, id: &str) -> u32 {
        lock(&self.streams).iter().filter(|((t, _), _)| t == id).map(|(_, n)| *n).sum()
    }

    /// Count an HTTP stream as open until the returned guard drops.
    pub(crate) fn open_stream(self: &Arc<Self>, id: &str, index: usize) -> StreamGuard {
        *lock(&self.streams).entry((id.to_owned(), index)).or_insert(0) += 1;
        self.emit(TorrentEvent::StreamOpened { id: id.to_owned(), index });
        StreamGuard { inner: self.clone(), id: id.to_owned(), index }
    }

    fn files(&self, id: &str) -> Result<Vec<TorrentFile>, TorrentError> {
        let handle = self.get(id)?;
        let id = handle.info_hash().as_string();
        let Some(meta) = handle.metadata.load_full() else {
            return Ok(Vec::new());
        };
        let stats = handle.stats();
        let only = handle.only_files();
        let streams = lock(&self.streams);
        Ok(meta
            .file_infos
            .iter()
            .enumerate()
            .filter(|(_, fi)| !fi.attrs.padding)
            .map(|(index, fi)| {
                let have = stats.file_progress.get(index).copied().unwrap_or(0).min(fi.len);
                TorrentFile {
                    index,
                    path: file_path(fi),
                    name: file_name(fi),
                    size_bytes: fi.len,
                    downloaded_bytes: have,
                    progress: if fi.len == 0 { 1.0 } else { have as f64 / fi.len as f64 },
                    selected: only.as_ref().is_none_or(|o| o.contains(&index)),
                    streams: streams.get(&(id.clone(), index)).copied().unwrap_or(0),
                }
            })
            .collect())
    }

    /// Have-bitfield of the torrent (`None` while initializing / in error) and its piece count.
    fn have_pieces(&self, handle: &ManagedTorrent) -> Option<(Vec<bool>, u32)> {
        let (bf, total) = self.api.api_dump_haves(TorrentIdOrHash::Hash(handle.info_hash())).ok()?;
        let n = usize::try_from(total).unwrap_or(0);
        Some(((0..n).map(|i| bf.get(i).is_some_and(|b| *b)).collect(), total))
    }

    fn stats(&self, id: &str) -> Result<TorrentStats, TorrentError> {
        let handle = self.get(id)?;
        let id = handle.info_hash().as_string();
        let st = handle.stats();
        let meta = handle.metadata.load_full();
        let (pieces, piece_bytes, size_bytes) = meta
            .as_ref()
            .map(|m| (m.lengths().total_pieces(), u64::from(m.lengths().default_piece_length()), m.lengths().total_length()))
            .unwrap_or((0, 0, 0));
        let pieces_have = self
            .have_pieces(&handle)
            .map(|(have, _)| u32::try_from(have.iter().filter(|h| **h).count()).unwrap_or(u32::MAX))
            .unwrap_or(0);
        let state = map_state(st.state, st.finished);
        let (down_bps, up_bps, live_peers, seeders, known) = match st.live.as_ref() {
            Some(live) => (
                live.download_speed.as_bytes(),
                live.upload_speed.as_bytes(),
                live.snapshot.peer_stats.live,
                live.snapshot.peer_stats.live_seeders,
                live.snapshot.peer_stats.seen,
            ),
            None => (0, 0, 0, 0, 0),
        };
        let selected_bytes = st.total_bytes;
        let progress = fraction(st.progress_bytes, selected_bytes, st.finished);
        let remaining = selected_bytes.saturating_sub(st.progress_bytes);
        let eta_secs =
            (state == TorrentState::Downloading && down_bps > 0 && remaining > 0).then(|| remaining.div_ceil(down_bps));
        let streaming = self.open_streams(&id) > 0;
        let persist = self.labels.get(&id).is_some_and(|r| r.persist);
        let elapsed_secs = lock(&self.added).get(&id).map(|t| t.elapsed().as_secs()).unwrap_or(0);
        let listening = self.session.announce_port().is_some();
        Ok(TorrentStats {
            name: handle.name().unwrap_or_else(|| id.clone()),
            id,
            state,
            error: st.error.clone(),
            progress,
            downloaded_bytes: st.progress_bytes,
            uploaded_bytes: st.uploaded_bytes,
            selected_bytes,
            size_bytes,
            pieces,
            piece_bytes,
            pieces_have,
            down_bps,
            up_bps,
            seeders,
            leechers: live_peers.saturating_sub(seeders),
            peers_connected: live_peers,
            peers_known: known,
            eta_secs,
            streaming,
            elapsed_secs,
            protocol: ProtocolStatus {
                dht: self.session.get_dht().is_some(),
                lsd: self.config.lsd,
                // librqbit always speaks ut_pex; there is no switch to turn it off.
                pex: true,
                // librqbit does not report whether a gateway answered: UPnP on + listening is the best signal.
                nat: self.config.upnp && listening,
                forwarding: self.config.upnp && listening,
                persisting: persist && self.config.persist_session,
                streaming,
            },
        })
    }

    fn piece_bitfield(&self, id: &str, file_index: usize) -> Result<Vec<(f64, f64)>, TorrentError> {
        let handle = self.get(id)?;
        let Some(meta) = handle.metadata.load_full() else {
            return Ok(Vec::new());
        };
        let fi = meta
            .file_infos
            .get(file_index)
            .ok_or_else(|| TorrentError::NoSuchFile { id: id.to_owned(), index: file_index })?;
        let Some((have, total)) = self.have_pieces(&handle) else {
            return Ok(Vec::new());
        };
        Ok(file_ranges(fi, u64::from(meta.lengths().default_piece_length()), &have, total))
    }

    fn peers(&self, id: &str) -> Result<Vec<PeerInfo>, TorrentError> {
        let handle = self.get(id)?;
        let id = handle.info_hash().as_string();
        let Ok(snapshot) = self.api.api_peer_stats(TorrentIdOrHash::Hash(handle.info_hash()), Default::default()) else {
            // Not live (paused, initializing): nobody is connected.
            return Ok(Vec::new());
        };
        let now = Instant::now();
        let mut rates = lock(&self.peer_rates);
        let previous = rates.remove(&id).unwrap_or_default();
        let mut next = HashMap::new();
        let mut out = Vec::new();
        for (addr, ps) in snapshot.peers {
            let fetched = ps.counters.fetched_bytes;
            let uploaded = ps.counters.uploaded_bytes;
            let rate = match previous.get(&addr) {
                Some(prev) if now.duration_since(prev.at) < PEER_RATE_MIN_SAMPLE => *prev,
                Some(prev) => {
                    let secs = now.duration_since(prev.at).as_secs_f64();
                    PeerRate {
                        at: now,
                        fetched,
                        uploaded,
                        down_bps: (fetched.saturating_sub(prev.fetched) as f64 / secs) as u64,
                        up_bps: (uploaded.saturating_sub(prev.uploaded) as f64 / secs) as u64,
                    }
                }
                None => PeerRate { at: now, fetched, uploaded, down_bps: 0, up_bps: 0 },
            };
            next.insert(addr.clone(), rate);
            let mut flags = String::new();
            if rate.down_bps > 0 {
                flags.push('D');
            }
            if rate.up_bps > 0 {
                flags.push('U');
            }
            if ps.counters.incoming_connections > 0 {
                flags.push('I');
            }
            if ps.conn_kind.is_some_and(|k| k.to_string() == "utp") {
                flags.push('P');
            }
            out.push(PeerInfo {
                addr,
                client: ps.client_name,
                // librqbit keeps each peer's bitfield private: progress is not known.
                progress: 0.0,
                down_bps: rate.down_bps,
                up_bps: rate.up_bps,
                downloaded_bytes: fetched,
                uploaded_bytes: uploaded,
                country: None,
                flags,
            });
        }
        rates.insert(id, next);
        out.sort_by(|a, b| b.down_bps.cmp(&a.down_bps).then(b.up_bps.cmp(&a.up_bps)).then(a.addr.cmp(&b.addr)));
        Ok(out)
    }

    fn trackers(&self, id: &str) -> Result<Vec<TrackerInfo>, TorrentError> {
        let handle = self.get(id)?;
        let mut urls: Vec<String> = handle.shared().trackers.iter().map(|u| u.to_string()).collect();
        for t in &self.config.trackers {
            if let Ok(u) = url::Url::parse(t.trim()) {
                urls.push(u.to_string());
            }
        }
        urls.sort();
        urls.dedup();
        // librqbit does not expose announce results: every tracker is `Unknown`.
        Ok(urls
            .into_iter()
            .map(|url| TrackerInfo { url, status: TrackerStatus::Unknown, message: None, downloaded: None, seeders: None, leechers: None })
            .collect())
    }

    fn list(&self) -> Vec<LibraryEntry> {
        if self.ensure_running().is_err() {
            return Vec::new();
        }
        let mut out: Vec<LibraryEntry> = self
            .handles()
            .into_iter()
            .map(|handle| {
                let id = handle.info_hash().as_string();
                let st = handle.stats();
                let (size_bytes, files) = handle
                    .metadata
                    .load_full()
                    .map(|m| {
                        let files = m.file_infos.iter().filter(|f| !f.attrs.padding).count();
                        (m.lengths().total_length(), u32::try_from(files).unwrap_or(u32::MAX))
                    })
                    .unwrap_or((0, 0));
                let rec = self.labels.get(&id).unwrap_or_default();
                LibraryEntry {
                    name: handle.name().unwrap_or_else(|| id.clone()),
                    id,
                    size_bytes,
                    files,
                    progress: fraction(st.progress_bytes, st.total_bytes, st.finished),
                    state: map_state(st.state, st.finished),
                    added_unix: rec.added_unix,
                    persist: rec.persist,
                    save_path: handle.output_folder().to_path_buf(),
                    label: rec.label,
                }
            })
            .collect();
        out.sort_by(|a, b| b.added_unix.cmp(&a.added_unix).then(a.name.cmp(&b.name)));
        out
    }

    /// Every second: turn state changes into `StateChanged` / `FileCompleted` / `Completed` / `Error` events.
    fn spawn_monitor(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        let cancel = self.cancel.clone();
        self.handle.spawn(async move {
            let mut tick = tokio::time::interval(MONITOR_INTERVAL);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = tick.tick() => {}
                }
                let Some(inner) = weak.upgrade() else { break };
                inner.monitor_tick();
            }
        });
    }

    fn monitor_tick(&self) {
        let mut events = Vec::new();
        {
            let mut seen = lock(&self.monitor);
            let mut present = HashSet::new();
            for handle in self.handles() {
                let id = handle.info_hash().as_string();
                present.insert(id.clone());
                let st = handle.stats();
                let state = map_state(st.state, st.finished);
                // Paused-while-initializing torrents report no per-file progress: keep the last real snapshot.
                let files_done: Option<Vec<bool>> = (!st.file_progress.is_empty())
                    .then(|| {
                        handle.metadata.load_full().map(|m| {
                            m.file_infos
                                .iter()
                                .enumerate()
                                .map(|(i, fi)| fi.len > 0 && st.file_progress.get(i).is_some_and(|&have| have >= fi.len))
                                .collect::<Vec<bool>>()
                        })
                    })
                    .flatten();
                let prev = seen.get(&id).cloned();
                if let Some(prev) = &prev {
                    if prev.state != state {
                        events.push(TorrentEvent::StateChanged { id: id.clone(), state });
                        if state == TorrentState::Error {
                            events.push(TorrentEvent::Error { id: id.clone(), message: st.error.clone().unwrap_or_default() });
                        }
                    }
                    if let Some(done) = &files_done
                        && prev.files_done.len() == done.len()
                    {
                        for (index, (was, is)) in prev.files_done.iter().zip(done).enumerate() {
                            if !was && *is {
                                events.push(TorrentEvent::FileCompleted { id: id.clone(), index });
                            }
                        }
                    }
                    if !prev.finished && st.finished && state != TorrentState::Initializing {
                        events.push(TorrentEvent::Completed { id: id.clone() });
                    }
                }
                let files_done = files_done.or_else(|| prev.map(|p| p.files_done)).unwrap_or_default();
                seen.insert(id, Seen { state, finished: st.finished, files_done });
            }
            seen.retain(|id, _| present.contains(id));
        }
        for e in events {
            self.emit(e);
        }
    }
}

fn is_error(handle: &ManagedTorrent) -> bool {
    handle.with_state(|s| matches!(s, ManagedTorrentState::Error(_)))
}

/// `have / total` clamped to 0.0..=1.0 (nothing selected: 1.0 when librqbit says finished).
fn fraction(have: u64, total: u64, finished: bool) -> f64 {
    if total == 0 {
        return if finished { 1.0 } else { 0.0 };
    }
    (have as f64 / total as f64).min(1.0)
}

/// Downloaded byte ranges of `fi` from the torrent have-bitfield, merged, as fractions of the file.
fn file_ranges(fi: &FileInfo, piece_len: u64, have: &[bool], total_pieces: u32) -> Vec<(f64, f64)> {
    if fi.len == 0 || piece_len == 0 {
        return Vec::new();
    }
    let file_start = fi.offset_in_torrent;
    let file_end = file_start + fi.len;
    let mut ranges: Vec<(u64, u64)> = Vec::new();
    for piece in fi.piece_range.clone() {
        if piece >= total_pieces {
            break;
        }
        if !have.get(piece as usize).copied().unwrap_or(false) {
            continue;
        }
        let piece_start = u64::from(piece) * piece_len;
        let start = piece_start.max(file_start) - file_start;
        let end = (piece_start + piece_len).min(file_end).saturating_sub(file_start);
        if end <= start {
            continue;
        }
        match ranges.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => ranges.push((start, end)),
        }
    }
    let len = fi.len as f64;
    ranges.into_iter().map(|(s, e)| (s as f64 / len, e as f64 / len)).collect()
}

/// Keeps `TorrentFile::streams` / `ProtocolStatus::streaming` up while an HTTP response body is alive.
pub(crate) struct StreamGuard {
    inner: Arc<Inner>,
    id: String,
    index: usize,
}

impl Drop for StreamGuard {
    fn drop(&mut self) {
        {
            let mut streams = lock(&self.inner.streams);
            let key = (self.id.clone(), self.index);
            if let Some(n) = streams.get_mut(&key) {
                *n = n.saturating_sub(1);
                if *n == 0 {
                    streams.remove(&key);
                }
            }
        }
        self.inner.emit(TorrentEvent::StreamClosed { id: self.id.clone(), index: self.index });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fi(offset: u64, len: u64, pieces: std::ops::Range<u32>) -> FileInfo {
        FileInfo {
            relative_filename: PathBuf::from("Season 1").join("[Group] Show - 01.mkv"),
            offset_in_torrent: offset,
            piece_range: pieces,
            attrs: Default::default(),
            len,
        }
    }

    #[test]
    fn ranges_of_a_complete_file() {
        let f = fi(0, 100, 0..4);
        assert_eq!(file_ranges(&f, 32, &[true; 4], 4), vec![(0.0, 1.0)]);
    }

    #[test]
    fn ranges_merge_and_clip_to_the_file() {
        // File at bytes 40..140 of a torrent with 32-byte pieces: pieces 1..=4 (32..160).
        let f = fi(40, 100, 1..5);
        let have = [false, true, true, false, true];
        let r = file_ranges(&f, 32, &have, 5);
        // Pieces 1-2 cover file bytes 0..56 (torrent 40..96); piece 4 covers 88..100 (torrent 128..140).
        assert_eq!(r.len(), 2);
        assert!(r[0].0.abs() < 1e-9 && (r[0].1 - 0.56).abs() < 1e-9);
        assert!((r[1].0 - 0.88).abs() < 1e-9 && (r[1].1 - 1.0).abs() < 1e-9);
        assert!(file_ranges(&f, 32, &[false; 5], 5).is_empty());
    }

    #[test]
    fn file_path_is_slash_separated() {
        let f = fi(0, 1, 0..1);
        assert_eq!(file_path(&f), "Season 1/[Group] Show - 01.mkv");
        assert_eq!(file_name(&f), "[Group] Show - 01.mkv");
    }

    #[test]
    fn hashes_parse() {
        assert!(parse_hash("abcdef0123456789abcdef0123456789abcdef01").is_some());
        assert!(parse_hash("ABCDEF0123456789ABCDEF0123456789ABCDEF01").is_some());
        assert!(parse_hash("not a hash").is_none());
    }
}
