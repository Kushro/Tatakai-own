//! Transcoding sessions: on-demand fMP4 HLS with a RAM window.
//!
//! A *run* (generation of the engine) produces segments `seg_<k>.m4s` starting at `k`; FFmpeg PUTs them to the loopback
//! sink ([`crate::sink`]). Segments live in a bounded RAM window (512 MiB, oldest evicted first). Requests that fall
//! outside the window restart the engine at the requested segment (seek). The sink applies back-pressure (it delays
//! the answer to the PUT) when the engine is too far ahead of the playhead reported by the heartbeat, so FFmpeg blocks
//! instead of decoding the whole file. 120 s without heartbeat closes the session.

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
use crate::media::engine::{Engine, EngineExit, EngineJob};
use crate::media::profiles::{self, Encoder, ProfileSpec, SEGMENT_SECS};
use tk_core::net::{Policy, sanitize_headers};

pub const HEARTBEAT_SECS: u64 = 4;
pub const IDLE_TIMEOUT_SECS: u64 = 120;
const TOMBSTONES: usize = 64;
/// Segments kept behind the playhead for seek-back.
const KEEP_BEHIND: u32 = 6;

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
            segment_wait: Duration::from_secs(20),
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
    audio_track: u32,
    shader: Option<String>,
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
            next_run: AtomicU32::new(1),
        })
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

    // ---- create / close ---------------------------------------------------------------------------------------------

    pub async fn create(self: &Arc<Self>, device_id: &str, req: CreateRequest) -> ApiResult<SessionCreated> {
        if !req.start_secs.is_finite() || req.start_secs < 0.0 || req.audio_track > 32 {
            return Err(ErrorCode::RequestInvalid.into());
        }
        // profile
        let wanted = req.profile.as_deref().unwrap_or(profiles::DEFAULT_PROFILE);
        let profile = profiles::find(wanted).filter(|p| self.env.profile_available(p)).ok_or(ApiError(ErrorCode::ProfileUnavailable))?;
        let encoder = self.env.encoder_for(profile).ok_or(ApiError(ErrorCode::ProfileUnavailable))?;

        // source -> loopback input
        let (input_url, fetch_token, duration) = self.open_source(&req.source).await?;

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
            sink_token: uuid::Uuid::new_v4().simple().to_string(),
            duration,
            audio_track: req.audio_track,
            shader,
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
            }),
            notify: Notify::new(),
            last_heartbeat: Mutex::new(Instant::now()),
        });
        lock(&self.sessions).insert(id.clone(), session.clone());
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

    async fn open_source(&self, src: &SourceRequest) -> ApiResult<(String, String, Option<f64>)> {
        match src {
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
                Ok((self.fetcher.file_url(&token), token, duration))
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
                let token = self.fetcher.register(SourceSpec { url: target.clone(), headers, extension_permissions: perms });
                let result = async {
                    self.fetcher.validate(&token, &target).await?;
                    if hls { self.fetcher.probe_hls(&token, &target).await } else { Ok(None) }
                }
                .await;
                match result {
                    Ok(duration) => Ok((self.fetcher.entry_url(&token, &target, hls), token, duration)),
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
        // the working dir is `<work_root>/<session id>`: a uuid we generated, never user input
        if s.workdir.parent() == Some(self.work_root.as_path()) {
            let _ = std::fs::remove_dir_all(&s.workdir);
        }
        let mut t = lock(&self.tombstones);
        t.push_back(id.to_owned());
        while t.len() > TOMBSTONES {
            t.pop_front();
        }
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
            st.window = Window::default();
            st.finished = false;
            if matches!(st.phase, Phase::Failed(_)) {
                st.phase = Phase::Preparing;
            }
        }
        let sink_base = format!("http://{sink_addr}/sink/{}/{run_gen}", s.sink_token);
        let params = PipelineParams {
            profile: s.profile,
            encoder: s.encoder,
            input_url: &s.input_url,
            sink_base: &sink_base,
            start_segment,
            audio_track: s.audio_track,
            shader_file: s.shader.as_deref(),
            adapter: 0,
        };
        let args = match args::build(&params) {
            Ok(a) => a,
            Err(_) => return self.fail(s, run_gen, FailReason::EngineUnavailable),
        };
        let job = EngineJob { args, workdir: s.workdir.clone(), start_segment, sink_base };
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
                            return Err(SinkError::Stale);
                        }
                        let too_far = k > st.playhead.saturating_add(self.tuning.max_lead_segments);
                        st.throttled = too_far;
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
                st.window.bytes += body.len();
                if let Some(old) = st.window.segs.insert(k, body) {
                    st.window.bytes -= old.len();
                }
                self.evict(&mut st);
                if st.phase == Phase::Preparing && st.window.init.is_some() {
                    st.phase = Phase::Ready;
                }
                drop(st);
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
            // drop the oldest segment that is not the one being consumed
            let Some(k) = st.window.segs.keys().next().copied() else { break };
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
                let st = s.state();
                match st.phase {
                    Phase::Closed => return Err(ErrorCode::SessionGone.into()),
                    Phase::Failed(_) => return Err(ErrorCode::SourceUnavailable.into()),
                    _ => {}
                }
                if let Some(b) = st.window.segs.get(&k) {
                    return Ok(b.clone());
                }
                let frontier = st.window.segs.keys().next_back().map_or(st.gen_start, |m| m + 1);
                let behind = k < st.gen_start || st.window.segs.keys().next().is_some_and(|first| k < *first);
                let too_far = k > frontier.saturating_add(self.tuning.seek_ahead_segments);
                // finished runs never produce k again; evicted or never-produced segments need a new run
                let dead = st.finished && !st.window.segs.contains_key(&k);
                ((behind || too_far || dead) && !restarted).then_some(k)
            };
            if let Some(at) = restart_at {
                restarted = true;
                self.start_run(s, at).await;
                continue;
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
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
                let top = st.window.segs.keys().next_back().map_or(st.gen_start, |m| m + 1);
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
        let frontier = st.window.segs.keys().next_back().copied();
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
