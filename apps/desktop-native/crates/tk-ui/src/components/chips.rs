//! `chips.rs`: filter / assist chips, info chips (metadata), status pills, inline banners, key caps.

use dioxus::prelude::*;

use crate::icons::Icon;

/// Status tone. M3 has no success/warning roles: `Ok` uses primary containers and `Warn` tertiary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tone {
    #[default]
    Neutral,
    Ok,
    Warn,
    Err,
    Primary,
}

impl Tone {
    pub fn css(self) -> &'static str {
        match self {
            Self::Neutral => "neutral",
            Self::Ok => "ok",
            Self::Warn => "warn",
            Self::Err => "err",
            Self::Primary => "primary",
        }
    }
}

/// Small uppercase status label with a dot (`plain` hides the dot).
#[component]
pub fn Pill(text: String, #[props(default)] tone: Tone, #[props(default)] plain: bool) -> Element {
    let plain = if plain { "tk-pill--plain" } else { "" };
    rsx! { span { class: "tk-pill tk-pill--{tone.css()} {plain}", "{text}" } }
}

/// Non-interactive metadata chip ("TV · 2024", "8.4").
#[component]
pub fn InfoChip(text: String, #[props(default)] icon: String) -> Element {
    rsx! {
        span { class: "tk-chip tk-chip--info",
            if !icon.is_empty() {
                Icon { name: icon.clone(), size: 16 }
            }
            "{text}"
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChipKind {
    /// Toggles a filter (`on` -> aria-pressed, pill shape, check mark).
    #[default]
    Filter,
    /// Starts an action ("Copy code", "Open folder").
    Assist,
}

#[component]
pub fn Chip(
    label: String,
    #[props(default)] kind: ChipKind,
    #[props(default)] icon: String,
    #[props(default)] on: bool,
    #[props(default)] disabled: bool,
    #[props(default)] onclick: EventHandler<MouseEvent>,
    /// When set, a trailing "x" removes the chip; its accessible name is `dismiss_label`.
    #[props(default)] on_dismiss: Option<EventHandler<()>>,
    #[props(default)] dismiss_label: String,
) -> Element {
    let (kind_css, pressed) = match kind {
        ChipKind::Filter => ("", Some(if on { "true" } else { "false" })),
        ChipKind::Assist => ("tk-chip--assist", None),
    };
    let show_check = kind == ChipKind::Filter && on && icon.is_empty();
    rsx! {
        button {
            class: "tk-chip {kind_css}",
            r#type: "button",
            "aria-pressed": pressed,
            "data-on": "{on}",
            disabled,
            onclick: move |e| onclick.call(e),
            if show_check {
                Icon { name: "check".to_string(), size: 18 }
            } else if !icon.is_empty() {
                Icon { name: icon.clone(), size: 18 }
            }
            span { "{label}" }
            if let Some(dismiss) = on_dismiss {
                span {
                    class: "tk-chip__x",
                    role: "button",
                    tabindex: "0",
                    "aria-label": "{dismiss_label}",
                    title: "{dismiss_label}",
                    onclick: move |e| {
                        e.stop_propagation();
                        dismiss.call(());
                    },
                    onkeydown: move |e| {
                        if e.key() == Key::Enter {
                            e.stop_propagation();
                            dismiss.call(());
                        }
                    },
                    Icon { name: "close".to_string(), size: 14 }
                }
            }
        }
    }
}

/// Inline banner: icon, text and optional actions (children). Errors that need attention use this or
/// `ErrorState`, never a toast.
#[component]
pub fn Banner(
    text: String,
    #[props(default)] icon: String,
    #[props(default)] tone: Tone,
    children: Element,
) -> Element {
    let role = if tone == Tone::Err { "alert" } else { "status" };
    rsx! {
        div { class: "tk-banner tk-banner--{tone.css()}", role,
            if !icon.is_empty() {
                Icon { name: icon.clone(), size: 22 }
            }
            span { class: "tk-banner__text", "{text}" }
            div { class: "tk-banner__actions", {children} }
        }
    }
}

/// Keyboard key cap (`Ctrl`, `K`).
#[component]
pub fn Kbd(keys: String) -> Element {
    rsx! { kbd { class: "tk-kbd", "{keys}" } }
}
