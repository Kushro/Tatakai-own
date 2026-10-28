//! `state.rs`: empty, error and skeleton states. Skeletons fade in after 150 ms (pure CSS delay).

use dioxus::prelude::*;

use crate::components::button::{Button, ButtonVariant};
use crate::i18n::t;
use crate::icons::Icon;
use crate::model::UiError;

/// Empty state with a morphing blob behind the icon. `children` adds extra actions.
#[component]
pub fn EmptyState(
    icon: String,
    title: String,
    #[props(default)] body: String,
    #[props(default)] action_label: String,
    #[props(default)] on_action: EventHandler<()>,
    children: Element,
) -> Element {
    rsx! {
        div { class: "tk-state",
            div { class: "tk-state__icon", Icon { name: icon.clone(), size: 36 } }
            h2 { class: "tk-state__title", "{title}" }
            if !body.is_empty() {
                p { class: "tk-state__body", "{body}" }
            }
            div { class: "tk-state__actions",
                if !action_label.is_empty() {
                    Button { label: action_label.clone(), variant: ButtonVariant::Tonal, onclick: move |_| on_action.call(()) }
                }
                {children}
            }
        }
    }
}

/// Inline error (never a toast): localized title, an optional human cause, the technical detail in a
/// collapsible selectable block, Retry and any extra actions passed as children.
#[component]
pub fn ErrorState(
    error: UiError,
    #[props(default)] on_retry: EventHandler<()>,
    /// Hide the Retry button (errors that retrying cannot fix).
    #[props(default = true)] retry: bool,
    /// Overrides the title derived from `error.kind`.
    #[props(default)] title: Option<String>,
    /// One sentence explaining why it happened and what to do ("Another program is using port 47321").
    #[props(default)] cause: String,
    #[props(default)] compact: bool,
    children: Element,
) -> Element {
    let text = title.unwrap_or_else(|| t(error.kind.key()));
    let summary = t("state.error.detail");
    rsx! {
        div { class: "tk-state tk-state--error", "data-compact": "{compact}", role: "alert",
            div { class: "tk-state__icon", Icon { name: "warning".to_string(), size: 24 } }
            div { class: "tk-state__text",
                p { class: "tk-state__title", "{text}" }
                if !cause.is_empty() {
                    p { class: "tk-state__cause", "{cause}" }
                }
                if !error.detail.is_empty() {
                    details { class: "tk-state__details",
                        summary { "{summary}" }
                        code { "{error.detail}" }
                    }
                }
            }
            div { class: "tk-state__actions",
                if retry {
                    Button {
                        label: t("common.retry"),
                        variant: ButtonVariant::Tonal,
                        icon: "refresh".to_string(),
                        onclick: move |_| on_retry.call(()),
                    }
                }
                {children}
            }
        }
    }
}

/// A row of placeholder posters.
#[component]
pub fn SkeletonRow(#[props(default = 8)] count: usize) -> Element {
    rsx! {
        div { class: "tk-skel-row", "aria-hidden": "true",
            for i in 0..count {
                div { key: "{i}", class: "tk-skel tk-skel--card" }
            }
        }
    }
}

#[component]
pub fn SkeletonGrid(#[props(default = 12)] count: usize) -> Element {
    rsx! {
        div { class: "tk-grid", "aria-hidden": "true",
            for i in 0..count {
                div { key: "{i}", class: "tk-skel tk-skel--card" }
            }
        }
    }
}

/// Text placeholder lines (`widths` in percent).
#[component]
pub fn SkeletonLines(#[props(default = vec![80, 60, 40])] widths: Vec<u8>) -> Element {
    rsx! {
        div { class: "tk-list", "aria-hidden": "true",
            for (i , w) in widths.into_iter().enumerate() {
                div { key: "{i}", class: "tk-skel tk-skel--line", style: "width:{w}%" }
            }
        }
    }
}
