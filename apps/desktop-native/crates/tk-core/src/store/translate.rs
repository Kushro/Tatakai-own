//! `LocalStore` methods over `translation_cache` (wave A, worker **d**; docs/redesign/15 section 3). Child module
//! of `store` (uses its private helpers). One row per translated cue text: the key is
//! `tk_core::translate::cache_key(provider, source, target, cache_key, text)`, so a re-watch (or the same line in
//! another episode with the same cache partition) never pays the provider again.

use rusqlite::{OptionalExtension, params};

use crate::error::StoreError;
use crate::store::LocalStore;

impl LocalStore {
    /// Cached translation for `key` (`sha256(provider | source | target | cache_key | text)` as lowercase hex).
    pub fn translation_get(&self, key: &str) -> Result<Option<String>, StoreError> {
        self.with(|c| c.query_row("SELECT text FROM translation_cache WHERE key = ?1", [key], |r| r.get::<_, String>(0)).optional())
    }

    /// Store (or replace) one translated text. `now` = unix seconds.
    pub fn translation_put(&self, key: &str, provider: &str, source_lang: Option<&str>, target_lang: &str, text: &str, now: i64) -> Result<(), StoreError> {
        self.with(|c| {
            c.execute(
                "INSERT INTO translation_cache (key, provider, source_lang, target_lang, text, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(key) DO UPDATE SET provider = ?2, source_lang = ?3, target_lang = ?4, text = ?5, created_at = ?6",
                params![key, provider, source_lang, target_lang, text, now],
            )?;
            Ok(())
        })
    }

    /// Remove every cached translation; returns how many rows went.
    pub fn translation_clear(&self) -> Result<usize, StoreError> {
        self.with(|c| c.execute("DELETE FROM translation_cache", []))
    }
}

#[cfg(test)]
mod tests {
    use crate::store::LocalStore;

    #[test]
    fn put_get_replace_and_clear() {
        let s = LocalStore::open_in_memory().unwrap();
        assert_eq!(s.translation_get("k1").unwrap(), None);
        s.translation_put("k1", "google", None, "es", "Hola", 10).unwrap();
        s.translation_put("k2", "deepl", Some("en"), "es", "Adiós", 11).unwrap();
        assert_eq!(s.translation_get("k1").unwrap().as_deref(), Some("Hola"));
        s.translation_put("k1", "google", None, "es", "Hola de nuevo", 12).unwrap();
        assert_eq!(s.translation_get("k1").unwrap().as_deref(), Some("Hola de nuevo"));
        assert_eq!(s.translation_clear().unwrap(), 2);
        assert_eq!(s.translation_get("k2").unwrap(), None);
        assert_eq!(s.translation_clear().unwrap(), 0);
    }
}
