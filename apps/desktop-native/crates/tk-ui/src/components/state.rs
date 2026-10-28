//! `state.rs`: empty, error and skeleton states. Skeletons fade in after 150 ms (pure CSS delay).

use dioxus::prelude::*;

use crate::components::button::{Button, ButtonVariant};
use crate::i18n::t;
use crate::icons::Icon;
use crate::model::UiError;

#[component]
pub fn EmptyState(
    icon: String,
    title: String,
    #[props(default)] body: String,
    #[props(default)] action_label: String,
    #[props(default)] on_action: EventHandler<()>,
) -> Element {
    rsx! {
        div { class: "tk-state",
            div { class: "tk-state__icon", Icon { name: icon.clone(), size: 40 } }
            h2 { class: "tk-state__title", "{title}" }
            if !body.is_empty() {
                p { class: "tk-state__body", "{body}" }
            }
            if !action_label.is_empty() {
                Button { label: action_label.clone(), variant: ButtonVariant::Tonal, onclick: move |_| on_action.call(()) }
            }
        }
    }
}

/// Inline error with the localized cause and a Retry action (never a toast).
#[component]
pub fn ErrorState(
    error: UiError,
    #[props(default)] on_retry: EventHandler<()>,
    #[props(default)] compact: bool,
) -> Element {
    let text = t(error.kind.key());
    let icon_size = if compact { 28 } else { 40 };
    rsx! {
        div { class: "tk-state tk-state--error", "data-compact": "{compact}", role: "alert",
            div { class: "tk-state__icon", Icon { name: "shield".to_string(), size: icon_size } }
            p { class: "tk-state__title", "{text}" }
            if !error.detail.is_empty() {
                p { class: "tk-state__detail", title: t("state.error.detail"), "{error.detail}" }
            }
            Button {
                label: t("common.retry"),
                variant: ButtonVariant::Tonal,
                icon: "refresh".to_string(),
                onclick: move |_| on_retry.call(()),
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
