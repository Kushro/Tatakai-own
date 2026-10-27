//! Backend used when the `live` feature is off: settings persistence, firewall flow and window chrome
//! work; every catalogue / extension / Link call returns an explicit "not wired" error that the UI shows
//! inline. This is NOT the shipping backend - see the `live` feature in Cargo.toml.

use std::path::PathBuf;

use async_trait::async_trait;
use tk_ui::backend::{
    AccountVm, Backend, DetailsVm, EpisodeVm, ExtensionVm, HomeVm, LinkStatusVm, MediaVm, PlaybackVm, UiResult,
};
use tk_ui::model::{ErrorKind, Pairing, Settings, UiError};

use crate::settings_store;

pub struct OfflineBackend {
    settings_file: PathBuf,
}

impl OfflineBackend {
    pub fn new(settings_file: PathBuf) -> Self {
        Self { settings_file }
    }
}

fn not_wired<T>() -> UiResult<T> {
    Err(UiError::new(ErrorKind::Other, "tk-core / tk-ext / tk-link are not linked in this build (enable the `live` feature)"))
}

#[async_trait]
impl Backend for OfflineBackend {
    async fn home(&self) -> UiResult<HomeVm> {
        not_wired()
    }
    async fn search(&self, _query: &str, _page: u32) -> UiResult<Vec<MediaVm>> {
        not_wired()
    }
    async fn details(&self, _id: &str) -> UiResult<DetailsVm> {
        not_wired()
    }
    async fn episodes(&self, _id: &str) -> UiResult<Vec<EpisodeVm>> {
        not_wired()
    }
    async fn playback(&self, _id: &str, _episode: u32) -> UiResult<PlaybackVm> {
        not_wired()
    }
    async fn record_progress(&self, _id: &str, _episode: u32, _progress_secs: u32, _duration_secs: Option<u32>) -> UiResult<()> {
        Ok(())
    }

    async fn extensions(&self) -> UiResult<Vec<ExtensionVm>> {
        Ok(Vec::new())
    }
    async fn install_extension(&self, _path: &str) -> UiResult<ExtensionVm> {
        not_wired()
    }
    async fn set_extension_enabled(&self, _id: &str, _enabled: bool) -> UiResult<()> {
        not_wired()
    }
    async fn remove_extension(&self, _id: &str) -> UiResult<()> {
        not_wired()
    }

    async fn load_settings(&self) -> UiResult<Settings> {
        Ok(settings_store::load(&self.settings_file))
    }
    async fn save_settings(&self, settings: &Settings) -> UiResult<()> {
        settings_store::save(&self.settings_file, settings).map_err(|e| UiError::new(ErrorKind::Other, e.to_string()))
    }
    async fn account(&self) -> UiResult<AccountVm> {
        Ok(AccountVm { email: None, configured: false })
    }
    async fn sign_in(&self, _email: &str, _password: &str) -> UiResult<AccountVm> {
        not_wired()
    }
    async fn sign_out(&self) -> UiResult<()> {
        Ok(())
    }

    async fn link_status(&self) -> UiResult<LinkStatusVm> {
        Ok(LinkStatusVm { running: false, port: crate::firewall::DEFAULT_PORT, devices: Vec::new() })
    }
    async fn link_open_pairing(&self) -> UiResult<Pairing> {
        not_wired()
    }
    async fn link_revoke(&self, _device_id: &str) -> UiResult<()> {
        not_wired()
    }
    async fn link_allow_private_network(&self) -> UiResult<()> {
        crate::backend_common::allow_private_network().await
    }
}
