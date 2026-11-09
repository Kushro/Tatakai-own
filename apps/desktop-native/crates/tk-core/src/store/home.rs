//! `LocalStore` methods over `tracked_shows`, `airing_reminders`, `recs_feedback` and `home_visits` (wave B, worker
//! **b**; docs/redesign/16 section 3.5). Child module of `store`. Planned methods: `set_tracked(media_id, title,
//! poster, anilist_id, now)` / `remove_tracked` / `is_tracked` / `tracked_shows()`; `reminder_announced(media_id,
//! episode, now)` / `was_announced`; `set_recs_feedback(media_id, feedback, now)` / `recs_feedback()` /
//! `clear_recs_feedback`; `home_visited(section, now)` / `last_home_visit(section)`.

use crate::store::LocalStore;

/// Tables of this file (the contract's `SCHEMA` creates them; the test below checks they exist).
pub const TABLES: &[&str] = &["tracked_shows", "airing_reminders", "recs_feedback", "home_visits"];

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
