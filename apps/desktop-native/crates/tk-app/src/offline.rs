//! Backend used when the `live` feature is off: settings persistence, firewall flow and window chrome
//! work; every catalogue / extension / Link call returns an explicit "not wired" error
//! (`ErrorKind::NotWired`) that the UI shows inline. Calls that have a meaningful empty answer (library,
//! logs, capabilities) return it. This is NOT the shipping backend - see the `live` feature in Cargo.toml.

use std::path::PathBuf;

use async_trait::async_trait;
use tk_ui::backend::{
    AccountVm, Backend, DetailsVm, EpisodeVm, ExtLogVm, ExtensionVm, HomeVm, LibraryVm, LinkCapsVm, LinkStatusVm,
    MediaVm, PlaybackVm, UiResult,
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

/// tk-core / tk-ext / tk-link are not linked in this build (enable the `live` feature).
fn not_wired<T>(what: &str) -> UiResult<T> {
    Err(UiError::not_wired(what))
}

fn stopped_link(port: u16) -> LinkStatusVm {
    LinkStatusVm { running: false, port, devices: Vec::new(), error: Some(UiError::not_wired("link")) }
}

#[async_trait]
impl Backend for OfflineBackend {
    async fn home(&self) -> UiResult<HomeVm> {
        not_wired("home")
    }
    async fn search(&self, _query: &str, _page: u32) -> UiResult<Vec<MediaVm>> {
        not_wired("search")
    }
    async fn details(&self, _id: &str) -> UiResult<DetailsVm> {
        not_wired("details")
    }
    async fn episodes(&self, _id: &str) -> UiResult<Vec<EpisodeVm>> {
        not_wired("episodes")
    }
    async fn playback(&self, _id: &str, _episode: u32) -> UiResult<PlaybackVm> {
        not_wired("playback")
    }
    async fn record_progress(&self, _id: &str, _episode: u32, _progress_secs: u32, _duration_secs: Option<u32>) -> UiResult<()> {
        Ok(())
    }

    async fn extensions(&self) -> UiResult<Vec<ExtensionVm>> {
        Ok(Vec::new())
    }
    async fn install_extension(&self, _path: &str) -> UiResult<ExtensionVm> {
        not_wired("install_extension")
    }
    async fn set_extension_enabled(&self, _id: &str, _enabled: bool) -> UiResult<()> {
        not_wired("set_extension_enabled")
    }
    async fn remove_extension(&self, _id: &str) -> UiResult<()> {
        not_wired("remove_extension")
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
        not_wired("sign_in")
    }
    async fn sign_out(&self) -> UiResult<()> {
        Ok(())
    }

    async fn link_status(&self) -> UiResult<LinkStatusVm> {
        Ok(LinkStatusVm { running: false, port: crate::firewall::DEFAULT_PORT, devices: Vec::new(), error: None })
    }
    async fn link_open_pairing(&self) -> UiResult<Pairing> {
        not_wired("link_open_pairing")
    }
    async fn link_revoke(&self, _device_id: &str) -> UiResult<()> {
        not_wired("link_revoke")
    }
    async fn link_allow_private_network(&self) -> UiResult<()> {
        crate::backend_common::allow_private_network(crate::firewall::DEFAULT_PORT).await
    }

    // --- wave 1 (docs/redesign/08) ---
    // `sources` / `playback_source` / `extension_detail` / `link_rename_device` keep the trait default
    // (`NotWired`): the player then falls back to `playback`, which is not wired either.

    async fn report_source_result(&self, _source_id: &str, _ok: bool) -> UiResult<()> {
        Ok(()) // nothing to learn from without sources
    }
    async fn set_picked_source(&self, _id: &str, _episode: u32, _source_id: Option<&str>) -> UiResult<()> {
        Ok(())
    }
    async fn library(&self) -> UiResult<LibraryVm> {
        Ok(LibraryVm::default()) // no local store in this build: an empty library
    }
    async fn sync_now(&self) -> UiResult<()> {
        Err(UiError::new(ErrorKind::Auth, "no account service in this build"))
    }
    async fn extension_logs(&self, _id: &str) -> UiResult<Vec<ExtLogVm>> {
        Ok(Vec::new())
    }
    async fn link_restart(&self, port: u16) -> UiResult<LinkStatusVm> {
        Ok(stopped_link(port))
    }
    async fn link_capabilities(&self) -> UiResult<LinkCapsVm> {
        Ok(LinkCapsVm::default())
    }
}
