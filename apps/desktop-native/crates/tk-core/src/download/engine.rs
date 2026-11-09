//! The download queue (worker **h**; checklist `downloads.engine.hls`, `downloads.engine.mp4`, `downloads.resume`,
//! `downloads.queue`; classic `desktop/ipc/ipc-download-manager.cjs`, `runtime/download/{hls,file}-checkpoint.cjs`).
//!
//! * Rows live in `LocalStore::downloads`; the engine keeps only the running set, live speed / ETA and the control
//!   flag of each worker in memory. A scheduler task starts `queued` rows (oldest first) while fewer than
//!   `concurrent` run; it wakes on every enqueue / resume and every [`TICK`].
//! * HLS: master playlist -> variant by the row's `quality` -> media playlist -> segments fetched 4 at a time with
//!   the source headers, AES-128 decrypted, each written as its own file under `<target>.parts/` (a segment file
//!   exists only once complete: that is the resume checkpoint). At the end they are concatenated into `<target>`
//!   (`.ts`, `.mp4` for fMP4) and, when `ffmpeg` is on the PATH, remuxed to `.mp4` without re-encoding.
//! * MP4 / any direct file: `<target>.part` grows with `Range: bytes=<len>-` on resume; a body that turns out to be
//!   a playlist switches the row to HLS.
//! * Manga chapter: one page URL per line of `Job::url`, saved as `NNN.<ext>` in the chapter folder; novel chapter:
//!   `Job::url` is either an http(s) URL fetched as is or the chapter text itself (JSON list of paragraphs or plain
//!   text) written to the target file. Both mark worker l's `offline_chapters` when done.
//! * Pause keeps the checkpoint; cancel removes it; a crash leaves `downloading` rows that [`Engine::start`] puts
//!   back in the queue.
//! * Speed limit: one token bucket for the whole engine (`speed_kib`, 0 = unlimited).

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;

use super::files::{self, Damage};
use super::hls::{self, Parsed};
use super::{Job, KIND_HLS, KIND_MANGA, KIND_MP4, KIND_NOVEL};
use crate::error::{DownloadError, StoreError};
use crate::store::LocalStore;
use crate::store::downloads::{DownloadRow, OfflineChapterMark};

/// Scheduler period when nothing wakes it.
pub const TICK: Duration = Duration::from_secs(3);
/// HLS segments fetched in parallel inside one download.
pub const SEGMENT_PARALLELISM: usize = 4;
/// Attempts per segment / page before the download fails.
const ATTEMPTS: u32 = 3;
/// Progress is written to the store at most this often.
const SAVE_EVERY: Duration = Duration::from_secs(2);
/// Speed window.
const SPEED_WINDOW: Duration = Duration::from_secs(5);
/// Playlist nesting accepted (master -> media; some CDNs add one more hop).
const MAX_PLAYLIST_HOPS: usize = 3;
/// Per-request timeout of the engine's own requests (the shared client's 20 s is too short for big segments).
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

fn now_unix() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Display data of a new row (everything `Job` does not carry).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Meta {
    pub title: String,
    pub poster: Option<String>,
    pub chapter: Option<String>,
    pub source_id: Option<String>,
    /// `"1080p"`, `"best"`...: HLS variant choice.
    pub quality: Option<String>,
    pub audio_lang: Option<String>,
    pub sub_lang: Option<String>,
}

/// Live numbers of a running row (`Engine::progress`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Progress {
    pub id: i64,
    pub bytes_done: u64,
    pub bytes_total: Option<u64>,
    pub speed_bps: u64,
    pub eta_secs: Option<u64>,
}

const RUN: u8 = 0;
const PAUSE: u8 = 1;
const CANCEL: u8 = 2;
const STOP: u8 = 3;

/// Why a worker stopped early.
enum Halt {
    Paused,
    Cancelled,
    /// App shutting down: the row goes back to `queued` and resumes at the next start.
    Shutdown,
    Failed(DownloadError),
}

impl From<DownloadError> for Halt {
    fn from(e: DownloadError) -> Self {
        Self::Failed(e)
    }
}
impl From<reqwest::Error> for Halt {
    fn from(e: reqwest::Error) -> Self {
        Self::Failed(e.into())
    }
}
impl From<std::io::Error> for Halt {
    fn from(e: std::io::Error) -> Self {
        Self::Failed(e.into())
    }
}
impl From<StoreError> for Halt {
    fn from(e: StoreError) -> Self {
        Self::Failed(e.into())
    }
}

#[derive(Default)]
struct Live {
    done: u64,
    total: Option<u64>,
    samples: VecDeque<(Instant, u64)>,
}

/// Control block of one running row.
struct Control {
    cmd: AtomicU8,
    delete_file: AtomicBool,
    live: Mutex<Live>,
}

impl Control {
    fn new() -> Self {
        Self { cmd: AtomicU8::new(RUN), delete_file: AtomicBool::new(false), live: Mutex::new(Live::default()) }
    }

    fn check(&self) -> Result<(), Halt> {
        match self.cmd.load(Ordering::Relaxed) {
            PAUSE => Err(Halt::Paused),
            CANCEL => Err(Halt::Cancelled),
            STOP => Err(Halt::Shutdown),
            _ => Ok(()),
        }
    }

    fn set_totals(&self, done: u64, total: Option<u64>) {
        if let Ok(mut l) = self.live.lock() {
            l.done = done;
            if total.is_some() {
                l.total = total;
            }
        }
    }

    fn add(&self, n: u64) {
        if let Ok(mut l) = self.live.lock() {
            l.done += n;
            let now = Instant::now();
            l.samples.push_back((now, n));
            while l.samples.front().is_some_and(|(t, _)| now.duration_since(*t) > SPEED_WINDOW) {
                l.samples.pop_front();
            }
        }
    }

    fn snapshot(&self, id: i64) -> Progress {
        let (done, total, speed) = match self.live.lock() {
            Ok(l) => {
                let bytes: u64 = l.samples.iter().map(|(_, n)| n).sum();
                let span = l.samples.front().map_or(0.0, |(t, _)| t.elapsed().as_secs_f64()).max(1.0);
                (l.done, l.total, (bytes as f64 / span) as u64)
            }
            Err(_) => (0, None, 0),
        };
        Progress { id, bytes_done: done, bytes_total: total, speed_bps: speed, eta_secs: eta(done, total, speed) }
    }
}

/// Seconds left at `speed` bytes/s; `None` when unknown.
pub fn eta(done: u64, total: Option<u64>, speed: u64) -> Option<u64> {
    let total = total?;
    if speed == 0 || total <= done {
        return if total <= done { Some(0) } else { None };
    }
    Some((total - done).div_ceil(speed))
}

/// Shared token bucket (`speed_kib`, 0 = unlimited).
struct Limiter {
    kib: AtomicU32,
    state: tokio::sync::Mutex<(Instant, u64)>,
}

impl Limiter {
    async fn take(&self, n: u64) {
        let kib = self.kib.load(Ordering::Relaxed);
        if kib == 0 {
            return;
        }
        let rate = f64::from(kib) * 1024.0;
        let wait = {
            let mut s = self.state.lock().await;
            let now = Instant::now();
            if now.duration_since(s.0) > Duration::from_secs(2) {
                *s = (now, 0);
            }
            s.1 += n;
            let allowed = Duration::from_secs_f64(s.1 as f64 / rate);
            allowed.saturating_sub(now.duration_since(s.0))
        };
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
    }
}

struct Inner {
    client: reqwest::Client,
    store: Arc<LocalStore>,
    dir: Mutex<PathBuf>,
    concurrent: AtomicUsize,
    limiter: Limiter,
    running: Mutex<HashMap<i64, Arc<Control>>>,
    wake: tokio::sync::Notify,
    started: AtomicBool,
    stopped: AtomicBool,
}

/// The engine. Cheap to clone (one shared state).
#[derive(Clone)]
pub struct Engine {
    inner: Arc<Inner>,
}

impl Engine {
    /// No task is spawned here (the caller may not be inside a runtime yet); [`Engine::start`] does that.
    pub fn new(client: reqwest::Client, store: Arc<LocalStore>, dir: PathBuf, concurrent: u8) -> Self {
        Self {
            inner: Arc::new(Inner {
                client,
                store,
                dir: Mutex::new(dir),
                concurrent: AtomicUsize::new(usize::from(concurrent.max(1))),
                limiter: Limiter { kib: AtomicU32::new(0), state: tokio::sync::Mutex::new((Instant::now(), 0)) },
                running: Mutex::new(HashMap::new()),
                wake: tokio::sync::Notify::new(),
                started: AtomicBool::new(false),
                stopped: AtomicBool::new(false),
            }),
        }
    }

    pub fn store(&self) -> &Arc<LocalStore> {
        &self.inner.store
    }

    /// Download folder in use.
    pub fn dir(&self) -> PathBuf {
        self.inner.dir.lock().map(|d| d.clone()).unwrap_or_default()
    }

    /// New downloads go to `dir` (running ones keep their target).
    pub fn set_dir(&self, dir: PathBuf) {
        if let Ok(mut d) = self.inner.dir.lock() {
            *d = dir;
        }
    }

    /// `speed_kib` 0 = unlimited; `concurrent` is clamped to 1..=6.
    pub fn set_limits(&self, speed_kib: u32, concurrent: u8) {
        self.inner.limiter.kib.store(speed_kib, Ordering::Relaxed);
        self.inner.concurrent.store(usize::from(concurrent.clamp(1, 6)), Ordering::Relaxed);
        self.inner.wake.notify_one();
    }

    /// Start the scheduler once (idempotent; needs a tokio runtime, otherwise it is retried by the next call).
    /// Rows left `downloading` by a crash or a closed app go back to the queue first (`downloads.resume`).
    pub fn start(&self) {
        if tokio::runtime::Handle::try_current().is_err() {
            return;
        }
        if self.inner.started.swap(true, Ordering::SeqCst) {
            return;
        }
        match self.inner.store.downloads_in_state("downloading") {
            Ok(rows) => {
                for r in rows {
                    // Reader chapter rows (l / m) ran in their own task, which died with the app: mark them failed.
                    if is_reader_row(&r.kind, &r.url) {
                        let _ = self.inner.store.set_download_state(r.id, "failed", Some("interrupted"), None, None);
                        continue;
                    }
                    if let Err(e) = self.inner.store.set_download_state(r.id, "queued", None, None, None) {
                        tracing::warn!(id = r.id, error = %e, "could not requeue an interrupted download");
                    }
                }
            }
            Err(e) => tracing::warn!(error = %e, "could not read interrupted downloads"),
        }
        let inner = self.inner.clone();
        tokio::spawn(async move { scheduler(inner).await });
    }

    /// Queue a job with no display data (title = media id).
    pub fn enqueue(&self, job: Job) -> Result<i64, DownloadError> {
        let meta = Meta { title: job.media_id.clone(), ..Meta::default() };
        self.enqueue_with(job, meta)
    }

    /// Queue a job; returns the row id.
    pub fn enqueue_with(&self, job: Job, meta: Meta) -> Result<i64, DownloadError> {
        let row = DownloadRow {
            media_id: job.media_id,
            title: if meta.title.trim().is_empty() { "?".to_owned() } else { meta.title },
            poster: meta.poster,
            episode: job.episode,
            chapter: meta.chapter,
            kind: job.kind,
            state: "queued".to_owned(),
            url: job.url,
            headers: job.headers,
            source_id: meta.source_id,
            quality: meta.quality,
            audio_lang: meta.audio_lang,
            sub_lang: meta.sub_lang,
            path: Some(job.target.to_string_lossy().into_owned()),
            added_at: now_unix(),
            ..DownloadRow::default()
        };
        let id = self.inner.store.insert_download(&row)?;
        self.start();
        self.inner.wake.notify_one();
        Ok(id)
    }

    fn control(&self, id: i64) -> Option<Arc<Control>> {
        self.inner.running.lock().ok().and_then(|r| r.get(&id).cloned())
    }

    fn row(&self, id: i64) -> Result<DownloadRow, DownloadError> {
        self.inner.store.download(id)?.ok_or(DownloadError::NotFound(id))
    }

    pub fn pause(&self, id: i64) -> Result<(), DownloadError> {
        if let Some(c) = self.control(id) {
            c.cmd.store(PAUSE, Ordering::SeqCst);
            return Ok(());
        }
        let r = self.row(id)?;
        if r.state == "queued" || r.state == "downloading" {
            self.inner.store.set_download_state(id, "paused", None, None, None)?;
        }
        Ok(())
    }

    /// Back to the queue from `paused` / `failed` (keeps the checkpoint) or `cancelled` / `broken` (starts over).
    pub fn resume(&self, id: i64) -> Result<(), DownloadError> {
        if self.control(id).is_some() {
            return Ok(());
        }
        let r = self.row(id)?;
        match r.state.as_str() {
            "paused" | "failed" => {}
            "cancelled" | "broken" => {
                if let Some(p) = r.path.as_deref() {
                    let target = Path::new(p);
                    files::remove_partials(target);
                    if r.state == "broken" {
                        files::remove_path(target);
                    }
                }
                self.inner.store.reset_download_progress(id)?;
            }
            _ => return Ok(()),
        }
        self.inner.store.set_download_state(id, "queued", None, None, None)?;
        self.start();
        self.inner.wake.notify_one();
        Ok(())
    }

    /// Stop a row. In-progress data always goes; `delete_file` also removes a completed file / folder.
    pub fn cancel(&self, id: i64, delete_file: bool) -> Result<(), DownloadError> {
        if let Some(c) = self.control(id) {
            c.delete_file.store(delete_file, Ordering::SeqCst);
            c.cmd.store(CANCEL, Ordering::SeqCst);
            return Ok(());
        }
        let r = self.row(id)?;
        if let Some(p) = r.path.as_deref() {
            files::remove_partials(Path::new(p));
            if delete_file {
                files::remove_path(Path::new(p));
            }
        }
        if r.state != "completed" || delete_file {
            self.inner.store.set_download_state(id, "cancelled", None, None, None)?;
        }
        Ok(())
    }

    /// Delete a row (stopping it first); `delete_file` also removes its data.
    pub async fn remove(&self, id: i64, delete_file: bool) -> Result<(), DownloadError> {
        if let Some(c) = self.control(id) {
            c.delete_file.store(delete_file, Ordering::SeqCst);
            c.cmd.store(CANCEL, Ordering::SeqCst);
            // Wait (bounded) for the worker to let go of its files.
            for _ in 0..50 {
                if self.control(id).is_none() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
        let r = self.row(id)?;
        if let Some(p) = r.path.as_deref() {
            files::remove_partials(Path::new(p));
            if delete_file {
                files::remove_path(Path::new(p));
            }
        }
        self.inner.store.delete_download(id)?;
        Ok(())
    }

    /// Re-check a finished row on disk (`downloads.integrity-check`): damage -> `broken`, a `broken` row whose file
    /// is back -> `completed`. Other states are returned unchanged.
    pub fn verify(&self, id: i64) -> Result<DownloadRow, DownloadError> {
        let r = self.row(id)?;
        if r.state != "completed" && r.state != "broken" {
            return Ok(r);
        }
        let damage = match r.path.as_deref() {
            Some(p) => files::check(&r.kind, Path::new(p), r.bytes_total, r.segments_total),
            None => Some(Damage::Missing),
        };
        match (damage, r.state.as_str()) {
            (Some(d), "completed") => {
                self.inner.store.set_download_state(id, "broken", Some(d.as_str()), None, None)?;
            }
            (None, "broken") => {
                self.inner.store.set_download_state(id, "completed", None, None, None)?;
            }
            _ => {}
        }
        self.row(id)
    }

    /// Verify every completed row (start-up pass); returns how many turned `broken`.
    pub fn verify_completed(&self) -> usize {
        let rows = self.inner.store.downloads_in_state("completed").unwrap_or_default();
        rows.iter().filter(|r| self.verify(r.id).is_ok_and(|v| v.state == "broken")).count()
    }

    /// Live numbers of the running rows.
    pub fn progress(&self) -> Vec<Progress> {
        self.inner.running.lock().map(|r| r.iter().map(|(id, c)| c.snapshot(*id)).collect()).unwrap_or_default()
    }

    /// Rows running right now.
    pub fn running(&self) -> usize {
        self.inner.running.lock().map(|r| r.len()).unwrap_or(0)
    }

    /// Stop everything for app exit: running rows go back to `queued` (they resume at the next start).
    pub fn shutdown(&self) {
        self.inner.stopped.store(true, Ordering::SeqCst);
        if let Ok(r) = self.inner.running.lock() {
            for c in r.values() {
                c.cmd.store(STOP, Ordering::SeqCst);
            }
        }
        self.inner.wake.notify_one();
    }
}

/// A chapter row the manga / novel readers (workers l / m) record and run themselves
/// (`LocalStore::record_chapter_download` / `novel_insert_download`: url `manga:<ext>/<chapter>` or
/// `novel:<ext>/<chapter>`). It has no media URL, so the engine never runs or requeues it.
pub fn is_reader_row(kind: &str, url: &str) -> bool {
    (kind == KIND_MANGA || kind == KIND_NOVEL)
        && url.strip_prefix(kind).and_then(|r| r.strip_prefix(':')).is_some_and(|r| !r.starts_with("//"))
}

async fn scheduler(inner: Arc<Inner>) {
    loop {
        if inner.stopped.load(Ordering::SeqCst) {
            return;
        }
        fill(&inner);
        tokio::select! {
            _ = inner.wake.notified() => {}
            _ = tokio::time::sleep(TICK) => {}
        }
    }
}

/// Start queued rows while there are free slots.
fn fill(inner: &Arc<Inner>) {
    let limit = inner.concurrent.load(Ordering::Relaxed).max(1);
    let queued = match inner.store.downloads_in_state("queued") {
        Ok(q) => q,
        Err(e) => {
            tracing::warn!(error = %e, "download queue unreadable");
            return;
        }
    };
    for row in queued {
        if is_reader_row(&row.kind, &row.url) {
            continue;
        }
        let ctl = {
            let Ok(mut running) = inner.running.lock() else { return };
            if running.len() >= limit {
                return;
            }
            if running.contains_key(&row.id) {
                continue;
            }
            let c = Arc::new(Control::new());
            running.insert(row.id, c.clone());
            c
        };
        let inner = inner.clone();
        tokio::spawn(async move {
            let id = row.id;
            work(&inner, row, &ctl).await;
            if let Ok(mut r) = inner.running.lock() {
                r.remove(&id);
            }
            inner.wake.notify_one();
        });
    }
}

/// Run one row to its end state.
async fn work(inner: &Arc<Inner>, row: DownloadRow, ctl: &Arc<Control>) {
    let id = row.id;
    let store = &inner.store;
    if let Err(e) = store.set_download_state(id, "downloading", None, None, None) {
        tracing::warn!(id, error = %e, "download state not saved");
    }
    ctl.set_totals(row.bytes_done, row.bytes_total);
    let Some(target) = row.path.clone().map(PathBuf::from) else {
        let _ = store.set_download_state(id, "failed", Some("no target path"), None, None);
        return;
    };
    let w = Worker { inner, row: &row, ctl, target: target.clone(), last_save: Mutex::new(Instant::now()) };
    let result = match row.kind.as_str() {
        KIND_HLS => w.hls(&row.url).await,
        KIND_MP4 => w.file().await,
        KIND_MANGA => w.pages().await,
        KIND_NOVEL => w.text().await,
        other => Err(Halt::Failed(DownloadError::Playlist(format!("unknown kind {other}")))),
    };
    let outcome = match result {
        Ok((path, bytes)) => {
            let p = path.to_string_lossy().into_owned();
            let segments = store.download(id).ok().flatten().map_or(row.segments_done, |r| r.segments_done);
            if let Err(e) = store.update_download_progress(id, bytes, Some(bytes), segments, None) {
                tracing::warn!(id, error = %e, "final download size not saved");
            }
            store.set_download_state(id, "completed", None, Some(&p), Some(now_unix()))
        }
        Err(Halt::Paused) => store.set_download_state(id, "paused", None, None, None),
        Err(Halt::Shutdown) => store.set_download_state(id, "queued", None, None, None),
        Err(Halt::Cancelled) => {
            files::remove_partials(&target);
            if ctl.delete_file.load(Ordering::SeqCst) {
                files::remove_path(&target);
            }
            store.set_download_state(id, "cancelled", None, None, None)
        }
        Err(Halt::Failed(e)) => {
            tracing::warn!(id, error = %e, "download failed");
            store.set_download_state(id, "failed", Some(&e.to_string()), None, None)
        }
    };
    if let Err(e) = outcome {
        tracing::warn!(id, error = %e, "download end state not saved");
    }
}

struct Worker<'a> {
    inner: &'a Arc<Inner>,
    row: &'a DownloadRow,
    ctl: &'a Arc<Control>,
    target: PathBuf,
    last_save: Mutex<Instant>,
}

impl Worker<'_> {
    fn request(&self, url: &str, range: Option<String>) -> reqwest::RequestBuilder {
        let mut b = self.inner.client.get(url).timeout(REQUEST_TIMEOUT);
        for (k, v) in &self.row.headers {
            // A stored Range would fight the engine's own.
            if !k.eq_ignore_ascii_case("range") {
                b = b.header(k.as_str(), v.as_str());
            }
        }
        if let Some(r) = range {
            b = b.header(reqwest::header::RANGE, r);
        }
        b
    }

    /// Save the checkpoint when [`SAVE_EVERY`] passed (or `force`).
    fn save(&self, bytes: u64, total: Option<u64>, segments: u32, segments_total: Option<u32>, force: bool) {
        let due = self.last_save.lock().map(|mut t| {
            if force || t.elapsed() >= SAVE_EVERY {
                *t = Instant::now();
                true
            } else {
                false
            }
        });
        if due.unwrap_or(true)
            && let Err(e) = self.inner.store.update_download_progress(self.row.id, bytes, total, segments, segments_total)
        {
            tracing::warn!(id = self.row.id, error = %e, "download checkpoint not saved");
        }
    }

    /// GET a whole body with retries, the speed limit and the control flag (segments, pages, keys, playlists).
    async fn fetch(&self, url: &str, range: Option<(u64, u64)>, count: bool) -> Result<(Vec<u8>, Option<String>), Halt> {
        let mut last = None;
        for attempt in 0..ATTEMPTS {
            self.ctl.check()?;
            if attempt > 0 {
                tokio::time::sleep(Duration::from_millis(800 * u64::from(attempt))).await;
            }
            match self.fetch_once(url, range, count).await {
                Ok(v) => return Ok(v),
                Err(Halt::Failed(e)) => {
                    // 4xx other than 408 / 429 will not get better.
                    let fatal = matches!(e, DownloadError::Http(s) if (400..500).contains(&s) && s != 408 && s != 429);
                    if fatal {
                        return Err(Halt::Failed(e));
                    }
                    last = Some(e);
                }
                Err(h) => return Err(h),
            }
        }
        Err(Halt::Failed(last.unwrap_or_else(|| DownloadError::Network("no response".into()))))
    }

    async fn fetch_once(&self, url: &str, range: Option<(u64, u64)>, count: bool) -> Result<(Vec<u8>, Option<String>), Halt> {
        let mut resp = self.request(url, range.map(hls::range_header)).send().await?;
        let status = resp.status();
        if !status.is_success() {
            return Err(Halt::Failed(DownloadError::Http(status.as_u16())));
        }
        let ct = resp.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).map(str::to_owned);
        let mut body = Vec::with_capacity(resp.content_length().unwrap_or(0).min(64 << 20) as usize);
        while let Some(chunk) = resp.chunk().await? {
            self.ctl.check()?;
            self.inner.limiter.take(chunk.len() as u64).await;
            if count {
                self.ctl.add(chunk.len() as u64);
            }
            body.extend_from_slice(&chunk);
        }
        Ok((body, ct))
    }

    // ---- HLS --------------------------------------------------------------------------------------

    async fn hls(&self, url: &str) -> Result<(PathBuf, u64), Halt> {
        let want = hls::wanted_height(self.row.quality.as_deref());
        let mut url = url.to_owned();
        let mut media = None;
        for _ in 0..MAX_PLAYLIST_HOPS {
            let (body, _) = self.fetch(&url, None, false).await?;
            match hls::parse(&body)? {
                Parsed::Media(m) => {
                    media = Some(m);
                    break;
                }
                Parsed::Master(m) => {
                    let i = hls::pick_variant(&m.variants, want).ok_or_else(|| DownloadError::Playlist("no variant".into()))?;
                    url = hls::absolute(&url, &m.variants[i].uri)?;
                }
            }
        }
        let media = media.ok_or_else(|| DownloadError::Playlist("too many playlist hops".into()))?;
        if !media.end_list && media.playlist_type.is_none() {
            // No #EXT-X-ENDLIST: a live window or a sloppy VOD list. Either way only what is listed now is fetched.
            tracing::info!(id = self.row.id, "playlist without ENDLIST; downloading the listed segments");
        }
        let plan = hls::plan(&url, &media)?;
        let total = plan.segments.len();
        let target = if plan.fmp4 { self.target.with_extension("mp4") } else { self.target.clone() };
        let parts = files::parts_dir(&self.target);
        tokio::fs::create_dir_all(&parts).await?;

        // Keys first (few, small).
        let mut keys: HashMap<String, [u8; 16]> = HashMap::new();
        for seg in &plan.segments {
            if let Some(k) = &seg.key
                && !keys.contains_key(&k.uri)
            {
                let (bytes, _) = self.fetch(&k.uri, None, false).await?;
                let key: [u8; 16] =
                    bytes.as_slice().try_into().map_err(|_| DownloadError::Playlist(format!("key of {} bytes", bytes.len())))?;
                keys.insert(k.uri.clone(), key);
            }
        }
        if let Some(init) = &plan.init {
            let f = files::init_file(&parts);
            if !f.is_file() {
                let (bytes, _) = self.fetch(&init.url, init.range, true).await?;
                write_atomic(&f, &bytes).await?;
            }
        }

        // Checkpoint: segment files already on disk.
        let todo: Vec<usize> = (0..total).filter(|i| !files::segment_file(&parts, *i).is_file()).collect();
        let mut done = total - todo.len();
        let mut bytes_done = files::path_bytes(&parts);
        let estimate = |done: usize, bytes: u64| if done == 0 { None } else { Some(bytes / done as u64 * total as u64) };
        self.ctl.set_totals(bytes_done, estimate(done, bytes_done));
        self.save(bytes_done, estimate(done, bytes_done), done as u32, Some(total as u32), true);

        let keys = &keys;
        let plan_ref = &plan;
        let parts_ref = &parts;
        let mut stream = futures_util::stream::iter(todo)
            .map(move |i| async move {
                let seg = &plan_ref.segments[i];
                let (mut bytes, _) = self.fetch(&seg.url, seg.range, true).await?;
                if let Some(k) = &seg.key {
                    let key = keys.get(&k.uri).ok_or_else(|| DownloadError::Playlist("key missing".into()))?;
                    hls::decrypt_aes128(&mut bytes, key, &k.iv)?;
                }
                write_atomic(&files::segment_file(parts_ref, i), &bytes).await?;
                Ok::<u64, Halt>(bytes.len() as u64)
            })
            .buffer_unordered(SEGMENT_PARALLELISM);
        while let Some(r) = stream.next().await {
            let n = r?;
            done += 1;
            bytes_done += n;
            let est = estimate(done, bytes_done);
            self.ctl.set_totals(self.ctl.snapshot(0).bytes_done, est);
            self.save(bytes_done, est, done as u32, Some(total as u32), false);
        }
        drop(stream);
        self.ctl.check()?;

        let (parts_c, target_c) = (parts.clone(), target.clone());
        let size = tokio::task::spawn_blocking(move || files::concat_segments(&parts_c, total, &target_c))
            .await
            .map_err(|e| DownloadError::Network(format!("concat task: {e}")))??;
        files::remove_path(&parts);
        let (path, size) = remux(target, size).await;
        self.save(size, Some(size), total as u32, Some(total as u32), true);
        Ok((path, size))
    }

    // ---- direct file (MP4) --------------------------------------------------------------------------

    async fn file(&self) -> Result<(PathBuf, u64), Halt> {
        let part = files::part_file(&self.target);
        if let Some(parent) = part.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let mut last = None;
        for attempt in 0..ATTEMPTS {
            self.ctl.check()?;
            if attempt > 0 {
                tokio::time::sleep(Duration::from_secs(u64::from(attempt) * 2)).await;
            }
            match self.file_once(&part).await {
                Ok(FileOutcome::Done(size)) => {
                    tokio::fs::rename(&part, &self.target).await?;
                    return Ok((self.target.clone(), size));
                }
                Ok(FileOutcome::Playlist) => {
                    // The "MP4" is an HLS playlist: switch the row and download it as such.
                    files::remove_path(&part);
                    let _ = self.inner.store.set_download_source(self.row.id, KIND_HLS, &self.row.url, &self.row.headers);
                    let ts = self.target.with_extension("ts");
                    let w = Worker { target: ts, last_save: Mutex::new(Instant::now()), ..*self };
                    return w.hls(&self.row.url).await;
                }
                Err(Halt::Failed(e)) => {
                    let fatal = matches!(e, DownloadError::Http(s) if (400..500).contains(&s) && s != 408 && s != 429 && s != 416);
                    if fatal {
                        return Err(Halt::Failed(e));
                    }
                    last = Some(e);
                }
                Err(h) => return Err(h),
            }
        }
        Err(Halt::Failed(last.unwrap_or_else(|| DownloadError::Network("no response".into()))))
    }

    async fn file_once(&self, part: &Path) -> Result<FileOutcome, Halt> {
        let have = tokio::fs::metadata(part).await.map(|m| m.len()).unwrap_or(0);
        let range = (have > 0).then(|| format!("bytes={have}-"));
        let mut resp = self.request(&self.row.url, range).send().await?;
        let status = resp.status().as_u16();
        if status == 416 && have > 0 {
            // Nothing left to fetch: the part is the whole file.
            return Ok(FileOutcome::Done(have));
        }
        if !resp.status().is_success() {
            return Err(Halt::Failed(DownloadError::Http(status)));
        }
        let resumed = status == 206;
        let headers = resp.headers();
        let total = if resumed {
            headers
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(content_range_total)
                .or_else(|| resp.content_length().map(|l| l + have))
        } else {
            resp.content_length()
        };
        let ct = headers.get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_ascii_lowercase();
        if !resumed && (ct.contains("mpegurl") || self.row.url.to_ascii_lowercase().split('?').next().is_some_and(|p| p.ends_with(".m3u8"))) {
            return Ok(FileOutcome::Playlist);
        }
        let mut file = if resumed {
            tokio::fs::OpenOptions::new().append(true).open(part).await?
        } else {
            tokio::fs::File::create(part).await?
        };
        let mut written = if resumed { have } else { 0 };
        self.ctl.set_totals(written, total);
        let mut first = !resumed;
        while let Some(chunk) = resp.chunk().await? {
            if let Err(h) = self.ctl.check() {
                file.flush().await?;
                self.save(written, total, 0, None, true);
                return Err(h);
            }
            if first {
                first = false;
                if hls::is_playlist(&chunk) {
                    return Ok(FileOutcome::Playlist);
                }
            }
            self.inner.limiter.take(chunk.len() as u64).await;
            file.write_all(&chunk).await?;
            written += chunk.len() as u64;
            self.ctl.add(chunk.len() as u64);
            self.save(written, total, 0, None, false);
        }
        file.flush().await?;
        drop(file);
        if let Some(t) = total
            && written < t
        {
            return Err(Halt::Failed(DownloadError::Network(format!("connection closed at {written} of {t} bytes"))));
        }
        Ok(FileOutcome::Done(written))
    }

    // ---- manga pages / novel text ------------------------------------------------------------------

    async fn pages(&self) -> Result<(PathBuf, u64), Halt> {
        let urls: Vec<&str> = self.row.url.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        if urls.is_empty() {
            return Err(Halt::Failed(DownloadError::Playlist("no pages".into())));
        }
        let dir = self.target.clone();
        tokio::fs::create_dir_all(&dir).await?;
        let total = urls.len() as u32;
        let mut done = 0u32;
        for (i, url) in urls.iter().enumerate() {
            self.ctl.check()?;
            if files::existing_page(&dir, i).is_none() {
                let (bytes, ct) = self.fetch(url, None, true).await?;
                let ext = files::image_ext(ct.as_deref(), url);
                write_atomic(&dir.join(format!("{:03}.{ext}", i + 1)), &bytes).await?;
            }
            done += 1;
            self.save(files::path_bytes(&dir), None, done, Some(total), false);
        }
        let bytes = files::path_bytes(&dir);
        self.save(bytes, Some(bytes), total, Some(total), true);
        self.mark_offline(KIND_MANGA, Some(total), None, bytes);
        Ok((dir, bytes))
    }

    async fn text(&self) -> Result<(PathBuf, u64), Halt> {
        let src = self.row.url.trim();
        let content = if src.starts_with("http://") || src.starts_with("https://") {
            let (bytes, _) = self.fetch(src, None, true).await?;
            String::from_utf8_lossy(&bytes).into_owned()
        } else {
            src.to_owned()
        };
        let json = novel_json(&content);
        if let Some(parent) = self.target.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        write_atomic(&self.target, json.as_bytes()).await?;
        let bytes = json.len() as u64;
        self.mark_offline(KIND_NOVEL, None, Some(json), bytes);
        Ok((self.target.clone(), bytes))
    }

    /// Record the chapter in `offline_chapters` when the target follows `<dir>/<media>/<ext>/<chapter>`.
    fn mark_offline(&self, kind: &str, pages: Option<u32>, text_json: Option<String>, bytes: u64) {
        let Some((ext_id, chapter_id)) = chapter_ids(&self.target, kind == KIND_NOVEL) else { return };
        let mark = OfflineChapterMark {
            kind: kind.to_owned(),
            media_id: self.row.media_id.clone(),
            ext_id,
            chapter_id,
            dir: Some(self.target.to_string_lossy().into_owned()),
            text_json,
            pages,
            bytes,
            saved_at: now_unix(),
        };
        if let Err(e) = self.inner.store.mark_offline_chapter(&mark) {
            tracing::warn!(id = self.row.id, error = %e, "offline chapter not recorded");
        }
    }
}

enum FileOutcome {
    Done(u64),
    Playlist,
}

/// `bytes 100-199/1000` -> 1000.
pub fn content_range_total(v: &str) -> Option<u64> {
    v.rsplit_once('/').and_then(|(_, t)| t.trim().parse().ok())
}

/// `(ext_id, chapter_id)` from `<...>/<ext>/<chapter>` (novel targets drop their `.json`).
pub fn chapter_ids(target: &Path, strip_ext: bool) -> Option<(String, String)> {
    let chapter = if strip_ext { target.file_stem()? } else { target.file_name()? };
    let ext = target.parent()?.file_name()?;
    Some((ext.to_string_lossy().into_owned(), chapter.to_string_lossy().into_owned()))
}

/// Novel chapter body as the JSON list of paragraphs `offline_chapters.text_json` keeps: a JSON list stays as is,
/// anything else is split on blank lines.
pub fn novel_json(content: &str) -> String {
    if let Ok(v) = serde_json::from_str::<Vec<String>>(content) {
        return serde_json::to_string(&v).unwrap_or_else(|_| "[]".to_owned());
    }
    let paragraphs: Vec<String> = content
        .replace("\r\n", "\n")
        .split("\n\n")
        .map(|p| p.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|p| !p.is_empty())
        .collect();
    serde_json::to_string(&paragraphs).unwrap_or_else(|_| "[]".to_owned())
}

/// Write `<path>.tmp` then rename (a file under `path` is always complete).
async fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    tokio::fs::write(&tmp, bytes).await?;
    tokio::fs::rename(&tmp, path).await
}

/// `ffmpeg -c copy` a finished `.ts` into `.mp4` when ffmpeg is installed; the `.ts` stays when it is not (mpv
/// plays both). Returns the final path and size.
async fn remux(ts: PathBuf, size: u64) -> (PathBuf, u64) {
    if !ts.extension().is_some_and(|e| e.eq_ignore_ascii_case("ts")) {
        return (ts, size);
    }
    let mp4 = ts.with_extension("mp4");
    let (src, dst) = (ts.clone(), mp4.clone());
    let ok = tokio::task::spawn_blocking(move || {
        let mut cmd = std::process::Command::new("ffmpeg");
        cmd.args(["-nostdin", "-loglevel", "error", "-y", "-i"])
            .arg(&src)
            .args(["-map", "0", "-c", "copy", "-bsf:a", "aac_adtstoasc", "-movflags", "+faststart"])
            .arg(&dst)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW: no console flash.
            cmd.creation_flags(0x0800_0000);
        }
        cmd.status().map(|s| s.success()).unwrap_or(false)
    })
    .await
    .unwrap_or(false);
    let mp4_size = std::fs::metadata(&mp4).map(|m| m.len()).unwrap_or(0);
    if ok && mp4_size > 0 {
        files::remove_path(&ts);
        (mp4, mp4_size)
    } else {
        files::remove_path(&mp4);
        (ts, size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reader_rows_are_not_engine_jobs() {
        assert!(is_reader_row("manga", "manga:ext/ch-1"));
        assert!(is_reader_row("novel", "novel:toko/abc"));
        assert!(!is_reader_row("manga", "https://cdn.example/1.jpg\nhttps://cdn.example/2.jpg"));
        assert!(!is_reader_row("novel", "Hello\n\nWorld"));
        assert!(!is_reader_row("hls", "manga:ext/ch-1"));
    }

    #[test]
    fn eta_math() {
        assert_eq!(eta(0, Some(100), 10), Some(10));
        assert_eq!(eta(95, Some(100), 10), Some(1));
        assert_eq!(eta(100, Some(100), 0), Some(0));
        assert_eq!(eta(0, None, 10), None);
        assert_eq!(eta(0, Some(100), 0), None);
    }

    #[test]
    fn content_range() {
        assert_eq!(content_range_total("bytes 100-199/1000"), Some(1000));
        assert_eq!(content_range_total("bytes 100-199/*"), None);
    }

    #[test]
    fn chapter_targets() {
        let p = Path::new("/d/30013/ext.a/ch-1");
        assert_eq!(chapter_ids(p, false), Some(("ext.a".to_owned(), "ch-1".to_owned())));
        let n = Path::new("/d/30013/ext.a/ch-1.json");
        assert_eq!(chapter_ids(n, true), Some(("ext.a".to_owned(), "ch-1".to_owned())));
    }

    #[test]
    fn novel_bodies() {
        assert_eq!(novel_json("[\"a\",\"b\"]"), "[\"a\",\"b\"]");
        assert_eq!(novel_json("one\nline\n\n two \r\n\r\nthree"), "[\"one line\",\"two\",\"three\"]");
    }

    #[test]
    fn control_flags() {
        let c = Control::new();
        assert!(c.check().is_ok());
        c.add(1000);
        assert_eq!(c.snapshot(1).bytes_done, 1000);
        c.cmd.store(PAUSE, Ordering::SeqCst);
        assert!(matches!(c.check(), Err(Halt::Paused)));
        c.cmd.store(CANCEL, Ordering::SeqCst);
        assert!(matches!(c.check(), Err(Halt::Cancelled)));
    }

    #[tokio::test]
    async fn engine_rows_without_network() {
        let store = Arc::new(LocalStore::open_in_memory().unwrap());
        let dir = tempfile::tempdir().unwrap();
        let e = Engine::new(reqwest::Client::new(), store.clone(), dir.path().to_path_buf(), 2);
        // A novel chapter with inline text completes without any request.
        let target = dir.path().join("1").join("ext").join("c1.json");
        let id = e
            .enqueue_with(
                Job { media_id: "1".into(), episode: None, kind: KIND_NOVEL.into(), url: "Hello\n\nWorld".into(), headers: vec![], target },
                Meta { title: "Novel".into(), chapter: Some("c1".into()), ..Meta::default() },
            )
            .unwrap();
        for _ in 0..100 {
            if store.download(id).unwrap().unwrap().state == "completed" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let row = store.download(id).unwrap().unwrap();
        assert_eq!(row.state, "completed");
        assert_eq!(e.verify(id).unwrap().state, "completed");
        // Deleting the file makes it broken; resume starts it over.
        files::remove_path(Path::new(row.path.as_deref().unwrap()));
        assert_eq!(e.verify(id).unwrap().state, "broken");
        e.remove(id, true).await.unwrap();
        assert!(store.download(id).unwrap().is_none());
    }
}
