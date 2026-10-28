//! `dialog.rs`: modal confirmation with scrim. Esc and a scrim click cancel; focus moves into the box.

use dioxus::prelude::*;

use crate::components::button::{Button, ButtonVariant};
use crate::i18n::t;
use crate::icons::Icon;

#[component]
pub fn ConfirmDialog(
    title: String,
    body: String,
    confirm_label: String,
    /// Optional hero icon above the title (centers title and body).
    #[props(default)] icon: String,
    /// Destructive action: error-colored icon and confirm button. Defaults to true (the original use).
    #[props(default = true)] danger: bool,
    /// 640px instead of 420px (forms, lists).
    #[props(default)] wide: bool,
    #[props(default)] cancel_label: Option<String>,
    #[props(default)] confirm_disabled: bool,
    #[props(default)] on_confirm: EventHandler<()>,
    #[props(default)] on_cancel: EventHandler<()>,
    /// Extra content between the body and the actions (a field, a list...).
    children: Element,
) -> Element {
    let cancel = cancel_label.unwrap_or_else(|| t("common.cancel"));
    let confirm_variant = if danger { ButtonVariant::Danger } else { ButtonVariant::Filled };
    let has_icon = !icon.is_empty();
    rsx! {
        div { class: "tk-scrim", onclick: move |_| on_cancel.call(()),
            div {
                class: "tk-dialog",
                role: "alertdialog",
                "aria-modal": "true",
                "aria-label": "{title}",
                tabindex: "-1",
                "data-danger": "{danger}",
                "data-wide": "{wide}",
                "data-icon": "{has_icon}",
                onmounted: move |e| async move {
                    let _ = e.set_focus(true).await;
                },
                onclick: move |e| e.stop_propagation(),
                onkeydown: move |e| {
                    if e.key() == Key::Escape {
                        e.stop_propagation();
                        on_cancel.call(());
                    }
                },
                if has_icon {
                    div { class: "tk-dialog__icon", Icon { name: icon.clone(), size: 24 } }
                }
                h2 { class: "tk-dialog__title", "{title}" }
                if !body.is_empty() {
                    p { class: "tk-dialog__body", "{body}" }
                }
                {children}
                div { class: "tk-dialog__actions",
                    Button { label: cancel, variant: ButtonVariant::Text, onclick: move |_| on_cancel.call(()) }
                    Button { label: confirm_label, variant: confirm_variant, disabled: confirm_disabled, onclick: move |_| on_confirm.call(()) }
                }
            }
        }
    }
}
