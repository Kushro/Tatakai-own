//! Persistent state of the server (worker **k**; classic `server/src/storage.mjs` + `node-storage.mjs`):
//! `<data>/server/state.json` = `{ id, viewerHash, devices: [DeviceRecord], tokenHashes: { <deviceId>: sha256(token) },
//! bootstrap, repositories, publicEpoch }`, written atomically (temp file + rename). In-memory copy behind a `Mutex`;
//! every mutation persists (the `lastSeenAt` touch of an authenticated request at most once a minute per device).
//! Pairing codes and login attempts live in memory only (a restart invalidates live codes, like the classic server's
//! 5-minute window).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::auth::{self, Pairing};

/// Device tokens kept at most (classic `device_limit`).
pub const DEVICE_LIMIT: usize = 200;
/// Repositories kept at most (classic `repository_limit`).
pub const REPOSITORY_LIMIT: usize = 50;
/// `lastSeenAt` is written to disk at most this often per device (it is always current in memory).
const LAST_SEEN_PERSIST_SECS: i64 = 60;
/// Failed logins / pair attempts per role inside [`ATTEMPT_WINDOW_SECS`] before `429 login_throttled`.
pub const MAX_ATTEMPTS: u32 = 10;
pub const ATTEMPT_WINDOW_SECS: i64 = 60;

#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("device {0} not found")]
    NoDevice(String),
    #[error("state lock poisoned")]
    Poisoned,
    /// `PAIRING_LIMIT` live codes (`409 pairing_limit`).
    #[error("too many live pairing codes")]
    PairingLimit,
    /// `DEVICE_LIMIT` tokens (`409 device_limit`).
    #[error("too many paired devices")]
    DeviceLimit,
    /// `REPOSITORY_LIMIT` repositories (`409 repository_limit`).
    #[error("too many repositories")]
    RepositoryLimit,
    /// Password outside 10..=1024 characters, or hashing failed.
    #[error("invalid password: {0}")]
    Password(String),
}

/// One paired device / token (`GET /devices` row, without the hash).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceRecord {
    pub id: String,
    pub label: String,
    /// `viewer` | `admin`.
    pub role: String,
    pub created_at: i64,
    #[serde(default)]
    pub last_seen_at: Option<i64>,
}

/// A catalogue repository mirrored for phones (`GET | POST /repositories`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Repository {
    pub id: String,
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Persisted {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    viewer_hash: Option<String>,
    #[serde(default)]
    devices: Vec<DeviceRecord>,
    /// `device id -> sha256(token)`.
    #[serde(default)]
    token_hashes: BTreeMap<String, String>,
    /// Secret of `X-Tatakai-Bootstrap` (owner token for local tools; the app itself never needs HTTP).
    #[serde(default)]
    bootstrap: Option<String>,
    #[serde(default)]
    repositories: Vec<Repository>,
    /// Changes when the viewer password changes, so anonymous media tickets minted before stop working.
    #[serde(default)]
    public_epoch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LivePairing {
    code: String,
    origin: String,
    expires_unix: i64,
}

/// `(count, window end)` of failed attempts per role.
#[derive(Debug, Default)]
struct Attempts(BTreeMap<String, (u32, i64)>);

pub struct ServerState {
    file: PathBuf,
    data: Mutex<Persisted>,
    pairings: Mutex<Vec<LivePairing>>,
    attempts: Mutex<Attempts>,
    /// Last time `lastSeenAt` was persisted per device.
    seen_persisted: Mutex<BTreeMap<String, i64>>,
}

impl ServerState {
    /// Load `<dir>/state.json` (missing = fresh state with a new server id).
    pub fn open(dir: &std::path::Path) -> Result<Self, StateError> {
        std::fs::create_dir_all(dir)?;
        let file = dir.join("state.json");
        let mut data: Persisted = match std::fs::read(&file) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Persisted::default(),
            Err(e) => return Err(e.into()),
        };
        if data.id.is_none() {
            data.id = Some(uuid::Uuid::new_v4().simple().to_string());
        }
        if data.bootstrap.is_none() {
            data.bootstrap = Some(auth::new_token());
        }
        // A hash from the contract stub (never a real password) is dropped instead of locking everyone out.
        if data.viewer_hash.as_deref().is_some_and(|h| !h.starts_with("$argon2")) {
            data.viewer_hash = None;
        }
        let s = Self {
            file,
            data: Mutex::new(data),
            pairings: Mutex::new(Vec::new()),
            attempts: Mutex::new(Attempts::default()),
            seen_persisted: Mutex::new(BTreeMap::new()),
        };
        s.persist()?;
        Ok(s)
    }

    fn persist(&self) -> Result<(), StateError> {
        let data = self.data.lock().map_err(|_| StateError::Poisoned)?.clone();
        let tmp = self.file.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&data)?)?;
        std::fs::rename(&tmp, &self.file)?;
        Ok(())
    }

    fn mutate<R>(&self, f: impl FnOnce(&mut Persisted) -> Result<R, StateError>) -> Result<R, StateError> {
        let r = {
            let mut d = self.data.lock().map_err(|_| StateError::Poisoned)?;
            f(&mut d)?
        };
        self.persist()?;
        Ok(r)
    }

    pub fn server_id(&self) -> Option<String> {
        self.data.lock().ok().and_then(|d| d.id.clone())
    }

    pub fn device_count(&self) -> u32 {
        self.data.lock().map(|d| d.devices.iter().filter(|x| x.role == "viewer").count() as u32).unwrap_or(0)
    }

    pub fn devices(&self) -> Vec<DeviceRecord> {
        self.data.lock().map(|d| d.devices.clone()).unwrap_or_default()
    }

    pub fn device(&self, id: &str) -> Option<DeviceRecord> {
        self.data.lock().ok().and_then(|d| d.devices.iter().find(|x| x.id == id).cloned())
    }

    pub fn viewer_password_set(&self) -> bool {
        self.data.lock().map(|d| d.viewer_hash.is_some()).unwrap_or(false)
    }

    /// Session id of anonymous viewers (no pairing required, no viewer password): `public:<epoch>`.
    pub fn public_session_id(&self) -> String {
        let d = self.data.lock();
        let epoch = d.ok().and_then(|d| d.public_epoch.clone().or_else(|| d.id.clone())).unwrap_or_default();
        format!("public:{epoch}")
    }

    /// Codes still alive (expired ones are dropped first).
    pub fn live_pairings(&self) -> usize {
        let now = auth::now_unix();
        self.pairings
            .lock()
            .map(|mut p| {
                p.retain(|x| x.expires_unix > now);
                p.len()
            })
            .unwrap_or(0)
    }

    /// New single-use code (expired ones are dropped first). At the limit the oldest live code is dropped; the
    /// HTTP route and [`crate::ServerHandle::create_pairing`] use [`Self::try_create_pairing`] instead (409).
    pub fn create_pairing(&self, origin: &str) -> Pairing {
        let code = auth::new_code();
        if let Ok(mut p) = self.pairings.lock() {
            let now = auth::now_unix();
            p.retain(|x| x.expires_unix > now && x.code != code);
            while p.len() >= auth::PAIRING_LIMIT {
                p.remove(0);
            }
            p.push(LivePairing { code: code.clone(), origin: origin.to_owned(), expires_unix: now + i64::from(auth::PAIRING_TTL_SECS) });
        }
        let link = auth::deep_link(origin, &code);
        Pairing { code, expires_in: auth::PAIRING_TTL_SECS, url: link.clone(), deep_link: link }
    }

    /// Like [`Self::create_pairing`] but refuses with [`StateError::PairingLimit`] when 50 codes are alive.
    pub fn try_create_pairing(&self, origin: &str) -> Result<Pairing, StateError> {
        if self.live_pairings() >= auth::PAIRING_LIMIT {
            return Err(StateError::PairingLimit);
        }
        Ok(self.create_pairing(origin))
    }

    /// Consume a code: `Some(device record + raw token)` when valid (unknown, used or expired -> `None`).
    pub fn pair(&self, code: &str, label: &str) -> Result<Option<(DeviceRecord, String)>, StateError> {
        let code = code.trim();
        if code.is_empty() || code.len() > 64 {
            return Ok(None);
        }
        let found = {
            let mut p = self.pairings.lock().map_err(|_| StateError::Poisoned)?;
            let now = auth::now_unix();
            p.retain(|x| x.expires_unix > now);
            match p.iter().position(|x| auth::secrets_equal(&x.code, code)) {
                Some(i) => {
                    p.remove(i);
                    true
                }
                None => false,
            }
        };
        if !found {
            return Ok(None);
        }
        self.issue("viewer", label).map(Some)
    }

    /// Create a device token for `role` (`viewer` | `admin`); the raw token is returned once, only its hash is kept.
    pub fn issue(&self, role: &str, label: &str) -> Result<(DeviceRecord, String), StateError> {
        let label = sanitize_label(label);
        let token = auth::new_token();
        let hash = auth::token_hash(&token);
        let record = DeviceRecord {
            id: uuid::Uuid::new_v4().simple().to_string(),
            label,
            role: if role == "admin" { "admin".to_owned() } else { "viewer".to_owned() },
            created_at: auth::now_unix(),
            last_seen_at: Some(auth::now_unix()),
        };
        let rec = record.clone();
        self.mutate(move |d| {
            if d.devices.len() >= DEVICE_LIMIT {
                return Err(StateError::DeviceLimit);
            }
            d.token_hashes.insert(rec.id.clone(), hash);
            d.devices.push(rec);
            Ok(())
        })?;
        Ok((record, token))
    }

    /// The device of a bearer token (`None` = unknown or revoked); refreshes its `lastSeenAt`.
    pub fn authenticate(&self, token: &str) -> Option<DeviceRecord> {
        if token.is_empty() || token.len() > 256 {
            return None;
        }
        let hash = auth::token_hash(token);
        let now = auth::now_unix();
        let (record, persist) = {
            let mut d = self.data.lock().ok()?;
            let id = d.token_hashes.iter().find(|(_, h)| auth::secrets_equal(h, &hash)).map(|(id, _)| id.clone())?;
            let dev = d.devices.iter_mut().find(|x| x.id == id)?;
            dev.last_seen_at = Some(now);
            let record = dev.clone();
            let mut seen = self.seen_persisted.lock().ok()?;
            let last = seen.get(&id).copied().unwrap_or(0);
            let persist = now - last >= LAST_SEEN_PERSIST_SECS;
            if persist {
                seen.insert(id, now);
            }
            (record, persist)
        };
        if persist && let Err(e) = self.persist() {
            tracing::debug!(error = %e, "could not persist lastSeenAt");
        }
        Some(record)
    }

    pub fn revoke(&self, device_id: &str) -> Result<(), StateError> {
        self.mutate(|d| {
            let before = d.devices.len();
            d.devices.retain(|x| x.id != device_id);
            d.token_hashes.remove(device_id);
            if d.devices.len() == before {
                return Err(StateError::NoDevice(device_id.to_owned()));
            }
            Ok(())
        })
    }

    /// Store the argon2id hash of `password` (`None` clears). Live pairing codes are dropped and anonymous media
    /// tickets stop working (new public epoch), like the classic `POST /auth/password`.
    pub fn set_viewer_password(&self, password: Option<&str>) -> Result<(), StateError> {
        let hash = match password {
            Some(p) if !auth::password_len_ok(p) => {
                return Err(StateError::Password(format!(
                    "use between {} and {} characters",
                    auth::PASSWORD_MIN_CHARS,
                    auth::PASSWORD_MAX_CHARS
                )));
            }
            Some(p) => Some(auth::hash_password(p).map_err(StateError::Password)?),
            None => None,
        };
        if let Ok(mut p) = self.pairings.lock() {
            p.clear();
        }
        self.mutate(move |d| {
            d.viewer_hash = hash;
            d.public_epoch = Some(uuid::Uuid::new_v4().simple().to_string());
            Ok(())
        })
    }

    /// `POST /auth/login` viewer check (no password set = `false`).
    pub fn verify_viewer_password(&self, password: &str) -> bool {
        let hash = self.data.lock().ok().and_then(|d| d.viewer_hash.clone());
        hash.is_some_and(|h| auth::verify_password(password, &h))
    }

    /// `X-Tatakai-Bootstrap` check (owner access for local tools).
    pub fn bootstrap_ok(&self, secret: &str) -> bool {
        let stored = self.data.lock().ok().and_then(|d| d.bootstrap.clone());
        !secret.is_empty() && stored.is_some_and(|s| auth::secrets_equal(&s, secret))
    }

    /// Count one failed attempt for `role`; `false` once the budget of the window is spent.
    pub fn note_failure(&self, role: &str) -> bool {
        let now = auth::now_unix();
        let Ok(mut a) = self.attempts.lock() else { return false };
        let e = a.0.entry(role.to_owned()).or_insert((0, now + ATTEMPT_WINDOW_SECS));
        if e.1 <= now {
            *e = (0, now + ATTEMPT_WINDOW_SECS);
        }
        e.0 += 1;
        e.0 <= MAX_ATTEMPTS
    }

    /// Whether `role` is throttled right now.
    pub fn throttled(&self, role: &str) -> bool {
        let now = auth::now_unix();
        self.attempts.lock().map(|a| a.0.get(role).is_some_and(|(n, until)| *until > now && *n >= MAX_ATTEMPTS)).unwrap_or(false)
    }

    pub fn repositories(&self) -> Vec<Repository> {
        self.data.lock().map(|d| d.repositories.clone()).unwrap_or_default()
    }

    pub fn add_repository(&self, name: &str, url: &str) -> Result<Repository, StateError> {
        let row = Repository {
            id: uuid::Uuid::new_v4().simple().to_string(),
            name: name.trim().chars().take(100).collect(),
            url: url.to_owned(),
        };
        let out = row.clone();
        self.mutate(move |d| {
            if d.repositories.iter().any(|r| r.url == row.url) {
                return Ok(());
            }
            if d.repositories.len() >= REPOSITORY_LIMIT {
                return Err(StateError::RepositoryLimit);
            }
            d.repositories.push(row);
            Ok(())
        })?;
        Ok(self.repositories().into_iter().find(|r| r.url == out.url).unwrap_or(out))
    }

    pub fn remove_repository(&self, id: &str) -> Result<bool, StateError> {
        self.mutate(|d| {
            let before = d.repositories.len();
            d.repositories.retain(|r| r.id != id);
            Ok(d.repositories.len() != before)
        })
    }
}

/// Device labels as the classic server stores them: trimmed, control characters removed, at most 100 characters,
/// `Device` when empty.
pub fn sanitize_label(label: &str) -> String {
    let clean: String = label.trim().chars().filter(|c| !c.is_control()).take(100).collect();
    if clean.is_empty() { "Device".to_owned() } else { clean }
}

#[cfg(test)]
mod tests {
    use super::{ServerState, StateError, sanitize_label};

    #[test]
    fn fresh_state_gets_an_id_and_persists() {
        let dir = tempfile::tempdir().unwrap();
        let s = ServerState::open(dir.path()).unwrap();
        let id = s.server_id().unwrap();
        assert!(!id.is_empty());
        let again = ServerState::open(dir.path()).unwrap();
        assert_eq!(again.server_id().unwrap(), id);
        assert_eq!(again.device_count(), 0);
    }

    #[test]
    fn pairing_codes_are_six_digits_with_a_deep_link() {
        let dir = tempfile::tempdir().unwrap();
        let s = ServerState::open(dir.path()).unwrap();
        let p = s.create_pairing("http://10.0.0.2:8099");
        assert_eq!(p.code.len(), 6);
        assert!(p.deep_link.starts_with("tatakai://extensionserver/connect?server="));
        assert_eq!(p.url, p.deep_link);
    }

    #[test]
    fn a_code_pairs_once_and_the_token_authenticates_until_revoked() {
        let dir = tempfile::tempdir().unwrap();
        let s = ServerState::open(dir.path()).unwrap();
        let p = s.create_pairing("http://10.0.0.2:8099");
        assert!(s.pair("000000x", "Phone").unwrap().is_none(), "unknown code");
        let (dev, token) = s.pair(&p.code, " Pixel\u{7} ").unwrap().expect("valid code");
        assert_eq!(dev.label, "Pixel");
        assert_eq!(dev.role, "viewer");
        assert!(s.pair(&p.code, "again").unwrap().is_none(), "single use");
        assert_eq!(s.authenticate(&token).unwrap().id, dev.id);
        assert!(s.authenticate("nope").is_none());
        // persisted: a reopened state still knows the token
        let again = ServerState::open(dir.path()).unwrap();
        assert_eq!(again.authenticate(&token).unwrap().id, dev.id);
        s.revoke(&dev.id).unwrap();
        assert!(s.authenticate(&token).is_none());
        assert!(matches!(s.revoke(&dev.id), Err(StateError::NoDevice(_))));
    }

    #[test]
    fn viewer_password_is_argon2id_and_never_plain() {
        let dir = tempfile::tempdir().unwrap();
        let s = ServerState::open(dir.path()).unwrap();
        assert!(matches!(s.set_viewer_password(Some("short")), Err(StateError::Password(_))));
        s.set_viewer_password(Some("a long viewer password")).unwrap();
        assert!(s.viewer_password_set());
        assert!(s.verify_viewer_password("a long viewer password"));
        assert!(!s.verify_viewer_password("another long password"));
        let raw = std::fs::read_to_string(dir.path().join("state.json")).unwrap();
        assert!(raw.contains("$argon2id$") && !raw.contains("a long viewer password"));
        s.set_viewer_password(None).unwrap();
        assert!(!s.viewer_password_set());
    }

    #[test]
    fn pairing_limit_and_throttle() {
        let dir = tempfile::tempdir().unwrap();
        let s = ServerState::open(dir.path()).unwrap();
        for _ in 0..crate::auth::PAIRING_LIMIT {
            s.try_create_pairing("http://h:1").unwrap();
        }
        assert!(matches!(s.try_create_pairing("http://h:1"), Err(StateError::PairingLimit)));
        for _ in 0..super::MAX_ATTEMPTS {
            assert!(s.note_failure("viewer"));
        }
        assert!(!s.note_failure("viewer"));
        assert!(s.throttled("viewer"));
        assert!(!s.throttled("pair"));
    }

    #[test]
    fn repositories_dedupe_by_url() {
        let dir = tempfile::tempdir().unwrap();
        let s = ServerState::open(dir.path()).unwrap();
        let a = s.add_repository("Repo", "https://r.example/index.json").unwrap();
        let b = s.add_repository("Again", "https://r.example/index.json").unwrap();
        assert_eq!(a.id, b.id);
        assert_eq!(s.repositories().len(), 1);
        assert!(s.remove_repository(&a.id).unwrap());
        assert!(s.repositories().is_empty());
        assert_eq!(sanitize_label("   "), "Device");
    }
}
