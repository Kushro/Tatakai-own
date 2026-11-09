//! Debrid services as torrent sources (wave A, worker **i**; docs/redesign/15 section 4): Real-Debrid
//! (`api.real-debrid.com/rest/1.0`), TorBox (`api.torbox.app/v1/api`), AllDebrid (`api.alldebrid.com/v4`) and
//! Premiumize (`www.premiumize.me/api`). Flow: `check_cached(info_hashes)` marks picker rows; `resolve(magnet,
//! episode file)` adds the magnet to the account, selects the file and returns a direct HTTPS link the player
//! loads as a web stream. Keys live in `crate::secrets` (`keys::debrid(service.id())`). Contract: [`Service`],
//! [`Account`], [`Resolved`] and the [`DebridClient`] entry points are final. One private file per service.
//!
//! Cache checks per service (ported from the classic `debrid-orchestrator.ts` / `cloud-debrid-client.ts`):
//! * TorBox `GET /torrents/checkcached` and Premiumize `POST /cache/check` answer for a batch of hashes without
//!   side effects.
//! * Real-Debrid and AllDebrid removed their "instant availability" endpoints. A hash counts as cached when the
//!   account already holds it ready (`GET /torrents` / `GET /magnet/status`), or when adding it is ready at once:
//!   AllDebrid's `magnet/upload` says `ready`; Real-Debrid is probed with `addMagnet` + `selectFiles(all)` +
//!   `info` (status `downloaded`). Probes that turn out not cached are deleted from the account again. Callers
//!   probe only a few hashes per list (`live/debrid.rs`).

mod alldebrid;
mod premiumize;
mod realdebrid;
mod torbox;

use std::time::Duration;

use serde_json::Value;

use crate::error::DebridError;
use crate::release::{CandidateFile, is_video_path, pick_episode_file};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Service {
    RealDebrid,
    TorBox,
    AllDebrid,
    Premiumize,
}

impl Service {
    pub const ALL: [Service; 4] = [Self::RealDebrid, Self::TorBox, Self::AllDebrid, Self::Premiumize];

    /// Matches the UI `DebridServiceVm::id()` and the `service` column of `debrid_cache`.
    pub fn id(self) -> &'static str {
        match self {
            Self::RealDebrid => "real_debrid",
            Self::TorBox => "torbox",
            Self::AllDebrid => "alldebrid",
            Self::Premiumize => "premiumize",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.id() == id)
    }

    /// Brand name for messages.
    pub fn label(self) -> &'static str {
        match self {
            Self::RealDebrid => "Real-Debrid",
            Self::TorBox => "TorBox",
            Self::AllDebrid => "AllDebrid",
            Self::Premiumize => "Premiumize",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub service: Service,
    pub username: Option<String>,
    /// Unix seconds, when the service says.
    pub premium_until: Option<i64>,
}

/// A direct link for one file of a cached torrent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub url: String,
    pub file_name: String,
    pub size_bytes: Option<u64>,
}

/// How often a just-added cached torrent is polled until the service lists its files / links.
pub(crate) const POLL_INTERVAL: Duration = Duration::from_millis(800);
/// Polls before a "cached" torrent that is still not ready counts as not cached.
pub(crate) const POLL_ATTEMPTS: u32 = 12;

pub struct DebridClient {
    client: reqwest::Client,
}

impl DebridClient {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }

    /// Validate `key` and return the account.
    pub async fn account(&self, service: Service, key: &str) -> Result<Account, DebridError> {
        let key = clean_key(service, key)?;
        match service {
            Service::RealDebrid => realdebrid::account(&self.client, key).await,
            Service::TorBox => torbox::account(&self.client, key).await,
            Service::AllDebrid => alldebrid::account(&self.client, key).await,
            Service::Premiumize => premiumize::account(&self.client, key).await,
        }
    }

    /// Which of `info_hashes` (lowercase hex) the service has cached. Every input hash gets one answer, in order.
    pub async fn check_cached(&self, service: Service, key: &str, info_hashes: &[String]) -> Result<Vec<(String, bool)>, DebridError> {
        let key = clean_key(service, key)?;
        let hashes: Vec<String> = info_hashes.iter().map(|h| h.trim().to_ascii_lowercase()).filter(|h| is_hex_hash(h)).collect();
        if hashes.is_empty() {
            return Ok(Vec::new());
        }
        let cached = match service {
            Service::RealDebrid => realdebrid::check_cached(&self.client, key, &hashes).await?,
            Service::TorBox => torbox::check_cached(&self.client, key, &hashes).await?,
            Service::AllDebrid => alldebrid::check_cached(&self.client, key, &hashes).await?,
            Service::Premiumize => premiumize::check_cached(&self.client, key, &hashes).await?,
        };
        Ok(hashes.into_iter().map(|h| (h.clone(), cached.iter().any(|c| c == &h))).collect())
    }

    /// Add `magnet`, pick the file for `episode` (`tk_core::release::pick_episode_file` over the file list) and
    /// return its direct link.
    pub async fn resolve(&self, service: Service, key: &str, magnet: &str, episode: u32, titles: &[String]) -> Result<Resolved, DebridError> {
        let key = clean_key(service, key)?;
        let magnet = magnet.trim();
        if !magnet.to_ascii_lowercase().starts_with("magnet:?") {
            return Err(DebridError::Parse("a magnet link is required".into()));
        }
        let resolved = match service {
            Service::RealDebrid => realdebrid::resolve(&self.client, key, magnet, episode, titles).await?,
            Service::TorBox => torbox::resolve(&self.client, key, magnet, episode, titles).await?,
            Service::AllDebrid => alldebrid::resolve(&self.client, key, magnet, episode, titles).await?,
            Service::Premiumize => premiumize::resolve(&self.client, key, magnet, episode, titles).await?,
        };
        if !is_https(&resolved.url) {
            return Err(DebridError::Parse(format!("{} did not return an https link", service.label())));
        }
        Ok(resolved)
    }
}

fn clean_key(service: Service, key: &str) -> Result<&str, DebridError> {
    let key = key.trim();
    if key.is_empty() { Err(DebridError::NoKey(service.id().to_owned())) } else { Ok(key) }
}

/// 40 lowercase-or-uppercase hex characters.
pub fn is_hex_hash(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

fn is_https(url: &str) -> bool {
    url.get(..8).is_some_and(|p| p.eq_ignore_ascii_case("https://"))
}

/// The v1 info-hash of a magnet as lowercase hex (`btih:` in hex or base32), `None` without one.
pub fn magnet_info_hash(magnet: &str) -> Option<String> {
    let lower = magnet.to_ascii_lowercase();
    let start = lower.find("xt=urn:btih:")? + "xt=urn:btih:".len();
    let raw: String = magnet[start..].chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
    if raw.len() == 40 && is_hex_hash(&raw) {
        return Some(raw.to_ascii_lowercase());
    }
    if raw.len() == 32 {
        return base32_to_hex(&raw);
    }
    None
}

fn base32_to_hex(s: &str) -> Option<String> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut buffer: u32 = 0;
    let mut bits = 0;
    let mut out = Vec::with_capacity(20);
    for c in s.bytes() {
        let v = ALPHABET.iter().position(|a| *a == c.to_ascii_uppercase())? as u32;
        buffer = (buffer << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((buffer >> bits) & 0xff) as u8);
        }
    }
    (out.len() == 20).then(|| hex::encode(out))
}

/// One file of a torrent as a service lists it. `id` is whatever the service needs to select / unlock it (a file
/// id, a hoster link, a direct link).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RemoteFile {
    pub id: String,
    /// `/`-separated path inside the torrent.
    pub path: String,
    pub size: u64,
}

/// The file of `files` for `episode`: `pick_episode_file` first; when it finds nothing (a service that strips
/// folders, odd names) the largest video file of a single-video torrent.
pub(crate) fn pick_file<'a>(files: &'a [RemoteFile], episode: u32, titles: &[String]) -> Option<&'a RemoteFile> {
    let candidates: Vec<CandidateFile> = files
        .iter()
        .enumerate()
        .map(|(index, f)| CandidateFile { index, path: f.path.clone(), size_bytes: f.size })
        .collect();
    if let Some(i) = pick_episode_file(&candidates, episode, titles) {
        return files.get(i);
    }
    let videos: Vec<&RemoteFile> = files.iter().filter(|f| is_video_path(&f.path)).collect();
    match videos.as_slice() {
        [only] => Some(*only),
        _ => None,
    }
}

/// Last component of a `/`-separated path.
pub(crate) fn base_name(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_owned()
}

/// `value` as text: JSON strings as is, numbers printed.
pub(crate) fn text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_owned()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// `value` as a non-negative integer (numbers or numeric strings).
pub(crate) fn number(value: &Value) -> Option<u64> {
    match value {
        Value::Number(n) => n.as_u64().or_else(|| n.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64)),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// RFC 3339 date -> Unix seconds.
pub(crate) fn rfc3339_unix(value: &Value) -> Option<i64> {
    let s = value.as_str()?;
    time::OffsetDateTime::parse(s.trim(), &time::format_description::well_known::Rfc3339).ok().map(|t| t.unix_timestamp())
}

/// Turn an HTTP response into JSON, mapping 401 / 403 to `Unauthorized` and other failures to `Http`.
pub(crate) async fn json_of(resp: reqwest::Response) -> Result<Value, DebridError> {
    let status = resp.status();
    let body = resp.text().await?;
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(DebridError::Unauthorized);
    }
    let value: Value = if body.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&body).map_err(|e| {
            if status.is_success() { DebridError::Parse(e.to_string()) } else { DebridError::Http(status.as_u16()) }
        })?
    };
    if !status.is_success() {
        return Err(DebridError::Http(status.as_u16()));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magnet_hashes_hex_and_base32() {
        let hex = "0123456789abcdef0123456789abcdef01234567";
        assert_eq!(magnet_info_hash(&format!("magnet:?xt=urn:btih:{}&dn=x", hex.to_uppercase())).as_deref(), Some(hex));
        // base32 of the same 20 bytes
        assert_eq!(magnet_info_hash("magnet:?xt=urn:btih:AERUKZ4JVPG66AJDIVTYTK6N54ASGRLH").as_deref(), Some(hex));
        assert_eq!(magnet_info_hash("magnet:?dn=nothing"), None);
        assert_eq!(magnet_info_hash("magnet:?xt=urn:btih:short"), None);
    }

    #[test]
    fn picks_the_episode_file_then_a_lone_video() {
        let f = |id: &str, path: &str, size: u64| RemoteFile { id: id.into(), path: path.into(), size };
        let files = vec![
            f("1", "Show/[Group] Show - 01 [1080p].mkv", 900),
            f("2", "Show/[Group] Show - 02 [1080p].mkv", 900),
            f("3", "Show/readme.txt", 1),
        ];
        assert_eq!(pick_file(&files, 2, &["Show".into()]).map(|f| f.id.as_str()), Some("2"));
        let lone = vec![f("9", "weird name.mkv", 5), f("8", "cover.jpg", 1)];
        assert_eq!(pick_file(&lone, 7, &[]).map(|f| f.id.as_str()), Some("9"));
        assert!(pick_file(&[f("x", "notes.txt", 1)], 1, &[]).is_none());
    }

    #[test]
    fn json_helpers() {
        assert_eq!(text(&serde_json::json!(12)), Some("12".into()));
        assert_eq!(text(&serde_json::json!("  a ")), Some("a".into()));
        assert_eq!(text(&serde_json::json!("")), None);
        assert_eq!(number(&serde_json::json!("42")), Some(42));
        assert_eq!(number(&serde_json::json!(4.0)), Some(4));
        assert_eq!(rfc3339_unix(&serde_json::json!("1970-01-02T00:00:00.000Z")), Some(86_400));
        assert_eq!(base_name("a/b/c.mkv"), "c.mkv");
        assert!(is_https("HTTPS://x") && !is_https("http://x"));
        assert_eq!(Service::from_id("torbox"), Some(Service::TorBox));
        assert!(clean_key(Service::TorBox, "  ").is_err());
    }
}
