//! `config.json` (review E8): created at first launch from the compile-time `TATAKAI_SUPABASE_URL` /
//! `TATAKAI_SUPABASE_ANON_KEY` (or empty). The app starts without an account either way.
//!
//! Same JSON shape as `tk_core::AppConfig` (B5) so the file is interchangeable; the integrator may
//! replace this module with a re-export once tk-core is merged.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const CONFIG_FILE: &str = "config.json";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub supabase_url: Option<String>,
    pub supabase_anon_key: Option<String>,
    pub metadata_priority: Vec<String>,
}

impl AppConfig {
    pub fn path_in(dir: &Path) -> PathBuf {
        dir.join(CONFIG_FILE)
    }

    pub fn from_build_env() -> Self {
        Self::from_values(option_env!("TATAKAI_SUPABASE_URL"), option_env!("TATAKAI_SUPABASE_ANON_KEY"))
    }

    pub fn from_values(url: Option<&str>, key: Option<&str>) -> Self {
        let clean = |v: Option<&str>| v.map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
        Self { supabase_url: clean(url), supabase_anon_key: clean(key), metadata_priority: Vec::new() }
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

    /// `(url, anon_key)` when both are set.
    #[cfg_attr(not(feature = "live"), allow(dead_code))]
    pub fn supabase(&self) -> Option<(&str, &str)> {
        match (self.supabase_url.as_deref(), self.supabase_anon_key.as_deref()) {
            (Some(u), Some(k)) if !u.is_empty() && !k.is_empty() => Some((u, k)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_run_writes_defaults_then_reloads_them() {
        let tmp = tempfile::tempdir().unwrap();
        let defaults = AppConfig::from_values(Some(" https://x.supabase.co "), Some("anon"));
        let created = AppConfig::load_or_create(tmp.path(), defaults.clone()).unwrap();
        assert_eq!(created.supabase(), Some(("https://x.supabase.co", "anon")));
        assert!(AppConfig::path_in(tmp.path()).is_file());
        // The second call ignores new defaults: the file wins.
        let again = AppConfig::load_or_create(tmp.path(), AppConfig::default()).unwrap();
        assert_eq!(again, defaults);
    }

    #[test]
    fn empty_build_env_yields_no_account() {
        let c = AppConfig::from_values(None, Some("   "));
        assert_eq!(c.supabase(), None);
    }

    #[test]
    fn corrupt_file_is_an_error_and_is_left_alone() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(AppConfig::path_in(tmp.path()), "{ nope").unwrap();
        assert!(AppConfig::load_or_create(tmp.path(), AppConfig::default()).is_err());
        assert_eq!(std::fs::read_to_string(AppConfig::path_in(tmp.path())).unwrap(), "{ nope");
    }

    #[test]
    fn partial_files_are_accepted() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(AppConfig::path_in(tmp.path()), r#"{"supabase_url":"https://a.b"}"#).unwrap();
        let c = AppConfig::load_or_create(tmp.path(), AppConfig::default()).unwrap();
        assert_eq!(c.supabase(), None);
        assert_eq!(c.supabase_url.as_deref(), Some("https://a.b"));
    }
}
