//! `LocalStore` methods over `downloads`, `local_library` and `auto_download_runs` (wave B, worker **h**;
//! docs/redesign/16 section 3.5). Child module of `store`. Planned: `DownloadRow` (one row of `downloads`, `state` =
//! `DownloadStateVm::id()`, `kind` = `DownloadKindVm::id()`), `insert_download(&DownloadRow) -> i64`,
//! `update_download_progress(id, bytes_done, bytes_total, segments_done, segments_total)`, `set_download_state(id,
//! state, error, path, completed_at)`, `download(id)`, `downloads(active_first)`, `downloads_for(media_id)`,
//! `delete_download(id)`, `clear_download_history() -> usize`, `mark_download_watched(media_id, episode, now)`;
//! `LocalItemRow`, `upsert_local_item`, `local_items()`, `local_item_for(media_id, episode)`, `set_local_match(id,
//! media_id, title, poster, episode)`, `delete_local_item(id)`; `insert_auto_run(&AutoRunRow)`, `last_auto_run()`.

use crate::store::LocalStore;

/// Tables of this file (the contract's `SCHEMA` creates them; the test below checks they exist).
pub const TABLES: &[&str] = &["downloads", "local_library", "auto_download_runs"];

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
