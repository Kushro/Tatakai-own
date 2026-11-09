//! Paired-device registry: in-memory list + JSON file, hot revocation (live connections of a revoked device are closed).
//!
//! Wave B (worker k): `platform` and `last_seen_at` per device (`link.last-seen`: refreshed by every authenticated
//! request through [`DeviceRegistry::touch`], written to disk at most once a minute per device), and the per-device
//! event mailbox of `GET /v1/events` (`link.send-to-device`: [`DeviceRegistry::push_event`] /
//! [`DeviceRegistry::take_events`], at most [`EVENTS_QUEUE_MAX`] per device, each kept [`EVENT_TTL_SECS`]).

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde::{Deserialize, Serialize};
use tokio::sync::{Notify, broadcast};

use crate::control::{EVENTS_QUEUE_MAX, PhoneEvent};

/// Queued events older than this are dropped (contract 16 section 3.8: 10 minutes).
pub const EVENT_TTL_SECS: i64 = 10 * 60;
/// `last_seen_at` reaches `devices.json` at most this often per device (it is always current in memory).
pub const LAST_SEEN_PERSIST_SECS: i64 = 60;

/// Platform of a phone from its `User-Agent` (`okhttp` / `Dalvik` / `Android` -> `android`, iPhone / iPad / iOS ->
/// `ios`); `None` when the agent says nothing useful. Pure.
pub fn platform_from_user_agent(ua: &str) -> Option<&'static str> {
    let l = ua.to_ascii_lowercase();
    if l.contains("android") || l.contains("okhttp") || l.contains("dalvik") {
        Some("android")
    } else if l.contains("iphone") || l.contains("ipad") || l.contains("ios") || l.contains("cfnetwork") {
        Some("ios")
    } else {
        None
    }
}

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
    /// `android` | `ios` | empty (unknown). Set at pairing and refined from the `User-Agent` of later requests.
    #[serde(default)]
    pub platform: String,
    /// Unix seconds of the last authenticated request (`None` = never seen after pairing).
    #[serde(default)]
    pub last_seen_at: Option<i64>,
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
    /// `device id -> (queued at, event)` of `GET /v1/events`.
    events: Mutex<HashMap<String, VecDeque<(i64, PhoneEvent)>>>,
    /// Wakes the long-polls of `GET /v1/events` when something is queued.
    events_ready: Arc<Notify>,
    /// Last time `last_seen_at` was persisted per device.
    seen_persisted: Mutex<HashMap<String, i64>>,
}

impl DeviceRegistry {
    /// File-backed registry (`path` is injected). A missing file starts empty; a corrupt one is an error.
    pub fn open(path: PathBuf) -> std::io::Result<Self> {
        let devices = match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).map_err(std::io::Error::other)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e),
        };
        Ok(Self::with(Some(path), devices))
    }

    pub fn in_memory() -> Self {
        Self::with(None, Vec::new())
    }

    fn with(path: Option<PathBuf>, devices: Vec<Device>) -> Self {
        Self {
            path,
            devices: Mutex::new(devices),
            revoked_tx: broadcast::channel(64).0,
            events: Mutex::new(HashMap::new()),
            events_ready: Arc::new(Notify::new()),
            seen_persisted: Mutex::new(HashMap::new()),
        }
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

    /// Set the display name (persisted in `devices.json`, so it survives restarts). Empty names and unknown or
    /// revoked devices are refused.
    pub fn rename(&self, id: &str, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        let mut g = self.lock();
        let Some(d) = g.iter_mut().find(|d| d.id == id && d.revoked_at.is_none()) else { return false };
        d.name = name.to_owned();
        if let Err(e) = self.persist(&g) {
            tracing::error!(error = %e, "could not persist device name");
        }
        true
    }

    /// Subscribe to revocation events (device ids).
    pub fn subscribe_revocations(&self) -> broadcast::Receiver<String> {
        self.revoked_tx.subscribe()
    }

    /// An authenticated request of `id` at `now` (`link.last-seen`): refresh `last_seen_at` and, when the
    /// `User-Agent` tells, the platform. Persisted at most every [`LAST_SEEN_PERSIST_SECS`] per device.
    pub fn touch(&self, id: &str, now: i64, user_agent: Option<&str>) {
        let platform = user_agent.and_then(platform_from_user_agent);
        let mut g = self.lock();
        let Some(d) = g.iter_mut().find(|d| d.id == id) else { return };
        d.last_seen_at = Some(now);
        let platform_changed = platform.is_some_and(|p| d.platform != p);
        if let Some(p) = platform {
            d.platform = p.to_owned();
        }
        let due = {
            let mut seen = self.seen_persisted.lock().unwrap_or_else(PoisonError::into_inner);
            let last = seen.get(id).copied().unwrap_or(0);
            let due = platform_changed || now - last >= LAST_SEEN_PERSIST_SECS;
            if due {
                seen.insert(id.to_owned(), now);
            }
            due
        };
        if due && let Err(e) = self.persist(&g) {
            tracing::debug!(error = %e, "could not persist last_seen_at");
        }
    }

    /// Queue `event` for `id` (oldest dropped above [`EVENTS_QUEUE_MAX`]) and wake its long-poll. `false` when the
    /// device is unknown or revoked.
    pub fn push_event(&self, id: &str, event: PhoneEvent, now: i64) -> bool {
        if !self.lock().iter().any(|d| d.id == id && d.revoked_at.is_none()) {
            return false;
        }
        {
            let mut q = self.events.lock().unwrap_or_else(PoisonError::into_inner);
            let list = q.entry(id.to_owned()).or_default();
            list.retain(|(at, _)| now - *at < EVENT_TTL_SECS);
            list.push_back((now, event));
            while list.len() > EVENTS_QUEUE_MAX {
                list.pop_front();
            }
        }
        self.events_ready.notify_waiters();
        true
    }

    /// Drain the live events of `id` (expired ones are dropped), oldest first.
    pub fn take_events(&self, id: &str, now: i64) -> Vec<PhoneEvent> {
        let mut q = self.events.lock().unwrap_or_else(PoisonError::into_inner);
        q.remove(id).map(|l| l.into_iter().filter(|(at, _)| now - *at < EVENT_TTL_SECS).map(|(_, e)| e).collect()).unwrap_or_default()
    }

    /// Notified whenever an event is queued for any device (the long-poll re-checks its own queue).
    pub fn events_ready(&self) -> Arc<Notify> {
        self.events_ready.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(id: &str) -> Device {
        Device {
            id: id.into(),
            name: "Phone".into(),
            fingerprint: format!("fp-{id}"),
            created_at: 0,
            not_after: 1000,
            revoked_at: None,
            platform: String::new(),
            last_seen_at: None,
        }
    }

    #[test]
    fn touch_tracks_last_seen_and_platform() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("devices.json");
        let reg = DeviceRegistry::open(path.clone()).unwrap();
        reg.add(dev("a")).unwrap();
        reg.touch("a", 100, Some("okhttp/4.12.0"));
        assert_eq!(reg.get("a").unwrap().last_seen_at, Some(100));
        assert_eq!(reg.get("a").unwrap().platform, "android");
        reg.touch("a", 110, None);
        assert_eq!(reg.get("a").unwrap().last_seen_at, Some(110), "always current in memory");
        let disk = DeviceRegistry::open(path).unwrap();
        assert_eq!(disk.get("a").unwrap().last_seen_at, Some(100), "persisted at most once a minute");
        assert_eq!(platform_from_user_agent("Mozilla/5.0 (iPhone; CPU iPhone OS 17_0)"), Some("ios"));
        assert_eq!(platform_from_user_agent("curl/8"), None);
    }

    #[test]
    fn event_mailbox_is_bounded_and_drained() {
        let reg = DeviceRegistry::in_memory();
        reg.add(dev("a")).unwrap();
        let ev = |n: u32| PhoneEvent::Handoff { session_id: format!("s{n}"), media_id: None, episode: Some(n), position_secs: 0, title: None };
        assert!(!reg.push_event("zzz", ev(0), 0), "unknown device");
        for n in 0..(EVENTS_QUEUE_MAX as u32 + 3) {
            assert!(reg.push_event("a", ev(n), 10));
        }
        let got = reg.take_events("a", 11);
        assert_eq!(got.len(), EVENTS_QUEUE_MAX);
        assert_eq!(got[0], ev(3), "oldest dropped first");
        assert!(reg.take_events("a", 11).is_empty(), "drained on delivery");
        reg.push_event("a", ev(1), 0);
        assert!(reg.take_events("a", EVENT_TTL_SECS + 1).is_empty(), "expired");
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

    #[test]
    fn rename_persists_and_refuses_bad_input() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("devices.json");
        let reg = DeviceRegistry::open(path.clone()).unwrap();
        reg.add(dev("a")).unwrap();
        reg.add(dev("b")).unwrap();
        assert!(reg.rename("a", "  Pixel de Ana "));
        assert!(!reg.rename("a", "   "), "empty name");
        assert!(!reg.rename("zzz", "x"), "unknown device");
        assert!(reg.revoke("b", 5));
        assert!(!reg.rename("b", "x"), "revoked device");
        let again = DeviceRegistry::open(path).unwrap();
        assert_eq!(again.get("a").unwrap().name, "Pixel de Ana");
        assert_eq!(again.get("b").unwrap().name, "Phone");
    }
}
