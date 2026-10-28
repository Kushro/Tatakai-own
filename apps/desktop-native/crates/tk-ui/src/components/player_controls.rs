//! `player_controls.rs`: the controls are ordinary web UI drawn over the hole (review E5).
//! Play/pause morphs between a circle and a rounded square (CSS `border-radius`).

use dioxus::prelude::*;
use tk_player::ShaderPreset;

use crate::components::button::IconButton;
use crate::components::progress::WavyProgress;
use crate::i18n::{t, tf};
use crate::icons::Icon;
use crate::model::{PlayerState, format_time};

#[component]
pub fn PlayerControls(
    state: PlayerState,
    title: String,
    episode: u32,
    visible: bool,
    preset: ShaderPreset,
    /// e.g. "Source 1 of 3"; empty hides the chip.
    #[props(default)] source_label: String,
    #[props(default)] has_next: bool,
    #[props(default)] on_back: EventHandler<()>,
    #[props(default)] on_toggle_pause: EventHandler<()>,
    /// Fraction 0.0..=1.0 of the duration.
    #[props(default)] on_seek: EventHandler<f64>,
    #[props(default)] on_volume: EventHandler<u8>,
    #[props(default)] on_toggle_mute: EventHandler<()>,
    #[props(default)] on_preset: EventHandler<ShaderPreset>,
    #[props(default)] on_next: EventHandler<()>,
    #[props(default)] on_fullscreen: EventHandler<()>,
) -> Element {
    let mut menu_open = use_signal(|| false);
    let pos = format_time(state.position);
    let dur = format_time(state.duration);
    let ep = tf("player.episode", &[("n", &episode.to_string())]);
    let seek_value = (state.fraction() * 1000.0).round() as i32;
    let volume_value = if state.muted { 0 } else { state.volume };
    let play_label = if state.paused { t("player.play") } else { t("player.pause") };
    let play_icon = if state.paused { "play" } else { "pause" };
    let mute_label = if state.muted { t("player.unmute") } else { t("player.mute") };
    let mute_icon = if state.muted { "mute" } else { "volume" };
    let seek_label = t("player.seek");
    let volume_label = t("player.volume");
    let menu_title = t("preset.title");
    rsx! {
        div { class: "tk-pc", "data-visible": "{visible}",
            div { class: "tk-pc__top",
                IconButton { icon: "back".to_string(), label: t("player.back"), onclick: move |_| on_back.call(()) }
                div { class: "tk-pc__title",
                    span { "{title}" }
                    span { class: "tk-pc__ep", "{ep}" }
                }
                if !source_label.is_empty() {
                    span { class: "tk-chip", "{source_label}" }
                }
            }
            if state.buffering || state.error.is_some() {
                div { class: "tk-pc__center",
                    if let Some(err) = state.error.clone() {
                        div { class: "tk-pc__error", role: "alert", "{err}" }
                    } else {
                        crate::components::loading::LoadingIndicator { contained: true }
                    }
                }
            }
            div { class: "tk-pc__bottom",
                if state.buffering {
                    WavyProgress { fraction: state.fraction(), label: t("player.buffering") }
                }
                input {
                    class: "tk-pc__seek",
                    r#type: "range",
                    min: "0",
                    max: "1000",
                    value: "{seek_value}",
                    "aria-label": "{seek_label}",
                    style: "--tk-seek:{state.fraction() * 100.0}%",
                    oninput: move |e| {
                        if let Ok(v) = e.value().parse::<f64>() {
                            on_seek.call(v / 1000.0);
                        }
                    },
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
                    div { class: "tk-pc__menu-wrap",
                        IconButton {
                            icon: "enhance".to_string(),
                            label: t("player.enhance"),
                            toggled: preset != ShaderPreset::Off,
                            onclick: move |_| menu_open.set(!menu_open()),
                        }
                        if menu_open() {
                            div { class: "tk-menu", role: "menu", "aria-label": "{menu_title}",
                                for (i , p) in ShaderPreset::ALL.into_iter().enumerate() {
                                    button {
                                        key: "{p.id()}",
                                        class: "tk-menu__item",
                                        r#type: "button",
                                        role: "menuitemradio",
                                        "aria-checked": "{p == preset}",
                                        "data-selected": "{p == preset}",
                                        onclick: move |_| {
                                            menu_open.set(false);
                                            on_preset.call(p);
                                        },
                                        span { {t(&format!("preset.{}", p.id()))} }
                                        span { class: "tk-menu__hint", "Ctrl+{i + 1}" }
                                    }
                                }
                            }
                        }
                    }
                    IconButton { icon: "fullscreen".to_string(), label: t("player.fullscreen"), onclick: move |_| on_fullscreen.call(()) }
                }
            }
        }
    }
}
