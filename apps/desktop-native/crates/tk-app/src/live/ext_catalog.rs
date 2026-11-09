//! Stremio add-ons, extension updates, per-extension preferences, repositories and custom providers (wave B, worker
//! **j**; docs/redesign/16-parity-wave-b-contract.md section 4; checklist `ext.stremio*`, `ext.updates.*`,
//! `ext.preferences`, `ext.repositories`, `ext.install-url`, `ext.kill-switch`, `ext.custom-providers.*`).
//! tk-core side: `tk_core::stremio` (manifest + `/stream/<type>/<id>.json`), `LocalStore` tables of
//! `tk-core/src/store/ext.rs` (`stremio_addons`, `ext_repos`, `ext_prefs`, `ext_updates`, `provider_bookmarks`); the
//! extension host (`b.ext.host()`, `tk_ext::ExtensionHost::call` with the `custom/home`, `custom/search`,
//! `custom/info`, `custom/watch`, `custom/read` routes).
//!
//! `LiveBackend` routes every `ext:<ext_id>:<source_id>:<item_id>` id (`is_provider_id`) of `details`, `episodes`,
//! `playback`, `sources` and `record_progress` here, so provider titles work in the normal Details page and player
//! (`ext.custom-providers.info` / `.watch` / `.continue`; KNOWN-ISSUES of wave A). Their history rows use the `ext:`
//! id as `anime_id` (no AniList / MAL ids: they stay on this PC), so "Continue watching" lists them like any other.
//!
//! Hooks for other owners (applied by the integrator, see the commit message): [`home_rows`] (Home rows of
//! `customHome`, worker b's `live/home.rs`), [`playback_source`] (`LiveBackend::playback_source` for `ext:` ids),
//! [`is_remote_ext_id`] (ranking of `server:` / `stremio:` candidates in `LiveBackend::sources`),
//! [`provider_chapters`] / [`provider_pages`] (`ext.custom-providers.read`, worker l's `live/manga.rs`).

use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use time::OffsetDateTime;
use tk_core::sources::{SourceCandidate, SourceKind, source_key};
use tk_core::store::ext::{ExtRepoRow, ExtUpdateRow, KILL_SWITCH_KEY, ORIGIN_KEY_PREFIX, ProviderBookmarkRow, STREMIO_MATCH_PREFIX, StremioRow};
use tk_core::{Compat, HistoryEntry, Media, MediaKind, MediaRef, stremio};
use tk_ext::manifest::{PrefField, PrefKind};
use tk_ui::backend::{
    ChapterProviderVm, ChapterRefVm, ChapterVm, CustomProviderVm, DetailsVm, EpisodeVm, ExtCatalogItemVm, ExtPrefFieldVm,
    ExtPrefKindVm, ExtRepoVm, ExtUpdateVm, ExtensionVm, MediaVm, PageVm, PlaybackVm, ProviderItemVm, SearchPageVm, SectionVm,
    SourceVm, StremioAddonVm, UiResult,
};
use tk_ui::model::{ErrorKind, UiError};

use crate::live::LiveBackend;

// ---------------------------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------------------------

/// `.kai` downloads (install by URL, updates).
const KAI_CAP: usize = 12 * 1024 * 1024;
/// JSON documents (manifests, repository indexes, update feeds).
const JSON_CAP: usize = 2 * 1024 * 1024;
/// Provider info / home rows / watch candidates are reused this long.
const PROVIDER_TTL: Duration = Duration::from_secs(5 * 60);
/// A repository catalogue older than this is fetched again when opened.
const REPO_TTL_SECS: i64 = 24 * 3600;
/// One Stremio request.
const STREMIO_TIMEOUT: Duration = Duration::from_secs(12);
/// One extension's `custom/home` for the Home rows.
const HOME_ROW_TIMEOUT: Duration = Duration::from_secs(10);
/// Cards of one extension row on Home.
const HOME_ROW_ITEMS: usize = 20;

fn http() -> reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent("TatakaiNext/0.1")
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_default()
    })
    .clone()
}

fn now_unix() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

fn ext_err(e: impl std::fmt::Display) -> UiError {
    UiError::new(ErrorKind::Extension, e.to_string())
}

fn store_err(e: impl std::fmt::Display) -> UiError {
    UiError::new(ErrorKind::Other, e.to_string())
}

fn net_err(e: reqwest::Error) -> UiError {
    let kind = if e.is_timeout() || e.is_connect() { ErrorKind::Offline } else { ErrorKind::Extension };
    UiError::new(kind, e.without_url().to_string())
}

fn not_installed(id: &str) -> UiError {
    UiError::new(ErrorKind::Extension, format!("{id} is not installed"))
}

/// `https://...` (or `http://` to a LAN / loopback host); anything else is refused.
pub(crate) fn checked_url(input: &str) -> Result<reqwest::Url, UiError> {
    let url = reqwest::Url::parse(input.trim()).map_err(|_| UiError::new(ErrorKind::Other, format!("invalid URL: {}", input.trim())))?;
    let host = url.host_str().unwrap_or("");
    let ok = match url.scheme() {
        "https" => !host.is_empty(),
        "http" => tk_core::extserver::is_local_network_host(host),
        _ => false,
    };
    if !ok {
        return Err(UiError::new(ErrorKind::Other, format!("only https URLs are accepted: {}", input.trim())));
    }
    Ok(url)
}

/// GET with a size cap (the body is read chunk by chunk, never more than `cap` bytes).
async fn fetch_limited(url: &reqwest::Url, cap: usize, timeout: Option<Duration>) -> Result<Vec<u8>, UiError> {
    let mut req = http().get(url.clone());
    if let Some(t) = timeout {
        req = req.timeout(t);
    }
    let mut resp = req.send().await.map_err(net_err)?;
    let status = resp.status();
    if !status.is_success() {
        return Err(UiError::new(ErrorKind::Extension, format!("{}: HTTP {}", url.host_str().unwrap_or(""), status.as_u16())));
    }
    let too_big = || UiError::new(ErrorKind::Extension, format!("download larger than {} MiB", cap / (1024 * 1024)));
    if resp.content_length().is_some_and(|n| n > cap as u64) {
        return Err(too_big());
    }
    let mut out: Vec<u8> = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(net_err)? {
        if out.len() + chunk.len() > cap {
            return Err(too_big());
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

async fn fetch_json(url: &reqwest::Url, timeout: Option<Duration>) -> Result<Value, UiError> {
    let bytes = fetch_limited(url, JSON_CAP, timeout).await?;
    serde_json::from_slice(&bytes).map_err(|e| UiError::new(ErrorKind::Extension, format!("invalid JSON: {e}")))
}

/// A TTL cache shared by the provider calls.
struct Ttl<K, V> {
    map: Mutex<HashMap<K, (Instant, V)>>,
}

impl<K: std::hash::Hash + Eq + Clone, V: Clone> Ttl<K, V> {
    fn new() -> Self {
        Self { map: Mutex::new(HashMap::new()) }
    }
    fn get(&self, k: &K) -> Option<V> {
        let m = self.map.lock().ok()?;
        m.get(k).filter(|(at, _)| at.elapsed() < PROVIDER_TTL).map(|(_, v)| v.clone())
    }
    fn put(&self, k: K, v: V) {
        if let Ok(mut m) = self.map.lock() {
            m.retain(|_, (at, _)| at.elapsed() < PROVIDER_TTL);
            m.insert(k, (Instant::now(), v));
        }
    }
    fn clear(&self) {
        if let Ok(mut m) = self.map.lock() {
            m.clear();
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Provider ids
// ---------------------------------------------------------------------------------------------

/// `ext:<extension id>:<key>`: a title that only an extension knows (Search "in extension", provider pages).
pub fn is_provider_id(id: &str) -> bool {
    id.starts_with("ext:")
}

/// The parts of `ext:<ext_id>:<source_id>:<item_id>` (`live/search.rs::ext_item_id`; the item id may contain `:`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ProviderRef {
    pub ext_id: String,
    pub source_id: String,
    pub item_id: String,
}

impl ProviderRef {
    pub fn id(&self) -> String {
        super::search::ext_item_id(&self.ext_id, &self.source_id, &self.item_id)
    }
}

pub(crate) fn parse_provider_id(id: &str) -> Option<ProviderRef> {
    let mut it = id.splitn(4, ':');
    let (Some("ext"), Some(ext), Some(source), Some(item)) = (it.next(), it.next(), it.next(), it.next()) else { return None };
    if ext.is_empty() || source.is_empty() || item.is_empty() {
        return None;
    }
    Some(ProviderRef { ext_id: ext.to_owned(), source_id: source.to_owned(), item_id: item.to_owned() })
}

/// `item_key` of `Backend::provider_item` / `set_provider_bookmark` = `<source_id>:<item_id>`.
fn provider_ref(ext_id: &str, item_key: &str) -> Result<ProviderRef, UiError> {
    parse_provider_id(&format!("ext:{ext_id}:{item_key}"))
        .ok_or_else(|| UiError::new(ErrorKind::Other, format!("invalid provider item {ext_id}:{item_key}")))
}

fn bad_id(id: &str) -> UiError {
    UiError::new(ErrorKind::Other, format!("invalid provider id {id}"))
}

/// Candidates that come from a paired extension server (`server:<id>`) or a Stremio add-on (`stremio:<id>`): they are
/// not installed extensions, so the ranking must treat them as enabled (`LiveBackend::sources`, integrator note).
pub fn is_remote_ext_id(ext_id: &str) -> bool {
    ext_id.starts_with("server:") || ext_id.starts_with("stremio:")
}

// ---------------------------------------------------------------------------------------------
// custom/info
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct InfoEntry {
    pub id: String,
    pub label: String,
    /// Episode / chapter number the app uses (unique, >= 1).
    pub number: u32,
    /// The provider's own number (fractional chapters), when given.
    pub raw_number: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ProviderInfo {
    pub title: String,
    pub image: Option<String>,
    pub banner: Option<String>,
    pub description: Option<String>,
    pub meta: Vec<(String, String)>,
    pub entries: Vec<InfoEntry>,
}

fn text_of(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.trim().to_owned()).filter(|s| !s.is_empty()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn http_url(v: &Value) -> Option<String> {
    text_of(v).filter(|u| u.starts_with("https://") || u.starts_with("http://"))
}

/// `CustomInfoResult` -> [`ProviderInfo`]. Entries are numbered by their `number` when every one has a distinct
/// positive integer number; otherwise by position (1-based), so the player's episode number always maps back.
pub(crate) fn parse_info(body: &Value) -> Option<ProviderInfo> {
    let title = text_of(&body["title"])?;
    let raw: Vec<(String, String, Option<f64>)> = body["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|e| {
            let id = text_of(&e["id"])?;
            let number = e["number"].as_f64().or_else(|| e["number"].as_str().and_then(|s| s.trim().parse().ok()));
            let label = text_of(&e["label"]).or_else(|| text_of(&e["title"])).unwrap_or_else(|| id.clone());
            Some((id, label, number))
        })
        .collect();
    let integral: Vec<Option<u32>> = raw
        .iter()
        .map(|(_, _, n)| n.filter(|x| *x >= 1.0 && x.fract() == 0.0 && *x < f64::from(u32::MAX)).map(|x| x as u32))
        .collect();
    let distinct = {
        let mut seen = std::collections::HashSet::new();
        integral.iter().all(|n| n.is_some_and(|n| seen.insert(n)))
    };
    let mut entries: Vec<InfoEntry> = raw
        .into_iter()
        .enumerate()
        .map(|(i, (id, label, raw_number))| InfoEntry {
            id,
            label,
            number: if distinct { integral[i].unwrap_or(i as u32 + 1) } else { i as u32 + 1 },
            raw_number,
        })
        .collect();
    if distinct {
        entries.sort_by_key(|e| e.number);
    }
    let meta: Vec<(String, String)> = body["meta"]
        .as_object()
        .map(|o| o.iter().filter_map(|(k, v)| text_of(v).map(|v| (k.clone(), v))).collect())
        .unwrap_or_default();
    Some(ProviderInfo {
        title,
        image: http_url(&body["image"]),
        banner: http_url(&body["banner"]),
        description: text_of(&body["description"]),
        meta,
        entries,
    })
}

fn meta_value(meta: &[(String, String)], keys: &[&str]) -> Option<String> {
    meta.iter().find(|(k, _)| keys.iter().any(|x| k.eq_ignore_ascii_case(x))).map(|(_, v)| v.clone())
}

fn info_cache() -> &'static Ttl<ProviderRef, ProviderInfo> {
    static C: OnceLock<Ttl<ProviderRef, ProviderInfo>> = OnceLock::new();
    C.get_or_init(Ttl::new)
}

fn enabled_extension(b: &LiveBackend, ext_id: &str) -> Result<tk_core::InstalledExtension, UiError> {
    let info = b.ext.host().get(ext_id).ok_or_else(|| not_installed(ext_id))?;
    if !info.enabled {
        return Err(UiError::new(ErrorKind::Extension, format!("{} is disabled", info.name)));
    }
    Ok(info)
}

async fn provider_info(b: &LiveBackend, r: &ProviderRef) -> Result<ProviderInfo, UiError> {
    if let Some(i) = info_cache().get(r) {
        return Ok(i);
    }
    enabled_extension(b, &r.ext_id)?;
    let body = b
        .ext
        .host()
        .call(&r.ext_id, "custom/info", json!({ "sourceId": r.source_id, "id": r.item_id }))
        .await
        .map_err(ext_err)?;
    let info = parse_info(&body).ok_or_else(|| UiError::new(ErrorKind::Extension, format!("{}: custom/info returned no title", r.ext_id)))?;
    b.remember_title(&r.id(), &info.title, info.image.as_deref());
    info_cache().put(r.clone(), info.clone());
    Ok(info)
}

fn ext_name(b: &LiveBackend, ext_id: &str) -> String {
    b.ext.host().get(ext_id).map(|e| e.name).unwrap_or_else(|| ext_id.to_owned())
}

fn history_of(b: &LiveBackend, id: &str) -> Vec<HistoryEntry> {
    b.store.progress_for(id).unwrap_or_default()
}

fn details_vm(b: &LiveBackend, r: &ProviderRef, info: &ProviderInfo) -> DetailsVm {
    let id = r.id();
    let mut rows = history_of(b, &id);
    rows.sort_by(|a, b| b.watched_at.cmp(&a.watched_at));
    let (resume_episode, resume_secs) = match rows.first() {
        Some(e) if e.completed => (e.episode + 1, 0),
        Some(e) => (e.episode, e.progress_sec),
        None => (0, 0),
    };
    let genres: Vec<String> = meta_value(&info.meta, &["genres", "genre", "géneros", "generos", "tags"])
        .map(|g| g.split([',', '/', '·']).map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect())
        .unwrap_or_default();
    DetailsVm {
        media: MediaVm {
            id,
            title: info.title.clone(),
            cover: info.image.clone(),
            color: None,
            subtitle: ext_name(b, &r.ext_id),
            score: None,
        },
        banner: info.banner.clone().or_else(|| info.image.clone()),
        synopsis: info.description.clone(),
        genres,
        status: meta_value(&info.meta, &["status", "estado"]),
        episodes_total: u32::try_from(info.entries.len()).ok().filter(|n| *n > 0),
        resume_episode,
        resume_secs,
    }
}

fn episodes_vm(b: &LiveBackend, id: &str, info: &ProviderInfo) -> Vec<EpisodeVm> {
    let watched: HashMap<u32, f64> = history_of(b, id)
        .iter()
        .map(|e| {
            let f = if e.completed {
                1.0
            } else {
                e.duration_sec.filter(|d| *d > 0).map_or(0.0, |d| (f64::from(e.progress_sec) / f64::from(d)).clamp(0.0, 1.0))
            };
            (e.episode, f)
        })
        .collect();
    info.entries
        .iter()
        .map(|e| EpisodeVm {
            number: e.number,
            title: Some(e.label.clone()),
            image: None,
            air_date: None,
            is_filler: false,
            watched: watched.get(&e.number).copied().unwrap_or(0.0),
        })
        .collect()
}

pub async fn details(b: &LiveBackend, id: &str) -> UiResult<DetailsVm> {
    let r = parse_provider_id(id).ok_or_else(|| bad_id(id))?;
    let info = provider_info(b, &r).await?;
    Ok(details_vm(b, &r, &info))
}

pub async fn episodes(b: &LiveBackend, id: &str) -> UiResult<Vec<EpisodeVm>> {
    let r = parse_provider_id(id).ok_or_else(|| bad_id(id))?;
    let info = provider_info(b, &r).await?;
    Ok(episodes_vm(b, id, &info))
}

// ---------------------------------------------------------------------------------------------
// custom/watch -> candidates, playback, sources
// ---------------------------------------------------------------------------------------------

/// The `type` the host's own `normalize` would give one `custom/watch` source (the route returns the bundle's raw
/// `SourceResult`s): explicit `sourceType` / `type`, else from the URL.
fn source_type(s: &Value) -> String {
    let explicit = s["sourceType"].as_str().or_else(|| s["type"].as_str()).unwrap_or("").trim().to_ascii_lowercase();
    if !explicit.is_empty() {
        return explicit;
    }
    let url = s["url"].as_str().unwrap_or("");
    let path = url.split(['?', '#']).next().unwrap_or(url).to_ascii_lowercase();
    if path.ends_with(".m3u8") {
        "hls".into()
    } else if path.ends_with(".mp4") {
        "mp4".into()
    } else if path.ends_with(".mpd") {
        "dash".into()
    } else if url.to_ascii_lowercase().starts_with("magnet:") || path.ends_with(".torrent") {
        "torrent".into()
    } else {
        "embed".into()
    }
}

/// `{ sources: SourceResult[] }` of `custom/watch` -> the normalized route shape `tk_ext::map_candidates` reads.
pub(crate) fn normalize_watch(provider: &str, body: &Value) -> Value {
    let list = if body.is_array() { body } else { &body["sources"] };
    let sources: Vec<Value> = list
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| s.is_object())
        .map(|s| {
            let ty = source_type(s);
            let mut o = s.clone();
            o["type"] = json!(ty.clone());
            o["sourceType"] = json!(ty);
            if o["providerName"].as_str().is_none_or(str::is_empty) {
                o["providerName"] = json!(s["source"].as_str().filter(|p| !p.is_empty()).unwrap_or(provider));
            }
            if o["audioLanguage"].is_null()
                && let Some(l) = s["language"].as_str()
            {
                o["audioLanguage"] = json!(l);
            }
            o
        })
        .collect();
    json!({ "sources": sources })
}

fn watch_cache() -> &'static Ttl<(String, u32), Vec<SourceCandidate>> {
    static C: OnceLock<Ttl<(String, u32), Vec<SourceCandidate>>> = OnceLock::new();
    C.get_or_init(Ttl::new)
}

async fn watch_candidates(b: &LiveBackend, id: &str, episode: u32, fresh: bool) -> Result<Vec<SourceCandidate>, UiError> {
    let key = (id.to_owned(), episode);
    if !fresh && let Some(v) = watch_cache().get(&key) {
        return Ok(v);
    }
    let r = parse_provider_id(id).ok_or_else(|| bad_id(id))?;
    let info = provider_info(b, &r).await?;
    let entry = info
        .entries
        .iter()
        .find(|e| e.number == episode)
        .ok_or_else(|| UiError::new(ErrorKind::NoSource, format!("{id}: episode {episode} not listed")))?;
    let ext = enabled_extension(b, &r.ext_id)?;
    let body = b
        .ext
        .host()
        .call(&r.ext_id, "custom/watch", json!({ "sourceId": r.source_id, "id": r.item_id, "episodeId": entry.id }))
        .await;
    let body = match body {
        Ok(v) => {
            b.ext.log(&r.ext_id, true, "watch", format!("\"{}\" {} -> ok", info.title, entry.label));
            v
        }
        Err(e) => {
            b.ext.log(&r.ext_id, false, "watch", e.to_string());
            return Err(ext_err(e));
        }
    };
    let mut list = tk_ext::map_candidates(&r.ext_id, &ext.name, &normalize_watch(&ext.name, &body));
    for c in &mut list {
        c.reliability = b.store.reliability(&c.ext_id, &c.label).unwrap_or(tk_core::sources::INITIAL_RELIABILITY);
    }
    watch_cache().put(key, list.clone());
    Ok(list)
}

/// Synthetic `Media` of a provider title (the sniffer and the stream mapping want one).
fn provider_media(r: &ProviderRef, info: &ProviderInfo) -> Media {
    Media {
        media_ref: MediaRef::default(),
        kind: MediaKind::Anime,
        title: info.title.clone(),
        titles: Default::default(),
        cover: info.image.clone(),
        banner: info.banner.clone(),
        color: None,
        synopsis: info.description.clone(),
        genres: Vec::new(),
        episodes: u32::try_from(info.entries.len()).ok(),
        status: None,
        season: None,
        year: None,
        score: None,
        source_provider: format!("ext:{}", r.ext_id),
    }
}

fn start_secs(b: &LiveBackend, id: &str, episode: u32) -> u32 {
    history_of(b, id).iter().find(|e| e.episode == episode && !e.completed).map_or(0, |e| e.progress_sec)
}

fn ranked(b: &LiveBackend, id: &str, episode: u32, candidates: Vec<SourceCandidate>) -> Vec<SourceVm> {
    let settings = crate::settings_store::load(&b.settings_file);
    let mut candidates = candidates;
    if !settings.server.sniffer_enabled {
        candidates.retain(|c| !c.web.as_ref().is_some_and(|w| w.embed));
    }
    let prefs = super::sources::core_prefs(&settings.sources);
    let enabled: HashMap<String, bool> =
        b.ext.installed().into_iter().map(|e| (e.id, e.enabled && e.compat != Compat::Unsupported)).collect();
    let picked = b.store.picked_source(id, episode).ok().flatten();
    super::sources::ranked_vms(candidates, &prefs, |ext_id| enabled.get(ext_id).copied().unwrap_or(false), picked.as_deref())
}

async fn play_candidate(b: &LiveBackend, id: &str, episode: u32, c: &SourceCandidate) -> UiResult<PlaybackVm> {
    let r = parse_provider_id(id).ok_or_else(|| bad_id(id))?;
    let info = provider_info(b, &r).await?;
    let media = provider_media(&r, &info);
    if c.web.as_ref().is_some_and(|w| w.embed) {
        let mut vm = super::sniffer::playback_embed(b, &media.media_ref, &media, episode, c).await?;
        vm.start_secs = start_secs(b, id, episode);
        return Ok(vm);
    }
    let stream = match c.kind {
        SourceKind::Torrent => return Err(UiError::new(ErrorKind::TorrentM2, source_key(c))),
        SourceKind::Web => c.to_stream().ok_or_else(|| UiError::new(ErrorKind::NoSource, source_key(c)))?,
    };
    Ok(PlaybackVm {
        title: info.title.clone(),
        poster: info.image.clone(),
        episodes_total: media.episodes,
        start_secs: start_secs(b, id, episode),
        streams: vec![super::sources::stream_vm(stream)],
    })
}

/// `Backend::playback` for provider titles: the playable web streams of `custom/watch`, best first.
pub async fn playback(b: &LiveBackend, id: &str, episode: u32) -> UiResult<PlaybackVm> {
    let r = parse_provider_id(id).ok_or_else(|| bad_id(id))?;
    let info = provider_info(b, &r).await?;
    let candidates = watch_candidates(b, id, episode, false).await?;
    let ranked_ids: Vec<String> = ranked(b, id, episode, candidates.clone()).into_iter().map(|s| s.id).collect();
    let by_rank = |c: &SourceCandidate| ranked_ids.iter().position(|k| *k == source_key(c)).unwrap_or(usize::MAX);
    let mut direct: Vec<&SourceCandidate> =
        candidates.iter().filter(|c| c.kind == SourceKind::Web && !c.web.as_ref().is_some_and(|w| w.embed)).collect();
    direct.sort_by_key(|c| by_rank(c));
    let streams: Vec<_> = direct.iter().filter_map(|c| c.to_stream()).map(super::sources::stream_vm).collect();
    if !streams.is_empty() {
        return Ok(PlaybackVm {
            title: info.title.clone(),
            poster: info.image.clone(),
            episodes_total: u32::try_from(info.entries.len()).ok(),
            start_secs: start_secs(b, id, episode),
            streams,
        });
    }
    // Only embed pages: the sniffer resolves the best one (when enabled).
    let mut embeds: Vec<&SourceCandidate> = candidates.iter().filter(|c| c.web.as_ref().is_some_and(|w| w.embed)).collect();
    embeds.sort_by_key(|c| by_rank(c));
    let sniffer_on = crate::settings_store::load(&b.settings_file).server.sniffer_enabled;
    match embeds.first() {
        Some(c) if sniffer_on => play_candidate(b, id, episode, c).await,
        _ => Err(UiError::new(ErrorKind::NoSource, format!("{}: {}", r.ext_id, info.title))),
    }
}

pub async fn sources(b: &LiveBackend, id: &str, episode: u32) -> UiResult<Vec<SourceVm>> {
    let candidates = watch_candidates(b, id, episode, false).await?;
    Ok(ranked(b, id, episode, candidates))
}

/// `LiveBackend::playback_source` for provider ids (integrator note: the generic body resolves `id` through the
/// metadata router, which does not know `ext:` titles).
pub async fn playback_source(b: &LiveBackend, id: &str, episode: u32, source_id: &str) -> UiResult<PlaybackVm> {
    let find = |list: Vec<SourceCandidate>| list.into_iter().find(|c| source_key(c) == source_id);
    let candidate = match find(watch_candidates(b, id, episode, false).await?) {
        Some(c) => c,
        None => find(watch_candidates(b, id, episode, true).await?).ok_or_else(|| UiError::new(ErrorKind::NoSource, source_id))?,
    };
    play_candidate(b, id, episode, &candidate).await
}

/// History rows of provider titles (`ext.custom-providers.continue`): `HistoryEntry` with the `ext:` id as `anime_id`
/// and no AniList / MAL ids (local only; Link skips rows without them, which is intended).
pub async fn record_progress(b: &LiveBackend, id: &str, episode: u32, progress_secs: u32, duration_secs: Option<u32>) -> UiResult<()> {
    let r = parse_provider_id(id).ok_or_else(|| bad_id(id))?;
    let cached = info_cache().get(&r);
    let seen = b.titles.lock().ok().and_then(|t| t.get(id).cloned());
    let (title, poster) = match seen {
        Some(t) => t,
        None => match &cached {
            Some(i) => (i.title.clone(), i.image.clone()),
            None => match b.store.provider_bookmark(id).ok().flatten() {
                Some(bm) => (bm.title, bm.poster),
                None => (String::new(), None),
            },
        },
    };
    let entry = provider_history(id, &title, poster.as_deref(), episode, progress_secs, duration_secs, OffsetDateTime::now_utc());
    b.store.record_history(&entry).map_err(store_err)?;
    if entry.completed {
        if let Err(e) = b.store.clear_picked_source(id, episode) {
            tracing::warn!(error = %e, "could not clear the picked source");
        }
        // The last entry watched: a bookmark of the title is marked completed (`ext.custom-providers.bookmark`).
        let last = cached.as_ref().and_then(|i| i.entries.iter().map(|e| e.number).max());
        if last.is_some_and(|l| episode >= l) {
            let _ = b.store.set_provider_bookmark_completed(id, true);
        }
    }
    Ok(())
}

/// The history row of a provider title (pure; `anime_id` = the `ext:` id).
pub(crate) fn provider_history(
    id: &str,
    title: &str,
    poster: Option<&str>,
    episode: u32,
    progress_secs: u32,
    duration_secs: Option<u32>,
    at: OffsetDateTime,
) -> HistoryEntry {
    HistoryEntry {
        anime_id: id.to_owned(),
        title: title.to_owned(),
        poster: poster.map(str::to_owned),
        episode_id: HistoryEntry::episode_key(id, episode),
        episode,
        progress_sec: progress_secs,
        duration_sec: duration_secs,
        completed: HistoryEntry::is_completed(progress_secs, duration_secs),
        watched_at: at,
        anilist_id: None,
        mal_id: None,
    }
}

// ---------------------------------------------------------------------------------------------
// custom/read (worker l consumes these through `manga_chapters` / `manga_pages` for `ext:` ids)
// ---------------------------------------------------------------------------------------------

fn hex(s: &str) -> String {
    s.bytes().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<String> {
    if s.len() % 2 != 0 {
        return None;
    }
    let bytes: Option<Vec<u8>> = (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect();
    String::from_utf8(bytes?).ok()
}

/// `ChapterRefVm::chapter_id` of a provider chapter: `custom.<hex source>.<hex item>.<hex entry>` (route-safe).
pub(crate) fn provider_chapter_id(source_id: &str, item_id: &str, entry_id: &str) -> String {
    format!("custom.{}.{}.{}", hex(source_id), hex(item_id), hex(entry_id))
}

/// `(source_id, item_id, entry_id)` of a [`provider_chapter_id`].
pub(crate) fn parse_provider_chapter(chapter_id: &str) -> Option<(String, String, String)> {
    let mut it = chapter_id.strip_prefix("custom.")?.split('.');
    let (s, i, e) = (unhex(it.next()?)?, unhex(it.next()?)?, unhex(it.next()?)?);
    (it.next().is_none() && !s.is_empty() && !i.is_empty() && !e.is_empty()).then_some((s, i, e))
}

/// Whether a chapter reference points at a custom provider (`provider_pages` reads it).
pub fn is_provider_chapter(r: &ChapterRefVm) -> bool {
    parse_provider_chapter(&r.chapter_id).is_some()
}

/// Chapters of a provider title of kind `read` (`ext.custom-providers.read`): one provider block, the extension's.
pub async fn provider_chapters(b: &LiveBackend, id: &str) -> UiResult<Vec<ChapterProviderVm>> {
    let r = parse_provider_id(id).ok_or_else(|| bad_id(id))?;
    let ext_name = ext_name(b, &r.ext_id);
    let chapters = match provider_info(b, &r).await {
        Ok(info) => info,
        Err(e) => return Ok(vec![ChapterProviderVm { ext_id: r.ext_id, ext_name, lang: None, chapters: Vec::new(), error: Some(e) }]),
    };
    let list = chapters
        .entries
        .iter()
        .map(|e| ChapterVm {
            reference: ChapterRefVm { ext_id: r.ext_id.clone(), chapter_id: provider_chapter_id(&r.source_id, &r.item_id, &e.id) },
            number: e.raw_number.map_or(e.number as f32, |n| n as f32),
            volume: None,
            title: Some(e.label.clone()),
            lang: None,
            group: None,
            published_unix: None,
            pages: None,
            read: 0.0,
            downloaded: false,
        })
        .collect();
    Ok(vec![ChapterProviderVm { ext_id: r.ext_id, ext_name, lang: None, chapters: list, error: None }])
}

/// `CustomReadResult` -> pages (`pageNumber` order, `http(s)` images only).
pub(crate) fn parse_pages(body: &Value) -> Vec<PageVm> {
    let list = if body.is_array() { body } else { &body["pages"] };
    let mut out: Vec<(u32, PageVm)> = list
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .filter_map(|(i, p)| {
            let url = http_url(&p["imageUrl"]).or_else(|| http_url(&p["url"])).or_else(|| http_url(p))?;
            let n = p["pageNumber"].as_u64().and_then(|n| u32::try_from(n).ok()).unwrap_or(i as u32 + 1);
            let headers = p["headers"]
                .as_object()
                .map(|o| o.iter().filter_map(|(k, v)| v.as_str().map(|v| (k.clone(), v.to_owned()))).collect())
                .unwrap_or_default();
            Some((n, PageVm { index: 0, url, headers, width: p["width"].as_u64().map(|w| w as u32), height: p["height"].as_u64().map(|h| h as u32) }))
        })
        .collect();
    out.sort_by_key(|(n, _)| *n);
    out.into_iter()
        .enumerate()
        .map(|(i, (_, mut p))| {
            p.index = i as u32;
            p
        })
        .collect()
}

/// Pages of a provider chapter (`custom/read`).
pub async fn provider_pages(b: &LiveBackend, r: &ChapterRefVm) -> UiResult<Vec<PageVm>> {
    let (source, item, entry) =
        parse_provider_chapter(&r.chapter_id).ok_or_else(|| UiError::new(ErrorKind::Other, format!("invalid provider chapter {}", r.chapter_id)))?;
    enabled_extension(b, &r.ext_id)?;
    let body = b
        .ext
        .host()
        .call(&r.ext_id, "custom/read", json!({ "sourceId": source, "id": item, "chapterId": entry }))
        .await
        .map_err(ext_err)?;
    let pages = parse_pages(&body);
    if pages.is_empty() {
        return Err(UiError::new(ErrorKind::NoSource, format!("{}: no pages", r.ext_id)));
    }
    Ok(pages)
}

// ---------------------------------------------------------------------------------------------
// Custom providers: list, home rows, provider page, bookmarks
// ---------------------------------------------------------------------------------------------

fn provider_kind(source_kind: &str) -> &'static str {
    match source_kind {
        "read" | "manga" => "manga",
        "novel" => "novel",
        _ => "anime",
    }
}

pub async fn custom_providers(b: &LiveBackend) -> UiResult<Vec<CustomProviderVm>> {
    let host = b.ext.host();
    let mut out = Vec::new();
    for e in host.list() {
        let Some(m) = host.manifest(&e.id) else { continue };
        for s in &m.custom_sources {
            out.push(CustomProviderVm {
                ext_id: e.id.clone(),
                ext_name: e.name.clone(),
                name: s.name.clone(),
                kind: provider_kind(&s.kind).to_owned(),
                has_home: true,
                has_search: s.kind == "watch" || s.kind == "read",
            });
        }
    }
    Ok(out)
}

/// `CustomHomeResult` -> `(section title, cards)`.
pub(crate) fn parse_home(ext_id: &str, source_id: &str, body: &Value) -> Vec<(String, Vec<MediaVm>)> {
    body["sections"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| {
            let title = text_of(&s["title"])?;
            let items = super::search::map_custom_results(ext_id, source_id, &json!({ "results": s["items"].clone() }));
            Some((title, items))
        })
        .collect()
}

async fn source_home(b: &LiveBackend, ext_id: &str, source_id: &str) -> Result<Vec<(String, Vec<MediaVm>)>, UiError> {
    let body = b.ext.host().call(ext_id, "custom/home", json!({ "sourceId": source_id })).await.map_err(ext_err)?;
    Ok(parse_home(ext_id, source_id, &body))
}

fn home_cache() -> &'static Ttl<String, Vec<SectionVm>> {
    static C: OnceLock<Ttl<String, Vec<SectionVm>>> = OnceLock::new();
    C.get_or_init(Ttl::new)
}

/// Rows of one provider page: `title_key` = `ext:<extension name>:<row title>` (one failed source = one failed row).
pub async fn provider_home(b: &LiveBackend, ext_id: &str) -> UiResult<Vec<SectionVm>> {
    if let Some(v) = home_cache().get(&ext_id.to_owned()) {
        return Ok(v);
    }
    let info = enabled_extension(b, ext_id)?;
    let manifest = b.ext.host().manifest(ext_id).ok_or_else(|| not_installed(ext_id))?;
    if manifest.custom_sources.is_empty() {
        return Err(UiError::new(ErrorKind::Extension, format!("{} has no provider pages", info.name)));
    }
    let mut out: Vec<SectionVm> = Vec::new();
    let homes = super::ext_server::join_all_futures(manifest.custom_sources.iter().map(|s| source_home(b, ext_id, &s.id))).await;
    for (s, r) in manifest.custom_sources.iter().zip(homes) {
        match r {
            Ok(sections) => {
                for (title, items) in sections {
                    let key = format!("ext:{}:{title}", info.name);
                    if !out.iter().any(|x| x.title_key == key) {
                        out.push(SectionVm { title_key: key, items: Ok(items) });
                    }
                }
            }
            Err(e) => {
                b.ext.log(ext_id, false, "custom/home", format!("{}: {}", s.name, e.detail));
                out.push(SectionVm { title_key: format!("ext:{}:{}", info.name, s.name), items: Err(e) });
            }
        }
    }
    if out.iter().all(|s| s.items.is_ok()) {
        home_cache().put(ext_id.to_owned(), out.clone());
    }
    Ok(out)
}

/// "See all" of a provider row: `row` is the row title (the part after `ext:<name>:`). Home rows are not paged by
/// the extensions, so page 1 holds the whole row.
pub async fn provider_feed(b: &LiveBackend, ext_id: &str, row: &str, page: u32) -> UiResult<SearchPageVm> {
    let rows = provider_home(b, ext_id).await?;
    let name = ext_name(b, ext_id);
    let key = format!("ext:{name}:{row}");
    let section = rows.into_iter().find(|s| s.title_key == key).ok_or_else(|| UiError::new(ErrorKind::Other, format!("{name}: no row {row}")))?;
    let items = section.items?;
    let total = u32::try_from(items.len()).ok();
    Ok(SearchPageVm { items: if page <= 1 { items } else { Vec::new() }, page: page.max(1), has_next: false, total })
}

/// Home rows of the extensions with watch providers (`ext.custom-providers.home-rows`): one row per extension,
/// `title_key` = `ext:<extension name>` (Home renders "From <name>"), cards = the first row of each provider.
/// Integrator: `live/home.rs::home` appends `ext_catalog::home_rows(b).await` to `sections`.
pub async fn home_rows(b: &LiveBackend) -> Vec<SectionVm> {
    let host = b.ext.host();
    let exts: Vec<(tk_core::InstalledExtension, Vec<String>)> = host
        .list()
        .into_iter()
        .filter(|e| e.enabled && e.compat != Compat::Unsupported)
        .filter_map(|e| {
            let m = host.manifest(&e.id)?;
            let sources: Vec<String> = m.custom_sources.iter().filter(|s| s.kind == "watch").map(|s| s.id.clone()).collect();
            (!sources.is_empty()).then_some((e, sources))
        })
        .collect();
    let work = exts.iter().map(|(e, sources)| async move {
        let all = super::ext_server::join_all_futures(sources.iter().map(|s| async move {
            match tokio::time::timeout(HOME_ROW_TIMEOUT, source_home(b, &e.id, s)).await {
                Ok(r) => r,
                Err(_) => Err(UiError::new(ErrorKind::Offline, format!("{}: custom/home timed out", e.name))),
            }
        }))
        .await;
        let mut items: Vec<MediaVm> = Vec::new();
        let mut first_err: Option<UiError> = None;
        for r in all {
            match r {
                Ok(sections) => {
                    if let Some((_, cards)) = sections.into_iter().next() {
                        for c in cards {
                            if items.len() < HOME_ROW_ITEMS && !items.iter().any(|x| x.id == c.id) {
                                items.push(c);
                            }
                        }
                    }
                }
                Err(err) => {
                    if first_err.is_none() {
                        first_err = Some(err);
                    }
                }
            }
        }
        let result = match first_err {
            Some(err) if items.is_empty() => Err(err),
            _ => Ok(items),
        };
        SectionVm { title_key: format!("ext:{}", e.name), items: result }
    });
    super::ext_server::join_all_futures(work).await
}

pub async fn provider_item(b: &LiveBackend, ext_id: &str, item_key: &str) -> UiResult<ProviderItemVm> {
    let r = provider_ref(ext_id, item_key)?;
    let info = provider_info(b, &r).await?;
    let id = r.id();
    Ok(ProviderItemVm {
        details: details_vm(b, &r, &info),
        episodes: episodes_vm(b, &id, &info),
        bookmarked: b.store.provider_bookmark(&id).ok().flatten().is_some(),
    })
}

/// Card of a bookmark: the subtitle names the extension, `✓` once completed.
fn bookmark_vm(b: &LiveBackend, row: &ProviderBookmarkRow) -> MediaVm {
    let name = ext_name(b, &row.ext_id);
    MediaVm {
        id: row.media_id.clone(),
        title: row.title.clone(),
        cover: row.poster.clone(),
        color: None,
        subtitle: if row.completed { format!("✓ {name}") } else { name },
        score: None,
    }
}

pub async fn provider_bookmarks(b: &LiveBackend) -> UiResult<Vec<MediaVm>> {
    Ok(b.store.provider_bookmarks().map_err(store_err)?.iter().map(|r| bookmark_vm(b, r)).collect())
}

pub async fn set_provider_bookmark(b: &LiveBackend, ext_id: &str, item_key: &str, bookmarked: bool) -> UiResult<()> {
    let r = provider_ref(ext_id, item_key)?;
    let id = r.id();
    if !bookmarked {
        b.store.remove_provider_bookmark(&id).map_err(store_err)?;
        return Ok(());
    }
    let known = info_cache().get(&r).map(|i| (i.title, i.image));
    let seen = b.titles.lock().ok().and_then(|t| t.get(&id).cloned());
    let (title, poster) = match known.or(seen) {
        Some(t) => t,
        None => {
            let i = provider_info(b, &r).await?;
            (i.title, i.image)
        }
    };
    let row = ProviderBookmarkRow { media_id: id, ext_id: r.ext_id.clone(), title, poster, completed: false, added_at: now_unix() };
    b.store.set_provider_bookmark(&row).map_err(store_err)
}

// ---------------------------------------------------------------------------------------------
// Remote candidates: extension servers + Stremio
// ---------------------------------------------------------------------------------------------

/// Candidates from paired extension servers (`ext.server-client.sources`, `ext.ecosystems.via-server`) and Stremio
/// add-ons (`ext.stremio`) for the ranking of `LiveBackend::episode_candidates`. `ext_id` of a remote candidate is
/// `server:<server id>` (`ext_name` = the server's name); of a Stremio one `stremio:<addon id>` (the add-on's name).
pub async fn extra_candidates(b: &LiveBackend, media: &Media, episode: u32) -> Vec<SourceCandidate> {
    let (remote, addons) = tokio::join!(super::ext_server::remote_candidates(b, media, episode), stremio_candidates(b, media, episode));
    remote.into_iter().chain(addons).collect()
}

fn stremio_vm(row: &StremioRow) -> StremioAddonVm {
    match stremio::Manifest::parse(&row.manifest_json) {
        Ok(m) => StremioAddonVm {
            id: row.id.clone(),
            name: m.name.clone(),
            manifest_url: row.manifest_url.clone(),
            version: m.version.clone(),
            description: m.description.clone().filter(|d| !d.trim().is_empty()),
            types: m.types.clone(),
            resources: m.resource_names(),
            enabled: row.enabled,
        },
        Err(e) => StremioAddonVm {
            id: row.id.clone(),
            name: stremio::base_url(&row.manifest_url),
            manifest_url: row.manifest_url.clone(),
            version: String::new(),
            description: Some(e),
            types: Vec::new(),
            resources: Vec::new(),
            enabled: row.enabled,
        },
    }
}

pub async fn stremio_addons(b: &LiveBackend) -> UiResult<Vec<StremioAddonVm>> {
    Ok(b.store.stremio_addons().map_err(store_err)?.iter().map(stremio_vm).collect())
}

fn match_key_of(id: &str) -> String {
    format!("{STREMIO_MATCH_PREFIX}{id}")
}

/// Install (or refresh) an add-on from its manifest URL. Adding an URL that is already there refreshes the manifest
/// ("Update") and forgets the remembered title matches ("Reset matches", `ext.stremio.manage`).
pub async fn stremio_add(b: &LiveBackend, manifest_url: &str) -> UiResult<StremioAddonVm> {
    let url = stremio::manifest_url(manifest_url).ok_or_else(|| UiError::new(ErrorKind::Other, format!("invalid add-on URL: {}", manifest_url.trim())))?;
    let parsed = checked_url(&url)?;
    let bytes = fetch_limited(&parsed, JSON_CAP, Some(STREMIO_TIMEOUT)).await?;
    let text = String::from_utf8(bytes).map_err(|_| UiError::new(ErrorKind::Extension, "manifest.json: not UTF-8"))?;
    let m = stremio::Manifest::parse(&text).map_err(|e| UiError::new(ErrorKind::Extension, e))?;
    if !m.resource_names().iter().any(|r| r == "stream") {
        return Err(UiError::new(ErrorKind::Extension, format!("{}: the add-on serves no streams", m.name)));
    }
    let existing = b.store.stremio_by_url(&url).map_err(store_err)?;
    let row = StremioRow {
        id: existing.as_ref().map_or_else(tk_core::extserver::new_id, |r| r.id.clone()),
        manifest_url: url.clone(),
        manifest_json: text,
        enabled: existing.as_ref().is_none_or(|r| r.enabled),
        added_at: existing.as_ref().map_or_else(now_unix, |r| r.added_at),
    };
    b.store.upsert_stremio(&row).map_err(store_err)?;
    let _ = b.store.delete_setting(&match_key_of(&row.id));
    let row = b.store.stremio_by_url(&url).map_err(store_err)?.unwrap_or(row);
    Ok(stremio_vm(&row))
}

pub async fn stremio_remove(b: &LiveBackend, id: &str) -> UiResult<()> {
    if !b.store.delete_stremio(id).map_err(store_err)? {
        return Err(UiError::new(ErrorKind::Other, format!("add-on {id} not found")));
    }
    let _ = b.store.delete_setting(&match_key_of(id));
    Ok(())
}

pub async fn stremio_set_enabled(b: &LiveBackend, id: &str, enabled: bool) -> UiResult<()> {
    if !b.store.set_stremio_enabled(id, enabled).map_err(store_err)? {
        return Err(UiError::new(ErrorKind::Other, format!("add-on {id} not found")));
    }
    Ok(())
}

/// MAL / Kitsu ids of an AniList title (ani.zip), cached for the session.
async fn title_ids(r: &MediaRef) -> stremio::TitleIds {
    static IDS: OnceLock<Mutex<HashMap<u32, MediaRef>>> = OnceLock::new();
    let mut ids = stremio::TitleIds { kitsu: r.kitsu_id, mal: r.mal_id, anilist: r.anilist_id };
    if ids.kitsu.is_some() && ids.mal.is_some() {
        return ids;
    }
    let Some(al) = r.anilist_id else { return ids };
    let cache = IDS.get_or_init(|| Mutex::new(HashMap::new()));
    let known = cache.lock().ok().and_then(|m| m.get(&al).copied());
    let mapped = match known {
        Some(m) => Some(m),
        None => match tk_core::metadata::anizip::AniZip::new(http()).ids(r).await {
            Ok(Some(m)) => {
                if let Ok(mut c) = cache.lock() {
                    c.insert(al, m);
                }
                Some(m)
            }
            Ok(None) => None,
            Err(e) => {
                tracing::debug!(error = %e, "ani.zip mapping failed (Stremio lookup uses the AniList id only)");
                None
            }
        },
    };
    if let Some(m) = mapped {
        ids.kitsu = ids.kitsu.or(m.kitsu_id);
        ids.mal = ids.mal.or(m.mal_id);
    }
    ids
}

/// Streams of every enabled add-on for one episode (`Settings::ext.stremio_enabled`). Each add-on tries its
/// `(type, id)` targets in order (the remembered match first) and remembers the first one that answers streams.
pub async fn stremio_candidates(b: &LiveBackend, media: &Media, episode: u32) -> Vec<SourceCandidate> {
    if !crate::settings_store::load(&b.settings_file).ext.stremio_enabled {
        return Vec::new();
    }
    let rows: Vec<StremioRow> = b.store.stremio_addons().unwrap_or_default().into_iter().filter(|r| r.enabled).collect();
    if rows.is_empty() {
        return Vec::new();
    }
    let ids = title_ids(&media.media_ref).await;
    let title_key = super::media_id(&media.media_ref);
    let work = rows.iter().map(|row| {
        let title_key = title_key.clone();
        async move {
            let Ok(m) = stremio::Manifest::parse(&row.manifest_json) else { return Vec::new() };
            let mkey = match_key_of(&row.id);
            let mut matches: BTreeMap<String, String> = b.store.get_setting(&mkey).ok().flatten().unwrap_or_default();
            let remembered = matches.get(&title_key).cloned();
            for (kind, id) in stremio::targets(&m, ids, episode, remembered.as_deref()).into_iter().take(3) {
                let url = stremio::resource_url(&row.manifest_url, "stream", &kind, &id);
                let Ok(url) = reqwest::Url::parse(&url) else { continue };
                match fetch_json(&url, Some(STREMIO_TIMEOUT)).await {
                    Ok(body) => {
                        let list = tk_ext::map_candidates(&format!("stremio:{}", row.id), &m.name, &stremio::normalize_streams(&m.name, &body));
                        if list.is_empty() {
                            continue;
                        }
                        let key = stremio::match_key(&kind, &id);
                        if !title_key.is_empty() && remembered.as_deref() != Some(key.as_str()) {
                            matches.insert(title_key.clone(), key);
                            let _ = b.store.set_setting(&mkey, &matches);
                        }
                        return list;
                    }
                    Err(e) => {
                        tracing::info!(addon = %m.name, detail = %e.detail, "Stremio add-on request failed");
                        if e.kind == ErrorKind::Offline {
                            break;
                        }
                    }
                }
            }
            Vec::new()
        }
    });
    super::ext_server::join_all_futures(work).await.into_iter().flatten().collect()
}

// ---------------------------------------------------------------------------------------------
// Install by URL, repositories, updates
// ---------------------------------------------------------------------------------------------

/// `.kai` package (zip) by its magic bytes.
fn is_zip(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04")
}

/// A Stremio manifest (resources + no `.kai` `type`): the UI hands those to `stremio_add`.
pub(crate) fn is_stremio_manifest(v: &Value) -> bool {
    v.get("resources").is_some_and(Value::is_array) && v.get("permissions").is_none() && v.get("id").is_some()
}

/// Detail of the error `install_extension_url` returns for a Stremio manifest (the UI then adds it as an add-on).
pub const STREMIO_MANIFEST_DETAIL: &str = "stremio_manifest";

/// Where the bundle of a hosted `manifest.json` lives: `bundle` / `main` / `entry` of the manifest (relative to it),
/// else `bundle.js` next to it.
pub(crate) fn bundle_url(manifest_url: &reqwest::Url, manifest: &Value) -> Option<reqwest::Url> {
    let named = ["bundle", "bundleUrl", "main", "entry"].iter().find_map(|k| manifest[*k].as_str()).map(str::trim).filter(|s| !s.is_empty());
    manifest_url.join(named.unwrap_or("bundle.js")).ok()
}

async fn install_from_url(b: &LiveBackend, input: &str, op: &str) -> UiResult<ExtensionVm> {
    let url = checked_url(input)?;
    let bytes = fetch_limited(&url, KAI_CAP, None).await?;
    let vm = if is_zip(&bytes) {
        b.ext.install_bytes(bytes, op).await?
    } else {
        let text = String::from_utf8(bytes).map_err(|_| UiError::new(ErrorKind::Extension, "not a .kai package nor a manifest.json"))?;
        let v: Value = serde_json::from_str(&text).map_err(|_| UiError::new(ErrorKind::Extension, "not a .kai package nor a manifest.json"))?;
        if is_stremio_manifest(&v) {
            return Err(UiError::new(ErrorKind::Extension, STREMIO_MANIFEST_DETAIL));
        }
        let bundle_at = bundle_url(&url, &v).ok_or_else(|| UiError::new(ErrorKind::Extension, "manifest.json: no bundle URL"))?;
        let bundle_at = checked_url(bundle_at.as_str())?;
        let bundle = fetch_limited(&bundle_at, 10 * 1024 * 1024, None).await?;
        let bundle = String::from_utf8(bundle).map_err(|_| UiError::new(ErrorKind::Extension, "bundle.js: not UTF-8"))?;
        b.ext.install_unpacked(text, bundle, op).await?
    };
    let _ = b.store.set_setting(&format!("{ORIGIN_KEY_PREFIX}{}", vm.id), &url.to_string());
    push_preferences(b, &vm.id);
    Ok(vm)
}

/// `Backend::install_extension_url` (`ext.install-url`): a `.kai` or a hosted `manifest.json` (+ its bundle) by
/// `https`. The URL is remembered as the extension's origin for update checks.
pub async fn install_url(b: &LiveBackend, url: &str) -> UiResult<ExtensionVm> {
    install_from_url(b, url, "install").await
}

/// One installable entry of a repository index.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CatalogItem {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub url: String,
    pub kind: String,
    pub notes: Option<String>,
}

fn catalog_kind(v: &str) -> &'static str {
    match v.to_ascii_lowercase().as_str() {
        "manga" | "read" | "comic" => "manga",
        "novel" | "novels" | "lightnovel" => "novel",
        "torrent" | "torrents" => "torrent",
        _ => "anime",
    }
}

/// Parse a repository index: an array of entries, or an object with `name` and `extensions` / `items` / `packages` /
/// `list`. Entry fields: `id`, `name` / `title`, `version`, `description`, `url` / `download` / `downloadUrl` /
/// `package` / `kai` / `file` / `manifest` / `manifestUrl` (relative to the index), `type` / `kind` / `category`,
/// `changelog` / `notes`. Only `.kai` packages and `manifest.json` URLs are kept (ecosystem packages such as APKs
/// install on an extension server, not here). Returns `(repository name, items)`.
pub(crate) fn parse_catalog(text: &str, base: &reqwest::Url) -> Result<(Option<String>, Vec<CatalogItem>), String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("invalid repository index: {e}"))?;
    let (name, list) = if v.is_array() {
        (None, v.clone())
    } else {
        let list = ["extensions", "items", "packages", "list", "data"].iter().map(|k| &v[*k]).find(|x| x.is_array()).cloned();
        (text_of(&v["name"]), list.ok_or_else(|| "repository index: no extension list".to_owned())?)
    };
    let mut out: Vec<CatalogItem> = Vec::new();
    for e in list.as_array().into_iter().flatten() {
        let Some(raw_url) = ["url", "download", "downloadUrl", "package", "kai", "file", "manifest", "manifestUrl"]
            .iter()
            .find_map(|k| text_of(&e[*k]))
        else {
            continue;
        };
        let Ok(url) = base.join(&raw_url) else { continue };
        if url.scheme() != "https" && url.scheme() != "http" {
            continue;
        }
        let path = url.path().to_ascii_lowercase();
        if !(path.ends_with(".kai") || path.ends_with("manifest.json")) {
            continue;
        }
        let Some(name) = text_of(&e["name"]).or_else(|| text_of(&e["title"])) else { continue };
        let id = text_of(&e["id"]).unwrap_or_else(|| name.to_ascii_lowercase().replace(' ', "-"));
        if out.iter().any(|x| x.id == id) {
            continue;
        }
        out.push(CatalogItem {
            id,
            name,
            version: text_of(&e["version"]).unwrap_or_default(),
            description: text_of(&e["description"]).or_else(|| text_of(&e["summary"])).unwrap_or_default(),
            url: url.to_string(),
            kind: catalog_kind(&["type", "kind", "category", "contentType"].iter().find_map(|k| text_of(&e[*k])).unwrap_or_default()).to_owned(),
            notes: text_of(&e["changelog"]).or_else(|| text_of(&e["notes"])).or_else(|| text_of(&e["releaseNotes"])),
        });
    }
    Ok((name, out))
}

/// `latest` is a newer version than `current` (dot-separated numbers, optional `v`, a release beats its pre-release).
pub(crate) fn version_newer(latest: &str, current: &str) -> bool {
    fn split(v: &str) -> (Vec<u64>, bool) {
        let v = v.trim().trim_start_matches(['v', 'V']);
        let (core, pre) = match v.find(['-', '+']) {
            Some(i) => (&v[..i], v[i..].starts_with('-')),
            None => (v, false),
        };
        let nums = core
            .split('.')
            .map(|p| p.chars().take_while(char::is_ascii_digit).collect::<String>().parse::<u64>().unwrap_or(0))
            .collect();
        (nums, pre)
    }
    let (a, a_pre) = split(latest);
    let (b, b_pre) = split(current);
    if latest.trim().is_empty() {
        return false;
    }
    let n = a.len().max(b.len());
    for i in 0..n {
        let (x, y) = (a.get(i).copied().unwrap_or(0), b.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    b_pre && !a_pre
}

fn repo_items(row: &ExtRepoRow) -> Vec<CatalogItem> {
    let Some(text) = row.catalog_json.as_deref() else { return Vec::new() };
    let Ok(base) = reqwest::Url::parse(&row.url) else { return Vec::new() };
    parse_catalog(text, &base).map(|(_, v)| v).unwrap_or_default()
}

fn repo_vm(row: &ExtRepoRow) -> ExtRepoVm {
    let items = row.catalog_json.as_deref().map(|_| repo_items(row).len() as u32).unwrap_or(0);
    let error = row.last_error.clone().or_else(|| (row.catalog_json.is_some() && items == 0).then(|| "no .kai packages in this index".to_owned()));
    ExtRepoVm { id: row.id.clone(), name: row.name.clone(), url: row.url.clone(), items, fetched_unix: row.fetched_at, error }
}

pub async fn repos(b: &LiveBackend) -> UiResult<Vec<ExtRepoVm>> {
    Ok(b.store.ext_repos().map_err(store_err)?.iter().map(repo_vm).collect())
}

async fn fetch_index(url: &reqwest::Url) -> Result<(String, Option<String>, usize), UiError> {
    let bytes = fetch_limited(url, JSON_CAP, None).await?;
    let text = String::from_utf8(bytes).map_err(|_| UiError::new(ErrorKind::Extension, "repository index: not UTF-8"))?;
    let (name, items) = parse_catalog(&text, url).map_err(|e| UiError::new(ErrorKind::Extension, e))?;
    Ok((text, name, items.len()))
}

/// Add a repository by URL: the index is fetched first (a URL that is not an index is refused).
pub async fn repo_add(b: &LiveBackend, url: &str) -> UiResult<ExtRepoVm> {
    let parsed = checked_url(url)?;
    let (text, name, _) = fetch_index(&parsed).await?;
    let url_s = parsed.to_string();
    let existing = b.store.ext_repos().map_err(store_err)?.into_iter().find(|r| r.url == url_s);
    let row = ExtRepoRow {
        id: existing.as_ref().map_or_else(tk_core::extserver::new_id, |r| r.id.clone()),
        name: name.unwrap_or_else(|| parsed.host_str().unwrap_or("repository").to_owned()),
        url: url_s.clone(),
        catalog_json: Some(text.clone()),
        fetched_at: Some(now_unix()),
        last_error: None,
        added_at: existing.as_ref().map_or_else(now_unix, |r| r.added_at),
    };
    b.store.upsert_ext_repo(&row).map_err(store_err)?;
    b.store.set_ext_repo_catalog(&row.id, Some(&text), now_unix(), None).map_err(store_err)?;
    let stored = b.store.ext_repos().map_err(store_err)?.into_iter().find(|r| r.url == url_s).unwrap_or(row);
    Ok(repo_vm(&stored))
}

pub async fn repo_remove(b: &LiveBackend, id: &str) -> UiResult<()> {
    if !b.store.delete_ext_repo(id).map_err(store_err)? {
        return Err(UiError::new(ErrorKind::Other, format!("repository {id} not found")));
    }
    Ok(())
}

pub async fn repo_catalog(b: &LiveBackend, id: &str, refresh: bool) -> UiResult<Vec<ExtCatalogItemVm>> {
    let mut row = b
        .store
        .ext_repos()
        .map_err(store_err)?
        .into_iter()
        .find(|r| r.id == id)
        .ok_or_else(|| UiError::new(ErrorKind::Other, format!("repository {id} not found")))?;
    let stale = row.fetched_at.is_none_or(|t| now_unix() - t > REPO_TTL_SECS);
    if refresh || stale || row.catalog_json.is_none() {
        let url = checked_url(&row.url)?;
        match fetch_index(&url).await {
            Ok((text, _, _)) => {
                b.store.set_ext_repo_catalog(&row.id, Some(&text), now_unix(), None).map_err(store_err)?;
                row.catalog_json = Some(text);
                row.last_error = None;
            }
            Err(e) => {
                let _ = b.store.set_ext_repo_catalog(&row.id, None, now_unix(), Some(&e.detail));
                if row.catalog_json.is_none() {
                    return Err(e);
                }
            }
        }
    }
    let installed: HashMap<String, String> = b.ext.installed().into_iter().map(|e| (e.id, e.version)).collect();
    Ok(repo_items(&row)
        .into_iter()
        .map(|i| ExtCatalogItemVm {
            installed: installed.contains_key(&i.id),
            installed_version: installed.get(&i.id).cloned(),
            id: i.id,
            name: i.name,
            version: i.version,
            description: i.description,
            url: i.url,
            kind: i.kind,
        })
        .collect())
}

/// What one update source says: `(version, package url, notes)`.
type Offer = (String, String, Option<String>);

/// Version of the package behind a `.kai` / `manifest.json` URL.
async fn offer_from_package(url: &reqwest::Url) -> Option<Offer> {
    if url.path().to_ascii_lowercase().ends_with(".kai") {
        let bytes = fetch_limited(url, KAI_CAP, None).await.ok()?;
        let pkg = tk_ext::package::read_kai_bytes(&bytes).ok()?;
        return Some((pkg.manifest.version, url.to_string(), None));
    }
    let v = fetch_json(url, None).await.ok()?;
    Some((text_of(&v["version"])?, url.to_string(), text_of(&v["changelog"]).or_else(|| text_of(&v["notes"]))))
}

/// Manifest `updateUrl`: a JSON `{ version, url, notes? }` (relative `url` allowed) or a package URL.
async fn offer_from_feed(feed: &str) -> Option<Offer> {
    let url = checked_url(feed).ok()?;
    let path = url.path().to_ascii_lowercase();
    if path.ends_with(".kai") || path.ends_with("manifest.json") {
        return offer_from_package(&url).await;
    }
    let v = fetch_json(&url, None).await.ok()?;
    let version = text_of(&v["version"])?;
    let pkg = text_of(&v["url"]).or_else(|| text_of(&v["download"])).and_then(|u| url.join(&u).ok())?;
    Some((version, pkg.to_string(), text_of(&v["notes"]).or_else(|| text_of(&v["changelog"]))))
}

/// The newest offer for one installed extension: repository catalogues, its update feed, its install URL.
async fn find_update(b: &LiveBackend, info: &tk_core::InstalledExtension, catalogs: &[CatalogItem]) -> Option<Offer> {
    fn consider(best: &mut Option<Offer>, o: Offer) {
        if best.as_ref().is_none_or(|b| version_newer(&o.0, &b.0)) {
            *best = Some(o);
        }
    }
    let mut best: Option<Offer> = None;
    for i in catalogs.iter().filter(|i| i.id == info.id && !i.version.is_empty()) {
        let notes = i.notes.clone().or_else(|| Some(i.description.clone()).filter(|d| !d.is_empty()));
        consider(&mut best, (i.version.clone(), i.url.clone(), notes));
    }
    if let Some(feed) = b.ext.host().manifest(&info.id).and_then(|m| m.update_url)
        && let Some(o) = offer_from_feed(&feed).await
    {
        consider(&mut best, o);
    }
    if best.is_none()
        && let Ok(Some(origin)) = b.store.get_setting::<String>(&format!("{ORIGIN_KEY_PREFIX}{}", info.id))
        && let Ok(url) = checked_url(&origin)
        && let Some(o) = offer_from_package(&url).await
    {
        consider(&mut best, o);
    }
    best
}

fn update_vm(b: &LiveBackend, row: &ExtUpdateRow) -> Option<ExtUpdateVm> {
    let latest = row.latest.clone().filter(|l| version_newer(l, &row.current))?;
    Some(ExtUpdateVm { id: row.ext_id.clone(), name: ext_name(b, &row.ext_id), current: row.current.clone(), latest, url: row.url.clone(), notes: row.notes.clone() })
}

/// Compare installed extensions with their origin (`ext.updates.check`), at most every
/// `Settings::ext.check_interval_hours` (the rail badge polls hourly; rows live in `ext_updates`).
pub async fn updates_check(b: &LiveBackend) -> UiResult<Vec<ExtUpdateVm>> {
    let interval = i64::from(crate::settings_store::load(&b.settings_file).ext.check_interval_hours.max(1)) * 3600;
    let installed = b.ext.installed();
    let rows = b.store.ext_updates().map_err(store_err)?;
    let now = now_unix();
    let fresh = installed.iter().all(|e| rows.iter().any(|r| r.ext_id == e.id && r.current == e.version && now - r.checked_at < interval));
    if fresh {
        return Ok(rows.iter().filter(|r| installed.iter().any(|e| e.id == r.ext_id)).filter_map(|r| update_vm(b, r)).collect());
    }
    let catalogs: Vec<CatalogItem> = b.store.ext_repos().unwrap_or_default().iter().flat_map(repo_items).collect();
    let offers = super::ext_server::join_all_futures(installed.iter().map(|e| find_update(b, e, &catalogs))).await;
    for r in &rows {
        if !installed.iter().any(|e| e.id == r.ext_id) {
            let _ = b.store.clear_ext_update(&r.ext_id);
        }
    }
    let mut out = Vec::new();
    for (e, offer) in installed.iter().zip(offers) {
        let row = ExtUpdateRow {
            ext_id: e.id.clone(),
            current: e.version.clone(),
            latest: offer.as_ref().map(|o| o.0.clone()),
            url: offer.as_ref().map(|o| o.1.clone()),
            notes: offer.and_then(|o| o.2),
            checked_at: now,
        };
        b.store.set_ext_update(&row).map_err(store_err)?;
        out.extend(update_vm(b, &row));
    }
    Ok(out)
}

/// Install the newer package of one extension, keeping its preferences (tk-ext) and its enabled state.
pub async fn update(b: &LiveBackend, id: &str) -> UiResult<ExtensionVm> {
    let before = b.ext.host().get(id).ok_or_else(|| not_installed(id))?;
    let row = b
        .store
        .ext_updates()
        .map_err(store_err)?
        .into_iter()
        .find(|r| r.ext_id == id)
        .filter(|r| r.latest.as_deref().is_some_and(|l| version_newer(l, &r.current)))
        .ok_or_else(|| UiError::new(ErrorKind::Other, format!("{} is up to date", before.name)))?;
    let url = row.url.clone().ok_or_else(|| UiError::new(ErrorKind::Extension, format!("{}: no package URL", before.name)))?;
    let origin = b.store.get_setting::<String>(&format!("{ORIGIN_KEY_PREFIX}{id}")).ok().flatten();
    let mut vm = install_from_url(b, &url, "update").await?;
    if vm.id != id {
        return Err(UiError::new(ErrorKind::Extension, format!("the package at {url} is {}, not {id}", vm.id)));
    }
    // Keep the original install URL as origin when the update came from a repository.
    if let Some(o) = origin {
        let _ = b.store.set_setting(&format!("{ORIGIN_KEY_PREFIX}{id}"), &o);
    }
    if vm.enabled != before.enabled && vm.compat.can_enable() {
        b.ext.set_enabled(id, before.enabled).await?;
        vm.enabled = before.enabled;
    }
    let _ = b.store.set_ext_update(&ExtUpdateRow { current: vm.version.clone(), latest: None, url: None, notes: None, checked_at: now_unix(), ..row });
    Ok(vm)
}

/// Update everything with a known newer version; returns how many were updated (the first failure is returned when
/// nothing could be updated).
pub async fn update_all(b: &LiveBackend) -> UiResult<u32> {
    let pending = updates_check(b).await?;
    let mut done = 0u32;
    let mut first_err: Option<UiError> = None;
    for u in &pending {
        match update(b, &u.id).await {
            Ok(_) => done += 1,
            Err(e) => {
                b.ext.log(&u.id, false, "update", e.detail.clone());
                if first_err.is_none() {
                    first_err = Some(e);
                }
            }
        }
    }
    match first_err {
        Some(e) if done == 0 => Err(e),
        _ => Ok(done),
    }
}

// ---------------------------------------------------------------------------------------------
// Kill switch
// ---------------------------------------------------------------------------------------------

/// Which ids "Enable all" turns on: the remembered list (still installed and runnable) or, without one, every
/// runnable extension.
pub(crate) fn kill_switch_targets(installed: &[(String, bool, bool)], previous: Option<&[String]>) -> Vec<String> {
    // (id, enabled, runnable)
    match previous.filter(|p| !p.is_empty()) {
        Some(prev) => installed.iter().filter(|(id, on, ok)| *ok && !*on && prev.contains(id)).map(|(id, _, _)| id.clone()).collect(),
        None => installed.iter().filter(|(_, on, ok)| *ok && !*on).map(|(id, _, _)| id.clone()).collect(),
    }
}

/// "Disable all" / "Enable all" (`ext.kill-switch`); returns how many changed.
pub async fn set_all_enabled(b: &LiveBackend, enabled: bool) -> UiResult<u32> {
    let installed: Vec<(String, bool, bool)> =
        b.ext.installed().into_iter().map(|e| (e.id, e.enabled, e.compat != Compat::Unsupported)).collect();
    let mut changed = 0u32;
    watch_cache().clear();
    home_cache().clear();
    if !enabled {
        let on: Vec<String> = installed.iter().filter(|(_, on, _)| *on).map(|(id, _, _)| id.clone()).collect();
        if on.is_empty() {
            return Ok(0);
        }
        b.store.set_setting(KILL_SWITCH_KEY, &on).map_err(store_err)?;
        for id in &on {
            b.ext.set_enabled(id, false).await?;
            b.ext.log(id, true, "kill-switch", "disabled with \"Disable all\"");
            changed += 1;
        }
        return Ok(changed);
    }
    let previous: Option<Vec<String>> = b.store.get_setting(KILL_SWITCH_KEY).ok().flatten();
    for id in kill_switch_targets(&installed, previous.as_deref()) {
        b.ext.set_enabled(&id, true).await?;
        b.ext.log(&id, true, "kill-switch", "enabled with \"Enable all\"");
        changed += 1;
    }
    let _ = b.store.delete_setting(KILL_SWITCH_KEY);
    Ok(changed)
}

// ---------------------------------------------------------------------------------------------
// Preferences
// ---------------------------------------------------------------------------------------------

fn pref_kind(k: PrefKind) -> ExtPrefKindVm {
    match k {
        PrefKind::Text => ExtPrefKindVm::Text,
        PrefKind::Secret => ExtPrefKindVm::Secret,
        PrefKind::Toggle => ExtPrefKindVm::Toggle,
        PrefKind::Select => ExtPrefKindVm::Select,
    }
}

/// Declared fields + stored values -> the map the bundle sees (defaults filled in; undeclared keys dropped).
pub(crate) fn merged_prefs(fields: &[PrefField], stored: &[(String, String)]) -> BTreeMap<String, String> {
    fields
        .iter()
        .map(|f| {
            let v = stored.iter().find(|(k, _)| *k == f.key).map(|(_, v)| v.clone()).unwrap_or_else(|| f.default.clone());
            (f.key.clone(), v)
        })
        .collect()
}

/// A value for a field, normalized; `None` when it does not fit the field.
pub(crate) fn normalize_pref(field: &PrefField, value: &str) -> Option<String> {
    match field.kind {
        PrefKind::Toggle => match value.trim() {
            "true" | "1" | "on" => Some("true".into()),
            "false" | "0" | "off" | "" => Some("false".into()),
            _ => None,
        },
        PrefKind::Select => (value.is_empty() || field.options.iter().any(|(v, _)| v == value)).then(|| value.to_owned()),
        PrefKind::Text | PrefKind::Secret => (value.len() <= 4096).then(|| value.to_owned()),
    }
}

/// Push the stored preferences of one extension into the host (the next call sees them).
pub(crate) fn push_preferences(b: &LiveBackend, id: &str) {
    let host = b.ext.host();
    let Some(m) = host.manifest(id) else { return };
    let stored = b.store.ext_prefs(id).unwrap_or_default();
    if let Err(e) = host.set_preferences(id, merged_prefs(&m.preference_fields(), &stored)) {
        tracing::debug!(extension = %id, error = %e, "preferences not applied");
    }
}

/// Start-up: every installed extension gets its stored preferences.
pub(crate) fn push_all_preferences(b: &LiveBackend) {
    for e in b.ext.installed() {
        push_preferences(b, &e.id);
    }
}

pub async fn preferences(b: &LiveBackend, id: &str) -> UiResult<Vec<ExtPrefFieldVm>> {
    let m = b.ext.host().manifest(id).ok_or_else(|| not_installed(id))?;
    let fields = m.preference_fields();
    let stored = b.store.ext_prefs(id).map_err(store_err)?;
    let values = merged_prefs(&fields, &stored);
    Ok(fields
        .into_iter()
        .map(|f| ExtPrefFieldVm {
            value: values.get(&f.key).cloned().unwrap_or_default(),
            kind: pref_kind(f.kind),
            key: f.key,
            label: f.label,
            hint: f.hint,
            options: f.options,
        })
        .collect())
}

pub async fn set_preference(b: &LiveBackend, id: &str, key: &str, value: &str) -> UiResult<()> {
    let m = b.ext.host().manifest(id).ok_or_else(|| not_installed(id))?;
    let field = m
        .preference_fields()
        .into_iter()
        .find(|f| f.key == key)
        .ok_or_else(|| UiError::new(ErrorKind::Extension, format!("{id} declares no preference {key}")))?;
    let value = normalize_pref(&field, value).ok_or_else(|| UiError::new(ErrorKind::Other, format!("invalid value for {key}")))?;
    b.store.set_ext_pref(id, key, &value, now_unix()).map_err(store_err)?;
    push_preferences(b, id);
    watch_cache().clear();
    home_cache().clear();
    // The key only: values may be secrets.
    b.ext.log(id, true, "preferences", format!("{key} updated"));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_ids_have_the_ext_prefix() {
        assert!(is_provider_id("ext:demo:src:42"));
        assert!(!is_provider_id("21"));
        assert!(!is_provider_id("mal:1"));
        let r = parse_provider_id("ext:demo:src:a:b").unwrap();
        assert_eq!((r.ext_id.as_str(), r.source_id.as_str(), r.item_id.as_str()), ("demo", "src", "a:b"));
        assert_eq!(r.id(), "ext:demo:src:a:b");
        assert!(parse_provider_id("ext:demo:src").is_none());
        assert!(parse_provider_id("ext::src:1").is_none());
        assert_eq!(provider_ref("demo", "src:9").unwrap().item_id, "9");
        assert!(is_remote_ext_id("server:abc") && is_remote_ext_id("stremio:x") && !is_remote_ext_id("toko"));
    }

    #[test]
    fn info_entries_keep_distinct_numbers_or_fall_back_to_positions() {
        let body = json!({ "title": "Show", "image": "https://x/i.jpg", "meta": { "Status": "Ongoing", "Genres": "Action, Drama" },
            "entries": [ { "id": "e2", "label": "Episode 2", "number": 2 }, { "id": "e1", "label": "Episode 1", "number": 1 } ] });
        let i = parse_info(&body).unwrap();
        assert_eq!(i.entries.iter().map(|e| (e.id.as_str(), e.number)).collect::<Vec<_>>(), vec![("e1", 1), ("e2", 2)]);
        assert_eq!(meta_value(&i.meta, &["status"]).as_deref(), Some("Ongoing"));
        let dup = json!({ "title": "S", "entries": [ { "id": "a", "number": 1 }, { "id": "b", "number": 1 }, { "id": "c", "number": 1.5 } ] });
        let i = parse_info(&dup).unwrap();
        assert_eq!(i.entries.iter().map(|e| e.number).collect::<Vec<_>>(), vec![1, 2, 3]);
        assert_eq!(i.entries[2].raw_number, Some(1.5));
        assert_eq!(i.entries[0].label, "a");
        assert!(parse_info(&json!({ "entries": [] })).is_none());
    }

    #[test]
    fn watch_sources_get_a_type() {
        let body = json!({ "sources": [
            { "url": "https://cdn/x/master.m3u8?t=1", "quality": "1080p" },
            { "url": "https://cdn/x/v.mp4" },
            { "url": "https://player.example/embed/1" },
            { "url": "https://cdn/y", "sourceType": "HLS", "source": "Mirror" }
        ] });
        let n = normalize_watch("Ext", &body);
        let types: Vec<&str> = n["sources"].as_array().unwrap().iter().map(|s| s["type"].as_str().unwrap()).collect();
        assert_eq!(types, vec!["hls", "mp4", "embed", "hls"]);
        assert_eq!(n["sources"][0]["providerName"], "Ext");
        assert_eq!(n["sources"][3]["providerName"], "Mirror");
        let c = tk_ext::map_candidates("ext.a", "Ext", &n);
        assert!(c.iter().any(|c| c.web.as_ref().is_some_and(|w| w.embed)));
        assert!(c.iter().filter(|c| c.to_stream().is_some()).count() >= 2);
    }

    #[test]
    fn provider_history_rows_are_local_only() {
        let at = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        let e = provider_history("ext:a:s:1", "Show", None, 3, 95, Some(100), at);
        assert_eq!((e.anime_id.as_str(), e.episode_id.as_str(), e.completed), ("ext:a:s:1", "ext:a:s:1?ep=3", true));
        assert_eq!((e.anilist_id, e.mal_id), (None, None));
    }

    #[test]
    fn provider_chapter_ids_round_trip() {
        let id = provider_chapter_id("src", "item/1:2", "ch.5");
        assert!(id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'.'));
        assert_eq!(parse_provider_chapter(&id), Some(("src".into(), "item/1:2".into(), "ch.5".into())));
        assert!(parse_provider_chapter("chapter-1").is_none());
        assert!(parse_provider_chapter("custom.zz.00.00").is_none());
        assert!(is_provider_chapter(&ChapterRefVm { ext_id: "e".into(), chapter_id: id }));
    }

    #[test]
    fn pages_are_ordered_and_reindexed() {
        let body = json!({ "pages": [ { "pageNumber": 2, "imageUrl": "https://i/2.jpg" }, { "pageNumber": 1, "imageUrl": "https://i/1.jpg", "headers": { "Referer": "https://r" } }, { "imageUrl": "file:///x" } ] });
        let p = parse_pages(&body);
        assert_eq!(p.len(), 2);
        assert_eq!((p[0].index, p[0].url.as_str()), (0, "https://i/1.jpg"));
        assert_eq!(p[0].headers, vec![("Referer".to_owned(), "https://r".to_owned())]);
    }

    #[test]
    fn home_sections_map_to_cards() {
        let body = json!({ "sections": [ { "title": "Latest", "items": [ { "id": "7", "title": "A", "image": "https://i/a.jpg" } ] }, { "items": [] } ] });
        let s = parse_home("ext.a", "src", &body);
        assert_eq!(s.len(), 1);
        assert_eq!((s[0].0.as_str(), s[0].1[0].id.as_str()), ("Latest", "ext:ext.a:src:7"));
    }

    #[test]
    fn urls_must_be_https_or_lan() {
        assert!(checked_url("https://example.org/x.kai").is_ok());
        assert!(checked_url("http://192.168.1.4:8000/x.kai").is_ok());
        assert!(checked_url("http://example.org/x.kai").is_err());
        assert!(checked_url("file:///etc/passwd").is_err());
        assert!(checked_url("nonsense").is_err());
    }

    #[test]
    fn repository_indexes_keep_kai_packages() {
        let base = reqwest::Url::parse("https://repo.example/tatakai/index.json").unwrap();
        let text = r#"{ "name": "My repo", "extensions": [
            { "id": "toko", "name": "Toko", "version": "1.2.0", "url": "toko.kai", "type": "anime", "changelog": "Faster" },
            { "name": "Hosted", "version": "0.1", "manifest": "https://cdn.example/hosted/manifest.json", "kind": "manga" },
            { "name": "Aniyomi thing", "apk": "x.apk", "url": "apk/x.apk" },
            { "id": "toko", "name": "Duplicate", "url": "toko2.kai" }
        ] }"#;
        let (name, items) = parse_catalog(text, &base).unwrap();
        assert_eq!(name.as_deref(), Some("My repo"));
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].url, "https://repo.example/tatakai/toko.kai");
        assert_eq!((items[0].kind.as_str(), items[0].notes.as_deref()), ("anime", Some("Faster")));
        assert_eq!((items[1].id.as_str(), items[1].kind.as_str()), ("hosted", "manga"));
        assert_eq!(parse_catalog(r#"[{"id":"a","name":"A","url":"https://x/a.kai"}]"#, &base).unwrap().1.len(), 1);
        assert!(parse_catalog("{}", &base).is_err());
        assert!(parse_catalog("nope", &base).is_err());
    }

    #[test]
    fn versions_compare_numerically() {
        assert!(version_newer("1.10.0", "1.9.3"));
        assert!(version_newer("v2", "1.99"));
        assert!(version_newer("1.0.0", "1.0.0-beta.2"));
        assert!(!version_newer("1.0.0-beta", "1.0.0"));
        assert!(!version_newer("1.0", "1.0.0"));
        assert!(!version_newer("", "1.0"));
        assert!(!version_newer("0.9", "1.0"));
    }

    #[test]
    fn kill_switch_restores_only_the_previous_set() {
        let installed = vec![("a".to_owned(), false, true), ("b".to_owned(), false, true), ("c".to_owned(), false, false)];
        assert_eq!(kill_switch_targets(&installed, Some(&["a".to_owned(), "c".to_owned(), "gone".to_owned()][..])), vec!["a"]);
        assert_eq!(kill_switch_targets(&installed, None), vec!["a", "b"]);
        assert_eq!(kill_switch_targets(&installed, Some(&[][..])), vec!["a", "b"]);
    }

    #[test]
    fn preferences_merge_defaults_and_validate_values() {
        let m = tk_ext::Manifest::parse(
            r#"{"id":"p","name":"P","version":"1","type":"onlinestream","description":"","permissions":[],
               "preferences":[{"key":"dub","type":"toggle","default":true},{"key":"q","type":"select","options":["1080","720"],"default":"1080"},{"key":"token","type":"secret"}]}"#,
        )
        .unwrap();
        let f = m.preference_fields();
        let merged = merged_prefs(&f, &[("q".into(), "720".into()), ("stale".into(), "x".into())]);
        assert_eq!(merged.get("dub").map(String::as_str), Some("true"));
        assert_eq!(merged.get("q").map(String::as_str), Some("720"));
        assert!(!merged.contains_key("stale"));
        assert_eq!(normalize_pref(&f[0], "0").as_deref(), Some("false"));
        assert_eq!(normalize_pref(&f[0], "maybe"), None);
        assert_eq!(normalize_pref(&f[1], "480"), None);
        assert_eq!(normalize_pref(&f[2], "s3cr3t").as_deref(), Some("s3cr3t"));
        assert_eq!(pref_kind(f[2].kind), ExtPrefKindVm::Secret);
    }

    #[test]
    fn install_url_helpers() {
        assert!(is_zip(b"PK\x03\x04rest"));
        assert!(!is_zip(b"{\"id\":1}"));
        assert!(is_stremio_manifest(&json!({ "id": "org.x", "resources": ["stream"], "types": ["anime"] })));
        assert!(!is_stremio_manifest(&json!({ "id": "x", "resources": [], "permissions": [] })));
        let m = reqwest::Url::parse("https://cdn.example/ext/manifest.json").unwrap();
        assert_eq!(bundle_url(&m, &json!({})).unwrap().as_str(), "https://cdn.example/ext/bundle.js");
        assert_eq!(bundle_url(&m, &json!({ "main": "dist/index.js" })).unwrap().as_str(), "https://cdn.example/ext/dist/index.js");
    }
}
