//! `LocalStore` methods over `torrent_episode_map` and `debrid_cache` (wave A, worker **i**; docs/redesign/15
//! section 3). Child module of `store` (uses its private helpers). Contract stubs; i implements with tests.

use crate::error::StoreError;
use crate::store::LocalStore;

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
}
