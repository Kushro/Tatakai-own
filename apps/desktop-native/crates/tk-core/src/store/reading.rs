//! `LocalStore` methods over `read_progress`, `chapter_lists`, `offline_chapters`, `read_list_entries` and
//! `read_list_queue` (wave B, worker **l**; worker **m** calls them with `kind = "novel"` and never edits this file;
//! docs/redesign/16 section 3.5). Child module of `store`. Planned: `ReadProgressRow` + `set_read_progress(&row)` /
//! `read_progress(kind, media_id)` / `continue_reading(kind, limit)` (newest per title); `chapter_list(kind, media_id,
//! ext_id, max_age_secs) -> Option<String>` / `set_chapter_list(...)` / `clear_chapter_lists(kind, media_id)`;
//! `OfflineChapterRow` + `set_offline_chapter` / `offline_chapter(kind, media_id, ext_id, chapter_id)` /
//! `offline_chapters(kind, media_id)` / `delete_offline_chapter` / `offline_bytes(kind)`; `ReadListRow` +
//! `put_read_list_entry` / `read_list_entry` / `read_list_entries(kind, status)` / `delete_read_list_entry`;
//! `enqueue_read_list_op` / `read_list_queue` / `dequeue_read_list_op` / `mark_read_list_op_failed` (same shape as
//! worker g's `store/lists.rs` queue so `list_sync_now` can drain both).

use crate::store::LocalStore;

/// Tables of this file (the contract's `SCHEMA` creates them; the test below checks they exist).
pub const TABLES: &[&str] = &["read_progress", "chapter_lists", "offline_chapters", "read_list_entries", "read_list_queue"];

impl LocalStore {
    // Contract: the owner adds the methods here (see the module doc). `self.with(|c| ...)` is the private helper of
    // `store/mod.rs`; row structs live in this file next to the methods.
}

#[cfg(test)]
mod tests {
    use super::TABLES;
    use crate::store::LocalStore;

    #[test]
    fn tables_exist() {
        let s = LocalStore::open_in_memory().unwrap();
        for t in TABLES {
            assert!(s.table_exists(t).unwrap(), "{t} missing");
        }
    }
}
