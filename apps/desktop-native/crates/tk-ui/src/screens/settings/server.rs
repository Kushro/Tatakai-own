//! Settings > Server & network (wave B, worker **k**; checklist `server.settings-ui`, `server.native-mode`,
//! `server.proxy`, `server.flaresolverr`, `server.cors-proxy-local`, `sources.embed-sniffer.timeout-setting`; classic
//! `HomeServerSettings.tsx`, `ProxySettingsPanel.tsx`, `FlareSolverrSettings.tsx`): start the extension server with
//! the app (`data-server-enabled`), port, name, pairing required, media proxy for phones, resolve timeout /
//! concurrency, FlareSolverr URL + "Test" (`flaresolverr_test`, `data-flaresolverr="idle|testing|ok|error"`),
//! outbound HTTP / SOCKS5 proxy + "Test" (`data-proxy`), the loopback proxy for mpv, the embed sniffer switch and
//! timeout 5..=60 s (`data-sniffer`, `data-sniffer-timeout`), and "Open the server page" -> `Route::ServerPage`.
//! Every change dispatches the whole `ServerPrefs` (`SettingsAction::SetServer`).

use dioxus::prelude::*;

use super::{SettingRow, parse_port};
use crate::app::{Route, Services, Ui};
use crate::backend::FlareSolverrVm;
use crate::components::{Button, ButtonSize, ButtonVariant, ErrorState, Switch, TextField};
use crate::i18n::{t, tf};
use crate::model::server::{PROXY_TEST_PREFIX, SNIFF_MAX_SECS, SNIFF_MIN_SECS, ServerPrefs, proxy_url_ok, test_state_attr};
use crate::model::{SettingsAction, UiError};

/// A number typed in a field, accepted only inside `min..=max`. Pure.
pub fn parse_in_range(text: &str, min: u32, max: u32) -> Option<u32> {
    text.trim().parse::<u32>().ok().filter(|v| (min..=max).contains(v))
}

/// An `http(s)://` URL with a host, as FlareSolverr endpoints are typed (empty = off). Pure.
pub fn endpoint_ok(text: &str) -> bool {
    let t = text.trim();
    (t.starts_with("http://") || t.starts_with("https://")) && t.split_once("://").is_some_and(|(_, rest)| !rest.is_empty() && !rest.contains(' '))
}

#[component]
pub fn ServerSection(commit_saved: Callback<SettingsAction>) -> Element {
    let ui = use_context::<Ui>();
    let navigator = use_navigator();
    let prefs = ui.settings.read().server.clone();

    let set = move |f: Box<dyn Fn(&mut ServerPrefs)>| {
        let mut p = ui.settings.peek().server.clone();
        f(&mut p);
        commit_saved.call(SettingsAction::SetServer(p));
    };
    let toggle = move |f: fn(&mut ServerPrefs, bool)| {
        move |v: bool| {
            let mut p = ui.settings.peek().server.clone();
            f(&mut p, v);
            commit_saved.call(SettingsAction::SetServer(p));
        }
    };

    rsx! {
        p { class: "tk-panel__hint", {t("settings.server.hint")} }
        div { class: "tk-settings__group", "data-server-enabled": "{prefs.enabled}",
            SettingRow { label: t("settings.server.enabled"), hint: t("settings.server.enabled_hint"),
                Switch { checked: prefs.enabled, label: t("settings.server.enabled"), on_toggle: toggle(|p, v| p.enabled = v) }
            }
            NumberRow {
                label: t("settings.server.port"),
                hint: t("settings.server.port_hint"),
                value: u32::from(prefs.port),
                min: 1024,
                max: 65535,
                on_commit: move |v: u32| {
                    if let Some(port) = parse_port(&v.to_string()) {
                        set(Box::new(move |p: &mut ServerPrefs| p.port = port));
                    }
                },
            }
            TextRow {
                label: t("settings.server.name"),
                hint: t("settings.server.name_hint"),
                value: prefs.name.clone(),
                on_commit: move |v: String| set(Box::new(move |p: &mut ServerPrefs| p.name = v.clone())),
            }
            SettingRow { label: t("settings.server.require_pairing"), hint: t("settings.server.require_pairing_hint"),
                Switch { checked: prefs.require_pairing, label: t("settings.server.require_pairing"), on_toggle: toggle(|p, v| p.require_pairing = v) }
            }
            SettingRow { label: t("settings.server.media_proxy"), hint: t("settings.server.media_proxy_hint"),
                Switch { checked: prefs.proxy_enabled, label: t("settings.server.media_proxy"), on_toggle: toggle(|p, v| p.proxy_enabled = v) }
            }
            NumberRow {
                label: t("settings.server.timeout"),
                hint: t("settings.server.timeout_hint"),
                value: u32::from(prefs.timeout_secs),
                min: 5,
                max: 120,
                on_commit: move |v: u32| set(Box::new(move |p: &mut ServerPrefs| p.timeout_secs = v as u8)),
            }
            NumberRow {
                label: t("settings.server.concurrency"),
                hint: t("settings.server.concurrency_hint"),
                value: u32::from(prefs.concurrency),
                min: 1,
                max: 8,
                on_commit: move |v: u32| set(Box::new(move |p: &mut ServerPrefs| p.concurrency = v as u8)),
            }
            div {
                Button {
                    label: t("settings.server.open_page"),
                    variant: ButtonVariant::Tonal,
                    icon: "desktop".to_string(),
                    onclick: move |_| {
                        navigator.push(Route::ServerPage {});
                    },
                }
            }
        }

        h3 { class: "tk-settings__subtitle", {t("settings.server.network")} }
        TestRow {
            label: t("settings.server.flaresolverr"),
            hint: t("settings.server.flaresolverr_hint"),
            placeholder: "http://127.0.0.1:8191".to_string(),
            value: prefs.flaresolverr_url.clone().unwrap_or_default(),
            proxy: false,
            on_commit: move |v: String| {
                let v = v.trim().trim_end_matches('/').to_owned();
                set(Box::new(move |p: &mut ServerPrefs| p.flaresolverr_url = Some(v.clone()).filter(|u| !u.is_empty())));
            },
        }
        TestRow {
            label: t("settings.server.outbound_proxy"),
            hint: t("settings.server.outbound_proxy_hint"),
            placeholder: "socks5h://127.0.0.1:9050".to_string(),
            value: prefs.outbound_proxy.clone().unwrap_or_default(),
            proxy: true,
            on_commit: move |v: String| {
                let v = v.trim().trim_end_matches('/').to_owned();
                set(Box::new(move |p: &mut ServerPrefs| p.outbound_proxy = Some(v.clone()).filter(|u| !u.is_empty())));
            },
        }
        SettingRow { label: t("settings.server.local_proxy"), hint: t("settings.server.local_proxy_hint"),
            Switch { checked: prefs.local_proxy, label: t("settings.server.local_proxy"), on_toggle: toggle(|p, v| p.local_proxy = v) }
        }

        h3 { class: "tk-settings__subtitle", {t("settings.server.sniffer_title")} }
        div { "data-sniffer": "{prefs.sniffer_enabled}", "data-sniffer-timeout": "{prefs.sniffer_timeout_secs}",
            SettingRow { label: t("settings.server.sniffer"), hint: t("settings.server.sniffer_hint"),
                Switch { checked: prefs.sniffer_enabled, label: t("settings.server.sniffer"), on_toggle: toggle(|p, v| p.sniffer_enabled = v) }
            }
            NumberRow {
                label: t("settings.server.sniffer_timeout"),
                hint: tf("settings.server.sniffer_timeout_hint", &[("min", &SNIFF_MIN_SECS.to_string()), ("max", &SNIFF_MAX_SECS.to_string())]),
                value: u32::from(prefs.sniffer_timeout_secs),
                min: u32::from(SNIFF_MIN_SECS),
                max: u32::from(SNIFF_MAX_SECS),
                disabled: !prefs.sniffer_enabled,
                on_commit: move |v: u32| set(Box::new(move |p: &mut ServerPrefs| p.sniffer_timeout_secs = v as u8)),
            }
        }
    }
}

/// A number field committed on Enter / Save, refused outside `min..=max`.
#[component]
fn NumberRow(label: String, hint: String, value: u32, min: u32, max: u32, #[props(default)] disabled: bool, on_commit: EventHandler<u32>) -> Element {
    let mut draft = use_signal(|| None::<String>);
    let text = draft().unwrap_or_else(|| value.to_string());
    let parsed = parse_in_range(&text, min, max);
    let invalid = draft().is_some() && parsed.is_none();
    let commit = use_callback(move |_: ()| {
        if let Some(v) = parse_in_range(&draft().unwrap_or_default(), min, max) {
            on_commit.call(v);
            draft.set(None);
        }
    });
    rsx! {
        SettingRow { label: label.clone(), hint: hint.clone(),
            div { class: "tk-settings__port",
                TextField {
                    label: label.clone(),
                    value: text,
                    mono: true,
                    error: invalid,
                    hint: if invalid { tf("settings.server.range", &[("min", &min.to_string()), ("max", &max.to_string())]) } else { String::new() },
                    disabled,
                    oninput: move |v: String| draft.set(Some(v)),
                    onsubmit: move |_| commit.call(()),
                }
                Button {
                    label: t("common.save"),
                    variant: ButtonVariant::Tonal,
                    size: ButtonSize::Sm,
                    disabled: disabled || draft().is_none() || parsed.is_none(),
                    onclick: move |_| commit.call(()),
                }
            }
        }
    }
}

/// A text field committed on Enter / Save.
#[component]
fn TextRow(label: String, hint: String, value: String, on_commit: EventHandler<String>) -> Element {
    let mut draft = use_signal(|| None::<String>);
    let text = draft().unwrap_or_else(|| value.clone());
    let ok = text.chars().count() <= 60;
    let commit = use_callback(move |_: ()| {
        if let Some(v) = draft().filter(|v| v.chars().count() <= 60) {
            on_commit.call(v);
            draft.set(None);
        }
    });
    rsx! {
        SettingRow { label: label.clone(), hint: hint.clone(),
            div { class: "tk-settings__port",
                TextField {
                    label: label.clone(),
                    value: text,
                    error: !ok,
                    oninput: move |v: String| draft.set(Some(v)),
                    onsubmit: move |_| commit.call(()),
                }
                Button {
                    label: t("common.save"),
                    variant: ButtonVariant::Tonal,
                    size: ButtonSize::Sm,
                    disabled: draft().is_none() || !ok,
                    onclick: move |_| commit.call(()),
                }
            }
        }
    }
}

/// An endpoint field (FlareSolverr or outbound proxy) with Save and "Test" (`Backend::flaresolverr_test`; the
/// proxy test goes through the same call with [`PROXY_TEST_PREFIX`]). The result is shown inline and as
/// `data-flaresolverr` / `data-proxy`.
#[component]
fn TestRow(label: String, hint: String, placeholder: String, value: String, proxy: bool, on_commit: EventHandler<String>) -> Element {
    let services = use_context::<Services>();
    let backend = services.backend.clone();
    let mut draft = use_signal(|| None::<String>);
    let mut testing = use_signal(|| false);
    let mut result = use_signal(|| None::<FlareSolverrVm>);
    let mut error = use_signal(|| None::<UiError>);
    let text = draft().unwrap_or_else(|| value.clone());
    let valid = text.trim().is_empty() || if proxy { proxy_url_ok(text.trim()) } else { endpoint_ok(&text) };

    let commit = use_callback(move |_: ()| {
        if let Some(v) = draft() {
            on_commit.call(v);
            draft.set(None);
        }
    });
    let test = {
        let backend = backend.clone();
        let text = text.clone();
        move |_| {
            if testing() || text.trim().is_empty() {
                return;
            }
            testing.set(true);
            result.set(None);
            error.set(None);
            let arg = if proxy { format!("{PROXY_TEST_PREFIX}{}", text.trim()) } else { text.trim().to_owned() };
            let backend = backend.clone();
            spawn(async move {
                match backend.flaresolverr_test(Some(&arg)).await {
                    Ok(r) => result.set(Some(r)),
                    Err(e) => error.set(Some(e)),
                }
                testing.set(false);
            });
        }
    };
    let state = test_state_attr(testing(), result().map(|r| r.ok).or(error().map(|_| false)));
    let test_ok = state == "ok";
    let outcome = match result() {
        Some(r) if r.ok => {
            let ms = r.latency_ms.map(|m| m.to_string()).unwrap_or_default();
            match r.version {
                Some(v) => tf("settings.server.test_ok_version", &[("version", &v), ("ms", &ms)]),
                None => tf("settings.server.test_ok", &[("ms", &ms)]),
            }
        }
        Some(r) => tf("settings.server.test_failed", &[("error", &r.error.unwrap_or_default())]),
        None => String::new(),
    };
    rsx! {
        SettingRow { label: label.clone(), hint: hint.clone(), stack: true,
            div {
                class: "tk-server__test",
                "data-flaresolverr": if !proxy { state },
                "data-proxy": if proxy { state },
                div { class: "tk-settings__port",
                    TextField {
                        label: label.clone(),
                        value: text.clone(),
                        placeholder,
                        mono: true,
                        error: !valid,
                        hint: if valid { String::new() } else { t("settings.server.url_invalid") },
                        oninput: move |v: String| draft.set(Some(v)),
                        onsubmit: move |_| commit.call(()),
                    }
                    Button {
                        label: t("common.save"),
                        variant: ButtonVariant::Tonal,
                        size: ButtonSize::Sm,
                        disabled: draft().is_none() || !valid,
                        onclick: move |_| commit.call(()),
                    }
                    Button {
                        label: if testing() { t("settings.server.testing") } else { t("settings.server.test") },
                        variant: ButtonVariant::Outlined,
                        size: ButtonSize::Sm,
                        icon: "bolt".to_string(),
                        disabled: testing() || text.trim().is_empty() || !valid,
                        onclick: test,
                    }
                }
                if !outcome.is_empty() {
                    p { class: "tk-server__test-result", "data-ok": "{test_ok}", "{outcome}" }
                }
                if let Some(e) = error() {
                    ErrorState { error: e, compact: true, retry: false }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_and_url_fields() {
        assert_eq!(parse_in_range(" 20 ", 5, 60), Some(20));
        assert_eq!(parse_in_range("4", 5, 60), None);
        assert_eq!(parse_in_range("61", 5, 60), None);
        assert_eq!(parse_in_range("x", 5, 60), None);
        assert!(endpoint_ok("http://127.0.0.1:8191"));
        assert!(!endpoint_ok("127.0.0.1:8191"));
        assert!(!endpoint_ok("http://"));
    }
}
