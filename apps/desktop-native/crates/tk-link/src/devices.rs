//! Paired-device registry: in-memory list + JSON file, hot revocation (live connections of a revoked device are closed).

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, PoisonError};

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    /// SHA-256 (hex) of the client certificate DER.
    pub fingerprint: String,
    pub created_at: i64,
    pub not_after: i64,
    #[serde(default)]
    pub revoked_at: Option<i64>,
}

impl Device {
    pub fn is_active(&self, now: i64) -> bool {
        self.revoked_at.is_none() && now < self.not_after
    }
}

pub struct DeviceRegistry {
    path: Option<PathBuf>,
    devices: Mutex<Vec<Device>>,
    revoked_tx: broadcast::Sender<String>,
}

impl DeviceRegistry {
    /// File-backed registry (`path` is injected). A missing file starts empty; a corrupt one is an error.
    pub fn open(path: PathBuf) -> std::io::Result<Self> {
        let devices = match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).map_err(std::io::Error::other)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e),
        };
        Ok(Self { path: Some(path), devices: Mutex::new(devices), revoked_tx: broadcast::channel(64).0 })
    }

    pub fn in_memory() -> Self {
        Self { path: None, devices: Mutex::new(Vec::new()), revoked_tx: broadcast::channel(64).0 }
    }

    fn lock(&self) -> MutexGuard<'_, Vec<Device>> {
        self.devices.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn persist(&self, devices: &[Device]) -> std::io::Result<()> {
        let Some(path) = &self.path else { return Ok(()) };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(devices).map_err(std::io::Error::other)?)?;
        std::fs::rename(&tmp, path)
    }

    pub fn add(&self, d: Device) -> std::io::Result<()> {
        let mut g = self.lock();
        g.push(d);
        self.persist(&g)
    }

    pub fn list(&self) -> Vec<Device> {
        self.lock().clone()
    }

    pub fn get(&self, id: &str) -> Option<Device> {
        self.lock().iter().find(|d| d.id == id).cloned()
    }

    pub fn by_fingerprint(&self, fp: &str) -> Option<Device> {
        self.lock().iter().find(|d| d.fingerprint == fp).cloned()
    }

    pub fn is_active(&self, id: &str, now: i64) -> bool {
        self.lock().iter().any(|d| d.id == id && d.is_active(now))
    }

    /// Revoke immediately (in memory, effective on the next request) and notify live connections.
    pub fn revoke(&self, id: &str, now: i64) -> bool {
        let changed = {
            let mut g = self.lock();
            let Some(d) = g.iter_mut().find(|d| d.id == id && d.revoked_at.is_none()) else { return false };
            d.revoked_at = Some(now);
            if let Err(e) = self.persist(&g) {
                tracing::error!(error = %e, "could not persist device revocation");
            }
            true
        };
        let _ = self.revoked_tx.send(id.to_owned());
        changed
    }

    pub fn rename(&self, id: &str, name: &str) -> bool {
        let mut g = self.lock();
        let Some(d) = g.iter_mut().find(|d| d.id == id) else { return false };
        d.name = name.to_owned();
        let _ = self.persist(&g);
        true
    }

    /// Subscribe to revocation events (device ids).
    pub fn subscribe_revocations(&self) -> broadcast::Receiver<String> {
        self.revoked_tx.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(id: &str) -> Device {
        Device { id: id.into(), name: "Phone".into(), fingerprint: format!("fp-{id}"), created_at: 0, not_after: 1000, revoked_at: None }
    }

    #[test]
    fn persists_revocation_and_notifies() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("link").join("devices.json");
        let reg = DeviceRegistry::open(path.clone()).unwrap();
        let mut rx = reg.subscribe_revocations();
        reg.add(dev("a")).unwrap();
        assert!(reg.is_active("a", 10));
        assert!(!reg.is_active("a", 1000), "expired certificate");
        assert!(reg.revoke("a", 20));
        assert!(!reg.is_active("a", 21));
        assert!(!reg.revoke("a", 22), "second revoke is a no-op");
        assert_eq!(rx.try_recv().unwrap(), "a");
        let again = DeviceRegistry::open(path).unwrap();
        assert!(!again.is_active("a", 21));
        assert_eq!(again.by_fingerprint("fp-a").unwrap().revoked_at, Some(20));
    }
}
