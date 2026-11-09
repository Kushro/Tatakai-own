//! Persistent state of the server (worker **k**; classic `server/src/storage.mjs` + `node-storage.mjs`):
//! `<data>/server/state.json` = `{ id, settings, viewer: { hash } | null, sessions: { <deviceId>: { tokenHash, role,
//! label, createdAt, lastSeenAt } }, pairings: [...] }`, written atomically (temp file + rename). In-memory copy behind
//! a `Mutex`; every mutation persists. Contract stub: the shape is final, k implements the bodies.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::auth::{self, Pairing};

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
    token_hashes: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LivePairing {
    code: String,
    origin: String,
    expires_unix: i64,
}

pub struct ServerState {
    file: PathBuf,
    data: Mutex<Persisted>,
    pairings: Mutex<Vec<LivePairing>>,
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
        let s = Self { file, data: Mutex::new(data), pairings: Mutex::new(Vec::new()) };
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

    pub fn server_id(&self) -> Option<String> {
        self.data.lock().ok().and_then(|d| d.id.clone())
    }

    pub fn device_count(&self) -> u32 {
        self.data.lock().map(|d| d.devices.len() as u32).unwrap_or(0)
    }

    pub fn devices(&self) -> Vec<DeviceRecord> {
        self.data.lock().map(|d| d.devices.clone()).unwrap_or_default()
    }

    pub fn viewer_password_set(&self) -> bool {
        self.data.lock().map(|d| d.viewer_hash.is_some()).unwrap_or(false)
    }

    /// New single-use code (expired ones are dropped first). Contract stub for the limit: k adds `PAIRING_LIMIT`.
    pub fn create_pairing(&self, origin: &str) -> Pairing {
        let code = auth::new_code();
        if let Ok(mut p) = self.pairings.lock() {
            let now = auth::now_unix();
            p.retain(|x| x.expires_unix > now);
            p.push(LivePairing { code: code.clone(), origin: origin.to_owned(), expires_unix: now + i64::from(auth::PAIRING_TTL_SECS) });
        }
        let link = auth::deep_link(origin, &code);
        Pairing { code, expires_in: auth::PAIRING_TTL_SECS, url: link.clone(), deep_link: link }
    }

    /// Consume a code: `Some(device record + raw token)` when valid (k implements; stub rejects everything).
    pub fn pair(&self, _code: &str, _label: &str) -> Result<Option<(DeviceRecord, String)>, StateError> {
        Ok(None)
    }

    pub fn revoke(&self, device_id: &str) -> Result<(), StateError> {
        {
            let mut d = self.data.lock().map_err(|_| StateError::Poisoned)?;
            let before = d.devices.len();
            d.devices.retain(|x| x.id != device_id);
            d.token_hashes.remove(device_id);
            if d.devices.len() == before {
                return Err(StateError::NoDevice(device_id.to_owned()));
            }
        }
        self.persist()
    }

    /// Store the argon2id hash of `password` (`None` clears). Contract stub: k hashes with `argon2`.
    pub fn set_viewer_password(&self, password: Option<&str>) -> Result<(), StateError> {
        {
            let mut d = self.data.lock().map_err(|_| StateError::Poisoned)?;
            d.viewer_hash = password.map(|p| format!("plain-placeholder:{}", auth::token_hash(p)));
        }
        self.persist()
    }
}

#[cfg(test)]
mod tests {
    use super::ServerState;

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
}
