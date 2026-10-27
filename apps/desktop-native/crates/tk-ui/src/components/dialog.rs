//! `dialog.rs`: destructive-action confirmation with scrim.

use dioxus::prelude::*;

use crate::components::button::{Button, ButtonVariant};
use crate::i18n::t;

#[component]
pub fn ConfirmDialog(
    title: String,
    body: String,
    confirm_label: String,
    #[props(default)] on_confirm: EventHandler<()>,
    #[props(default)] on_cancel: EventHandler<()>,
) -> Element {
    rsx! {
        div { class: "tk-scrim", onclick: move |_| on_cancel.call(()),
            div {
                class: "tk-dialog",
                role: "alertdialog",
                "aria-modal": "true",
                "aria-label": "{title}",
                onclick: move |e| e.stop_propagation(),
                h2 { class: "tk-dialog__title", "{title}" }
                p { class: "tk-dialog__body", "{body}" }
                div { class: "tk-dialog__actions",
                    Button { label: t("common.cancel"), variant: ButtonVariant::Text, onclick: move |_| on_cancel.call(()) }
                    Button { label: confirm_label, variant: ButtonVariant::Filled, class: "tk-btn--danger", onclick: move |_| on_confirm.call(()) }
                }
            }
        }
    }
}
