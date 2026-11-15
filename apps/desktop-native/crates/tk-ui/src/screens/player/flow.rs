//! Source flow of the player screen (wave A, worker **e**; docs/redesign/15-parity-wave-a-contract.md section 4):
//! the [`Flow`] handles shared by the resolve loop, the mpv event pump and the UI callbacks, plus the small
//! helpers around `Backend` calls. Moved verbatim out of `player.rs` by the wave-A contract; `mod.rs` renders,
//! `torrent.rs` keeps the torrent session, `subs.rs` (worker d) the subtitle logic.
//!
//! Wave 1 (docs/redesign/08 section 7): the ranked `Backend::sources` list drives a pure
//! [`FlowState`](crate::player_state::FlowState) (Resolving / Buffering / Playing / Error / Manual) with
//! failover and `report_source_result`; Manual mode resolves nothing until a source is picked. When the
//! backend has no `sources` yet (`NotWired`) the old `Backend::playback` streams feed the same flow.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use dioxus::prelude::*;
use dioxus::router::Navigator;
use tk_player::{ExternalEvent, ExternalPlayer, MediaSource, Player, PlayerError, ShaderPreset, SubtitleSource};
use tokio::sync::broadcast::error::RecvError;

use crate::app::Route;
use crate::backend::{BackendRef, PlaybackVm, ReleaseTagsVm, SourceKindVm, SourceVm};
use crate::i18n::{t, tf};
use crate::model::{
    ErrorKind, PlatformVm, PlayerAction, PlayerState, SourcePrefsVm, UiError, next_episode, reduce_player, should_report,
};
use crate::platform::now_ms;
use crate::player_state::{FlowState, MAX_LEGACY_STREAMS, Phase, is_info_hash};

use super::torrent::{BATCH_NEXT, BatchNext, NO_FILE_NOTICE_EXTRA_MS, TorrentSession, TorrentSlot};

/// BUG-134: longest wait for the confirmed pick to leave "Resolving" before its list is refreshed.
const REVALIDATE_WAIT_MS: u64 = 8_000;

thread_local! {
    /// Set by an episode switch for the screen being left: `Some(fullscreen)` keeps the window as it is.
    pub(super) static EPISODE_SWITCH: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
    /// Read once by the next screen: start in fullscreen.
    pub(super) static CARRY_FULLSCREEN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub(super) fn dispatch(sig: &mut Signal<PlayerState>, action: PlayerAction) {
    let next = reduce_player(&sig.peek(), action);
    sig.set(next);
}

pub(super) fn media_source(vm: &PlaybackVm, idx: usize) -> MediaSource {
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

pub(super) fn load_stream(p: &Player, vm: &PlaybackVm, idx: usize, start: f64) -> Result<(), PlayerError> {
    p.load(&media_source(vm, idx), start)
}

/// "Check the log in <real data dir>/logs" (docs/redesign/09 section 4.2); the old text when the backend has no path.
pub(super) fn no_player_text(backend: &BackendRef) -> String {
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
pub(super) struct ExternalSlot(RefCell<Option<Arc<ExternalPlayer>>>);

impl ExternalSlot {
    pub(super) fn set(&self, p: Option<Arc<ExternalPlayer>>) {
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

pub(super) fn report(backend: &BackendRef, id: &str, episode: u32, st: &PlayerState) {
    if st.progress_secs() == 0 {
        return;
    }
    let (backend, id, pos, dur) = (backend.clone(), id.to_owned(), st.progress_secs(), st.duration_secs());
    // `spawn_forever`: the last report is sent while the screen unmounts ("Back", another episode); a task of the
    // player's own scope is cancelled with it and the final position was never saved (BUG-034).
    dioxus::core::spawn_forever(async move {
        if let Err(e) = backend.record_progress(&id, episode, pos, dur).await {
            tracing::warn!(detail = %e.detail, "recording progress failed");
        }
    });
}

/// Feeds the reliability average (`ok` = playback started). `NotWired` is expected until F lands.
pub(super) fn report_source(backend: &BackendRef, source_id: String, ok: bool) {
    let backend = backend.clone();
    spawn(async move {
        if let Err(e) = backend.report_source_result(&source_id, ok).await
            && e.kind != ErrorKind::NotWired
        {
            tracing::warn!(detail = %e.detail, "reporting the source result failed");
        }
    });
}

pub(super) fn remember_pick(backend: &BackendRef, id: &str, episode: u32, source_id: Option<String>) {
    let (backend, id) = (backend.clone(), id.to_owned());
    spawn(async move {
        if let Err(e) = backend.set_picked_source(&id, episode, source_id.as_deref()).await
            && e.kind != ErrorKind::NotWired
        {
            tracing::warn!(detail = %e.detail, "saving the picked source failed");
        }
    });
}

/// Applies `next`; when its shader files are missing (development build, fresh Linux install) downloads them once
/// through `Backend::fetch_shaders` and tries again. `on_active` runs when the preset is finally active (callers
/// persist the choice there, so a preset that is refused or never renders is never saved). `on_refused` runs when
/// the renderer cannot take the preset (BUG-033): the player falls back to Off and says why.
pub(super) fn choose_preset(
    backend: &BackendRef,
    p: Arc<Player>,
    next: ShaderPreset,
    mut notice: Signal<Option<String>>,
    on_active: impl FnOnce() + 'static,
    on_refused: impl FnOnce() + 'static,
) {
    if !p.supports_preset(next) {
        refuse_preset(&p, next, &mut notice);
        on_refused();
        return;
    }
    let data_dir = backend.data_dir_display();
    match p.set_shader_preset(next) {
        Ok(()) => {
            notice.set(None);
            arm_preset_guard(&data_dir, next);
            on_active();
        }
        Err(PlayerError::ShadersMissing(_) | PlayerError::NoShadersDir) => {
            notice.set(Some(t("player.shaders_fetching")));
            let backend = backend.clone();
            spawn(async move {
                match backend.fetch_shaders().await {
                    Ok(_) => {
                        if apply_preset(&p, next, &mut notice) {
                            arm_preset_guard(&data_dir, next);
                            on_active();
                        }
                    }
                    Err(e) => notice.set(Some(tf("player.shaders_fetch_failed", &[("detail", &e.detail)]))),
                }
            });
        }
        Err(PlayerError::PresetUnsupported(_)) => {
            refuse_preset(&p, next, &mut notice);
            on_refused();
        }
        Err(e) => tracing::warn!(error = %e, "changing the shader preset failed"),
    }
}

/// BUG-033: the renderer refused `preset`; go back to no preset and explain it.
fn refuse_preset(p: &Player, preset: ShaderPreset, notice: &mut Signal<Option<String>>) {
    tracing::warn!(preset = preset.id(), "preset not supported by the embedded renderer; falling back to Off");
    let _ = p.set_shader_preset(ShaderPreset::Off);
    disarm_preset_guard();
    notice.set(Some(tf("player.preset_unsupported", &[("preset", &t(&format!("preset.{}", preset.id())))])));
}

/// Returns whether the preset is now active.
pub(super) fn apply_preset(p: &Player, preset: ShaderPreset, notice: &mut Signal<Option<String>>) -> bool {
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

// --- BUG-033 crash guard ---------------------------------------------------------------------------------------
// A shader that kills mpv takes the whole app with it (libmpv is in-process), and a saved preset would crash every
// later playback. While a preset has not yet rendered a few seconds of video, `<data>/preset-guard` holds its id;
// the next player start finds the file, turns the preset off and says so.

const GUARD_FILE: &str = "preset-guard";
/// Seconds of playback a preset must survive before it counts as safe.
const GUARD_SAFE_SECS: f64 = 5.0;

/// (guard file, first playback position seen after arming).
static PRESET_GUARD: std::sync::Mutex<Option<(PathBuf, Option<f64>)>> = std::sync::Mutex::new(None);

fn guard_path(data_dir: &str) -> Option<PathBuf> {
    (!data_dir.trim().is_empty()).then(|| std::path::Path::new(data_dir).join(GUARD_FILE))
}

pub(super) fn arm_preset_guard(data_dir: &str, preset: ShaderPreset) {
    if preset == ShaderPreset::Off {
        disarm_preset_guard();
        return;
    }
    let Some(path) = guard_path(data_dir) else { return };
    if let Err(e) = std::fs::write(&path, preset.id()) {
        tracing::warn!(error = %e, "writing the preset guard failed");
        return;
    }
    if let Ok(mut g) = PRESET_GUARD.lock() {
        *g = Some((path, None));
    }
}

/// The preset is gone or proved safe: forget the guard.
pub(super) fn disarm_preset_guard() {
    if let Ok(mut g) = PRESET_GUARD.lock()
        && let Some((path, _)) = g.take()
    {
        let _ = std::fs::remove_file(path);
    }
}

/// Called with every playback position: once the armed preset rendered [`GUARD_SAFE_SECS`], the guard is removed.
pub(super) fn preset_guard_tick(pos: f64) {
    let safe = {
        let Ok(mut g) = PRESET_GUARD.lock() else { return };
        let Some((_, start)) = g.as_mut() else { return };
        match *start {
            // A seek backwards restarts the count.
            Some(s) if pos >= s => pos - s >= GUARD_SAFE_SECS,
            _ => {
                *start = Some(pos);
                false
            }
        }
    };
    if safe {
        disarm_preset_guard();
    }
}

/// The preset the previous run was trying when it died, if any (the guard file is consumed).
pub(super) fn take_crashed_preset(data_dir: &str) -> Option<ShaderPreset> {
    let path = guard_path(data_dir)?;
    let id = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    ShaderPreset::from_id(id.trim())
}

/// Old `Backend::playback` streams as flow sources (index i = `vm.streams[i]`).
pub(super) fn legacy_sources(vm: &PlaybackVm) -> Vec<SourceVm> {
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
            debrid: None,
            embed: false,
        })
        .collect()
}

/// Handles shared by the resolve loop, the mpv event pump and the UI callbacks (all `Copy` except the
/// backend and the id).
#[derive(Clone)]
pub(super) struct Flow {
    pub(super) backend: BackendRef,
    pub(super) id: String,
    pub(super) episode: u32,
    pub(super) flow: Signal<FlowState>,
    pub(super) pstate: Signal<PlayerState>,
    /// The playback of the current source (title, poster, start position).
    pub(super) meta: Signal<Option<PlaybackVm>>,
    /// Legacy path only: the streams from `Backend::playback`.
    pub(super) legacy: Signal<Option<PlaybackVm>>,
    pub(super) player: Signal<Option<Arc<Player>>>,
    pub(super) flash: Signal<Option<(String, u64)>>,
    /// External mode: the mpv executable (`None` in the embedded modes).
    pub(super) ext_mpv: Signal<Option<PathBuf>>,
    /// External mode with lsfg-vk frame generation (`Settings::video.frame_gen`, Linux).
    pub(super) frame_gen: Signal<Option<tk_player::FrameGen>>,
    pub(super) external: Rc<ExternalSlot>,
    /// External mode: the file played to the end / mpv exited by itself.
    pub(super) ext_ended: Signal<bool>,
    pub(super) ext_closed: Signal<bool>,
    pub(super) next_since: Signal<Option<u64>>,
    pub(super) nav: Navigator,
    /// The torrent being streamed (mirrors `tslot` for rendering).
    pub(super) torrent: Signal<Option<TorrentSession>>,
    pub(super) tslot: Rc<TorrentSlot>,
    /// Batch next episode: `(source id, torrent id)`; that source plays the torrent again instead of resolving.
    pub(super) reuse: Signal<Option<(String, String)>>,
    /// A torrent whose files had no match for this episode (the error screen offers its Files page).
    pub(super) files_hint: Signal<Option<String>>,
    pub(super) picker_open: Signal<bool>,
    /// The window is in fullscreen (carried to the next episode's screen, [`EPISODE_SWITCH`]).
    pub(super) fullscreen: Signal<bool>,
    /// `Settings::sources` at boot: a remembered pick that no longer meets them is ignored (audit bug
    /// `sources.remembered-vs-prefs`, `FlowState::new`).
    pub(super) source_prefs: SourcePrefsVm,
    /// BUG-080: the original stream of an attempt that loaded the filtered HLS master instead.
    pub(super) hls_original: Rc<RefCell<Option<HlsOriginal>>>,
}

/// The original master of `attempt` when it is still the current one and nothing played yet (pure part of
/// [`Flow::take_hls_original`]).
fn take_hls_original(slot: &mut Option<HlsOriginal>, attempt: u64, current: u64, phase: Phase) -> Option<HlsOriginal> {
    if attempt != current || phase != Phase::Buffering || slot.as_ref().is_none_or(|o| o.attempt != attempt) {
        return None;
    }
    slot.take()
}

/// BUG-080 safety net: when the filtered master (BUG-059) fails before anything plays, the attempt loads the master
/// it replaced, once, before failing over.
#[derive(Clone)]
pub(super) struct HlsOriginal {
    pub(super) attempt: u64,
    pub(super) vm: PlaybackVm,
    pub(super) start: f64,
}

impl Flow {
    pub(super) fn flash(&self, text: String) {
        let mut f = self.flash;
        f.set(Some((text, now_ms())));
    }

    /// Resolves and loads whatever `FlowState::to_resolve` asks for, following failover until a stream is
    /// handed to mpv, the list runs out, or a newer attempt (a pick) takes over. `resume` keeps the
    /// position when switching sources mid-episode.
    pub(super) async fn drive(self, resume: Option<f64>) {
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
            let source = flow.peek().sources[idx].clone();
            let source_id = source.id.clone();
            dispatch(&mut pstate, PlayerAction::Loading);
            // A batch handed over by the previous episode plays that torrent again (no source resolution).
            let reused = self.reuse.peek().as_ref().filter(|(sid, _)| *sid == source_id).map(|(_, tid)| tid.clone());
            let mut session = None;
            let resolved = if legacy {
                self.legacy
                    .peek()
                    .clone()
                    .map(|vm| PlaybackVm { streams: vm.streams.get(idx).cloned().into_iter().collect(), ..vm })
                    .ok_or_else(|| UiError::new(ErrorKind::NoSource, ""))
            } else if source.kind == SourceKindVm::Torrent && reused.is_none() && source.debrid.is_some_and(|d| d.cached) {
                // Wave A, worker i: a torrent cached on a connected debrid service plays as a direct web stream
                // (`SourceVm::debrid`); the engine is never started for it.
                self.backend.debrid_play(&source_id, &self.id, self.episode).await
            } else if source.kind == SourceKindVm::Torrent {
                // docs/redesign/10 section 5.5: torrents go through `torrent_play` (loopback HTTP URL), never
                // `playback_source`.
                let target = reused.clone().unwrap_or_else(|| source_id.clone());
                match self.backend.torrent_play(&target, &self.id, self.episode).await {
                    Ok(tvm) => {
                        session = Some(TorrentSession {
                            torrent_id: tvm.torrent_id,
                            file_index: tvm.file_index,
                            file_name: tvm.file_name,
                            batch: tvm.batch,
                            source: SourceVm { picked: false, ..source.clone() },
                        });
                        Ok(tvm.playback)
                    }
                    Err(e) => Err(e),
                }
            } else {
                self.backend.playback_source(&self.id, self.episode, &source_id).await
            };
            if flow.peek().attempt != attempt {
                return;
            }
            if let Err(e) = &resolved
                && source.kind == SourceKindVm::Torrent
                && e.kind == ErrorKind::NoSource
                && is_info_hash(&e.detail)
            {
                let mut hint = self.files_hint;
                hint.set(Some(e.detail.to_ascii_lowercase()));
            }
            if resolved.is_ok() {
                // A web source (or another torrent) replaces the torrent that was playing: that one ends.
                self.set_torrent(session);
            }
            let loaded = match resolved {
                Ok(vm) if vm.streams.is_empty() => Err(UiError::new(ErrorKind::NoSource, "")),
                Ok(mut vm) => {
                    let start = resume.filter(|p| *p > 0.0).unwrap_or(f64::from(vm.start_secs));
                    meta.set(Some(vm.clone()));
                    self.hls_original.replace(None);
                    // BUG-059: FFmpeg opens every rendition of an HLS master before the first frame; mpv gets a
                    // copy with the variant and languages it plays (headerless sources such as proxy URLs).
                    if let Some(s) = vm.streams.first()
                        && let Some(local) = self.backend.mpv_hls_master(&s.url, &s.headers).await
                    {
                        if flow.peek().attempt != attempt {
                            return;
                        }
                        self.hls_original.replace(Some(HlsOriginal { attempt, vm: vm.clone(), start }));
                        vm.streams[0].url = local;
                    }
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
                    if reused.is_some() {
                        // The batch has no file for this episode (or the torrent is gone): resolve the episode's
                        // sources like a normal start.
                        let mut reuse = self.reuse;
                        reuse.set(None);
                        self.reload().await;
                    }
                }
            }
        }
    }

    /// The episode's ranked sources, as at boot (used when a handed-over batch cannot play this episode).
    pub(super) async fn reload(&self) {
        let mode = self.flow.peek().mode;
        match self.backend.sources(&self.id, self.episode).await {
            Ok(list) => {
                let state = FlowState::new(list, mode, &self.source_prefs);
                if state.dropped_pick.is_some() {
                    remember_pick(&self.backend, &self.id, self.episode, None);
                }
                let waiting = state.phase == Phase::Manual;
                let mut flow = self.flow;
                flow.set(state);
                if waiting {
                    let mut open = self.picker_open;
                    open.set(true);
                }
            }
            Err(e) => tracing::warn!(kind = ?e.kind, detail = %e.detail, "listing the sources again failed"),
        }
    }

    /// The torrent now streaming (`None` for a web source); the previous one ends if it is a different torrent.
    pub(super) fn set_torrent(&self, session: Option<TorrentSession>) {
        self.tslot.replace(&self.backend, session.clone());
        let mut t = self.torrent;
        if *t.peek() != session {
            t.set(session);
        }
    }

    /// Remember the playhead for `completed` (cheap: a `Cell`).
    pub(super) fn track_fraction(&self, fraction: f64) {
        if fraction.is_finite() {
            self.tslot.fraction.set(fraction);
        }
    }

    /// Another episode replaces this screen (remounted, BUG-028): keep the window's fullscreen across it.
    fn switch_episode(&self, episode: u32) {
        let fullscreen = *self.fullscreen.peek();
        EPISODE_SWITCH.with(|s| s.set(Some(fullscreen)));
        CARRY_FULLSCREEN.with(|c| c.set(fullscreen));
        // Replace, not push: "Back" leaves the player instead of walking through every episode watched.
        self.nav.replace(Route::PlayerPage { id: self.id.clone(), episode });
    }

    /// "Next episode": a batch torrent is handed to the next screen (same torrent, its next file); otherwise the
    /// next screen resolves sources as usual and this torrent ends when this screen goes away.
    pub(super) fn go_next(&self) {
        let session = self.tslot.session.borrow().clone();
        if let Some(s) = session.filter(|s| s.batch) {
            BATCH_NEXT.with(|b| {
                *b.borrow_mut() =
                    Some(BatchNext { id: self.id.clone(), episode: self.episode + 1, torrent_id: s.torrent_id, source: s.source })
            });
            self.tslot.handed_over.set(true);
        }
        self.switch_episode(self.episode + 1);
    }

    /// "Previous episode" (button, `J`): episode N-1 with its best source. Nothing before episode 1.
    pub(super) fn go_prev(&self) {
        if self.episode > 1 {
            self.go_to(self.episode - 1);
        }
    }

    /// Jump to another episode of this title (episode list, previous episode): the position of this one is saved
    /// first; the new screen resolves its sources as usual.
    pub(super) fn go_to(&self, episode: u32) {
        if episode == 0 || episode == self.episode {
            return;
        }
        report(&self.backend, &self.id, self.episode, &self.pstate.peek());
        self.switch_episode(episode);
    }

    /// Records a failure of `attempt`; the caller (or the `drive` loop) resolves the next one.
    pub(super) fn fail(&self, attempt: u64, e: UiError) {
        let mut flow = self.flow;
        // A torrent without this episode's file: say so, longer than a flash, since playback now continues
        // from a different source than the one picked.
        let no_file = e.kind == ErrorKind::NoSource && is_info_hash(&e.detail);
        let failed = flow.write().fail(attempt, e);
        if let Some(sid) = failed {
            report_source(&self.backend, sid, false);
        }
        if flow.peek().to_resolve().is_some() {
            if no_file && flow.peek().last_failed_torrent() {
                let mut f = self.flash;
                let text = tf("player.torrent.no_file_next", &[("n", &self.episode.to_string())]);
                f.set(Some((text, now_ms() + NO_FILE_NOTICE_EXTRA_MS)));
            } else {
                self.flash(t("player.error.trying_next"));
            }
        }
    }

    /// BUG-134: the confirmed pick started from the last known list; ask for the full one behind it (once the pick
    /// left "Resolving", or after `REVALIDATE_WAIT`, so the listing does not compete with its start) and swap it in
    /// without touching what plays. A failure keeps the old list. `boot_gen` / `boot`: a later boot drops this one.
    pub(super) async fn revalidate_sources(self, boot_gen: Signal<u64>, boot: u64) {
        let started = now_ms();
        while self.flow.peek().phase == Phase::Resolving && now_ms().saturating_sub(started) < REVALIDATE_WAIT_MS {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        let listed = self.backend.sources(&self.id, self.episode).await;
        if *boot_gen.peek() != boot {
            return;
        }
        match listed {
            Ok(list) => {
                tracing::info!(sources = list.len(), "source list refreshed behind the confirmed pick");
                let mut flow = self.flow;
                if flow.write().refresh_sources(list) {
                    self.flash(t("player.error.trying_next"));
                    spawn(self.clone().drive(None));
                }
            }
            Err(e) => tracing::warn!(kind = ?e.kind, detail = %e.detail, "refreshing the source list failed; keeping the last one"),
        }
    }

    /// A user pick (picker, quality menu, "Use the best web source"): remember it and play it.
    pub(super) fn pick(&self, source_id: String) {
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
    pub(super) fn auto(&self) {
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

    /// BUG-080: the filtered HLS master of the current attempt failed before playing: the original master it
    /// replaced (once), `None` when this attempt did not use one or already went back to it.
    pub(super) fn take_hls_original(&self, attempt: u64) -> Option<HlsOriginal> {
        let (current, phase) = {
            let f = self.flow.peek();
            (f.attempt, f.phase)
        };
        take_hls_original(&mut self.hls_original.borrow_mut(), attempt, current, phase)
    }

    /// Load the original master after the filtered copy failed; if that fails too, the attempt fails as usual.
    pub(super) async fn play_hls_original(self, o: HlsOriginal) {
        tracing::warn!(url = %o.vm.streams.first().map(|s| s.url.as_str()).unwrap_or(""),
            "the filtered HLS master did not open; retrying with the original master (BUG-080)");
        let mpv = self.ext_mpv.peek().clone();
        let loaded = if let Some(mpv) = mpv {
            self.start_external(mpv, &o.vm, o.start, o.attempt).await
        } else {
            match self.player.peek().clone() {
                Some(p) => load_stream(&p, &o.vm, 0, o.start).map_err(|e| UiError::new(ErrorKind::Other, e.to_string())),
                None => Err(UiError::new(ErrorKind::Other, no_player_text(&self.backend))),
            }
        };
        if let Err(e) = loaded {
            tracing::warn!(kind = ?e.kind, detail = %e.detail, "source failed");
            self.fail(o.attempt, e);
            if self.flow.peek().to_resolve().is_some() {
                let next: std::pin::Pin<Box<dyn std::future::Future<Output = ()>>> = Box::pin(self.clone().drive(None));
                spawn(next);
            }
        }
    }

    /// "Retry" on the final error.
    pub(super) fn retry(&self) {
        let mut flow = self.flow;
        flow.write().retry();
        spawn(self.clone().drive(None));
    }

    /// External mode: open the resolved stream in the machine's mpv (closing the previous window) and pump its
    /// events. `Ok` hands the source to mpv; whether it plays is decided by the pump (`Position > 0` / `Duration`).
    pub(super) async fn start_external(&self, mpv: PathBuf, vm: &PlaybackVm, start: f64, attempt: u64) -> Result<(), UiError> {
        let src = media_source(vm, 0);
        let n = self.episode.to_string();
        let title = if vm.title.is_empty() {
            tf("player.episode", &[("n", &n)])
        } else {
            tf("player.external.window_title", &[("title", &vm.title), ("n", &n)])
        };
        self.external.set(None);
        let fg = self.frame_gen.peek().clone();
        let spawned = tokio::task::spawn_blocking(move || ExternalPlayer::spawn_with(&mpv, &src, start, &title, fg.as_ref())).await;
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
    pub(super) async fn pump_external(
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
                        self.track_fraction(pstate.peek().fraction());
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
                    self.track_fraction(1.0);
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
                    if let Some(o) = self.take_hls_original(attempt) {
                        // BUG-080: the filtered master first falls back to the original one.
                        let next: std::pin::Pin<Box<dyn std::future::Future<Output = ()>>> = Box::pin(self.clone().play_hls_original(o));
                        spawn(next);
                        return;
                    }
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
                    // The (fullscreen) mpv window is gone: bring the app back to the front (X11; Wayland compositors
                    // give focus back to it by themselves).
                    if let Some(w) = try_consume_context::<dioxus::desktop::DesktopContext>() {
                        w.set_focus();
                    }
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

#[cfg(test)]
mod preset_guard_tests {
    use super::*;

    #[test]
    fn a_crash_before_the_preset_renders_is_remembered_and_a_survivor_is_forgotten() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().to_string_lossy().into_owned();
        // Armed and the process "dies": the next start finds the preset once.
        arm_preset_guard(&data, ShaderPreset::ArtCnn);
        if let Ok(mut g) = PRESET_GUARD.lock() {
            *g = None; // what a restart leaves: only the file
        }
        assert_eq!(take_crashed_preset(&data), Some(ShaderPreset::ArtCnn));
        assert_eq!(take_crashed_preset(&data), None);
        // Armed, then plays GUARD_SAFE_SECS: the file is gone.
        arm_preset_guard(&data, ShaderPreset::Fsr);
        preset_guard_tick(100.0);
        preset_guard_tick(102.0);
        assert!(dir.path().join(GUARD_FILE).is_file(), "2 s is not enough");
        preset_guard_tick(90.0); // seek back restarts the count
        preset_guard_tick(94.0);
        assert!(dir.path().join(GUARD_FILE).is_file());
        preset_guard_tick(95.5);
        assert!(!dir.path().join(GUARD_FILE).exists());
        assert_eq!(take_crashed_preset(&data), None);
        // No data dir: nothing is written, nothing panics.
        arm_preset_guard("", ShaderPreset::Fsr);
        assert_eq!(take_crashed_preset(""), None);
    }
}

#[cfg(test)]
mod hls_original_tests {
    use super::*;

    fn original(attempt: u64) -> HlsOriginal {
        let vm = PlaybackVm { title: String::new(), poster: None, episodes_total: None, streams: Vec::new(), start_secs: 0 };
        HlsOriginal { attempt, vm, start: 12.0 }
    }

    #[test]
    fn the_filtered_master_falls_back_to_the_original_once_before_playing() {
        // BUG-080: a failure of the filtered copy loads the original master, once, only for its own attempt.
        let mut slot = Some(original(3));
        assert!(take_hls_original(&mut slot, 3, 3, Phase::Playing).is_none(), "it already played");
        assert!(take_hls_original(&mut slot, 2, 3, Phase::Buffering).is_none(), "an older attempt");
        assert!(take_hls_original(&mut slot, 3, 4, Phase::Buffering).is_none(), "a pick took over");
        assert_eq!(take_hls_original(&mut slot, 3, 3, Phase::Buffering).map(|o| o.start), Some(12.0));
        assert!(take_hls_original(&mut slot, 3, 3, Phase::Buffering).is_none(), "only once: then the usual failover");
        let mut other = Some(original(1));
        assert!(take_hls_original(&mut other, 2, 2, Phase::Buffering).is_none(), "the slot belongs to another attempt");
        assert!(other.is_some());
    }
}
