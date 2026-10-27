//! Extension host glue. Written against docs/redesign/04 section 3.3:
//!
//! ```text
//! pub struct ExtensionHost;  // install_kai(path) -> InstalledExtension ; list() ; set_enabled ; remove
//! impl ExtensionHost { pub async fn resolve_streams(&self, media: &Media, episode: u32) -> Vec<StreamSource>; }
//! ```
//!
//! CONTRACT-GUESS (tk-ext was a placeholder when this was written; adjust to the real signatures):
//! * `ExtensionHost::new(dir: PathBuf) -> Result<Self, ExtError>`
//! * `install_kai(&self, &Path) -> Result<InstalledExtension, ExtError>` (sync)
//! * `list(&self) -> Vec<InstalledExtension>`, `set_enabled(&self, &str, bool) -> Result<(), ExtError>`,
//!   `remove(&self, &str) -> Result<(), ExtError>` (sync)
//! * `ExtError: Display`

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tk_core::{Compat, InstalledExtension, Media, StreamSource};
use tk_ext::ExtensionHost;
use tk_ui::backend::ExtensionVm;
use tk_ui::model::{CompatLevel, ErrorKind, UiError};

pub struct Extensions {
    host: Arc<ExtensionHost>,
}

fn to_vm(e: InstalledExtension) -> ExtensionVm {
    ExtensionVm {
        id: e.id,
        name: e.name,
        version: e.version,
        compat: match e.compat {
            Compat::Full => CompatLevel::Full,
            Compat::Partial => CompatLevel::Partial,
            Compat::Unsupported => CompatLevel::Unsupported,
        },
        enabled: e.enabled,
        permissions: e.permissions,
    }
}

fn err(e: impl std::fmt::Display) -> UiError {
    UiError::new(ErrorKind::Extension, e.to_string())
}

impl Extensions {
    pub fn new(dir: PathBuf) -> anyhow::Result<Self> {
        Ok(Self { host: Arc::new(ExtensionHost::new(dir).map_err(|e| anyhow::anyhow!("{e}"))?) })
    }

    pub async fn list(&self) -> Result<Vec<ExtensionVm>, UiError> {
        let host = self.host.clone();
        tokio::task::spawn_blocking(move || host.list().into_iter().map(to_vm).collect())
            .await
            .map_err(err)
    }

    pub async fn install(&self, path: &str) -> Result<ExtensionVm, UiError> {
        let (host, path) = (self.host.clone(), Path::new(path).to_path_buf());
        tokio::task::spawn_blocking(move || host.install_kai(&path).map(to_vm).map_err(err)).await.map_err(err)?
    }

    pub async fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), UiError> {
        let (host, id) = (self.host.clone(), id.to_owned());
        tokio::task::spawn_blocking(move || host.set_enabled(&id, enabled).map_err(err)).await.map_err(err)?
    }

    pub async fn remove(&self, id: &str) -> Result<(), UiError> {
        let (host, id) = (self.host.clone(), id.to_owned());
        tokio::task::spawn_blocking(move || host.remove(&id).map_err(err)).await.map_err(err)?
    }

    /// Streams from every enabled extension, best first (the host orders them).
    pub async fn resolve(&self, media: &Media, episode: u32) -> Vec<StreamSource> {
        self.host.resolve_streams(media, episode).await
    }
}
