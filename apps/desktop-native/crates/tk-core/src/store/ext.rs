//! `LocalStore` methods over `ext_servers`, `stremio_addons`, `ext_repos`, `ext_prefs`, `ext_updates` and
//! `provider_bookmarks` (wave B, worker **j**; docs/redesign/16 section 3.5). Child module of `store`. Planned:
//! `ExtServerRow` + `upsert_ext_server` / `ext_servers()` / `set_ext_server_enabled` / `set_ext_server_result(id,
//! ok_at, error)` / `delete_ext_server`; `StremioRow` + `upsert_stremio` / `stremio_addons()` / `set_stremio_enabled` /
//! `delete_stremio`; `ExtRepoRow` + `upsert_ext_repo` / `ext_repos()` / `set_ext_repo_catalog(id, json, fetched_at,
//! error)` / `delete_ext_repo`; `ext_prefs(ext_id) -> Vec<(key, value)>` / `set_ext_pref` / `clear_ext_prefs(ext_id)`;
//! `ExtUpdateRow` + `set_ext_update` / `ext_updates()` / `clear_ext_update(ext_id)`; `set_provider_bookmark` /
//! `remove_provider_bookmark` / `provider_bookmarks()` / `set_provider_bookmark_completed`. The kill switch's
//! "previously enabled" list goes in the `settings` table under `KILL_SWITCH_KEY`.

use crate::store::LocalStore;

/// Tables of this file (the contract's `SCHEMA` creates them; the test below checks they exist).
pub const TABLES: &[&str] = &["ext_servers", "stremio_addons", "ext_repos", "ext_prefs", "ext_updates", "provider_bookmarks"];

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
