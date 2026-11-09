//! `LocalStore` methods over `social_cache` (wave B, worker **n**; docs/redesign/16 section 3.5). Child module of
//! `store`. Planned: `social_get(key, now) -> Option<String>` / `social_put(key, value, expires_at)` /
//! `social_clear()`. Keys: `profile:<name|me>`, `unread`, `reviews:<media_id>:<page>`, `notifications:<page>`.

use crate::store::LocalStore;

/// Tables of this file (the contract's `SCHEMA` creates them; the test below checks they exist).
pub const TABLES: &[&str] = &["social_cache"];

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
