//! `ExtensionHost`: installed-extension registry on disk + per-extension QuickJS workers.
//!
//! Layout under the injected `root`: `<root>/<id>/{manifest.json, bundle.js, state.json}`. Nothing outside `root`
//! is ever read or written (tests pass a tempdir; the app passes `%APPDATA%\Tatakai-Next\extensions`).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tk_core::net::{Policy, SafeHttp, host_matches_permissions};
use tk_core::sources::SourceCandidate;
use tk_core::{Compat, InstalledExtension, Media, StreamSource, SubtitleTrack};

use crate::browser::{BrowserRuntime, browser_domains};
use crate::candidates::{candidate_routes, map_candidates};
use crate::compat::{CompatReport, analyze_bundle, deferred_permissions, exposes_torrent_all};
use crate::error::{ExtError, InstallError};
use crate::manifest::{Manifest, valid_id};
use crate::package::{Package, read_kai, read_kai_bytes};
use crate::runtime::{BrowserCfg, CALL_TIMEOUT, CallKind, LogBuf, Worker, WorkerConfig};

#[derive(Serialize, Deserialize)]
struct StateFile {
    enabled: bool,
    compat: String,
    compat_reason: Option<String>,
}

struct Entry {
    manifest: Manifest,
    manifest_json: String,
    bundle: Arc<String>,
    report: CompatReport,
    /// `compat::exposes_torrent_all(bundle)`, computed once per load / install (docs/redesign/10 section 6).
    torrent_all: bool,
    enabled: bool,
    logs: LogBuf,
    worker: Option<Arc<Worker>>,
    /// User preferences (wave B, `ext.preferences`); the app keeps them in `ext_prefs` and pushes them with
    /// [`ExtensionHost::set_preferences`]. Survive a reinstall (update) of the same id.
    prefs: BTreeMap<String, String>,
}

impl Entry {
    fn info(&self) -> InstalledExtension {
        InstalledExtension {
            id: self.manifest.id.clone(),
            name: self.manifest.name.clone(),
            version: self.manifest.version.clone(),
            kind: self.manifest.kind.clone(),
            permissions: self.manifest.permissions.clone(),
            compat: self.report.level,
            compat_reason: self.report.reason.clone(),
            enabled: self.enabled,
        }
    }
}

pub struct ExtensionHost {
    root: PathBuf,
    entries: Mutex<HashMap<String, Entry>>,
    call_timeout: std::time::Duration,
    loopback_for_tests: bool,
    /// Hidden-web-view browser for bundles with permitted `browserDomains` (docs/redesign/09 section 5).
    browser: Option<Arc<dyn BrowserRuntime>>,
}

impl ExtensionHost {
    /// Open (creating) the extension store at `root` and load every installed extension.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, InstallError> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        let host = Self {
            root,
            entries: Mutex::new(HashMap::new()),
            call_timeout: CALL_TIMEOUT,
            loopback_for_tests: false,
            browser: None,
        };
        host.reload()?;
        Ok(host)
    }

    /// Override the per-call wall-clock limit (tests only exercise this with small values).
    pub fn with_call_timeout(mut self, t: std::time::Duration) -> Self {
        self.call_timeout = t;
        self
    }

    /// Give bundles a browser runtime (`configureBrowserRuntime`). Workers started afterwards get it; call right
    /// after `open`, before the first extension call.
    pub fn with_browser_runtime(mut self, runtime: Arc<dyn BrowserRuntime>) -> Self {
        self.browser = Some(runtime);
        self
    }

    /// The injected browser runtime, if any.
    pub fn browser_runtime(&self) -> Option<Arc<dyn BrowserRuntime>> {
        self.browser.clone()
    }

    /// TEST ONLY: let extension `fetch` reach loopback servers on any port. Never call this from application code.
    #[doc(hidden)]
    pub fn with_loopback_for_tests(mut self) -> Self {
        self.loopback_for_tests = true;
        self
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn reload(&self) -> Result<(), InstallError> {
        let mut map = HashMap::new();
        for dir in std::fs::read_dir(&self.root)?.flatten() {
            let p = dir.path();
            if !p.is_dir() {
                continue;
            }
            let (Ok(mj), Ok(bundle)) = (std::fs::read_to_string(p.join("manifest.json")), std::fs::read_to_string(p.join("bundle.js"))) else {
                continue;
            };
            let Ok(manifest) = Manifest::parse(&mj) else { continue };
            if p.file_name().and_then(|n| n.to_str()) != Some(manifest.id.as_str()) {
                continue; // directory name must equal the id
            }
            let report = analyze_bundle(&bundle);
            let enabled = std::fs::read_to_string(p.join("state.json"))
                .ok()
                .and_then(|s| serde_json::from_str::<StateFile>(&s).ok())
                .map_or(report.level != Compat::Unsupported, |s| s.enabled && report.level != Compat::Unsupported);
            let logs = deferred_log(&manifest);
            let torrent_all = exposes_torrent_all(&bundle);
            map.insert(
                manifest.id.clone(),
                Entry {
                    manifest,
                    manifest_json: mj,
                    bundle: Arc::new(bundle),
                    report,
                    torrent_all,
                    enabled,
                    logs,
                    worker: None,
                    prefs: BTreeMap::new(),
                },
            );
        }
        *self.entries.lock().map_err(|_| InstallError::invalid("extension registry poisoned"))? = map;
        Ok(())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Entry>> {
        self.entries.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn persist(&self, e: &Entry) -> Result<(), InstallError> {
        let dir = self.root.join(&e.manifest.id);
        let st = StateFile {
            enabled: e.enabled,
            compat: format!("{:?}", e.report.level),
            compat_reason: e.report.reason.clone(),
        };
        std::fs::write(dir.join("state.json"), serde_json::to_string_pretty(&st).unwrap_or_default())?;
        Ok(())
    }

    /// Install (or replace) a `.kai` package. A package whose compat level is `unsupported` installs successfully but
    /// stays disabled so the UI can explain why.
    pub fn install_kai(&self, path: &Path) -> Result<InstalledExtension, InstallError> {
        self.install_package(read_kai(path)?)
    }

    /// Install (or replace) a `.kai` held in memory (wave B, `ext.install-url` / `ext.updates.sheet`: the app
    /// downloads it first). Same rules as [`Self::install_kai`]; a replaced extension keeps its preferences.
    pub fn install_kai_bytes(&self, bytes: &[u8]) -> Result<InstalledExtension, InstallError> {
        self.install_package(read_kai_bytes(bytes)?)
    }

    /// Install from a hosted `manifest.json` + `bundle.js` pair (wave B, `ext.install-url` with a manifest URL).
    /// Same validation as a package (manifest rules, 10 MiB bundle).
    pub fn install_unpacked(&self, manifest_json: &str, bundle: &str) -> Result<InstalledExtension, InstallError> {
        if bundle.len() > 10 * 1024 * 1024 {
            return Err(InstallError::invalid("bundle.js: larger than 10 MiB"));
        }
        if manifest_json.len() > 256 * 1024 {
            return Err(InstallError::invalid("manifest.json: larger than 256 KiB"));
        }
        let manifest = Manifest::parse(manifest_json)?;
        self.install_package(Package { manifest, manifest_json: manifest_json.to_owned(), bundle: bundle.to_owned() })
    }

    fn install_package(&self, pkg: Package) -> Result<InstalledExtension, InstallError> {
        let Package { manifest, manifest_json, bundle } = pkg;
        let report = analyze_bundle(&bundle);
        let dir = self.root.join(&manifest.id); // id is validated by Manifest::parse (no separators)
        if !valid_id(&manifest.id) || dir.parent() != Some(self.root.as_path()) {
            return Err(InstallError::invalid("manifest.id: invalid"));
        }
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join("manifest.json"), &manifest_json)?;
        std::fs::write(dir.join("bundle.js"), &bundle)?;
        let entry = Entry {
            enabled: report.level != Compat::Unsupported,
            logs: deferred_log(&manifest),
            torrent_all: exposes_torrent_all(&bundle),
            manifest,
            manifest_json,
            bundle: Arc::new(bundle),
            report,
            worker: None,
            prefs: BTreeMap::new(),
        };
        let mut entry = entry;
        if let Some(prev) = self.lock().get(&entry.manifest.id) {
            entry.prefs = prev.prefs.clone();
        }
        self.persist(&entry)?;
        let info = entry.info();
        self.lock().insert(info.id.clone(), entry); // dropping a replaced entry stops its worker
        Ok(info)
    }

    pub fn list(&self) -> Vec<InstalledExtension> {
        let mut v: Vec<_> = self.lock().values().map(Entry::info).collect();
        v.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.id.cmp(&b.id)));
        v
    }

    pub fn get(&self, id: &str) -> Option<InstalledExtension> {
        self.lock().get(id).map(Entry::info)
    }

    /// The installed manifest (custom sources, declared preferences, update feed).
    pub fn manifest(&self, id: &str) -> Option<Manifest> {
        self.lock().get(id).map(|e| e.manifest.clone())
    }

    /// Replace the user preferences of an extension (wave B, `ext.preferences`). The running worker is dropped so
    /// the next call rebuilds the runtime with the new `globalThis.preferences`.
    pub fn set_preferences(&self, id: &str, prefs: BTreeMap<String, String>) -> Result<(), ExtError> {
        let mut map = self.lock();
        let e = map.get_mut(id).ok_or_else(|| ExtError::NotInstalled(id.to_owned()))?;
        if e.prefs != prefs {
            e.prefs = prefs;
            e.worker = None;
        }
        Ok(())
    }

    /// Current preferences of an extension (empty when none or not installed).
    pub fn preferences(&self, id: &str) -> BTreeMap<String, String> {
        self.lock().get(id).map(|e| e.prefs.clone()).unwrap_or_default()
    }

    pub fn compat_report(&self, id: &str) -> Option<CompatReport> {
        self.lock().get(id).map(|e| e.report.clone())
    }

    /// Enable/disable. An `unsupported` extension can never be enabled.
    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), ExtError> {
        let mut map = self.lock();
        let e = map.get_mut(id).ok_or_else(|| ExtError::NotInstalled(id.to_owned()))?;
        if enabled && e.report.level == Compat::Unsupported {
            return Err(ExtError::Unsupported);
        }
        e.enabled = enabled;
        if !enabled {
            e.worker = None;
        }
        self.persist(e).map_err(|err| ExtError::Runtime(err.to_string()))
    }

    /// Uninstall: removes `<root>/<id>` only.
    pub fn remove(&self, id: &str) -> Result<(), ExtError> {
        if !valid_id(id) {
            return Err(ExtError::NotInstalled(id.to_owned()));
        }
        let removed = self.lock().remove(id);
        if removed.is_none() {
            return Err(ExtError::NotInstalled(id.to_owned()));
        }
        let dir = self.root.join(id);
        if dir.parent() == Some(self.root.as_path()) && dir.is_dir() {
            std::fs::remove_dir_all(&dir).map_err(|e| ExtError::Runtime(e.to_string()))?;
        }
        Ok(())
    }

    /// Console output captured from the extension (newest last).
    pub fn logs(&self, id: &str) -> Vec<String> {
        self.lock().get(id).map(|e| e.logs.lines()).unwrap_or_default()
    }

    fn worker_for(&self, id: &str, route_for_gate: Option<&str>) -> Result<Arc<Worker>, ExtError> {
        let mut map = self.lock();
        let e = map.get_mut(id).ok_or_else(|| ExtError::NotInstalled(id.to_owned()))?;
        if e.report.level == Compat::Unsupported {
            return Err(ExtError::Unsupported);
        }
        if !e.enabled {
            return Err(ExtError::Disabled(id.to_owned()));
        }
        if let Some(route) = route_for_gate
            && matches!(route, "sources" | "stream" | "torrent")
            && !e.manifest.gated_routes().iter().any(|r| r == route)
        {
            return Err(ExtError::RouteNotAllowed(route.to_owned()));
        }
        if let Some(w) = &e.worker {
            return Ok(w.clone());
        }
        let perms = e.manifest.permissions.clone();
        let filter_perms = perms.clone();
        let mut policy = Policy::extension_fetch();
        if self.loopback_for_tests {
            policy.allow_loopback = true;
        }
        let http = SafeHttp::new(policy).with_host_filter(Arc::new(move |h: &str| host_matches_permissions(h, &filter_perms)));
        // Legacy host-server.cjs: a browser only for permitted `browserDomains` (configureBrowserRuntime is JS side).
        let domains = browser_domains(&e.manifest);
        let browser = match &self.browser {
            Some(runtime) if !domains.is_empty() => Some(BrowserCfg { runtime: runtime.clone(), domains }),
            _ => None,
        };
        let worker = Arc::new(Worker::spawn(WorkerConfig {
            id: e.manifest.id.clone(),
            manifest_json: e.manifest_json.clone(),
            bundle: e.bundle.clone(),
            permissions: perms,
            http,
            logs: e.logs.clone(),
            call_timeout: self.call_timeout,
            browser,
            preferences: serde_json::to_string(&e.prefs).unwrap_or_else(|_| "{}".to_owned()),
        })?);
        if !e.prefs.is_empty() {
            // Keys only: values may be secrets.
            let keys: Vec<&str> = e.prefs.keys().map(String::as_str).collect();
            e.logs.push("log", &format!("preferences: {} value(s) loaded ({})", keys.len(), keys.join(", ")));
        }
        e.worker = Some(worker.clone());
        Ok(worker)
    }

    /// Answer a logical host route (`stream`, `providers`, `custom/home`, ...) with the JSON body the legacy host returned.
    pub async fn call(&self, ext_id: &str, route: &str, params: Value) -> Result<Value, ExtError> {
        if !is_known_route(route) {
            return Err(ExtError::RouteNotFound(route.to_owned()));
        }
        let worker = self.worker_for(ext_id, Some(route))?;
        let payload = serde_json::to_string(&params).map_err(|e| ExtError::BadRequest(e.to_string()))?;
        let out = worker.call(CallKind::Route, route.to_owned(), payload).await?;
        serde_json::from_str(&out).map_err(|e| ExtError::Runtime(e.to_string()))
    }

    /// Call a method of the bundle instance directly (`args` is a JSON array). Used by tests and diagnostics.
    pub async fn invoke(&self, ext_id: &str, method: &str, args: Value) -> Result<Value, ExtError> {
        let worker = self.worker_for(ext_id, None)?;
        let out = worker.call(CallKind::Method, method.to_owned(), args.to_string()).await?;
        serde_json::from_str(&out).map_err(|e| ExtError::Runtime(e.to_string()))
    }

    /// Resolve playable (HLS/MP4/DASH) streams for `media` episode `episode` from every enabled, runnable
    /// `onlinestream`/`custom` extension. Failures of one extension never fail the others.
    pub async fn resolve_streams(&self, media: &Media, episode: u32) -> Vec<StreamSource> {
        let candidates: Vec<(String, f64)> = {
            let map = self.lock();
            let mut v: Vec<_> = map
                .values()
                .filter(|e| e.enabled && e.report.level != Compat::Unsupported && e.manifest.kind != "torrent")
                .filter(|e| e.manifest.gated_routes().iter().any(|r| r == "stream"))
                .map(|e| (e.manifest.id.clone(), e.manifest.priority()))
                .collect();
            v.sort_by(|a, b| a.1.total_cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
            v
        };
        let params = stream_params(media, episode);
        let futs = candidates.iter().map(|(id, _)| {
            let params = params.clone();
            async move {
                match self.call(id, "stream", params).await {
                    Ok(v) => map_sources(id, &v),
                    Err(e) => {
                        tracing::warn!(extension = %id, error = %e, "stream resolution failed");
                        Vec::new()
                    }
                }
            }
        });
        futures_util::future::join_all(futs).await.into_iter().flatten().collect()
    }

    /// Every source the selector can show for `media` episode `episode`, unranked (docs/redesign/08 section 3):
    /// web entries of the `stream` route of `onlinestream`/`custom` extensions plus torrent entries of the `torrent`
    /// route of `torrent` extensions and of custom extensions with torrent providers ([`candidate_routes`],
    /// docs/redesign/10 section 6). Only enabled, runnable extensions are asked; reliability is
    /// [`tk_core::sources::INITIAL_RELIABILITY`] until the caller applies `LocalStore::reliability`. Failures of one
    /// extension (or one route) never fail the others.
    pub async fn resolve_candidates(&self, media: &Media, episode: u32) -> Vec<SourceCandidate> {
        // One entry per extension with its routes. Extensions run concurrently, but the routes of ONE extension run one
        // after the other, `torrent` first: an extension is a single JS thread, and running `stream` (dozens of
        // scraping providers, ~40 s) next to `torrent` starved the torrent providers past their own per-provider
        // timeout (Toko's Nyaa dropped every result in the app while it returned 20 alone).
        let exts: Vec<(String, String, Vec<&'static str>, f64)> = {
            let map = self.lock();
            let mut v: Vec<_> = map
                .values()
                .filter(|e| e.enabled && e.report.level != Compat::Unsupported)
                .map(|e| (e.manifest.id.clone(), e.manifest.name.clone(), candidate_routes(&e.manifest, e.torrent_all), e.manifest.priority()))
                .filter(|(_, _, routes, _)| !routes.is_empty())
                .collect();
            v.sort_by(|a, b| a.3.total_cmp(&b.3).then_with(|| a.0.cmp(&b.0)));
            v
        };
        let params = stream_params(media, episode);
        let futs = exts.iter().map(|(id, name, routes, _)| {
            let params = params.clone();
            async move {
                let mut ordered: Vec<&'static str> = routes.iter().copied().filter(|r| *r == "torrent").collect();
                ordered.extend(routes.iter().copied().filter(|r| *r != "torrent"));
                let mut by_route: Vec<(&'static str, Vec<SourceCandidate>)> = Vec::new();
                for route in ordered {
                    by_route.push((route, self.list_route(id, name, route, params.clone()).await));
                }
                // Output keeps the manifest's route order (stream before torrent), whatever order they ran in.
                routes
                    .iter()
                    .flat_map(|r| by_route.iter().filter(|(x, _)| x == r).flat_map(|(_, c)| c.clone()).collect::<Vec<_>>())
                    .collect::<Vec<_>>()
            }
        });
        futures_util::future::join_all(futs).await.into_iter().flatten().collect()
    }

    /// One route of one extension mapped to candidates (failures are logged and yield none).
    async fn list_route(&self, id: &str, name: &str, route: &'static str, params: Value) -> Vec<SourceCandidate> {
        let started = std::time::Instant::now();
        let episode = params["episode"].clone();
        match self.call(id, route, params).await {
            Ok(v) => {
                let raw = v["sources"].as_array().map_or(0, Vec::len);
                let mapped = map_candidates(id, name, &v);
                if tracing::enabled!(tracing::Level::DEBUG) {
                    let mut by: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
                    for s in v["sources"].as_array().into_iter().flatten() {
                        let key = s["source"].as_str().or_else(|| s["providerKey"].as_str()).unwrap_or("?").to_owned();
                        *by.entry(key).or_default() += 1;
                    }
                    tracing::debug!(extension = %id, route = %route, episode = %episode, raw, mapped = mapped.len(), ms = started.elapsed().as_millis() as u64, providers = ?by, "source listing");
                }
                mapped
            }
            Err(e) => {
                tracing::warn!(extension = %id, route = %route, error = %e, "source listing failed");
                Vec::new()
            }
        }
    }
}

/// Fresh log buffer that starts with one `warn` line per accepted-but-deferred permission (`torrent:session`).
fn deferred_log(m: &Manifest) -> LogBuf {
    let logs = LogBuf::default();
    for p in deferred_permissions(m) {
        logs.push("warn", &format!("compat: {p} accepted; torrents are listed and played by the app's torrent engine, the bundle gets no session of its own"));
    }
    logs
}

fn is_known_route(route: &str) -> bool {
    matches!(
        route,
        "health" | "sources" | "stream" | "torrent" | "providers" | "manga/chapters" | "manga/pages" | "novel/search" | "novel/chapters"
            | "novel/content" | "custom/home" | "custom/search" | "custom/info" | "custom/watch" | "custom/read"
    ) || route.strip_prefix("providers/").is_some_and(|n| !n.is_empty() && !n.contains('/'))
}

fn subtitle_format(url: &str) -> &'static str {
    let path = url.split(['?', '#']).next().unwrap_or(url).to_ascii_lowercase();
    if path.ends_with(".srt") {
        "srt"
    } else if path.ends_with(".ass") || path.ends_with(".ssa") {
        "ass"
    } else {
        "vtt"
    }
}

/// `nativeMapping.StreamSource` of routes.v1.json: only hls / mp4 / dash(.mpd) are playable.
pub fn map_sources(ext_id: &str, body: &Value) -> Vec<StreamSource> {
    let Some(list) = body["sources"].as_array() else { return Vec::new() };
    list.iter()
        .filter_map(|s| {
            let url = s["url"].as_str().filter(|u| !u.is_empty())?;
            let container = web_container(s, url)?;
            Some(StreamSource {
                url: url.to_owned(),
                headers: headers_of(s),
                container,
                quality: s["quality"].as_str().filter(|q| !q.is_empty()).map(str::to_owned),
                provider: s["providerName"].as_str().unwrap_or(ext_id).to_owned(),
                extension_id: ext_id.to_owned(),
                subtitles: subtitles_of(s),
            })
        })
        .collect()
}

/// Playable container of a normalized source (`type` / `sourceType`); `None` for embed / custom / torrent.
pub(crate) fn web_container(s: &Value, url: &str) -> Option<tk_core::Container> {
    use tk_core::Container;
    match s["type"].as_str().or_else(|| s["sourceType"].as_str()).unwrap_or("") {
        "hls" => Some(Container::Hls),
        "mp4" => Some(Container::Mp4),
        "dash" if url.split(['?', '#']).next().is_some_and(|p| p.ends_with(".mpd")) => Some(Container::Dash),
        _ => None,
    }
}

pub(crate) fn headers_of(s: &Value) -> std::collections::BTreeMap<String, String> {
    s["headers"]
        .as_object()
        .map(|o| o.iter().filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_owned()))).collect())
        .unwrap_or_default()
}

pub(crate) fn subtitles_of(s: &Value) -> Vec<SubtitleTrack> {
    s["subtitles"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|t| {
                    let u = t["url"].as_str().filter(|u| !u.is_empty())?;
                    Some(SubtitleTrack {
                        url: u.to_owned(),
                        lang: t["language"].as_str().or_else(|| t["lang"].as_str()).unwrap_or("und").to_owned(),
                        label: t["label"].as_str().unwrap_or("").to_owned(),
                        format: subtitle_format(u).to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Query params of the `stream` / `torrent` routes for one episode (legacy query-string keys, SSE off).
fn stream_params(media: &Media, episode: u32) -> Value {
    let mut titles: Vec<String> = vec![media.title.clone()];
    for t in media.titles.values() {
        if !titles.contains(t) {
            titles.push(t.clone());
        }
    }
    json!({
        "anilistId": media.media_ref.anilist_id.unwrap_or(0),
        "titles": titles,
        "episode": episode,
        "stream": "0",
        // Same defaults the legacy host sent (desktop/runtime/extension-api-host optionsFromQuery); bundles clamp
        // the per-provider timeout themselves.
        "resolution": "1080p",
        "providerOptions": { "timeoutMs": 20_000 },
    })
}
