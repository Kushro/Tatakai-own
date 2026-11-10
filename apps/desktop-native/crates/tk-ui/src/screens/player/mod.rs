//! Player screen (review E5). mpv draws into a child window under the transparent web view; this page
//! leaves a hole (`#tk-video-hole`) whose rect is reported to `tk-player` and draws the controls over it.
//! The hole only becomes transparent once mpv reports `vo-configured`; until then it is an opaque scrim.
//!
//! Split by the wave-A contract (docs/redesign/15-parity-wave-a-contract.md section 2): this file renders the
//! screen and owns its hooks (worker **e**), [`flow`] holds the source flow (worker **e**), [`torrent`] the
//! torrent session (shared, read-only for worker **i**) and [`subs`] the subtitle logic (worker **d**: preferred
//! language, secondary subtitle, style, delay, external files, translation, capture).
//!
//! Wave 2 (docs/redesign/09 section 4.4): Settings > Player picks the engine at boot. Embedded enhanced
//! prepares mpv-anime-build first (falls back to Embedded with a notice); with its uosc UI our controls and
//! player shortcuts hide and the hole relays pointer / keys to mpv. External mpv never creates the embedded
//! player: the same source flow spawns the machine's mpv (`tk_player::ExternalPlayer`) and this page shows an
//! informative card (Stop / Back) until mpv closes, then returns to where the user came from.
//!
//! Wave 3 (docs/redesign/10 section 5.5): a torrent source resolves through `Backend::torrent_play` (a loopback
//! HTTP URL, so the three engines load it like any stream). While it loads, the status shows "Waiting for
//! metadata…" and then speed / peers / file progress (`torrent_stats`, 1 s); the seek bar draws the downloaded
//! ranges (`seek_marks`) and the ready-to-play run, plus intro / outro / recap marks (`skip_marks`, any source).
//! "Next episode" of a batch hands the same torrent to the next screen; leaving ends the session
//! (`torrent_playback_ended`).
//!
//! Wave A (worker **e**, docs/redesign/15 section 4): previous episode and the episode list ([`episodes`]),
//! auto-next with a configurable countdown (or a card that waits when auto-next is off), auto-skip intro, sleep
//! timer, loop, stable volume, real speed / smoothing, every rebindable shortcut, the `?` shortcut panel and
//! Discord Rich Presence ([`presence`]). A remembered source that no longer meets the preferences is ignored and
//! forgotten (`FlowState::new`, audit bug).

mod episodes;
mod flow;
pub mod presence;
/// Wave B (worker k): remote commands from a paired phone -> player shortcuts.
pub mod remote;
pub mod subs;
mod torrent;

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use dioxus::desktop::tao::dpi::PhysicalSize;
use dioxus::desktop::{DesktopContext, LogicalSize, use_window};
use dioxus::prelude::*;
use tk_player::{EngineMode, KeyAction, Player, PlayerEvent, Rect, ShaderPreset, TrackInfo, TrackKind};
use tokio::sync::broadcast::error::RecvError;

use crate::app::{Route, Services, Ui};
use crate::backend::{DetailsVm, PlaybackVm, PresenceVm, SourceKindVm, SourceVm, TorrentStateVm, TorrentStatsVm};
use crate::components::player_controls::{NextEpisodeCard, ShortcutHelp, track_label};
use crate::components::{
    Button, ButtonVariant, EmptyState, ErrorState, IconButton, LoadingIndicator, PlayerControls, SourcePicker,
    WavyProgress,
};
use crate::i18n::{t, tf};
use crate::icons::Icon;
use crate::model::{
    EnhancedUiVm, ErrorKind, PlayerAction, PlayerModeVm, PlayerState, SEEK_STEP_SECS, SettingsAction, UiError, format_time,
    next_episode, reduce_settings, should_report,
};
use crate::platform::{self, now_ms};
use crate::player_state::{
    FLASH_MS, FlowState, Markers, Phase, SUB_DELAY_STEP_SECS, SleepChoice, SleepTimer, auto_skip_target, countdown_left,
    cycle_track, format_speed, is_info_hash, keeps_controls_up, mpv_key_name, mpv_mouse_button, range_spans,
    ready_span, source_name, speed_step, volume_step,
};
use crate::shortcuts::Shortcut;

use self::episodes::EpisodeSheet;
use self::flow::{
    ExternalSlot, Flow, button_index, choose_preset, dispatch, legacy_sources, no_player_text, remember_pick, report,
    report_source,
};
use self::torrent::{SEEK_MARKS_EVERY, TorrentSession, TorrentSlot, end_torrent, take_batch_next, torrent_line};

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


/// Pointer movement re-flashes uosc's controls at most this often (its flash lasts 2 s).
const UOSC_FLASH_EVERY_MS: u64 = 1000;

/// `?` toggles the shortcut panel. Not a `Shortcut` (the panel is not rebindable): a listener of its own, replaced
/// on every mount so only the current screen's channel receives it.
const HELP_JS: &str = r#"
if (window.__tkHelpKey) window.removeEventListener('keydown', window.__tkHelpKey);
window.__tkHelpKey = (e) => {
  const el = e.target;
  const editable = !!el && (el.tagName === 'INPUT' && el.type !== 'range' || el.tagName === 'TEXTAREA' || el.isContentEditable);
  if (e.key === '?' && !editable && !e.ctrlKey && !e.metaKey && document.querySelector('.tk-shell[data-player="true"]')) dioxus.send(true);
};
window.addEventListener('keydown', window.__tkHelpKey);
"#;

/// Episode count from the playing source, else from the details.
fn known_total(meta: Option<&PlaybackVm>, info: Option<&DetailsVm>) -> Option<u32> {
    meta.and_then(|m| m.episodes_total).or_else(|| info.and_then(|d| d.episodes_total))
}

/// Title from the playing source, else from the details.
fn known_title(meta: Option<&PlaybackVm>, info: Option<&DetailsVm>) -> String {
    meta.map(|m| m.title.clone())
        .filter(|s| !s.is_empty())
        .or_else(|| info.map(|d| d.media.title.clone()))
        .unwrap_or_default()
}

/// `+100 ms` / `-200 ms` / `0 ms`.
fn delay_label(secs: f64) -> String {
    let ms = (secs * 1000.0).round() as i64;
    if ms > 0 { format!("+{ms} ms") } else { format!("{ms} ms") }
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
    // UX review #19: the controls stay up while the pointer rests on a bar (they used to hide under it).
    let mut bars_hover = use_signal(|| false);
    let flash = use_signal(|| None::<(String, u64)>);
    let mut player = use_signal(|| None::<Arc<Player>>);
    let mut preset = use_signal(|| ui.settings.peek().preset());
    let mut fullscreen = use_signal(|| false);
    let mut clock = use_signal(now_ms);
    let mut picker_open = use_signal(|| false);
    // Wave A, worker d: the subtitle sheet (style, delay, secondary, external file, translation).
    let mut subs_sheet = use_signal(|| false);
    let mut tracks = use_signal(Vec::<TrackInfo>::new);
    let mut sub_track = use_signal(|| 0_i64);
    let mut audio_track = use_signal(|| 0_i64);
    let mut speed = use_signal(|| 1.0_f64);
    let mut smoothing = use_signal(|| false);
    let mut pip = use_signal(|| false);
    let pip_win = use_hook(|| Rc::new(PipWindow::default()));
    let mut next_since = use_signal(|| None::<u64>);
    // Wave A (worker e): loop, sleep timer, auto-skip (once per screen), episode list and shortcut panel.
    let mut loop_on = use_signal(|| false);
    let mut sleep = use_signal(SleepTimer::default);
    let mut intro_skipped = use_signal(|| false);
    let mut episodes_open = use_signal(|| false);
    let mut help_open = use_signal(|| false);
    // Intro / outro / recap marks (`Backend::skip_marks`, AniSkip), for web and torrent sources alike.
    let mut markers = use_signal(Markers::default);
    // Torrent playback (docs/redesign/10 section 5.5).
    let torrent = use_signal(|| None::<TorrentSession>);
    let tslot = use_hook(|| Rc::new(TorrentSlot::default()));
    let reuse = use_signal(|| None::<(String, String)>);
    let files_hint = use_signal(|| None::<String>);
    let mut tstats = use_signal(|| None::<TorrentStatsVm>);
    // Downloaded ranges of the playing file (byte fractions).
    let mut have = use_signal(Vec::<(f64, f64)>::new);
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
    // Last time uosc was asked to show its controls (pointer movement over the hole).
    let mut uosc_flashed = use_signal(|| 0_u64);
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
        torrent,
        tslot: tslot.clone(),
        reuse,
        files_hint,
        picker_open,
        source_prefs: ui.settings.peek().sources.clone(),
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
                let batch = take_batch_next(&backend, &id, episode);
                let listed = match batch {
                    // Next episode of a batch: the same torrent, no source resolution (docs/redesign/10 section 5.5).
                    Some(next) => {
                        let mut reuse = ctx.reuse;
                        reuse.set(Some((next.source.id.clone(), next.torrent_id.clone())));
                        ctx.flash(t("player.torrent.batch_next"));
                        // Picked, so Manual mode plays it at once too.
                        Ok(vec![SourceVm { picked: true, ..next.source }])
                    }
                    None => backend.sources(&id, episode).await,
                };
                let state = match listed {
                    // A remembered pick that no longer meets the preferences is ignored and forgotten (audit bug).
                    Ok(list) => FlowState::new(list, mode, &settings.sources),
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
                if state.dropped_pick.is_some() {
                    remember_pick(&backend, &id, episode, None);
                }
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
                // The mpv instance outlives this screen: start every episode from a known state (normal speed, no
                // loop, no interpolation) and the saved stable-volume choice.
                let _ = p.set_speed(1.0);
                let _ = p.set_loop_file(false);
                let _ = p.set_interpolation(false);
                let _ = p.set_stable_volume(settings.flow.stable_volume);
                let initial = settings.preset();
                // Our presets are an Embedded-only feature: mpv-anime-build brings its own shaders.
                if initial != ShaderPreset::Off && !*enhanced.peek() {
                    choose_preset(&backend, p.clone(), initial, notice, || {});
                }
                // Manual mode without a pick: `to_resolve` is None, so this returns without network.
                spawn(ctx.clone().drive(None));

                let mut last_report = now_ms();
                let mut last_pos = -1.0_f64;
                // Worker i (torrent.preload-next): asked once per episode.
                let mut preload_asked = false;
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
                                ctx.track_fraction(pstate.peek().fraction());
                                // A single-file torrent preloads the next episode's best torrent at 80 % (a batch
                                // selects its next file by itself, live/torrent_play.rs).
                                if !preload_asked
                                    && pstate.peek().fraction() >= 0.8
                                    && ctx.tslot.session.borrow().as_ref().is_some_and(|s| !s.batch)
                                {
                                    preload_asked = true;
                                    let (b, id, ep) = (backend.clone(), ctx.id.clone(), ctx.episode);
                                    spawn(async move {
                                        if let Err(e) = b.torrent_preload_next(&id, ep).await {
                                            tracing::debug!(error = ?e, "next episode not preloaded");
                                        }
                                    });
                                }
                            }
                            if phase == Phase::Buffering && pos > 0.0 && !pstate.peek().buffering
                                && let Some(sid) = flow.write().started()
                            {
                                report_source(&backend, sid, true);
                            }
                            // Auto-skip intro (Settings > Player): once per episode, so seeking back into the
                            // intro lets the user watch it.
                            let auto_skip = ui.settings.peek().flow.auto_skip_intro;
                            let done = *intro_skipped.peek();
                            let target = auto_skip_target(&markers.peek(), pos, done);
                            if auto_skip
                                && phase == Phase::Playing
                                && let Some(target) = target
                            {
                                intro_skipped.set(true);
                                let _ = p.seek(target);
                                dispatch(&mut pstate, PlayerAction::Position(target));
                                ctx.flash(t("player.intro_skipped"));
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
                            // Wave A, worker d: preferred subtitle / audio language (audit bug), secondary subtitle,
                            // style and delay are applied here (`subs::on_tracks`).
                            let prefs = ui.settings.peek().clone();
                            let (sid, aid) = subs::on_tracks(Some(&p), &list, &prefs.subs, &prefs.sources);
                            sub_track.set(sid);
                            audio_track.set(aid);
                            tracks.set(list);
                        }
                        PlayerEvent::Eof => {
                            if phase != Phase::Playing {
                                continue;
                            }
                            dispatch(&mut pstate, PlayerAction::Eof);
                            ctx.track_fraction(1.0);
                            let mut done = pstate.peek().clone();
                            done.position = done.duration.max(done.position);
                            report(&backend, &id, episode, &done);
                            // Loop: mpv starts the file again by itself (`loop-file`), no next-episode card.
                            if *loop_on.peek() {
                                continue;
                            }
                            // Sleep timer "end of episode": stay paused here instead of playing the next one.
                            if *sleep.peek() == SleepTimer::EndOfEpisode {
                                sleep.set(SleepTimer::Off);
                                ctx.flash(t("player.sleep.done"));
                                continue;
                            }
                            let total = known_total(ctx.meta.peek().as_ref(), details.peek().as_ref().and_then(|d| d.as_ref()));
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

    // Next-episode countdown reached zero (`Settings::flow.auto_next_secs`). With auto-next off the card waits for
    // the user and nothing changes episode on its own.
    use_effect({
        let ctx = ctx.clone();
        move || {
            let now = clock();
            let Some(since) = next_since() else { return };
            let (auto_next, secs) = {
                let s = ui.settings.peek();
                (s.flow.auto_next, u64::from(s.flow.auto_next_secs))
            };
            if auto_next && countdown_left(since, now, secs) == 0 {
                next_since.set(None);
                ctx.go_next();
            }
        }
    });

    // Worker d: tk-player only emits `PlayerEvent::Tracks` on file load, so re-read the list when the subtitle
    // sheet adds or switches tracks (loaded file, pre-translated track).
    use_effect(move || {
        let _rev = *subs::TRACKS_REV.read();
        if let Some(p) = player.peek().clone() {
            let list = subs::read_tracks(&p);
            if !list.is_empty() {
                sub_track.set(subs::selected_id(&list, TrackKind::Sub));
                audio_track.set(subs::selected_id(&list, TrackKind::Audio));
                tracks.set(list);
            }
        }
    });

    // Sleep timer ran out: pause, and do not move on to the next episode.
    use_effect({
        let ctx = ctx.clone();
        move || {
            let now = clock();
            let timer = *sleep.peek();
            if !timer.due(now) {
                return;
            }
            sleep.set(SleepTimer::Off);
            next_since.set(None);
            if let Some(p) = player.peek().clone() {
                let _ = p.pause();
            }
            ctx.flash(t("player.sleep.done"));
        }
    });

    // `?`: the shortcut panel (not while mpv draws its own UI or plays in its own window).
    use_future(move || async move {
        let mut eval = document::eval(HELP_JS);
        while eval.recv::<bool>().await.is_ok() {
            if !*uosc.peek() && ext_mpv.peek().is_none() {
                help_open.toggle();
            }
        }
    });

    // Discord Rich Presence while watching (worker e): on mount, on every pause change and every 15 s.
    let paused_now = use_memo(move || pstate.read().paused);
    let send_watching = {
        let backend = backend.clone();
        move || {
            // Setting off: nothing reaches Discord and the shell reports `off`.
            if !ui.settings.peek().flow.discord_presence {
                presence::send(&backend, PresenceVm::Clear);
                return;
            }
            let state = {
                let m = meta.peek();
                let d = details.peek();
                presence::watching_state(m.as_ref(), d.as_ref().and_then(|d| d.as_ref()), &pstate.peek(), episode)
            };
            presence::send(&backend, state);
        }
    };
    use_effect({
        let send = send_watching.clone();
        move || {
            let _ = paused_now();
            send();
        }
    });
    use_future({
        let send = send_watching.clone();
        move || {
            let send = send.clone();
            async move {
                loop {
                    tokio::time::sleep(Duration::from_secs(presence::WATCHING_EVERY_SECS)).await;
                    send();
                }
            }
        }
    });

    // Intro / outro / recap marks of this episode (AniSkip through the backend; nothing marked when unknown).
    use_future({
        let (backend, id) = (backend.clone(), id.clone());
        move || {
            let (backend, id) = (backend.clone(), id.clone());
            async move {
                match backend.skip_marks(&id, episode).await {
                    Ok(list) => markers.set(Markers::from_skips(&list)),
                    Err(e) if e.kind == ErrorKind::NotWired => {}
                    Err(e) => tracing::debug!(kind = ?e.kind, detail = %e.detail, "no skip marks"),
                }
            }
        }
    });

    // Torrent stats (overlay line, external card) every second and the downloaded ranges every few seconds, while
    // a torrent source is resolving or playing. Before `torrent_play` answers, the info-hash of the source is polled
    // (metadata / peers of a torrent being added); errors just mean "nothing yet".
    use_future({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move {
                let mut tick: u64 = 0;
                let mut last_file: Option<(String, usize)> = None;
                loop {
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    tick += 1;
                    let session = torrent.peek().clone();
                    let pending = {
                        let f = flow.peek();
                        f.current_source()
                            .filter(|s| s.kind == SourceKindVm::Torrent && f.phase == Phase::Resolving)
                            .and_then(|s| s.info_hash.clone())
                            .filter(|h| is_info_hash(h))
                            .map(|h| h.to_ascii_lowercase())
                    };
                    let file = session.as_ref().map(|s| (s.torrent_id.clone(), s.file_index));
                    if file != last_file {
                        last_file = file.clone();
                        have.set(Vec::new());
                        // Ask for the ranges of a new file on this tick.
                        tick = 0;
                    }
                    let Some(tid) = session.as_ref().map(|s| s.torrent_id.clone()).or(pending) else {
                        if tstats.peek().is_some() {
                            tstats.set(None);
                        }
                        continue;
                    };
                    match backend.torrent_stats(&tid).await {
                        Ok(s) => tstats.set(Some(s)),
                        Err(_) if session.is_none() => tstats.set(None),
                        Err(_) => {}
                    }
                    if let Some((tid, index)) = file
                        && tick % SEEK_MARKS_EVERY == 0
                        && let Ok(ranges) = backend.seek_marks(&tid, index).await
                    {
                        have.set(ranges);
                    }
                }
            }
        }
    });

    // Release the picture (and the PiP window state) when leaving the screen.
    use_drop({
        let win = win.clone();
        let host = host.clone();
        let pip_win = pip_win.clone();
        let external = external.clone();
        let (backend, tslot) = (backend.clone(), tslot.clone());
        move || {
            // An external session never outlives its screen.
            external.set(None);
            // Leaving the player ends its torrent session (after-watch policy), unless the torrent was handed on.
            let session = tslot.session.borrow_mut().take();
            if let Some(s) = session
                && !tslot.handed_over.get()
            {
                end_torrent(&backend, s.torrent_id, tslot.completed());
            }
            if let Some(p) = host.current() {
                p.set_rect(None, 1.0);
                p.stop();
            }
            set_pip(&win, false, &pip_win);
            win.set_fullscreen(false);
            // Leaving the player: back to "browsing" (the shell's route presence then refines it, e.g. a details
            // page sends "viewing").
            let state = if ui.settings.peek().flow.discord_presence { PresenceVm::Browsing } else { PresenceVm::Clear };
            presence::send(&backend, state);
        }
    });

    // Keyboard.
    let mut seen = use_signal(|| ui.shortcut.peek().as_ref().map_or(0, |e| e.seq));
    use_effect({
        let (backend, id, win, ctx, pip_win) = (backend.clone(), id.clone(), win.clone(), ctx.clone(), pip_win.clone());
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
                // Esc closes the shortcut panel / episode list, then leaves full screen, then the player.
                Shortcut::Escape => {
                    if *help_open.peek() {
                        help_open.set(false);
                    } else if *episodes_open.peek() {
                        episodes_open.set(false);
                    } else if *fullscreen.peek() {
                        fullscreen.set(false);
                        win.set_fullscreen(false);
                    } else {
                        report(&backend, &id, episode, &pstate.peek());
                        navigator.go_back();
                    }
                }
                Shortcut::Preset(n) => {
                    if let Some(next) = ShaderPreset::from_shortcut(n) {
                        let saver = backend.clone();
                        let mut preset = preset;
                        choose_preset(&backend, p.clone(), next, notice, move || {
                            preset.set(next);
                            let s = reduce_settings(&ui.settings.peek(), SettingsAction::SetPreset(next));
                            ui.commit_settings(&saver, s);
                        });
                    }
                }
                // Wave A (worker e): the rest of the rebindable player actions (docs/redesign/15 section 4).
                Shortcut::PrevEpisode => ctx.go_prev(),
                Shortcut::NextEpisode => {
                    let total = known_total(meta.peek().as_ref(), details.peek().as_ref().and_then(|d| d.as_ref()));
                    if next_episode(episode, total).is_some() {
                        report(&backend, &id, episode, &pstate.peek());
                        next_since.set(None);
                        ctx.go_next();
                    }
                }
                Shortcut::ToggleMute => {
                    let muted = pstate.peek().muted;
                    let _ = p.set_mute(!muted);
                    dispatch(&mut pstate, PlayerAction::ToggleMute);
                }
                Shortcut::TogglePip => {
                    let on = !*pip.peek();
                    pip.set(on);
                    set_pip(&win, on, &pip_win);
                }
                Shortcut::Screenshot => {
                    let title = known_title(meta.peek().as_ref(), details.peek().as_ref().and_then(|d| d.as_ref()));
                    subs::capture(&ctx, &title, episode);
                }
                Shortcut::VolumeUp | Shortcut::VolumeDown => {
                    let up = evt.action == Shortcut::VolumeUp;
                    let (volume, muted) = {
                        let st = pstate.peek();
                        (st.volume, st.muted)
                    };
                    let v = volume_step(volume, up);
                    let _ = p.set_volume(f64::from(v));
                    dispatch(&mut pstate, PlayerAction::SetVolume(v));
                    if muted && up {
                        let _ = p.set_mute(false);
                        dispatch(&mut pstate, PlayerAction::ToggleMute);
                    }
                    ctx.flash(tf("player.volume_flash", &[("n", &v.to_string())]));
                }
                Shortcut::SpeedUp | Shortcut::SpeedDown => {
                    let next = speed_step(*speed.peek(), evt.action == Shortcut::SpeedUp);
                    speed.set(next);
                    let _ = p.set_speed(next);
                    ctx.flash(format_speed(next));
                }
                Shortcut::CycleSub => {
                    let current = *sub_track.peek();
                    let next = cycle_track(&tracks.peek(), TrackKind::Sub, current, true);
                    if let Some(next) = next {
                        let _ = p.set_sub_track(if next == 0 { None } else { Some(next) });
                        sub_track.set(next);
                        let label = tracks.peek().iter().find(|tr| tr.kind == TrackKind::Sub && tr.id == next).map(track_label);
                        ctx.flash(format!("{}: {}", t("player.subs"), label.unwrap_or_else(|| t("player.off"))));
                    } else {
                        ctx.flash(t("player.no_subs"));
                    }
                }
                Shortcut::CycleAudio => {
                    let current = *audio_track.peek();
                    let next = cycle_track(&tracks.peek(), TrackKind::Audio, current, false);
                    if let Some(next) = next {
                        let _ = p.set_audio_track(Some(next));
                        audio_track.set(next);
                        let label = tracks.peek().iter().find(|tr| tr.kind == TrackKind::Audio && tr.id == next).map(track_label);
                        if let Some(label) = label {
                            ctx.flash(format!("{}: {label}", t("player.audio")));
                        }
                    }
                }
                Shortcut::SubDelayEarlier | Shortcut::SubDelayLater => {
                    let step = if evt.action == Shortcut::SubDelayLater { SUB_DELAY_STEP_SECS } else { -SUB_DELAY_STEP_SECS };
                    let current = p.property_f64("sub-delay").unwrap_or(0.0);
                    let next = ((current + step) * 10.0).round() / 10.0;
                    let _ = p.set_sub_delay(next);
                    ctx.flash(tf("player.sub_delay", &[("ms", &delay_label(next))]));
                }
                Shortcut::SkipSegment => {
                    let position = pstate.peek().position;
                    let target = markers.peek().skip_at(position).map(|(_, end)| end);
                    if let Some(target) = target {
                        let _ = p.seek(target);
                        dispatch(&mut pstate, PlayerAction::Position(target));
                    }
                }
                Shortcut::ToggleLoop => {
                    let on = !*loop_on.peek();
                    loop_on.set(on);
                    let _ = p.set_loop_file(on);
                    ctx.flash(if on { t("player.loop_on") } else { t("player.loop_off") });
                }
                // Global keys: the shell handles them.
                Shortcut::ToggleNav | Shortcut::FocusSearch | Shortcut::NavTo(_) => {}
            }
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
    let flow_prefs = ui.settings.read().flow.clone();
    let sleep_now = sleep();
    let sleep_left = sleep_now.left_secs(now);
    let capture_title = title.clone();
    let visible = st.controls_visible(now) || keeps_controls_up(fs.phase, st.video_ready) || next_since().is_some() || bars_hover();
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
    // `.tk-pc[data-stream-kind]` (docs/redesign/15 section 5, sources.debrid.*): a cached torrent with no engine
    // session played through `Backend::debrid_play` (Flow's debrid branch).
    // Wave B (h + k): a web stream is `local` (a finished download / local-library file, h's `LOCAL_PROVIDER`),
    // `embed` (resolved by the sniffer), `proxy` (through the loopback media proxy) or plain `web`.
    let first_stream = vm.as_ref().and_then(|m| m.streams.first());
    let web_kind = match first_stream {
        Some(st) if st.provider == crate::model::downloads::LOCAL_PROVIDER => "local",
        Some(st) => match crate::model::server::stream_kind(&st.url) {
            "direct" => "web",
            k => k,
        },
        None => "web",
    };
    let stream_kind = match &current {
        _ if torrent.read().is_some() => "torrent",
        Some(s) if s.kind == SourceKindVm::Torrent && s.debrid.is_some_and(|d| d.cached) => "debrid",
        Some(s) if s.kind == SourceKindVm::Torrent => "torrent",
        Some(_) => web_kind,
        None if first_stream.is_some() => web_kind,
        None => "none",
    };
    // Torrent: "Waiting for metadata…" while `torrent_play` adds the torrent, then the speed / peers / file line
    // while mpv waits for data (docs/redesign/10 section 5.5).
    let torrent_now = current.as_ref().is_some_and(|s| s.kind == SourceKindVm::Torrent);
    let session = torrent();
    let stats = tstats();
    let ranges = if session.is_some() { have() } else { Vec::new() };
    let metadata_wait = torrent_now
        && fs.phase == Phase::Resolving
        && stats.as_ref().is_none_or(|s| s.state == TorrentStateVm::Initializing);
    let resolving_label = match &current {
        _ if metadata_wait => t("player.torrent.metadata"),
        Some(s) if !s.ext_name.is_empty() => format!("{} · {}", t("player.resolving"), s.ext_name),
        _ => t("player.resolving"),
    };
    let waiting_data = matches!(fs.phase, Phase::Resolving | Phase::Buffering) || st.buffering;
    let status_line = if torrent_now && waiting_data { torrent_line(stats.as_ref(), &ranges) } else { String::new() };
    let have_spans = range_spans(&ranges);
    let ready = if ranges.is_empty() { None } else { ready_span(&ranges, st.fraction()) };
    // "Open in the torrent client": select it there and go to its Overview (or its Files when no file matched).
    let open_client = {
        let tslot = tslot.clone();
        move |files: bool| {
            let Some(tid) = torrent.peek().as_ref().map(|s| s.torrent_id.clone()).or_else(|| files_hint.peek().clone()) else {
                return;
            };
            // The user wants to look at this torrent: leaving the player must not delete it.
            tslot.handed_over.set(true);
            let mut sel = ui.torrent;
            sel.set(Some(tid));
            if files {
                navigator.push(Route::TorrentFilesPage {});
            } else {
                navigator.push(Route::TorrentOverviewPage {});
            }
        }
    };
    let qualities = fs.qualities();
    let quality = current.as_ref().map(|s| s.quality.clone()).unwrap_or_default();
    let quality_auto = !fs.sources.iter().any(|s| s.picked);
    let hole_open = st.video_ready && failure().is_none() && matches!(fs.phase, Phase::Buffering | Phase::Playing);
    let flash_text = flash().filter(|(_, at)| now.saturating_sub(*at) < FLASH_MS).map(|(text, _)| text);
    let next_secs = u64::from(flow_prefs.auto_next_secs);
    let next_left = next_since().map(|since| countdown_left(since, now, next_secs));
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
        // A torrent whose files had no match for this episode: point at its Files page.
        let pick_file = files_hint().filter(|_| fs.last_failed_torrent());
        let cause = if pick_file.is_some() {
            t("player.torrent.no_file_cause")
        } else if m2 || fs.failed.is_empty() {
            String::new()
        } else {
            t("player.error.no_more")
        };
        let best_web = if m2 || fs.last_failed_torrent() { fs.best_web().map(|s| s.id.clone()) } else { None };
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
                        if pick_file.is_some() {
                            Button {
                                label: t("player.torrent.pick_file"),
                                variant: ButtonVariant::Filled,
                                icon: "magnet".to_string(),
                                onclick: {
                                    let open = open_client.clone();
                                    move |_| open(true)
                                },
                            }
                        }
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
        // External mpv shows the same speed / peers line for the whole session, not only while it waits.
        let ext_torrent_line = if torrent_now { torrent_line(stats.as_ref(), &ranges) } else { String::new() };
        let file_line = session.as_ref().map(|s| tf("player.torrent.file", &[("name", &s.file_name)]));
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
                        if let Some(name) = file_line {
                            p { class: "tk-xmpv__file", title: "{name}", "{name}" }
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
                            if !ext_torrent_line.is_empty() {
                                p { class: "tk-xmpv__torrent",
                                    Icon { name: "download".to_string(), size: 16 }
                                    span { "{ext_torrent_line}" }
                                }
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
                            if session.is_some() {
                                Button {
                                    label: t("player.torrent.open_client"),
                                    variant: ButtonVariant::Tonal,
                                    icon: "magnet".to_string(),
                                    onclick: {
                                        let open = open_client.clone();
                                        move |_| open(false)
                                    },
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
            // A torrent stalls mid-episode while pieces arrive: show the line with the status.
            Phase::Buffering if !status_line.is_empty() => Some(t("player.buffering")),
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
                        div { class: "tk-pc__wait",
                            div { class: "tk-pc__status", role: "status",
                                LoadingIndicator { small: true }
                                span { "{text}" }
                            }
                            if !status_line.is_empty() {
                                span { class: "tk-pc__tline", "{status_line}" }
                            }
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
            // Wave B mount point (worker h, `downloads.player-button`): queues the current episode with the current
            // source. Renders nothing until h lands; `visible` follows the controls.
            crate::components::PlayerDownloadButton {
                media_id: id.clone(),
                episode,
                source_id: fs.current.and_then(|i| fs.sources.get(i)).map(|s| s.id.clone()),
                visible,
            }
            PlayerControls {
                on_bars_hover: move |over: bool| bars_hover.set(over),
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
                has_prev: episode > 1,
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
                        if let Some(p) = player.peek().clone() {
                            let saver = backend.clone();
                            let mut preset = preset;
                            choose_preset(&backend, p, next, notice, move || {
                                preset.set(next);
                                let s = reduce_settings(&ui.settings.peek(), SettingsAction::SetPreset(next));
                                ui.commit_settings(&saver, s);
                            });
                        }
                    }
                },
                on_next: {
                    let ctx = ctx.clone();
                    move |_| ctx.go_next()
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
                    let target = markers.peek().skip_at(position).map(|(_, end)| end);
                    if let Some(target) = target
                        && let Some(p) = player.peek().clone()
                    {
                        let _ = p.seek(target);
                        dispatch(&mut pstate, PlayerAction::Position(target));
                    }
                },
                // The menus switch mpv's track through worker d's `subs::select_sub` / `select_audio`, keep the
                // choice in the UI and confirm it.
                on_sub: {
                    let ctx = ctx.clone();
                    move |sid: i64| {
                        sub_track.set(sid);
                        let p = player.peek().clone();
                        subs::select_sub(p.as_ref(), sid);
                        let label = tracks.peek().iter().find(|tr| tr.kind == TrackKind::Sub && tr.id == sid).map(crate::components::player_controls::track_label);
                        ctx.flash(label.unwrap_or_else(|| t("player.off")));
                    }
                },
                on_audio: {
                    let ctx = ctx.clone();
                    move |aid: i64| {
                        audio_track.set(aid);
                        let p = player.peek().clone();
                        subs::select_audio(p.as_ref(), aid);
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
                        if let Some(p) = player.peek().clone() {
                            let _ = p.set_speed(s);
                        }
                        ctx.flash(format_speed(s));
                    }
                },
                on_smoothing: {
                    let ctx = ctx.clone();
                    move |_| {
                        let on = !*smoothing.peek();
                        smoothing.set(on);
                        if let Some(p) = player.peek().clone() {
                            let _ = p.set_interpolation(on);
                        }
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
                    let (ctx, title) = (ctx.clone(), capture_title.clone());
                    move |_| subs::capture(&ctx, &title, episode)
                },
                on_subtitle_settings: move |_| subs_sheet.set(true),
                have: have_spans,
                ready,
                status_line: status_line.clone(),
                torrent_client: session.is_some(),
                on_torrent_client: {
                    let open = open_client.clone();
                    move |_| open(false)
                },
                on_prev: {
                    let ctx = ctx.clone();
                    move |_| ctx.go_prev()
                },
                on_episodes: move |_| episodes_open.set(true),
                loop_on: loop_on(),
                on_loop: {
                    let ctx = ctx.clone();
                    move |_| {
                        let on = !*loop_on.peek();
                        loop_on.set(on);
                        if let Some(p) = player.peek().clone() {
                            let _ = p.set_loop_file(on);
                        }
                        ctx.flash(if on { t("player.loop_on") } else { t("player.loop_off") });
                    }
                },
                stable_volume: flow_prefs.stable_volume,
                on_stable_volume: {
                    let (ctx, backend) = (ctx.clone(), backend.clone());
                    move |_| {
                        let mut f = ui.settings.peek().flow.clone();
                        f.stable_volume = !f.stable_volume;
                        let on = f.stable_volume;
                        let s = reduce_settings(&ui.settings.peek(), SettingsAction::SetFlow(f));
                        ui.commit_settings(&backend, s);
                        if let Some(p) = player.peek().clone() {
                            let _ = p.set_stable_volume(on);
                        }
                        ctx.flash(if on { t("player.stable_volume_on") } else { t("player.stable_volume_off") });
                    }
                },
                sleep: sleep_now,
                sleep_left,
                on_sleep: {
                    let ctx = ctx.clone();
                    move |c: SleepChoice| {
                        let timer = SleepTimer::from_choice(c, now_ms());
                        sleep.set(timer);
                        let text = match c {
                            SleepChoice::Off => t("player.sleep.cancelled"),
                            SleepChoice::Minutes(m) => tf("player.sleep.set", &[("n", &m.to_string())]),
                            SleepChoice::EndOfEpisode => t("player.sleep.set_end"),
                        };
                        ctx.flash(text);
                    }
                },
                on_help: move |_| help_open.set(true),
                shortcuts: flow_prefs.shortcuts.clone(),
                auto_next: flow_prefs.auto_next,
                auto_next_secs: flow_prefs.auto_next_secs,
                auto_skip: flow_prefs.auto_skip_intro,
                source_picked: fs.has_pick(),
                stream_kind: stream_kind.to_string(),
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
                        let now = now_ms();
                        if now.saturating_sub(*uosc_flashed.peek()) > UOSC_FLASH_EVERY_MS {
                            uosc_flashed.set(now);
                            let _ = p.flash_uosc_controls();
                        }
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
                    total: next_secs,
                    counting: flow_prefs.auto_next,
                    poster: poster.clone(),
                    on_play: {
                        let ctx = ctx.clone();
                        move |_| {
                            next_since.set(None);
                            ctx.go_next();
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
            EpisodeSheet {
                open: episodes_open(),
                id: id.clone(),
                current: episode,
                on_pick: {
                    let ctx = ctx.clone();
                    move |n: u32| {
                        episodes_open.set(false);
                        next_since.set(None);
                        ctx.go_to(n);
                    }
                },
                on_close: move |_| episodes_open.set(false),
            }
            ShortcutHelp {
                open: help_open(),
                shortcuts: flow_prefs.shortcuts.clone(),
                on_close: move |_| help_open.set(false),
            }
            subs::SubtitleSheet {
                open: subs_sheet(),
                player: subs::PlayerHandle(player()),
                tracks: tracks(),
                sub_track: sub_track(),
                on_close: move |_| subs_sheet.set(false),
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
