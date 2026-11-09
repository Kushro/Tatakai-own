//! `Backend` debrid calls (wave A, worker **i**; docs/redesign/15 section 4; checklist `sources.debrid.*`).
//!
//! Keys live in `b.secrets` under `tk_core::secrets::keys::debrid(<service id>)`, never in a settings file. The
//! services are tried in `DebridServiceVm::ALL` order (Real-Debrid, TorBox, AllDebrid, Premiumize): the first
//! connected one that has a torrent cached plays it.
//!
//! * `debrid_accounts` / `set_debrid_key`: validate the key against the service (`DebridClient::account`); a rejected
//!   key is not saved and comes back as `DebridAccountVm::error`. Account answers are cached for `ACCOUNT_TTL`.
//! * `mark_cached` (called by `LiveBackend::sources` on the ranked list): every torrent row's info-hash is checked on
//!   every connected service. Answers are cached in `debrid_cache` for `CACHE_TTL_SECS`; Real-Debrid and AllDebrid
//!   only answer by adding the magnet (see `tk_core::debrid`), so at most `PROBES_SLOW` new hashes per list are asked
//!   there (`PROBES_FAST` on TorBox / Premiumize), all within `CHECK_BUDGET`.
//! * **Ranking** (`sources.debrid.ranking`): a cached row is ranked like a web source. Its score loses the torrent's
//!   seeder terms of `tk_core::sources::score` (`+ min(seeders, 500) / 50`, `- 20` under 20 seeders) and gains
//!   `CACHED_BONUS` (+15): `score' = score - min(s, 500) / 50 + (20 if s < 20) + 15`. The list is re-sorted (stable)
//!   and `recommended` moves to the new first row.
//! * **Manual torrents**: a torrent the user aligned by hand for this title and episode (Torrents > Library "Add
//!   torrent", Files "Play this file"; `torrent_episode_map`) is put first in the list as `manual|torrent|<hash>`
//!   (`torrent_play::manual_source_id`). `mark_cached` does not get the title and episode, so they are taken from the
//!   newest entry of `LiveBackend::candidates` whose sources are exactly this list (the `sources()` call that is
//!   running right now).
//! * `debrid_play`: resolves the episode file of the torrent behind a source id to a direct `https://` link and
//!   returns it like a web stream (`StreamVm::provider` = the service name).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use tk_core::debrid::{Account, DebridClient, Service, magnet_info_hash};
use tk_core::secrets::keys;
use tk_core::sources::{SourceKind, parse_source_key, source_key};
use tk_core::DebridError;
use tk_ui::backend::{
    DebridAccountVm, DebridServiceVm, DebridStatusVm, PlaybackVm, ReleaseTagsVm, SourceKindVm, SourceVm, StreamVm, UiResult,
};
use tk_ui::model::{ErrorKind, UiError};

use super::torrent_play::{MANUAL_EXT, find_candidate, manual_source_id, titles};
use crate::live::LiveBackend;

/// How long a cache answer of a service is trusted (store `debrid_cache`).
const CACHE_TTL_SECS: i64 = 10 * 60;
/// How long an account answer (`debrid_accounts`) is reused.
const ACCOUNT_TTL: Duration = Duration::from_secs(10 * 60);
/// New hashes asked per list on services whose check adds the magnet (Real-Debrid, AllDebrid).
const PROBES_SLOW: usize = 3;
/// New hashes asked per list on services with a side-effect-free batch check (TorBox, Premiumize).
const PROBES_FAST: usize = 40;
/// Longest the source list waits for the cache checks.
const CHECK_BUDGET: Duration = Duration::from_secs(8);
/// Score bonus of a cached row (see the module docs).
pub const CACHED_BONUS: f32 = 15.0;

fn client() -> &'static DebridClient {
    static CLIENT: OnceLock<DebridClient> = OnceLock::new();
    CLIENT.get_or_init(|| {
        let http = reqwest::Client::builder()
            .user_agent("TatakaiNext/0.1")
            .timeout(Duration::from_secs(20))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        DebridClient::new(http)
    })
}

fn accounts_cache() -> &'static Mutex<HashMap<DebridServiceVm, (Instant, DebridAccountVm)>> {
    static CACHE: OnceLock<Mutex<HashMap<DebridServiceVm, (Instant, DebridAccountVm)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn service_of(vm: DebridServiceVm) -> Service {
    match vm {
        DebridServiceVm::RealDebrid => Service::RealDebrid,
        DebridServiceVm::TorBox => Service::TorBox,
        DebridServiceVm::AllDebrid => Service::AllDebrid,
        DebridServiceVm::Premiumize => Service::Premiumize,
    }
}

fn now_unix() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

/// The saved key of a service (`None` when missing, empty or unreadable).
fn key_of(b: &LiveBackend, service: DebridServiceVm) -> Option<String> {
    match b.secrets.get(&keys::debrid(service.id())) {
        Ok(v) => v.map(|k| k.trim().to_owned()).filter(|k| !k.is_empty()),
        Err(e) => {
            tracing::warn!(service = service.id(), error = %e, "reading a debrid key failed");
            None
        }
    }
}

/// Services with a saved key, in preference order.
fn connected(b: &LiveBackend) -> Vec<(DebridServiceVm, String)> {
    DebridServiceVm::ALL.into_iter().filter_map(|s| key_of(b, s).map(|k| (s, k))).collect()
}

fn error_text(e: &DebridError) -> String {
    match e {
        DebridError::Unauthorized => "the service rejected this API key".to_owned(),
        other => other.to_string(),
    }
}

fn account_vm(service: DebridServiceVm, account: Result<Account, DebridError>) -> DebridAccountVm {
    match account {
        Ok(a) => DebridAccountVm { service, connected: true, username: a.username, premium_until_unix: a.premium_until, error: None },
        Err(e) => DebridAccountVm { service, connected: false, username: None, premium_until_unix: None, error: Some(error_text(&e)) },
    }
}

fn disconnected(service: DebridServiceVm) -> DebridAccountVm {
    DebridAccountVm { service, connected: false, username: None, premium_until_unix: None, error: None }
}

async fn account_for(b: &LiveBackend, service: DebridServiceVm) -> DebridAccountVm {
    let Some(key) = key_of(b, service) else { return disconnected(service) };
    if let Some(hit) = accounts_cache()
        .lock()
        .ok()
        .and_then(|c| c.get(&service).filter(|(at, _)| at.elapsed() < ACCOUNT_TTL).map(|(_, vm)| vm.clone()))
    {
        return hit;
    }
    let vm = account_vm(service, client().account(service_of(service), &key).await);
    // A network failure is not cached (the next look retries).
    let transient = vm.error.is_some() && !vm.error.as_deref().is_some_and(|e| e.contains("rejected"));
    if !transient && let Ok(mut c) = accounts_cache().lock() {
        c.insert(service, (Instant::now(), vm.clone()));
    }
    vm
}

/// `Backend::debrid_accounts`: the four services in preference order.
pub async fn debrid_accounts(b: &LiveBackend) -> UiResult<Vec<DebridAccountVm>> {
    let [s0, s1, s2, s3] = DebridServiceVm::ALL;
    let (a, c, d, e) = tokio::join!(account_for(b, s0), account_for(b, s1), account_for(b, s2), account_for(b, s3));
    Ok(vec![a, c, d, e])
}

/// `Backend::set_debrid_key`: `Some` validates and saves (a rejected key is not saved: `error` says why); `None`
/// removes the key. A network failure is an error (`Offline`), so the UI can retry.
pub async fn set_debrid_key(b: &LiveBackend, service: DebridServiceVm, key: Option<&str>) -> UiResult<DebridAccountVm> {
    let secret = keys::debrid(service.id());
    if let Ok(mut c) = accounts_cache().lock() {
        c.remove(&service);
    }
    let Some(key) = key.map(str::trim).filter(|k| !k.is_empty()) else {
        b.secrets.delete(&secret).map_err(|e| UiError::new(ErrorKind::Other, e.to_string()))?;
        return Ok(disconnected(service));
    };
    match client().account(service_of(service), key).await {
        Ok(account) => {
            b.secrets.set(&secret, key).map_err(|e| UiError::new(ErrorKind::Other, e.to_string()))?;
            let vm = account_vm(service, Ok(account));
            if let Ok(mut c) = accounts_cache().lock() {
                c.insert(service, (Instant::now(), vm.clone()));
            }
            tracing::info!(service = service.id(), "debrid key saved");
            Ok(vm)
        }
        Err(e @ (DebridError::Unauthorized | DebridError::NoKey(_) | DebridError::Http(400..=499))) => Ok(account_vm(service, Err(e))),
        Err(e @ DebridError::Network(_)) => Err(UiError::new(ErrorKind::Offline, e.to_string())),
        Err(e) => Err(UiError::new(ErrorKind::Other, e.to_string())),
    }
}

/// `Backend::debrid_play`: the torrent behind `source` (a torrent `SourceVm::id`, a manual `manual|torrent|<hash>`
/// row or a magnet) resolved on the first connected service that has it cached.
pub async fn debrid_play(b: &LiveBackend, source: &str, id: &str, episode: u32) -> UiResult<PlaybackVm> {
    let (r, media) = b.media(id).await?;
    let (magnet, quality) = magnet_for(b, source, id, episode, &media).await?;
    let hash = magnet_info_hash(&magnet).ok_or_else(|| UiError::new(ErrorKind::NoSource, "magnet without an info-hash"))?;
    let services = connected(b);
    if services.is_empty() {
        return Err(UiError::new(ErrorKind::NoSource, "no debrid service is connected"));
    }
    let names = titles(&media);
    let mut last: Option<UiError> = None;
    for (service, key) in services {
        let known = b.store.debrid_cached(service.id(), &hash, now_unix(), CACHE_TTL_SECS).ok().flatten();
        if known == Some(false) {
            continue;
        }
        match client().resolve(service_of(service), &key, &magnet, episode, &names).await {
            Ok(resolved) => {
                let _ = b.store.set_debrid_cached(service.id(), &hash, true, now_unix());
                tracing::info!(service = service.id(), file = %resolved.file_name, "debrid stream ready");
                return Ok(PlaybackVm {
                    title: media.title.clone(),
                    poster: media.cover.clone(),
                    episodes_total: media.episodes,
                    start_secs: b.start_secs(&r, episode),
                    streams: vec![StreamVm {
                        url: resolved.url,
                        headers: Vec::new(),
                        provider: service.label().to_owned(),
                        quality: quality.clone(),
                        subtitles: Vec::new(),
                    }],
                });
            }
            Err(e) => {
                if matches!(e, DebridError::NotCached) {
                    let _ = b.store.set_debrid_cached(service.id(), &hash, false, now_unix());
                }
                tracing::warn!(service = service.id(), error = %e, "debrid resolve failed");
                let kind = match e {
                    DebridError::Network(_) => ErrorKind::Offline,
                    _ => ErrorKind::NoSource,
                };
                last = Some(UiError::new(kind, format!("{}: {e}", service.label())));
            }
        }
    }
    Err(last.unwrap_or_else(|| UiError::new(ErrorKind::NoSource, "not cached on any connected debrid service")))
}

/// Magnet (and quality label) of the torrent behind `source`.
async fn magnet_for(b: &LiveBackend, source: &str, id: &str, episode: u32, media: &tk_core::Media) -> UiResult<(String, Option<String>)> {
    if source.trim().to_ascii_lowercase().starts_with("magnet:?") {
        return Ok((source.trim().to_owned(), None));
    }
    let Some((ext, kind, label)) = parse_source_key(source) else {
        return Err(UiError::new(ErrorKind::Other, format!("not a torrent source: {source}")));
    };
    if kind != SourceKind::Torrent {
        return Err(UiError::new(ErrorKind::Other, format!("{source} is not a torrent source")));
    }
    if ext == MANUAL_EXT {
        return Ok((format!("magnet:?xt=urn:btih:{}", label.to_ascii_lowercase()), None));
    }
    let candidate = find_candidate(b, source, id, Some(episode), media).await?;
    let t = candidate.torrent.as_ref().ok_or_else(|| UiError::new(ErrorKind::NoSource, source.to_owned()))?;
    let magnet = t
        .magnet
        .clone()
        .filter(|m| m.to_ascii_lowercase().starts_with("magnet:?"))
        .unwrap_or_else(|| format!("magnet:?xt=urn:btih:{}", t.info_hash.to_ascii_lowercase()));
    let quality = Some(candidate.quality.label().to_owned()).filter(|q| !q.is_empty());
    Ok((magnet, quality))
}

/// Fill `SourceVm::debrid` of the torrent rows from the cache check of every connected service (best effort: on
/// any error the rows stay `None`), put the manual torrents of this title / episode first, and re-rank (module docs).
pub async fn mark_cached(b: &LiveBackend, sources: Vec<SourceVm>) -> Vec<SourceVm> {
    let mut sources = with_manual_rows(b, sources);
    let services = connected(b);
    if services.is_empty() {
        return sources;
    }
    let hashes: Vec<String> = sources
        .iter()
        .filter(|s| s.kind == SourceKindVm::Torrent)
        .filter_map(|s| s.info_hash.as_deref())
        .map(str::to_ascii_lowercase)
        .filter(|h| tk_core::debrid::is_hex_hash(h))
        .collect();
    if hashes.is_empty() {
        return sources;
    }
    let now = now_unix();
    let mut answers: HashMap<(DebridServiceVm, String), bool> = HashMap::new();
    let mut to_ask: Vec<(DebridServiceVm, String, Vec<String>)> = Vec::new();
    for (service, key) in &services {
        let mut unknown = Vec::new();
        for h in &hashes {
            match b.store.debrid_cached(service.id(), h, now, CACHE_TTL_SECS).ok().flatten() {
                Some(cached) => {
                    answers.insert((*service, h.clone()), cached);
                }
                None if !unknown.contains(h) => unknown.push(h.clone()),
                None => {}
            }
        }
        let limit = match service {
            DebridServiceVm::RealDebrid | DebridServiceVm::AllDebrid => PROBES_SLOW,
            DebridServiceVm::TorBox | DebridServiceVm::Premiumize => PROBES_FAST,
        };
        unknown.truncate(limit);
        if !unknown.is_empty() {
            to_ask.push((*service, key.clone(), unknown));
        }
    }
    let checks = async {
        let mut out = Vec::new();
        for (service, key, list) in &to_ask {
            match client().check_cached(service_of(*service), key, list).await {
                Ok(found) => out.push((*service, found)),
                Err(e) => tracing::debug!(service = service.id(), error = %e, "debrid cache check failed"),
            }
        }
        out
    };
    match tokio::time::timeout(CHECK_BUDGET, checks).await {
        Ok(results) => {
            for (service, found) in results {
                for (h, cached) in found {
                    let _ = b.store.set_debrid_cached(service.id(), &h, cached, now);
                    answers.insert((service, h), cached);
                }
            }
        }
        Err(_) => tracing::debug!("debrid cache checks ran out of time"),
    }
    let order: Vec<DebridServiceVm> = services.iter().map(|(s, _)| *s).collect();
    for s in &mut sources {
        if s.kind != SourceKindVm::Torrent {
            continue;
        }
        let Some(h) = s.info_hash.as_deref().map(str::to_ascii_lowercase) else { continue };
        s.debrid = status_for(&order, &answers, &h);
    }
    rerank(sources)
}

/// The first service (in preference order) that has `hash` cached; else the first one that answered "not cached";
/// `None` when no service answered.
fn status_for(order: &[DebridServiceVm], answers: &HashMap<(DebridServiceVm, String), bool>, hash: &str) -> Option<DebridStatusVm> {
    if let Some(service) = order.iter().find(|s| answers.get(&(**s, hash.to_owned())) == Some(&true)) {
        return Some(DebridStatusVm { service: *service, cached: true });
    }
    order
        .iter()
        .find(|s| answers.contains_key(&(**s, hash.to_owned())))
        .map(|s| DebridStatusVm { service: *s, cached: false })
}

/// Score of a cached row ranked like a web source (module docs).
pub fn cached_score(score: f32, seeders: Option<u32>) -> f32 {
    let s = seeders.unwrap_or(0);
    let mut v = score - s.min(500) as f32 / 50.0;
    if s < 20 {
        v += 20.0;
    }
    v + CACHED_BONUS
}

/// Apply `cached_score` to cached rows and sort again (stable); `recommended` = the new first row. Manual rows stay on
/// top (the user aligned them for this episode).
fn rerank(mut sources: Vec<SourceVm>) -> Vec<SourceVm> {
    let mut changed = false;
    for s in &mut sources {
        if s.debrid.is_some_and(|d| d.cached) {
            s.score = cached_score(s.score, s.seeders);
            changed = true;
        }
    }
    if changed {
        sources.sort_by(|a, b| {
            let manual = |s: &SourceVm| s.ext_id == MANUAL_EXT;
            manual(b).cmp(&manual(a)).then_with(|| b.score.total_cmp(&a.score))
        });
        for (i, s) in sources.iter_mut().enumerate() {
            s.recommended = i == 0;
        }
    }
    sources
}

/// `(media id, episode)` of the `sources()` call that produced `list`: the newest cached resolution whose source ids
/// are exactly the ids of `list` (see the module docs).
fn current_episode(b: &LiveBackend, list: &[SourceVm]) -> Option<(String, u32)> {
    let mut ids: Vec<&str> = list.iter().filter(|s| s.ext_id != MANUAL_EXT).map(|s| s.id.as_str()).collect();
    ids.sort_unstable();
    let cache = b.candidates.lock().ok()?;
    cache
        .iter()
        .filter(|(_, (_, candidates))| {
            let mut keys: Vec<String> = candidates.iter().map(source_key).collect();
            keys.sort_unstable();
            keys.dedup();
            let mut want: Vec<&str> = ids.clone();
            want.dedup();
            keys.len() == want.len() && keys.iter().zip(&want).all(|(k, w)| k == w)
        })
        .max_by_key(|(_, (at, _))| *at)
        .map(|(key, _)| key.clone())
}

/// `list` with the manual torrents of its title / episode first (one row per torrent, not duplicated).
fn with_manual_rows(b: &LiveBackend, list: Vec<SourceVm>) -> Vec<SourceVm> {
    let Some((id, episode)) = current_episode(b, &list) else { return list };
    let mapped = b.store.torrents_for_episode(&id, episode).unwrap_or_default();
    if mapped.is_empty() {
        return list;
    }
    let picked = b.store.picked_source(&id, episode).ok().flatten();
    let engine = b.torrent.running();
    let top = list.iter().map(|s| s.score).fold(0.0_f32, f32::max);
    let mut rows: Vec<SourceVm> = Vec::new();
    for (torrent_id, index) in mapped {
        let source_id = manual_source_id(&torrent_id);
        if list.iter().any(|s| s.id == source_id) || rows.iter().any(|s| s.id == source_id) {
            continue;
        }
        let file = engine.as_ref().and_then(|e| e.files(&torrent_id).ok()).and_then(|f| f.into_iter().find(|f| f.index == index));
        let name = file
            .as_ref()
            .map(|f| f.name.clone())
            .or_else(|| engine.as_ref().and_then(|e| e.list().into_iter().find(|x| x.id == torrent_id)).map(|x| x.name))
            .unwrap_or_else(|| torrent_id.chars().take(12).collect());
        rows.push(manual_row(&source_id, &torrent_id, &name, file.map(|f| f.size_bytes), top, picked.as_deref()));
    }
    if rows.is_empty() {
        return list;
    }
    let any_picked = rows.iter().any(|r| r.picked);
    let mut out: Vec<SourceVm> = rows;
    for (i, r) in out.iter_mut().enumerate() {
        r.recommended = i == 0;
    }
    out.extend(list.into_iter().map(|mut s| {
        s.recommended = false;
        if any_picked {
            s.picked = false;
        }
        s
    }));
    out
}

fn manual_row(source_id: &str, hash: &str, name: &str, size: Option<u64>, top: f32, picked: Option<&str>) -> SourceVm {
    let tags = tk_core::release::parse_release(name);
    SourceVm {
        id: source_id.to_owned(),
        ext_id: MANUAL_EXT.to_owned(),
        ext_name: "Torrent".to_owned(),
        kind: SourceKindVm::Torrent,
        label: name.to_owned(),
        quality: tags.resolution.label().to_owned(),
        container: None,
        subs: Vec::new(),
        dubs: Vec::new(),
        reliability: 1.0,
        note: None,
        latency_ms: None,
        size_bytes: size,
        seeders: None,
        leechers: None,
        info_hash: Some(hash.to_owned()),
        ext_enabled: true,
        score: top + 1.0,
        recommended: false,
        picked: picked == Some(source_id),
        tags: ReleaseTagsVm { group: tags.group.clone(), episodes: tags.episodes, ..ReleaseTagsVm::default() },
        debrid: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, score: f32, seeders: Option<u32>, hash: Option<&str>) -> SourceVm {
        let mut r = manual_row(id, hash.unwrap_or(""), "[Group] Show - 01 [1080p].mkv", None, 0.0, None);
        r.ext_id = "ext".into();
        r.score = score;
        r.seeders = seeders;
        r.info_hash = hash.map(str::to_owned);
        r
    }

    #[test]
    fn cached_rows_lose_the_seeder_terms_and_gain_the_bonus() {
        // 5 seeders: +0.1 and -20 in `score`; cached: both undone, +15
        assert!((cached_score(10.0, Some(5)) - (10.0 - 0.1 + 20.0 + 15.0)).abs() < 1e-4);
        // 1000 seeders: +10 in `score`
        assert!((cached_score(60.0, Some(1000)) - (60.0 - 10.0 + 15.0)).abs() < 1e-4);
        assert!((cached_score(0.0, None) - 35.0).abs() < 1e-4);
    }

    #[test]
    fn rerank_moves_cached_rows_up() {
        let mut list = vec![row("a|web|x", 50.0, None, None), row("b|torrent|y", 30.0, Some(3), Some("h"))];
        list[0].kind = SourceKindVm::Web;
        list[0].recommended = true;
        list[1].debrid = Some(DebridStatusVm { service: DebridServiceVm::TorBox, cached: true });
        let out = rerank(list);
        assert_eq!(out[0].id, "b|torrent|y");
        assert!(out[0].recommended && !out[1].recommended);
        // nothing cached: untouched
        let plain = rerank(vec![row("a|web|x", 1.0, None, None), row("c|web|z", 2.0, None, None)]);
        assert_eq!(plain[0].id, "a|web|x");
    }

    #[test]
    fn status_prefers_the_first_service_with_a_hit() {
        let order = [DebridServiceVm::RealDebrid, DebridServiceVm::TorBox];
        let mut answers = HashMap::new();
        answers.insert((DebridServiceVm::RealDebrid, "h".to_owned()), false);
        answers.insert((DebridServiceVm::TorBox, "h".to_owned()), true);
        assert_eq!(status_for(&order, &answers, "h"), Some(DebridStatusVm { service: DebridServiceVm::TorBox, cached: true }));
        answers.insert((DebridServiceVm::TorBox, "h".to_owned()), false);
        assert_eq!(status_for(&order, &answers, "h"), Some(DebridStatusVm { service: DebridServiceVm::RealDebrid, cached: false }));
        assert_eq!(status_for(&order, &answers, "other"), None);
    }

    #[test]
    fn services_map_one_to_one() {
        for vm in DebridServiceVm::ALL {
            assert_eq!(service_of(vm).id(), vm.id());
            assert_eq!(service_of(vm).label(), vm.label());
        }
        let manual = manual_row("manual|torrent|h", "h", "[SubsPlease] Show - 03 (1080p).mkv", Some(9), 40.0, Some("manual|torrent|h"));
        assert!(manual.picked && manual.ext_enabled);
        assert_eq!((manual.score, manual.size_bytes), (41.0, Some(9)));
        assert_eq!(manual.quality, "1080p");
    }
}
