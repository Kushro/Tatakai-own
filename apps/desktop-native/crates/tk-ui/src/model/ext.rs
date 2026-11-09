//! Extensions preferences and view-models (wave B, worker **j**; docs/redesign/16-parity-wave-b-contract.md
//! section 3.3): extension-server client, Stremio add-ons, updates, per-extension preferences, repositories,
//! custom providers. `ExtPrefs` is embedded in `Settings::ext` (`SettingsAction::SetExt`). Worker j owns this
//! file; `model/mod.rs` stays frozen. `ExtensionVm` / `ExtensionDetailVm` / `ExtLogVm` stay in `backend.rs`.

use serde::{Deserialize, Serialize};

use crate::backend::{DetailsVm, EpisodeVm};

/// Bounds of [`ExtPrefs::check_interval_hours`].
pub const CHECK_INTERVAL_MIN_HOURS: u8 = 1;
pub const CHECK_INTERVAL_MAX_HOURS: u8 = 168;

/// `Settings::ext`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExtPrefs {
    /// Look for extension updates at start-up and every `check_interval_hours` (`ext.updates.check`).
    pub auto_check_updates: bool,
    pub check_interval_hours: u8,
    /// Show the update sheet at launch when updates exist (`ext.updates.sheet`).
    pub notify_updates: bool,
    /// When every enabled extension server is offline, resolve with the local extensions only
    /// (`ext.server-client.fallback`).
    pub server_fallback_local: bool,
    /// Stremio add-ons take part in source resolution (`ext.stremio`).
    pub stremio_enabled: bool,
}

impl Default for ExtPrefs {
    fn default() -> Self {
        Self {
            auto_check_updates: true,
            check_interval_hours: 24,
            notify_updates: true,
            server_fallback_local: true,
            stremio_enabled: true,
        }
    }
}

impl ExtPrefs {
    pub fn sanitized(mut self) -> Self {
        self.check_interval_hours = self.check_interval_hours.clamp(CHECK_INTERVAL_MIN_HOURS, CHECK_INTERVAL_MAX_HOURS);
        self
    }
}

/// A paired extension server (classic `ExtensionServersSettings`, `core/extensions/server/*`). The token lives in
/// `tk_core::Secrets` (`keys::ext_server(id)`), never here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtServerVm {
    pub id: String,
    pub name: String,
    /// Origin (`http://192.168.1.10:8099`).
    pub url: String,
    pub enabled: bool,
    /// `None` = not tested yet this session.
    pub online: Option<bool>,
    /// Sources the server offers (enabled ones).
    pub sources: u32,
    /// `viewer` / `admin`.
    pub role: String,
    pub last_error: Option<String>,
    pub last_ok_unix: Option<i64>,
}

/// `Backend::ext_server_pair` input: a pairing code (QR / deep link / typed), a viewer password, or neither when the
/// server allows anonymous viewers.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExtServerPairVm {
    pub url: String,
    pub code: Option<String>,
    pub password: Option<String>,
    /// Display name; `None` = the server's own name.
    pub name: Option<String>,
}

/// One source a server offers (`GET /api/server/v1/sources`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtSourceVm {
    pub id: String,
    pub name: String,
    /// `tatakai`, `aniyomi`, `mihon`, `cloudstream`, `sora`, `mangayomi`, `seanime`, `stremio`.
    pub ecosystem: String,
    /// `watch` / `read` / `novel`.
    pub kind: String,
    pub languages: Vec<String>,
    pub enabled: bool,
    pub available: bool,
    pub reason: Option<String>,
}

/// A Stremio add-on (`stremio_addons` table; classic `SideloadExtensionModal` "Stremio addon").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StremioAddonVm {
    pub id: String,
    pub name: String,
    pub manifest_url: String,
    pub version: String,
    pub description: Option<String>,
    /// `movie`, `series`, `anime` ...
    pub types: Vec<String>,
    /// `stream`, `catalog`, `meta`, `subtitles`.
    pub resources: Vec<String>,
    pub enabled: bool,
}

/// A newer version of an installed extension (`ext_updates` table; classic `ExtensionUpdateSheet`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtUpdateVm {
    pub id: String,
    pub name: String,
    pub current: String,
    pub latest: String,
    /// Package URL to install the update from.
    pub url: Option<String>,
    pub notes: Option<String>,
}

/// Control of one preference field (`ext.preferences`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExtPrefKindVm {
    Text,
    /// Stored in the preferences table too, but the UI masks it.
    Secret,
    Toggle,
    Select,
}

/// One preference an extension declares (manifest `preferences` / `settings` list) with its stored value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtPrefFieldVm {
    pub key: String,
    pub label: String,
    pub hint: Option<String>,
    pub kind: ExtPrefKindVm,
    /// `(value, label)` for `Select`.
    pub options: Vec<(String, String)>,
    /// Current value (`"true"` / `"false"` for toggles); empty = default.
    pub value: String,
}

/// An extension repository added by URL (`ext_repos` table; classic Aniyomi / Mihon / CloudStream repos).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtRepoVm {
    pub id: String,
    pub name: String,
    pub url: String,
    pub items: u32,
    pub fetched_unix: Option<i64>,
    pub error: Option<String>,
}

/// One entry of a repository catalogue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtCatalogItemVm {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    /// Package URL (`.kai`, or an ecosystem package the extension server installs).
    pub url: String,
    /// `anime` / `manga` / `novel` / `torrent`.
    pub kind: String,
    pub installed: bool,
    pub installed_version: Option<String>,
}

/// A custom provider page (manifest `customSources`; classic `ExtensionSlot` pages + "provider rows" on Home).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomProviderVm {
    pub ext_id: String,
    pub ext_name: String,
    /// Source name inside the extension.
    pub name: String,
    /// `anime` / `manga` / `novel`.
    pub kind: String,
    pub has_home: bool,
    pub has_search: bool,
}

/// A title of a custom provider (`custom/info` route): id prefix `ext:<ext_id>:<key>`.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderItemVm {
    pub details: DetailsVm,
    pub episodes: Vec<EpisodeVm>,
    pub bookmarked: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sanitized() {
        let d = ExtPrefs::default();
        assert_eq!(d.clone().sanitized(), d);
        assert_eq!(ExtPrefs { check_interval_hours: 0, ..d }.sanitized().check_interval_hours, CHECK_INTERVAL_MIN_HOURS);
    }
}
