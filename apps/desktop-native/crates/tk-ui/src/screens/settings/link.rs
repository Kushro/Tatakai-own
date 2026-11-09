//! Settings > Link: server port (restart) and the shortcut to the Link page.
//!
//! Split out of `screens/settings.rs` by the wave-A contract (docs/redesign/15-parity-wave-a-contract.md
//! section 2). Owner: nobody in wave A (wave B worker k). The page shell (`mod.rs`: index, scroll spy, `SettingsPanel`, `SettingRow`) is frozen.

use dioxus::prelude::*;

use super::parse_port;
use crate::app::{Route, Services};
use crate::components::title_bar::ShellStatus;
use crate::components::{Button, ButtonSize, ButtonVariant, ErrorState, SkeletonLines, TextField, use_snackbar};
use crate::i18n::{t, tf};
use crate::model::{ErrorKind, UiError};

#[component]
pub fn LinkSection() -> Element {
    let navigator = use_navigator();
    rsx! {
        p { class: "tk-panel__hint", {t("settings.link.hint")} }
        LinkPortRow {}
        div {
            Button {
                label: t("settings.link.manage"),
                variant: ButtonVariant::Tonal,
                icon: "link".to_string(),
                onclick: move |_| {
                    navigator.push(Route::LinkPage {});
                },
            }
        }
    }
}

/// Link server port: shows the current port; Enter / Save restarts the server there
/// (`Backend::link_restart`). A taken port is reported inline with the readable cause.
#[component]
fn LinkPortRow() -> Element {
    let services = use_context::<Services>();
    let backend = services.backend.clone();
    let shell = try_use_context::<ShellStatus>();
    let snack = use_snackbar();
    let mut status = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.link_status().await }
        }
    });
    let mut draft = use_signal(|| None::<String>);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<UiError>);

    let apply = use_callback({
        let backend = backend.clone();
        move |_: ()| {
            if busy() {
                return;
            }
            let Some(port) = draft().as_deref().and_then(parse_port) else { return };
            busy.set(true);
            error.set(None);
            let backend = backend.clone();
            spawn(async move {
                match backend.link_restart(port).await {
                    Ok(s) => {
                        if let Some(mut sh) = shell {
                            sh.link.set(Some(s.clone()));
                        }
                        if s.running {
                            snack.show(tf("link.status.running", &[("port", &s.port.to_string())]));
                            draft.set(None);
                        } else if let Some(e) = s.error {
                            error.set(Some(e));
                        }
                        status.restart();
                    }
                    Err(e) => error.set(Some(e)),
                }
                busy.set(false);
            });
        }
    });

    let current_port = status.read().as_ref().and_then(|r| r.as_ref().ok()).map(|s| s.port);
    let text = draft().unwrap_or_else(|| current_port.map(|p| p.to_string()).unwrap_or_default());
    let invalid = draft().is_some_and(|d| parse_port(&d).is_none());
    let err = error();
    let cause = match &err {
        Some(e) if e.kind == ErrorKind::AddrInUse => {
            format!("{} {}", tf("link.err.port", &[("port", &text)]), t("link.err.port_fix"))
        }
        _ => String::new(),
    };
    let load_error = status.read().as_ref().and_then(|r| r.as_ref().err()).cloned();

    if status.read().is_none() {
        return rsx! {
            SkeletonLines { widths: vec![30] }
        };
    }
    if let Some(e) = load_error {
        return rsx! {
            ErrorState { error: e, compact: true, on_retry: move |_| status.restart() }
        };
    }

    rsx! {
        div { class: "tk-settings__port",
            TextField {
                label: t("settings.link.port"),
                value: text,
                mono: true,
                error: invalid,
                hint: if invalid { t("settings.link.port_invalid") } else { t("settings.link.port_hint") },
                disabled: busy(),
                oninput: move |v: String| draft.set(Some(v)),
                onsubmit: move |_| apply.call(()),
            }
            Button {
                label: t("common.save"),
                variant: ButtonVariant::Tonal,
                size: ButtonSize::Sm,
                disabled: busy() || invalid || draft().is_none(),
                onclick: move |_| apply.call(()),
            }
        }
        if let Some(e) = err {
            ErrorState { error: e, compact: true, retry: false, cause }
        }
    }
}
