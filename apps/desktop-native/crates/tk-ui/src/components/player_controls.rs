//! `player_controls.rs`: the controls are ordinary web UI drawn over the hole (review E5), following the
//! prototype `player.js`. Play/pause morphs between a circle and a rounded square (CSS `border-radius`).
//! Pure props in, events out: the screen (`screens/player.rs`) owns mpv, the flow state and the backend.

use std::rc::Rc;

use dioxus::prelude::*;
use tk_player::{ShaderPreset, TrackInfo, TrackKind};

use crate::backend::SkipKindVm;
use crate::components::button::{Button, ButtonSize, ButtonVariant, IconButton};
use crate::components::chips::{Pill, Tone};
use crate::components::loading::LoadingIndicator;
use crate::components::menu::{Menu, MenuAlign, MenuItem, MenuPlacement, MenuSeparator};
use crate::components::progress::WavyProgress;
use crate::i18n::{t, tf};
use crate::icons::Icon;
use crate::model::{PlayerState, format_time};
use crate::player_state::{Markers, NEXT_COUNTDOWN_SECS, Phase, SPEEDS, format_speed, hover_fraction};

/// Menu label of an mpv track: its title, else its language, else `#id`.
pub fn track_label(tr: &TrackInfo) -> String {
    match (&tr.title, &tr.lang) {
        (Some(title), Some(lang)) if !title.is_empty() => format!("{title} · {}", lang.to_uppercase()),
        (Some(title), _) if !title.is_empty() => title.clone(),
        (_, Some(lang)) if !lang.is_empty() => lang.to_uppercase(),
        _ => format!("#{}", tr.id),
    }
}

#[component]
pub fn PlayerControls(
    state: PlayerState,
    title: String,
    episode: u32,
    visible: bool,
    preset: ShaderPreset,
    /// Our upscaling presets apply (false in the enhanced mode: mpv-anime-build brings its own shaders).
    #[props(default = true)] presets: bool,
    #[props(default)] phase: Phase,
    /// "Looking for a source · Toko" while resolving.
    #[props(default)] resolving_label: String,
    /// "Source 1 of 3 · Toko · Alpha · 1080p"; empty hides the chip.
    #[props(default)] source_label: String,
    /// `language` (web) / `download` (torrent).
    #[props(default)] source_icon: String,
    /// The chip opens the source picker (false on the legacy path).
    #[props(default)] source_clickable: bool,
    #[props(default)] has_next: bool,
    #[props(default)] markers: Markers,
    /// Poster URL for the seek hover preview; empty = time only.
    #[props(default)] poster: String,
    /// Audio and subtitle tracks from mpv (`PlayerEvent::Tracks`).
    #[props(default)] tracks: Vec<TrackInfo>,
    /// Selected subtitle track id; 0 = off.
    #[props(default)] sub_track: i64,
    #[props(default)] audio_track: i64,
    /// Qualities the source list offers, highest first.
    #[props(default)] qualities: Vec<String>,
    /// Quality of the playing source ("" unknown).
    #[props(default)] quality: String,
    /// The system choice is playing (no manual pick).
    #[props(default)] quality_auto: bool,
    #[props(default = 1.0)] speed: f64,
    #[props(default)] smoothing: bool,
    #[props(default)] pip: bool,
    #[props(default)] on_back: EventHandler<()>,
    #[props(default)] on_toggle_pause: EventHandler<()>,
    /// Fraction 0.0..=1.0 of the duration.
    #[props(default)] on_seek: EventHandler<f64>,
    /// Relative seconds (-5 / +5).
    #[props(default)] on_seek_by: EventHandler<f64>,
    #[props(default)] on_volume: EventHandler<u8>,
    #[props(default)] on_toggle_mute: EventHandler<()>,
    #[props(default)] on_preset: EventHandler<ShaderPreset>,
    #[props(default)] on_next: EventHandler<()>,
    #[props(default)] on_fullscreen: EventHandler<()>,
    /// Source chip / "Pick a source".
    #[props(default)] on_source: EventHandler<()>,
    #[props(default)] on_skip: EventHandler<()>,
    /// Subtitle track id, 0 = off.
    #[props(default)] on_sub: EventHandler<i64>,
    #[props(default)] on_audio: EventHandler<i64>,
    /// `None` = automatic (system choice).
    #[props(default)] on_quality: EventHandler<Option<String>>,
    #[props(default)] on_speed: EventHandler<f64>,
    #[props(default)] on_smoothing: EventHandler<()>,
    #[props(default)] on_pip: EventHandler<()>,
    #[props(default)] on_capture: EventHandler<()>,
    /// "Subtitle style..." entry of the subtitles menu: opens worker d's `screens::player::subs::SubtitleSheet`
    /// (wave A; docs/redesign/15 section 4).
    #[props(default)] on_subtitle_settings: EventHandler<()>,
    /// Torrent playback (docs/redesign/10 section 5.5): downloaded ranges of the file as `(left %, width %)` of the
    /// seek bar (`player_state::range_spans`).
    #[props(default)] have: Vec<(f64, f64)>,
    /// Ready-to-play range from the playhead, `(left %, width %)` (`player_state::ready_span`).
    #[props(default)] ready: Option<(f64, f64)>,
    /// "4.20 MB/s · 12 peers · 37% of the file" while the torrent loads or mpv waits for data; empty hides it.
    #[props(default)] status_line: String,
    /// Show "Open in the torrent client" (a torrent is playing).
    #[props(default)] torrent_client: bool,
    #[props(default)] on_torrent_client: EventHandler<()>,
) -> Element {
    let mut preset_open = use_signal(|| false);
    let mut subs_open = use_signal(|| false);
    let mut audio_open = use_signal(|| false);
    let mut more_open = use_signal(|| false);
    let mut hover = use_signal(|| None::<f64>);
    let mut seek_w = use_signal(|| 0.0_f64);
    let mut seek_el = use_signal(|| None::<Rc<MountedData>>);

    let pos = format_time(state.position);
    let dur = format_time(state.duration);
    let ep = tf("player.episode", &[("n", &episode.to_string())]);
    let fraction = state.fraction();
    let pct = fraction * 100.0;
    let seek_value = (fraction * 1000.0).round() as i32;
    let volume_value = if state.muted { 0 } else { state.volume };
    let play_label = if state.paused { t("player.play") } else { t("player.pause") };
    let play_icon = if state.paused { "play" } else { "pause" };
    let mute_label = if state.muted { t("player.unmute") } else { t("player.mute") };
    let mute_icon = if state.muted || state.volume == 0 { "mute" } else { "volume" };
    let seek_label = t("player.seek");
    let volume_label = t("player.volume");
    let preset_text = t(&format!("preset.{}", preset.id()));
    let marks = markers.bar_marks(state.duration);
    // "Skip intro" (and recap / ending) while the playhead is inside a known mark.
    let skip = (phase == Phase::Playing).then(|| markers.skip_at(state.position)).flatten().map(|(kind, _)| match kind {
        SkipKindVm::Intro => t("player.skip_intro"),
        SkipKindVm::Recap => t("player.torrent.skip_recap"),
        SkipKindVm::Outro => t("player.torrent.skip_outro"),
    });
    let client_label = t("player.torrent.open_client");
    let duration = state.duration;
    let preview = hover().map(|f| (f * 100.0, format_time(f * duration)));
    let subs: Vec<TrackInfo> = tracks.iter().filter(|tr| tr.kind == TrackKind::Sub).cloned().collect();
    let audios: Vec<TrackInfo> = tracks.iter().filter(|tr| tr.kind == TrackKind::Audio).cloned().collect();
    let buffering = phase == Phase::Buffering || (phase == Phase::Playing && state.buffering);
    let more_title = format!("{} · {}", t("player.quality"), t("player.speed"));
    let quality_label = t("player.quality");
    let speed_label = t("player.speed");
    let smoothing_hint = t("player.smoothing_hint");
    let src_title = t("src.title");
    rsx! {
        div { class: "tk-pc", "data-visible": "{visible}",
            div { class: "tk-pc__top",
                IconButton { icon: "back".to_string(), label: t("player.back"), onclick: move |_| on_back.call(()) }
                div { class: "tk-pc__title",
                    span { class: "tk-pc__name", "{title}" }
                    div { class: "tk-pc__ep",
                        span { "{ep}" }
                        if !source_label.is_empty() {
                            if source_clickable {
                                button {
                                    class: "tk-chip tk-pc__src",
                                    r#type: "button",
                                    title: "{src_title}",
                                    "aria-haspopup": "dialog",
                                    onclick: move |_| on_source.call(()),
                                    Icon { name: source_icon.clone(), size: 16 }
                                    span { "{source_label}" }
                                    Icon { name: "chevron-down".to_string(), size: 16 }
                                }
                            } else {
                                span { class: "tk-chip tk-pc__src",
                                    Icon { name: source_icon.clone(), size: 16 }
                                    span { "{source_label}" }
                                }
                            }
                        }
                        if presets && preset != ShaderPreset::Off {
                            Pill { text: preset_text, tone: Tone::Primary, plain: true }
                        }
                    }
                }
                if torrent_client {
                    IconButton { icon: "magnet".to_string(), label: client_label, onclick: move |_| on_torrent_client.call(()) }
                }
                Menu {
                    open: more_open(),
                    label: more_title.clone(),
                    title: more_title.clone(),
                    on_close: move |_| more_open.set(false),
                    trigger: rsx! {
                        IconButton { icon: "more".to_string(), label: t("common.more"), onclick: move |_| more_open.toggle() }
                    },
                    MenuItem {
                        label: format!("{quality_label}: {}", t("player.auto")),
                        icon: "hd".to_string(),
                        checked: Some(quality_auto),
                        on_select: move |_| {
                            more_open.set(false);
                            on_quality.call(None);
                        },
                    }
                    for (q , qv) in qualities.iter().map(|q| (q.clone(), q.clone())) {
                        MenuItem {
                            key: "{q}",
                            label: format!("{quality_label}: {q}"),
                            icon: "hd".to_string(),
                            checked: Some(!quality_auto && q == quality),
                            on_select: move |_| {
                                more_open.set(false);
                                on_quality.call(Some(qv.clone()));
                            },
                        }
                    }
                    MenuSeparator {}
                    for s in SPEEDS {
                        MenuItem {
                            key: "{s}",
                            label: format!("{speed_label} {}", format_speed(s)),
                            icon: "speed".to_string(),
                            checked: Some(s == speed),
                            on_select: move |_| {
                                more_open.set(false);
                                on_speed.call(s);
                            },
                        }
                    }
                    MenuSeparator {}
                    MenuItem {
                        label: t("player.smoothing"),
                        icon: "bolt".to_string(),
                        hint: smoothing_hint.clone(),
                        checked: Some(smoothing),
                        on_select: move |_| {
                            more_open.set(false);
                            on_smoothing.call(());
                        },
                    }
                }
            }
            div { class: "tk-pc__center",
                {match phase {
                    Phase::Resolving => rsx! {
                        div { class: "tk-pc__wait",
                            div { class: "tk-pc__status", role: "status",
                                LoadingIndicator { small: true }
                                span { "{resolving_label}" }
                            }
                            if !status_line.is_empty() {
                                span { class: "tk-pc__tline", "{status_line}" }
                            }
                        }
                    },
                    Phase::Manual => rsx! {
                        Button {
                            label: t("src.choose_first"),
                            icon: "tune".to_string(),
                            variant: ButtonVariant::Tonal,
                            class: "tk-pc__choose".to_string(),
                            onclick: move |_| on_source.call(()),
                        }
                    },
                    _ if buffering => rsx! {
                        div { class: "tk-pc__wait",
                            LoadingIndicator { contained: true }
                            if !status_line.is_empty() {
                                span { class: "tk-pc__tline", role: "status", "{status_line}" }
                            }
                        }
                    },
                    _ => rsx! {},
                }}
            }
            if let Some(skip_label) = skip {
                Button {
                    label: skip_label,
                    icon: "skip".to_string(),
                    variant: ButtonVariant::Tonal,
                    size: ButtonSize::Sm,
                    class: "tk-pc__skip".to_string(),
                    onclick: move |_| on_skip.call(()),
                }
            }
            div { class: "tk-pc__bottom",
                if buffering {
                    WavyProgress { fraction: 1.0, label: t("player.buffering"), thin: true }
                }
                div { class: "tk-pc__seek",
                    div { class: "tk-pc__track",
                        // Torrent: downloaded ranges under the played part, then the ready-to-play run ahead.
                        if !have.is_empty() {
                            div { class: "tk-pc__have",
                                for (i , (left , width)) in have.iter().copied().enumerate() {
                                    span { key: "{i}", style: "left:{left}%;width:{width}%" }
                                }
                            }
                        }
                        if let Some((left, width)) = ready {
                            div { class: "tk-pc__ready", style: "left:{left}%;width:{width}%" }
                        }
                        div { class: "tk-pc__played", style: "--p:{pct}%" }
                        div { class: "tk-pc__markers",
                            for (i , (kind , left , width)) in marks.into_iter().map(|m| (m.kind.id(), m.left, m.width)).enumerate() {
                                span {
                                    key: "{i}",
                                    class: "tk-pc__marker",
                                    "data-kind": kind,
                                    style: "left:{left}%;width:{width}%",
                                }
                            }
                        }
                        div { class: "tk-pc__thumb", style: "--p:{pct}%" }
                    }
                    input {
                        class: "tk-pc__range",
                        r#type: "range",
                        min: "0",
                        max: "1000",
                        value: "{seek_value}",
                        "aria-label": "{seek_label}",
                        onmounted: move |e| seek_el.set(Some(e.data())),
                        onmouseenter: move |_| async move {
                            let el = seek_el.peek().clone();
                            if let Some(el) = el
                                && let Ok(r) = el.get_client_rect().await
                            {
                                seek_w.set(r.size.width);
                            }
                        },
                        onmousemove: move |e| {
                            hover.set(Some(hover_fraction(e.element_coordinates().x, *seek_w.peek())));
                        },
                        onmouseleave: move |_| hover.set(None),
                        oninput: move |e| {
                            if let Ok(v) = e.value().parse::<f64>() {
                                on_seek.call(v / 1000.0);
                            }
                        },
                    }
                    if let Some((x, at)) = preview {
                        div { class: "tk-pc__preview", style: "--x:{x}%", "aria-hidden": "true",
                            if !poster.is_empty() {
                                img { src: "{poster}", alt: "" }
                            }
                            span { "{at}" }
                        }
                    }
                }
                div { class: "tk-pc__row",
                    button {
                        class: "tk-pc__play",
                        r#type: "button",
                        "data-paused": "{state.paused}",
                        "aria-label": "{play_label}",
                        title: "{play_label}",
                        onclick: move |_| on_toggle_pause.call(()),
                        Icon { name: play_icon.to_string(), size: 28 }
                    }
                    IconButton { icon: "replay".to_string(), label: t("player.replay"), onclick: move |_| on_seek_by.call(-5.0) }
                    IconButton { icon: "forward".to_string(), label: t("player.forward"), onclick: move |_| on_seek_by.call(5.0) }
                    if has_next {
                        IconButton { icon: "next".to_string(), label: t("player.next"), onclick: move |_| on_next.call(()) }
                    }
                    span { class: "tk-pc__time", "{pos} / {dur}" }
                    div { class: "tk-pc__spacer" }
                    IconButton { icon: mute_icon.to_string(), label: mute_label, onclick: move |_| on_toggle_mute.call(()) }
                    input {
                        class: "tk-pc__volume",
                        r#type: "range",
                        min: "0",
                        max: "100",
                        value: "{volume_value}",
                        "aria-label": "{volume_label}",
                        oninput: move |e| {
                            if let Ok(v) = e.value().parse::<u8>() {
                                on_volume.call(v);
                            }
                        },
                    }
                    Menu {
                        open: subs_open(),
                        label: t("player.subs"),
                        title: t("player.subs"),
                        placement: MenuPlacement::Above,
                        align: MenuAlign::End,
                        on_close: move |_| subs_open.set(false),
                        trigger: rsx! {
                            IconButton { icon: "cc".to_string(), label: t("player.subs"), toggled: sub_track != 0, onclick: move |_| subs_open.toggle() }
                        },
                        MenuItem {
                            label: t("player.off"),
                            icon: "close".to_string(),
                            checked: Some(sub_track == 0),
                            on_select: move |_| {
                                subs_open.set(false);
                                on_sub.call(0);
                            },
                        }
                        for tr in subs.into_iter() {
                            MenuItem {
                                key: "{tr.id}",
                                label: track_label(&tr),
                                checked: Some(tr.id == sub_track),
                                on_select: move |_| {
                                    subs_open.set(false);
                                    on_sub.call(tr.id);
                                },
                            }
                        }
                        MenuSeparator {}
                        MenuItem {
                            label: t("player.subtitle_style"),
                            icon: "tune".to_string(),
                            on_select: move |_| {
                                subs_open.set(false);
                                on_subtitle_settings.call(());
                            },
                        }
                    }
                    Menu {
                        open: audio_open(),
                        label: t("player.audio"),
                        title: t("player.audio"),
                        placement: MenuPlacement::Above,
                        align: MenuAlign::End,
                        on_close: move |_| audio_open.set(false),
                        trigger: rsx! {
                            IconButton { icon: "audio".to_string(), label: t("player.audio"), onclick: move |_| audio_open.toggle() }
                        },
                        if audios.is_empty() {
                            MenuItem { label: t("player.auto"), icon: "audio".to_string(), checked: Some(true), disabled: true }
                        }
                        for tr in audios.into_iter() {
                            MenuItem {
                                key: "{tr.id}",
                                label: track_label(&tr),
                                checked: Some(tr.id == audio_track),
                                on_select: move |_| {
                                    audio_open.set(false);
                                    on_audio.call(tr.id);
                                },
                            }
                        }
                    }
                    if presets {
                        Menu {
                            open: preset_open(),
                            label: t("preset.title"),
                            title: t("preset.title"),
                            placement: MenuPlacement::Above,
                            align: MenuAlign::End,
                            on_close: move |_| preset_open.set(false),
                            trigger: rsx! {
                                IconButton {
                                    icon: "enhance".to_string(),
                                    label: t("player.enhance"),
                                    toggled: preset != ShaderPreset::Off,
                                    onclick: move |_| preset_open.toggle(),
                                }
                            },
                            for (i , p) in ShaderPreset::ALL.into_iter().enumerate() {
                                MenuItem {
                                    key: "{p.id()}",
                                    label: t(&format!("preset.{}", p.id())),
                                    hint: format!("Ctrl+{}", i + 1),
                                    checked: Some(p == preset),
                                    on_select: move |_| {
                                        preset_open.set(false);
                                        on_preset.call(p);
                                    },
                                }
                            }
                        }
                    }
                    IconButton { icon: "pip".to_string(), label: t("player.pip"), toggled: pip, toggle: true, onclick: move |_| on_pip.call(()) }
                    IconButton { icon: "camera".to_string(), label: t("player.capture"), onclick: move |_| on_capture.call(()) }
                    IconButton { icon: "fullscreen".to_string(), label: t("player.fullscreen"), onclick: move |_| on_fullscreen.call(()) }
                }
                div { class: "tk-pc__shortcuts", {t("player.shortcuts")} }
            }
        }
    }
}

/// End-of-episode card: poster, "Next episode · Episode n" and a ring counting down `left` seconds.
/// The ring plays now; the close button cancels.
#[component]
pub fn NextEpisodeCard(
    episode: u32,
    left: u64,
    #[props(default)] poster: String,
    #[props(default)] on_play: EventHandler<()>,
    #[props(default)] on_cancel: EventHandler<()>,
) -> Element {
    let ep = tf("player.episode", &[("n", &episode.to_string())]);
    let ring = left.min(NEXT_COUNTDOWN_SECS) * 100 / NEXT_COUNTDOWN_SECS;
    let ring_label = tf("player.next_in", &[("s", &left.to_string())]);
    rsx! {
        div { class: "tk-pc__next", role: "status",
            div { class: "tk-pc__next-art",
                if poster.is_empty() {
                    Icon { name: "film".to_string(), size: 28 }
                } else {
                    img { src: "{poster}", alt: "" }
                }
            }
            div { class: "tk-pc__next-text",
                b { {t("player.next")} }
                small { "{ep}" }
            }
            button {
                class: "tk-pc__ring",
                r#type: "button",
                style: "--t:{ring}",
                "aria-label": "{ring_label}",
                title: "{ring_label}",
                onclick: move |_| on_play.call(()),
                i { "{left}" }
            }
            IconButton { icon: "close".to_string(), label: t("common.cancel"), onclick: move |_| on_cancel.call(()) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_labels_prefer_title_then_language() {
        let tr = |title: Option<&str>, lang: Option<&str>| TrackInfo {
            id: 2,
            kind: TrackKind::Sub,
            lang: lang.map(str::to_owned),
            title: title.map(str::to_owned),
            selected: false,
        };
        assert_eq!(track_label(&tr(Some("Toko"), Some("es"))), "Toko · ES");
        assert_eq!(track_label(&tr(Some("Toko"), None)), "Toko");
        assert_eq!(track_label(&tr(None, Some("ja"))), "JA");
        assert_eq!(track_label(&tr(None, None)), "#2");
    }
}
