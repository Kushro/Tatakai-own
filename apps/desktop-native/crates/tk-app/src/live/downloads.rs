//! Download engine, Downloads page, local library and auto-download (wave B, worker **h**;
//! docs/redesign/16-parity-wave-b-contract.md section 4; checklist `downloads.*`; classic
//! `desktop/ipc/ipc-download-manager.cjs`, `desktop/services/auto-downloader.cjs`, `desktop/ipc/ipc-library.cjs`).
//!
//! * Engine: `tk_core::download::Engine` (HLS with AES-128, MP4 `Range` resume, manga / novel chapters, queue with
//!   `Settings::downloads.concurrent`, speed limit). Rows in `LocalStore` (`downloads`, `local_library`,
//!   `auto_download_runs`), files under `Settings::downloads.dir` (empty = `<user Downloads>/Tatakai/Episodes`,
//!   apart from the torrent folder `<user Downloads>/Tatakai`).
//! * `on_start`: applies the prefs, puts interrupted rows back in the queue (`downloads.resume`), verifies completed
//!   rows (`downloads.integrity-check`), watches the library folders (`notify`) and spawns the scheduler: the
//!   auto-downloader every `auto_check_minutes` while the app is open (`downloads.auto.schedule`) plus the watched /
//!   storage-limit housekeeping. The scheduler holds `'static` clones of what it needs ([`Ctx`]), never the backend.
//! * Torrents stay in the torrent engine (worker i): `downloads()` mirrors its kept torrents as `DownloadKindVm::Torrent`
//!   rows with negative ids ([`torrent_row_id`]) and the row actions route back to it.
//!
//! ## Contract with workers l and m (manga / novel chapters)
//!
//! `manga_download_chapters` / `novel_download_chapters` build one `tk_core::download::Job` per chapter and call
//! [`enqueue_jobs`] (or [`enqueue_jobs_with`] to set the title / chapter label). Build the target with
//! [`chapter_target`] so the engine can record the chapter in `offline_chapters` when it finishes:
//!
//! | `Job::kind` | `Job::url` | `Job::target` |
//! |---|---|---|
//! | `KIND_MANGA` (`"manga"`) | one page URL per line, reading order | `chapter_target(b, media, ext, chapter, false)` (a folder; pages `001.jpg`...) |
//! | `KIND_NOVEL` (`"novel"`) | an http(s) URL, or the chapter text (JSON list of paragraphs or plain text) | `chapter_target(b, media, ext, chapter, true)` (`.json`) |
//!
//! `Job::headers` go with every request; `Job::episode` is `None`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tk_core::download::watch::LibraryWatcher;
use tk_core::download::{Engine, Job, KIND_HLS, KIND_MP4, Meta, Progress, files, library};
use tk_core::sources::{INITIAL_RELIABILITY, Quality, SourceCandidate, SourceKind, SourcePrefs, parse_source_key, rank, source_key};
use tk_core::store::downloads::{AutoRunRow, DownloadRow, LocalItemRow};
use tk_core::{Compat, DownloadError, HistoryEntry, LocalStore, MediaKind, MetadataRouter};
use tk_ext::ExtensionHost;
use tk_ui::backend::{
    AutoDownloadRunVm, Backend, DownloadRequestVm, DownloadStorageVm, DownloadVm, LibraryScanVm, LocalItemVm, MediaVm,
    PlaybackVm, StreamVm, TorrentLibraryItemVm, TorrentStateVm, UiResult,
};
use tk_ui::model::downloads::{
    AUTO_MAX_PER_TITLE, DownloadKindVm, DownloadPrefs, DownloadStateVm, LOCAL_PROVIDER, OPEN_ROOT_FOLDER, counts_as_have,
    is_aired, limit_bytes, plan_new_episodes, storage_limit_reached,
};
use tk_ui::model::{ErrorKind, Settings, UiError, format_unix_utc};

use crate::live::LiveBackend;
use crate::paths::AppPaths;
use crate::settings_store;

/// First scheduler pass after start-up (the UI and the extensions settle first).
const FIRST_PASS_DELAY: Duration = Duration::from_secs(90);
/// Scheduler period (the auto-download interval itself is `auto_check_minutes`).
const SCHEDULER_TICK: Duration = Duration::from_secs(60);
/// Quiet time after the last watcher event before the batch is imported.
const WATCH_DEBOUNCE: Duration = Duration::from_secs(5);
/// Lookup of a title for a local file played offline (the episode count for the player's "next").
const OFFLINE_TITLE_TIMEOUT: Duration = Duration::from_secs(2);
/// A search result must look like the guessed title this much (`library::title_score`) to be matched.
const MATCH_MIN_SCORE: u32 = 70;

fn now_unix() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn other(detail: impl Into<String>) -> UiError {
    UiError::new(ErrorKind::Other, detail)
}

fn store_err(e: impl std::fmt::Display) -> UiError {
    other(e.to_string())
}

fn dl_err(e: DownloadError) -> UiError {
    match e {
        DownloadError::Network(_) => UiError::new(ErrorKind::Offline, e.to_string()),
        _ => other(e.to_string()),
    }
}

/// State shared by the backend calls, the scheduler and the library watcher.
struct Shared {
    started: AtomicBool,
    /// One auto-download pass at a time ("Check now" while the timer runs).
    auto_busy: AtomicBool,
    watcher: Mutex<Option<LibraryWatcher>>,
    watch_dirs: Mutex<Vec<PathBuf>>,
}

/// The engine: queue, workers, local-library watcher and the auto-download timer. Constructed once by
/// `LiveBackend::new`; no task runs before `on_start`.
pub struct Downloads {
    paths: AppPaths,
    engine: Engine,
    shared: Arc<Shared>,
}

impl Downloads {
    pub fn new(paths: AppPaths, store: Arc<LocalStore>, client: reqwest::Client) -> Self {
        let engine = Engine::new(client, store, default_dir(&paths), 2);
        Self {
            paths,
            engine,
            shared: Arc::new(Shared {
                started: AtomicBool::new(false),
                auto_busy: AtomicBool::new(false),
                watcher: Mutex::new(None),
                watch_dirs: Mutex::new(Vec::new()),
            }),
        }
    }

    fn apply(&self, prefs: &DownloadPrefs) {
        let prefs = prefs.clone().sanitized();
        self.engine.set_dir(dir_for(&self.paths, &prefs));
        self.engine.set_limits(prefs.speed_limit_kib, prefs.concurrent);
    }
}

/// `<user Downloads>/Tatakai/Episodes`, else `<data>/downloads` (separate from the torrent folder).
pub fn default_dir(paths: &AppPaths) -> PathBuf {
    directories::UserDirs::new()
        .and_then(|u| u.download_dir().map(|d| d.join("Tatakai").join("Episodes")))
        .unwrap_or_else(|| paths.root.join("downloads"))
}

/// Folder in use for `prefs` (`dir` empty = the default).
fn dir_for(paths: &AppPaths, prefs: &DownloadPrefs) -> PathBuf {
    let d = prefs.dir.trim();
    if d.is_empty() { default_dir(paths) } else { PathBuf::from(d) }
}

/// Everything the background tasks need, as `'static` clones (no backend reference).
#[derive(Clone)]
struct Ctx {
    store: Arc<LocalStore>,
    router: Arc<MetadataRouter>,
    host: Arc<ExtensionHost>,
    settings_file: PathBuf,
    engine: Engine,
    shared: Arc<Shared>,
}

fn ctx(b: &LiveBackend) -> Ctx {
    Ctx {
        store: b.store.clone(),
        router: b.router.clone(),
        host: b.ext.host(),
        settings_file: b.settings_file.clone(),
        engine: b.downloads.engine.clone(),
        shared: b.downloads.shared.clone(),
    }
}

impl Ctx {
    fn settings(&self) -> Settings {
        settings_store::load(&self.settings_file)
    }
}

// ---- start-up / settings ------------------------------------------------------------------------------------------

/// Start-up: resume `queued` / `downloading` rows from their checkpoint (`downloads.resume`), verify completed rows
/// (`downloads.integrity-check`), watch the library folders and arm the auto-download timer (`downloads.auto.schedule`).
/// Returns quickly: the long-running parts are spawned tasks.
pub async fn on_start(b: &LiveBackend) {
    let d = &b.downloads;
    if d.shared.started.swap(true, Ordering::SeqCst) {
        return;
    }
    let settings = settings_store::load(&b.settings_file);
    d.apply(&settings.downloads);
    d.engine.start();
    let engine = d.engine.clone();
    match tokio::task::spawn_blocking(move || engine.verify_completed()).await {
        Ok(0) => {}
        Ok(n) => tracing::info!(broken = n, "completed downloads whose file is missing or truncated"),
        Err(e) => tracing::warn!(error = %e, "download verification task failed"),
    }
    let c = ctx(b);
    restart_watcher(&c, &settings.downloads.library_dirs);
    if tokio::runtime::Handle::try_current().is_ok() {
        let _ = tokio::spawn(scheduler(c));
    }
}

/// `Settings::downloads` changed (folder, limits, library dirs). The auto-download prefs are read on every pass.
pub async fn on_settings(b: &LiveBackend, settings: &Settings) {
    b.downloads.apply(&settings.downloads);
    if b.downloads.shared.started.load(Ordering::SeqCst) {
        restart_watcher(&ctx(b), &settings.downloads.library_dirs);
    }
}

/// (Re)start the `notify` watcher when the folder list changed. New videos are imported in debounced batches.
fn restart_watcher(c: &Ctx, dirs: &[String]) {
    let dirs: Vec<PathBuf> = dirs.iter().map(|d| PathBuf::from(d.trim())).filter(|d| !d.as_os_str().is_empty()).collect();
    {
        let Ok(mut current) = c.shared.watch_dirs.lock() else { return };
        if *current == dirs {
            return;
        }
        current.clone_from(&dirs);
    }
    let Ok(mut slot) = c.shared.watcher.lock() else { return };
    // Dropping the old watcher drops its sender: the old import task ends.
    *slot = None;
    if dirs.is_empty() || tokio::runtime::Handle::try_current().is_err() {
        return;
    }
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<PathBuf>();
    match LibraryWatcher::start(&dirs, move |p| {
        let _ = tx.send(p);
    }) {
        Ok(w) => *slot = Some(w),
        Err(e) => {
            tracing::warn!(error = %e, "library folders are not watched (scan them by hand)");
            return;
        }
    }
    drop(slot);
    let c = c.clone();
    let _ = tokio::spawn(async move {
        while let Some(first) = rx.recv().await {
            let mut batch: HashSet<PathBuf> = HashSet::from([first]);
            while let Ok(Some(p)) = tokio::time::timeout(WATCH_DEBOUNCE, rx.recv()).await {
                batch.insert(p);
            }
            let paths: Vec<PathBuf> = batch.into_iter().filter(|p| p.is_file()).collect();
            if paths.is_empty() {
                continue;
            }
            match import_paths(&c, paths).await {
                Ok(vm) => tracing::info!(added = vm.added, matched = vm.matched, "library folder change imported"),
                Err(e) => tracing::warn!(detail = %e.detail, "library folder change not imported"),
            }
        }
    });
}

/// Whether the auto-downloader is due (`last` = the last pass, `minutes` = `auto_check_minutes`).
pub fn auto_due(last: Option<i64>, now: i64, minutes: u16) -> bool {
    last.is_none_or(|l| now - l >= i64::from(minutes) * 60)
}

/// Background loop: auto-download when due, then housekeeping (watched marks, `delete_watched_days`, storage cap).
async fn scheduler(c: Ctx) {
    tokio::time::sleep(FIRST_PASS_DELAY).await;
    loop {
        let prefs = c.settings().downloads.sanitized();
        if prefs.auto_new_episodes {
            let last = c.store.last_auto_run().ok().flatten().map(|r| r.ran_at);
            if auto_due(last, now_unix(), prefs.auto_check_minutes) {
                match run_auto(&c).await {
                    Ok(run) => tracing::info!(checked = run.checked, enqueued = run.enqueued, "auto-download pass"),
                    Err(e) => tracing::debug!(detail = %e.detail, "auto-download pass skipped"),
                }
            }
        }
        housekeeping(&c, &prefs).await;
        tokio::time::sleep(SCHEDULER_TICK).await;
    }
}

/// Stamp `watched_at` on completed episodes the history says were finished; delete watched files after
/// `delete_watched_days`; over the storage cap, delete the oldest watched ones first.
async fn housekeeping(c: &Ctx, prefs: &DownloadPrefs) {
    let now = now_unix();
    let Ok(rows) = c.store.downloads_in_state("completed") else { return };
    let mut finished: HashMap<String, HashSet<u32>> = HashMap::new();
    for r in rows.iter().filter(|r| r.watched_at.is_none() && r.episode.is_some()) {
        let eps = finished.entry(r.media_id.clone()).or_insert_with(|| {
            super::media_ref(&r.media_id)
                .and_then(|m| HistoryEntry::anime_key(&m))
                .and_then(|k| c.store.progress_for(&k).ok())
                .unwrap_or_default()
                .into_iter()
                .filter(|e| e.completed)
                .map(|e| e.episode)
                .collect()
        });
        if let Some(ep) = r.episode
            && eps.contains(&ep)
            && let Err(e) = c.store.mark_download_watched(&r.media_id, ep, now)
        {
            tracing::warn!(error = %e, "watched mark not saved");
        }
    }
    let Ok(rows) = c.store.downloads_in_state("completed") else { return };
    let mut doomed = watched_expired(&rows, prefs.delete_watched_days, now);
    if let Some(limit) = limit_bytes(prefs.storage_limit_gib) {
        let used = c.store.completed_download_bytes().unwrap_or(0);
        for id in over_limit(&rows, used, limit) {
            if !doomed.contains(&id) {
                doomed.push(id);
            }
        }
    }
    for id in doomed {
        match c.engine.remove(id, true).await {
            Ok(()) => tracing::info!(id, "watched download deleted"),
            Err(e) => tracing::warn!(id, error = %e, "watched download not deleted"),
        }
    }
}

/// Completed rows watched more than `days` ago (0 = keep forever).
pub fn watched_expired(rows: &[DownloadRow], days: u16, now: i64) -> Vec<i64> {
    if days == 0 {
        return Vec::new();
    }
    let cutoff = now - i64::from(days) * 86_400;
    rows.iter().filter(|r| r.watched_at.is_some_and(|w| w <= cutoff)).map(|r| r.id).collect()
}

/// Watched rows to delete (oldest watched first) until `used` fits `limit`. Unwatched files are never touched.
pub fn over_limit(rows: &[DownloadRow], used: u64, limit: u64) -> Vec<i64> {
    if used <= limit {
        return Vec::new();
    }
    let mut watched: Vec<&DownloadRow> = rows.iter().filter(|r| r.watched_at.is_some()).collect();
    watched.sort_by_key(|r| r.watched_at);
    let mut left = used;
    let mut out = Vec::new();
    for r in watched {
        if left <= limit {
            break;
        }
        left = left.saturating_sub(r.bytes_total.unwrap_or(r.bytes_done));
        out.push(r.id);
    }
    out
}

// ---- source choice ------------------------------------------------------------------------------------------------

/// Tallest frame of a quality tag (`None` = unknown).
fn height(q: Quality) -> Option<u32> {
    match q {
        Quality::Unknown => None,
        Quality::P480 => Some(480),
        Quality::P720 => Some(720),
        Quality::P1080 => Some(1080),
        Quality::P1440 => Some(1440),
        Quality::P2160 => Some(2160),
    }
}

fn is_embed(c: &SourceCandidate) -> bool {
    c.web.as_ref().is_some_and(|w| w.embed)
}

/// `(preferred server, direct stream, preferred audio, within the quality cap)`: higher sorts first.
fn preference(c: &SourceCandidate, prefs: &DownloadPrefs, max_height: Option<u32>) -> (bool, bool, bool, bool) {
    let server = prefs
        .preferred_server
        .as_deref()
        .is_some_and(|s| c.ext_id == s || c.label.eq_ignore_ascii_case(s) || c.ext_name.eq_ignore_ascii_case(s));
    let audio = if c.dubs.is_empty() {
        prefs.preferred_audio == "ja"
    } else {
        c.dubs.iter().any(|d| d.eq_ignore_ascii_case(&prefs.preferred_audio))
    };
    let within = max_height.is_none_or(|m| height(c.quality).is_none_or(|h| h <= m));
    (server, !is_embed(c), audio, within)
}

/// Downloadable web candidates in the order to try: the player's ranking (`tk_core::sources::rank` with the source
/// prefs), then the download prefs on top (server, direct over embed, audio, quality cap). Torrents, disabled
/// extensions and (unless `allow_embed`) embed pages are dropped.
pub fn order_candidates(
    candidates: Vec<SourceCandidate>,
    core: &SourcePrefs,
    enabled: impl Fn(&str) -> bool,
    prefs: &DownloadPrefs,
    max_height: Option<u32>,
    allow_embed: bool,
) -> Vec<SourceCandidate> {
    let mut v: Vec<SourceCandidate> = rank(candidates, core, &enabled)
        .into_iter()
        .map(|(c, _)| c)
        .filter(|c| c.kind == SourceKind::Web && c.web.is_some() && enabled(&c.ext_id))
        .filter(|c| allow_embed || !is_embed(c))
        .collect();
    v.sort_by_key(|c| std::cmp::Reverse(preference(c, prefs, max_height)));
    v
}

/// Whether a URL points at an HLS playlist.
pub fn is_hls_url(url: &str, container: Option<&str>) -> bool {
    if container.is_some_and(|c| c.eq_ignore_ascii_case("hls")) {
        return true;
    }
    let path = url.split(['?', '#']).next().unwrap_or("").to_ascii_lowercase();
    path.ends_with(".m3u8") || path.ends_with(".m3u")
}

/// File extension of a direct download from its URL (`mp4` unless the URL says `mkv` / `webm` / `avi` / `mov`).
pub fn direct_ext(url: &str) -> &'static str {
    let path = url.split(['?', '#']).next().unwrap_or("").to_ascii_lowercase();
    for ext in ["mkv", "webm", "avi", "mov", "m4v"] {
        if path.ends_with(&format!(".{ext}")) {
            return match ext {
                "mkv" => "mkv",
                "webm" => "webm",
                "avi" => "avi",
                "mov" => "mov",
                _ => "m4v",
            };
        }
    }
    "mp4"
}

/// The engine job of one episode (`.ts` for HLS, remuxed to `.mp4` when ffmpeg exists).
pub fn episode_job(dir: &Path, media_id: &str, title: &str, episode: u32, url: &str, headers: Vec<(String, String)>, hls: bool) -> Job {
    let (kind, ext) = if hls { (KIND_HLS, "ts") } else { (KIND_MP4, direct_ext(url)) };
    Job {
        media_id: media_id.to_owned(),
        episode: Some(episode),
        kind: kind.to_owned(),
        url: url.to_owned(),
        headers,
        target: files::episode_target(dir, title, episode, ext),
    }
}

/// What to download for one episode: `(title, poster, stream, hls, source key)`.
struct Resolved {
    title: String,
    poster: Option<String>,
    stream: StreamVm,
    hls: bool,
    source_id: Option<String>,
}

fn first_stream(pv: PlaybackVm, source_id: Option<String>) -> UiResult<Resolved> {
    let PlaybackVm { title, poster, streams, .. } = pv;
    let stream = streams.into_iter().next().ok_or_else(|| UiError::new(ErrorKind::NoSource, ""))?;
    if stream.provider == LOCAL_PROVIDER {
        return Err(other("this episode is already on disk"));
    }
    let hls = is_hls_url(&stream.url, None);
    Ok(Resolved { title, poster, stream, hls, source_id })
}

async fn resolve(b: &LiveBackend, media_id: &str, episode: u32, req: &DownloadRequestVm, settings: &Settings) -> UiResult<Resolved> {
    if let Some(sid) = req.source_id.as_deref() {
        if let Some((_, SourceKind::Torrent, _)) = parse_source_key(sid) {
            return Err(other("torrent sources download through the torrent client"));
        }
        let pv = if super::ext_catalog::is_provider_id(media_id) {
            Backend::playback(b, media_id, episode).await?
        } else {
            Backend::playback_source(b, media_id, episode, sid).await?
        };
        return first_stream(pv, Some(sid.to_owned()));
    }
    if super::ext_catalog::is_provider_id(media_id) {
        return first_stream(Backend::playback(b, media_id, episode).await?, None);
    }
    let prefs = settings.downloads.clone().sanitized();
    let (_, media) = b.media(media_id).await?;
    let candidates = b.episode_candidates(media_id, episode, &media, false).await;
    let enabled: HashMap<String, bool> =
        b.ext.installed().into_iter().map(|e| (e.id, e.enabled && e.compat != Compat::Unsupported)).collect();
    let cap = req.quality.as_deref().and_then(|q| q.trim_end_matches(['p', 'P']).parse::<u32>().ok());
    let ordered = order_candidates(
        candidates,
        &super::sources::core_prefs(&settings.sources),
        |id| enabled.get(id).copied().unwrap_or(false),
        &prefs,
        cap,
        settings.server.sniffer_enabled,
    );
    let mut last_err = None;
    for c in ordered {
        let key = source_key(&c);
        if is_embed(&c) {
            // The sniffer turns the embed page into its media URL (worker k).
            match Backend::playback_source(b, media_id, episode, &key).await {
                Ok(pv) => match first_stream(pv, Some(key)) {
                    Ok(r) => return Ok(r),
                    Err(e) => last_err = Some(e),
                },
                Err(e) => last_err = Some(e),
            }
            continue;
        }
        let hls = is_hls_url(c.web.as_ref().map_or("", |w| w.url.as_str()), c.container.as_deref());
        if let Some(s) = c.to_stream() {
            return Ok(Resolved {
                title: media.title.clone(),
                poster: media.cover.clone(),
                stream: super::sources::stream_vm(s),
                hls,
                source_id: Some(key),
            });
        }
    }
    Err(last_err.unwrap_or_else(|| UiError::new(ErrorKind::NoSource, format!("no downloadable source for episode {episode}"))))
}

/// A row of the episode that is done or on its way (enqueueing twice returns it instead of a duplicate).
fn existing_row(store: &LocalStore, media_id: &str, episode: u32) -> Option<i64> {
    store
        .downloads_for(media_id)
        .ok()?
        .into_iter()
        .find(|r| r.episode == Some(episode) && counts_as_have(DownloadStateVm::from_id(&r.state)))
        .map(|r| r.id)
}

// ---- view-models --------------------------------------------------------------------------------------------------

/// 0.0..=1.0 from the segment / page checkpoint when known, else the bytes.
pub fn fraction(state: DownloadStateVm, done: u64, total: Option<u64>, seg_done: u32, seg_total: Option<u32>) -> f64 {
    if state == DownloadStateVm::Completed {
        return 1.0;
    }
    if let Some(t) = seg_total.filter(|t| *t > 0) {
        return (f64::from(seg_done) / f64::from(t)).clamp(0.0, 1.0);
    }
    match total.filter(|t| *t > 0) {
        Some(t) => (done as f64 / t as f64).clamp(0.0, 1.0),
        None => 0.0,
    }
}

fn row_vm(r: &DownloadRow, live: Option<&Progress>) -> DownloadVm {
    let state = DownloadStateVm::from_id(&r.state);
    let bytes_done = live.map_or(r.bytes_done, |p| p.bytes_done.max(r.bytes_done));
    let bytes_total = live.and_then(|p| p.bytes_total).or(r.bytes_total);
    DownloadVm {
        id: r.id,
        media_id: r.media_id.clone(),
        title: r.title.clone(),
        poster: r.poster.clone(),
        episode: r.episode,
        chapter: r.chapter.clone(),
        kind: DownloadKindVm::from_id(&r.kind),
        state,
        progress: fraction(state, bytes_done, bytes_total, r.segments_done, r.segments_total),
        bytes_done,
        bytes_total,
        speed_bps: if state == DownloadStateVm::Downloading { live.map_or(0, |p| p.speed_bps) } else { 0 },
        eta_secs: if state == DownloadStateVm::Downloading { live.and_then(|p| p.eta_secs) } else { None },
        error: r.error.clone().filter(|_| matches!(state, DownloadStateVm::Failed | DownloadStateVm::Broken)),
        path: r.path.clone(),
        added_unix: r.added_at,
        completed_unix: r.completed_at,
    }
}

/// Negative, stable row id of a mirrored torrent (FNV-1a of its id).
pub fn torrent_row_id(torrent_id: &str) -> i64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in torrent_id.as_bytes() {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    -((h & 0x3fff_ffff_ffff_ffff) as i64) - 1
}

pub fn torrent_state(state: TorrentStateVm, progress: f64) -> DownloadStateVm {
    match state {
        TorrentStateVm::Initializing => DownloadStateVm::Queued,
        TorrentStateVm::Downloading => DownloadStateVm::Downloading,
        TorrentStateVm::Seeding => DownloadStateVm::Completed,
        TorrentStateVm::Paused if progress >= 1.0 => DownloadStateVm::Completed,
        TorrentStateVm::Paused => DownloadStateVm::Paused,
        TorrentStateVm::Error => DownloadStateVm::Failed,
    }
}

fn torrent_vm(item: &TorrentLibraryItemVm, speed: Option<(u64, Option<u64>)>, error: Option<String>) -> DownloadVm {
    let state = torrent_state(item.state, item.progress);
    let title = item.series.clone().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| item.name.clone());
    DownloadVm {
        id: torrent_row_id(&item.id),
        media_id: item.media_id.clone().unwrap_or_default(),
        title,
        poster: item.poster.clone(),
        episode: item.episode,
        chapter: None,
        kind: DownloadKindVm::Torrent,
        state,
        progress: if state == DownloadStateVm::Completed { 1.0 } else { item.progress.clamp(0.0, 1.0) },
        bytes_done: (item.size_bytes as f64 * item.progress.clamp(0.0, 1.0)) as u64,
        bytes_total: Some(item.size_bytes).filter(|s| *s > 0),
        speed_bps: speed.map_or(0, |s| s.0),
        eta_secs: speed.and_then(|s| s.1),
        error,
        path: Some(item.path.clone()).filter(|p| !p.is_empty()),
        added_unix: item.added_unix,
        completed_unix: None,
    }
}

/// Kept torrents of the torrent client (only when it is running: listing never starts it).
async fn torrent_rows(b: &LiveBackend) -> Vec<DownloadVm> {
    if b.torrent.running().is_none() {
        return Vec::new();
    }
    let Ok(items) = b.torrent.library().await else { return Vec::new() };
    let mut out = Vec::new();
    for item in items.iter().filter(|i| i.persist) {
        let active = matches!(item.state, TorrentStateVm::Downloading | TorrentStateVm::Initializing);
        let stats = if active || item.state == TorrentStateVm::Error { b.torrent.stats(&item.id).await.ok() } else { None };
        let speed = stats.as_ref().filter(|_| active).map(|s| (s.down_bps, s.eta_secs));
        let error = stats.and_then(|s| s.error).filter(|_| item.state == TorrentStateVm::Error);
        out.push(torrent_vm(item, speed, error));
    }
    out
}

/// The torrent id behind a mirrored (negative) row id.
async fn torrent_for(b: &LiveBackend, id: i64) -> UiResult<String> {
    let items = if b.torrent.running().is_some() { b.torrent.library().await? } else { Vec::new() };
    items.into_iter().find(|i| torrent_row_id(&i.id) == id).map(|i| i.id).ok_or_else(|| other(format!("download {id} not found")))
}

fn media_vm(id: &str, title: &str, poster: Option<&str>) -> MediaVm {
    MediaVm { id: id.to_owned(), title: title.to_owned(), cover: poster.map(str::to_owned), color: None, subtitle: String::new(), score: None }
}

fn local_vm(r: &LocalItemRow) -> LocalItemVm {
    let path = Path::new(&r.path);
    LocalItemVm {
        id: r.id,
        path: r.path.clone(),
        file_name: r.file_name.clone(),
        size_bytes: r.size_bytes,
        media: r.media_id.as_deref().map(|m| media_vm(m, r.title.as_deref().unwrap_or(&r.file_name), r.poster.as_deref())),
        episode: r.episode,
        added_unix: r.added_at,
        playable: path.is_file() && library::is_library_video(path),
    }
}

fn run_vm(r: &AutoRunRow) -> AutoDownloadRunVm {
    AutoDownloadRunVm { ran_unix: r.ran_at, checked: r.checked, enqueued: r.enqueued, skipped: r.skipped.clone() }
}

// ---- Backend calls ------------------------------------------------------------------------------------------------

/// Called by `LiveBackend::playback` first: a completed download or local-library file of `episode` plays offline
/// (`downloads.local-library.play-offline`). `Ok(None)` = nothing usable on disk (the online path follows). Never
/// blocks on the network for long (the title lookup is bounded).
pub async fn local_playback(b: &LiveBackend, media_id: &str, episode: u32) -> UiResult<Option<PlaybackVm>> {
    let Some((path, title, poster)) = local_file(&b.store, media_id, episode) else { return Ok(None) };
    let (start_secs, episodes_total) = match super::media_ref(media_id) {
        Some(r) => {
            let total = match tokio::time::timeout(OFFLINE_TITLE_TIMEOUT, b.media(media_id)).await {
                Ok(Ok((_, m))) => m.episodes,
                _ => None,
            };
            (b.start_secs(&r, episode), total)
        }
        None => (0, None),
    };
    b.remember_title(media_id, &title, poster.as_deref());
    Ok(Some(PlaybackVm {
        title,
        poster,
        episodes_total,
        start_secs,
        streams: vec![StreamVm { url: path, headers: Vec::new(), provider: LOCAL_PROVIDER.to_owned(), quality: None, subtitles: Vec::new() }],
    }))
}

/// `(path, title, poster)` of a healthy file of the episode: a completed download first, then the local library.
fn local_file(store: &LocalStore, media_id: &str, episode: u32) -> Option<(String, String, Option<String>)> {
    if let Ok(Some(r)) = store.completed_download_for(media_id, episode)
        && let Some(p) = r.path.clone()
        && files::check(&r.kind, Path::new(&p), r.bytes_total, r.segments_total).is_none()
    {
        return Some((p, r.title, r.poster));
    }
    if let Ok(Some(item)) = store.local_item_for(media_id, episode)
        && Path::new(&item.path).is_file()
    {
        let title = item.title.clone().unwrap_or_else(|| item.file_name.clone());
        return Some((item.path, title, item.poster));
    }
    None
}

pub async fn downloads(b: &LiveBackend) -> UiResult<Vec<DownloadVm>> {
    let rows = b.store.downloads(true).map_err(store_err)?;
    let live: HashMap<i64, Progress> = b.downloads.engine.progress().into_iter().map(|p| (p.id, p)).collect();
    let mut out: Vec<DownloadVm> = rows.iter().map(|r| row_vm(r, live.get(&r.id))).collect();
    out.extend(torrent_rows(b).await);
    // Active first (queue order kept), then the history.
    out.sort_by_key(|d| !d.state.is_active());
    Ok(out)
}

pub async fn enqueue(b: &LiveBackend, media_id: &str, episode: u32, req: &DownloadRequestVm) -> UiResult<i64> {
    if let Some(id) = existing_row(&b.store, media_id, episode) {
        return Ok(id);
    }
    let settings = settings_store::load(&b.settings_file);
    let prefs = settings.downloads.clone().sanitized();
    let r = resolve(b, media_id, episode, req, &settings).await?;
    let job = episode_job(&b.downloads.engine.dir(), media_id, &r.title, episode, &r.stream.url, r.stream.headers.clone(), r.hls);
    let meta = Meta {
        title: r.title,
        poster: r.poster,
        chapter: None,
        source_id: r.source_id,
        quality: req.quality.clone(),
        audio_lang: req.audio_lang.clone().or(Some(prefs.preferred_audio)),
        sub_lang: req.sub_lang.clone(),
    };
    b.downloads.engine.enqueue_with(job, meta).map_err(dl_err)
}

/// Queue several episodes; a failure of one does not stop the rest (all failing returns the first error).
pub async fn enqueue_many(b: &LiveBackend, media_id: &str, episodes: &[u32], req: &DownloadRequestVm) -> UiResult<Vec<i64>> {
    let mut ids = Vec::new();
    let mut first_err = None;
    for ep in episodes {
        match enqueue(b, media_id, *ep, req).await {
            Ok(id) => ids.push(id),
            Err(e) => {
                tracing::warn!(episode = ep, detail = %e.detail, "episode not queued");
                first_err.get_or_insert(e);
            }
        }
    }
    match first_err {
        Some(e) if ids.is_empty() => Err(e),
        _ => Ok(ids),
    }
}

pub async fn pause(b: &LiveBackend, id: i64) -> UiResult<()> {
    if id < 0 {
        return b.torrent.pause(&torrent_for(b, id).await?).await;
    }
    b.downloads.engine.pause(id).map_err(dl_err)
}

/// Back to the queue: paused / failed rows keep their checkpoint, cancelled / broken ones start over.
pub async fn resume(b: &LiveBackend, id: i64) -> UiResult<()> {
    if id < 0 {
        return b.torrent.resume(&torrent_for(b, id).await?).await;
    }
    b.downloads.engine.resume(id).map_err(dl_err)
}

pub async fn cancel(b: &LiveBackend, id: i64, delete_file: bool) -> UiResult<()> {
    if id < 0 {
        return b.torrent.remove(&torrent_for(b, id).await?, delete_file).await;
    }
    b.downloads.engine.cancel(id, delete_file).map_err(dl_err)
}

pub async fn remove(b: &LiveBackend, id: i64, delete_file: bool) -> UiResult<()> {
    if id < 0 {
        return b.torrent.remove(&torrent_for(b, id).await?, delete_file).await;
    }
    b.downloads.engine.remove(id, delete_file).await.map_err(dl_err)
}

/// Finished / failed / cancelled rows go from the history (files stay).
pub async fn clear_history(b: &LiveBackend) -> UiResult<u32> {
    b.store.clear_download_history().map(|n| n as u32).map_err(store_err)
}

/// `OPEN_ROOT_FOLDER` (0) opens the download folder; a row opens its file's folder (a chapter its own folder).
pub async fn open_folder(b: &LiveBackend, id: i64) -> UiResult<()> {
    if id < 0 {
        return b.torrent.open_folder(&torrent_for(b, id).await?).await;
    }
    let dir = if id == OPEN_ROOT_FOLDER {
        b.downloads.engine.dir()
    } else {
        let r = b.store.download(id).map_err(store_err)?.ok_or_else(|| other(format!("download {id} not found")))?;
        folder_of(&r).unwrap_or_else(|| b.downloads.engine.dir())
    };
    let _ = std::fs::create_dir_all(&dir);
    open_in_file_manager(&dir)
}

/// Folder that holds a row's data.
fn folder_of(r: &DownloadRow) -> Option<PathBuf> {
    let p = PathBuf::from(r.path.as_deref()?);
    if p.is_dir() { Some(p) } else { p.parent().map(Path::to_path_buf) }
}

/// `xdg-open` / `explorer` / `open` on a folder (no shell, no console window).
fn open_in_file_manager(dir: &Path) -> UiResult<()> {
    #[cfg(windows)]
    let mut cmd = {
        use std::os::windows::process::CommandExt;
        let mut cmd = std::process::Command::new("explorer");
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        cmd
    };
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("open");
    #[cfg(not(any(windows, target_os = "macos")))]
    let mut cmd = std::process::Command::new("xdg-open");
    cmd.arg(dir);
    match cmd.spawn() {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            Ok(())
        }
        Err(e) => Err(other(format!("could not open {}: {e}", dir.display()))),
    }
}

pub async fn storage(b: &LiveBackend) -> UiResult<DownloadStorageVm> {
    let prefs = settings_store::load(&b.settings_file).downloads.sanitized();
    let dir = b.downloads.engine.dir();
    let used_bytes = b.store.completed_download_bytes().map_err(store_err)?;
    let items = b.store.downloads_in_state("completed").map_err(store_err)?.len() as u32;
    let probe = dir.clone();
    let free_bytes = tokio::task::spawn_blocking(move || super::system::free_bytes(&probe)).await.ok().flatten();
    Ok(DownloadStorageVm { dir: dir.display().to_string(), used_bytes, limit_bytes: limit_bytes(prefs.storage_limit_gib), free_bytes, items })
}

pub async fn local_library(b: &LiveBackend) -> UiResult<Vec<LocalItemVm>> {
    let rows = b.store.local_items().map_err(store_err)?;
    Ok(rows.iter().map(local_vm).collect())
}

/// Scan `dir` (or every library folder) and import the videos found.
pub async fn library_scan(b: &LiveBackend, dir: Option<&str>) -> UiResult<LibraryScanVm> {
    let roots: Vec<PathBuf> = match dir.map(str::trim).filter(|d| !d.is_empty()) {
        Some(d) => vec![PathBuf::from(d)],
        None => settings_store::load(&b.settings_file).downloads.sanitized().library_dirs.iter().map(PathBuf::from).collect(),
    };
    if roots.is_empty() {
        return Err(other("no library folder: add one in Settings > Downloads or pick a folder to scan"));
    }
    if let Some(missing) = roots.iter().find(|r| !r.exists()) {
        return Err(other(format!("{} does not exist", missing.display())));
    }
    import_paths(&ctx(b), roots).await
}

/// Import files (or folders) the user picked.
pub async fn library_import(b: &LiveBackend, paths: &[String]) -> UiResult<LibraryScanVm> {
    let paths: Vec<PathBuf> = paths.iter().map(|p| PathBuf::from(p.trim())).filter(|p| !p.as_os_str().is_empty()).collect();
    if paths.is_empty() {
        return Ok(LibraryScanVm::default());
    }
    import_paths(&ctx(b), paths).await
}

/// Walk `paths` (files and folders), add every video to `local_library` and match the new ones by release name +
/// title search (`downloads.local-library.import` / `.scan`).
async fn import_paths(c: &Ctx, paths: Vec<PathBuf>) -> UiResult<LibraryScanVm> {
    let found: Vec<(PathBuf, u64)> = tokio::task::spawn_blocking(move || {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for p in paths {
            for f in library::walk_videos(&p) {
                if seen.insert(f.clone()) {
                    let size = std::fs::metadata(&f).map(|m| m.len()).unwrap_or(0);
                    out.push((f, size));
                }
            }
        }
        out
    })
    .await
    .map_err(|e| other(format!("library scan task: {e}")))?;
    let mut vm = LibraryScanVm::default();
    let mut matches: HashMap<String, Option<(String, String, Option<String>)>> = HashMap::new();
    let now = now_unix();
    for (path, size) in found {
        vm.scanned += 1;
        let file_name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let row = LocalItemRow {
            path: path.to_string_lossy().into_owned(),
            file_name: file_name.clone(),
            size_bytes: size,
            added_at: now,
            ..LocalItemRow::default()
        };
        let (id, inserted) = c.store.upsert_local_item(&row).map_err(store_err)?;
        if inserted {
            vm.added += 1;
        }
        if c.store.local_item(id).ok().flatten().is_some_and(|r| r.media_id.is_some()) {
            vm.matched += 1;
            continue;
        }
        let guess = library::guess(&path);
        let Some(title) = guess.title.clone() else {
            vm.unmatched.push(file_name);
            continue;
        };
        let key = library::normalize_title(&title);
        if !matches.contains_key(&key) {
            let m = match_title(c, &title).await;
            matches.insert(key.clone(), m);
        }
        match matches.get(&key).cloned().flatten() {
            Some((media_id, name, poster)) => {
                c.store.set_local_match(id, &media_id, &name, poster.as_deref(), guess.episode).map_err(store_err)?;
                vm.matched += 1;
            }
            None => vm.unmatched.push(file_name),
        }
    }
    Ok(vm)
}

/// Best catalogue match of a guessed title: `(media id, title, poster)` when plausible.
async fn match_title(c: &Ctx, title: &str) -> Option<(String, String, Option<String>)> {
    let page = c.router.search(MediaKind::Anime, title, 1).await.ok()?;
    page.items
        .iter()
        .map(|m| {
            let best = std::iter::once(m.title.as_str())
                .chain(m.titles.values().map(String::as_str))
                .map(|t| library::title_score(title, t))
                .max()
                .unwrap_or(0);
            (best, m)
        })
        .filter(|(s, _)| *s >= MATCH_MIN_SCORE)
        .max_by_key(|(s, _)| *s)
        .map(|(_, m)| (super::media_id(&m.media_ref), m.title.clone(), m.cover.clone()))
        .filter(|(id, _, _)| !id.is_empty())
}

/// Assign a title + episode to a local file (`downloads.local-library.match`).
pub async fn local_match(b: &LiveBackend, item_id: i64, media_id: &str, episode: u32) -> UiResult<()> {
    let item = b.store.local_item(item_id).map_err(store_err)?.ok_or_else(|| other(format!("local file {item_id} not found")))?;
    let (title, poster) = match b.media(media_id).await {
        Ok((_, m)) => (m.title, m.cover),
        Err(e) => {
            tracing::debug!(detail = %e.detail, "title lookup for a local match failed");
            (item.title.clone().unwrap_or_else(|| media_id.to_owned()), item.poster.clone())
        }
    };
    let episode = Some(episode).filter(|e| *e > 0);
    b.store.set_local_match(item_id, media_id, &title, poster.as_deref(), episode).map_err(store_err)
}

pub async fn local_remove(b: &LiveBackend, item_id: i64, delete_file: bool) -> UiResult<()> {
    let item = b.store.local_item(item_id).map_err(store_err)?.ok_or_else(|| other(format!("local file {item_id} not found")))?;
    if delete_file {
        let p = PathBuf::from(&item.path);
        if p.is_file()
            && let Err(e) = std::fs::remove_file(&p)
        {
            return Err(other(format!("could not delete {}: {e}", p.display())));
        }
    }
    b.store.delete_local_item(item_id).map_err(store_err)
}

/// "Check now": one auto-download pass with the current prefs (also when the automatic rule is off).
pub async fn auto_check_now(b: &LiveBackend) -> UiResult<AutoDownloadRunVm> {
    run_auto(&ctx(b)).await.map(|r| run_vm(&r))
}

pub async fn auto_last_run(b: &LiveBackend) -> UiResult<Option<AutoDownloadRunVm>> {
    Ok(b.store.last_auto_run().map_err(store_err)?.as_ref().map(run_vm))
}

/// Clears the busy flag of a pass when it ends (also on early returns).
struct BusyGuard<'a>(&'a AtomicBool);

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// One auto-download pass (`downloads.auto.new-episodes`): for each title of the local "Watching" list, the aired
/// episodes after the watched progress that are not on disk / queued, at most `AUTO_MAX_PER_TITLE`, each with the
/// best direct web source for the profile. Stops before queueing when the storage cap is reached.
async fn run_auto(c: &Ctx) -> UiResult<AutoRunRow> {
    if c.shared.auto_busy.swap(true, Ordering::SeqCst) {
        return Err(other("an automatic check is already running"));
    }
    let _guard = BusyGuard(&c.shared.auto_busy);
    let settings = c.settings();
    let prefs = settings.downloads.clone().sanitized();
    let now = now_unix();
    let mut run = AutoRunRow { ran_at: now, ..AutoRunRow::default() };
    let used = c.store.completed_download_bytes().unwrap_or(0);
    let limit = limit_bytes(prefs.storage_limit_gib);
    if storage_limit_reached(used, limit) {
        run.skipped.push(format!("storage limit reached ({} GiB)", prefs.storage_limit_gib));
        c.store.insert_auto_run(&run).map_err(store_err)?;
        return Ok(run);
    }
    let entries = c.store.list_entries(tk_core::lists::sync::LOCAL, Some("watching")).map_err(store_err)?;
    let today = format_unix_utc(now).get(..10).unwrap_or_default().to_owned();
    let rows = c.store.downloads(false).unwrap_or_default();
    let local = c.store.local_items().unwrap_or_default();
    let enabled: HashMap<String, bool> =
        c.host.list().into_iter().map(|e| (e.id, e.enabled && e.compat != Compat::Unsupported)).collect();
    let core = super::sources::core_prefs(&settings.sources);
    for entry in entries {
        run.checked += 1;
        let label = if entry.title.trim().is_empty() { entry.media_id.clone() } else { entry.title.clone() };
        let Some(r) = super::media_ref(&entry.media_id) else {
            run.skipped.push(format!("{label}: unsupported id"));
            continue;
        };
        let eps = match c.router.episodes(&r).await {
            Ok(v) => v,
            Err(e) => {
                run.skipped.push(format!("{label}: {e}"));
                continue;
            }
        };
        let aired: Vec<u32> = eps.iter().filter(|e| is_aired(e.air_date.as_deref(), &today)).map(|e| e.number).collect();
        let mut have: Vec<u32> = rows
            .iter()
            .filter(|d| d.media_id == entry.media_id && counts_as_have(DownloadStateVm::from_id(&d.state)))
            .filter_map(|d| d.episode)
            .collect();
        have.extend(local.iter().filter(|l| l.media_id.as_deref() == Some(entry.media_id.as_str())).filter_map(|l| l.episode));
        let plan = plan_new_episodes(entry.progress, &aired, &have, AUTO_MAX_PER_TITLE);
        if plan.is_empty() {
            continue;
        }
        let media = match c.router.media(MediaKind::Anime, &r).await {
            Ok(Some(m)) => m,
            Ok(None) => {
                run.skipped.push(format!("{label}: not found"));
                continue;
            }
            Err(e) => {
                run.skipped.push(format!("{label}: {e}"));
                continue;
            }
        };
        for ep in plan {
            let mut cands = c.host.resolve_candidates(&media, ep).await;
            for cand in &mut cands {
                cand.reliability = c.store.reliability(&cand.ext_id, &cand.label).unwrap_or(INITIAL_RELIABILITY);
            }
            let ordered = order_candidates(
                cands,
                &core,
                |id| enabled.get(id).copied().unwrap_or(false),
                &prefs,
                prefs.auto_profile.max_height(),
                false,
            );
            let Some((cand, stream)) = ordered.into_iter().find_map(|c| c.to_stream().map(|s| (c, s))) else {
                run.skipped.push(format!("{label} E{ep}: no downloadable source"));
                continue;
            };
            let hls = is_hls_url(&stream.url, cand.container.as_deref());
            let job = episode_job(
                &c.engine.dir(),
                &entry.media_id,
                &media.title,
                ep,
                &stream.url,
                stream.headers.clone().into_iter().collect(),
                hls,
            );
            let meta = Meta {
                title: media.title.clone(),
                poster: media.cover.clone(),
                chapter: None,
                source_id: Some(source_key(&cand)),
                quality: prefs.auto_profile.quality().map(str::to_owned),
                audio_lang: Some(prefs.preferred_audio.clone()),
                sub_lang: None,
            };
            match c.engine.enqueue_with(job, meta) {
                Ok(_) => run.enqueued += 1,
                Err(e) => run.skipped.push(format!("{label} E{ep}: {e}")),
            }
        }
    }
    c.store.insert_auto_run(&run).map_err(store_err)?;
    Ok(run)
}

/// Re-check a row on disk (`downloads.integrity-check`): missing / truncated -> `broken`; a repaired file -> back to
/// `completed`.
pub async fn verify(b: &LiveBackend, id: i64) -> UiResult<DownloadVm> {
    if id < 0 {
        return torrent_rows(b).await.into_iter().find(|d| d.id == id).ok_or_else(|| other(format!("download {id} not found")));
    }
    let engine = b.downloads.engine.clone();
    let row = tokio::task::spawn_blocking(move || engine.verify(id))
        .await
        .map_err(|e| other(format!("verify task: {e}")))?
        .map_err(dl_err)?;
    Ok(row_vm(&row, None))
}

// ---- workers l / m ------------------------------------------------------------------------------------------------

/// Target of a manga chapter folder (`novel = false`) or a novel chapter file (`novel = true`, `.json`) under the
/// download folder in use. `ext_id` / `chapter_id` are recovered from it when the chapter finishes.
pub fn chapter_target(b: &LiveBackend, media_id: &str, ext_id: &str, chapter_id: &str, novel: bool) -> PathBuf {
    let t = files::chapter_target(&b.downloads.engine.dir(), media_id, ext_id, chapter_id);
    if novel { t.with_extension("json") } else { t }
}

/// Queue chapter (or episode) jobs built by workers l / m; the title comes from the titles opened this session.
pub async fn enqueue_jobs(b: &LiveBackend, jobs: Vec<Job>) -> UiResult<Vec<i64>> {
    let with: Vec<(Job, Meta)> = jobs
        .into_iter()
        .map(|j| {
            let (title, poster) = b.titles.lock().ok().and_then(|t| t.get(&j.media_id).cloned()).unwrap_or_else(|| (j.media_id.clone(), None));
            let chapter = j.target.file_stem().map(|s| s.to_string_lossy().into_owned()).filter(|_| j.episode.is_none());
            (j, Meta { title, poster, chapter, ..Meta::default() })
        })
        .collect();
    enqueue_jobs_with(b, with).await
}

/// Queue jobs with their display data. A job whose target is already queued or downloaded returns that row.
pub async fn enqueue_jobs_with(b: &LiveBackend, jobs: Vec<(Job, Meta)>) -> UiResult<Vec<i64>> {
    let rows = b.store.downloads(false).map_err(store_err)?;
    let mut ids = Vec::with_capacity(jobs.len());
    for (job, meta) in jobs {
        let target = job.target.to_string_lossy().into_owned();
        if let Some(r) = rows.iter().find(|r| {
            r.kind == job.kind && r.path.as_deref() == Some(target.as_str()) && counts_as_have(DownloadStateVm::from_id(&r.state))
        }) {
            ids.push(r.id);
            continue;
        }
        ids.push(b.downloads.engine.enqueue_with(job, meta).map_err(dl_err)?);
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tk_core::sources::{SourceMode, WebInfo};

    fn cand(ext: &str, label: &str, q: Quality, dubs: &[&str], embed: bool) -> SourceCandidate {
        SourceCandidate {
            ext_id: ext.into(),
            ext_name: ext.to_uppercase(),
            kind: SourceKind::Web,
            label: label.into(),
            quality: q,
            container: Some("HLS".into()),
            subs: vec!["es".into()],
            dubs: dubs.iter().map(|d| (*d).to_owned()).collect(),
            reliability: 0.9,
            note: None,
            web: Some(WebInfo {
                url: format!("https://{label}.test/a.m3u8"),
                headers: Default::default(),
                latency_ms: None,
                subtitles: Vec::new(),
                embed,
            }),
            torrent: None,
            tags: Default::default(),
        }
    }

    fn core() -> SourcePrefs {
        SourcePrefs { mode: SourceMode::Auto, prefer: None, min_quality: Quality::P480, sub_lang: "es".into(), dub_lang: "ja".into() }
    }

    #[test]
    fn candidates_follow_the_download_prefs() {
        let list = vec![
            cand("a", "Alpha", Quality::P1080, &["ja"], false),
            cand("b", "Beta", Quality::P720, &["es"], false),
            cand("c", "Embed", Quality::P1080, &["ja"], true),
        ];
        let prefs = DownloadPrefs::default();
        let all = |_: &str| true;
        let v = order_candidates(list.clone(), &core(), all, &prefs, None, false);
        assert_eq!(v.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(), vec!["Alpha", "Beta"]);
        // Spanish audio first.
        let es = DownloadPrefs { preferred_audio: "es".into(), ..DownloadPrefs::default() };
        assert_eq!(order_candidates(list.clone(), &core(), all, &es, None, false)[0].label, "Beta");
        // A preferred server wins over everything else.
        let srv = DownloadPrefs { preferred_server: Some("b".into()), ..DownloadPrefs::default() };
        assert_eq!(order_candidates(list.clone(), &core(), all, &srv, None, false)[0].label, "Beta");
        // Data saver: the 720p source is within the cap.
        assert_eq!(order_candidates(list.clone(), &core(), all, &prefs, Some(720), false)[0].label, "Beta");
        // Embeds only when allowed, and after direct streams.
        let with_embed = order_candidates(list.clone(), &core(), all, &prefs, None, true);
        assert_eq!(with_embed.last().map(|c| c.label.as_str()), Some("Embed"));
        // Disabled extensions are dropped.
        let only_a = |id: &str| id == "a";
        assert_eq!(order_candidates(list, &core(), only_a, &prefs, None, true).len(), 1);
    }

    #[test]
    fn urls_and_jobs() {
        assert!(is_hls_url("https://x/a/index.M3U8?token=1", None));
        assert!(is_hls_url("https://x/a/video", Some("HLS")));
        assert!(!is_hls_url("https://x/a/video.mp4", Some("MP4")));
        assert_eq!(direct_ext("https://x/v.mkv?x=1"), "mkv");
        assert_eq!(direct_ext("https://x/v"), "mp4");
        let j = episode_job(Path::new("/d"), "21", "One Piece", 3, "https://x/a.m3u8", vec![], true);
        assert_eq!(j.kind, KIND_HLS);
        assert_eq!(j.episode, Some(3));
        assert_eq!(j.target.extension().and_then(|e| e.to_str()), Some("ts"));
        let m = episode_job(Path::new("/d"), "21", "One Piece", 3, "https://x/a.mp4", vec![], false);
        assert_eq!(m.kind, KIND_MP4);
        assert_eq!(m.target.extension().and_then(|e| e.to_str()), Some("mp4"));
    }

    #[test]
    fn schedule_and_cleanup() {
        assert!(auto_due(None, 1_000, 30));
        assert!(!auto_due(Some(1_000), 1_000 + 29 * 60, 30));
        assert!(auto_due(Some(1_000), 1_000 + 30 * 60, 30));
        let row = |id: i64, watched: Option<i64>, bytes: u64| DownloadRow {
            id,
            state: "completed".into(),
            watched_at: watched,
            bytes_total: Some(bytes),
            ..DownloadRow::default()
        };
        let rows = vec![row(1, Some(100), 10), row(2, None, 10), row(3, Some(50), 10), row(4, Some(10 * 86_400), 10)];
        assert_eq!(watched_expired(&rows, 0, 20 * 86_400), Vec::<i64>::new());
        assert_eq!(watched_expired(&rows, 7, 20 * 86_400), vec![1, 3, 4]);
        // 40 bytes used, cap 25: the oldest watched (3, then 1) go; the unwatched one never does.
        assert_eq!(over_limit(&rows, 40, 25), vec![3, 1]);
        assert!(over_limit(&rows, 20, 25).is_empty());
    }

    #[test]
    fn progress_and_torrent_rows() {
        assert_eq!(fraction(DownloadStateVm::Completed, 0, None, 0, None), 1.0);
        assert_eq!(fraction(DownloadStateVm::Downloading, 50, Some(100), 0, None), 0.5);
        assert_eq!(fraction(DownloadStateVm::Downloading, 50, Some(100), 3, Some(4)), 0.75);
        assert_eq!(fraction(DownloadStateVm::Queued, 10, None, 0, None), 0.0);
        let a = torrent_row_id("abc");
        assert!(a < 0);
        assert_eq!(a, torrent_row_id("abc"));
        assert_ne!(a, torrent_row_id("abd"));
        assert_eq!(torrent_state(TorrentStateVm::Seeding, 1.0), DownloadStateVm::Completed);
        assert_eq!(torrent_state(TorrentStateVm::Paused, 0.4), DownloadStateVm::Paused);
        assert_eq!(torrent_state(TorrentStateVm::Paused, 1.0), DownloadStateVm::Completed);
        assert_eq!(torrent_state(TorrentStateVm::Error, 0.1), DownloadStateVm::Failed);
    }

    #[test]
    fn row_mapping() {
        let r = DownloadRow {
            id: 7,
            media_id: "21".into(),
            title: "One Piece".into(),
            episode: Some(3),
            kind: "hls".into(),
            state: "downloading".into(),
            bytes_done: 10,
            segments_done: 1,
            segments_total: Some(4),
            error: Some("old".into()),
            ..DownloadRow::default()
        };
        let live = Progress { id: 7, bytes_done: 40, bytes_total: Some(160), speed_bps: 20, eta_secs: Some(6) };
        let vm = row_vm(&r, Some(&live));
        assert_eq!(vm.kind, DownloadKindVm::Hls);
        assert_eq!(vm.state, DownloadStateVm::Downloading);
        assert_eq!(vm.bytes_done, 40);
        assert_eq!(vm.speed_bps, 20);
        assert_eq!(vm.progress, 0.25);
        assert_eq!(vm.error, None);
        let paused = row_vm(&DownloadRow { state: "paused".into(), ..r }, Some(&live));
        assert_eq!(paused.speed_bps, 0);
        assert_eq!(paused.eta_secs, None);
    }
}
