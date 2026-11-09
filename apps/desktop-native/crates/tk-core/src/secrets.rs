//! Secret storage (wave A, worker **f**; docs/redesign/15-parity-wave-a-contract.md section 3): AniList / MAL
//! OAuth tokens (`lists`, worker g), subtitle-translation API keys (`translate`, worker d) and debrid keys
//! (`debrid`, worker i) go through [`Secrets`], never through `ui-settings.json` or `config.json`.
//!
//! Where a value lives:
//! * the OS keyring through the `keyring` crate (Secret Service on Linux via zbus, Credential Manager on Windows,
//!   Keychain on macOS), service [`SERVICE`], one entry per key (the key is the keyring "user");
//! * when the keyring is unavailable (bare Linux session without a Secret Service, a locked keyring that does not
//!   answer, tests, `TATAKAI_NEXT_NO_KEYRING=1`), the fallback file `<data>/secrets.json`: a flat JSON object
//!   `{ "<key>": "<value>" }` written atomically with mode `0600` on Unix.
//!
//! Every keyring call runs on its own short-lived thread with a timeout ([`KEYRING_TIMEOUT`]): the `keyring` crate's
//! async Secret Service backend blocks on its own tokio runtime and deadlocks when called from a runtime thread,
//! and a locked keyring may wait for a password prompt forever. A timeout or a platform error falls back to the
//! file, so a secret is never lost; a value that later lands in the keyring is removed from the file.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use crate::error::SecretError;

/// Keyring service name (one entry per key).
pub const SERVICE: &str = "Tatakai-Next";
/// Fallback file name under the data directory.
pub const FALLBACK_FILE: &str = "secrets.json";
/// Longest wait for one keyring operation before the file fallback is used.
pub const KEYRING_TIMEOUT: Duration = Duration::from_secs(5);
/// Environment switch that forces the file fallback (tests, headless sessions).
pub const NO_KEYRING_ENV: &str = "TATAKAI_NEXT_NO_KEYRING";

/// Well-known keys.
pub mod keys {
    /// AniList OAuth token (`lists::Token` as JSON).
    pub const ANILIST_TOKEN: &str = "lists.anilist.token";
    /// MAL OAuth token + refresh token (`lists::Token` as JSON).
    pub const MAL_TOKEN: &str = "lists.mal.token";
    /// `debrid.<service id>.key`
    pub fn debrid(service_id: &str) -> String {
        format!("debrid.{service_id}.key")
    }
    /// `translate.<provider id>.key` (`TranslationProviderVm::id()`: `deepl`, `azure`, `openai`).
    pub fn translation(provider_id: &str) -> String {
        format!("translate.{provider_id}.key")
    }
}

/// Serializes writes of the fallback file inside this process (several `Secrets` clones share one file).
static FILE_LOCK: Mutex<()> = Mutex::new(());

/// Where secrets live. Cheap to clone (paths only); the keyring is opened per call.
#[derive(Debug, Clone)]
pub struct Secrets {
    fallback: PathBuf,
    /// Skip the OS keyring (tests, `TATAKAI_NEXT_NO_KEYRING=1`).
    file_only: bool,
}

/// Outcome of one keyring operation.
enum Kr<T> {
    Ok(T),
    /// The keyring answered: nothing stored under that key.
    Missing,
    /// No usable keyring (error or timeout): use the file.
    Unavailable(String),
}

impl Secrets {
    /// `data_dir` holds the fallback file.
    pub fn open(data_dir: &Path) -> Self {
        let file_only = std::env::var_os(NO_KEYRING_ENV).is_some_and(|v| !v.is_empty());
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

    /// The stored value, `None` when nothing is stored. The keyring wins over the file.
    pub fn get(&self, key: &str) -> Result<Option<String>, SecretError> {
        check_key(key)?;
        if self.uses_keyring() {
            match keyring_get(key) {
                Kr::Ok(v) => return Ok(Some(v)),
                Kr::Missing => {}
                Kr::Unavailable(e) => tracing::debug!(error = %e, "keyring unavailable; reading the fallback file"),
            }
        }
        Ok(self.read_file()?.remove(key))
    }

    /// Store `value` under `key` (an empty value deletes it).
    pub fn set(&self, key: &str, value: &str) -> Result<(), SecretError> {
        check_key(key)?;
        if value.is_empty() {
            return self.delete(key);
        }
        if self.uses_keyring() {
            match keyring_set(key, value) {
                Kr::Ok(()) => {
                    // the keyring has it now: do not leave an older copy in the file
                    self.file_remove(key)?;
                    return Ok(());
                }
                Kr::Missing => {}
                Kr::Unavailable(e) => tracing::info!(error = %e, "keyring unavailable; storing the secret in the fallback file"),
            }
        }
        self.file_put(key, value)
    }

    /// Remove `key` from the keyring and from the file. Deleting a missing key is not an error.
    pub fn delete(&self, key: &str) -> Result<(), SecretError> {
        check_key(key)?;
        if self.uses_keyring()
            && let Kr::Unavailable(e) = keyring_delete(key)
        {
            tracing::debug!(error = %e, "keyring unavailable while deleting a secret");
        }
        self.file_remove(key)
    }

    /// Delete every key in `keys` (keyring + file) and then the fallback file itself ("Reset app"). Errors of single
    /// keyring entries are ignored; a file that cannot be removed is an error.
    pub fn wipe(&self, keys: &[String]) -> Result<(), SecretError> {
        if self.uses_keyring() {
            for k in keys {
                let _ = keyring_delete(k);
            }
        }
        let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        match std::fs::remove_file(&self.fallback) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    // ---- fallback file ----------------------------------------------------------------------------------------

    fn read_file(&self) -> Result<BTreeMap<String, String>, SecretError> {
        match std::fs::read_to_string(&self.fallback) {
            Ok(text) if text.trim().is_empty() => Ok(BTreeMap::new()),
            Ok(text) => Ok(serde_json::from_str(&text)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(e) => Err(e.into()),
        }
    }

    fn file_put(&self, key: &str, value: &str) -> Result<(), SecretError> {
        let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut map = self.read_file()?;
        map.insert(key.to_owned(), value.to_owned());
        write_private(&self.fallback, &serde_json::to_vec_pretty(&map)?)
    }

    fn file_remove(&self, key: &str) -> Result<(), SecretError> {
        let _guard = FILE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (mut map, corrupt) = match self.read_file() {
            Ok(m) => (m, false),
            // a corrupt file must not block deleting: it is replaced below
            Err(SecretError::Json(_)) => (BTreeMap::new(), true),
            Err(e) => return Err(e),
        };
        let removed = map.remove(key).is_some();
        if !removed && !corrupt {
            return Ok(());
        }
        if map.is_empty() {
            return match std::fs::remove_file(&self.fallback) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e.into()),
            };
        }
        write_private(&self.fallback, &serde_json::to_vec_pretty(&map)?)
    }
}

fn check_key(key: &str) -> Result<(), SecretError> {
    if key.trim().is_empty() {
        return Err(SecretError::Keyring("empty secret key".to_owned()));
    }
    Ok(())
}

/// Atomic write (`<file>.tmp` + rename) readable only by the user on Unix.
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), SecretError> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let _ = std::fs::remove_file(&tmp);
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(&tmp)?;
    f.write_all(bytes)?;
    f.write_all(b"\n")?;
    f.sync_all()?;
    drop(f);
    std::fs::rename(&tmp, path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

// ---- keyring (each call on its own thread, bounded by KEYRING_TIMEOUT) ------------------------------------------

fn on_thread<T: Send + 'static>(op: impl FnOnce() -> Kr<T> + Send + 'static) -> Kr<T> {
    let (tx, rx) = std::sync::mpsc::channel();
    let spawned = std::thread::Builder::new().name("tk-keyring".to_owned()).spawn(move || {
        let _ = tx.send(op());
    });
    if let Err(e) = spawned {
        return Kr::Unavailable(format!("cannot start the keyring thread: {e}"));
    }
    match rx.recv_timeout(KEYRING_TIMEOUT) {
        Ok(r) => r,
        Err(_) => Kr::Unavailable("the keyring did not answer in time".to_owned()),
    }
}

fn entry(key: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, key).map_err(|e| e.to_string())
}

fn keyring_get(key: &str) -> Kr<String> {
    let key = key.to_owned();
    on_thread(move || match entry(&key) {
        Err(e) => Kr::Unavailable(e),
        Ok(entry) => match entry.get_password() {
            Ok(v) => Kr::Ok(v),
            Err(keyring::Error::NoEntry) => Kr::Missing,
            Err(e) => Kr::Unavailable(e.to_string()),
        },
    })
}

fn keyring_set(key: &str, value: &str) -> Kr<()> {
    let (key, value) = (key.to_owned(), value.to_owned());
    on_thread(move || match entry(&key) {
        Err(e) => Kr::Unavailable(e),
        Ok(entry) => match entry.set_password(&value) {
            Ok(()) => Kr::Ok(()),
            Err(e) => Kr::Unavailable(e.to_string()),
        },
    })
}

fn keyring_delete(key: &str) -> Kr<()> {
    let key = key.to_owned();
    on_thread(move || match entry(&key) {
        Err(e) => Kr::Unavailable(e),
        Ok(entry) => match entry.delete_credential() {
            Ok(()) => Kr::Ok(()),
            Err(keyring::Error::NoEntry) => Kr::Missing,
            Err(e) => Kr::Unavailable(e.to_string()),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_fallback_round_trips_and_deletes() {
        let tmp = tempfile::tempdir().unwrap();
        let s = Secrets::file_only(tmp.path());
        assert!(!s.uses_keyring());
        assert_eq!(s.get(keys::ANILIST_TOKEN).unwrap(), None);
        s.set(keys::ANILIST_TOKEN, "tok-1").unwrap();
        s.set(&keys::debrid("real_debrid"), "rd").unwrap();
        assert_eq!(s.get(keys::ANILIST_TOKEN).unwrap().as_deref(), Some("tok-1"));
        // overwrite
        s.set(keys::ANILIST_TOKEN, "tok-2").unwrap();
        assert_eq!(s.get(keys::ANILIST_TOKEN).unwrap().as_deref(), Some("tok-2"));
        // a second handle on the same directory sees the same values
        let again = Secrets::file_only(tmp.path());
        assert_eq!(again.get(&keys::debrid("real_debrid")).unwrap().as_deref(), Some("rd"));
        s.delete(keys::ANILIST_TOKEN).unwrap();
        s.delete(keys::ANILIST_TOKEN).unwrap(); // missing key: not an error
        assert_eq!(s.get(keys::ANILIST_TOKEN).unwrap(), None);
        assert!(s.fallback_path().exists());
        s.delete(&keys::debrid("real_debrid")).unwrap();
        assert!(!s.fallback_path().exists(), "the file goes away with its last key");
    }

    #[test]
    fn empty_value_deletes_and_empty_key_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let s = Secrets::file_only(tmp.path());
        s.set(&keys::translation("deepl"), "k").unwrap();
        s.set(&keys::translation("deepl"), "").unwrap();
        assert_eq!(s.get(&keys::translation("deepl")).unwrap(), None);
        assert!(s.set("  ", "x").is_err());
        assert!(s.get("").is_err());
    }

    #[test]
    fn the_file_uses_the_documented_shape_and_is_private() {
        let tmp = tempfile::tempdir().unwrap();
        let s = Secrets::file_only(tmp.path());
        s.set(&keys::translation("deepl"), "secret").unwrap();
        let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(tmp.path().join(FALLBACK_FILE)).unwrap()).unwrap();
        assert_eq!(json, serde_json::json!({ "translate.deepl.key": "secret" }));
        assert!(!tmp.path().join("secrets.json.tmp").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(s.fallback_path()).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[test]
    fn a_corrupt_file_reads_as_an_error_but_can_be_repaired() {
        let tmp = tempfile::tempdir().unwrap();
        let s = Secrets::file_only(tmp.path());
        std::fs::write(s.fallback_path(), "{nope").unwrap();
        assert!(s.get(keys::MAL_TOKEN).is_err());
        s.delete(keys::MAL_TOKEN).unwrap();
        assert_eq!(s.get(keys::MAL_TOKEN).unwrap(), None);
    }

    #[test]
    fn wipe_removes_the_file() {
        let tmp = tempfile::tempdir().unwrap();
        let s = Secrets::file_only(tmp.path());
        s.set(keys::MAL_TOKEN, "m").unwrap();
        s.wipe(&[keys::MAL_TOKEN.to_owned()]).unwrap();
        assert!(!s.fallback_path().exists());
        s.wipe(&[]).unwrap();
    }

    #[test]
    fn key_helpers_follow_the_contract() {
        assert_eq!(keys::debrid("torbox"), "debrid.torbox.key");
        assert_eq!(keys::translation("openai"), "translate.openai.key");
    }
}
