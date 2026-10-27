//! Tatakai Link: pairing (QR + code + expiry countdown), paired devices and the firewall flow.

use std::time::Duration;

use dioxus::prelude::*;

use crate::app::Services;
use crate::backend::DeviceVm;
use crate::components::{Button, ButtonVariant, EmptyState, ErrorState, WavyProgress};
use crate::i18n::{t, tf};
use crate::model::{Pairing, UiError, format_code, format_countdown, format_unix_utc};
use crate::platform::now_unix;
use crate::qr::qr_svg;

/// The pairing window is 5 minutes (docs/redesign/03 section 2); used only to scale the progress bar.
const WINDOW_SECS: f64 = 300.0;

#[component]
pub fn LinkPage() -> Element {
    let services = use_context::<Services>();
    let backend = services.backend.clone();

    let mut status = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.link_status().await }
        }
    });
    let mut pairing = use_signal(|| None::<Pairing>);
    let mut now = use_signal(now_unix);
    let mut pairing_error = use_signal(|| None::<UiError>);
    let mut fw_busy = use_signal(|| false);
    let mut fw_result = use_signal(|| None::<Result<(), UiError>>);

    // One-second clock for the countdown (only runs while this page is open).
    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            now.set(now_unix());
        }
    });

    let open_window = use_callback({
        let backend = backend.clone();
        move |_: ()| {
            pairing_error.set(None);
            let backend = backend.clone();
            spawn(async move {
                match backend.link_open_pairing().await {
                    Ok(p) => pairing.set(Some(p)),
                    Err(e) => pairing_error.set(Some(e)),
                }
                status.restart();
            });
        }
    });

    let allow_private = {
        let backend = backend.clone();
        move |_| {
            if fw_busy() {
                return;
            }
            fw_busy.set(true);
            fw_result.set(None);
            let backend = backend.clone();
            spawn(async move {
                let r = backend.link_allow_private_network().await;
                fw_result.set(Some(r));
                fw_busy.set(false);
            });
        }
    };

    let status_line = match &*status.read() {
        Some(Ok(s)) if s.running => tf("link.status.running", &[("port", &s.port.to_string())]),
        Some(Ok(_)) => t("link.status.stopped"),
        _ => String::new(),
    };

    let devices_block = match &*status.read() {
        None => rsx! {
            div { class: "tk-skel tk-skel--list", "aria-hidden": "true" }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), compact: true, on_retry: move |_| status.restart() }
        },
        Some(Ok(s)) if s.devices.is_empty() => rsx! {
            EmptyState { icon: "link".to_string(), title: t("link.no_devices") }
        },
        Some(Ok(s)) => rsx! {
            ul { class: "tk-devices",
                for d in s.devices.clone() {
                    DeviceRow {
                        key: "{d.id}",
                        device: d.clone(),
                        on_revoke: {
                            let backend = backend.clone();
                            move |id: String| {
                                let backend = backend.clone();
                                spawn(async move {
                                    let _ = backend.link_revoke(&id).await;
                                    status.restart();
                                });
                            }
                        },
                    }
                }
            }
        },
    };

    let pairing_block = match pairing() {
        Some(p) if !p.is_expired(now()) => {
            let remaining = p.remaining_secs(now());
            let fraction = remaining as f64 / WINDOW_SECS;
            let svg = qr_svg(&p.uri);
            rsx! {
                div { class: "tk-pair",
                    if let Some(svg) = svg {
                        div { class: "tk-qr", role: "img", "aria-label": t("link.qr_label"), dangerous_inner_html: "{svg}" }
                    }
                    div { class: "tk-pair__side",
                        p { class: "tk-setting__hint", {t("link.scan_hint")} }
                        div { class: "tk-setting__label", {t("link.code")} }
                        div { class: "tk-code", aria_live: "off", {format_code(&p.code)} }
                        p { class: "tk-setting__hint", {tf("link.expires_in", &[("time", &format_countdown(remaining))])} }
                        WavyProgress { fraction, label: t("link.expires_in") }
                    }
                }
            }
        }
        Some(_) => rsx! {
            div { class: "tk-pair tk-pair--expired",
                p { class: "tk-setting__hint", {t("link.expired")} }
                Button {
                    label: t("link.regenerate"),
                    icon: "refresh".to_string(),
                    onclick: move |_| open_window.call(()),
                }
            }
        },
        None => rsx! {
            Button {
                label: t("link.open_window"),
                icon: "link".to_string(),
                onclick: move |_| open_window.call(()),
            }
        },
    };

    rsx! {
        div { class: "tk-page tk-page--link",
            h1 { class: "tk-page__title", {t("link.title")} }
            p { class: "tk-setting__hint", {t("link.intro")} }
            if !status_line.is_empty() {
                p { class: "tk-status", {status_line} }
            }

            section { class: "tk-panel",
                h2 { class: "tk-panel__title", {t("link.pair")} }
                {pairing_block}
                if let Some(e) = pairing_error() {
                    ErrorState { error: e.clone(), compact: true, on_retry: move |_| open_window.call(()) }
                }
            }

            section { class: "tk-panel",
                h2 { class: "tk-panel__title", {t("link.devices")} }
                {devices_block}
            }

            section { class: "tk-panel",
                h2 { class: "tk-panel__title", {t("link.allow_private")} }
                p { class: "tk-setting__hint", {t("link.allow_private.hint")} }
                Button {
                    label: t("link.allow_private"),
                    icon: "shield".to_string(),
                    variant: ButtonVariant::Tonal,
                    disabled: fw_busy(),
                    onclick: allow_private,
                }
                match fw_result() {
                    Some(Ok(())) => rsx! {
                        p { class: "tk-form__ok", role: "status", {t("link.allow_private.done")} }
                    },
                    Some(Err(e)) => rsx! {
                        ErrorState { error: e, compact: true }
                    },
                    None => rsx! {},
                }
            }
        }
    }
}

#[component]
fn DeviceRow(device: DeviceVm, #[props(default)] on_revoke: EventHandler<String>) -> Element {
    let platform_key = if device.platform.eq_ignore_ascii_case("android") { "link.platform.android" } else { "link.platform.unknown" };
    let seen = match device.last_seen_unix {
        Some(ts) => tf("link.last_seen", &[("when", &format_unix_utc(ts))]),
        None => t("link.never_seen"),
    };
    let id = device.id.clone();
    let revoke_label = tf("link.revoke_named", &[("name", &device.name)]);
    rsx! {
        li { class: "tk-device",
            div { class: "tk-device__text",
                div { class: "tk-device__name", "{device.name}" }
                div { class: "tk-device__sub", {t(platform_key)} " - " "{seen}" }
            }
            button {
                class: "tk-btn tk-btn--outlined tk-btn--sm",
                r#type: "button",
                "aria-label": "{revoke_label}",
                onclick: move |_| on_revoke.call(id.clone()),
                {t("link.revoke")}
            }
        }
    }
}
