//! Link server glue. Written against docs/redesign/04 section 3.3:
//!
//! ```text
//! pub trait LinkHost: Send + Sync + 'static { fn library(&self) -> Vec<LibraryItem>; fn library_path(&self, id: &str) -> Option<PathBuf>; fn ffmpeg_path(&self) -> Option<PathBuf>; }
//! pub async fn start(cfg: LinkConfig, host: Arc<dyn LinkHost>) -> Result<LinkHandle, LinkError>;
//! impl LinkHandle { pub fn open_pairing_window(&self) -> PairingPayload /* uri + code + expires */; pub fn devices(&self) -> Vec<Device>;
//!   pub fn revoke(&self, id: &str); pub fn events(&self) -> broadcast::Receiver<LinkEvent>; pub async fn shutdown(self); }
//! ```
//!
//! CONTRACT-GUESS (tk-link was a placeholder when this was written; adjust to the real types):
//! * `LinkConfig::new(data_dir: PathBuf)` (default port 47321)
//! * `PairingPayload { uri: String, code: String, expires_at: i64 /* unix seconds */ }`
//! * `Device { id: String, name: String, platform: String, last_seen: Option<i64> /* unix seconds */ }`
//! * `LinkError: Display`
//!
//! The server starts the first time the Link screen asks for status (so the OS firewall prompt never
//! appears at first launch - review E6) and runs until the process exits.

use std::path::PathBuf;
use std::sync::Arc;

use tk_link::{Device, LinkConfig, LinkHandle, LinkHost};
use tk_ui::backend::{DeviceVm, LinkStatusVm};
use tk_ui::model::{ErrorKind, Pairing, UiError};

/// What the Link server may ask of the app. M0: no library folder; FFmpeg only if `fetch-deps.ps1` ran.
struct AppLinkHost;

impl LinkHost for AppLinkHost {
    fn library(&self) -> Vec<tk_link::LibraryItem> {
        Vec::new()
    }
    fn library_path(&self, _id: &str) -> Option<PathBuf> {
        None
    }
    fn ffmpeg_path(&self) -> Option<PathBuf> {
        crate::paths::ffmpeg_path(None)
    }
}

pub struct LinkService {
    dir: PathBuf,
    handle: tokio::sync::Mutex<Option<Arc<LinkHandle>>>,
}

fn err(e: impl std::fmt::Display) -> UiError {
    UiError::new(ErrorKind::Other, e.to_string())
}

fn device_vm(d: Device) -> DeviceVm {
    DeviceVm { id: d.id, name: d.name, platform: d.platform, last_seen_unix: d.last_seen }
}

impl LinkService {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir, handle: tokio::sync::Mutex::new(None) }
    }

    async fn ensure(&self) -> Result<Arc<LinkHandle>, UiError> {
        let mut guard = self.handle.lock().await;
        if let Some(h) = guard.as_ref() {
            return Ok(h.clone());
        }
        let handle = Arc::new(tk_link::start(LinkConfig::new(self.dir.clone()), Arc::new(AppLinkHost)).await.map_err(err)?);
        *guard = Some(handle.clone());
        Ok(handle)
    }

    pub async fn status(&self) -> Result<LinkStatusVm, UiError> {
        let h = self.ensure().await?;
        Ok(LinkStatusVm { running: true, port: crate::firewall::DEFAULT_PORT, devices: h.devices().into_iter().map(device_vm).collect() })
    }

    pub async fn open_pairing(&self) -> Result<Pairing, UiError> {
        let p = self.ensure().await?.open_pairing_window();
        Ok(Pairing { uri: p.uri, code: p.code, expires_unix: p.expires_at })
    }

    pub async fn revoke(&self, id: &str) -> Result<(), UiError> {
        self.ensure().await?.revoke(id);
        Ok(())
    }
}
