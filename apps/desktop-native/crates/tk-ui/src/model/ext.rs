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

/// Intervals offered for [`ExtPrefs::check_interval_hours`] (hours).
pub const CHECK_INTERVALS: [u8; 5] = [6, 12, 24, 72, 168];

/// The kill switch is "on" when there are extensions and none is enabled (`ext.kill-switch`, `data-kill-switch`).
pub fn kill_switch_on(enabled: &[bool]) -> bool {
    !enabled.is_empty() && enabled.iter().all(|e| !e)
}

/// Stable refusal codes of an extension server (`/api/server/v1` error catalogue) that have a translated text
/// (`extserver.error.<code>`).
pub const SERVER_ERROR_CODES: [&str; 9] = [
    "authentication_required",
    "pairing_required",
    "pairing_expired",
    "device_not_authorized",
    "forbidden",
    "pairing_limit",
    "setup_required",
    "invalid_code",
    "rate_limited",
];

/// `extserver.error.<code>` for a refusal detail (`pairing_expired`, `authentication_required: ...`), if known.
pub fn server_error_key(detail: &str) -> Option<String> {
    let code = detail.split([':', ' ']).next().unwrap_or("").trim();
    SERVER_ERROR_CODES.contains(&code).then(|| format!("extserver.error.{code}"))
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
                match hex {
                    Some(b) => {
                        out.push(b);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The `server=` origin of a `tatakai://extensionserver/connect?...` link (decoded), for the confirmation text.
pub fn link_server(link: &str) -> Option<String> {
    let query = link.split_once('?')?.1;
    query
        .split('&')
        .find_map(|kv| kv.strip_prefix("server="))
        .map(percent_decode)
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

/// `ext:` id parts of a provider card: `(ext_id, source_id, item_key)` where `item_key` = `<source_id>:<item_id>`
/// (what `Backend::provider_item` / `set_provider_bookmark` take).
pub fn provider_id_parts(id: &str) -> Option<(String, String, String)> {
    let rest = id.strip_prefix("ext:")?;
    let (ext, key) = rest.split_once(':')?;
    let (source, item) = key.split_once(':')?;
    (!ext.is_empty() && !source.is_empty() && !item.is_empty()).then(|| (ext.to_owned(), source.to_owned(), key.to_owned()))
}

/// Row title of a provider page section (`title_key` = `ext:<extension name>:<row>`).
pub fn provider_row_title(title_key: &str, ext_name: &str) -> String {
    let prefix = format!("ext:{ext_name}:");
    title_key.strip_prefix(&prefix).or_else(|| title_key.strip_prefix("ext:")).unwrap_or(title_key).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kill_switch_and_server_errors() {
        assert!(kill_switch_on(&[false, false]));
        assert!(!kill_switch_on(&[false, true]));
        assert!(!kill_switch_on(&[]));
        assert_eq!(server_error_key("pairing_expired").as_deref(), Some("extserver.error.pairing_expired"));
        assert_eq!(server_error_key("authentication_required: no token").as_deref(), Some("extserver.error.authentication_required"));
        assert_eq!(server_error_key("http 502"), None);
    }

    #[test]
    fn deep_link_server_is_decoded() {
        let l = "tatakai://extensionserver/connect?server=http%3A%2F%2F192.168.1.10%3A8099&code=123456";
        assert_eq!(link_server(l).as_deref(), Some("http://192.168.1.10:8099"));
        assert_eq!(link_server("tatakai://extensionserver/connect?code=1"), None);
        assert_eq!(percent_decode("a%2"), "a%2");
        assert_eq!(percent_decode("%zz"), "%zz");
    }

    #[test]
    fn provider_ids_and_row_titles() {
        assert_eq!(provider_id_parts("ext:toko:src:a:b"), Some(("toko".into(), "src".into(), "src:a:b".into())));
        assert_eq!(provider_id_parts("21"), None);
        assert_eq!(provider_id_parts("ext:toko:src"), None);
        assert_eq!(provider_row_title("ext:Toko:Latest", "Toko"), "Latest");
        assert_eq!(provider_row_title("ext:Other", "Toko"), "Other");
    }

    #[test]
    fn defaults_are_sanitized() {
        let d = ExtPrefs::default();
        assert_eq!(d.clone().sanitized(), d);
        assert_eq!(ExtPrefs { check_interval_hours: 0, ..d }.sanitized().check_interval_hours, CHECK_INTERVAL_MIN_HOURS);
    }
    // ---- F6 (parity wave B) ----

    #[test]
    fn check_interval_is_clamped_both_ways_and_kept_in_range() {
        let d = ExtPrefs::default();
        assert_eq!(ExtPrefs { check_interval_hours: 255, ..d.clone() }.sanitized().check_interval_hours, CHECK_INTERVAL_MAX_HOURS);
        assert_eq!(ExtPrefs { check_interval_hours: 6, ..d.clone() }.sanitized().check_interval_hours, 6);
        // the booleans are never touched
        let off = ExtPrefs { auto_check_updates: false, notify_updates: false, server_fallback_local: false, stremio_enabled: false, ..d };
        assert_eq!(off.clone().sanitized(), off);
    }

    #[test]
    fn missing_fields_take_the_defaults() {
        let p: ExtPrefs = serde_json::from_str(r#"{"stremio_enabled":false}"#).unwrap();
        assert_eq!(p, ExtPrefs { stremio_enabled: false, ..ExtPrefs::default() });
        let p: ExtPrefs = serde_json::from_str("{}").unwrap();
        assert_eq!(p, ExtPrefs::default());
    }
}
