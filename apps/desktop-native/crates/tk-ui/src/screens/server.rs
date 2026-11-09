//! Native extension server page `/server` (wave B, worker **k**; checklist `server.native-mode`, `server.settings-ui`,
//! `server.pairing`, `server.devices`, `server.viewer-password`, `server.logs`, `server.proxy`,
//! `server.cors-proxy-local`; classic `settings/HomeServerSettings.tsx`): status card (`Backend::server_status`,
//! start / stop, `data-server-running`, `data-server-port`, the LAN URLs phones use), pairing (QR + code + deep link,
//! `server_open_pairing`, `data-server-pairing`), devices (`server_devices`, revoke with confirmation,
//! `data-device-id`), viewer password (`server_set_viewer_password`, `data-viewer-password`), media proxy and cache
//! (`server_proxy_status`, `server_clear_cache`) and the request / error log (`server_logs`, polled every 3 s,
//! `data-server-log-cursor`). The preferences (port, name, pairing, proxy, FlareSolverr, sniffer) are in
//! Settings > Server & network. Reached from the Link page and from that settings section.

use std::time::Duration;

use dioxus::prelude::*;

use crate::app::{Route, Services, Ui};
use crate::backend::{ServerDeviceVm, ServerLogVm, ServerPairingVm, ServerStatusVm};
use crate::components::{
    Button, ButtonSize, ButtonVariant, ConfirmDialog, EmptyState, ErrorState, IconButton, IconButtonSize, Pill, SkeletonLines,
    TextField, Tone, WavyProgress, use_snackbar,
};
use crate::i18n::{t, tf};
use crate::icons::Icon;
use crate::model::server::{ServerPrefs, SERVER_DEFAULT_PORT};
use crate::model::{ErrorKind, SettingsAction, UiError, format_countdown, format_unix_utc, reduce_settings};
use crate::platform::now_unix;
use crate::player_state::format_bytes;
use crate::screens::link::rel_time;

/// Log lines kept on screen (the server keeps 500).
pub const LOG_LINES: usize = 200;
/// How often the log and the status refresh while the page is open.
const POLL_SECS: u64 = 3;

/// Append a log page to what is shown: a cursor lower than the one we had means the server restarted (new log),
/// so the list starts over; at most [`LOG_LINES`] newest lines are kept. Pure.
pub fn merge_logs(mut shown: Vec<ServerLogVm>, shown_cursor: u64, page: Vec<ServerLogVm>, cursor: u64) -> Vec<ServerLogVm> {
    if cursor < shown_cursor {
        shown.clear();
    }
    for e in page {
        if !shown.iter().any(|x| x.id == e.id) {
            shown.push(e);
        }
    }
    let excess = shown.len().saturating_sub(LOG_LINES);
    shown.drain(..excess);
    shown
}

/// Copy `text` to the clipboard (web view API).
fn copy_text(text: &str) {
    let arg = serde_json::to_string(text).unwrap_or_else(|_| "\"\"".into());
    let _ = document::eval(&format!("navigator.clipboard&&navigator.clipboard.writeText({arg});"));
}

/// A monospace value with a copy button.
#[component]
fn CopyRow(text: String) -> Element {
    let snack = use_snackbar();
    let value = text.clone();
    rsx! {
        div { class: "tk-server__copy",
            code { class: "tk-server__mono", "{text}" }
            IconButton {
                icon: "copy".to_string(),
                label: t("common.copy"),
                size: IconButtonSize::Sm,
                onclick: move |_| {
                    copy_text(&value);
                    snack.show(t("common.copied"));
                },
            }
        }
    }
}

#[component]
pub fn ServerPage() -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let backend = services.backend.clone();
    let navigator = use_navigator();
    let snack = use_snackbar();

    let mut status = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.server_status().await }
        }
    });
    let mut devices = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.server_devices().await }
        }
    });
    let mut proxy = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.server_proxy_status().await }
        }
    });
    let mut pairing = use_signal(|| None::<(ServerPairingVm, i64)>);
    let mut pair_error = use_signal(|| None::<UiError>);
    let mut busy = use_signal(|| false);
    let mut action_error = use_signal(|| None::<UiError>);
    let mut revoke_target = use_signal(|| None::<ServerDeviceVm>);
    let mut now = use_signal(now_unix);
    let mut logs = use_signal(Vec::<ServerLogVm>::new);
    let mut log_cursor = use_signal(|| 0_u64);
    let mut log_error = use_signal(|| None::<UiError>);
    let mut log_loaded = use_signal(|| false);

    // Clock (pairing countdown) and log / status polling, only while the page is open.
    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            now.set(now_unix());
        }
    });
    use_future({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move {
                let mut tick = 0_u64;
                loop {
                    let after = *log_cursor.peek();
                    match backend.server_logs(after).await {
                        Ok(page) => {
                            let shown = logs.peek().clone();
                            logs.set(merge_logs(shown, after, page.entries, page.cursor));
                            log_cursor.set(page.cursor);
                            log_error.set(None);
                        }
                        Err(e) => log_error.set(Some(e)),
                    }
                    log_loaded.set(true);
                    tokio::time::sleep(Duration::from_secs(POLL_SECS)).await;
                    tick += 1;
                    if tick % 4 == 0 {
                        status.restart();
                        devices.restart();
                        proxy.restart();
                    }
                }
            }
        }
    });

    // Start / stop also saves `Settings::server.enabled`, so the next launch does the same.
    let set_running = use_callback({
        let backend = backend.clone();
        move |on: bool| {
            if busy() {
                return;
            }
            busy.set(true);
            action_error.set(None);
            let mut prefs: ServerPrefs = ui.settings.peek().server.clone();
            prefs.enabled = on;
            let next = reduce_settings(&ui.settings.peek(), SettingsAction::SetServer(prefs));
            ui.commit_settings(&backend, next);
            let backend = backend.clone();
            spawn(async move {
                let r = if on { backend.server_start().await.map(|_| ()) } else { backend.server_stop().await };
                if let Err(e) = r {
                    action_error.set(Some(e));
                }
                if !on {
                    pairing.set(None);
                }
                status.restart();
                devices.restart();
                proxy.restart();
                busy.set(false);
            });
        }
    });

    let open_pairing = use_callback({
        let backend = backend.clone();
        move |_: ()| {
            pair_error.set(None);
            let backend = backend.clone();
            spawn(async move {
                match backend.server_open_pairing().await {
                    Ok(p) => pairing.set(Some((p, now_unix()))),
                    Err(e) => pair_error.set(Some(e)),
                }
            });
        }
    });

    let do_revoke = use_callback({
        let backend = backend.clone();
        move |_: ()| {
            let Some(d) = revoke_target() else { return };
            revoke_target.set(None);
            let backend = backend.clone();
            spawn(async move {
                match backend.server_revoke(&d.id).await {
                    Ok(()) => snack.show(tf("server.devices.revoked", &[("name", &d.label)])),
                    Err(e) => action_error.set(Some(e)),
                }
                devices.restart();
                status.restart();
            });
        }
    });

    let clear_cache = use_callback({
        let backend = backend.clone();
        move |_: ()| {
            let backend = backend.clone();
            spawn(async move {
                match backend.server_clear_cache().await {
                    Ok(()) => snack.show(t("server.proxy.cleared")),
                    Err(e) => action_error.set(Some(e)),
                }
                proxy.restart();
            });
        }
    });

    let st: Option<Result<ServerStatusVm, UiError>> = status.read().clone();
    let running = st.as_ref().and_then(|r| r.as_ref().ok()).is_some_and(|s| s.running);
    let port = st.as_ref().and_then(|r| r.as_ref().ok()).map_or(SERVER_DEFAULT_PORT, |s| s.port);

    let status_block = match st {
        None => rsx! {
            SkeletonLines { widths: vec![60, 40] }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e, title: t("server.status.error"), on_retry: move |_| status.restart() }
        },
        Some(Ok(s)) => {
            let cause = match &s.error {
                Some(e) if e.kind == ErrorKind::AddrInUse => tf("server.err.port", &[("port", &s.port.to_string())]),
                _ => String::new(),
            };
            let started = s.started_unix.map(|ts| tf("server.status.since", &[("when", &rel_time(ts, now()))])).unwrap_or_default();
            rsx! {
                div {
                    class: "tk-server__status",
                    "data-state": if s.running { "running" } else { "stopped" },
                    role: "status",
                    Icon { name: "desktop".to_string(), size: 28 }
                    div { class: "tk-server__status-text",
                        b {
                            if s.running {
                                {tf("server.status.running", &[("port", &s.port.to_string())])}
                            } else {
                                {t("server.status.stopped")}
                            }
                        }
                        span { class: "tk-server__sub",
                            "{s.name}"
                            if !started.is_empty() {
                                " · {started}"
                            }
                        }
                        div { class: "tk-server__pills",
                            Pill { text: tf("server.status.sources", &[("n", &s.sources.to_string())]), plain: true }
                            Pill { text: tf("server.status.devices", &[("n", &s.devices.to_string())]), plain: true }
                            if s.pairing_required {
                                Pill { text: t("server.status.pairing_required"), tone: Tone::Primary, plain: true }
                            }
                            if s.viewer_password_set {
                                Pill { text: t("server.status.password_set"), tone: Tone::Ok, plain: true }
                            }
                        }
                    }
                    if s.running {
                        Button {
                            label: t("server.stop"),
                            variant: ButtonVariant::Outlined,
                            icon: "pause".to_string(),
                            disabled: busy(),
                            onclick: move |_| set_running.call(false),
                        }
                    } else {
                        Button {
                            label: t("server.start"),
                            icon: "play".to_string(),
                            disabled: busy(),
                            onclick: move |_| set_running.call(true),
                        }
                    }
                }
                if let Some(e) = s.error.clone() {
                    ErrorState { error: e, compact: true, retry: false, cause }
                }
                if s.running {
                    div { class: "tk-server__urls",
                        div { class: "tk-setting__label", {t("server.urls")} }
                        p { class: "tk-panel__hint", {t("server.urls.hint")} }
                        for u in s.urls.clone() {
                            CopyRow { key: "{u}", text: u.clone() }
                        }
                    }
                }
                if let Some(id) = s.server_id.clone() {
                    p { class: "tk-server__sub", {tf("server.status.id", &[("id", &id)])} }
                }
            }
        }
    };

    let pairing_block = match pairing() {
        _ if !running => rsx! {
            p { class: "tk-panel__hint", {t("server.pair.start_first")} }
        },
        Some((p, at)) if i64::from(p.expires_in_secs) - (now() - at) > 0 => {
            let remaining = i64::from(p.expires_in_secs) - (now() - at);
            let fraction = remaining as f64 / f64::from(p.expires_in_secs.max(1));
            let code = p.code.clone();
            let snack2 = snack;
            rsx! {
                p { class: "tk-panel__hint", {t("server.pair.hint")} }
                div { class: "tk-pair", "data-server-pairing": "open",
                    if !p.qr_svg.is_empty() {
                        div { class: "tk-qr", role: "img", "aria-label": t("server.pair.qr"), dangerous_inner_html: "{p.qr_svg}" }
                    }
                    div { class: "tk-pair__side",
                        div { class: "tk-setting__label", {t("server.pair.code")} }
                        div { class: "tk-code",
                            span { "{p.code}" }
                            IconButton {
                                icon: "copy".to_string(),
                                label: t("common.copy"),
                                size: IconButtonSize::Sm,
                                onclick: move |_| {
                                    copy_text(&code);
                                    snack2.show(t("common.copied"));
                                },
                            }
                        }
                        div { class: "tk-setting__label", {t("server.pair.link")} }
                        CopyRow { text: p.deep_link.clone() }
                        div { class: "tk-pair__timer",
                            Icon { name: "history".to_string(), size: 18 }
                            span { {t("server.pair.expires")} }
                            b { class: "tk-num", {format_countdown(remaining)} }
                        }
                        WavyProgress { fraction, label: t("server.pair.expires") }
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
            div { class: "tk-pair tk-pair--expired", "data-server-pairing": "expired",
                p { class: "tk-panel__hint", {t("server.pair.expired")} }
                div {
                    Button { label: t("server.pair.new"), icon: "refresh".to_string(), onclick: move |_| open_pairing.call(()) }
                }
            }
        },
        None => rsx! {
            p { class: "tk-panel__hint", {t("server.pair.hint")} }
            div { "data-server-pairing": "closed",
                Button { label: t("server.pair.open"), icon: "qr".to_string(), onclick: move |_| open_pairing.call(()) }
            }
        },
    };

    let devices_block = match &*devices.read() {
        None => rsx! {
            SkeletonLines { widths: vec![70, 70] }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), compact: true, on_retry: move |_| devices.restart() }
        },
        Some(Ok(list)) if list.is_empty() => rsx! {
            EmptyState { icon: "phone".to_string(), title: t("server.devices.empty"), body: t("server.devices.empty_body") }
        },
        Some(Ok(list)) => {
            let rows = list.clone();
            rsx! {
                ul { class: "tk-devices",
                    for d in rows {
                        ServerDeviceRow { key: "{d.id}", device: d.clone(), now: now(), on_revoke: move |d: ServerDeviceVm| revoke_target.set(Some(d)) }
                    }
                }
            }
        }
    };

    let proxy_block = match &*proxy.read() {
        None => rsx! {
            SkeletonLines { widths: vec![50] }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), compact: true, on_retry: move |_| proxy.restart() }
        },
        Some(Ok(p)) => {
            let usage = tf(
                "server.proxy.cache",
                &[("n", &p.cache_entries.to_string()), ("size", &format_bytes(p.cache_bytes)), ("max", &format_bytes(p.max_cache_bytes))],
            );
            rsx! {
                div { class: "tk-server__proxy", "data-proxy-enabled": "{p.enabled}",
                    p { class: "tk-panel__hint",
                        if p.enabled { {t("server.proxy.on")} } else { {t("server.proxy.off")} }
                    }
                    if let Some(base) = p.base_url.clone() {
                        CopyRow { text: base }
                    }
                    p { class: "tk-server__sub", "{usage}" }
                    div {
                        Button {
                            label: t("server.proxy.clear"),
                            variant: ButtonVariant::Tonal,
                            size: ButtonSize::Sm,
                            icon: "trash".to_string(),
                            onclick: move |_| clear_cache.call(()),
                        }
                    }
                }
            }
        }
    };

    let shown_logs = logs();
    let cursor = log_cursor();
    let log_block = if !log_loaded() {
        rsx! {
            SkeletonLines { widths: vec![80, 60, 70] }
        }
    } else if let Some(e) = log_error() {
        rsx! {
            ErrorState { error: e, compact: true, retry: false }
        }
    } else if shown_logs.is_empty() {
        rsx! {
            EmptyState {
                icon: "terminal".to_string(),
                title: t("server.logs.empty"),
                body: if running { t("server.logs.empty_running") } else { t("server.logs.empty_stopped") },
            }
        }
    } else {
        rsx! {
            ol { class: "tk-server__log",
                for e in shown_logs.into_iter().rev() {
                    LogLine { key: "{e.id}", entry: e.clone() }
                }
            }
        }
    };

    let revoke_label = revoke_target().map(|d| d.label).unwrap_or_default();

    rsx! {
        div { class: "tk-page tk-server", "data-page": "server.title", "data-server-running": "{running}", "data-server-port": "{port}",
            div { class: "tk-page__head",
                div {
                    h1 { class: "tk-page__title", {t("server.title")} }
                    p { class: "tk-page__intro", {t("server.intro")} }
                }
                Button {
                    label: t("server.settings"),
                    variant: ButtonVariant::Tonal,
                    icon: "settings".to_string(),
                    onclick: move |_| {
                        navigator.push(Route::SettingsPage {});
                    },
                }
            }
            {status_block}
            if let Some(e) = action_error() {
                ErrorState { error: e, compact: true, retry: false }
            }
            div { class: "tk-server__grid",
                section { class: "tk-panel",
                    h2 { class: "tk-panel__title",
                        Icon { name: "qr".to_string(), size: 24 }
                        {t("server.pair")}
                    }
                    {pairing_block}
                    if let Some(e) = pair_error() {
                        ErrorState { error: e, compact: true, on_retry: move |_| open_pairing.call(()) }
                    }
                }
                section { class: "tk-panel",
                    h2 { class: "tk-panel__title",
                        Icon { name: "phone".to_string(), size: 24 }
                        {t("server.devices")}
                    }
                    {devices_block}
                }
                section { class: "tk-panel",
                    h2 { class: "tk-panel__title",
                        Icon { name: "shield".to_string(), size: 24 }
                        {t("server.password")}
                    }
                    ViewerPassword {
                        set: status.read().as_ref().and_then(|r| r.as_ref().ok()).is_some_and(|s| s.viewer_password_set),
                        on_changed: move |_| status.restart(),
                    }
                }
                section { class: "tk-panel",
                    h2 { class: "tk-panel__title",
                        Icon { name: "cast".to_string(), size: 24 }
                        {t("server.proxy")}
                    }
                    {proxy_block}
                }
            }
            section { class: "tk-panel", "data-server-log-cursor": "{cursor}",
                h2 { class: "tk-panel__title",
                    Icon { name: "terminal".to_string(), size: 24 }
                    {t("server.logs")}
                }
                {log_block}
            }
        }

        if revoke_target().is_some() {
            ConfirmDialog {
                title: t("server.devices.revoke_title"),
                body: tf("server.devices.revoke_body", &[("name", &revoke_label)]),
                confirm_label: t("server.devices.revoke"),
                icon: "shield".to_string(),
                on_cancel: move |_| revoke_target.set(None),
                on_confirm: move |_| do_revoke.call(()),
            }
        }
    }
}

#[component]
fn ServerDeviceRow(device: ServerDeviceVm, now: i64, on_revoke: EventHandler<ServerDeviceVm>) -> Element {
    let seen = match device.last_seen_unix {
        Some(ts) => tf("server.devices.seen", &[("when", &rel_time(ts, now))]),
        None => t("server.devices.never"),
    };
    let paired = tf("server.devices.paired", &[("when", &format_unix_utc(device.created_unix))]);
    let role = if device.role == "admin" { t("server.devices.role_admin") } else { t("server.devices.role_viewer") };
    let label = tf("server.devices.revoke_named", &[("name", &device.label)]);
    let target = device.clone();
    rsx! {
        li { class: "tk-device", "data-device-id": "{device.id}", "data-role": "{device.role}",
            span { class: "tk-device__ico", Icon { name: "phone".to_string(), size: 22 } }
            div { class: "tk-device__text",
                div { class: "tk-device__name", span { "{device.label}" } }
                div { class: "tk-device__sub", "{role} · {seen} · {paired}" }
            }
            if !device.current {
                div { class: "tk-device__ctl",
                    button {
                        class: "tk-btn tk-btn--outlined tk-btn--xs",
                        r#type: "button",
                        "aria-label": "{label}",
                        onclick: move |_| on_revoke.call(target.clone()),
                        {t("server.devices.revoke")}
                    }
                }
            }
        }
    }
}

#[component]
fn LogLine(entry: ServerLogVm) -> Element {
    let when = format_unix_utc(entry.at_unix);
    let mut parts: Vec<String> = Vec::new();
    if let Some(r) = &entry.route {
        parts.push(r.clone());
    }
    if let Some(n) = &entry.name {
        parts.push(n.clone());
    }
    if let Some(c) = entry.count {
        parts.push(tf("server.logs.count", &[("n", &c.to_string())]));
    }
    if let Some(ms) = entry.duration_ms {
        parts.push(format!("{ms} ms"));
    }
    let detail = parts.join(" · ");
    let failed = entry.error.is_some();
    rsx! {
        li { class: "tk-server__log-line", "data-event": "{entry.event}", "data-error": "{failed}",
            span { class: "tk-server__mono tk-server__log-time", "{when}" }
            b { "{entry.event}" }
            span { "{detail}" }
            if let Some(e) = entry.error.clone() {
                span { class: "tk-server__log-err", "{e}" }
            }
        }
    }
}

/// Viewer password: set (10..=1024 characters, stored as an argon2id hash by the server) or clear.
#[component]
fn ViewerPassword(set: bool, on_changed: EventHandler<()>) -> Element {
    let services = use_context::<Services>();
    let backend = services.backend.clone();
    let snack = use_snackbar();
    let mut draft = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<UiError>);
    let chars = draft().chars().count();
    let too_short = chars > 0 && chars < 10;

    let save = use_callback({
        let backend = backend.clone();
        move |value: Option<String>| {
            if busy() {
                return;
            }
            busy.set(true);
            error.set(None);
            let backend = backend.clone();
            spawn(async move {
                match backend.server_set_viewer_password(value.as_deref()).await {
                    Ok(()) => {
                        draft.set(String::new());
                        snack.show(if value.is_some() { t("server.password.saved") } else { t("server.password.cleared") });
                        on_changed.call(());
                    }
                    Err(e) => error.set(Some(e)),
                }
                busy.set(false);
            });
        }
    });

    rsx! {
        div { class: "tk-server__password", "data-viewer-password": if set { "set" } else { "none" },
            p { class: "tk-panel__hint", {t("server.password.hint")} }
            p { class: "tk-server__sub",
                if set { {t("server.password.is_set")} } else { {t("server.password.not_set")} }
            }
            TextField {
                label: t("server.password.field"),
                kind: "password".to_string(),
                value: draft(),
                error: too_short,
                hint: if too_short { t("server.password.short") } else { String::new() },
                disabled: busy(),
                oninput: move |v: String| draft.set(v),
                onsubmit: move |_| {
                    if draft().chars().count() >= 10 {
                        save.call(Some(draft()));
                    }
                },
            }
            div { class: "tk-server__row",
                Button {
                    label: t("common.save"),
                    variant: ButtonVariant::Tonal,
                    size: ButtonSize::Sm,
                    disabled: busy() || chars < 10,
                    onclick: move |_| save.call(Some(draft())),
                }
                if set {
                    Button {
                        label: t("server.password.clear"),
                        variant: ButtonVariant::Text,
                        size: ButtonSize::Sm,
                        disabled: busy(),
                        onclick: move |_| save.call(None),
                    }
                }
            }
            if let Some(e) = error() {
                ErrorState { error: e, compact: true, retry: false }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(id: u64) -> ServerLogVm {
        ServerLogVm { id, at_unix: 0, event: "request".into(), route: None, name: None, count: None, duration_ms: None, error: None }
    }

    #[test]
    fn logs_append_dedupe_cap_and_reset() {
        let shown = merge_logs(Vec::new(), 0, vec![line(1), line(2)], 2);
        assert_eq!(shown.len(), 2);
        let shown = merge_logs(shown, 2, vec![line(2), line(3)], 3);
        assert_eq!(shown.iter().map(|l| l.id).collect::<Vec<_>>(), vec![1, 2, 3]);
        let restarted = merge_logs(shown, 3, vec![line(1)], 1);
        assert_eq!(restarted.len(), 1, "a lower cursor = a new log");
        let many: Vec<ServerLogVm> = (1..=(LOG_LINES as u64 + 10)).map(line).collect();
        let capped = merge_logs(Vec::new(), 0, many, LOG_LINES as u64 + 10);
        assert_eq!(capped.len(), LOG_LINES);
        assert_eq!(capped[0].id, 11);
    }
}
