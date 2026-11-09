//! `LocalStore` methods over `social_cache` (wave B, worker **n**; docs/redesign/16 section 3.5). Child module of
//! `store`: short-lived AniList social answers (JSON) keyed by query so the rail badge, Details and the profile
//! pages do not hammer AniList (90 requests / minute). Keys: `profile:<name|me>`, `unread`,
//! `reviews:<media_id>:<page>`, `notifications:<page>`, `follow:<user_id>:<edge>:<page>`.

use rusqlite::{OptionalExtension, params};

use crate::error::StoreError;
use crate::store::LocalStore;

/// Tables of this file (the contract's `SCHEMA` creates them; the test below checks they exist).
pub const TABLES: &[&str] = &["social_cache"];

impl LocalStore {
    /// The cached value of `key` when it has not expired at `now` (unix seconds).
    pub fn social_get(&self, key: &str, now: i64) -> Result<Option<String>, StoreError> {
        self.with(|c| {
            c.query_row(
                "SELECT value FROM social_cache WHERE key = ?1 AND expires_at > ?2",
                params![key, now],
                |r| r.get::<_, String>(0),
            )
            .optional()
        })
    }

    /// Insert or replace `key`, valid until `expires_at` (unix seconds).
    pub fn social_put(&self, key: &str, value: &str, expires_at: i64) -> Result<(), StoreError> {
        self.with(|c| {
            c.execute(
                "INSERT INTO social_cache (key, value, expires_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(key) DO UPDATE SET value = ?2, expires_at = ?3",
                params![key, value, expires_at],
            )
            .map(|_| ())
        })
    }

    /// Drop every key that starts with `prefix` (after a write that makes them stale). Returns the rows removed.
    pub fn social_forget(&self, prefix: &str) -> Result<usize, StoreError> {
        // `LIKE` would treat `_` / `%` in the prefix as wildcards: compare the leading substring instead.
        self.with(|c| {
            c.execute(
                "DELETE FROM social_cache WHERE substr(key, 1, length(?1)) = ?1",
                params![prefix],
            )
        })
    }

    /// Drop expired rows (called opportunistically) and return how many went.
    pub fn social_prune(&self, now: i64) -> Result<usize, StoreError> {
        self.with(|c| c.execute("DELETE FROM social_cache WHERE expires_at <= ?1", params![now]))
    }

    /// Empty the cache (account disconnected, Settings > Data).
    pub fn social_clear(&self) -> Result<(), StoreError> {
        self.with(|c| c.execute("DELETE FROM social_cache", []).map(|_| ()))
    }
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

    #[test]
    fn values_expire() {
        let s = LocalStore::open_in_memory().unwrap();
        s.social_put("unread", "3", 100).unwrap();
        assert_eq!(s.social_get("unread", 99).unwrap().as_deref(), Some("3"));
        assert_eq!(s.social_get("unread", 100).unwrap(), None);
        s.social_put("unread", "0", 200).unwrap();
        assert_eq!(s.social_get("unread", 150).unwrap().as_deref(), Some("0"));
        assert_eq!(s.social_get("missing", 0).unwrap(), None);
    }

    #[test]
    fn forget_by_prefix_prune_and_clear() {
        let s = LocalStore::open_in_memory().unwrap();
        s.social_put("profile:me", "a", 1000).unwrap();
        s.social_put("profile:Ana", "b", 1000).unwrap();
        s.social_put("profile_x", "c", 1000).unwrap();
        s.social_put("reviews:21:1", "d", 10).unwrap();
        assert_eq!(s.social_forget("profile:").unwrap(), 2);
        assert_eq!(s.social_get("profile_x", 0).unwrap().as_deref(), Some("c"));
        assert_eq!(s.social_prune(10).unwrap(), 1);
        s.social_clear().unwrap();
        assert_eq!(s.social_get("profile_x", 0).unwrap(), None);
    }
}
