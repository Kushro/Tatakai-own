//! Transcoding sessions: on-demand fMP4 HLS with a RAM window.
//!
//! A *run* (generation of the engine) produces segments `seg_<k>.m4s` starting at `k`; FFmpeg PUTs them to the loopback
//! sink ([`crate::sink`]). Segments live in a bounded RAM window (512 MiB, oldest evicted first). Requests that fall
//! outside the window restart the engine at the requested segment (seek). The sink applies back-pressure (it delays
//! the answer to the PUT) when the engine is too far ahead of the playhead reported by the heartbeat, so FFmpeg blocks
//! instead of decoding the whole file. 120 s without heartbeat closes the session.
//!
//! 0.4.1 (Link stuck after a long pause): FFmpeg n9's HLS muxer does NOT wait for the answer to its PUT (it closes the
//! connection after a non-blocking read), so a delayed answer never held it: it kept encoding and every held PUT stayed
//! open with its segment in memory. A second PUT arriving while one is held proves it, and the run is then *parked*:
//! killed, its next segment remembered, and started again there when the playhead (or a request) comes within reach
//! ([`supervise_decision`]). A run that shows no life for [`SessionTuning::stall_timeout`] while it should be producing
//! is restarted where it stopped; after [`MAX_AUTO_RESTARTS`] fruitless attempts the session fails (`engine_stalled`)
//! and its media answer `410 segment_unavailable` instead of `503` forever. The window never evicts what the phone is
//! about to play ([`Manager::evict`]).
//!
//! LSFG profiles probe the source frame rate once per session ([`Engine::source_rate`], fallback 23.976) and run each
//! generation as the three-process chain of [`args::build_lsfg`]; killing a run kills the whole chain.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use bytes::Bytes;
use serde::Serialize;
use tokio::sync::Notify;
use url::Url;

use crate::api::{ApiError, ApiResult, ErrorCode};
use crate::fetcher::{FetchError, Fetcher, SourceSpec};
use crate::media::args::{self, PipelineParams};
use crate::media::capabilities::{Environment, MAX_RAM_BYTES};
use crate::media::engine::{Engine, EngineExit, EngineJob, LsfgStages};
use crate::media::probe::{FALLBACK_RATE, FrameRate, SourceInfo};
use crate::media::profiles::{self, Encoder, ProfileSpec, SEGMENT_SECS};
use crate::subtitles::{self, Lookup, ScratchDir, SubFormat, SubtitleStore};
use tk_core::net::{Policy, sanitize_headers};

pub const HEARTBEAT_SECS: u64 = 4;
pub const IDLE_TIMEOUT_SECS: u64 = 120;
const TOMBSTONES: usize = 64;
/// Adding a magnet waits for its metadata (DHT / trackers): give up after this long (BUG-009).
pub const TORRENT_OPEN_TIMEOUT: Duration = Duration::from_secs(75);
/// BUG-131: how long the PC may take to prepare a session that is not a fresh `torrent` source (a profile change, a
/// reopen, a link) before it answers `source_unavailable`: below the phones' 20 s read timeout (`LinkCrypto`), so a
/// renew always ends with `Link session created` or `Link session refused` instead of a request the phone abandoned.
pub const PREPARE_TIMEOUT: Duration = Duration::from_secs(15);
/// Sources of the last sessions kept for [`SourceRequest::Session`].
pub const ORIGINS_KEPT: usize = 128;
/// Segments kept behind the playhead for seek-back.
/// BUG-095: segments kept behind the playhead (60 s), so a short rewind is served from RAM instead of restarting the
/// engine (with A4K + LSFG a restart costs ~15 s). Still bounded by the RAM budget.
const KEEP_BEHIND: u32 = 30;
/// BUG-095: a request at most this many segments past the running engine's frontier may wait for it instead of
/// restarting it there, when the engine makes them sooner than a restart would ([`wait_for_frontier`]).
pub const MAX_WAIT_AHEAD: u32 = 30;
/// 0.4.1: segments right behind the playhead that are never evicted (the phone may ask for the current one again after a
/// retry or a short rewind).
const PROTECT_BEHIND: u32 = 2;
/// 0.4.1: automatic restarts (stalled, failed or barren runs) without any segment produced in between before the
/// session fails with `engine_stalled`.
pub const MAX_AUTO_RESTARTS: u32 = 3;
/// 0.4.1: a parked run is started again this many segments (plus its measured restart cost) before the phone would
/// reach the segment it stopped at.
const RESUME_MARGIN: u32 = 5;

#[derive(Debug, Clone)]
pub struct SessionTuning {
    pub segment_wait: Duration,
    pub idle_timeout: Duration,
    pub max_ram_bytes: usize,
    /// How far (in segments) the engine may run ahead of the playhead before the sink blocks it.
    pub max_lead_segments: u32,
    /// A request further than this many segments past the frontier restarts the engine there.
    pub seek_ahead_segments: u32,
    /// BUG-131: time limit of a session's preparation unless it is a fresh `torrent` source ([`PREPARE_TIMEOUT`]).
    pub prepare_timeout: Duration,
    /// 0.4.0: how long a subtitle file request waits for the extraction before `503` + `Retry-After`
    /// ([`crate::subtitles::REQUEST_WAIT`]).
    pub subtitle_wait: Duration,
    /// 0.4.1: a run that shows no life (no PUT at all) for this long while it should be producing is stalled and
    /// restarted ([`supervise_decision`]); twice its measured restart cost when that is longer.
    pub stall_timeout: Duration,
}

impl Default for SessionTuning {
    fn default() -> Self {
        Self {
            // below the phones' 20 s read timeout (`LinkCrypto`): they get a clean 503 + Retry-After and retry, never a
            // socket timeout mid-wait
            segment_wait: Duration::from_secs(crate::media::capabilities::SEGMENT_WAIT_SECS),
            idle_timeout: Duration::from_secs(IDLE_TIMEOUT_SECS),
            max_ram_bytes: MAX_RAM_BYTES as usize,
            max_lead_segments: 15,
            seek_ahead_segments: 4,
            prepare_timeout: PREPARE_TIMEOUT,
            subtitle_wait: crate::subtitles::REQUEST_WAIT,
            stall_timeout: Duration::from_secs(30),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Preparing,
    Ready,
    Failed(FailReason),
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailReason {
    SourceUnavailable,
    SourceReadFailed,
    EngineUnavailable,
    UrlSourceDenied,
    MemoryLimit,
    /// 0.4.1: the engine stopped producing and [`MAX_AUTO_RESTARTS`] restarts did not bring it back.
    EngineStalled,
}

impl FailReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SourceUnavailable => "source_unavailable",
            Self::SourceReadFailed => "source_read_failed",
            Self::EngineUnavailable => "engine_unavailable",
            Self::UrlSourceDenied => "url_source_denied",
            Self::MemoryLimit => "memory_limit",
            Self::EngineStalled => "engine_stalled",
        }
    }
}

#[derive(Default)]
struct Window {
    init: Option<Bytes>,
    segs: BTreeMap<u32, Bytes>,
    bytes: usize,
}

struct State {
    phase: Phase,
    /// Engine generation; PUTs from older runs are rejected.
    run_gen: u32,
    gen_start: u32,
    window: Window,
    playhead: u32,
    paused: bool,
    /// The current run finished (all segments produced).
    finished: bool,
    kill: Option<Arc<Notify>>,
    throttled: bool,
    /// BUG-058: the highest segment of the current run a phone is waiting for. Back-pressure never holds the engine
    /// below it: the phone's player decides how much it needs before it plays, and a PC that waits for the playhead
    /// to move while the player waits for that segment deadlocks (buffering forever).
    wanted: u32,
    /// BUG-009 / BUG-023: the end of the EVENT playlist (sessions without a duration). It never shrinks (a restart at
    /// an earlier segment would otherwise make the playlist shorter, which Media3 takes for a stale or broken one).
    event_top: u32,
    /// BUG-025 sustain measurement of the current run: when it started, segments stored, whether back-pressure held it.
    run_started: Instant,
    run_segments: u32,
    run_throttled: bool,
    /// BUG-095: the next segment the current run produces (the window also keeps segments of earlier runs, so its
    /// last key is not the engine's frontier).
    run_top: u32,
    /// BUG-095: when the current run was spawned, the measured cost of a (re)start (spawn -> first segment) and the
    /// engine's time per segment (back-pressure waits excluded), to decide between waiting and restarting.
    run_spawned: Instant,
    restart_cost: Option<Duration>,
    seg_interval: Option<Duration>,
    last_stored: Option<Instant>,
    /// 0.4.1: production is parked (no engine running) and resumes at this segment ([`supervise_decision`]).
    parked_at: Option<u32>,
    /// 0.4.1: PUTs of the current run held by back-pressure right now.
    held: u32,
    /// 0.4.1: the encoder does not wait for the sink's answer (a second PUT arrived while one was held): back-pressure
    /// parks the run, and it may make twice the lead before that (fewer restarts).
    no_wait: bool,
    /// 0.4.1: the last sign of life of the current run (spawned, any PUT received).
    last_life: Instant,
    /// 0.4.1: automatic restarts since the last segment stored ([`MAX_AUTO_RESTARTS`]).
    auto_restarts: u32,
    /// 0.4.1: the pipeline reached the real end of the source here (one past its last segment): `ended` in the state,
    /// requests at or after it answer 404 like past the playlist, and nothing restarts there.
    source_end: Option<u32>,
    /// 0.4.1: where the last run that ended early stopped ([`run_end`]).
    early_end: Option<u32>,
}

/// 0.4.1: how a run that exited cleanly ended ([`run_end`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RunEnd {
    /// At the end of the source.
    Source,
    /// Before it (its input stopped): the last segment may be cut short.
    Early,
}

/// Segments before the announced end that still count as the end of the source: the container duration and what the
/// pipeline really makes differ by a frame or a short last segment.
const END_SLACK: u32 = 3;

/// 0.4.1: a run exited with success. FFmpeg also exits with 0 when its input breaks (an I/O error ends the demuxer), so
/// only a run that got within [`END_SLACK`] of the announced length (or a source of unknown length) reached the end of
/// the source; otherwise its last segment is the one being written when the input stopped, shorter than the playlist
/// says (a jump in the video), so it is dropped and the next run makes it again.
fn run_end(st: &mut State, total: Option<u32>) -> RunEnd {
    let top = if st.run_segments > 0 { st.run_top } else { st.gen_start };
    // a run that ends early at the same place twice is a source shorter than it says, not a broken input
    let again = st.run_segments > 0 && st.early_end == Some(top);
    if total.is_none_or(|t| top.saturating_add(END_SLACK) >= t) || again {
        st.source_end = Some(top);
        return RunEnd::Source;
    }
    st.early_end = (st.run_segments > 0).then_some(top);
    if st.run_segments > 0 && st.run_top > st.gen_start {
        let last = st.run_top - 1;
        if let Some(b) = st.window.segs.remove(&last) {
            st.window.bytes -= b.len();
        }
        st.run_top = last;
    }
    RunEnd::Early
}

/// 0.4.1: what the supervisor does with a session ([`supervise_decision`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Supervise {
    /// Start the parked production again at this segment.
    Resume(u32),
    /// The run stalled: restart it at this segment.
    Restart(u32),
    /// The run stalled and the automatic restarts ran out.
    Fail,
}

/// 0.4.1: the part of a session's state [`supervise_decision`] looks at.
#[derive(Debug, Clone, Copy)]
pub struct RunView {
    /// Preparing or ready (not failed, not closed).
    pub active: bool,
    pub parked_at: Option<u32>,
    /// The next segment the current run produces.
    pub run_top: u32,
    pub playhead: u32,
    /// The highest segment a phone waits for.
    pub wanted: u32,
    pub finished: bool,
    pub held: u32,
    /// Time since the run's last sign of life.
    pub quiet: Duration,
    pub restart_cost: Option<Duration>,
    pub auto_restarts: u32,
}

/// 0.4.1: whether a parked run resumes, a silent run is restarted, or the session gives up. Pure.
///
/// * Parked: resume at its next segment once the playhead is within `restart cost + RESUME_MARGIN` segments of it (so
///   the restart's first segment arrives before the phone runs out), or as soon as a phone asks for it.
/// * Running, not held by back-pressure, not finished, and silent for `stall_timeout` (or twice the measured restart
///   cost, if longer): stalled. Restart where it stopped, up to [`MAX_AUTO_RESTARTS`] times in a row, then fail.
pub fn supervise_decision(v: &RunView, max_lead: u32, stall_timeout: Duration) -> Option<Supervise> {
    if !v.active {
        return None;
    }
    if let Some(at) = v.parked_at {
        let cost = v.restart_cost.map_or(8, |c| (c.as_secs_f64() / f64::from(SEGMENT_SECS)).ceil() as u32);
        let reach = (cost + RESUME_MARGIN).clamp(RESUME_MARGIN, max_lead.max(RESUME_MARGIN));
        return (at <= v.playhead.saturating_add(reach) || at <= v.wanted).then_some(Supervise::Resume(at));
    }
    if v.finished || v.held > 0 {
        return None;
    }
    let limit = v.restart_cost.map_or(stall_timeout, |c| stall_timeout.max(c * 2));
    if v.quiet < limit {
        return None;
    }
    Some(if v.auto_restarts >= MAX_AUTO_RESTARTS { Supervise::Fail } else { Supervise::Restart(v.run_top) })
}

pub struct Session {
    pub id: String,
    pub device_id: String,
    pub profile: &'static ProfileSpec,
    encoder: Encoder,
    input_url: String,
    fetch_token: String,
    pub sink_token: String,
    pub duration: Option<f64>,
    /// BUG-009: the engine torrent this session reads (its after-watch policy runs when the session ends).
    torrent_id: Option<String>,
    audio_track: u32,
    shader: Option<String>,
    /// LSFG profiles: the probed source rate (or the fallback), fixed for the session.
    lsfg_rate: Option<FrameRate>,
    workdir: PathBuf,
    state: Mutex<State>,
    notify: Notify,
    last_heartbeat: Mutex<Instant>,
    /// 0.4.0: the source's subtitles ([`crate::subtitles`]); shared with the session this one renews, if any.
    pub(crate) subs: Arc<SubtitleStore>,
}

impl Session {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn total_segments(&self) -> Option<u32> {
        self.duration.map(|d| (d / f64::from(SEGMENT_SECS)).ceil() as u32)
    }
}

#[derive(Serialize)]
pub struct SessionCreated {
    pub session_id: String,
    pub generation: u32,
    pub playback_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<f64>,
    pub segment_secs: u32,
    pub heartbeat_interval_secs: u64,
    pub idle_timeout_secs: u64,
}

/// `(kind, renewed session id)` of a create request, for the BUG-131 log lines ("-" when it renews nothing).
fn source_log_fields(src: &SourceRequest) -> (&'static str, String) {
    match src {
        SourceRequest::Library { .. } => ("library", "-".to_owned()),
        SourceRequest::Session { session_id } => ("session", session_id.clone()),
        SourceRequest::Torrent { .. } => ("torrent", "-".to_owned()),
        SourceRequest::Url { .. } => ("url", "-".to_owned()),
    }
}

/// `POST /v1/sessions` body (already validated JSON shape).
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateRequest {
    pub source: SourceRequest,
    pub profile: Option<String>,
    #[serde(default)]
    pub audio_track: u32,
    #[serde(default)]
    pub start_secs: f64,
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum SourceRequest {
    Library { item_id: String },
    /// Contract v1 addition (BUG-008 / BUG-022 / BUG-038): the source of an earlier session of the SAME device, open or
    /// recently closed (the last [`ORIGINS_KEPT`]), e.g. to change its profile or to reopen it after the idle timeout
    /// at `start_secs`. A PC without it answers `400 request_invalid`.
    Session { session_id: String },
    /// Contract v1 addition (BUG-009): a torrent the PC adds to its own engine and streams to FFmpeg from its loopback
    /// server; `uri` = magnet or 40-hex info-hash, `media_id` + `episode` pick the file of a pack.
    Torrent { uri: String, media_id: String, episode: u32 },
    Url {
        url: String,
        #[serde(default)]
        headers: std::collections::BTreeMap<String, String>,
        #[serde(default)]
        extension_id: Option<String>,
        #[serde(default)]
        container: Option<String>,
    },
}

/// Back-pressure: segment `k` waits while it is more than `max_lead` segments past the playhead, unless a phone is
/// already waiting for it or for a later one (`wanted`, BUG-058). Pure.
pub fn lead_exceeded(k: u32, playhead: u32, wanted: u32, max_lead: u32) -> bool {
    k > playhead.saturating_add(max_lead) && k > wanted
}

/// BUG-095: whether a request `distance` segments past the running engine's frontier waits for it (`true`) or restarts
/// the engine there. Always waits within `min_ahead`; past it, up to [`MAX_WAIT_AHEAD`], only when the engine makes
/// those segments sooner than a restart produces its first one (both measured; unknown = restart, as before). Pure.
pub fn wait_for_frontier(distance: u32, min_ahead: u32, per_segment: Option<Duration>, restart_cost: Option<Duration>) -> bool {
    if distance <= min_ahead {
        return true;
    }
    match (per_segment, restart_cost) {
        (Some(per), Some(cost)) => distance <= MAX_WAIT_AHEAD && per * distance < cost,
        _ => false,
    }
}

/// Seconds of a run after which the sustain verdict is taken (BUG-025).
pub const SUSTAIN_WINDOW_SECS: u64 = 20;

/// BUG-025: whether the engine keeps up with real time. Held back by back-pressure = it was ahead (sustained). Otherwise,
/// after [`SUSTAIN_WINDOW_SECS`] of an unpaused run (timed from its first segment), it must have produced at least 95 % of that time in segments.
/// `None` = not decided yet. Pure.
pub fn sustain_verdict(throttled: bool, segments: u32, elapsed: Duration, paused: bool) -> Option<bool> {
    if throttled {
        return Some(true);
    }
    if paused || elapsed < Duration::from_secs(SUSTAIN_WINDOW_SECS) {
        return None;
    }
    // `segments` counts the first one, which starts the clock
    let produced = f64::from(segments.saturating_sub(1)) * f64::from(SEGMENT_SECS);
    Some(produced >= 0.95 * elapsed.as_secs_f64())
}

/// `(session id, device id, source, opened torrent stream)` of an earlier session ([`SourceRequest::Session`]).
type Origin = (String, String, SourceRequest, Option<crate::OpenedTorrent>);

/// A source opened for a session that does not own it yet. Dropping it (an error, [`PREPARE_TIMEOUT`]) gives back its
/// fetch token and its torrent (`torrent_session_ended`), so no failure path leaves the torrent counted as read.
struct OpenedSource {
    input_url: String,
    duration: Option<f64>,
    token: Option<String>,
    torrent: Option<crate::OpenedTorrent>,
    fetcher: Arc<Fetcher>,
    host: Arc<dyn crate::LinkHost>,
}

impl OpenedSource {
    /// The session takes over: its fetch token and torrent are released by [`Manager::close`].
    fn keep(mut self) -> (String, Option<crate::OpenedTorrent>) {
        (self.token.take().unwrap_or_default(), self.torrent.take())
    }
}

impl Drop for OpenedSource {
    fn drop(&mut self) {
        if let Some(t) = self.token.take() {
            self.fetcher.unregister(&t);
        }
        if let Some(t) = self.torrent.take() {
            self.host.torrent_session_ended(&t.torrent_id);
        }
    }
}

/// What [`Manager::prepare`] found before a session is built.
struct Prepared {
    profile: &'static ProfileSpec,
    encoder: Encoder,
    source: SourceRequest,
    opened: OpenedSource,
    duration: Option<f64>,
    lsfg_rate: Option<FrameRate>,
}

pub struct Manager {
    pub(crate) tuning: SessionTuning,
    pub(crate) engine: Arc<dyn Engine>,
    pub(crate) fetcher: Arc<Fetcher>,
    pub(crate) env: Environment,
    pub(crate) shaders_dir: Option<PathBuf>,
    pub(crate) work_root: PathBuf,
    pub(crate) allow_http: bool,
    pub(crate) host: Arc<dyn crate::LinkHost>,
    events: tokio::sync::broadcast::Sender<crate::LinkEvent>,
    pub(crate) sink_addr: Mutex<Option<std::net::SocketAddr>>,
    sessions: Mutex<HashMap<String, Arc<Session>>>,
    by_device: Mutex<HashMap<String, String>>,
    device_generation: Mutex<HashMap<String, u32>>,
    tombstones: Mutex<VecDeque<String>>,
    /// BUG-025: where measured sustain verdicts go (`/v1/capabilities` `measured_sustain`), and the sessions measured.
    bench: Mutex<Option<Arc<Mutex<crate::media::capabilities::BenchStore>>>>,
    measured: Mutex<std::collections::HashSet<String>>,
    /// `(session id, device id, source)` of the last [`ORIGINS_KEPT`] sessions ([`SourceRequest::Session`]).
    /// BUG-131: torrent sources also keep the stream they opened, so a renew reuses the torrent instead of resolving it
    /// again.
    origins: Mutex<VecDeque<Origin>>,
    next_run: AtomicU32,
    /// 0.4.0: the subtitles of each device's last source ([crate::subtitles::SubtitleStore]), kept across its renewals.
    subtitle_stores: Mutex<HashMap<String, (SourceRequest, Arc<SubtitleStore>)>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Manager {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tuning: SessionTuning,
        engine: Arc<dyn Engine>,
        fetcher: Arc<Fetcher>,
        env: Environment,
        shaders_dir: Option<PathBuf>,
        work_root: PathBuf,
        allow_http: bool,
        host: Arc<dyn crate::LinkHost>,
        events: tokio::sync::broadcast::Sender<crate::LinkEvent>,
    ) -> Arc<Self> {
        Arc::new(Self {
            tuning,
            engine,
            fetcher,
            env,
            shaders_dir,
            work_root,
            allow_http,
            host,
            events,
            sink_addr: Mutex::new(None),
            sessions: Mutex::default(),
            by_device: Mutex::default(),
            device_generation: Mutex::default(),
            tombstones: Mutex::default(),
            origins: Mutex::default(),
            bench: Mutex::default(),
            measured: Mutex::default(),
            next_run: AtomicU32::new(1),
            subtitle_stores: Mutex::default(),
        })
    }

    /// BUG-025: store measured sustain verdicts here.
    pub fn set_bench(&self, bench: Arc<Mutex<crate::media::capabilities::BenchStore>>) {
        *lock(&self.bench) = Some(bench);
    }

    /// Record the first verdict of session `s` for its profile (later runs of the same session do not overwrite it).
    fn note_sustain(&self, s: &Arc<Session>, sustained: bool) {
        if !lock(&self.measured).insert(s.id.clone()) {
            return;
        }
        tracing::info!(profile = s.profile.id, sustained, "Link profile sustain measured");
        if let Some(b) = lock(&self.bench).as_ref() {
            lock(b).record(s.profile.id, sustained);
        }
    }

    pub fn tuning(&self) -> &SessionTuning {
        &self.tuning
    }

    pub fn get(&self, id: &str) -> Option<Arc<Session>> {
        lock(&self.sessions).get(id).cloned()
    }

    pub fn by_sink_token(&self, token: &str) -> Option<Arc<Session>> {
        lock(&self.sessions).values().find(|s| s.sink_token == token).cloned()
    }

    /// The session of `device`, or why not: `Gone` for a tombstone, `NotFound` otherwise.
    pub fn owned(&self, id: &str, device: &str) -> ApiResult<Arc<Session>> {
        match self.get(id) {
            Some(s) if s.device_id == device => Ok(s),
            Some(_) => Err(ErrorCode::SessionNotFound.into()),
            None if lock(&self.tombstones).iter().any(|t| t == id) => Err(ErrorCode::SessionGone.into()),
            None => Err(ErrorCode::SessionNotFound.into()),
        }
    }

    pub fn active_count(&self) -> usize {
        lock(&self.sessions).len()
    }

    /// Devices that own a session right now.
    pub fn device_ids(&self) -> Vec<String> {
        lock(&self.by_device).keys().cloned().collect()
    }

    // ---- create / close ---------------------------------------------------------------------------------------------

    pub async fn create(self: &Arc<Self>, device_id: &str, req: CreateRequest) -> ApiResult<SessionCreated> {
        // BUG-131: one line per request and one per outcome, so a profile change the phone sent can be told apart from
        // one that never reached the PC (a refusal answers before any other log line).
        let (kind, renews) = source_log_fields(&req.source);
        let profile = req.profile.clone().unwrap_or_else(|| profiles::DEFAULT_PROFILE.to_owned());
        tracing::info!(device = device_id, source = kind, renews = %renews, profile = %profile, start_secs = req.start_secs, "Link session requested");
        let result = self.create_inner(device_id, req).await;
        match &result {
            Ok(c) => tracing::info!(session = %c.session_id, source = kind, renews = %renews, profile = %profile, "Link session created"),
            Err(e) => tracing::info!(source = kind, renews = %renews, profile = %profile, error = e.0.as_str(), "Link session refused"),
        }
        result
    }

    async fn create_inner(self: &Arc<Self>, device_id: &str, req: CreateRequest) -> ApiResult<SessionCreated> {
        if !req.start_secs.is_finite() || req.start_secs < 0.0 || req.audio_track > 32 {
            return Err(ErrorCode::RequestInvalid.into());
        }
        // BUG-131: a fresh torrent may wait for its metadata (the phone waits longer for those, `TORRENT_CREATE_SECS`);
        // everything else answers within [`PREPARE_TIMEOUT`]. Dropping the preparation releases what it opened.
        let Prepared { profile, encoder, source, opened, duration, lsfg_rate } = if matches!(req.source, SourceRequest::Torrent { .. }) {
            self.prepare(device_id, &req).await?
        } else {
            match tokio::time::timeout(self.tuning.prepare_timeout, self.prepare(device_id, &req)).await {
                Ok(r) => r?,
                Err(_) => {
                    tracing::warn!(secs = self.tuning.prepare_timeout.as_secs_f32(), "Link session source not ready in time");
                    return Err(ErrorCode::SourceUnavailable.into());
                }
            }
        };

        let id = uuid::Uuid::new_v4().to_string();
        let workdir = self.work_root.join(&id);
        std::fs::create_dir_all(&workdir).map_err(|_| ApiError(ErrorCode::Internal))?;
        let shader = args::prepare_shader(profile, self.shaders_dir.as_deref(), &workdir).map_err(|e| {
            tracing::warn!(error = %e, "Anime4K shader unavailable");
            let _ = std::fs::remove_dir_all(&workdir);
            ApiError(ErrorCode::ProfileUnavailable)
        })?;
        let input_url = opened.input_url.clone();
        let (fetch_token, torrent) = opened.keep();
        let torrent_id = torrent.as_ref().map(|t| t.torrent_id.clone());

        // 0.4.0: a renewal of the same source (profile change, Phone | PC | Mixto switch, reopen after the idle timeout; by
        // `session` or by the source itself) keeps its subtitles, extracted or not: the source would be listed and read whole
        // again. One store per device: its last source.
        let subs = {
            let mut stores = lock(&self.subtitle_stores);
            match stores.get(device_id) {
                Some((src, store)) if *src == source => store.clone(),
                _ => {
                    let store = Arc::new(SubtitleStore::default());
                    stores.insert(device_id.to_owned(), (source.clone(), store.clone()));
                    store
                }
            }
        };

        // one active session per device. BUG-131: only now, with the new source already open (a renewed torrent
        // session holds its torrent twice for a moment, so closing the old one never ends the torrent's playback).
        self.close_device_session(device_id);

        let start_segment = (req.start_secs / f64::from(SEGMENT_SECS)).floor() as u32;
        let total = duration.map(|d| (d / f64::from(SEGMENT_SECS)).ceil() as u32);
        let start_segment = total.map_or(start_segment, |t| start_segment.min(t.saturating_sub(1)));

        let session = Arc::new(Session {
            id: id.clone(),
            device_id: device_id.to_owned(),
            profile,
            encoder,
            input_url,
            fetch_token,
            torrent_id,
            sink_token: uuid::Uuid::new_v4().simple().to_string(),
            duration,
            audio_track: req.audio_track,
            shader,
            lsfg_rate,
            workdir,
            state: Mutex::new(State {
                phase: Phase::Preparing,
                run_gen: 0,
                gen_start: start_segment,
                window: Window::default(),
                playhead: start_segment,
                paused: false,
                finished: false,
                kill: None,
                throttled: false,
                wanted: start_segment,
                event_top: start_segment,
                run_started: Instant::now(),
                run_segments: 0,
                run_throttled: false,
                run_top: start_segment,
                run_spawned: Instant::now(),
                restart_cost: None,
                seg_interval: None,
                last_stored: None,
                parked_at: None,
                held: 0,
                no_wait: false,
                last_life: Instant::now(),
                auto_restarts: 0,
                source_end: None,
                early_end: None,
            }),
            notify: Notify::new(),
            last_heartbeat: Mutex::new(Instant::now()),
            subs,
        });
        lock(&self.sessions).insert(id.clone(), session.clone());
        {
            let mut o = lock(&self.origins);
            o.push_back((id.clone(), device_id.to_owned(), source, torrent));
            while o.len() > ORIGINS_KEPT {
                o.pop_front();
            }
        }
        lock(&self.by_device).insert(device_id.to_owned(), id.clone());
        let generation = {
            let mut g = lock(&self.device_generation);
            let e = g.entry(device_id.to_owned()).or_insert(0);
            *e += 1;
            *e
        };
        self.start_run(&session, start_segment).await;
        self.list_subtitles(&session);
        let _ = self.events.send(crate::LinkEvent::SessionStarted { device_id: device_id.to_owned(), session_id: id.clone() });
        Ok(SessionCreated {
            playback_path: format!("/v1/sessions/{id}/master.m3u8"),
            session_id: id,
            generation,
            duration_secs: duration,
            segment_secs: SEGMENT_SECS,
            heartbeat_interval_secs: HEARTBEAT_SECS,
            idle_timeout_secs: self.tuning.idle_timeout.as_secs(),
        })
    }

    /// The source of session `id` if `device` owns it (open or among the last [`ORIGINS_KEPT`]).
    fn origin_of(&self, device: &str, id: &str) -> Option<(SourceRequest, Option<crate::OpenedTorrent>)> {
        lock(&self.origins).iter().rev().find(|(sid, dev, _, _)| sid == id && dev == device).map(|(_, _, src, t)| (src.clone(), t.clone()))
    }

    /// Everything a session needs that can fail or take time: the profile, the source (opened) and the probe.
    async fn prepare(&self, device_id: &str, req: &CreateRequest) -> ApiResult<Prepared> {
        // profile
        let wanted = req.profile.as_deref().unwrap_or(profiles::DEFAULT_PROFILE);
        let profile = profiles::find(wanted).filter(|p| self.env.profile_available(p)).ok_or(ApiError(ErrorCode::ProfileUnavailable))?;
        let encoder = self.env.encoder_for(profile).ok_or(ApiError(ErrorCode::ProfileUnavailable))?;

        // source -> loopback input (a `session` source reuses the source of an earlier session of this device, and
        // BUG-131 the torrent stream it opened while the host still has that torrent)
        let (source, reuse) = match &req.source {
            SourceRequest::Session { session_id } => self.origin_of(device_id, session_id).ok_or(ApiError(ErrorCode::SessionNotFound))?,
            other => (other.clone(), None),
        };
        let opened = self.open_source(&source, reuse).await?;

        // one probe for what the source does not say: the frame rate (LSFG) and, BUG-009 / BUG-023, the duration of
        // sources that do not announce it (a torrent file, an MP4 by URL). With a duration the phone gets a VOD playlist
        // (real length, seek anywhere); without one it gets a growing EVENT playlist, which Media3 declares stuck when
        // back-pressure holds it still.
        let needs_rate = profile.fg.lsfg().is_some();
        let info = if needs_rate || opened.duration.is_none() { self.engine.source_info(&opened.input_url).await } else { SourceInfo::default() };
        // BUG-024: a link ffprobe cannot read (e.g. a full URL that answers 404) is refused here, so the phone says "the
        // PC could not open this link" instead of playing a session that fails a minute later
        if info.unreadable && matches!(source, SourceRequest::Url { .. } | SourceRequest::Torrent { .. }) {
            tracing::warn!("Link session refused: the source cannot be read");
            return Err(ErrorCode::SourceUnavailable.into());
        }
        let duration = opened.duration.or(info.duration_secs);
        if duration.is_none() {
            tracing::info!("source duration unknown; the session uses a growing (EVENT) playlist");
        }
        let lsfg_rate = needs_rate.then(|| {
            info.rate.unwrap_or_else(|| {
                tracing::info!("source frame rate unknown; LSFG assumes 24000/1001");
                FALLBACK_RATE
            })
        });
        Ok(Prepared { profile, encoder, source, opened, duration, lsfg_rate })
    }

    fn opened(&self, input_url: String, token: String, duration: Option<f64>, torrent: Option<crate::OpenedTorrent>) -> OpenedSource {
        OpenedSource { input_url, duration, token: Some(token), torrent, fetcher: self.fetcher.clone(), host: self.host.clone() }
    }

    /// The loopback input of a torrent stream the host opened (or held again): `opened` is released on any error.
    async fn torrent_input(&self, opened: crate::OpenedTorrent) -> ApiResult<OpenedSource> {
        // from here on, dropping `held` gives the torrent back
        let mut held = OpenedSource { input_url: String::new(), duration: None, token: None, torrent: Some(opened), fetcher: self.fetcher.clone(), host: self.host.clone() };
        let url = held.torrent.as_ref().map(|t| t.url.clone()).unwrap_or_default();
        let target = Url::parse(&url).map_err(|_| ApiError(ErrorCode::Internal))?;
        let trusted = crate::torrent_stream_origin(&target).ok_or(ApiError(ErrorCode::Internal))?;
        let token = self.fetcher.register_unmetered(SourceSpec { url: target.clone(), headers: Vec::new(), extension_permissions: None, trusted: vec![trusted] });
        held.token = Some(token.clone());
        if let Err(e) = self.fetcher.validate(&token, &target).await {
            return Err(match e {
                FetchError::Denied => ErrorCode::UrlSourceDenied.into(),
                FetchError::Unavailable => ErrorCode::SourceUnavailable.into(),
            });
        }
        held.input_url = self.fetcher.entry_url(&token, &target, false);
        Ok(held)
    }

    async fn open_source(&self, src: &SourceRequest, reuse: Option<crate::OpenedTorrent>) -> ApiResult<OpenedSource> {
        match src {
            SourceRequest::Session { .. } => Err(ErrorCode::RequestInvalid.into()),
            SourceRequest::Torrent { uri, media_id, episode } => {
                if uri.is_empty() || uri.len() > 4096 || uri.chars().any(char::is_control) || media_id.is_empty() || media_id.len() > 128 || *episode == 0 {
                    return Err(ErrorCode::RequestInvalid.into());
                }
                // BUG-131: a renew of a torrent session holds the torrent the earlier session opened (still in the
                // engine: no new lookup, no extension, no metadata wait); only when the host lost it is it opened again
                if let Some(prev) = reuse {
                    if self.host.reuse_torrent(&prev.torrent_id).await {
                        tracing::info!(torrent = %prev.torrent_id, "Link session reuses the torrent of the renewed session");
                        return self.torrent_input(prev).await;
                    }
                    tracing::info!(torrent = %prev.torrent_id, "the torrent of the renewed session is gone; opening it again");
                }
                let req = crate::TorrentRequest { uri: uri.clone(), media_id: media_id.clone(), episode: *episode };
                let opened = tokio::time::timeout(TORRENT_OPEN_TIMEOUT, self.host.open_torrent(req))
                    .await
                    .map_err(|_| ApiError(ErrorCode::SourceUnavailable))?
                    .map_err(ApiError)?;
                self.torrent_input(opened).await
            }
            SourceRequest::Library { item_id } => {
                if item_id.len() != 32 || !item_id.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err(ErrorCode::RequestInvalid.into());
                }
                let path = self.host.library_path(item_id).ok_or(ApiError(ErrorCode::LibraryItemNotFound))?;
                if !path.is_file() {
                    return Err(ErrorCode::LibraryItemNotFound.into());
                }
                let duration = self.host.library().iter().find(|i| &i.id == item_id).map(|i| i.duration_secs).filter(|d| *d > 0.0);
                let token = self.fetcher.register_file(path);
                Ok(self.opened(self.fetcher.file_url(&token), token, duration, None))
            }
            SourceRequest::Url { url, headers, extension_id, container } => {
                if url.len() > 4096 {
                    return Err(ErrorCode::RequestInvalid.into());
                }
                let target = Url::parse(url).map_err(|_| ApiError(ErrorCode::RequestInvalid))?;
                // headers: allow-list, size, CR/LF -> 400 (before any policy decision)
                let policy = Policy::link_source(self.allow_http);
                let headers = sanitize_headers(&policy, headers).map_err(|_| ApiError(ErrorCode::RequestInvalid))?;
                let hls = match container.as_deref() {
                    Some("hls") => true,
                    Some("mp4") => false,
                    Some(_) => return Err(ErrorCode::RequestInvalid.into()), // dash: not supported in M0
                    None => target.path().to_ascii_lowercase().ends_with(".m3u8"),
                };
                let perms = extension_id.as_deref().and_then(|id| self.host.extension_permissions(id));
                let trusted = self.host.trusted_media_origins();
                let token = self.fetcher.register(SourceSpec { url: target.clone(), headers, extension_permissions: perms, trusted });
                let result = async {
                    self.fetcher.validate(&token, &target).await?;
                    if hls { self.fetcher.probe_hls(&token, &target).await } else { Ok(None) }
                }
                .await;
                match result {
                    Ok(duration) => Ok(self.opened(self.fetcher.entry_url(&token, &target, hls), token, duration, None)),
                    Err(e) => {
                        self.fetcher.unregister(&token);
                        Err(match e {
                            FetchError::Denied => ErrorCode::UrlSourceDenied.into(),
                            FetchError::Unavailable => ErrorCode::SourceUnavailable.into(),
                        })
                    }
                }
            }
        }
    }

    pub fn close_device_session(&self, device_id: &str) {
        let id = lock(&self.by_device).remove(device_id);
        if let Some(id) = id {
            self.close(&id);
        }
    }

    pub fn close(&self, id: &str) {
        let Some(s) = lock(&self.sessions).remove(id) else { return };
        {
            let mut st = s.state();
            st.phase = Phase::Closed;
            if let Some(k) = st.kill.take() {
                k.notify_one();
            }
            st.window = Window::default();
        }
        s.notify.notify_waiters();
        // 0.4.0: a listing / extraction this session runs reads its loopback input, which goes away below
        s.subs.abandon(id);
        {
            let mut by = lock(&self.by_device);
            if by.get(&s.device_id).is_some_and(|cur| cur == id) {
                by.remove(&s.device_id);
            }
        }
        self.fetcher.unregister(&s.fetch_token);
        if let Some(t) = &s.torrent_id {
            self.host.torrent_session_ended(t);
        }
        // the working dir is `<work_root>/<session id>`: a uuid we generated, never user input
        if s.workdir.parent() == Some(self.work_root.as_path()) {
            let _ = std::fs::remove_dir_all(&s.workdir);
        }
        let mut t = lock(&self.tombstones);
        t.push_back(id.to_owned());
        while t.len() > TOMBSTONES {
            t.pop_front();
        }
        lock(&self.measured).remove(id);
        let _ = self.events.send(crate::LinkEvent::SessionEnded { session_id: id.to_owned() });
        tracing::info!(session = %id, "session closed");
    }

    pub fn close_all(&self) {
        let ids: Vec<String> = lock(&self.sessions).keys().cloned().collect();
        for id in ids {
            self.close(&id);
        }
    }

    /// Close sessions of a revoked device.
    pub fn close_for_device(&self, device_id: &str) {
        lock(&self.subtitle_stores).remove(device_id);
        let ids: Vec<String> = lock(&self.sessions).values().filter(|s| s.device_id == device_id).map(|s| s.id.clone()).collect();
        for id in ids {
            self.close(&id);
        }
    }

    /// Close sessions whose last heartbeat is older than the idle timeout. Returns how many were closed.
    pub fn reap_idle(&self) -> usize {
        let idle: Vec<String> = lock(&self.sessions)
            .values()
            .filter(|s| lock(&s.last_heartbeat).elapsed() > self.tuning.idle_timeout)
            .map(|s| s.id.clone())
            .collect();
        for id in &idle {
            tracing::info!(session = %id, "closing idle session");
            self.close(id);
        }
        idle.len()
    }

    // ---- engine runs -------------------------------------------------------------------------------------------------

    /// Start (or restart) the engine at `start_segment`, killing the previous run.
    pub(crate) async fn start_run(self: &Arc<Self>, s: &Arc<Session>, start_segment: u32) {
        let sink_addr = lock(&self.sink_addr).unwrap_or_else(|| std::net::SocketAddr::from(([127, 0, 0, 1], 0)));
        let kill = Arc::new(Notify::new());
        let run_gen = self.next_run.fetch_add(1, Ordering::SeqCst);
        {
            let mut st = s.state();
            if st.phase == Phase::Closed {
                return;
            }
            if let Some(old) = st.kill.replace(kill.clone()) {
                old.notify_one();
            }
            st.run_gen = run_gen;
            st.gen_start = start_segment;
            // a (re)start at `k` means the consumer is at `k`; the next heartbeat refines it
            st.playhead = start_segment;
            // BUG-095: the segments of earlier runs stay (same profile, absolute timestamps via `-output_ts_offset`):
            // a seek back to what was already made is served from RAM. Only what the eviction rules drop is lost.
            st.run_top = start_segment;
            self.evict(&mut st);
            st.run_spawned = Instant::now();
            st.last_stored = None;
            st.finished = false;
            st.wanted = start_segment;
            st.run_started = Instant::now();
            st.run_segments = 0;
            st.run_throttled = false;
            // 0.4.1: a new run is neither parked nor held, and its silence is counted from now
            st.parked_at = None;
            st.held = 0;
            st.throttled = false;
            st.last_life = Instant::now();
            if matches!(st.phase, Phase::Failed(_)) {
                st.phase = Phase::Preparing;
            }
            tracing::debug!(k = start_segment, run_gen, playhead = st.playhead, "Link engine run starting");
        }
        let sink_base = format!("http://{sink_addr}/sink/{}/{run_gen}", s.sink_token);
        let backend = self.env.backend();
        let params = PipelineParams {
            profile: s.profile,
            encoder: s.encoder,
            backend: &backend,
            input_url: &s.input_url,
            sink_base: &sink_base,
            start_segment,
            audio_track: s.audio_track,
            shader_file: s.shader.as_deref(),
            adapter: 0,
        };
        let built = match (s.lsfg_rate, self.env.lsfg.paths()) {
            (Some(rate), Some(paths)) => args::build_lsfg(&params, rate, &paths.dll).map(|a| {
                (a.encode, Some(LsfgStages { decode: a.decode, helper: paths.helper.clone(), helper_args: a.helper }))
            }),
            (Some(_), None) => return self.fail(s, run_gen, FailReason::EngineUnavailable),
            (None, _) => args::build(&params).map(|a| (a, None)),
        };
        let (args, lsfg) = match built {
            Ok(a) => a,
            Err(e) => {
                tracing::warn!(error = %e, "the media graph could not be built");
                return self.fail(s, run_gen, FailReason::EngineUnavailable);
            }
        };
        let job = EngineJob { args, workdir: s.workdir.clone(), start_segment, sink_base, lsfg };
        let mut proc = match self.engine.spawn(job).await {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(error = %e, "engine could not start");
                return self.fail(s, run_gen, FailReason::EngineUnavailable);
            }
        };
        let me = self.clone();
        let session = s.clone();
        tokio::spawn(async move {
            tokio::select! {
                exit = proc.wait() => {
                    let current = session.state().run_gen == run_gen;
                    if current {
                        match exit {
                            EngineExit::Success => {
                                let (top, made, end) = {
                                    let mut st = session.state();
                                    st.finished = true;
                                    let end = run_end(&mut st, session.total_segments());
                                    (st.run_top, st.run_segments, end)
                                };
                                match end {
                                    RunEnd::Source => tracing::info!(session = %session.id, top, segments = made, "Link engine reached the end of the source"),
                                    RunEnd::Early => tracing::info!(session = %session.id, top, segments = made, "Link engine run ended before the end of the source: its last segment is made again by the next run"),
                                }
                                session.notify.notify_waiters();
                            }
                            EngineExit::Failed if me.fetcher.was_denied(&session.fetch_token) => me.fail(&session, run_gen, FailReason::UrlSourceDenied),
                            EngineExit::Failed => {
                                // 0.4.1: a run that already produced may have hit a transient read error (a torrent
                                // stream that dropped): end it like an early end (its last segment dropped), so the next
                                // request goes on from there, instead of failing the session
                                let retry = {
                                    let mut st = session.state();
                                    let retry = st.run_segments > 0 && st.auto_restarts < MAX_AUTO_RESTARTS && st.phase != Phase::Closed;
                                    if retry {
                                        st.auto_restarts += 1;
                                        st.finished = true;
                                        if st.run_top > st.gen_start {
                                            let last = st.run_top - 1;
                                            if let Some(b) = st.window.segs.remove(&last) {
                                                st.window.bytes -= b.len();
                                            }
                                            st.run_top = last;
                                        }
                                    }
                                    retry.then_some(st.run_top)
                                };
                                match retry {
                                    Some(at) => {
                                        tracing::warn!(session = %session.id, k = at, "Link engine run failed: the next request goes on from where it stopped");
                                        session.notify.notify_waiters();
                                    }
                                    None => me.fail(&session, run_gen, FailReason::SourceReadFailed),
                                }
                            }
                        }
                    }
                }
                () = kill.notified() => proc.kill(),
            }
        });
    }

    fn fail(&self, s: &Arc<Session>, run_gen: u32, reason: FailReason) {
        let mut st = s.state();
        if st.run_gen == run_gen && st.phase != Phase::Closed {
            st.phase = Phase::Failed(reason);
        }
        drop(st);
        s.notify.notify_waiters();
    }

    // ---- sink (called by the PUT receiver) ----------------------------------------------------------------------------

    /// Store a file PUT by FFmpeg. Delays the answer (back-pressure) while the run is too far ahead of the playhead.
    pub async fn sink_put(&self, s: &Arc<Session>, run_gen: u32, name: &str, body: Bytes) -> Result<(), SinkError> {
        enum Kind {
            Init,
            Segment(u32),
            Ignore,
        }
        let kind = if name == "init.mp4" {
            Kind::Init
        } else if name == "live.m3u8" || name.ends_with(".tmp") {
            Kind::Ignore
        } else if let Some(k) = name.strip_prefix("seg_").and_then(|r| r.strip_suffix(".m4s")).and_then(|k| k.parse::<u32>().ok()) {
            Kind::Segment(k)
        } else {
            return Err(SinkError::BadName);
        };
        match kind {
            Kind::Ignore => {
                // the playlist FFmpeg rewrites after each segment: a sign of life of the run
                let mut st = s.state();
                if st.run_gen == run_gen {
                    st.last_life = Instant::now();
                }
                Ok(())
            }
            Kind::Init => {
                let mut st = s.state();
                if st.run_gen != run_gen || st.phase == Phase::Closed {
                    return Err(SinkError::Stale);
                }
                st.window.init = Some(body);
                st.last_life = Instant::now();
                if st.phase == Phase::Preparing {
                    st.phase = Phase::Ready;
                }
                drop(st);
                s.notify.notify_waiters();
                Ok(())
            }
            Kind::Segment(k) => {
                // back-pressure: hold the answer while the run is too far ahead. `held` counts the PUTs held right now;
                // the guard gives this one back however the wait ends (stale, closed, released, dropped by hyper).
                struct Held<'a>(&'a Session, u32, bool);
                impl Drop for Held<'_> {
                    fn drop(&mut self) {
                        if self.2 {
                            let mut st = self.0.state();
                            if st.run_gen == self.1 {
                                st.held = st.held.saturating_sub(1);
                            }
                        }
                    }
                }
                let mut held = Held(s, run_gen, false);
                loop {
                    let park = {
                        let mut st = s.state();
                        if st.run_gen != run_gen || st.phase == Phase::Closed {
                            tracing::debug!(k, run_gen, current = st.run_gen, "sink: stale segment");
                            return Err(SinkError::Stale);
                        }
                        st.last_life = Instant::now();
                        let lead = self.lead(&st);
                        let too_far = lead_exceeded(k, st.playhead, st.wanted, lead);
                        if too_far != st.throttled {
                            tracing::debug!(k, playhead = st.playhead, wanted = st.wanted, held = too_far, "Link engine back-pressure");
                        }
                        st.throttled = too_far;
                        st.run_throttled |= too_far;
                        if !too_far {
                            break;
                        }
                        if !held.2 {
                            held.2 = true;
                            st.held += 1;
                        }
                        // 0.4.1: an encoder that waits for the answer never has two PUTs held at once. FFmpeg n9 does not
                        // wait (it keeps encoding into new connections while this one is held): park the run instead.
                        (st.held >= 2).then(|| self.park(s, &mut st))
                    };
                    if let Some(ahead) = park {
                        if ahead {
                            self.note_sustain(s, true);
                        }
                        s.notify.notify_waiters();
                        return Err(SinkError::Stale);
                    }
                    tokio::select! { () = s.notify.notified() => {}, () = tokio::time::sleep(Duration::from_millis(250)) => {} }
                }
                drop(held);
                let mut st = s.state();
                if st.run_gen != run_gen || st.phase == Phase::Closed {
                    return Err(SinkError::Stale);
                }
                // BUG-095: restart cost (first segment of a run) and time per segment (from the previous store, so the
                // back-pressure wait of the previous PUT is not counted)
                let now = Instant::now();
                match st.last_stored {
                    None => st.restart_cost = Some(now.duration_since(st.run_spawned)),
                    Some(prev) => {
                        let dt = now.duration_since(prev);
                        st.seg_interval = Some(st.seg_interval.map_or(dt, |old| (old * 3 + dt) / 4));
                    }
                }
                st.event_top = st.event_top.max(k + 1);
                st.run_top = st.run_top.max(k + 1);
                st.window.bytes += body.len();
                if let Some(old) = st.window.segs.insert(k, body) {
                    st.window.bytes -= old.len();
                }
                self.evict(&mut st);
                // timed from the first segment: FFmpeg start-up (probe, shaders, LSFG) is not throughput
                if st.run_segments == 0 {
                    st.run_started = Instant::now();
                }
                st.last_stored = Some(Instant::now());
                st.last_life = Instant::now();
                st.run_segments += 1;
                st.auto_restarts = 0;
                let verdict = sustain_verdict(st.run_throttled, st.run_segments, st.run_started.elapsed(), st.paused);
                tracing::debug!(k, run_gen, bytes = st.window.bytes, playhead = st.playhead, "sink: segment stored");
                if st.phase == Phase::Preparing && st.window.init.is_some() {
                    st.phase = Phase::Ready;
                }
                drop(st);
                if let Some(sustained) = verdict {
                    self.note_sustain(s, sustained);
                }
                s.notify.notify_waiters();
                Ok(())
            }
        }
    }

    /// How far (segments) the run may get ahead of the playhead before back-pressure: twice the lead once the encoder
    /// is known not to wait for the sink (0.4.1), so a parked run has a long stretch to play before it starts again.
    fn lead(&self, st: &State) -> u32 {
        if st.no_wait { self.tuning.max_lead_segments * 2 } else { self.tuning.max_lead_segments }
    }

    /// 0.4.1: stop production because the run is far enough ahead: kill it (its held PUTs become stale) and remember its
    /// next segment, where [`supervise_decision`] starts it again. Returns whether the run was held (sustain verdict).
    fn park(&self, s: &Session, st: &mut State) -> bool {
        if let Some(k) = st.kill.take() {
            k.notify_one();
        }
        let at = st.run_top;
        // a fresh generation without a process: whatever the killed run still sends is stale
        st.run_gen = self.next_run.fetch_add(1, Ordering::SeqCst);
        st.parked_at = Some(at);
        st.no_wait = true;
        st.held = 0;
        st.throttled = true;
        tracing::info!(session = %s.id, at, playhead = st.playhead, wanted = st.wanted, "Link engine parked: far enough ahead (the encoder does not wait for the sink)");
        st.run_throttled
    }

    /// 0.4.1: what [`supervise_decision`] sees of `st`.
    fn run_view(st: &State) -> RunView {
        RunView {
            active: matches!(st.phase, Phase::Preparing | Phase::Ready),
            parked_at: st.parked_at,
            run_top: st.run_top,
            playhead: st.playhead,
            wanted: st.wanted,
            finished: st.finished,
            held: st.held,
            quiet: st.last_life.elapsed(),
            restart_cost: st.restart_cost,
            auto_restarts: st.auto_restarts,
        }
    }

    /// 0.4.1: resume parked runs the phone is about to need, restart stalled ones, fail the hopeless ones. Called by the
    /// Link's periodic task (with the idle reaper) and after each heartbeat.
    pub fn supervise(self: &Arc<Self>) {
        let sessions: Vec<Arc<Session>> = lock(&self.sessions).values().cloned().collect();
        for s in sessions {
            let decision = {
                let mut st = s.state();
                let d = supervise_decision(&Self::run_view(&st), self.tuning.max_lead_segments, self.tuning.stall_timeout);
                match d {
                    Some(Supervise::Restart(_)) => st.auto_restarts += 1,
                    Some(Supervise::Fail) => {
                        if let Some(k) = st.kill.take() {
                            k.notify_one();
                        }
                        st.phase = Phase::Failed(FailReason::EngineStalled);
                    }
                    // taken once: the run starts below
                    Some(Supervise::Resume(_)) => st.parked_at = None,
                    None => {}
                }
                d.map(|d| (d, st.playhead, st.last_life.elapsed()))
            };
            let Some((decision, playhead, quiet)) = decision else { continue };
            let at = match decision {
                Supervise::Resume(at) => {
                    tracing::info!(session = %s.id, at, playhead, "Link engine resumed: the playhead is getting close");
                    at
                }
                Supervise::Restart(at) => {
                    tracing::warn!(session = %s.id, at, playhead, quiet_secs = quiet.as_secs(), "Link engine stalled: restarting it where it stopped");
                    at
                }
                Supervise::Fail => {
                    tracing::warn!(session = %s.id, playhead, quiet_secs = quiet.as_secs(), "Link engine stalled again: the session failed (engine_stalled)");
                    s.notify.notify_waiters();
                    continue;
                }
            };
            let me = self.clone();
            tokio::spawn(async move { me.start_run(&s, at).await });
        }
    }

    /// Drop segments from the RAM window. 0.4.1: what the phone is about to play (from just behind the playhead up to
    /// what the lead or a waiting request asked for) is never evicted, even over the budget (back-pressure and parking
    /// bound it); before, the budget could drop the segments right after a paused playhead.
    fn evict(&self, st: &mut State) {
        let keep_from = st.playhead.saturating_sub(KEEP_BEHIND);
        let stale: Vec<u32> = st.window.segs.range(..keep_from).map(|(k, _)| *k).collect();
        for k in stale {
            if let Some(b) = st.window.segs.remove(&k) {
                st.window.bytes -= b.len();
            }
        }
        let protect_from = st.playhead.saturating_sub(PROTECT_BEHIND);
        let protect_to = st.playhead.saturating_add(self.lead(st)).max(st.wanted);
        while st.window.bytes > self.tuning.max_ram_bytes {
            // what is behind the playhead first (oldest first), then what lies past the protected range (furthest first:
            // segments of earlier runs after a seek, or made past the lead)
            let first = st.window.segs.keys().next().copied().filter(|k| *k < protect_from);
            let last = st.window.segs.keys().next_back().copied().filter(|k| *k > protect_to);
            let Some(k) = first.or(last) else {
                tracing::debug!(bytes = st.window.bytes, playhead = st.playhead, "Link window over its budget with only protected segments");
                break;
            };
            if let Some(b) = st.window.segs.remove(&k) {
                st.window.bytes -= b.len();
                tracing::debug!(k, playhead = st.playhead, bytes = st.window.bytes, "Link window evicted a segment");
            }
        }
    }

    // ---- subtitles (0.4.0, crate::subtitles) ---------------------------------------------------------------------------

    /// Lists the source's subtitle tracks in the background (unless the renewed session's store already has them).
    fn list_subtitles(self: &Arc<Self>, s: &Arc<Session>) {
        let Some(cancel) = s.subs.begin_probe(&s.id) else { return };
        let (engine, store, owner, input) = (self.engine.clone(), s.subs.clone(), s.id.clone(), s.input_url.clone());
        tokio::spawn(async move {
            tokio::select! {
                tracks = engine.subtitle_tracks(&input) => {
                    if let Some(t) = &tracks {
                        tracing::info!(tracks = t.len(), text = t.iter().filter(|x| x.text).count(), "Link source subtitles listed");
                    }
                    store.finish_probe(&owner, tracks);
                }
                () = cancel.cancelled() => {}
            }
        });
    }

    /// The single extraction pass of the store of `s`, started by the first file request (an older phone never asks,
    /// so a session it plays never reads the source twice).
    fn extract_subtitles(self: &Arc<Self>, s: &Arc<Session>) {
        let Some((tracks, cancel)) = s.subs.begin_extraction(&s.id) else { return };
        let dir = self.work_root.join(format!("subs-{}", uuid::Uuid::new_v4().simple()));
        let outputs = subtitles::plan_outputs(&tracks, &dir);
        let (engine, store, owner, input) = (self.engine.clone(), s.subs.clone(), s.id.clone(), s.input_url.clone());
        tokio::spawn(async move {
            if outputs.is_empty() {
                store.finish_extraction(&owner, Some(Default::default()));
                return;
            }
            if std::fs::create_dir_all(&dir).is_err() {
                store.finish_extraction(&owner, None);
                return;
            }
            let _scratch = ScratchDir(dir);
            tracing::info!(files = outputs.len(), "Link subtitle extraction started");
            let started = Instant::now();
            let result = tokio::select! {
                r = tokio::time::timeout(subtitles::EXTRACT_TIMEOUT, engine.extract_subtitles(&input, &outputs)) => r,
                () = cancel.cancelled() => {
                    tracing::info!("Link subtitle extraction stopped: its session closed");
                    return;
                }
            };
            match result {
                Ok(Ok(())) => {
                    let files = tokio::task::spawn_blocking(move || subtitles::collect_outputs(&outputs)).await.ok();
                    tracing::info!(secs = started.elapsed().as_secs(), ok = files.is_some(), "Link subtitle extraction finished");
                    store.finish_extraction(&owner, files);
                }
                Ok(Err(e)) => {
                    tracing::warn!(error = %e, "Link subtitle extraction failed");
                    store.finish_extraction(&owner, None);
                }
                Err(_) => {
                    tracing::warn!("Link subtitle extraction timed out");
                    store.finish_extraction(&owner, None);
                }
            }
        });
    }

    /// `GET /v1/sessions/{id}/subtitles/{n}.{ass|vtt}`: the file, waiting up to [`SessionTuning::subtitle_wait`] for
    /// the listing / the pass. `409 session_not_ready` while listing, `503 segment_wait_timeout` + `Retry-After` while
    /// extracting, `404` for a track or format that is not served, `503 source_unavailable` when it cannot be.
    pub async fn subtitle(self: &Arc<Self>, s: &Arc<Session>, n: u32, format: SubFormat) -> ApiResult<Bytes> {
        let deadline = tokio::time::Instant::now() + self.tuning.subtitle_wait;
        loop {
            let notified = s.subs.notify.notified();
            if s.state().phase == Phase::Closed {
                return Err(ErrorCode::SessionGone.into());
            }
            let waiting = match s.subs.lookup(n, format) {
                Lookup::File(b) => return Ok(b),
                Lookup::NotFound => return Err(ErrorCode::SessionNotFound.into()),
                Lookup::Unavailable => return Err(ErrorCode::SourceUnavailable.into()),
                Lookup::Listing => ErrorCode::SessionNotReady,
                Lookup::Extracting { start } => {
                    if start {
                        self.extract_subtitles(s);
                        continue;
                    }
                    ErrorCode::SegmentWaitTimeout
                }
            };
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                return Err(waiting.into());
            }
        }
    }

    // ---- reads (HLS surface) ------------------------------------------------------------------------------------------

    pub fn heartbeat(self: &Arc<Self>, s: &Arc<Session>, position_secs: f64, paused: bool) {
        *lock(&s.last_heartbeat) = Instant::now();
        let mut st = s.state();
        st.playhead = (position_secs.max(0.0) / f64::from(SEGMENT_SECS)).floor() as u32;
        st.paused = paused;
        drop(st);
        s.notify.notify_waiters();
        // 0.4.1: a parked run resumes as soon as the playhead gets close, not at the next tick
        self.supervise();
    }

    pub fn phase(&self, s: &Arc<Session>) -> Phase {
        s.state().phase
    }

    /// `init.mp4`, waiting for the engine if needed.
    pub async fn init_segment(self: &Arc<Self>, s: &Arc<Session>) -> ApiResult<Bytes> {
        let deadline = tokio::time::Instant::now() + self.tuning.segment_wait;
        loop {
            let notified = s.notify.notified();
            {
                let st = s.state();
                match st.phase {
                    Phase::Closed => return Err(ErrorCode::SessionGone.into()),
                    // 0.4.1: final (was 503 source_unavailable, which players retry for a minute)
                    Phase::Failed(_) => return Err(ErrorCode::SegmentUnavailable.into()),
                    _ => {}
                }
                if let Some(b) = &st.window.init {
                    return Ok(b.clone());
                }
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                return Err(ErrorCode::SegmentWaitTimeout.into());
            }
        }
    }

    /// Segment `k`: from the window, or restart the engine there and wait.
    pub async fn segment(self: &Arc<Self>, s: &Arc<Session>, k: u32) -> ApiResult<Bytes> {
        if s.total_segments().is_some_and(|n| k >= n) {
            return Err(ErrorCode::SessionNotFound.into());
        }
        let deadline = tokio::time::Instant::now() + self.tuning.segment_wait;
        let mut restarted = false;
        loop {
            let notified = s.notify.notified();
            let restart_at = {
                let mut st = s.state();
                match st.phase {
                    Phase::Closed => return Err(ErrorCode::SessionGone.into()),
                    // 0.4.1: a failed session never makes it: final, so the phone opens a new session instead of
                    // retrying a 503 (before: `source_unavailable` forever)
                    Phase::Failed(_) => return Err(ErrorCode::SegmentUnavailable.into()),
                    _ => {}
                }
                if let Some(b) = st.window.segs.get(&k) {
                    return Ok(b.clone());
                }
                // 0.4.1: the pipeline reached the end of the source before the announced length: nothing to make here
                if st.source_end.is_some_and(|e| k >= e) {
                    return Err(ErrorCode::SessionNotFound.into());
                }
                // BUG-095: the frontier of the running engine (the window also holds earlier runs' segments)
                let frontier = st.run_top;
                // not in the window and already passed by this run (or before its start): evicted or never made here
                let behind = k < st.run_top;
                let too_far = k >= frontier
                    && !wait_for_frontier(k - frontier, self.tuning.seek_ahead_segments, st.seg_interval, st.restart_cost);
                // finished runs never produce k again; a parked run is not running (0.4.1)
                let dead = st.finished;
                let parked = st.parked_at.is_some();
                let restart = (behind || too_far || dead || parked) && !restarted;
                if !restart && k > st.wanted {
                    // the phone waits for k: let the engine produce it even past the lead (BUG-058). Wake the held PUT
                    // and look again with a fresh `notified` (this one already saw the wake-up).
                    st.wanted = k;
                    drop(st);
                    s.notify.notify_waiters();
                    continue;
                }
                if restart && dead && !behind && !too_far && st.run_segments == 0 {
                    // 0.4.1: the run ended without making anything (an input that breaks right away): restarting it
                    // forever is the "preparing" loop. Count it, and give up after MAX_AUTO_RESTARTS.
                    st.auto_restarts += 1;
                    if st.auto_restarts > MAX_AUTO_RESTARTS {
                        tracing::warn!(session = %s.id, k, "Link engine runs end without making anything: the session failed (engine_stalled)");
                        st.phase = Phase::Failed(FailReason::EngineStalled);
                        drop(st);
                        s.notify.notify_waiters();
                        return Err(ErrorCode::SegmentUnavailable.into());
                    }
                }
                // a parked or early-ended run goes on from where it stopped (contiguous) when k is within reach of it
                let at = if (parked || dead) && !behind && !too_far { frontier } else { k };
                let reason = if parked {
                    "resume parked production"
                } else if behind {
                    "behind the window"
                } else if too_far {
                    "past the frontier"
                } else {
                    "after the run ended"
                };
                restart.then(|| {
                    st.parked_at = None;
                    (at, reason)
                })
            };
            if let Some((at, reason)) = restart_at {
                tracing::info!(k, at, reason, "segment outside the window: restarting the engine there");
                restarted = true;
                self.start_run(s, at).await;
                continue;
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                tracing::debug!(k, "segment wait timed out");
                return Err(ErrorCode::SegmentWaitTimeout.into());
            }
        }
    }

    /// `media.m3u8`: VOD (known duration) or EVENT (growing) playlist.
    pub fn media_playlist(&self, s: &Arc<Session>) -> ApiResult<String> {
        let st = s.state();
        match st.phase {
            Phase::Closed => return Err(ErrorCode::SessionGone.into()),
            Phase::Failed(_) => return Err(ErrorCode::SourceUnavailable.into()),
            Phase::Preparing => return Err(ErrorCode::SessionNotReady.into()),
            Phase::Ready => {}
        }
        let secs = f64::from(SEGMENT_SECS);
        let mut p = String::from("#EXTM3U\n#EXT-X-VERSION:7\n");
        p.push_str(&format!("#EXT-X-TARGETDURATION:{SEGMENT_SECS}\n#EXT-X-MEDIA-SEQUENCE:0\n"));
        match s.duration {
            Some(d) => {
                p.push_str("#EXT-X-PLAYLIST-TYPE:VOD\n#EXT-X-INDEPENDENT-SEGMENTS\n#EXT-X-MAP:URI=\"init.mp4\"\n");
                let n = s.total_segments().unwrap_or(0);
                for k in 0..n {
                    let len = if k + 1 == n { (d - secs * f64::from(k)).clamp(0.001, secs) } else { secs };
                    p.push_str(&format!("#EXTINF:{len:.6},\nseg_{k}.m4s\n"));
                }
                p.push_str("#EXT-X-ENDLIST\n");
            }
            None => {
                p.push_str("#EXT-X-PLAYLIST-TYPE:EVENT\n#EXT-X-INDEPENDENT-SEGMENTS\n#EXT-X-MAP:URI=\"init.mp4\"\n");
                let top = st.window.segs.keys().next_back().map_or(st.gen_start, |m| m + 1).max(st.event_top);
                for k in 0..top {
                    p.push_str(&format!("#EXTINF:{secs:.6},\nseg_{k}.m4s\n"));
                }
                if st.finished {
                    p.push_str("#EXT-X-ENDLIST\n");
                }
            }
        }
        Ok(p)
    }

    pub fn master_playlist(&self, s: &Arc<Session>) -> ApiResult<String> {
        match s.state().phase {
            Phase::Closed => return Err(ErrorCode::SessionGone.into()),
            Phase::Failed(_) => return Err(ErrorCode::SourceUnavailable.into()),
            Phase::Preparing => return Err(ErrorCode::SessionNotReady.into()),
            Phase::Ready => {}
        }
        let bw = u64::from(s.profile.video_kbps) * 1000 + 192_000;
        Ok(format!(
            "#EXTM3U\n#EXT-X-VERSION:7\n#EXT-X-INDEPENDENT-SEGMENTS\n#EXT-X-STREAM-INF:BANDWIDTH={bw},RESOLUTION={}x{}\nmedia.m3u8\n",
            s.profile.width, s.profile.height
        ))
    }

    pub fn state_json(&self, s: &Arc<Session>, instance_id: &str) -> serde_json::Value {
        let st = s.state();
        let (state, error) = match st.phase {
            Phase::Preparing => ("preparing", None),
            Phase::Ready => ("ready", None),
            Phase::Failed(r) => ("failed", Some(r.as_str())),
            Phase::Closed => ("closed", None),
        };
        let frontier = st.window.segs.range(st.gen_start..st.run_top).next_back().map(|(k, _)| *k);
        let mut v = serde_json::json!({
            "state": state,
            "generation": st.run_gen,
            "instance_id": instance_id,
            "profile": s.profile.id,
            "throttled": st.throttled,
            "output": { "width": s.profile.width, "height": s.profile.height },
            "window": { "first_segment": st.window.segs.keys().next().copied().unwrap_or(st.gen_start), "bytes": st.window.bytes },
        });
        if let Some(f) = frontier {
            v["frontier_segment"] = f.into();
            v["lead_secs"] = (f64::from(f.saturating_sub(st.playhead)) * f64::from(SEGMENT_SECS)).into();
        }
        if let Some(e) = error {
            v["error"] = e.into();
        }
        // 0.4.1 (additive): production parked (no engine running until the playhead gets close), the real end of the
        // source once the pipeline reached it, and what the PC holds around the playhead (for the phone's seek bar)
        v["parked"] = st.parked_at.is_some().into();
        if let Some(e) = st.source_end {
            v["ended"] = true.into();
            let end = f64::from(e) * f64::from(SEGMENT_SECS);
            v["end_secs"] = s.duration.map_or(end, |d| d.min(end)).into();
        }
        if let Some((from, to)) = produced_range(&st.window.segs, st.playhead) {
            let secs = |k: u32| {
                let t = f64::from(k) * f64::from(SEGMENT_SECS);
                s.duration.map_or(t, |d| d.min(t))
            };
            v["produced"] = serde_json::json!({ "from_secs": secs(from), "to_secs": secs(to) });
        }
        // 0.4.0 (Tatakai Link Phone | PC | Mixed): what the PC does with this session, so the phone can say it in the
        // player. Additive fields: older phones ignore them, older PCs simply do not send them.
        if let (Some(obj), serde_json::Value::Object(extra)) = (v.as_object_mut(), processing_json(s.profile, s.encoder, s.lsfg_rate)) {
            obj.extend(extra);
        }
        if let Some(fps) = v["fg_effective"]["out_fps"].as_f64() {
            v["output"]["fps"] = fps.into();
        }
        // 0.4.0: the source's subtitles (additive; absent while listing or when the PC cannot list them)
        drop(st);
        if let Some(tracks) = s.subs.tracks() {
            v["subtitles"] = serde_json::to_value(tracks).unwrap_or_default();
        }
        v["subtitles_state"] = s.subs.state_str().into();
        v
    }
}

/// 0.4.1: the run of consecutive segments the window holds around `playhead`, as `[from, to)` segment numbers: the one
/// that contains it, or else the first that starts after it (a seek ahead the PC is making). Pure.
pub fn produced_range(segs: &BTreeMap<u32, Bytes>, playhead: u32) -> Option<(u32, u32)> {
    let start = if segs.contains_key(&playhead) {
        // walk back to the start of the run that holds the playhead
        let mut from = playhead;
        while from > 0 && segs.contains_key(&(from - 1)) {
            from -= 1;
        }
        from
    } else {
        *segs.range(playhead..).next()?.0
    };
    let mut to = start;
    while segs.contains_key(&to) {
        to += 1;
    }
    Some((start, to))
}

/// 0.4.0: the processing a session runs, as additive `GET /v1/sessions/{id}` fields: `upscale` (`none` | `anime4k`),
/// `anime4k` (`{mode, variant}`, only with Anime4K), `fg` (`none` | `frc_amf` | `lsfg`), `encoder`, `video_kbps` and
/// `fg_effective` (`{multiplier, source_fps?, out_fps?}`, only with frame generation). For the LSFG auto profiles the
/// multiplier is the one chosen for this source (it was internal before: the phone only knew "auto 120"); `frc_amf`
/// always doubles and its rate is not probed, so it has no fps.
pub fn processing_json(profile: &ProfileSpec, encoder: Encoder, lsfg_rate: Option<FrameRate>) -> serde_json::Value {
    let mut v = serde_json::json!({
        "upscale": if matches!(profile.upscale, profiles::Upscale::None) { "none" } else { "anime4k" },
        "fg": profile.fg.as_str(),
        "encoder": encoder.as_str(),
        "video_kbps": profile.video_kbps,
    });
    if let profiles::Upscale::Anime4k { mode, variant } = profile.upscale {
        v["anime4k"] = serde_json::json!({ "mode": mode.as_str(), "variant": variant.as_str() });
    }
    match profile.fg {
        profiles::Fg::None => {}
        profiles::Fg::FrcAmf => v["fg_effective"] = serde_json::json!({ "multiplier": 2 }),
        profiles::Fg::Lsfg(mode) => {
            // the session probed the rate at creation; FALLBACK_RATE is what the engine uses when it could not
            let source = lsfg_rate.unwrap_or(FALLBACK_RATE);
            let multiplier = mode.multiplier(Some(source.fps()));
            let round3 = |x: f64| (x * 1000.0).round() / 1000.0;
            v["fg_effective"] = serde_json::json!({
                "multiplier": multiplier,
                "source_fps": round3(source.fps()),
                "out_fps": round3(source.fps() * f64::from(multiplier)),
            });
        }
    }
    v
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinkError {
    BadName,
    /// PUT from a run that is no longer current (or a closed session).
    Stale,
}

#[cfg(test)]
mod sustain_tests {
    use super::*;

    #[test]
    fn back_pressure_never_holds_a_segment_the_phone_waits_for() {
        // playhead 331, lead 15: 346 passes, 347 waits ...
        assert!(!lead_exceeded(346, 331, 331, 15));
        assert!(lead_exceeded(347, 331, 331, 15));
        // ... unless the phone asked for 347 (or later): BUG-058, buffering forever after "Sin mejora"
        assert!(!lead_exceeded(347, 331, 347, 15));
        assert!(!lead_exceeded(349, 331, 350, 15));
        assert!(lead_exceeded(351, 331, 350, 15));
    }

    #[test]
    fn far_requests_wait_for_the_engine_only_when_that_beats_a_restart() {
        let s = Duration::from_secs;
        assert!(wait_for_frontier(4, 4, None, None), "within the seek margin it always waits");
        assert!(!wait_for_frontier(5, 4, None, None), "nothing measured yet: restart, as before");
        // BUG-095: A4K + LSFG on AV1, ~2 s per 2-s segment and ~15 s to restart: +10 s (5 segments) waits ~10 s
        assert!(wait_for_frontier(5, 4, Some(s(2)), Some(s(15))));
        assert!(!wait_for_frontier(8, 4, Some(s(2)), Some(s(15))), "16 s of waiting is worse than a 15-s restart");
        // a fast engine (0.2 s per segment) waits for up to MAX_WAIT_AHEAD segments, never further
        assert!(wait_for_frontier(MAX_WAIT_AHEAD, 4, Some(Duration::from_millis(200)), Some(s(15))));
        assert!(!wait_for_frontier(MAX_WAIT_AHEAD + 1, 4, Some(Duration::from_millis(200)), Some(s(15))));
    }

    #[test]
    fn sustain_verdicts() {
        let w = Duration::from_secs(SUSTAIN_WINDOW_SECS);
        assert_eq!(sustain_verdict(true, 1, Duration::ZERO, false), Some(true), "held back by back-pressure = ahead");
        assert_eq!(sustain_verdict(false, 5, Duration::from_secs(5), false), None, "too early");
        assert_eq!(sustain_verdict(false, 30, w, true), None, "paused runs are not judged");
        // 2 s segments: 11 segments after the first in 20 s = 22 s of video
        assert_eq!(sustain_verdict(false, 11, w, false), Some(true));
        // 6 segments after the first = 12 s of video in 20 s: does not keep up
        assert_eq!(sustain_verdict(false, 7, w, false), Some(false));
    }
}

#[cfg(test)]
mod supervise_tests {
    use super::*;

    fn view() -> RunView {
        RunView {
            active: true,
            parked_at: None,
            run_top: 40,
            playhead: 20,
            wanted: 30,
            finished: false,
            held: 0,
            quiet: Duration::ZERO,
            restart_cost: None,
            auto_restarts: 0,
        }
    }

    const STALL: Duration = Duration::from_secs(30);

    #[test]
    fn a_parked_run_resumes_when_the_playhead_comes_within_its_restart_cost() {
        let s = Duration::from_secs;
        // A4K + LSFG: ~15 s to restart = 8 segments, + RESUME_MARGIN 5 = 13 segments of reach
        let parked = RunView { parked_at: Some(40), restart_cost: Some(s(15)), ..view() };
        assert_eq!(supervise_decision(&RunView { playhead: 26, wanted: 26, ..parked }, 15, STALL), None, "14 segments ahead: not yet");
        assert_eq!(supervise_decision(&RunView { playhead: 27, wanted: 27, ..parked }, 15, STALL), Some(Supervise::Resume(40)));
        // a phone asking for it resumes it at once
        assert_eq!(supervise_decision(&RunView { playhead: 0, wanted: 40, ..parked }, 15, STALL), Some(Supervise::Resume(40)));
        // a parked run is never "stalled" however long it stays quiet (the whole pause)
        assert_eq!(supervise_decision(&RunView { playhead: 0, wanted: 0, quiet: s(3600), ..parked }, 15, STALL), None);
    }

    #[test]
    fn a_silent_run_is_restarted_where_it_stopped_then_given_up() {
        let s = Duration::from_secs;
        assert_eq!(supervise_decision(&RunView { quiet: s(29), ..view() }, 15, STALL), None);
        assert_eq!(supervise_decision(&RunView { quiet: s(30), ..view() }, 15, STALL), Some(Supervise::Restart(40)));
        // twice a long measured restart cost before calling it stalled (a cold LSFG start)
        assert_eq!(supervise_decision(&RunView { quiet: s(30), restart_cost: Some(s(20)), ..view() }, 15, STALL), None);
        assert_eq!(supervise_decision(&RunView { quiet: s(41), restart_cost: Some(s(20)), ..view() }, 15, STALL), Some(Supervise::Restart(40)));
        // held by back-pressure, finished, failed or closed: not stalled
        for v in [RunView { held: 1, ..view() }, RunView { finished: true, ..view() }, RunView { active: false, ..view() }] {
            assert_eq!(supervise_decision(&RunView { quiet: s(600), ..v }, 15, STALL), None);
        }
        assert_eq!(supervise_decision(&RunView { quiet: s(30), auto_restarts: MAX_AUTO_RESTARTS, ..view() }, 15, STALL), Some(Supervise::Fail));
    }

    #[test]
    fn the_produced_range_is_the_run_of_segments_around_the_playhead() {
        let segs: BTreeMap<u32, Bytes> = [3, 4, 5, 6, 9, 10].into_iter().map(|k| (k, Bytes::from_static(b"x"))).collect();
        assert_eq!(produced_range(&segs, 5), Some((3, 7)));
        assert_eq!(produced_range(&segs, 3), Some((3, 7)));
        // a seek ahead of what is made: the next run of segments
        assert_eq!(produced_range(&segs, 7), Some((9, 11)));
        assert_eq!(produced_range(&segs, 11), None);
    }
}

#[cfg(test)]
mod create_log_tests {
    use super::{source_log_fields, SourceRequest};

    // BUG-131: the "Link session requested / created / refused" lines name the renewed session of a profile change
    #[test]
    fn a_session_source_names_the_session_it_renews() {
        let (kind, renews) = source_log_fields(&SourceRequest::Session { session_id: "945e3855".into() });
        assert_eq!((kind, renews.as_str()), ("session", "945e3855"));
        let (kind, renews) = source_log_fields(&SourceRequest::Torrent { uri: "magnet:?".into(), media_id: "1".into(), episode: 4 });
        assert_eq!((kind, renews.as_str()), ("torrent", "-"));
    }
}

#[cfg(test)]
mod processing_tests {
    use super::processing_json;
    use crate::media::probe::FrameRate;
    use crate::media::profiles::{Encoder, find};
    use serde_json::json;

    // 0.4.0: the phone says what the PC does ("Anime4K A + LSFG x5, 120 fps, h264_vaapi 11 Mb/s")
    #[test]
    fn an_auto_lsfg_session_reports_the_multiplier_it_chose() {
        let p = find("p-anime4k-lsfg-1080-auto120").unwrap();
        let v = processing_json(p, Encoder::H264Vaapi, Some(FrameRate { num: 24_000, den: 1001 }));
        assert_eq!(v["upscale"], "anime4k");
        assert_eq!(v["anime4k"], json!({ "mode": "a", "variant": "m" }));
        assert_eq!(v["fg"], "lsfg");
        assert_eq!(v["encoder"], "h264_vaapi");
        assert_eq!(v["video_kbps"], 11_000);
        assert_eq!(v["fg_effective"], json!({ "multiplier": 5, "source_fps": 23.976, "out_fps": 119.88 }));
        // a 30 fps source gets x4 from the same kind of profile
        let thirty = processing_json(find("p-lsfg-1080-auto120").unwrap(), Encoder::LibX264, Some(FrameRate { num: 30, den: 1 }));
        assert_eq!(thirty["fg_effective"], json!({ "multiplier": 4, "source_fps": 30.0, "out_fps": 120.0 }));
        assert_eq!(thirty["upscale"], "none");
        assert!(thirty.get("anime4k").is_none());
    }

    #[test]
    fn fixed_frame_generation_and_no_frame_generation() {
        let x3 = processing_json(find("p-lsfg-1080-x3").unwrap(), Encoder::H264Nvenc, Some(FrameRate { num: 25, den: 1 }));
        assert_eq!(x3["fg_effective"], json!({ "multiplier": 3, "source_fps": 25.0, "out_fps": 75.0 }));
        // without a probed rate the session's own fallback (23.976) is what the engine uses
        let fallback = processing_json(find("p-lsfg-1080-x2").unwrap(), Encoder::H264Nvenc, None);
        assert_eq!(fallback["fg_effective"]["out_fps"], 47.952);
        let amf = processing_json(find("p-anime4k-1080-fg2x").unwrap(), Encoder::H264Amf, None);
        assert_eq!((amf["fg"].as_str(), amf["fg_effective"].clone()), (Some("frc_amf"), json!({ "multiplier": 2 })));
        let none = processing_json(find("p-none").unwrap(), Encoder::LibOpenH264, None);
        assert_eq!(none, json!({ "upscale": "none", "fg": "none", "encoder": "libopenh264", "video_kbps": 6000 }));
        let up = processing_json(find("p-anime4k-1440").unwrap(), Encoder::H264Amf, None);
        assert_eq!(up["anime4k"], json!({ "mode": "a", "variant": "l" }));
        assert!(up.get("fg_effective").is_none());
    }
}
