//! Extension host glue over tk-ext.
//!
//! Real API (tk-ext): `ExtensionHost::open(root)`, sync `install_kai` / `list` / `get` / `set_enabled` / `remove`,
//! async `resolve_streams` / `resolve_candidates`, `logs(id)` (console lines of the bundle, oldest first).
//!
//! The per-extension log the UI shows merges two sources: operations the app performs (install, enable, stream
//! resolution, with a timestamp) kept here in memory, and the bundle's own console output from tk-ext.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tk_core::sources::SourceCandidate;
use tk_core::{Compat, InstalledExtension, Media, StreamSource};
use tk_ext::ExtensionHost;
use tk_ext::hashing::Algo;
use tk_ui::backend::{ExtLogLevel, ExtLogVm, ExtensionDetailVm, ExtensionVm};
use tk_ui::model::{CompatLevel, ErrorKind, UiError};

/// App-side log lines kept per extension (the oldest are dropped).
const APP_LOG_LINES: usize = 100;

pub struct Extensions {
    host: Arc<ExtensionHost>,
    log: Mutex<HashMap<String, VecDeque<ExtLogVm>>>,
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

fn not_installed(id: &str) -> UiError {
    UiError::new(ErrorKind::Extension, format!("{id} is not installed"))
}

fn now_unix() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

/// `[warn] message` (tk-ext `LogBuf` format) -> a log row; the console has no operation name or time.
fn console_line(line: &str) -> ExtLogVm {
    let (level, message) = match line.strip_prefix('[').and_then(|r| r.split_once("] ")) {
        Some((lvl, msg)) => (lvl, msg),
        None => ("log", line),
    };
    let level = match level {
        "error" => ExtLogLevel::Err,
        "warn" => ExtLogLevel::Warn,
        _ => ExtLogLevel::Ok,
    };
    ExtLogVm { level, op: "console".to_owned(), message: message.to_owned(), at_unix: None }
}

/// `9f2c…b71e`: first and last 2 bytes of the digest in hex.
fn display_hash(digest: &[u8]) -> String {
    let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
    if digest.len() < 4 {
        return hex(digest);
    }
    format!("{}…{}", hex(&digest[..2]), hex(&digest[digest.len() - 2..]))
}

fn modified_unix(path: &Path) -> Option<i64> {
    let t = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
    t.duration_since(std::time::UNIX_EPOCH).ok().and_then(|d| i64::try_from(d.as_secs()).ok())
}

/// Hash, date and domains of an installed extension, read from `<root>/<id>/{bundle.js, manifest.json}`.
fn detail(root: &Path, info: &InstalledExtension) -> ExtensionDetailVm {
    let dir = root.join(&info.id); // ids are validated by tk-ext (no separators)
    let hash = std::fs::read(dir.join("bundle.js")).map(|b| display_hash(&Algo::Sha256.digest(&b))).unwrap_or_default();
    ExtensionDetailVm {
        id: info.id.clone(),
        domains: info
            .permissions
            .iter()
            .filter_map(|p| p.strip_prefix("network:domain:"))
            .map(|d| d.trim().to_owned())
            .filter(|d| !d.is_empty())
            .collect(),
        hash,
        updated_unix: modified_unix(&dir.join("manifest.json")),
        kind: info.kind.clone(),
    }
}

impl Extensions {
    pub fn new(dir: PathBuf) -> anyhow::Result<Self> {
        Ok(Self {
            host: Arc::new(ExtensionHost::open(dir).map_err(|e| anyhow::anyhow!("{e}"))?),
            log: Mutex::new(HashMap::new()),
        })
    }

    /// Shared host, also used by the Link server to look up extension permissions.
    pub fn host(&self) -> Arc<ExtensionHost> {
        self.host.clone()
    }

    fn note(&self, id: &str, level: ExtLogLevel, op: &str, message: impl Into<String>) {
        let Ok(mut log) = self.log.lock() else { return };
        let q = log.entry(id.to_owned()).or_default();
        if q.len() >= APP_LOG_LINES {
            q.pop_front();
        }
        q.push_back(ExtLogVm { level, op: op.to_owned(), message: message.into(), at_unix: Some(now_unix()) });
    }

    /// Installed extensions (sync and cheap: the host keeps them in memory).
    pub fn installed(&self) -> Vec<InstalledExtension> {
        self.host.list()
    }

    pub async fn list(&self) -> Result<Vec<ExtensionVm>, UiError> {
        let host = self.host.clone();
        tokio::task::spawn_blocking(move || host.list().into_iter().map(to_vm).collect())
            .await
            .map_err(err)
    }

    pub async fn install(&self, path: &str) -> Result<ExtensionVm, UiError> {
        let (host, path) = (self.host.clone(), Path::new(path).to_path_buf());
        let info = tokio::task::spawn_blocking(move || host.install_kai(&path).map_err(err)).await.map_err(err)??;
        self.note(&info.id, ExtLogLevel::Ok, "install", format!("{} {}", info.name, info.version));
        match info.compat {
            Compat::Full => {}
            Compat::Partial => self.note(&info.id, ExtLogLevel::Warn, "compat", info.compat_reason.clone().unwrap_or_default()),
            Compat::Unsupported => self.note(&info.id, ExtLogLevel::Err, "compat", info.compat_reason.clone().unwrap_or_default()),
        }
        Ok(to_vm(info))
    }

    pub async fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), UiError> {
        let (host, owned) = (self.host.clone(), id.to_owned());
        let r = tokio::task::spawn_blocking(move || host.set_enabled(&owned, enabled).map_err(err)).await.map_err(err)?;
        match &r {
            Ok(()) => self.note(id, ExtLogLevel::Ok, "session", if enabled { "enabled" } else { "disabled" }),
            Err(e) => self.note(id, ExtLogLevel::Err, "session", e.detail.clone()),
        }
        r
    }

    pub async fn remove(&self, id: &str) -> Result<(), UiError> {
        let (host, owned) = (self.host.clone(), id.to_owned());
        tokio::task::spawn_blocking(move || host.remove(&owned).map_err(err)).await.map_err(err)??;
        if let Ok(mut log) = self.log.lock() {
            log.remove(id);
        }
        Ok(())
    }

    /// Streams from every enabled extension, best first (the host orders them). Logs one `stream` line per
    /// enabled extension: how many streams it returned and how long the resolution took.
    pub async fn resolve(&self, media: &Media, episode: u32) -> Vec<StreamSource> {
        let started = Instant::now();
        let streams = self.host.resolve_streams(media, episode).await;
        let ms = started.elapsed().as_millis();
        for e in self.host.list().into_iter().filter(|e| e.enabled && e.compat != Compat::Unsupported && e.kind != "torrent") {
            let n = streams.iter().filter(|s| s.extension_id == e.id).count();
            if n == 0 {
                self.note(&e.id, ExtLogLevel::Warn, "stream", format!("\"{}\" ep {episode} -> 0 streams in {ms} ms", media.title));
            } else {
                self.note(&e.id, ExtLogLevel::Ok, "stream", format!("\"{}\" ep {episode} -> {n} streams in {ms} ms", media.title));
            }
        }
        streams
    }

    /// Every source candidate of the episode (web `stream` route plus torrent `torrent` route, with server, subs,
    /// dubs and note), unranked. Logs one `stream` line per enabled extension like [`Self::resolve`].
    pub async fn resolve_candidates(&self, media: &Media, episode: u32) -> Vec<SourceCandidate> {
        let started = Instant::now();
        let list = self.host.resolve_candidates(media, episode).await;
        let ms = started.elapsed().as_millis();
        for e in self.host.list().into_iter().filter(|e| e.enabled && e.compat != Compat::Unsupported) {
            let n = list.iter().filter(|c| c.ext_id == e.id).count();
            let level = if n == 0 { ExtLogLevel::Warn } else { ExtLogLevel::Ok };
            self.note(&e.id, level, "stream", format!("\"{}\" ep {episode} -> {n} sources in {ms} ms", media.title));
        }
        list
    }

    /// Newest first: app operations (timestamped), then the bundle's console output.
    pub async fn logs(&self, id: &str) -> Result<Vec<ExtLogVm>, UiError> {
        if self.host.get(id).is_none() {
            return Err(not_installed(id));
        }
        let mut out: Vec<ExtLogVm> =
            self.log.lock().ok().and_then(|l| l.get(id).map(|q| q.iter().rev().cloned().collect())).unwrap_or_default();
        out.extend(self.host.logs(id).iter().rev().map(|l| console_line(l)));
        Ok(out)
    }

    pub async fn detail(&self, id: &str) -> Result<ExtensionDetailVm, UiError> {
        let info = self.host.get(id).ok_or_else(|| not_installed(id))?;
        let root = self.host.root().to_path_buf();
        tokio::task::spawn_blocking(move || detail(&root, &info)).await.map_err(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn console_lines_are_parsed() {
        let l = console_line("[warn] slow mirror");
        assert_eq!((l.level, l.message.as_str(), l.op.as_str()), (ExtLogLevel::Warn, "slow mirror", "console"));
        assert_eq!(console_line("[error] boom").level, ExtLogLevel::Err);
        assert_eq!(console_line("no prefix").message, "no prefix");
    }

    #[test]
    fn hash_display_form() {
        assert_eq!(display_hash(&[0x9f, 0x2c, 0x00, 0x11, 0xb7, 0x1e]), "9f2c…b71e");
        assert_eq!(display_hash(&[0xab]), "ab");
    }
}
