use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TorrentError {
    /// The id is not (or no longer) in the session.
    #[error("torrent {0} is not in the session")]
    NotFound(String),
    /// Not a magnet link, info-hash, `.torrent` file or URL the engine can use.
    #[error("not a usable torrent source: {0}")]
    InvalidSource(String),
    #[error("torrent {id} has no file {index}")]
    NoSuchFile { id: String, index: usize },
    /// Metadata did not arrive in time (no peers for a magnet), or another bounded wait expired.
    #[error("timed out: {0}")]
    Timeout(String),
    #[error("i/o error: {0}")]
    Io(String),
    /// Anything else librqbit reports.
    #[error("{0}")]
    Engine(String),
    /// The engine is not running or not built (contract stubs, failed start).
    #[error("torrent engine unavailable: {0}")]
    Unavailable(String),
}
