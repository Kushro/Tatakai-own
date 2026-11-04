//! Player screen (review E5). mpv draws into a child window under the transparent web view; this page
//! leaves a hole (`#tk-video-hole`) whose rect is reported to `tk-player` and draws the controls over it.
//! The hole only becomes transparent once mpv reports `vo-configured`; until then it is an opaque scrim.
//!
//! Wave 1 (docs/redesign/08 section 7): the ranked `Backend::sources` list drives a pure
//! [`FlowState`](crate::player_state::FlowState) (Resolving / Buffering / Playing / Error / Manual) with
//! failover and `report_source_result`; Manual mode resolves nothing until a source is picked. When the
//! backend has no `sources` yet (`NotWired`) the old `Backend::playback` streams feed the same flow.
//!
//! Wave 2 (docs/redesign/09 section 4.4): Settings > Player picks the engine at boot. Embedded enhanced
//! prepares mpv-anime-build first (falls back to Embedded with a notice); with its uosc UI our controls and
//! player shortcuts hide and the hole relays pointer / keys to mpv. External mpv never creates the embedded
//! player: the same source flow spawns the machine's mpv (`tk_player::ExternalPlayer`) and this page shows an
//! informative card (Stop / Back) until mpv closes, then returns to where the user came from.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use dioxus::desktop::tao::dpi::PhysicalSize;
use dioxus::desktop::{DesktopContext, LogicalSize, use_window};
use dioxus::html::input_data::MouseButton;
use dioxus::prelude::*;
use dioxus::router::Navigator;
use tk_player::{
    EngineMode, ExternalEvent, ExternalPlayer, KeyAction, MediaSource, Player, PlayerError, PlayerEvent, Rect,
    ShaderPreset, SubtitleSource, TrackInfo, TrackKind,
};
use tokio::sync::broadcast::error::RecvError;

use crate::app::{Route, Services, Ui};
use crate::backend::{BackendRef, PlaybackVm, ReleaseTagsVm, SourceKindVm, SourceVm};
use crate::components::player_controls::NextEpisodeCard;
use crate::components::{
    Button, ButtonVariant, EmptyState, ErrorState, IconButton, LoadingIndicator, PlayerControls, SourcePicker,
    WavyProgress,
};
use crate::i18n::{t, tf};
use crate::icons::Icon;
use crate::model::{
    EnhancedUiVm, ErrorKind, PlatformVm, PlayerAction, PlayerModeVm, PlayerState, SEEK_STEP_SECS, SettingsAction,
    UiError, format_time, next_episode, reduce_player, reduce_settings, should_report,
};
use crate::platform::{self, now_ms};
use crate::player_state::{
    FLASH_MS, FlowState, MAX_LEGACY_STREAMS, Markers, Phase, countdown_left, format_speed, keeps_controls_up, mpv_key_name,
    mpv_mouse_button, source_name,
};
use crate::shortcuts::Shortcut;

/// Reports the hole's geometry: `[left, top, width, height, devicePixelRatio]` in CSS pixels.
const RECT_JS: &str = r#"
const el = document.getElementById('tk-video-hole');
if (el) {
  const send = () => { const r = el.getBoundingClientRect(); dioxus.send([r.left, r.top, r.width, r.height, window.devicePixelRatio || 1]); };
  new ResizeObserver(send).observe(el);
  window.addEventListener('resize', send);
  send();
}
"#;

/// Picture-in-picture window size and the normal minimum (keep in sync with tk-app `main.rs`).
const PIP_SIZE: (f64, f64) = (480.0, 270.0);
const PIP_MIN: (f64, f64) = (320.0, 180.0);
const WINDOW_MIN: (f64, f64) = (720.0, 480.0);

fn dispatch(sig: &mut Signal<PlayerState>, action: PlayerAction) {
    let next = reduce_player(&sig.peek(), action);
    sig.set(next);
}

fn media_source(vm: &PlaybackVm, idx: usize) -> MediaSource {
    let s = &vm.streams[idx];
    MediaSource {
        url: s.url.clone(),
        headers: s.headers.clone(),
        subtitles: s
            .subtitles
            .iter()
            .map(|x| SubtitleSource { url: x.url.clone(), lang: x.lang.clone(), label: x.label.clone() })
            .collect(),
    }
}

fn load_stream(p: &Player, vm: &PlaybackVm, idx: usize, start: f64) -> Result<(), PlayerError> {
    p.load(&media_source(vm, idx), start)
}

/// "Check the log in <real data dir>/logs" (docs/redesign/09 section 4.2); the old text when the backend has no path.
fn no_player_text(backend: &BackendRef) -> String {
    let dir = backend.data_dir_display();
    if dir.is_empty() {
        return t("player.error.no_player");
    }
    let sep = if PlatformVm::current() == PlatformVm::Windows { '\\' } else { '/' };
    let logs = format!("{}{sep}logs", dir.trim_end_matches(['/', '\\']));
    tf("player.error.no_player_at", &[("dir", &logs)])
}

/// The external mpv of this screen, kept outside signals so `use_drop` can still stop it. Replacing or
/// clearing it stops the previous one (one mpv window per screen).
#[derive(Default)]
struct ExternalSlot(RefCell<Option<Arc<ExternalPlayer>>>);

impl ExternalSlot {
    fn set(&self, p: Option<Arc<ExternalPlayer>>) {
        let old = self.0.replace(p);
        if let Some(old) = old {
            old.stop();
        }
    }

    /// Forget the player without stopping it (it already exited).
    fn forget(&self) {
        self.0.borrow_mut().take();
    }

    fn is(&self, p: &Arc<ExternalPlayer>) -> bool {
        self.0.borrow().as_ref().is_some_and(|c| Arc::ptr_eq(c, p))
    }
}

/// `MouseEvent.button` index of a Dioxus mouse button (for [`mpv_mouse_button`]).
fn button_index(b: MouseButton) -> u8 {
    match b {
        MouseButton::Primary => 0,
        MouseButton::Auxiliary => 1,
        MouseButton::Secondary => 2,
        MouseButton::Fourth => 3,
        MouseButton::Fifth => 4,
        _ => u8::MAX,
    }
}

fn report(backend: &BackendRef, id: &str, episode: u32, st: &PlayerState) {
    if st.progress_secs() == 0 {
        return;
    }
    let (backend, id, pos, dur) = (backend.clone(), id.to_owned(), st.progress_secs(), st.duration_secs());
    spawn(async move {
        if let Err(e) = backend.record_progress(&id, episode, pos, dur).await {
            tracing::warn!(detail = %e.detail, "recording progress failed");
        }
    });
}

/// Feeds the reliability average (`ok` = playback started). `NotWired` is expected until F lands.
fn report_source(backend: &BackendRef, source_id: String, ok: bool) {
    let backend = backend.clone();
    spawn(async move {
        if let Err(e) = backend.report_source_result(&source_id, ok).await
            && e.kind != ErrorKind::NotWired
        {
            tracing::warn!(detail = %e.detail, "reporting the source result failed");
        }
    });
}

fn remember_pick(backend: &BackendRef, id: &str, episode: u32, source_id: Option<String>) {
    let (backend, id) = (backend.clone(), id.to_owned());
    spawn(async move {
        if let Err(e) = backend.set_picked_source(&id, episode, source_id.as_deref()).await
            && e.kind != ErrorKind::NotWired
        {
            tracing::warn!(detail = %e.detail, "saving the picked source failed");
        }
    });
}

/// Returns whether the preset is now active.
fn apply_preset(p: &Player, preset: ShaderPreset, notice: &mut Signal<Option<String>>) -> bool {
    match p.set_shader_preset(preset) {
        Ok(()) => {
            notice.set(None);
            true
        }
        Err(PlayerError::ShadersMissing(_) | PlayerError::NoShadersDir) => {
            notice.set(Some(t("player.shaders_missing")));
            false
        }
        Err(e) => {
            tracing::warn!(error = %e, "changing the shader preset failed");
            false
        }
    }
}

/// Old `Backend::playback` streams as flow sources (index i = `vm.streams[i]`).
fn legacy_sources(vm: &PlaybackVm) -> Vec<SourceVm> {
    vm.streams
        .iter()
        .take(MAX_LEGACY_STREAMS)
        .enumerate()
        .map(|(i, s)| SourceVm {
            id: format!("legacy|web|{i}"),
            ext_id: String::new(),
            ext_name: s.provider.clone(),
            kind: SourceKindVm::Web,
            label: s.quality.clone().unwrap_or_else(|| (i + 1).to_string()),
            quality: s.quality.clone().unwrap_or_default(),
            container: None,
            subs: s.subtitles.iter().map(|x| x.lang.clone()).collect(),
            dubs: Vec::new(),
            reliability: 0.0,
            note: None,
            latency_ms: None,
            size_bytes: None,
            seeders: None,
            leechers: None,
            info_hash: None,
            ext_enabled: true,
            score: 0.0,
            recommended: i == 0,
            picked: false,
            tags: ReleaseTagsVm::default(),
        })
        .collect()
}

/// Window state for picture-in-picture, kept outside signals so `use_drop` can still read it.
#[derive(Default)]
struct PipWindow {
    on: Cell<bool>,
    prev: Cell<Option<PhysicalSize<u32>>>,
}

/// Toggles picture-in-picture: a small always-on-top window; leaving restores the previous size.
fn set_pip(win: &DesktopContext, on: bool, state: &PipWindow) {
    if state.on.replace(on) == on {
        return;
    }
    win.set_always_on_top(on);
    if on {
        state.prev.set(Some(win.inner_size()));
        win.set_min_inner_size(Some(LogicalSize::new(PIP_MIN.0, PIP_MIN.1)));
        win.set_inner_size(LogicalSize::new(PIP_SIZE.0, PIP_SIZE.1));
    } else {
        if let Some(size) = state.prev.take() {
            win.set_inner_size(size);
        }
        win.set_min_inner_size(Some(LogicalSize::new(WINDOW_MIN.0, WINDOW_MIN.1)));
    }
}

/// Handles shared by the resolve loop, the mpv event pump and the UI callbacks (all `Copy` except the
/// backend and the id).
#[derive(Clone)]
struct Flow {
    backend: BackendRef,
    id: String,
    episode: u32,
    flow: Signal<FlowState>,
    pstate: Signal<PlayerState>,
    /// The playback of the current source (title, poster, start position).
    meta: Signal<Option<PlaybackVm>>,
    /// Legacy path only: the streams from `Backend::playback`.
    legacy: Signal<Option<PlaybackVm>>,
    player: Signal<Option<Arc<Player>>>,
    flash: Signal<Option<(String, u64)>>,
    /// External mode: the mpv executable (`None` in the embedded modes).
    ext_mpv: Signal<Option<PathBuf>>,
    external: Rc<ExternalSlot>,
    /// External mode: the file played to the end / mpv exited by itself.
    ext_ended: Signal<bool>,
    ext_closed: Signal<bool>,
    next_since: Signal<Option<u64>>,
    nav: Navigator,
}

impl Flow {
    fn flash(&self, text: String) {
        let mut f = self.flash;
        f.set(Some((text, now_ms())));
    }

    /// Resolves and loads whatever `FlowState::to_resolve` asks for, following failover until a stream is
    /// handed to mpv, the list runs out, or a newer attempt (a pick) takes over. `resume` keeps the
    /// position when switching sources mid-episode.
    async fn drive(self, resume: Option<f64>) {
        let mut flow = self.flow;
        let mut pstate = self.pstate;
        let mut meta = self.meta;
        loop {
            let (idx, attempt, legacy) = {
                let f = flow.peek();
                match f.to_resolve() {
                    Some(i) => (i, f.attempt, f.legacy),
                    None => return,
                }
            };
            let source_id = flow.peek().sources[idx].id.clone();
            dispatch(&mut pstate, PlayerAction::Loading);
            let resolved = if legacy {
                self.legacy
                    .peek()
                    .clone()
                    .map(|vm| PlaybackVm { streams: vm.streams.get(idx).cloned().into_iter().collect(), ..vm })
                    .ok_or_else(|| UiError::new(ErrorKind::NoSource, ""))
            } else {
                self.backend.playback_source(&self.id, self.episode, &source_id).await
            };
            if flow.peek().attempt != attempt {
                return;
            }
            let loaded = match resolved {
                Ok(vm) if vm.streams.is_empty() => Err(UiError::new(ErrorKind::NoSource, "")),
                Ok(vm) => {
                    let start = resume.filter(|p| *p > 0.0).unwrap_or(f64::from(vm.start_secs));
                    meta.set(Some(vm.clone()));
                    let mpv = self.ext_mpv.peek().clone();
                    if let Some(mpv) = mpv {
                        self.start_external(mpv, &vm, start, attempt).await
                    } else {
                        match self.player.peek().clone() {
                            Some(p) => load_stream(&p, &vm, 0, start).map_err(|e| UiError::new(ErrorKind::Other, e.to_string())),
                            None => Err(UiError::new(ErrorKind::Other, no_player_text(&self.backend))),
                        }
                    }
                }
                Err(e) => Err(e),
            };
            match loaded {
                Ok(()) => {
                    flow.write().resolved(attempt);
                    return;
                }
                Err(e) => {
                    tracing::warn!(kind = ?e.kind, detail = %e.detail, "source failed");
                    self.fail(attempt, e);
                }
            }
        }
    }

    /// Records a failure of `attempt`; the caller (or the `drive` loop) resolves the next one.
    fn fail(&self, attempt: u64, e: UiError) {
        let mut flow = self.flow;
        let failed = flow.write().fail(attempt, e);
        if let Some(sid) = failed {
            report_source(&self.backend, sid, false);
        }
        if flow.peek().to_resolve().is_some() {
            self.flash(t("player.error.trying_next"));
        }
    }

    /// A user pick (picker, quality menu, "Use the best web source"): remember it and play it.
    fn pick(&self, source_id: String) {
        let mut flow = self.flow;
        let resume = self.pstate.peek().position;
        if !flow.write().pick(&source_id) {
            return;
        }
        if !flow.peek().legacy {
            remember_pick(&self.backend, &self.id, self.episode, Some(source_id));
        }
        spawn(self.clone().drive(Some(resume)));
    }

    /// Back to the system choice: forget the pick and start over from the top of the list.
    fn auto(&self) {
        let mut flow = self.flow;
        if !flow.peek().sources.iter().any(|s| s.picked) {
            return;
        }
        let resume = self.pstate.peek().position;
        {
            let mut f = flow.write();
            for s in &mut f.sources {
                s.picked = false;
            }
            f.retry();
        }
        remember_pick(&self.backend, &self.id, self.episode, None);
        spawn(self.clone().drive(Some(resume)));
    }

    /// "Retry" on the final error.
    fn retry(&self) {
        let mut flow = self.flow;
        flow.write().retry();
        spawn(self.clone().drive(None));
    }

    /// External mode: open the resolved stream in the machine's mpv (closing the previous window) and pump its
    /// events. `Ok` hands the source to mpv; whether it plays is decided by the pump (`Position > 0` / `Duration`).
    async fn start_external(&self, mpv: PathBuf, vm: &PlaybackVm, start: f64, attempt: u64) -> Result<(), UiError> {
        let src = media_source(vm, 0);
        let n = self.episode.to_string();
        let title = if vm.title.is_empty() {
            tf("player.episode", &[("n", &n)])
        } else {
            tf("player.external.window_title", &[("title", &vm.title), ("n", &n)])
        };
        self.external.set(None);
        let spawned = tokio::task::spawn_blocking(move || ExternalPlayer::spawn(&mpv, &src, start, &title)).await;
        let p = match spawned {
            Ok(Ok(p)) => Arc::new(p),
            Ok(Err(e)) => return Err(UiError::new(ErrorKind::Other, e.to_string())),
            Err(e) => return Err(UiError::new(ErrorKind::Other, e.to_string())),
        };
        if self.flow.peek().attempt != attempt {
            // A pick took over while mpv was starting: `p` drops here and closes its window.
            return Ok(());
        }
        let rx = p.events();
        self.external.set(Some(p.clone()));
        spawn(self.clone().pump_external(p, rx, attempt));
        Ok(())
    }

    /// Events of one external mpv until it closes or is replaced. Progress is recorded like the embedded path.
    async fn pump_external(
        self,
        p: Arc<ExternalPlayer>,
        mut rx: tokio::sync::broadcast::Receiver<ExternalEvent>,
        attempt: u64,
    ) {
        let mut flow = self.flow;
        let mut pstate = self.pstate;
        let mut ext_ended = self.ext_ended;
        let mut ext_closed = self.ext_closed;
        let mut next_since = self.next_since;
        let mut last_report = now_ms();
        let mut last_pos = -1.0_f64;
        loop {
            let ev = match rx.recv().await {
                Ok(ev) => ev,
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => ExternalEvent::Closed,
            };
            if !self.external.is(&p) {
                // Replaced by another source, stopped by Stop / Back, or the screen is gone.
                return;
            }
            let phase = flow.peek().phase;
            match ev {
                ExternalEvent::Position(pos) => {
                    if (pos - last_pos).abs() >= 0.25 {
                        last_pos = pos;
                        dispatch(&mut pstate, PlayerAction::Position(pos));
                    }
                    if phase == Phase::Buffering && pos > 0.0
                        && let Some(sid) = flow.write().started()
                    {
                        report_source(&self.backend, sid, true);
                    }
                    let now = now_ms();
                    if should_report(last_report, now) {
                        last_report = now;
                        report(&self.backend, &self.id, self.episode, &pstate.peek());
                    }
                }
                ExternalEvent::Duration(d) => {
                    dispatch(&mut pstate, PlayerAction::Duration(d));
                    if phase == Phase::Buffering
                        && let Some(sid) = flow.write().started()
                    {
                        report_source(&self.backend, sid, true);
                    }
                }
                ExternalEvent::Paused(b) => {
                    dispatch(&mut pstate, PlayerAction::Paused(b));
                    if b {
                        report(&self.backend, &self.id, self.episode, &pstate.peek());
                    }
                }
                ExternalEvent::Ended => {
                    if phase != Phase::Playing || *ext_ended.peek() {
                        continue;
                    }
                    ext_ended.set(true);
                    dispatch(&mut pstate, PlayerAction::Eof);
                    let mut done = pstate.peek().clone();
                    done.position = done.duration.max(done.position);
                    report(&self.backend, &self.id, self.episode, &done);
                    let total = self.meta.peek().as_ref().and_then(|m| m.episodes_total);
                    if next_episode(self.episode, total).is_some() {
                        next_since.set(Some(now_ms()));
                    }
                }
                ExternalEvent::Error(msg) => {
                    tracing::warn!(%msg, "external mpv error");
                    if phase != Phase::Buffering {
                        continue;
                    }
                    // Nothing played yet: this source does not work, close the window and fail over.
                    self.external.set(None);
                    self.fail(attempt, UiError::new(ErrorKind::Other, msg));
                    if flow.peek().to_resolve().is_some() {
                        // Type-erased: `drive` -> `start_external` -> this pump -> `drive` would be a cyclic
                        // opaque future type otherwise.
                        let next: std::pin::Pin<Box<dyn std::future::Future<Output = ()>>> = Box::pin(self.clone().drive(None));
                        spawn(next);
                    }
                    return;
                }
                ExternalEvent::Closed => {
                    self.external.forget();
                    ext_closed.set(true);
                    if !*ext_ended.peek() {
                        report(&self.backend, &self.id, self.episode, &pstate.peek());
                    }
                    // Ended with a next episode: the countdown card stays; otherwise back to where we came from.
                    if next_since.peek().is_none() {
                        self.nav.go_back();
                    }
                    return;
                }
            }
        }
    }
}

/// Route component. Keys the screen by episode so "next episode" remounts it (fresh hooks and flow).
#[component]
pub fn PlayerPage(id: String, episode: u32) -> Element {
    rsx! {
        PlayerScreen { key: "{id}-{episode}", id: id.clone(), episode }
    }
}

#[component]
fn PlayerScreen(id: String, episode: u32) -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let navigator = use_navigator();
    let win = use_window();
    let backend = services.backend.clone();
    let host = services.player.clone();

    let mut pstate = use_signal(PlayerState::default);
    let flow = use_signal(FlowState::default);
    let meta = use_signal(|| None::<PlaybackVm>);
    let mut legacy = use_signal(|| None::<PlaybackVm>);
    let mut failure = use_signal(|| None::<(UiError, String)>);
    let mut notice = use_signal(|| None::<String>);
    let flash = use_signal(|| None::<(String, u64)>);
    let mut player = use_signal(|| None::<Arc<Player>>);
    let mut preset = use_signal(|| ui.settings.peek().preset());
    let mut fullscreen = use_signal(|| false);
    let mut clock = use_signal(now_ms);
    let mut picker_open = use_signal(|| false);
    let mut tracks = use_signal(Vec::<TrackInfo>::new);
    let mut sub_track = use_signal(|| 0_i64);
    let mut audio_track = use_signal(|| 0_i64);
    let mut speed = use_signal(|| 1.0_f64);
    let mut smoothing = use_signal(|| false);
    let mut pip = use_signal(|| false);
    let pip_win = use_hook(|| Rc::new(PipWindow::default()));
    let mut next_since = use_signal(|| None::<u64>);
    // Intro/outro markers hook: ani.zip / AniSkip arrive in M1; until then nothing is marked.
    let markers = use_signal(Markers::default);
    // Title, poster and episode count before any source resolves (Manual mode waits for a pick).
    let details = use_resource({
        let (backend, id) = (backend.clone(), id.clone());
        move || {
            let (backend, id) = (backend.clone(), id.clone());
            async move { backend.details(&id).await.ok() }
        }
    });

    // Engine (docs/redesign/09 section 4.4), decided at boot from Settings > Player.
    let mut ext_mpv = use_signal(|| None::<PathBuf>);
    let external = use_hook(|| Rc::new(ExternalSlot::default()));
    let ext_ended = use_signal(|| false);
    let ext_closed = use_signal(|| false);
    // External mode without an mpv executable: `Some(configured path or "")`.
    let mut mpv_missing = use_signal(|| None::<String>);
    // Embedded enhanced: mpv-anime-build is being verified / extracted.
    let mut preparing = use_signal(|| false);
    let mut enhanced = use_signal(|| false);
    // Enhanced with uosc: mpv draws the UI, the hole relays input.
    let mut uosc = use_signal(|| false);
    let mut dpr = use_signal(|| 1.0_f64);

    let ctx = Flow {
        backend: backend.clone(),
        id: id.clone(),
        episode,
        flow,
        pstate,
        meta,
        legacy,
        player,
        flash,
        ext_mpv,
        external: external.clone(),
        ext_ended,
        ext_closed,
        next_since,
        nav: navigator,
    };

    // Rank the sources, start mpv, resolve the first one and pump mpv's events.
    let mut boot = use_future({
        let (ctx, host) = (ctx.clone(), host.clone());
        move || {
            let (ctx, host) = (ctx.clone(), host.clone());
            async move {
                let (backend, id) = (ctx.backend.clone(), ctx.id.clone());
                let mut flow = ctx.flow;
                dispatch(&mut pstate, PlayerAction::Loading);
                dispatch(&mut pstate, PlayerAction::Activity(now_ms()));

                // Engine selection.
                let settings = ui.settings.peek().clone();
                mpv_missing.set(None);
                ext_mpv.set(None);
                enhanced.set(false);
                uosc.set(false);
                match settings.player_mode {
                    PlayerModeVm::External => {
                        let custom = settings.external_mpv_path.clone();
                        let path = custom.clone().map(PathBuf::from);
                        let found = tokio::task::spawn_blocking(move || tk_player::find_mpv(path.as_deref())).await.ok().flatten();
                        match found {
                            Some(mpv) => {
                                tracing::info!(mpv = %mpv.display(), "external player mode");
                                ext_mpv.set(Some(mpv));
                                // No embedded libmpv in this mode.
                                host.release();
                            }
                            None => {
                                mpv_missing.set(Some(custom.unwrap_or_default()));
                                return;
                            }
                        }
                    }
                    PlayerModeVm::EmbeddedEnhanced => {
                        preparing.set(true);
                        let prepared = backend.prepare_anime_build().await;
                        preparing.set(false);
                        match prepared {
                            Ok(config_dir) => {
                                let with_uosc = settings.enhanced_ui == EnhancedUiVm::Uosc;
                                host.set_engine(EngineMode::Enhanced { config_dir, uosc: with_uosc });
                                enhanced.set(true);
                                uosc.set(with_uosc);
                            }
                            Err(e) => {
                                tracing::warn!(kind = ?e.kind, detail = %e.detail, "mpv-anime-build unavailable; using the embedded engine");
                                host.set_engine(EngineMode::Embedded);
                                notice.set(Some(t("player.enhanced.fallback")));
                            }
                        }
                    }
                    PlayerModeVm::Embedded => host.set_engine(EngineMode::Embedded),
                }

                let mode = settings.sources.mode;
                let state = match backend.sources(&id, episode).await {
                    Ok(list) => FlowState::new(list, mode),
                    Err(e) if e.kind == ErrorKind::NotWired => match backend.playback(&id, episode).await {
                        Ok(vm) => {
                            let list = legacy_sources(&vm);
                            legacy.set(Some(vm));
                            FlowState::legacy(list)
                        }
                        Err(e) => {
                            failure.set(Some((e, String::new())));
                            return;
                        }
                    },
                    Err(e) => {
                        failure.set(Some((e, String::new())));
                        return;
                    }
                };
                let waiting = state.phase == Phase::Manual;
                flow.set(state);
                if waiting {
                    picker_open.set(true);
                }

                if ext_mpv.peek().is_some() {
                    // External mode: `drive` spawns mpv per resolved source and pumps its events.
                    spawn(ctx.clone().drive(None));
                    return;
                }

                let p = match host.get_or_init(&platform::embed_target()) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::error!(error = %e, "player start failed");
                        failure.set(Some((UiError::new(ErrorKind::Other, e.to_string()), no_player_text(&backend))));
                        return;
                    }
                };
                player.set(Some(p.clone()));
                if !p.is_embedded() {
                    // mpv fell back to its own window (the reason is in the log).
                    notice.set(Some(t("player.embed.own_window")));
                }
                let mut rx = p.events();

                // Hole geometry -> child window.
                spawn({
                    let p = p.clone();
                    async move {
                        let mut eval = document::eval(RECT_JS);
                        while let Ok(v) = eval.recv::<[f64; 5]>().await {
                            p.set_rect(Some(Rect { x: v[0], y: v[1], w: v[2], h: v[3] }), v[4]);
                            dpr.set(v[4]);
                        }
                    }
                });
                if *uosc.peek() {
                    // Keys reach the hole (and from there mpv) without a click first.
                    let _ = document::eval("document.getElementById('tk-video-hole')?.focus()");
                }

                let _ = p.set_volume(f64::from(settings.volume));
                dispatch(&mut pstate, PlayerAction::SetVolume(settings.volume));
                let initial = settings.preset();
                // Our presets are an Embedded-only feature: mpv-anime-build brings its own shaders.
                if initial != ShaderPreset::Off && !*enhanced.peek() {
                    apply_preset(&p, initial, &mut notice);
                }
                // Manual mode without a pick: `to_resolve` is None, so this returns without network.
                spawn(ctx.clone().drive(None));

                let mut last_report = now_ms();
                let mut last_pos = -1.0_f64;
                loop {
                    let ev = match rx.recv().await {
                        Ok(ev) => ev,
                        Err(RecvError::Lagged(_)) => continue,
                        Err(RecvError::Closed) => break,
                    };
                    let phase = flow.peek().phase;
                    let live = matches!(phase, Phase::Buffering | Phase::Playing);
                    match ev {
                        PlayerEvent::Position(pos) => {
                            // mpv reports per frame; the UI needs a few updates per second.
                            if (pos - last_pos).abs() >= 0.25 {
                                last_pos = pos;
                                dispatch(&mut pstate, PlayerAction::Position(pos));
                            }
                            if phase == Phase::Buffering && pos > 0.0 && !pstate.peek().buffering
                                && let Some(sid) = flow.write().started()
                            {
                                report_source(&backend, sid, true);
                            }
                            let now = now_ms();
                            if should_report(last_report, now) {
                                last_report = now;
                                report(&backend, &id, episode, &pstate.peek());
                            }
                        }
                        PlayerEvent::Duration(d) => dispatch(&mut pstate, PlayerAction::Duration(d)),
                        PlayerEvent::Paused(b) => {
                            dispatch(&mut pstate, PlayerAction::Paused(b));
                            if b {
                                report(&backend, &id, episode, &pstate.peek());
                            }
                        }
                        PlayerEvent::Buffering(b) => {
                            dispatch(&mut pstate, PlayerAction::Buffering(b));
                            if live && ((b && phase == Phase::Playing) || (!b && phase == Phase::Buffering)) {
                                flow.write().stalled(b);
                            }
                        }
                        PlayerEvent::VideoReady(b) => {
                            dispatch(&mut pstate, PlayerAction::VideoReady(b));
                            if b && phase == Phase::Buffering
                                && let Some(sid) = flow.write().started()
                            {
                                report_source(&backend, sid, true);
                            }
                        }
                        PlayerEvent::Tracks(list) => {
                            let sel = |kind: TrackKind| list.iter().find(|tr| tr.kind == kind && tr.selected).map_or(0, |tr| tr.id);
                            sub_track.set(sel(TrackKind::Sub));
                            audio_track.set(sel(TrackKind::Audio));
                            tracks.set(list);
                        }
                        PlayerEvent::Eof => {
                            if phase != Phase::Playing {
                                continue;
                            }
                            dispatch(&mut pstate, PlayerAction::Eof);
                            let mut done = pstate.peek().clone();
                            done.position = done.duration.max(done.position);
                            report(&backend, &id, episode, &done);
                            let total = ctx.meta.peek().as_ref().and_then(|m| m.episodes_total);
                            if next_episode(episode, total).is_some() {
                                next_since.set(Some(now_ms()));
                            }
                        }
                        PlayerEvent::Error(msg) => {
                            if !live {
                                continue;
                            }
                            tracing::warn!(%msg, "playback error");
                            let attempt = flow.peek().attempt;
                            let resume = pstate.peek().position;
                            ctx.fail(attempt, UiError::new(ErrorKind::Other, msg));
                            if flow.peek().to_resolve().is_some() {
                                spawn(ctx.clone().drive(Some(resume)));
                            }
                        }
                    }
                }
            }
        }
    });

    // Controls auto-hide, flash expiry and the next-episode countdown.
    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;
            clock.set(now_ms());
        }
    });

    // Next-episode countdown reached zero.
    use_effect({
        let id = id.clone();
        move || {
            let now = clock();
            let Some(since) = next_since() else { return };
            if countdown_left(since, now) == 0 {
                next_since.set(None);
                navigator.push(Route::PlayerPage { id: id.clone(), episode: episode + 1 });
            }
        }
    });

    // Release the picture (and the PiP window state) when leaving the screen.
    use_drop({
        let win = win.clone();
        let host = host.clone();
        let pip_win = pip_win.clone();
        let external = external.clone();
        move || {
            // An external session never outlives its screen.
            external.set(None);
            if let Some(p) = host.current() {
                p.set_rect(None, 1.0);
                p.stop();
            }
            set_pip(&win, false, &pip_win);
            win.set_fullscreen(false);
        }
    });

    // Keyboard.
    let mut seen = use_signal(|| ui.shortcut.peek().as_ref().map_or(0, |e| e.seq));
    use_effect({
        let (backend, id, win) = (backend.clone(), id.clone(), win.clone());
        move || {
            let Some(evt) = (ui.shortcut)() else { return };
            if *seen.peek() == evt.seq {
                return;
            }
            seen.set(evt.seq);
            // External mpv has its own window and keys; with uosc the hole relays keys to mpv instead.
            if ext_mpv.peek().is_some() {
                return;
            }
            if *uosc.peek() && !matches!(evt.action, Shortcut::Fullscreen | Shortcut::Escape) {
                return;
            }
            if *enhanced.peek() && matches!(evt.action, Shortcut::Preset(_)) {
                return;
            }
            let Some(p) = player.peek().clone() else { return };
            match evt.action {
                Shortcut::TogglePause => {
                    let _ = if pstate.peek().paused { p.play() } else { p.pause() };
                }
                Shortcut::SeekBack => {
                    let _ = p.seek_relative(-SEEK_STEP_SECS);
                }
                Shortcut::SeekForward => {
                    let _ = p.seek_relative(SEEK_STEP_SECS);
                }
                Shortcut::Fullscreen => {
                    let on = !*fullscreen.peek();
                    fullscreen.set(on);
                    win.set_fullscreen(on);
                }
                Shortcut::Escape => {
                    if *fullscreen.peek() {
                        fullscreen.set(false);
                        win.set_fullscreen(false);
                    }
                }
                Shortcut::Preset(n) => {
                    if let Some(next) = ShaderPreset::from_shortcut(n)
                        && apply_preset(&p, next, &mut notice)
                    {
                        preset.set(next);
                        let s = reduce_settings(&ui.settings.peek(), SettingsAction::SetPreset(next));
                        ui.commit_settings(&backend, s);
                    }
                }
                Shortcut::ToggleNav | Shortcut::FocusSearch | Shortcut::NavTo(_) => {}
            }
            let _ = &id;
        }
    });

    let st = pstate();
    let fs = flow();
    let now = clock();
    let info = details.read().clone().flatten();
    let vm = meta();
    let title = vm.as_ref().map(|m| m.title.clone()).filter(|s| !s.is_empty()).or_else(|| info.as_ref().map(|d| d.media.title.clone())).unwrap_or_default();
    let poster = vm.as_ref().and_then(|m| m.poster.clone()).or_else(|| info.as_ref().and_then(|d| d.media.cover.clone())).unwrap_or_default();
    let episodes_total = vm.as_ref().and_then(|m| m.episodes_total).or_else(|| info.as_ref().and_then(|d| d.episodes_total));
    let has_next = next_episode(episode, episodes_total).is_some();
    let visible = st.controls_visible(now) || keeps_controls_up(fs.phase, st.video_ready) || next_since().is_some();
    let current = fs.current_source().cloned();
    let source_label = match (&current, fs.position()) {
        (Some(s), Some((n, total))) if !fs.legacy || total > 1 => {
            let pos = tf("player.source", &[("n", &n.to_string()), ("total", &total.to_string())]);
            if fs.legacy {
                pos
            } else if s.quality.is_empty() {
                format!("{pos} · {}", source_name(s))
            } else {
                format!("{pos} · {} · {}", source_name(s), s.quality)
            }
        }
        _ => String::new(),
    };
    let source_icon = if current.as_ref().is_some_and(|s| s.kind == SourceKindVm::Torrent) { "download" } else { "language" };
    let resolving_label = match &current {
        Some(s) if !s.ext_name.is_empty() => format!("{} · {}", t("player.resolving"), s.ext_name),
        _ => t("player.resolving"),
    };
    let qualities = fs.qualities();
    let quality = current.as_ref().map(|s| s.quality.clone()).unwrap_or_default();
    let quality_auto = !fs.sources.iter().any(|s| s.picked);
    let hole_open = st.video_ready && failure().is_none() && matches!(fs.phase, Phase::Buffering | Phase::Playing);
    let flash_text = flash().filter(|(_, at)| now.saturating_sub(*at) < FLASH_MS).map(|(text, _)| text);
    let next_left = next_since().map(|since| countdown_left(since, now));
    // The picker marks the source being played as checked.
    let picker_sources: Vec<SourceVm> = fs
        .sources
        .iter()
        .enumerate()
        .map(|(i, s)| SourceVm { picked: Some(i) == fs.current, ..s.clone() })
        .collect();
    let prefs = ui.settings.read().sources.clone();

    let go_back = {
        let (backend, id) = (backend.clone(), id.clone());
        move |_| {
            report(&backend, &id, episode, &pstate.peek());
            navigator.go_back();
        }
    };

    let ext_mode = ext_mpv().is_some();
    let relay = uosc() && !ext_mode;
    let hole_tab = if relay { "0" } else { "-1" };
    // External Stop / Back: both close mpv and return (a session never outlives this screen).
    let ext_leave = {
        let (backend, id, external) = (backend.clone(), id.clone(), external.clone());
        move || {
            report(&backend, &id, episode, &pstate.peek());
            external.set(None);
            navigator.go_back();
        }
    };

    let body = if let Some(path) = mpv_missing() {
        let detail = if path.is_empty() { t("player.external.searched") } else { path };
        rsx! {
            div { class: "tk-player__fail",
                ErrorState {
                    error: UiError::new(ErrorKind::Other, detail),
                    title: t("player.external.not_found"),
                    cause: t("player.external.not_found_cause"),
                    on_retry: move |_| boot.restart(),
                    Button {
                        label: t("player.external.open_settings"),
                        variant: ButtonVariant::Filled,
                        icon: "settings".to_string(),
                        onclick: move |_| {
                            navigator.push(Route::SettingsPage {});
                        },
                    }
                }
                Button {
                    label: t("common.back"),
                    variant: ButtonVariant::Text,
                    icon: "back".to_string(),
                    onclick: move |_| {
                        navigator.go_back();
                    },
                }
            }
        }
    } else if preparing() {
        rsx! {
            div { class: "tk-player__fail", role: "status",
                LoadingIndicator {}
                p { class: "tk-player__prep", {t("player.enhanced.preparing")} }
            }
        }
    } else if let Some((err, cause)) = failure() {
        rsx! {
            div { class: "tk-player__fail",
                ErrorState {
                    error: err.clone(),
                    title: t("player.error.title"),
                    cause,
                    on_retry: move |_| {
                        failure.set(None);
                        boot.restart();
                    },
                }
                Button {
                    label: t("common.back"),
                    variant: ButtonVariant::Text,
                    icon: "back".to_string(),
                    onclick: move |_| {
                        navigator.go_back();
                    },
                }
            }
        }
    } else if fs.phase == Phase::Error {
        let err = fs.error.clone().unwrap_or_else(|| UiError::new(ErrorKind::NoSource, ""));
        let detail = if fs.failed.is_empty() {
            err.detail.clone()
        } else {
            fs.failed
                .iter()
                .map(|f| format!("{}: {}", f.name, if f.detail.is_empty() { t(f.kind.key()) } else { f.detail.clone() }))
                .collect::<Vec<_>>()
                .join("\n")
        };
        let m2 = err.kind == ErrorKind::TorrentM2;
        let title = if m2 { t("src.m2") } else { t("player.error.title") };
        let cause = if m2 || fs.failed.is_empty() { String::new() } else { t("player.error.no_more") };
        let best_web = if m2 { fs.best_web().map(|s| s.id.clone()) } else { None };
        let can_pick = !fs.legacy && !fs.sources.is_empty();
        rsx! {
            div { class: "tk-player__fail",
                if err.kind == ErrorKind::NoSource && fs.failed.is_empty() {
                    EmptyState {
                        icon: "extensions".to_string(),
                        title: t("details.no_extension.title"),
                        body: t("details.no_extension.body"),
                        action_label: t("details.go_extensions"),
                        on_action: move |_| {
                            navigator.push(Route::ExtensionsPage {});
                        },
                    }
                } else {
                    ErrorState {
                        error: UiError::new(err.kind, detail),
                        title,
                        cause,
                        on_retry: {
                            let ctx = ctx.clone();
                            move |_| ctx.retry()
                        },
                        if let Some(web) = best_web {
                            Button {
                                label: t("src.use_web"),
                                variant: ButtonVariant::Filled,
                                icon: "language".to_string(),
                                onclick: {
                                    let ctx = ctx.clone();
                                    move |_| ctx.pick(web.clone())
                                },
                            }
                        }
                        if can_pick {
                            Button {
                                label: tf("src.all", &[("n", &fs.sources.len().to_string())]),
                                variant: ButtonVariant::Tonal,
                                icon: "tune".to_string(),
                                onclick: move |_| picker_open.set(true),
                            }
                        }
                        Button {
                            label: t("details.go_extensions"),
                            variant: ButtonVariant::Text,
                            icon: "extensions".to_string(),
                            onclick: move |_| {
                                navigator.push(Route::ExtensionsPage {});
                            },
                        }
                    }
                }
                Button {
                    label: t("common.back"),
                    variant: ButtonVariant::Text,
                    icon: "back".to_string(),
                    onclick: move |_| {
                        navigator.go_back();
                    },
                }
            }
        }
    } else if ext_mode {
        let ended = ext_ended();
        let ep_label = tf("player.episode", &[("n", &episode.to_string())]);
        let busy = match fs.phase {
            Phase::Resolving => Some(resolving_label.clone()),
            Phase::Buffering => Some(t("player.external.starting")),
            _ => None,
        };
        let time = format!("{} / {}", format_time(st.position), format_time(st.duration));
        let progress_label = t("player.seek");
        let can_pick = !fs.legacy && !fs.sources.is_empty();
        rsx! {
            div { class: "tk-player__ext",
                div { class: "tk-xmpv",
                    div { class: "tk-xmpv__art",
                        if poster.is_empty() {
                            Icon { name: "external".to_string(), size: 48 }
                        } else {
                            img { src: "{poster}", alt: "" }
                        }
                    }
                    div { class: "tk-xmpv__body",
                        span { class: "tk-xmpv__eyebrow",
                            Icon { name: "external".to_string(), size: 16 }
                            span { {t("player.external.title")} }
                        }
                        h2 { class: "tk-xmpv__title", "{title}" }
                        p { class: "tk-xmpv__ep",
                            span { "{ep_label}" }
                            if !source_label.is_empty() {
                                span { class: "tk-xmpv__src", "{source_label}" }
                            }
                        }
                        div { class: "tk-xmpv__status", role: "status", "aria-live": "polite",
                            if let Some(text) = busy {
                                div { class: "tk-xmpv__busy",
                                    LoadingIndicator { small: true }
                                    span { "{text}" }
                                }
                            } else if fs.phase == Phase::Manual {
                                span { {t("src.choose_first")} }
                            } else if ended {
                                span { {t("player.external.ended")} }
                            } else {
                                div { class: "tk-xmpv__time",
                                    span { "{time}" }
                                    if st.paused {
                                        span { class: "tk-xmpv__paused", {t("player.external.paused")} }
                                    }
                                }
                                WavyProgress { fraction: st.fraction(), label: progress_label, active: !st.paused }
                            }
                        }
                        p { class: "tk-xmpv__hint", {t("player.external.hint")} }
                        div { class: "tk-xmpv__actions",
                            Button {
                                label: t("player.external.stop"),
                                variant: ButtonVariant::Filled,
                                icon: "close".to_string(),
                                onclick: {
                                    let leave = ext_leave.clone();
                                    move |_| leave()
                                },
                            }
                            if can_pick {
                                Button {
                                    label: tf("src.all", &[("n", &fs.sources.len().to_string())]),
                                    variant: ButtonVariant::Tonal,
                                    icon: "tune".to_string(),
                                    onclick: move |_| picker_open.set(true),
                                }
                            }
                            Button {
                                label: t("common.back"),
                                variant: ButtonVariant::Text,
                                icon: "back".to_string(),
                                onclick: {
                                    let leave = ext_leave.clone();
                                    move |_| leave()
                                },
                            }
                        }
                    }
                }
            }
        }
    } else if relay {
        // mpv-anime-build's uosc draws the controls; we keep only Back and the pre-playback status.
        let busy = match fs.phase {
            Phase::Resolving => Some(resolving_label.clone()),
            Phase::Buffering if !st.video_ready => Some(t("player.buffering")),
            _ => None,
        };
        rsx! {
            div { class: "tk-uosc", "data-visible": "{visible}",
                div { class: "tk-uosc__top",
                    IconButton {
                        icon: "back".to_string(),
                        label: t("player.back"),
                        onclick: {
                            let (backend, id) = (backend.clone(), id.clone());
                            move |_| {
                                report(&backend, &id, episode, &pstate.peek());
                                navigator.go_back();
                            }
                        },
                    }
                }
                if let Some(text) = busy {
                    div { class: "tk-pc__center",
                        div { class: "tk-pc__status", role: "status",
                            LoadingIndicator { small: true }
                            span { "{text}" }
                        }
                    }
                } else if fs.phase == Phase::Manual {
                    div { class: "tk-pc__center",
                        Button {
                            label: t("src.choose_first"),
                            icon: "tune".to_string(),
                            variant: ButtonVariant::Tonal,
                            class: "tk-pc__choose".to_string(),
                            onclick: move |_| picker_open.set(true),
                        }
                    }
                }
            }
        }
    } else {
        rsx! {
            PlayerControls {
                state: st.clone(),
                title,
                episode,
                visible,
                preset: preset(),
                presets: !enhanced(),
                phase: fs.phase,
                resolving_label,
                source_label,
                source_icon: source_icon.to_string(),
                source_clickable: !fs.legacy,
                has_next,
                markers: markers(),
                poster: poster.clone(),
                tracks: tracks(),
                sub_track: sub_track(),
                audio_track: audio_track(),
                qualities,
                quality,
                quality_auto,
                speed: speed(),
                smoothing: smoothing(),
                pip: pip(),
                on_back: go_back,
                on_toggle_pause: move |_| {
                    if let Some(p) = player.peek().clone() {
                        let _ = if pstate.peek().paused { p.play() } else { p.pause() };
                    }
                },
                on_seek: move |f: f64| {
                    if let Some(p) = player.peek().clone() {
                        let target = pstate.peek().seek_target(f);
                        let _ = p.seek(target);
                        dispatch(&mut pstate, PlayerAction::Position(target));
                    }
                },
                on_seek_by: move |d: f64| {
                    if let Some(p) = player.peek().clone() {
                        let _ = p.seek_relative(d);
                    }
                },
                on_volume: move |v: u8| {
                    if let Some(p) = player.peek().clone() {
                        let _ = p.set_volume(f64::from(v));
                    }
                    dispatch(&mut pstate, PlayerAction::SetVolume(v));
                },
                on_toggle_mute: move |_| {
                    if let Some(p) = player.peek().clone() {
                        let _ = p.set_mute(!pstate.peek().muted);
                    }
                    dispatch(&mut pstate, PlayerAction::ToggleMute);
                },
                on_preset: {
                    let backend = backend.clone();
                    move |next: ShaderPreset| {
                        if let Some(p) = player.peek().clone()
                            && apply_preset(&p, next, &mut notice)
                        {
                            preset.set(next);
                            let s = reduce_settings(&ui.settings.peek(), SettingsAction::SetPreset(next));
                            ui.commit_settings(&backend, s);
                        }
                    }
                },
                on_next: {
                    let id = id.clone();
                    move |_| {
                        navigator.push(Route::PlayerPage { id: id.clone(), episode: episode + 1 });
                    }
                },
                on_fullscreen: {
                    let win = win.clone();
                    move |_| {
                        let on = !*fullscreen.peek();
                        fullscreen.set(on);
                        win.set_fullscreen(on);
                    }
                },
                on_source: move |_| picker_open.set(true),
                on_skip: move |_| {
                    // Read the position first: a guard held in the let-chain would still borrow pstate in the body.
                    let position = pstate.peek().position;
                    if let Some(target) = markers.peek().skip_target(position)
                        && let Some(p) = player.peek().clone()
                    {
                        let _ = p.seek(target);
                        dispatch(&mut pstate, PlayerAction::Position(target));
                    }
                },
                // tk-player has no track / speed / interpolation setters yet: the menus keep the choice in
                // the UI and confirm it; wiring them is an integration note for the tk-player owner.
                on_sub: {
                    let ctx = ctx.clone();
                    move |sid: i64| {
                        sub_track.set(sid);
                        let label = tracks.peek().iter().find(|tr| tr.kind == TrackKind::Sub && tr.id == sid).map(crate::components::player_controls::track_label);
                        ctx.flash(label.unwrap_or_else(|| t("player.off")));
                    }
                },
                on_audio: {
                    let ctx = ctx.clone();
                    move |aid: i64| {
                        audio_track.set(aid);
                        if let Some(label) = tracks.peek().iter().find(|tr| tr.kind == TrackKind::Audio && tr.id == aid).map(crate::components::player_controls::track_label) {
                            ctx.flash(label);
                        }
                    }
                },
                on_quality: {
                    let ctx = ctx.clone();
                    move |q: Option<String>| match q {
                        None => ctx.auto(),
                        Some(q) => {
                            let target = flow.peek().source_for_quality(&q).map(|s| s.id.clone());
                            if let Some(sid) = target {
                                ctx.pick(sid);
                                ctx.flash(q);
                            }
                        }
                    }
                },
                on_speed: {
                    let ctx = ctx.clone();
                    move |s: f64| {
                        speed.set(s);
                        ctx.flash(format_speed(s));
                    }
                },
                on_smoothing: {
                    let ctx = ctx.clone();
                    move |_| {
                        let on = !*smoothing.peek();
                        smoothing.set(on);
                        ctx.flash(if on { t("player.smoothing") } else { format!("{}: {}", t("player.smoothing"), t("player.off")) });
                    }
                },
                on_pip: {
                    let (win, pip_win) = (win.clone(), pip_win.clone());
                    move |_| {
                        let on = !*pip.peek();
                        pip.set(on);
                        set_pip(&win, on, &pip_win);
                    }
                },
                on_capture: {
                    let ctx = ctx.clone();
                    move |_| ctx.flash(t("error.not_wired"))
                },
            }
        }
    };

    rsx! {
        div {
            class: "tk-player",
            onmousemove: move |_| {
                if now_ms().saturating_sub(pstate.peek().last_activity_ms) > 250 {
                    dispatch(&mut pstate, PlayerAction::Activity(now_ms()));
                }
            },
            div {
                id: "tk-video-hole",
                class: "tk-hole",
                "data-open": "{hole_open}",
                "data-relay": "{relay}",
                tabindex: hole_tab,
                // Embedded enhanced + uosc: pointer and keys go to mpv (coordinates are relative to the hole,
                // which is exactly the video surface).
                onmousemove: move |e: MouseEvent| {
                    if relay && let Some(p) = player.peek().clone() {
                        let pt = e.element_coordinates();
                        let _ = p.forward_mouse(pt.x, pt.y, *dpr.peek());
                    }
                },
                onmousedown: move |e: MouseEvent| {
                    if relay && let Some(p) = player.peek().clone()
                        && let Some(name) = e.trigger_button().and_then(|b| mpv_mouse_button(button_index(b)))
                    {
                        let pt = e.element_coordinates();
                        let _ = p.forward_mouse(pt.x, pt.y, *dpr.peek());
                        let _ = p.forward_key(name, KeyAction::Down);
                    }
                },
                onmouseup: move |e: MouseEvent| {
                    if relay && let Some(p) = player.peek().clone()
                        && let Some(name) = e.trigger_button().and_then(|b| mpv_mouse_button(button_index(b)))
                    {
                        let _ = p.forward_key(name, KeyAction::Up);
                    }
                },
                ondoubleclick: move |_| {
                    if relay && let Some(p) = player.peek().clone() {
                        let _ = p.forward_key("MBTN_LEFT_DBL", KeyAction::Press);
                    }
                },
                oncontextmenu: move |e: MouseEvent| {
                    if relay {
                        e.prevent_default();
                    }
                },
                onwheel: move |e: WheelEvent| {
                    if relay && let Some(p) = player.peek().clone() {
                        let d = e.delta().strip_units();
                        let name = if d.y < 0.0 {
                            "WHEEL_UP"
                        } else if d.y > 0.0 {
                            "WHEEL_DOWN"
                        } else if d.x < 0.0 {
                            "WHEEL_LEFT"
                        } else if d.x > 0.0 {
                            "WHEEL_RIGHT"
                        } else {
                            return;
                        };
                        let _ = p.forward_key(name, KeyAction::Press);
                    }
                },
                onkeydown: move |e: KeyboardEvent| {
                    if relay && let Some(p) = player.peek().clone() {
                        let key = e.key().to_string();
                        let m = e.modifiers();
                        // Escape and F stay ours (window full screen); the global bridge already handles them.
                        if key == "Escape" || (key.eq_ignore_ascii_case("f") && !m.ctrl() && !m.alt() && !m.meta()) {
                            return;
                        }
                        if let Some(name) = mpv_key_name(&key, m.ctrl(), m.alt(), m.shift(), m.meta()) {
                            e.prevent_default();
                            let _ = p.forward_key(&name, KeyAction::Down);
                        }
                    }
                },
                onkeyup: move |e: KeyboardEvent| {
                    if relay && let Some(p) = player.peek().clone() {
                        let key = e.key().to_string();
                        let m = e.modifiers();
                        if key == "Escape" || (key.eq_ignore_ascii_case("f") && !m.ctrl() && !m.alt() && !m.meta()) {
                            return;
                        }
                        if let Some(name) = mpv_key_name(&key, m.ctrl(), m.alt(), m.shift(), m.meta()) {
                            let _ = p.forward_key(&name, KeyAction::Up);
                        }
                    }
                },
            }
            {body}
            if let Some(left) = next_left {
                NextEpisodeCard {
                    episode: episode + 1,
                    left,
                    poster: poster.clone(),
                    on_play: {
                        let id = id.clone();
                        move |_| {
                            next_since.set(None);
                            navigator.push(Route::PlayerPage { id: id.clone(), episode: episode + 1 });
                        }
                    },
                    on_cancel: move |_| {
                        next_since.set(None);
                        // External mpv already exited: nothing left to show here.
                        if *ext_closed.peek() {
                            navigator.go_back();
                        }
                    },
                }
            }
            if let Some(text) = flash_text {
                div { class: "tk-toast tk-toast--flash", role: "status", "{text}" }
            }
            if let Some(n) = notice() {
                div { class: "tk-toast", role: "status", "{n}" }
            }
            SourcePicker {
                open: picker_open() && !fs.legacy,
                sources: picker_sources,
                prefs,
                on_pick: {
                    let ctx = ctx.clone();
                    move |sid: String| {
                        picker_open.set(false);
                        ctx.pick(sid);
                    }
                },
                on_close: move |_| picker_open.set(false),
            }
        }
    }
}
