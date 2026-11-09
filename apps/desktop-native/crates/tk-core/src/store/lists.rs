//! `LocalStore` methods over `list_entries`, `list_queue`, `list_conflicts` and `favorites` (wave A, worker **g**;
//! docs/redesign/15-parity-wave-a-contract.md section 3). Child module of `store`: it uses the private `with`
//! helper of `mod.rs`. Contract: the row types below are the final shape tk-app maps to the UI view-models; the
//! method bodies are stubs that g implements (plus tests on `LocalStore::open_in_memory`).

use crate::error::StoreError;
use crate::store::LocalStore;

/// One row of `list_entries` (`status` is `ListStatusVm::id()`: `watching | planning | completed | paused | dropped |
/// repeating`; `score` is 0..=100 or `None`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListEntryRow {
    pub service: String,
    pub media_id: String,
    pub anilist_id: Option<u32>,
    pub mal_id: Option<u32>,
    pub title: String,
    pub poster: Option<String>,
    pub status: String,
    pub progress: u32,
    pub episodes: Option<u32>,
    pub score: Option<u8>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub repeats: u32,
    pub notes: Option<String>,
    pub remote_id: Option<u64>,
    pub updated_at: i64,
    pub dirty: bool,
}

/// One queued write (`op` = `upsert | delete | favorite | unfavorite`, `payload` = JSON of the fields).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListQueueRow {
    pub id: i64,
    pub service: String,
    pub media_id: String,
    pub op: String,
    pub payload: String,
    pub created_at: i64,
    pub attempts: u32,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListConflictRow {
    pub id: i64,
    pub service: String,
    pub media_id: String,
    pub local_json: String,
    pub remote_json: String,
    pub detected_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FavoriteRow {
    pub media_id: String,
    pub title: String,
    pub poster: Option<String>,
    pub added_at: i64,
}

impl LocalStore {
    /// Upsert a mirror row (`dirty` as given). Contract stub.
    pub fn put_list_entry(&self, row: &ListEntryRow) -> Result<(), StoreError> {
        let _ = row;
        Ok(())
    }
    /// Contract stub: `None`.
    pub fn list_entry(&self, service: &str, media_id: &str) -> Result<Option<ListEntryRow>, StoreError> {
        let _ = (service, media_id);
        Ok(None)
    }
    /// Newest `updated_at` first; `status: None` = all. Contract stub: empty.
    pub fn list_entries(&self, service: &str, status: Option<&str>) -> Result<Vec<ListEntryRow>, StoreError> {
        let _ = (service, status);
        Ok(Vec::new())
    }
    pub fn delete_list_entry(&self, service: &str, media_id: &str) -> Result<(), StoreError> {
        let _ = (service, media_id);
        Ok(())
    }
    /// Append a write to the offline queue; returns its id. Contract stub: 0.
    pub fn enqueue_list_op(&self, service: &str, media_id: &str, op: &str, payload: &str, now: i64) -> Result<i64, StoreError> {
        let _ = (service, media_id, op, payload, now);
        Ok(0)
    }
    /// Oldest first. Contract stub: empty.
    pub fn list_queue(&self, service: Option<&str>) -> Result<Vec<ListQueueRow>, StoreError> {
        let _ = service;
        Ok(Vec::new())
    }
    pub fn dequeue_list_op(&self, id: i64) -> Result<(), StoreError> {
        let _ = id;
        Ok(())
    }
    pub fn mark_list_op_failed(&self, id: i64, error: &str) -> Result<(), StoreError> {
        let _ = (id, error);
        Ok(())
    }
    pub fn add_list_conflict(&self, row: &ListConflictRow) -> Result<i64, StoreError> {
        let _ = row;
        Ok(0)
    }
    pub fn list_conflicts(&self) -> Result<Vec<ListConflictRow>, StoreError> {
        Ok(Vec::new())
    }
    pub fn delete_list_conflict(&self, id: i64) -> Result<(), StoreError> {
        let _ = id;
        Ok(())
    }
    pub fn set_favorite(&self, row: &FavoriteRow) -> Result<(), StoreError> {
        let _ = row;
        Ok(())
    }
    pub fn remove_favorite(&self, media_id: &str) -> Result<(), StoreError> {
        let _ = media_id;
        Ok(())
    }
    pub fn is_favorite(&self, media_id: &str) -> Result<bool, StoreError> {
        let _ = media_id;
        Ok(false)
    }
    /// Newest first. Contract stub: empty.
    pub fn favorites(&self) -> Result<Vec<FavoriteRow>, StoreError> {
        Ok(Vec::new())
    }
}
