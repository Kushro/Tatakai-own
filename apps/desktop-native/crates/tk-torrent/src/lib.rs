//! tk-torrent: the desktop torrent engine (docs/redesign/10-wave3-torrents-contract.md section 2).
//!
//! * session: librqbit 9.1.0 (pure Rust): DHT, PEX, LSD, trackers, UPnP, session persistence + fastresume
//! * streaming: a loopback HTTP server (`127.0.0.1`, random token in the path, `Range` support) over librqbit
//!   `FileStream`, which prioritises the pieces at the read position, so mpv plays while downloading in every
//!   player mode (embedded, enhanced, external mpv)
//! * stats for the Hayase-style client: totals, speeds, pieces, peers, trackers, per-file progress, downloaded
//!   ranges for the seek bar
//!
//! UI-free and independent of tk-core: tk-app maps these types to `tk_ui::backend::Torrent*Vm` and picks the
//! episode file with `tk_core::release::pick_episode_file`.

mod engine;
mod error;
mod stream;
mod types;

pub use engine::{EVENTS_CAPACITY, METADATA_TIMEOUT_SECS, TorrentEngine};
pub use error::TorrentError;
pub use types::{
    AddOptions, AddSource, EngineConfig, LibraryEntry, Limits, PeerInfo, ProtocolStatus, TorrentEvent, TorrentFile, TorrentId,
    TorrentLabel, TorrentState, TorrentStats, TrackerInfo, TrackerStatus,
};

/// The librqbit release this crate is written against (pinned with `=` in the workspace).
pub const LIBRQBIT_VERSION: &str = "9.1.0";
