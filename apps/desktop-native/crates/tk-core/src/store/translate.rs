//! `LocalStore` methods over `translation_cache` (wave A, worker **d**; docs/redesign/15 section 3). Child module
//! of `store` (uses its private helpers). Contract stubs; d implements with tests on `open_in_memory`.

use crate::error::StoreError;
use crate::store::LocalStore;

impl LocalStore {
    /// Cached translation for `key` (`sha256(provider | source | target | cache_key | text)` as lowercase hex).
    /// Contract stub: `None`.
    pub fn translation_get(&self, key: &str) -> Result<Option<String>, StoreError> {
        let _ = key;
        Ok(None)
    }
    /// Contract stub.
    pub fn translation_put(&self, key: &str, provider: &str, source_lang: Option<&str>, target_lang: &str, text: &str, now: i64) -> Result<(), StoreError> {
        let _ = (key, provider, source_lang, target_lang, text, now);
        Ok(())
    }
    /// Remove every cached translation; returns how many rows went. Contract stub: 0.
    pub fn translation_clear(&self) -> Result<usize, StoreError> {
        Ok(0)
    }
}
