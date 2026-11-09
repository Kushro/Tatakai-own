//! `LocalStore` methods over `torrent_episode_map`, `debrid_cache` and `torrent_sessions` (wave A, worker **i**;
//! docs/redesign/15 section 3). Child module of `store` (uses its private helpers). Contract stubs; i implements
//! with tests.

use crate::error::StoreError;
use crate::store::LocalStore;

/// One row of `torrent_sessions` (classic "Session history": every torrent the engine ran, newest first).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TorrentSessionRow {
    pub id: i64,
    pub torrent_id: String,
    pub info_hash: String,
    pub name: String,
    pub media_id: Option<String>,
    pub series: Option<String>,
    pub episode: Option<u32>,
    pub started_at: i64,
    /// `None` while the torrent is still in the engine (or the app died with it: i closes those rows at start).
    pub ended_at: Option<i64>,
    pub downloaded_bytes: u64,
    pub uploaded_bytes: u64,
    pub completed: bool,
}

impl LocalStore {
    /// Replace the alignment of a torrent (`(file index, episode)` pairs). Contract stub.
    pub fn set_torrent_episode_map(&self, torrent_id: &str, media_id: Option<&str>, map: &[(usize, u32)]) -> Result<(), StoreError> {
        let _ = (torrent_id, media_id, map);
        Ok(())
    }
    /// `(file index, episode)` pairs of a torrent, file index ascending. Contract stub: empty.
    pub fn torrent_episode_map(&self, torrent_id: &str) -> Result<Vec<(usize, u32)>, StoreError> {
        let _ = torrent_id;
        Ok(Vec::new())
    }
    pub fn clear_torrent_episode_map(&self, torrent_id: &str) -> Result<(), StoreError> {
        let _ = torrent_id;
        Ok(())
    }
    /// `Some(cached)` when checked within `max_age_secs`. Contract stub: `None`.
    pub fn debrid_cached(&self, service: &str, info_hash: &str, now: i64, max_age_secs: i64) -> Result<Option<bool>, StoreError> {
        let _ = (service, info_hash, now, max_age_secs);
        Ok(None)
    }
    pub fn set_debrid_cached(&self, service: &str, info_hash: &str, cached: bool, now: i64) -> Result<(), StoreError> {
        let _ = (service, info_hash, cached, now);
        Ok(())
    }

    // ---- torrent session history (plan 14 section 0.1 #11; checklist `torrent.session-history`) ----

    /// Open a session row when a torrent enters the engine; returns the row id. Contract stub: `0`.
    pub fn open_torrent_session(&self, row: &TorrentSessionRow) -> Result<i64, StoreError> {
        let _ = row;
        Ok(0)
    }
    /// Close the open row of `torrent_id` with the final counters. Contract stub.
    pub fn close_torrent_session(&self, torrent_id: &str, ended_at: i64, downloaded_bytes: u64, uploaded_bytes: u64, completed: bool) -> Result<(), StoreError> {
        let _ = (torrent_id, ended_at, downloaded_bytes, uploaded_bytes, completed);
        Ok(())
    }
    /// Newest first, at most `limit`. Contract stub: empty.
    pub fn torrent_sessions(&self, limit: u32) -> Result<Vec<TorrentSessionRow>, StoreError> {
        let _ = limit;
        Ok(Vec::new())
    }
    /// Delete every closed row (open ones stay). Returns the rows removed. Contract stub: `0`.
    pub fn clear_torrent_sessions(&self) -> Result<usize, StoreError> {
        Ok(0)
    }
}
