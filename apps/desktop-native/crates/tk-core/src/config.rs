//! `config.json` (E8): `%APPDATA%\Tatakai-Next\config.json`, created on first start. Wave A (plan 14 section 0.1
//! #9): the Supabase URL / anon key are gone with the own login; the file keeps the metadata provider order and
//! stays the place for future machine-level options (nothing secret: tokens live in `secrets`). Older files that
//! still carry `supabase_url` / `supabase_anon_key` load fine (unknown fields are ignored) and are rewritten
//! without them on the next save.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::StoreError;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Metadata provider ids, highest priority first. Empty = default order.
    pub metadata_priority: Vec<String>,
}

pub const CONFIG_FILE: &str = "config.json";

impl AppConfig {
    pub fn path_in(dir: &Path) -> PathBuf {
        dir.join(CONFIG_FILE)
    }

    /// Read `<dir>/config.json`; a missing file yields `None`, a malformed one is an error.
    pub fn load(dir: &Path) -> Result<Option<Self>, StoreError> {
        match std::fs::read_to_string(Self::path_in(dir)) {
            Ok(text) => Ok(Some(serde_json::from_str(&text)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save(&self, dir: &Path) -> Result<(), StoreError> {
        std::fs::create_dir_all(dir)?;
        std::fs::write(Self::path_in(dir), serde_json::to_string_pretty(self)? + "\n")?;
        Ok(())
    }

    /// Load the config, creating it from `defaults` on first start.
    pub fn load_or_create(dir: &Path, defaults: Self) -> Result<Self, StoreError> {
        if let Some(c) = Self::load(dir)? {
            return Ok(c);
        }
        defaults.save(dir)?;
        Ok(defaults)
    }

    pub fn priority_or_default(&self) -> Vec<String> {
        if self.metadata_priority.is_empty() { crate::metadata::default_priority() } else { self.metadata_priority.clone() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_then_reloads_and_tolerates_legacy_files() {
        let dir = tempfile::tempdir().unwrap();
        assert!(AppConfig::load(dir.path()).unwrap().is_none());
        let created = AppConfig::load_or_create(dir.path(), AppConfig::default()).unwrap();
        assert_eq!(created.priority_or_default(), ["anilist", "kitsu", "anizip"]);
        // A file written by a build with the old login keeps loading; its Supabase fields are dropped.
        std::fs::write(
            AppConfig::path_in(dir.path()),
            r#"{"supabase_url":"https://x.supabase.co","supabase_anon_key":"k","metadata_priority":["kitsu"]}"#,
        )
        .unwrap();
        let c = AppConfig::load_or_create(dir.path(), AppConfig::default()).unwrap();
        assert_eq!(c.metadata_priority, ["kitsu"]);
        c.save(dir.path()).unwrap();
        assert!(!std::fs::read_to_string(AppConfig::path_in(dir.path())).unwrap().contains("supabase"));
        std::fs::write(AppConfig::path_in(dir.path()), "{ nope").unwrap();
        assert!(AppConfig::load(dir.path()).is_err());
    }
}
