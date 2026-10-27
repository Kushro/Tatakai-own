//! `ExtensionHost`: installed-extension registry on disk + per-extension QuickJS workers.
//!
//! Layout under the injected `root`: `<root>/<id>/{manifest.json, bundle.js, state.json}`. Nothing outside `root`
//! is ever read or written (tests pass a tempdir; the app passes `%APPDATA%\Tatakai-Next\extensions`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tk_core::net::{Policy, SafeHttp, host_matches_permissions};
use tk_core::{Compat, InstalledExtension, Media, StreamSource, SubtitleTrack};

use crate::compat::{CompatReport, analyze_bundle};
use crate::error::{ExtError, InstallError};
use crate::manifest::{Manifest, valid_id};
use crate::package::{Package, read_kai};
use crate::runtime::{CALL_TIMEOUT, CallKind, LogBuf, Worker, WorkerConfig};

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
    enabled: bool,
    logs: LogBuf,
    worker: Option<Arc<Worker>>,
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
}

impl ExtensionHost {
    /// Open (creating) the extension store at `root` and load every installed extension.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, InstallError> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        let host = Self { root, entries: Mutex::new(HashMap::new()), call_timeout: CALL_TIMEOUT, loopback_for_tests: false };
        host.reload()?;
        Ok(host)
    }

    /// Override the per-call wall-clock limit (tests only exercise this with small values).
    pub fn with_call_timeout(mut self, t: std::time::Duration) -> Self {
        self.call_timeout = t;
        self
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
            map.insert(
                manifest.id.clone(),
                Entry { manifest, manifest_json: mj, bundle: Arc::new(bundle), report, enabled, logs: LogBuf::default(), worker: None },
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
        let Package { manifest, manifest_json, bundle } = read_kai(path)?;
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
            manifest,
            manifest_json,
            bundle: Arc::new(bundle),
            report,
            logs: LogBuf::default(),
            worker: None,
        };
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
        let worker = Arc::new(Worker::spawn(WorkerConfig {
            id: e.manifest.id.clone(),
            manifest_json: e.manifest_json.clone(),
            bundle: e.bundle.clone(),
            permissions: perms,
            http,
            logs: e.logs.clone(),
            call_timeout: self.call_timeout,
        })?);
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
        let mut titles: Vec<String> = vec![media.title.clone()];
        for t in media.titles.values() {
            if !titles.contains(t) {
                titles.push(t.clone());
            }
        }
        let params = json!({
            "anilistId": media.media_ref.anilist_id.unwrap_or(0),
            "titles": titles,
            "episode": episode,
            "stream": "0",
        });
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
    use tk_core::Container;
    let Some(list) = body["sources"].as_array() else { return Vec::new() };
    list.iter()
        .filter_map(|s| {
            let url = s["url"].as_str().filter(|u| !u.is_empty())?;
            let ty = s["type"].as_str().or_else(|| s["sourceType"].as_str()).unwrap_or("");
            let container = match ty {
                "hls" => Container::Hls,
                "mp4" => Container::Mp4,
                "dash" if url.split(['?', '#']).next().is_some_and(|p| p.ends_with(".mpd")) => Container::Dash,
                _ => return None,
            };
            let headers = s["headers"]
                .as_object()
                .map(|o| o.iter().filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_owned()))).collect())
                .unwrap_or_default();
            let subtitles = s["subtitles"]
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
                .unwrap_or_default();
            Some(StreamSource {
                url: url.to_owned(),
                headers,
                container,
                quality: s["quality"].as_str().filter(|q| !q.is_empty()).map(str::to_owned),
                provider: s["providerName"].as_str().unwrap_or(ext_id).to_owned(),
                extension_id: ext_id.to_owned(),
                subtitles,
            })
        })
        .collect()
}
