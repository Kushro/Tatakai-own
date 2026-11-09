//! `LocalStore` methods over `tts_cache` (wave B, worker **m**; docs/redesign/16 section 3.5). Child module of
//! `store`. Planned: `TtsClipRow` + `tts_clip(key) -> Option<TtsClipRow>` / `put_tts_clip(&row)` / `tts_cache_bytes()`
//! / `evict_tts_cache(max_bytes) -> Vec<path>` (oldest first; the caller deletes the files) / `clear_tts_cache()`.

use crate::store::LocalStore;

/// Tables of this file (the contract's `SCHEMA` creates them; the test below checks they exist).
pub const TABLES: &[&str] = &["tts_cache"];

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
