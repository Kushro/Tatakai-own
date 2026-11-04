//! Tatakai Link: server status card (readable start errors + restart), steps 1-2-3, pairing (QR + code +
//! expiry countdown), paired devices (rename / revoke, "Connected now"), firewall flow and this PC's
//! capabilities. Prototype: `screens.js::S.link`.

use std::time::Duration;

use dioxus::prelude::*;

use crate::app::Services;
use crate::backend::{DeviceVm, LinkCapsVm, LinkStatusVm};
use crate::components::title_bar::ShellStatus;
use crate::components::{
    Banner, Button, ButtonSize, ButtonVariant, ConfirmDialog, EmptyState, ErrorState, IconButton, IconButtonSize, InfoChip,
    Pill, SkeletonLines, TextField, Tone, WavyProgress, use_snackbar,
};
use crate::i18n::{t, tf};
use crate::icons::Icon;
use crate::model::{ErrorKind, Pairing, PlatformVm, UiError, format_code, format_countdown, format_unix_utc};
use crate::platform::now_unix;
use crate::qr::qr_svg;

/// The pairing window is 5 minutes (docs/redesign/03 section 2); used only to scale the progress bar.
const WINDOW_SECS: f64 = 300.0;
/// Default Link port (tk-link); "Restart on another port" moves `PORT_STEP` away from the taken one
/// (prototype: 47321 -> 47331).
const DEFAULT_PORT: u16 = 47321;
const PORT_STEP: u16 = 10;

/// Port offered by "Restart on another port".
pub fn alt_port(port: u16) -> u16 {
    if port < 1024 {
        return DEFAULT_PORT;
    }
    match port.checked_add(PORT_STEP) {
        Some(p) => p,
        None => DEFAULT_PORT,
    }
}

/// "now", "5 min ago", "3 h ago", "yesterday", "4 days ago"; older than 30 days -> UTC date.
fn rel_time(ts: i64, now: i64) -> String {
    let d = (now - ts).max(0);
    match d {
        0..60 => t("time.now"),
        60..3_600 => tf("time.min_ago", &[("n", &(d / 60).to_string())]),
        3_600..86_400 => tf("time.h_ago", &[("n", &(d / 3_600).to_string())]),
        86_400..172_800 => t("time.yesterday"),
        172_800..2_592_000 => tf("time.d_ago", &[("n", &(d / 86_400).to_string())]),
        _ => format_unix_utc(ts),
    }
}

#[component]
pub fn LinkPage() -> Element {
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
    let mut caps = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.link_capabilities().await }
        }
    });
    let mut pairing = use_signal(|| None::<Pairing>);
    let mut now = use_signal(now_unix);
    let mut pairing_error = use_signal(|| None::<UiError>);
    let mut fw_busy = use_signal(|| false);
    let mut fw_result = use_signal(|| None::<Result<(), UiError>>);
    let mut restart_busy = use_signal(|| false);
    let mut restart_error = use_signal(|| None::<UiError>);
    let mut rename_target = use_signal(|| None::<DeviceVm>);
    let mut rename_value = use_signal(String::new);
    let mut revoke_target = use_signal(|| None::<DeviceVm>);
    let mut device_error = use_signal(|| None::<UiError>);

    // One-second clock for the countdown (only runs while this page is open).
    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            now.set(now_unix());
        }
    });

    // Keep the title bar chip in step with what this page shows.
    use_effect(move || {
        if let (Some(mut sh), Some(Ok(s))) = (shell, &*status.read())
            && *sh.link.peek() != Some(s.clone())
        {
            sh.link.set(Some(s.clone()));
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

    // Retry / restart on another port / start all go through `link_restart(port)`. A taken port comes
    // back as a status with `error: Some(AddrInUse)`, which the status card renders after the refetch.
    let restart = use_callback({
        let backend = backend.clone();
        move |port: u16| {
            if restart_busy() {
                return;
            }
            restart_busy.set(true);
            restart_error.set(None);
            let backend = backend.clone();
            spawn(async move {
                match backend.link_restart(port).await {
                    Ok(s) => {
                        if s.running {
                            snack.show(tf("link.status.running", &[("port", &s.port.to_string())]));
                        }
                        status.restart();
                    }
                    Err(e) => restart_error.set(Some(e)),
                }
                restart_busy.set(false);
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
                if r.is_ok() {
                    snack.show(t("link.allow_private.done"));
                }
                fw_result.set(Some(r));
                fw_busy.set(false);
            });
        }
    };

    let do_rename = use_callback({
        let backend = backend.clone();
        move |_: ()| {
            let Some(d) = rename_target() else { return };
            let name = rename_value().trim().to_owned();
            if name.is_empty() {
                return;
            }
            rename_target.set(None);
            device_error.set(None);
            let backend = backend.clone();
            spawn(async move {
                match backend.link_rename_device(&d.id, &name).await {
                    Ok(()) => {
                        snack.show(t("link.renamed"));
                        status.restart();
                    }
                    Err(e) => device_error.set(Some(e)),
                }
            });
        }
    });

    let do_revoke = use_callback({
        let backend = backend.clone();
        move |_: ()| {
            let Some(d) = revoke_target() else { return };
            revoke_target.set(None);
            device_error.set(None);
            let backend = backend.clone();
            spawn(async move {
                match backend.link_revoke(&d.id).await {
                    Ok(()) => {
                        snack.show(tf("link.revoked", &[("name", &d.name)]));
                        status.restart();
                    }
                    Err(e) => device_error.set(Some(e)),
                }
            });
        }
    });

    let current: Option<LinkStatusVm> = status.read().as_ref().and_then(|r| r.as_ref().ok()).cloned();
    let caps_ok: Option<LinkCapsVm> = caps.read().as_ref().and_then(|r| r.as_ref().ok()).cloned();
    let running = current.as_ref().is_some_and(|s| s.running);

    let status_block = match &*status.read() {
        None => rsx! {
            div { class: "tk-skel tk-link-status--skel", "aria-hidden": "true" }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), title: t("link.status.error"), on_retry: move |_| status.restart() }
        },
        Some(Ok(s)) => rsx! {
            StatusCard {
                status: s.clone(),
                caps: caps_ok.clone(),
                busy: restart_busy(),
                on_restart: move |p: u16| restart.call(p),
            }
        },
    };

    // The firewall rule flow is Windows-only (netsh + UAC); elsewhere the app does not touch the firewall, so
    // step 1 is a reminder that counts as done once the server listens.
    let windows = PlatformVm::current() == PlatformVm::Windows;
    let port_text = current.as_ref().map(|s| s.port).unwrap_or(DEFAULT_PORT).to_string();
    let fw_done = if windows { matches!(fw_result(), Some(Ok(()))) } else { running };
    let open_done = pairing().is_some_and(|p| !p.is_expired(now()));
    let scan_done = current.as_ref().is_some_and(|s| !s.devices.is_empty());
    let device_count = current.as_ref().map(|s| s.devices.len()).unwrap_or(0);

    let devices_block = match &*status.read() {
        None => rsx! {
            div { class: "tk-skel tk-skel--list", "aria-hidden": "true" }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), compact: true, on_retry: move |_| status.restart() }
        },
        Some(Ok(s)) if s.devices.is_empty() => rsx! {
            EmptyState { icon: "phone".to_string(), title: t("link.no_devices"), body: t("link.no_devices_body") }
        },
        Some(Ok(s)) => rsx! {
            ul { class: "tk-devices",
                for d in s.devices.clone() {
                    DeviceRow {
                        key: "{d.id}",
                        device: d.clone(),
                        now: now(),
                        on_rename: move |d: DeviceVm| {
                            rename_value.set(d.name.clone());
                            rename_target.set(Some(d));
                        },
                        on_revoke: move |d: DeviceVm| revoke_target.set(Some(d)),
                    }
                }
            }
        },
    };

    let pairing_block = match pairing() {
        _ if !running => rsx! {
            p { class: "tk-panel__hint", {t("link.status.stopped")} }
            div {
                Button { label: t("link.open_window"), icon: "link".to_string(), disabled: true }
            }
        },
        Some(p) if !p.is_expired(now()) => {
            let remaining = p.remaining_secs(now());
            let fraction = remaining as f64 / WINDOW_SECS;
            let svg = qr_svg(&p.uri);
            let code = p.code.clone();
            rsx! {
                p { class: "tk-panel__hint", {t("link.scan_hint")} }
                div { class: "tk-pair",
                    if let Some(svg) = svg {
                        div { class: "tk-qr", role: "img", "aria-label": t("link.qr_label"), dangerous_inner_html: "{svg}" }
                    }
                    div { class: "tk-pair__side",
                        div { class: "tk-setting__label", {t("link.code")} }
                        div { class: "tk-code",
                            span { aria_live: "off", {format_code(&p.code)} }
                            IconButton {
                                icon: "copy".to_string(),
                                label: t("common.copy"),
                                size: IconButtonSize::Sm,
                                onclick: move |_| {
                                    let arg = serde_json::to_string(&code).unwrap_or_else(|_| "\"\"".into());
                                    let _ = document::eval(&format!("navigator.clipboard&&navigator.clipboard.writeText({arg});"));
                                    snack.show(t("common.copied"));
                                },
                            }
                        }
                        div { class: "tk-pair__timer",
                            Icon { name: "history".to_string(), size: 18 }
                            span { {tf("link.expires_in", &[("time", "")])} }
                            b { class: "tk-num", {format_countdown(remaining)} }
                        }
                        WavyProgress { fraction, label: t("link.expires_in") }
                        div {
                            Button {
                                label: t("common.cancel"),
                                variant: ButtonVariant::Text,
                                size: ButtonSize::Sm,
                                onclick: move |_| pairing.set(None),
                            }
                        }
                    }
                }
            }
        }
        Some(_) => rsx! {
            div { class: "tk-pair tk-pair--expired",
                p { class: "tk-panel__hint", {t("link.expired")} }
                div {
                    Button {
                        label: t("link.regenerate"),
                        icon: "refresh".to_string(),
                        onclick: move |_| open_window.call(()),
                    }
                }
            }
        },
        None => rsx! {
            p { class: "tk-panel__hint", {t("link.scan_hint")} }
            div {
                Button {
                    label: t("link.open_window"),
                    icon: "link".to_string(),
                    onclick: move |_| open_window.call(()),
                }
            }
        },
    };

    let caps_block = match &*caps.read() {
        None => rsx! {
            SkeletonLines { widths: vec![70, 50, 60] }
        },
        Some(Err(e)) => rsx! {
            ErrorState {
                error: e.clone(),
                compact: true,
                retry: e.kind != ErrorKind::NotWired,
                on_retry: move |_| caps.restart(),
            }
        },
        Some(Ok(c)) => {
            let unknown = t("link.caps.unknown");
            let gpu = match (&c.gpu, &c.vulkan) {
                (Some(g), Some(v)) => format!("{g} · Vulkan {v}"),
                (Some(g), None) => g.clone(),
                _ => unknown.clone(),
            };
            let encoder = c.encoder.clone().unwrap_or_else(|| unknown.clone());
            let upscale = c.upscale.clone().unwrap_or_else(|| unknown.clone());
            let profiles = c.profiles.clone();
            let (measured, unmeasured) = (t("link.caps.measured"), t("link.caps.unknown"));
            rsx! {
                div { class: "tk-caps",
                    Cap { k: t("link.caps.gpu"), v: gpu }
                    Cap { k: t("link.caps.encoder"), v: encoder }
                    Cap { k: t("link.caps.upscale"), v: upscale }
                }
                div { class: "tk-caps__profiles",
                    div { class: "tk-setting__label", {t("link.caps.profiles")} }
                    if profiles.is_empty() {
                        p { class: "tk-panel__hint", "{unknown}" }
                    } else {
                        div { class: "tk-chips",
                            for (name , ok) in profiles {
                                span {
                                    key: "{name}",
                                    title: if ok.is_some() { measured.clone() } else { unmeasured.clone() },
                                    InfoChip { text: name.clone(), icon: profile_icon(ok).to_string() }
                                }
                            }
                        }
                    }
                }
            }
        }
    };

    let revoke_name = revoke_target().map(|d| d.name).unwrap_or_default();

    rsx! {
        div { class: "tk-page tk-page--link",
            div { class: "tk-page__head",
                div {
                    h1 { class: "tk-page__title", {t("link.title")} }
                    p { class: "tk-page__intro", {t("link.intro")} }
                }
            }

            {status_block}
            if let Some(e) = restart_error() {
                ErrorState { error: e.clone(), compact: true, retry: false }
            }

            ol { class: "tk-steps",
                if windows {
                    Step { n: 1, title: t("link.steps.fw"), sub: t("link.steps.fw_sub"), done: fw_done }
                } else {
                    Step { n: 1, title: t("link.steps.fw_other"), sub: t("link.steps.fw_other_sub"), done: fw_done }
                }
                Step { n: 2, title: t("link.steps.open"), sub: t("link.steps.open_sub"), done: open_done }
                Step { n: 3, title: t("link.steps.scan"), sub: t("link.steps.scan_sub"), done: scan_done }
            }

            div { class: "tk-link-grid",
                section { class: "tk-panel",
                    h2 { class: "tk-panel__title",
                        Icon { name: "qr".to_string(), size: 24 }
                        {t("link.pair")}
                    }
                    {pairing_block}
                    if let Some(e) = pairing_error() {
                        ErrorState { error: e.clone(), compact: true, on_retry: move |_| open_window.call(()) }
                    }
                }

                section { class: "tk-panel",
                    h2 { class: "tk-panel__title",
                        Icon { name: "phone".to_string(), size: 24 }
                        {t("link.devices")}
                        if device_count > 0 {
                            span { class: "tk-label-md tk-muted", "{device_count}" }
                        }
                    }
                    {devices_block}
                    if let Some(e) = device_error() {
                        ErrorState { error: e.clone(), compact: true, retry: false }
                    }
                }
            }

            div { class: "tk-link-grid",
                if windows {
                    section { class: "tk-panel",
                        h2 { class: "tk-panel__title",
                            Icon { name: "shield".to_string(), size: 24 }
                            {t("link.allow_private")}
                        }
                        p { class: "tk-panel__hint", {t("link.allow_private.hint")} }
                        if fw_done {
                            Banner { text: t("link.allow_private.already"), icon: "check".to_string(), tone: Tone::Ok }
                        } else {
                            div {
                                Button {
                                    label: if fw_busy() { t("link.allow_private.busy") } else { t("link.allow_private") },
                                    icon: "shield".to_string(),
                                    variant: ButtonVariant::Tonal,
                                    disabled: fw_busy(),
                                    onclick: allow_private,
                                }
                            }
                        }
                        if let Some(Err(e)) = fw_result() {
                            ErrorState { error: e, compact: true, retry: false }
                        }
                    }
                } else {
                    section { class: "tk-panel",
                        h2 { class: "tk-panel__title",
                            Icon { name: "shield".to_string(), size: 24 }
                            {t("link.firewall_other")}
                        }
                        p { class: "tk-panel__hint", {tf("link.firewall_other.hint", &[("port", &port_text)])} }
                    }
                }

                section { class: "tk-panel",
                    h2 { class: "tk-panel__title",
                        Icon { name: "gpu".to_string(), size: 24 }
                        {t("link.caps")}
                    }
                    {caps_block}
                }
            }
        }

        if rename_target().is_some() {
            ConfirmDialog {
                title: t("common.rename"),
                body: String::new(),
                confirm_label: t("common.save"),
                icon: "edit".to_string(),
                danger: false,
                confirm_disabled: rename_value().trim().is_empty(),
                on_cancel: move |_| rename_target.set(None),
                on_confirm: move |_| do_rename.call(()),
                TextField {
                    label: t("link.new_name"),
                    value: rename_value(),
                    oninput: move |v: String| rename_value.set(v),
                    onsubmit: move |_| do_rename.call(()),
                }
            }
        }

        if revoke_target().is_some() {
            ConfirmDialog {
                title: t("link.revoke.title"),
                body: tf("link.revoke.body", &[("name", &revoke_name)]),
                confirm_label: t("link.revoke"),
                icon: "shield".to_string(),
                on_cancel: move |_| revoke_target.set(None),
                on_confirm: move |_| do_revoke.call(()),
            }
        }
    }
}

/// Chip icon of a capability profile: supported, not supported, not measured yet.
fn profile_icon(ok: Option<bool>) -> &'static str {
    match ok {
        Some(true) => "check",
        Some(false) => "close",
        None => "",
    }
}

/// The server card: running (address + fingerprint), failed to start (cause, collapsible OS detail,
/// Retry and, for a taken port, "Restart on another port") or stopped (Start).
#[component]
fn StatusCard(
    status: LinkStatusVm,
    #[props(default)] caps: Option<LinkCapsVm>,
    #[props(default)] busy: bool,
    #[props(default)] on_restart: EventHandler<u16>,
) -> Element {
    let port = status.port;
    let port_text = port.to_string();

    if status.running {
        let addr = caps.as_ref().and_then(|c| c.lan_ip.clone()).map(|ip| format!("https://{ip}:{port}"));
        let fingerprint = caps.as_ref().map(|c| c.fingerprint.clone()).filter(|f| !f.is_empty());
        let fp_label = t("link.fingerprint");
        return rsx! {
            div { class: "tk-link-status", "data-state": "running", role: "status",
                div { class: "tk-link-status__orb", Icon { name: "link".to_string(), size: 28 } }
                div { class: "tk-link-status__text",
                    b { {tf("link.status.running", &[("port", &port_text)])} }
                    if let Some(addr) = addr {
                        span { {tf("link.status.addr", &[("addr", &addr)])} }
                    }
                    if let Some(fp) = fingerprint {
                        code { "{fp_label}: {fp}" }
                    }
                }
            }
        };
    }

    match status.error.clone() {
        Some(err) => {
            let addr_in_use = err.kind == ErrorKind::AddrInUse;
            let cause = if addr_in_use {
                format!("{} {}", tf("link.err.port", &[("port", &port_text)]), t("link.err.port_fix"))
            } else {
                t(err.kind.key())
            };
            let alt = alt_port(port);
            rsx! {
                div { class: "tk-link-status", "data-state": "error", role: "alert",
                    div { class: "tk-link-status__orb", Icon { name: "warning".to_string(), size: 28 } }
                    div { class: "tk-link-status__text",
                        b { {t("link.status.error")} }
                        span { "{cause}" }
                        if !err.detail.is_empty() {
                            details { class: "tk-link-status__details",
                                summary { {t("state.error.detail")} }
                                code { "{err.detail}" }
                            }
                        }
                    }
                    div { class: "tk-page__actions",
                        Button {
                            label: t("common.retry"),
                            icon: "refresh".to_string(),
                            variant: ButtonVariant::Tonal,
                            disabled: busy,
                            onclick: move |_| on_restart.call(port),
                        }
                        if addr_in_use {
                            Button {
                                label: t("link.restart"),
                                variant: ButtonVariant::Text,
                                disabled: busy,
                                onclick: move |_| on_restart.call(alt),
                            }
                        }
                    }
                }
            }
        }
        None => rsx! {
            div { class: "tk-link-status", "data-state": "stopped", role: "status",
                div { class: "tk-link-status__orb", Icon { name: "link".to_string(), size: 28 } }
                div { class: "tk-link-status__text",
                    b { {t("link.status.stopped")} }
                    span { {t("link.intro")} }
                }
                Button {
                    label: t("link.start"),
                    icon: "play".to_string(),
                    disabled: busy,
                    onclick: move |_| on_restart.call(port),
                }
            }
        },
    }
}

#[component]
fn Step(n: u8, title: String, sub: String, done: bool) -> Element {
    rsx! {
        li { class: "tk-step", "data-done": "{done}",
            span { class: "tk-step__n",
                if done {
                    Icon { name: "check".to_string(), size: 16 }
                } else {
                    "{n}"
                }
            }
            b { "{title}" }
            span { "{sub}" }
        }
    }
}

#[component]
fn Cap(k: String, v: String) -> Element {
    rsx! {
        div { class: "tk-cap",
            span { class: "tk-cap__k", "{k}" }
            span { class: "tk-cap__v", "{v}" }
        }
    }
}

#[component]
fn DeviceRow(
    device: DeviceVm,
    now: i64,
    #[props(default)] on_rename: EventHandler<DeviceVm>,
    #[props(default)] on_revoke: EventHandler<DeviceVm>,
) -> Element {
    let platform_key = if device.platform.eq_ignore_ascii_case("android") { "link.platform.android" } else { "link.platform.unknown" };
    let seen = match device.last_seen_unix {
        _ if device.online => t("link.online"),
        Some(ts) => tf("link.last_seen", &[("when", &rel_time(ts, now))]),
        None => t("link.never_seen"),
    };
    let icon = if device.name.to_lowercase().contains("tab") { "tablet" } else { "phone" };
    let revoke_label = tf("link.revoke_named", &[("name", &device.name)]);
    let (for_rename, for_revoke) = (device.clone(), device.clone());
    rsx! {
        li { class: "tk-device", "data-online": "{device.online}",
            span { class: "tk-device__ico", Icon { name: icon.to_string(), size: 22 } }
            div { class: "tk-device__text",
                div { class: "tk-device__name",
                    span { "{device.name}" }
                    if device.online {
                        Pill { text: t("link.online"), tone: Tone::Ok }
                    }
                }
                div { class: "tk-device__sub", {t(platform_key)} " · " "{seen}" " · " "{device.id}" }
            }
            div { class: "tk-device__ctl",
                IconButton {
                    icon: "edit".to_string(),
                    label: t("common.rename"),
                    size: IconButtonSize::Sm,
                    onclick: move |_| on_rename.call(for_rename.clone()),
                }
                button {
                    class: "tk-btn tk-btn--outlined tk-btn--xs",
                    r#type: "button",
                    "aria-label": "{revoke_label}",
                    onclick: move |_| on_revoke.call(for_revoke.clone()),
                    {t("link.revoke")}
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::alt_port;

    #[test]
    fn restart_port_moves_away_and_stays_valid() {
        assert_eq!(alt_port(47321), 47331);
        assert_eq!(alt_port(65530), 47321);
        assert_eq!(alt_port(0), 47321);
    }
}
