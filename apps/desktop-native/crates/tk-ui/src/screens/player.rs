//! Player screen (review E5). mpv draws into a child window under the transparent web view; this page
//! leaves a hole (`#tk-video-hole`) whose rect is reported to `tk-player` and draws the controls over it.
//! The hole only becomes transparent once mpv reports `vo-configured`; until then it is an opaque scrim.

use std::sync::Arc;
use std::time::Duration;

use dioxus::desktop::use_window;
use dioxus::prelude::*;
use tk_player::{MediaSource, Player, PlayerError, PlayerEvent, Rect, ShaderPreset, SubtitleSource};
use tokio::sync::broadcast::error::RecvError;

use crate::app::{Route, Services, Ui};
use crate::backend::{BackendRef, PlaybackVm};
use crate::components::{EmptyState, ErrorState, PlayerControls};
use crate::i18n::{t, tf};
use crate::model::{
    ErrorKind, PlayerAction, PlayerState, SEEK_STEP_SECS, SettingsAction, UiError, next_episode,
    reduce_player, reduce_settings, should_report,
};
use crate::platform::{self, now_ms};
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

/// At most this many sources are tried per episode.
const MAX_SOURCES: usize = 3;

fn dispatch(sig: &mut Signal<PlayerState>, action: PlayerAction) {
    let next = reduce_player(&sig.peek(), action);
    sig.set(next);
}

fn load_stream(p: &Player, vm: &PlaybackVm, idx: usize, start: f64) -> Result<(), PlayerError> {
    let s = &vm.streams[idx];
    let src = MediaSource {
        url: s.url.clone(),
        headers: s.headers.clone(),
        subtitles: s
            .subtitles
            .iter()
            .map(|x| SubtitleSource { url: x.url.clone(), lang: x.lang.clone(), label: x.label.clone() })
            .collect(),
    };
    p.load(&src, start)
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

#[component]
pub fn PlayerPage(id: String, episode: u32) -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let navigator = use_navigator();
    let win = use_window();
    let backend = services.backend.clone();
    let host = services.player.clone();

    let mut pstate = use_signal(PlayerState::default);
    let mut meta = use_signal(|| None::<PlaybackVm>);
    let mut source_idx = use_signal(|| 0_usize);
    let mut failure = use_signal(|| None::<UiError>);
    let mut notice = use_signal(|| None::<String>);
    let mut player = use_signal(|| None::<Arc<Player>>);
    let mut preset = use_signal(|| ui.settings.peek().preset());
    let mut fullscreen = use_signal(|| false);
    let mut clock = use_signal(now_ms);

    // Resolve streams, start mpv and pump its events.
    use_future({
        let (backend, host, id) = (backend.clone(), host.clone(), id.clone());
        move || {
            let (backend, host, id) = (backend.clone(), host.clone(), id.clone());
            async move {
                dispatch(&mut pstate, PlayerAction::Loading);
                dispatch(&mut pstate, PlayerAction::Activity(now_ms()));
                let vm = match backend.playback(&id, episode).await {
                    Ok(v) if v.streams.is_empty() => {
                        failure.set(Some(UiError::new(ErrorKind::NoSource, "")));
                        return;
                    }
                    Ok(v) => v,
                    Err(e) => {
                        failure.set(Some(e));
                        return;
                    }
                };
                meta.set(Some(vm.clone()));

                let p = match host.get_or_init(platform::main_hwnd()) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::error!(error = %e, "player start failed");
                        dispatch(&mut pstate, PlayerAction::Error(t("player.error.no_player")));
                        return;
                    }
                };
                player.set(Some(p.clone()));
                let mut rx = p.events();

                // Hole geometry -> child window.
                spawn({
                    let p = p.clone();
                    async move {
                        let mut eval = document::eval(RECT_JS);
                        while let Ok(v) = eval.recv::<[f64; 5]>().await {
                            p.set_rect(Some(Rect { x: v[0], y: v[1], w: v[2], h: v[3] }), v[4]);
                        }
                    }
                });

                let settings = ui.settings.peek().clone();
                let _ = p.set_volume(f64::from(settings.volume));
                dispatch(&mut pstate, PlayerAction::SetVolume(settings.volume));
                let initial = settings.preset();
                if initial != ShaderPreset::Off {
                    apply_preset(&p, initial, &mut notice);
                }
                if let Err(e) = load_stream(&p, &vm, 0, f64::from(vm.start_secs)) {
                    tracing::warn!(error = %e, "loading the first source failed");
                    dispatch(&mut pstate, PlayerAction::Error(e.to_string()));
                }

                let mut last_report = now_ms();
                let mut last_pos = -1.0_f64;
                loop {
                    let ev = match rx.recv().await {
                        Ok(ev) => ev,
                        Err(RecvError::Lagged(_)) => continue,
                        Err(RecvError::Closed) => break,
                    };
                    match ev {
                        PlayerEvent::Position(pos) => {
                            // mpv reports per frame; the UI needs a few updates per second.
                            if (pos - last_pos).abs() >= 0.25 {
                                last_pos = pos;
                                dispatch(&mut pstate, PlayerAction::Position(pos));
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
                        PlayerEvent::Buffering(b) => dispatch(&mut pstate, PlayerAction::Buffering(b)),
                        PlayerEvent::VideoReady(b) => dispatch(&mut pstate, PlayerAction::VideoReady(b)),
                        PlayerEvent::Tracks(_) => {}
                        PlayerEvent::Eof => {
                            dispatch(&mut pstate, PlayerAction::Eof);
                            let mut done = pstate.peek().clone();
                            done.position = done.duration.max(done.position);
                            report(&backend, &id, episode, &done);
                            if let Some(n) = next_episode(episode, vm.episodes_total) {
                                navigator.push(Route::PlayerPage { id: id.clone(), episode: n });
                            }
                            break;
                        }
                        PlayerEvent::Error(msg) => {
                            tracing::warn!(%msg, "playback error");
                            let next = source_idx() + 1;
                            if next < vm.streams.len().min(MAX_SOURCES) {
                                source_idx.set(next);
                                notice.set(Some(t("player.error.trying_next")));
                                let resume = pstate.peek().position;
                                dispatch(&mut pstate, PlayerAction::Loading);
                                if let Err(e) = load_stream(&p, &vm, next, resume) {
                                    dispatch(&mut pstate, PlayerAction::Error(e.to_string()));
                                }
                            } else {
                                dispatch(&mut pstate, PlayerAction::Error(t("player.error.no_more")));
                            }
                        }
                    }
                }
            }
        }
    });

    // Controls auto-hide clock.
    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_millis(500)).await;
            clock.set(now_ms());
        }
    });

    // Release the picture when leaving the screen.
    use_drop({
        let win = win.clone();
        let host = host.clone();
        move || {
            if let Some(p) = host.current() {
                p.set_rect(None, 1.0);
                p.stop();
            }
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
                Shortcut::ToggleNav | Shortcut::FocusSearch => {}
            }
            let _ = &id;
        }
    });

    let st = pstate();
    let now = clock();
    let visible = st.controls_visible(now);
    let title = meta().map(|m| m.title).unwrap_or_default();
    let total_sources = meta().map_or(0, |m| m.streams.len().min(MAX_SOURCES));
    let source_label = if total_sources > 1 {
        tf("player.source", &[("n", &(source_idx() + 1).to_string()), ("total", &total_sources.to_string())])
    } else {
        String::new()
    };
    let has_next = next_episode(episode, meta().and_then(|m| m.episodes_total)).is_some();
    let hole_open = st.video_ready && failure().is_none();

    let go_back = {
        let (backend, id) = (backend.clone(), id.clone());
        move |_| {
            report(&backend, &id, episode, &pstate.peek());
            navigator.go_back();
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
            div { id: "tk-video-hole", class: "tk-hole", "data-open": "{hole_open}" }
            if let Some(err) = failure() {
                div { class: "tk-player__fail",
                    if err.kind == ErrorKind::NoSource {
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
                        ErrorState { error: err.clone() }
                    }
                    crate::components::Button {
                        label: t("common.back"),
                        variant: crate::components::ButtonVariant::Text,
                        icon: "back".to_string(),
                        onclick: move |_| {
                            navigator.go_back();
                        },
                    }
                }
            } else {
                PlayerControls {
                    state: st.clone(),
                    title,
                    episode,
                    visible,
                    preset: preset(),
                    source_label,
                    has_next,
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
                            if let Some(n) = next_episode(episode, meta.peek().as_ref().and_then(|m| m.episodes_total)) {
                                navigator.push(Route::PlayerPage { id: id.clone(), episode: n });
                            }
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
                }
            }
            if let Some(n) = notice() {
                div { class: "tk-toast", role: "status", "{n}" }
            }
        }
    }
}
