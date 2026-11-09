//! Torrent playback path (docs/redesign/10 section 4.2). Owner: wave 3 B.
//!
//! `play`: `resolve_source` -> `engine.add(source, AddOptions { persist: after_watch == Keep, label, .. })` (or the
//! torrent already in the session) -> `tk_core::release::pick_episode_file(files, episode, titles)` ->
//! `engine.select_file` -> `engine.set_label` -> `engine.stream_url` -> `TorrentPlaybackVm` (title / poster / start
//! position like `playback_source`). No file matched -> `ErrorKind::NoSource` with the torrent id as detail (the
//! player then offers the Files page of the client).
//!
//! `skip_marks`: AniSkip by MAL id, cached per episode for the session.
//!
//! Wave A (worker i): the file comes from the manual alignment first (`LocalStore::torrent_episode_map`, written by
//! Torrents > Library "Add torrent" and Files "Play this file"), then from `pick_episode_file`. A torrent added by hand
//! for a title is offered in that title's source list as `manual|torrent|<info-hash>` (`live/debrid.rs::mark_cached`
//! adds the row); that source id plays the torrent by its hash. With `TorrentSettingsVm::preload_next`, a batch
//! starts downloading the next episode's file once the history says 80 % of this one was watched.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use tk_core::{HistoryEntry, LocalStore, Media};
use tk_core::release::{CandidateFile, is_video_path, parse_release, pick_episode_file};
use tk_core::sources::{SourceCandidate, SourceKind, parse_source_key, source_key};
use tk_torrent::{AddOptions, AddSource, TorrentEngine, TorrentLabel};
use tk_ui::backend::{AfterWatchVm, PlaybackVm, SkipKindVm, SkipMarkVm, StreamVm, TorrentPlaybackVm, UiResult};
use tk_ui::model::{ErrorKind, UiError};

use super::LiveBackend;
use super::torrent::engine_err;

/// AniSkip answers in well under a second; a slow answer must not hold the seek bar back.
const ANISKIP_TIMEOUT: Duration = Duration::from_secs(6);
const ANISKIP_BASE: &str = "https://api.aniskip.com/v2/skip-times";

/// Extension id of the source rows that stand for a torrent added by hand (`manual|torrent|<info-hash>`).
pub const MANUAL_EXT: &str = "manual";
/// Share of the episode after which the next file of a batch starts downloading (`preload_next`).
const PRELOAD_AT: f64 = 0.8;
/// How often the preload watcher reads the history.
const PRELOAD_POLL: Duration = Duration::from_secs(15);
/// The preload watcher gives up after this long (the player left long ago).
const PRELOAD_MAX: Duration = Duration::from_secs(4 * 3600);

/// Source id of a torrent added by hand (see the module docs).
pub fn manual_source_id(info_hash: &str) -> String {
    format!("{MANUAL_EXT}|torrent|{}", info_hash.to_ascii_lowercase())
}

/// The file of `episode` in the manual alignment `map` (`(file index, episode)` rows), if the user set one.
pub fn mapped_file(map: &[(usize, u32)], episode: u32) -> Option<usize> {
    map.iter().find(|(_, e)| *e == episode && episode > 0).map(|(i, _)| *i)
}

/// What `resolve` found besides the add source and the label: shown as `StreamVm::provider` / `quality`.
struct Resolved {
    source: AddSource,
    label: TorrentLabel,
    /// Release group (or extension name) of the candidate; `None` for a bare magnet / hash.
    provider: Option<String>,
    quality: Option<String>,
}

/// `Backend::torrent_play`.
pub async fn play(b: &LiveBackend, source: &str, id: &str, episode: u32) -> UiResult<TorrentPlaybackVm> {
    let (r, media) = b.media(id).await?;
    let resolved = resolve(b, source, id, Some(episode), &media).await?;
    let engine = b.torrent.engine().await?;
    let keep = super::torrent_settings::load(&b.paths()).after_watch == AfterWatchVm::Keep;
    let label = TorrentLabel { episode: Some(episode), ..resolved.label.clone() };
    // Start with nothing selected: librqbit begins downloading the selection as soon as the torrent is added, so a
    // season pack added with every file would fetch all of them before `select_file` narrows it to the episode.
    let add = AddOptions { only_files: Some(Vec::new()), persist: keep, label: Some(label.clone()), ..AddOptions::default() };
    let torrent_id = engine.add(resolved.source, add).await.map_err(engine_err)?;

    let files = engine.files(&torrent_id).map_err(engine_err)?;
    let candidates: Vec<CandidateFile> =
        files.iter().map(|f| CandidateFile { index: f.index, path: f.path.clone(), size_bytes: f.size_bytes }).collect();
    let videos = candidates.iter().filter(|f| is_video_path(&f.path)).count();
    let names = titles(&media);
    // Wave A (worker i): the user's own alignment wins over the file-name guess.
    let map = b.store.torrent_episode_map(&torrent_id).unwrap_or_default();
    let index = mapped_file(&map, episode)
        .filter(|i| files.iter().any(|f| f.index == *i))
        .or_else(|| pick_episode_file(&candidates, episode, &names))
        .ok_or_else(|| UiError::new(ErrorKind::NoSource, torrent_id.clone()))?;
    let file = files.iter().find(|f| f.index == index).ok_or_else(|| UiError::new(ErrorKind::NoSource, torrent_id.clone()))?;

    engine.select_file(&torrent_id, index).await.map_err(engine_err)?;
    // Batches: the Library shows the episode being watched now, not the one the torrent was first added for.
    if let Err(e) = engine.set_label(&torrent_id, label) {
        tracing::warn!(error = %e, "updating the torrent label failed");
    }
    let url = engine.stream_url(&torrent_id, index).map_err(engine_err)?;

    // Wave A (worker i, checklist `torrent.preload-next`): the next episode of a batch starts downloading at 80 %.
    if videos > 1 && super::torrent_settings::load(&b.paths()).preload_next {
        let next = mapped_file(&map, episode + 1)
            .filter(|i| files.iter().any(|f| f.index == *i))
            .or_else(|| pick_episode_file(&candidates, episode + 1, &names))
            .filter(|i| *i != index);
        if let (Some(next), Some(key)) = (next, HistoryEntry::anime_key(&r)) {
            spawn_preload(b.store.clone(), engine.clone(), torrent_id.clone(), key, episode, next);
        }
    }

    let tags = parse_release(&file.name);
    let provider = resolved.provider.or(tags.group.clone()).unwrap_or_else(|| "torrent".to_owned());
    let quality = resolved.quality.or_else(|| Some(tags.resolution.label().to_owned()).filter(|q| !q.is_empty()));
    tracing::info!(torrent = %torrent_id, file = index, name = %file.name, "torrent stream ready");
    Ok(TorrentPlaybackVm {
        playback: PlaybackVm {
            title: media.title.clone(),
            poster: media.cover.clone(),
            episodes_total: media.episodes,
            start_secs: b.start_secs(&r, episode),
            streams: vec![StreamVm { url, headers: Vec::new(), provider, quality, subtitles: Vec::new() }],
        },
        torrent_id,
        file_index: index,
        file_name: file.name.clone(),
        batch: videos > 1,
    })
}

/// What to add for `source` (a torrent `SourceVm::id` -> its candidate's magnet, else `magnet:?xt=urn:btih:<hash>`
/// with the candidate's trackers; a magnet; a 40-hex hash / torrent id) plus the label for the Library. Shared with
/// `torrent_settings::add_download` (D).
pub async fn resolve_source(b: &LiveBackend, source: &str, id: &str, episode: Option<u32>) -> UiResult<(AddSource, TorrentLabel)> {
    let (_, media) = b.media(id).await?;
    let r = resolve(b, source, id, episode, &media).await?;
    Ok((r.source, r.label))
}

async fn resolve(b: &LiveBackend, source: &str, id: &str, episode: Option<u32>, media: &Media) -> UiResult<Resolved> {
    let label = TorrentLabel { media_id: id.to_owned(), title: media.title.clone(), episode, poster: media.cover.clone() };
    // A torrent added by hand (`manual|torrent|<info-hash>`): played by its hash, it is already in the engine.
    if let Some((ext, SourceKind::Torrent, hash)) = parse_source_key(source)
        && ext == MANUAL_EXT
    {
        let add = AddSource::parse(&hash).ok_or_else(|| UiError::new(ErrorKind::NoSource, source.to_owned()))?;
        return Ok(Resolved { source: add, label, provider: Some("torrent".to_owned()), quality: None });
    }
    if let Some((_, kind, _)) = parse_source_key(source) {
        if kind != SourceKind::Torrent {
            return Err(UiError::new(ErrorKind::Other, format!("{source} is not a torrent source")));
        }
        let candidate = find_candidate(b, source, id, episode, media).await?;
        let t = candidate.torrent.as_ref().ok_or_else(|| UiError::new(ErrorKind::NoSource, source.to_owned()))?;
        let add = t
            .magnet
            .as_deref()
            .and_then(AddSource::parse)
            .or_else(|| {
                AddSource::parse(&t.info_hash)
                    .map(|_| AddSource::Magnet(format!("magnet:?xt=urn:btih:{}", t.info_hash.to_ascii_lowercase())))
            })
            .ok_or_else(|| UiError::new(ErrorKind::NoSource, source.to_owned()))?;
        let quality = Some(candidate.quality.label().to_owned()).filter(|q| !q.is_empty());
        let provider = candidate
            .tags
            .group
            .clone()
            .or_else(|| Some(candidate.label.clone()).filter(|l| !l.is_empty()))
            .or_else(|| Some(candidate.ext_name.clone()));
        return Ok(Resolved { source: add, label, provider, quality });
    }
    let add = AddSource::parse(source).ok_or_else(|| UiError::new(ErrorKind::Other, format!("not a torrent source: {source}")))?;
    Ok(Resolved { source: add, label, provider: None, quality: None })
}

/// The candidate behind a torrent `SourceVm::id`: the episode's list (cached, then fresh once) or, without an
/// episode (whole-torrent downloads), any cached list of that title before resolving episode 1.
pub(super) async fn find_candidate(b: &LiveBackend, source: &str, id: &str, episode: Option<u32>, media: &Media) -> UiResult<SourceCandidate> {
    if episode.is_none() {
        let cached = b.candidates.lock().ok().and_then(|c| {
            c.iter()
                .filter(|((mid, _), _)| mid.as_str() == id)
                .flat_map(|(_, (_, v))| v.iter())
                .find(|c| source_key(c) == source)
                .cloned()
        });
        if let Some(c) = cached {
            return Ok(c);
        }
    }
    let ep = episode.unwrap_or(1);
    let find = |list: Vec<SourceCandidate>| list.into_iter().find(|c| source_key(c) == source);
    if let Some(c) = find(b.episode_candidates(id, ep, media, false).await) {
        return Ok(c);
    }
    find(b.episode_candidates(id, ep, media, true).await).ok_or_else(|| UiError::new(ErrorKind::NoSource, source.to_owned()))
}

/// Main title first, then every other title the metadata knows (romaji / english / native), without repeats.
pub(super) fn titles(media: &Media) -> Vec<String> {
    let mut out = vec![media.title.clone()];
    for t in media.titles.values() {
        if !t.is_empty() && !out.iter().any(|o| o.eq_ignore_ascii_case(t)) {
            out.push(t.clone());
        }
    }
    out
}

/// Watch the history of `(anime key, episode)` and select `next` in the torrent once 80 % was watched. Runs on the
/// current runtime; ends when done, when the torrent left the engine or after `PRELOAD_MAX`.
fn spawn_preload(store: Arc<LocalStore>, engine: Arc<TorrentEngine>, torrent_id: String, key: String, episode: u32, next: usize) {
    let Ok(rt) = tokio::runtime::Handle::try_current() else { return };
    rt.spawn(async move {
        let started = Instant::now();
        while started.elapsed() < PRELOAD_MAX {
            tokio::time::sleep(PRELOAD_POLL).await;
            if engine.stats(&torrent_id).is_err() {
                return;
            }
            let rows = store.progress_for(&key).unwrap_or_default();
            if watched_share(&rows, episode).is_some_and(|f| f >= PRELOAD_AT) {
                match engine.select_file(&torrent_id, next).await {
                    Ok(()) => tracing::info!(torrent = %torrent_id, file = next, "preloading the next episode of the batch"),
                    Err(e) => tracing::warn!(torrent = %torrent_id, error = %e, "preloading the next episode failed"),
                }
                return;
            }
        }
    });
}

/// Watched share (0.0..=1.0) of `episode` from its history row; `None` without a row or a known duration.
fn watched_share(rows: &[HistoryEntry], episode: u32) -> Option<f64> {
    let row = rows.iter().find(|e| e.episode == episode)?;
    if row.completed {
        return Some(1.0);
    }
    let duration = row.duration_sec.filter(|d| *d > 0)?;
    Some((f64::from(row.progress_sec) / f64::from(duration)).clamp(0.0, 1.0))
}

/// The torrent the next episode would play from, for `torrent_preload_next`: a torrent of the engine labelled for
/// this title with a file for `episode` (manual alignment first, then the file names).
pub(super) fn batch_file_for(b: &LiveBackend, engine: &TorrentEngine, id: &str, episode: u32, names: &[String]) -> Option<(String, usize)> {
    for entry in engine.list() {
        if entry.label.as_ref().is_none_or(|l| l.media_id != id) {
            continue;
        }
        let Ok(files) = engine.files(&entry.id) else { continue };
        let map = b.store.torrent_episode_map(&entry.id).unwrap_or_default();
        let candidates: Vec<CandidateFile> =
            files.iter().map(|f| CandidateFile { index: f.index, path: f.path.clone(), size_bytes: f.size_bytes }).collect();
        if let Some(i) = mapped_file(&map, episode).or_else(|| pick_episode_file(&candidates, episode, names)) {
            return Some((entry.id, i));
        }
    }
    None
}

fn aniskip_client() -> Option<&'static reqwest::Client> {
    static CLIENT: OnceLock<Option<reqwest::Client>> = OnceLock::new();
    CLIENT
        .get_or_init(|| reqwest::Client::builder().user_agent("TatakaiNext/0.1").timeout(ANISKIP_TIMEOUT).build().ok())
        .as_ref()
}

/// `(mal id, episode) -> marks`, for the session. Only answers AniSkip actually gave are cached (a network error is
/// retried next time).
fn skip_cache() -> &'static Mutex<HashMap<(u32, u32), Vec<SkipMarkVm>>> {
    static CACHE: OnceLock<Mutex<HashMap<(u32, u32), Vec<SkipMarkVm>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// `Backend::skip_marks`: AniSkip (`https://api.aniskip.com/v2/skip-times/<mal id>/<episode>?types=op&types=ed&
/// types=recap&episodeLength=0`, fixed host, short timeout, cached per episode); empty when the title has no MAL id
/// or AniSkip has no data.
pub async fn skip_marks(b: &LiveBackend, id: &str, episode: u32) -> UiResult<Vec<SkipMarkVm>> {
    let mal = match super::media_ref(id).and_then(|r| r.mal_id) {
        Some(m) => Some(m),
        None => b.media(id).await.ok().and_then(|(_, m)| m.media_ref.mal_id),
    };
    let Some(mal) = mal.filter(|m| *m > 0) else { return Ok(Vec::new()) };
    if let Some(hit) = skip_cache().lock().ok().and_then(|c| c.get(&(mal, episode)).cloned()) {
        return Ok(hit);
    }
    let Some(client) = aniskip_client() else { return Ok(Vec::new()) };
    let url = format!("{ANISKIP_BASE}/{mal}/{episode}?types=op&types=ed&types=recap&episodeLength=0");
    let resp = match client.get(&url).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::debug!(error = %e, "AniSkip unreachable");
            return Ok(Vec::new());
        }
    };
    let status = resp.status();
    // 404 = AniSkip has nothing for this episode (a real answer, cached as empty).
    if !status.is_success() && status != reqwest::StatusCode::NOT_FOUND {
        tracing::debug!(%status, "AniSkip answered with an error");
        return Ok(Vec::new());
    }
    let marks = match resp.text().await {
        Ok(body) => parse_aniskip(&body),
        Err(_) => return Ok(Vec::new()),
    };
    if let Ok(mut c) = skip_cache().lock() {
        c.insert((mal, episode), marks.clone());
    }
    Ok(marks)
}

/// AniSkip v2 body (`{ found, results: [{ interval: { startTime, endTime }, skipType }] }`) -> marks sorted by start.
/// `op` / `mixed-op` = intro, `ed` / `mixed-ed` = outro, `recap`; anything else or malformed is skipped.
fn parse_aniskip(body: &str) -> Vec<SkipMarkVm> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(body) else { return Vec::new() };
    if v["found"].as_bool() == Some(false) {
        return Vec::new();
    }
    let mut out: Vec<SkipMarkVm> = v["results"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|r| {
                    let kind = match r["skipType"].as_str()? {
                        "op" | "mixed-op" => SkipKindVm::Intro,
                        "ed" | "mixed-ed" => SkipKindVm::Outro,
                        "recap" => SkipKindVm::Recap,
                        _ => return None,
                    };
                    let start = r["interval"]["startTime"].as_f64()?;
                    let end = r["interval"]["endTime"].as_f64()?;
                    (start.is_finite() && end.is_finite() && start >= 0.0 && end > start)
                        .then_some(SkipMarkVm { kind, start_secs: start, end_secs: end })
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| a.start_secs.total_cmp(&b.start_secs));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_alignment_and_source_ids() {
        let map = vec![(3, 1), (5, 2), (7, 0)];
        assert_eq!(mapped_file(&map, 2), Some(5));
        assert_eq!(mapped_file(&map, 4), None);
        assert_eq!(mapped_file(&map, 0), None, "episode 0 means 'no episode'");
        let id = manual_source_id(&"AB".repeat(20));
        assert_eq!(id, format!("manual|torrent|{}", "ab".repeat(20)));
        let (ext, kind, hash) = parse_source_key(&id).unwrap();
        assert_eq!((ext.as_str(), kind, hash.len()), (MANUAL_EXT, SourceKind::Torrent, 40));
    }

    #[test]
    fn watched_share_from_history() {
        let row = |episode: u32, progress: u32, duration: Option<u32>, completed: bool| HistoryEntry {
            anime_id: "21".into(),
            title: "Show".into(),
            poster: None,
            episode_id: format!("21?ep={episode}"),
            episode,
            progress_sec: progress,
            duration_sec: duration,
            completed,
            watched_at: time::OffsetDateTime::UNIX_EPOCH,
            anilist_id: Some(21),
            mal_id: None,
        };
        let rows = vec![row(1, 1200, Some(1440), false), row(2, 30, None, false), row(3, 0, Some(1440), true)];
        assert!(watched_share(&rows, 1).is_some_and(|f| (f - 1200.0 / 1440.0).abs() < 1e-9));
        assert_eq!(watched_share(&rows, 2), None);
        assert_eq!(watched_share(&rows, 3), Some(1.0));
        assert_eq!(watched_share(&rows, 9), None);
    }

    #[test]
    fn aniskip_bodies_map_to_marks() {
        let body = r#"{"found":true,"results":[
            {"interval":{"startTime":1320.5,"endTime":1410.0},"skipType":"ed","skipId":"b","episodeLength":1440.0},
            {"interval":{"startTime":84.0,"endTime":174.0},"skipType":"op","skipId":"a","episodeLength":1440.0},
            {"interval":{"startTime":0.0,"endTime":30.0},"skipType":"recap","skipId":"c","episodeLength":1440.0},
            {"interval":{"startTime":10.0,"endTime":5.0},"skipType":"op","skipId":"bad","episodeLength":1440.0},
            {"interval":{"startTime":10.0,"endTime":20.0},"skipType":"preview","skipId":"d","episodeLength":1440.0}
        ],"message":"Successfully found skip times","statusCode":200}"#;
        let marks = parse_aniskip(body);
        let kinds: Vec<_> = marks.iter().map(|m| m.kind).collect();
        assert_eq!(kinds, [SkipKindVm::Recap, SkipKindVm::Intro, SkipKindVm::Outro]);
        assert_eq!((marks[1].start_secs, marks[1].end_secs), (84.0, 174.0));
        assert!(parse_aniskip(r#"{"found":false,"results":[],"statusCode":404}"#).is_empty());
        assert!(parse_aniskip("not json").is_empty());
        let mixed = parse_aniskip(r#"{"found":true,"results":[{"interval":{"startTime":1.0,"endTime":2.0},"skipType":"mixed-op"}]}"#);
        assert_eq!(mixed[0].kind, SkipKindVm::Intro);
    }
}
