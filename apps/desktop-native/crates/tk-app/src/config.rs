//! `config.json` (review E8): created at first launch. Wave A (plan 14 section 0.1 #9): the compile-time
//! `TATAKAI_SUPABASE_URL` / `TATAKAI_SUPABASE_ANON_KEY` and the matching fields are gone with the own login; the
//! file keeps the metadata provider order. Older files with the Supabase fields still load (unknown fields are
//! ignored) and lose them on the next write. Nothing secret lives here (OAuth tokens: `tk_core::Secrets`).
//!
//! Same JSON shape as `tk_core::AppConfig` (B5) so the file is interchangeable; the integrator may
//! replace this module with a re-export once tk-core is merged.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const CONFIG_FILE: &str = "config.json";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub metadata_priority: Vec<String>,
}

impl AppConfig {
    pub fn path_in(dir: &Path) -> PathBuf {
        dir.join(CONFIG_FILE)
    }

    /// Defaults for a first launch (no build-time values any more).
    pub fn from_build_env() -> Self {
        Self::default()
    }

    /// Load `dir/config.json`, writing `defaults` first if it does not exist. A corrupt file is an error
    /// (it is the user's file: never overwritten silently).
    pub fn load_or_create(dir: &Path, defaults: Self) -> anyhow::Result<Self> {
        let path = Self::path_in(dir);
        match std::fs::read_to_string(&path) {
            Ok(text) => Ok(serde_json::from_str(&text).map_err(|e| anyhow::anyhow!("{} is not valid JSON: {e}", path.display()))?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir_all(dir)?;
                std::fs::write(&path, serde_json::to_string_pretty(&defaults)? + "\n")?;
                Ok(defaults)
            }
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_run_writes_defaults_then_reloads_them() {
        let tmp = tempfile::tempdir().unwrap();
        let defaults = AppConfig { metadata_priority: vec!["kitsu".into()] };
        let created = AppConfig::load_or_create(tmp.path(), defaults.clone()).unwrap();
        assert_eq!(created, defaults);
        assert!(AppConfig::path_in(tmp.path()).is_file());
        // The second call ignores new defaults: the file wins.
        let again = AppConfig::load_or_create(tmp.path(), AppConfig::default()).unwrap();
        assert_eq!(again, defaults);
    }

    #[test]
    fn corrupt_file_is_an_error_and_is_left_alone() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(AppConfig::path_in(tmp.path()), "{ nope").unwrap();
        assert!(AppConfig::load_or_create(tmp.path(), AppConfig::default()).is_err());
        assert_eq!(std::fs::read_to_string(AppConfig::path_in(tmp.path())).unwrap(), "{ nope");
    }

    #[test]
    fn legacy_supabase_fields_are_ignored() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(AppConfig::path_in(tmp.path()), r#"{"supabase_url":"https://a.b","supabase_anon_key":"k"}"#).unwrap();
        let c = AppConfig::load_or_create(tmp.path(), AppConfig::default()).unwrap();
        assert_eq!(c, AppConfig::default());
    }
}
