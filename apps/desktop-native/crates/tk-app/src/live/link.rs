//! Link server glue over tk-link (`start(LinkConfig, Arc<dyn LinkHost>)` -> `LinkHandle`).
//!
//! The server starts the first time the Link screen asks for status (so the OS firewall prompt never
//! appears at first launch - review E6) and runs until the process exits.

use std::path::PathBuf;
use std::sync::Arc;

use tk_ext::ExtensionHost;
use tk_link::{Device, LinkConfig, LinkHandle, LinkHost};
use tk_ui::backend::{DeviceVm, LinkStatusVm};
use tk_ui::model::{ErrorKind, Pairing, UiError};

/// What the Link server may ask of the app. M0: no library folder; FFmpeg only if `fetch-deps.ps1` ran.
/// Extension permissions come from the installed extension, so `url` sources can only reach its declared domains.
struct AppLinkHost {
    ext: Arc<ExtensionHost>,
}

impl LinkHost for AppLinkHost {
    fn extension_permissions(&self, id: &str) -> Option<Vec<String>> {
        self.ext.get(id).map(|e| e.permissions)
    }
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
    ext: Arc<ExtensionHost>,
    anime4k_dir: PathBuf,
    anime4k_lock: Option<PathBuf>,
    handle: tokio::sync::Mutex<Option<Arc<LinkHandle>>>,
}

fn server_name() -> String {
    std::env::var("COMPUTERNAME").ok().filter(|n| !n.trim().is_empty()).map_or_else(|| "Tatakai".to_owned(), |n| format!("Tatakai ({n})"))
}

fn now_unix() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

fn err(e: impl std::fmt::Display) -> UiError {
    UiError::new(ErrorKind::Other, e.to_string())
}

fn device_vm(d: Device) -> DeviceVm {
    // tk-link tracks no platform / last-seen; the pairing date is not "last seen", so leave it empty.
    DeviceVm { id: d.id, name: d.name, platform: String::new(), last_seen_unix: None }
}

impl LinkService {
    pub fn new(dir: PathBuf, ext: Arc<ExtensionHost>, anime4k_dir: PathBuf, anime4k_lock: Option<PathBuf>) -> Self {
        Self { dir, ext, anime4k_dir, anime4k_lock, handle: tokio::sync::Mutex::new(None) }
    }

    async fn ensure(&self) -> Result<Arc<LinkHandle>, UiError> {
        let mut guard = self.handle.lock().await;
        if let Some(h) = guard.as_ref() {
            return Ok(h.clone());
        }
        let mut cfg = LinkConfig::new(self.dir.clone(), server_name());
        cfg.anime4k_dir = Some(self.anime4k_dir.clone());
        cfg.anime4k_lock = self.anime4k_lock.clone();
        let host = Arc::new(AppLinkHost { ext: self.ext.clone() });
        let handle = Arc::new(tk_link::start(cfg, host).await.map_err(err)?);
        *guard = Some(handle.clone());
        Ok(handle)
    }

    pub async fn status(&self) -> Result<LinkStatusVm, UiError> {
        let h = self.ensure().await?;
        Ok(LinkStatusVm { running: true, port: crate::firewall::DEFAULT_PORT, devices: h.devices().into_iter().filter(|d| d.is_active(now_unix())).map(device_vm).collect() })
    }

    pub async fn open_pairing(&self) -> Result<Pairing, UiError> {
        let p = self.ensure().await?.open_pairing_window();
        Ok(Pairing { uri: p.uri(), code: p.code, expires_unix: p.expires_at.unix_timestamp() })
    }

    pub async fn revoke(&self, id: &str) -> Result<(), UiError> {
        self.ensure().await?.revoke(id);
        Ok(())
    }
}
