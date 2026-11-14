//! Transcoding sessions: on-demand fMP4 HLS with a RAM window.
//!
//! A *run* (generation of the engine) produces segments `seg_<k>.m4s` starting at `k`; FFmpeg PUTs them to the loopback
//! sink ([`crate::sink`]). Segments live in a bounded RAM window (512 MiB, oldest evicted first). Requests that fall
//! outside the window restart the engine at the requested segment (seek). The sink applies back-pressure (it delays
//! the answer to the PUT) when the engine is too far ahead of the playhead reported by the heartbeat, so FFmpeg blocks
//! instead of decoding the whole file. 120 s without heartbeat closes the session.
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
use tk_core::net::{Policy, sanitize_headers};

pub const HEARTBEAT_SECS: u64 = 4;
pub const IDLE_TIMEOUT_SECS: u64 = 120;
const TOMBSTONES: usize = 64;
/// Adding a magnet waits for its metadata (DHT / trackers): give up after this long (BUG-009).
pub const TORRENT_OPEN_TIMEOUT: Duration = Duration::from_secs(75);
/// Sources of the last sessions kept for [`SourceRequest::Session`].
pub const ORIGINS_KEPT: usize = 128;
/// Segments kept behind the playhead for seek-back.
/// BUG-095: segments kept behind the playhead (60 s), so a short rewind is served from RAM instead of restarting the
/// engine (with A4K + LSFG a restart costs ~15 s). Still bounded by the RAM budget.
const KEEP_BEHIND: u32 = 30;
/// BUG-095: a request at most this many segments past the running engine's frontier may wait for it instead of
/// restarting it there, when the engine makes them sooner than a restart would ([`wait_for_frontier`]).
pub const MAX_WAIT_AHEAD: u32 = 30;

#[derive(Debug, Clone)]
pub struct SessionTuning {
    pub segment_wait: Duration,
    pub idle_timeout: Duration,
    pub max_ram_bytes: usize,
    /// How far (in segments) the engine may run ahead of the playhead before the sink blocks it.
    pub max_lead_segments: u32,
    /// A request further than this many segments past the frontier restarts the engine there.
    pub seek_ahead_segments: u32,
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
}

impl FailReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SourceUnavailable => "source_unavailable",
            Self::SourceReadFailed => "source_read_failed",
            Self::EngineUnavailable => "engine_unavailable",
            Self::UrlSourceDenied => "url_source_denied",
            Self::MemoryLimit => "memory_limit",
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

#[derive(Debug, Clone, serde::Deserialize)]
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
    origins: Mutex<VecDeque<(String, String, SourceRequest)>>,
    next_run: AtomicU32,
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
        if !req.start_secs.is_finite() || req.start_secs < 0.0 || req.audio_track > 32 {
            return Err(ErrorCode::RequestInvalid.into());
        }
        // profile
        let wanted = req.profile.as_deref().unwrap_or(profiles::DEFAULT_PROFILE);
        let profile = profiles::find(wanted).filter(|p| self.env.profile_available(p)).ok_or(ApiError(ErrorCode::ProfileUnavailable))?;
        let encoder = self.env.encoder_for(profile).ok_or(ApiError(ErrorCode::ProfileUnavailable))?;

        // source -> loopback input (a `session` source reuses the source of an earlier session of this device)
        let source = match &req.source {
            SourceRequest::Session { session_id } => self.origin_of(device_id, session_id).ok_or(ApiError(ErrorCode::SessionNotFound))?,
            other => other.clone(),
        };
        let (input_url, fetch_token, duration, torrent_id) = self.open_source(&source).await?;

        // one probe for what the source does not say: the frame rate (LSFG) and, BUG-009 / BUG-023, the duration of
        // sources that do not announce it (a torrent file, an MP4 by URL). With a duration the phone gets a VOD playlist
        // (real length, seek anywhere); without one it gets a growing EVENT playlist, which Media3 declares stuck when
        // back-pressure holds it still.
        let needs_rate = profile.fg.lsfg().is_some();
        let info = if needs_rate || duration.is_none() { self.engine.source_info(&input_url).await } else { SourceInfo::default() };
        // BUG-024: a link ffprobe cannot read (e.g. a full URL that answers 404) is refused here, so the phone says "the
        // PC could not open this link" instead of playing a session that fails a minute later
        if info.unreadable && matches!(source, SourceRequest::Url { .. } | SourceRequest::Torrent { .. }) {
            tracing::warn!("Link session refused: the source cannot be read");
            self.fetcher.unregister(&fetch_token);
            if let Some(t) = &torrent_id {
                self.host.torrent_session_ended(t);
            }
            return Err(ErrorCode::SourceUnavailable.into());
        }
        let duration = duration.or(info.duration_secs);
        if duration.is_none() {
            tracing::info!("source duration unknown; the session uses a growing (EVENT) playlist");
        }
        let lsfg_rate = needs_rate.then(|| {
            info.rate.unwrap_or_else(|| {
                tracing::info!("source frame rate unknown; LSFG assumes 24000/1001");
                FALLBACK_RATE
            })
        });

        // one active session per device
        self.close_device_session(device_id);

        let id = uuid::Uuid::new_v4().to_string();
        let workdir = self.work_root.join(&id);
        std::fs::create_dir_all(&workdir).map_err(|_| ApiError(ErrorCode::Internal))?;
        let shader = args::prepare_shader(profile, self.shaders_dir.as_deref(), &workdir).map_err(|e| {
            tracing::warn!(error = %e, "Anime4K shader unavailable");
            ApiError(ErrorCode::ProfileUnavailable)
        })?;
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
            }),
            notify: Notify::new(),
            last_heartbeat: Mutex::new(Instant::now()),
        });
        lock(&self.sessions).insert(id.clone(), session.clone());
        {
            let mut o = lock(&self.origins);
            o.push_back((id.clone(), device_id.to_owned(), source));
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
    fn origin_of(&self, device: &str, id: &str) -> Option<SourceRequest> {
        lock(&self.origins).iter().rev().find(|(sid, dev, _)| sid == id && dev == device).map(|(_, _, src)| src.clone())
    }

    async fn open_source(&self, src: &SourceRequest) -> ApiResult<(String, String, Option<f64>, Option<String>)> {
        match src {
            SourceRequest::Session { .. } => Err(ErrorCode::RequestInvalid.into()),
            SourceRequest::Torrent { uri, media_id, episode } => {
                if uri.is_empty() || uri.len() > 4096 || uri.chars().any(char::is_control) || media_id.is_empty() || media_id.len() > 128 || *episode == 0 {
                    return Err(ErrorCode::RequestInvalid.into());
                }
                let req = crate::TorrentRequest { uri: uri.clone(), media_id: media_id.clone(), episode: *episode };
                let opened = tokio::time::timeout(TORRENT_OPEN_TIMEOUT, self.host.open_torrent(req))
                    .await
                    .map_err(|_| ApiError(ErrorCode::SourceUnavailable))?
                    .map_err(ApiError)?;
                let target = Url::parse(&opened.url).map_err(|_| ApiError(ErrorCode::Internal))?;
                let trusted = crate::torrent_stream_origin(&target).ok_or(ApiError(ErrorCode::Internal))?;
                let token = self.fetcher.register(SourceSpec { url: target.clone(), headers: Vec::new(), extension_permissions: None, trusted: vec![trusted] });
                if let Err(e) = self.fetcher.validate(&token, &target).await {
                    self.fetcher.unregister(&token);
                    self.host.torrent_session_ended(&opened.torrent_id);
                    return Err(match e {
                        FetchError::Denied => ErrorCode::UrlSourceDenied.into(),
                        FetchError::Unavailable => ErrorCode::SourceUnavailable.into(),
                    });
                }
                Ok((self.fetcher.entry_url(&token, &target, false), token, None, Some(opened.torrent_id)))
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
                Ok((self.fetcher.file_url(&token), token, duration, None))
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
                    Ok(duration) => Ok((self.fetcher.entry_url(&token, &target, hls), token, duration, None)),
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
            if matches!(st.phase, Phase::Failed(_)) {
                st.phase = Phase::Preparing;
            }
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
                                session.state().finished = true;
                                session.notify.notify_waiters();
                            }
                            EngineExit::Failed => {
                                let reason = if me.fetcher.was_denied(&session.fetch_token) { FailReason::UrlSourceDenied } else { FailReason::SourceReadFailed };
                                me.fail(&session, run_gen, reason);
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
            Kind::Ignore => Ok(()),
            Kind::Init => {
                let mut st = s.state();
                if st.run_gen != run_gen || st.phase == Phase::Closed {
                    return Err(SinkError::Stale);
                }
                st.window.init = Some(body);
                if st.phase == Phase::Preparing {
                    st.phase = Phase::Ready;
                }
                drop(st);
                s.notify.notify_waiters();
                Ok(())
            }
            Kind::Segment(k) => {
                // back-pressure
                loop {
                    let wait = {
                        let mut st = s.state();
                        if st.run_gen != run_gen || st.phase == Phase::Closed {
                            tracing::debug!(k, run_gen, current = st.run_gen, "sink: stale segment");
                            return Err(SinkError::Stale);
                        }
                        let too_far = lead_exceeded(k, st.playhead, st.wanted, self.tuning.max_lead_segments);
                        st.throttled = too_far;
                        st.run_throttled |= too_far;
                        too_far
                    };
                    if !wait {
                        break;
                    }
                    tokio::select! { () = s.notify.notified() => {}, () = tokio::time::sleep(Duration::from_millis(250)) => {} }
                }
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
                st.run_segments += 1;
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

    fn evict(&self, st: &mut State) {
        let keep_from = st.playhead.saturating_sub(KEEP_BEHIND);
        let stale: Vec<u32> = st.window.segs.range(..keep_from).map(|(k, _)| *k).collect();
        for k in stale {
            if let Some(b) = st.window.segs.remove(&k) {
                st.window.bytes -= b.len();
            }
        }
        while st.window.bytes > self.tuning.max_ram_bytes {
            // drop what is behind the playhead first (oldest first), then segments of earlier runs past what this run
            // made (furthest first), then the oldest that is not the one being consumed
            let first = st.window.segs.keys().next().copied();
            let last = st.window.segs.keys().next_back().copied();
            let Some(k) = first.filter(|k| *k < st.playhead).or(last.filter(|k| *k >= st.run_top.max(st.playhead + 1))).or(first) else { break };
            if k >= st.playhead && st.window.segs.len() == 1 {
                break;
            }
            if let Some(b) = st.window.segs.remove(&k) {
                st.window.bytes -= b.len();
            }
        }
    }

    // ---- reads (HLS surface) ------------------------------------------------------------------------------------------

    pub fn heartbeat(&self, s: &Arc<Session>, position_secs: f64, paused: bool) {
        *lock(&s.last_heartbeat) = Instant::now();
        let mut st = s.state();
        st.playhead = (position_secs.max(0.0) / f64::from(SEGMENT_SECS)).floor() as u32;
        st.paused = paused;
        drop(st);
        s.notify.notify_waiters();
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
                    Phase::Failed(_) => return Err(ErrorCode::SourceUnavailable.into()),
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
                    Phase::Failed(_) => return Err(ErrorCode::SourceUnavailable.into()),
                    _ => {}
                }
                if let Some(b) = st.window.segs.get(&k) {
                    return Ok(b.clone());
                }
                // BUG-095: the frontier of the running engine (the window also holds earlier runs' segments)
                let frontier = st.run_top;
                // not in the window and already passed by this run (or before its start): evicted or never made here
                let behind = k < st.run_top;
                let too_far = k >= frontier
                    && !wait_for_frontier(k - frontier, self.tuning.seek_ahead_segments, st.seg_interval, st.restart_cost);
                // finished runs never produce k again; evicted or never-produced segments need a new run
                let dead = st.finished && !st.window.segs.contains_key(&k);
                let restart = (behind || too_far || dead) && !restarted;
                if !restart && k > st.wanted {
                    // the phone waits for k: let the engine produce it even past the lead (BUG-058). Wake the held PUT
                    // and look again with a fresh `notified` (this one already saw the wake-up).
                    st.wanted = k;
                    drop(st);
                    s.notify.notify_waiters();
                    continue;
                }
                restart.then_some(k)
            };
            if let Some(at) = restart_at {
                tracing::info!(k, "segment outside the window: restarting the engine there");
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
        v
    }
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
