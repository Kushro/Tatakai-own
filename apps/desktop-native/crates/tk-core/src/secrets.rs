//! Secret storage (wave A, worker **f**; docs/redesign/15-parity-wave-a-contract.md section 3): the Supabase
//! session (`supabase::session_store`), AniList / MAL tokens (`lists`) and debrid keys (`debrid`) go through
//! [`Secrets`], never through `ui-settings.json`.
//!
//! Implementation expected from f: the OS keyring through the `keyring` crate (Secret Service on Linux via zbus,
//! Credential Manager on Windows, Keychain on macOS; service name `Tatakai-Next`, one entry per `key`) and, when
//! the keyring is unavailable (no Secret Service on a bare Linux session, tests), a fallback file
//! `<data>/secrets.json` written with mode `0600` on Unix. Tests inject a tempdir and force the fallback.
//! The contract ships the API and a stub that stores nothing.

use std::path::{Path, PathBuf};

use crate::error::SecretError;

/// Keyring service name (one entry per key).
pub const SERVICE: &str = "Tatakai-Next";
/// Fallback file name under the data directory.
pub const FALLBACK_FILE: &str = "secrets.json";

/// Well-known keys.
pub mod keys {
    pub const SUPABASE_SESSION: &str = "supabase.session";
    pub const ANILIST_TOKEN: &str = "lists.anilist.token";
    pub const MAL_TOKEN: &str = "lists.mal.token";
    /// `debrid.<service id>.key`
    pub fn debrid(service_id: &str) -> String {
        format!("debrid.{service_id}.key")
    }
}

/// Where secrets live. Cheap to clone (paths only); the keyring is opened per call.
#[derive(Debug, Clone)]
pub struct Secrets {
    fallback: PathBuf,
    /// Skip the OS keyring (tests, `TATAKAI_NEXT_NO_KEYRING=1`).
    file_only: bool,
}

impl Secrets {
    /// `data_dir` holds the fallback file.
    pub fn open(data_dir: &Path) -> Self {
        let file_only = std::env::var_os("TATAKAI_NEXT_NO_KEYRING").is_some_and(|v| !v.is_empty());
        Self { fallback: data_dir.join(FALLBACK_FILE), file_only }
    }

    /// File fallback only (tests).
    pub fn file_only(data_dir: &Path) -> Self {
        Self { fallback: data_dir.join(FALLBACK_FILE), file_only: true }
    }

    pub fn fallback_path(&self) -> &Path {
        &self.fallback
    }

    pub fn uses_keyring(&self) -> bool {
        !self.file_only
    }

    /// Contract stub: nothing is stored yet (worker f).
    pub fn get(&self, key: &str) -> Result<Option<String>, SecretError> {
        let _ = key;
        Ok(None)
    }

    /// Contract stub (worker f).
    pub fn set(&self, key: &str, value: &str) -> Result<(), SecretError> {
        let _ = (key, value);
        Ok(())
    }

    /// Contract stub (worker f). Deleting a missing key is not an error.
    pub fn delete(&self, key: &str) -> Result<(), SecretError> {
        let _ = key;
        Ok(())
    }
}
