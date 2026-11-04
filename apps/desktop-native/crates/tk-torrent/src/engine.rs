//! [`TorrentEngine`]: one librqbit session + the loopback streaming server (docs/redesign/10 section 2).
//!
//! Signatures are the wave 3 contract; bodies are stubs that fail with `TorrentError::Unavailable` until wave 3 A
//! lands. The engine is `Send + Sync` and used behind an `Arc` by tk-app (`live/torrent.rs`). Read methods are cheap
//! snapshots (no I/O, safe to call every second from the UI poll); methods that change the session are async.

use std::path::PathBuf;

use tokio::sync::broadcast;

use crate::error::TorrentError;
use crate::types::{
    AddOptions, AddSource, EngineConfig, LibraryEntry, Limits, PeerInfo, TorrentEvent, TorrentFile, TorrentId, TorrentLabel,
    TorrentStats, TrackerInfo,
};

/// Capacity of the events channel; slow receivers get `RecvError::Lagged` and keep going.
pub const EVENTS_CAPACITY: usize = 256;
/// Longest `add` waits for a magnet's metadata before failing with `TorrentError::Timeout`.
pub const METADATA_TIMEOUT_SECS: u64 = 90;

pub struct TorrentEngine {
    config: EngineConfig,
    events: broadcast::Sender<TorrentEvent>,
}

impl TorrentEngine {
    /// Start the session (DHT, listeners, UPnP, persistence per `config`) and the HTTP streaming server on
    /// `127.0.0.1:<http_port>` with a fresh random token. Restores persisted torrents.
    pub async fn start(config: EngineConfig) -> Result<Self, TorrentError> {
        let (events, _) = broadcast::channel(EVENTS_CAPACITY);
        let _ = Self { config, events };
        Err(TorrentError::not_implemented("TorrentEngine::start"))
    }

    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// Add a torrent and wait for its metadata (bounded by [`METADATA_TIMEOUT_SECS`]). Adding a torrent that is
    /// already in the session returns its id (options are merged: `only_files` extended, `persist` OR-ed, `label`
    /// replaced when `Some`). Cancel-safe.
    pub async fn add(&self, _source: AddSource, _opts: AddOptions) -> Result<TorrentId, TorrentError> {
        Err(TorrentError::not_implemented("TorrentEngine::add"))
    }

    /// Files of a torrent in index order.
    pub fn files(&self, _id: &str) -> Result<Vec<TorrentFile>, TorrentError> {
        Err(TorrentError::not_implemented("TorrentEngine::files"))
    }

    /// Make `index` wanted (added to `only_files`) and move it to the front of the piece queue; other selected files
    /// stay selected. Used for "next episode" in a batch.
    pub async fn select_file(&self, _id: &str, _index: usize) -> Result<(), TorrentError> {
        Err(TorrentError::not_implemented("TorrentEngine::select_file"))
    }

    /// `http://127.0.0.1:<port>/<token>/<id>/<index>/<url-encoded file name>`. The server answers `Range`
    /// requests; a read selects the file and prioritises the pieces from the read position onwards (librqbit
    /// `FileStream`), so mpv plays while downloading. Works for every player mode (embedded, enhanced, external mpv).
    pub fn stream_url(&self, _id: &str, _file_index: usize) -> Result<String, TorrentError> {
        Err(TorrentError::not_implemented("TorrentEngine::stream_url"))
    }

    pub fn stats(&self, _id: &str) -> Result<TorrentStats, TorrentError> {
        Err(TorrentError::not_implemented("TorrentEngine::stats"))
    }

    /// Downloaded ranges of one file as sorted, merged `(start, end)` fractions of the file (0.0..=1.0) for the
    /// seek bar. A piece shared with the neighbour file counts for both.
    pub fn piece_bitfield(&self, _id: &str, _file_index: usize) -> Result<Vec<(f64, f64)>, TorrentError> {
        Err(TorrentError::not_implemented("TorrentEngine::piece_bitfield"))
    }

    /// Connected peers, fastest download first.
    pub fn peers(&self, _id: &str) -> Result<Vec<PeerInfo>, TorrentError> {
        Err(TorrentError::not_implemented("TorrentEngine::peers"))
    }

    pub fn trackers(&self, _id: &str) -> Result<Vec<TrackerInfo>, TorrentError> {
        Err(TorrentError::not_implemented("TorrentEngine::trackers"))
    }

    /// Every torrent of the session, newest first.
    pub fn list(&self) -> Vec<LibraryEntry> {
        Vec::new()
    }

    /// Remove from the session (and the label file); `delete_files` also deletes the downloaded data.
    pub async fn remove(&self, _id: &str, _delete_files: bool) -> Result<(), TorrentError> {
        Err(TorrentError::not_implemented("TorrentEngine::remove"))
    }

    pub async fn pause(&self, _id: &str) -> Result<(), TorrentError> {
        Err(TorrentError::not_implemented("TorrentEngine::pause"))
    }

    pub async fn resume(&self, _id: &str) -> Result<(), TorrentError> {
        Err(TorrentError::not_implemented("TorrentEngine::resume"))
    }

    /// Session-wide limits, applied live.
    pub fn set_limits(&self, _limits: Limits) -> Result<(), TorrentError> {
        Err(TorrentError::not_implemented("TorrentEngine::set_limits"))
    }

    /// Replace the app label of a torrent (persisted).
    pub fn set_label(&self, _id: &str, _label: TorrentLabel) -> Result<(), TorrentError> {
        Err(TorrentError::not_implemented("TorrentEngine::set_label"))
    }

    /// Change `AddOptions::persist` of a torrent (Library "keep").
    pub fn set_persist(&self, _id: &str, _persist: bool) -> Result<(), TorrentError> {
        Err(TorrentError::not_implemented("TorrentEngine::set_persist"))
    }

    /// Folder that holds the torrent's data (`LibraryEntry::save_path`).
    pub fn save_path(&self, _id: &str) -> Result<PathBuf, TorrentError> {
        Err(TorrentError::not_implemented("TorrentEngine::save_path"))
    }

    pub fn events(&self) -> broadcast::Receiver<TorrentEvent> {
        self.events.subscribe()
    }

    /// Stop streams, flush persistence, cancel the session. Idempotent; the engine is unusable afterwards.
    pub async fn shutdown(&self) {}
}
