//! `config.json` (E8): `%APPDATA%\Tatakai-Next\config.json`, created on first start from compile-time
//! `option_env!` values (or empty). The app works without an account when nothing is configured.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::StoreError;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub supabase_url: Option<String>,
    pub supabase_anon_key: Option<String>,
    /// Metadata provider ids, highest priority first. Empty = default order.
    pub metadata_priority: Vec<String>,
}

pub const CONFIG_FILE: &str = "config.json";

impl AppConfig {
    pub fn path_in(dir: &Path) -> PathBuf {
        dir.join(CONFIG_FILE)
    }

    /// Values baked in at build time (`TATAKAI_SUPABASE_URL` / `TATAKAI_SUPABASE_ANON_KEY`). Never committed.
    pub fn from_build_env() -> Self {
        let clean = |v: Option<&str>| v.map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
        Self {
            supabase_url: clean(option_env!("TATAKAI_SUPABASE_URL")),
            supabase_anon_key: clean(option_env!("TATAKAI_SUPABASE_ANON_KEY")),
            metadata_priority: Vec::new(),
        }
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

    /// `(url, anon_key)` when both are present.
    pub fn supabase(&self) -> Option<(&str, &str)> {
        match (self.supabase_url.as_deref(), self.supabase_anon_key.as_deref()) {
            (Some(u), Some(k)) if !u.is_empty() && !k.is_empty() => Some((u, k)),
            _ => None,
        }
    }

    pub fn priority_or_default(&self) -> Vec<String> {
        if self.metadata_priority.is_empty() { crate::metadata::default_priority() } else { self.metadata_priority.clone() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_then_reloads_and_tolerates_partial_files() {
        let dir = tempfile::tempdir().unwrap();
        assert!(AppConfig::load(dir.path()).unwrap().is_none());
        let created = AppConfig::load_or_create(dir.path(), AppConfig::default()).unwrap();
        assert!(created.supabase().is_none());
        std::fs::write(AppConfig::path_in(dir.path()), r#"{"supabase_url":"https://x.supabase.co","supabase_anon_key":"k"}"#).unwrap();
        let c = AppConfig::load_or_create(dir.path(), AppConfig::default()).unwrap();
        assert_eq!(c.supabase(), Some(("https://x.supabase.co", "k")));
        assert_eq!(c.priority_or_default(), ["anilist", "kitsu", "anizip"]);
        std::fs::write(AppConfig::path_in(dir.path()), "{ nope").unwrap();
        assert!(AppConfig::load(dir.path()).is_err());
    }
}
