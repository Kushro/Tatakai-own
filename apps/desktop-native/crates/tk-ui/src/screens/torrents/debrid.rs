//! Torrents > Debrid (wave A, worker **i**; checklist `sources.debrid.*`): Real-Debrid, TorBox, AllDebrid and
//! Premiumize API keys with validation and account status (`Backend::debrid_accounts`, `set_debrid_key`); once
//! connected, torrent sources show the cached pill and play through the service. Route `/torrents/debrid`.
//!
//! Keys go to the OS keyring through the backend (`tk_core::Secrets`); the page never shows a saved key, only
//! "key saved" and "Remove". "Test and save" validates the key against the service before saving it. The services
//! are listed in the order they are tried (`data-priority`): the first connected one that has a torrent cached
//! plays it. Each card exposes `data-service` and `data-connected`.

use std::collections::HashMap;

use dioxus::prelude::*;

use crate::app::Services;
use crate::backend::{DebridAccountVm, DebridServiceVm, UiResult};
use crate::components::{
    Button, ButtonSize, ButtonVariant, ErrorState, LoadingIndicator, Pill, SkeletonLines, TextField, Tone, use_snackbar,
};
use crate::i18n::{t, tf};
use crate::icons::Icon;
use crate::model::{UiError, format_unix_utc};

/// `YYYY-MM-DD` of the premium end, when known.
fn premium_date(unix: Option<i64>) -> Option<String> {
    unix.filter(|u| *u > 0).map(|u| format_unix_utc(u).chars().take(10).collect())
}

/// Where the user finds the API key of each service.
fn key_page(service: DebridServiceVm) -> &'static str {
    match service {
        DebridServiceVm::RealDebrid => "real-debrid.com/apitoken",
        DebridServiceVm::TorBox => "torbox.app/settings",
        DebridServiceVm::AllDebrid => "alldebrid.com/apikeys",
        DebridServiceVm::Premiumize => "premiumize.me/account",
    }
}

#[component]
pub fn TorrentDebridPage() -> Element {
    let services = use_context::<Services>();
    let backend = services.backend.clone();
    let mut accounts = use_signal(|| None::<UiResult<Vec<DebridAccountVm>>>);

    let reload = use_callback({
        let backend = backend.clone();
        move |_: ()| {
            let backend = backend.clone();
            spawn(async move {
                accounts.set(Some(backend.debrid_accounts().await));
            });
        }
    });
    use_hook(|| reload.call(()));

    let body = match &*accounts.read() {
        None => rsx! {
            div { class: "tk-debrid__loading", "aria-busy": "true",
                SkeletonLines { widths: vec![90, 70, 90, 70] }
            }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), title: Some(t("debrid.error")), on_retry: move |_| reload.call(()) }
        },
        Some(Ok(list)) => {
            let by_service: HashMap<DebridServiceVm, DebridAccountVm> = list.iter().map(|a| (a.service, a.clone())).collect();
            let connected = list.iter().filter(|a| a.connected).count();
            let summary = if connected == 0 { t("debrid.none_connected") } else { tf("debrid.connected_n", &[("n", &connected.to_string())]) };
            rsx! {
                p { class: "tk-debrid__summary", "data-connected-count": "{connected}", "{summary}" }
                div { class: "tk-debrid__list",
                    for (i , service) in DebridServiceVm::ALL.into_iter().enumerate() {
                        ServiceCard {
                            key: "{service.id()}",
                            service: service,
                            priority: i + 1,
                            account: by_service.get(&service).cloned().unwrap_or(DebridAccountVm {
                                service,
                                connected: false,
                                username: None,
                                premium_until_unix: None,
                                error: None,
                            }),
                            on_changed: move |vm: DebridAccountVm| {
                                accounts.with_mut(|a| {
                                    if let Some(Ok(list)) = a {
                                        match list.iter_mut().find(|x| x.service == vm.service) {
                                            Some(slot) => *slot = vm.clone(),
                                            None => list.push(vm.clone()),
                                        }
                                    }
                                });
                            },
                        }
                    }
                }
            }
        }
    };

    rsx! {
        section { class: "tk-debrid", "aria-label": t("torrent.section.debrid"),
            div { class: "tk-tlib__head",
                h2 { class: "tk-tlib__title", {t("torrent.section.debrid")} }
                p { class: "tk-tlib__intro", {t("debrid.intro")} }
            }
            div { class: "tk-panel tk-debrid__how",
                Icon { name: "info".to_string(), size: 20 }
                p { {t("debrid.how")} }
            }
            {body}
        }
    }
}

#[component]
fn ServiceCard(service: DebridServiceVm, priority: usize, account: DebridAccountVm, on_changed: EventHandler<DebridAccountVm>) -> Element {
    let services = use_context::<Services>();
    let snack = use_snackbar();
    let backend = services.backend.clone();
    let mut key = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut failure = use_signal(|| None::<UiError>);

    let submit = use_callback({
        let backend = backend.clone();
        move |value: Option<String>| {
            if busy() {
                return;
            }
            busy.set(true);
            failure.set(None);
            let backend = backend.clone();
            spawn(async move {
                match backend.set_debrid_key(service, value.as_deref()).await {
                    Ok(vm) => {
                        if vm.connected {
                            key.set(String::new());
                            snack.show(tf("debrid.saved", &[("service", service.label())]));
                        } else if value.is_none() {
                            snack.show(tf("debrid.removed", &[("service", service.label())]));
                        }
                        on_changed.call(vm);
                    }
                    Err(e) => failure.set(Some(e)),
                }
                busy.set(false);
            });
        }
    });

    let label = service.label();
    let (status, tone) = if account.connected {
        (t("debrid.status.connected"), Tone::Ok)
    } else if account.error.is_some() {
        (t("debrid.status.rejected"), Tone::Err)
    } else {
        (t("debrid.status.not_connected"), Tone::Neutral)
    };
    let user = account.username.clone().map(|u| tf("debrid.user", &[("user", &u)]));
    let premium = premium_date(account.premium_until_unix).map(|d| tf("debrid.premium_until", &[("date", &d)]));
    let typed = key();
    let can_save = !typed.trim().is_empty() && !busy();
    let hint = tf("debrid.key_hint", &[("page", key_page(service))]);
    let priority_text = tf("debrid.priority", &[("n", &priority.to_string())]);

    rsx! {
        article {
            class: "tk-panel tk-debrid__card",
            "data-service": "{service.id()}",
            "data-connected": "{account.connected}",
            "data-priority": "{priority}",
            header { class: "tk-debrid__head",
                div { class: "tk-debrid__name",
                    Icon { name: "bolt".to_string(), size: 22 }
                    h3 { class: "tk-panel__title", "{label}" }
                }
                span { class: "tk-debrid__priority", "{priority_text}" }
                Pill { text: status, tone: tone }
            }
            if account.connected {
                div { class: "tk-debrid__account",
                    if let Some(u) = user {
                        span { class: "tk-debrid__user", "data-username": "true", "{u}" }
                    }
                    if let Some(p) = premium {
                        span { class: "tk-debrid__premium", "{p}" }
                    }
                    span { class: "tk-debrid__saved", "data-key": "saved",
                        Icon { name: "shield".to_string(), size: 16 }
                        {t("debrid.key_saved")}
                    }
                }
            }
            if let Some(err) = account.error.clone() {
                p { class: "tk-debrid__error", role: "alert", "{err}" }
            }
            div { class: "tk-debrid__form",
                TextField {
                    label: if account.connected { t("debrid.key_replace") } else { t("debrid.key") },
                    value: typed.clone(),
                    kind: "password".to_string(),
                    icon: "shield".to_string(),
                    mono: true,
                    hint: hint,
                    disabled: busy(),
                    oninput: move |v: String| key.set(v),
                    onsubmit: move |_| {
                        let v = key.peek().trim().to_owned();
                        if !v.is_empty() {
                            submit.call(Some(v));
                        }
                    },
                }
                div { class: "tk-debrid__actions",
                    if busy() {
                        LoadingIndicator { small: true }
                    }
                    Button {
                        label: t("debrid.test_save"),
                        icon: "check".to_string(),
                        size: ButtonSize::Sm,
                        disabled: !can_save,
                        onclick: move |_| submit.call(Some(key.peek().trim().to_owned())),
                    }
                    if account.connected {
                        Button {
                            label: t("debrid.test"),
                            icon: "refresh".to_string(),
                            variant: ButtonVariant::Text,
                            size: ButtonSize::Sm,
                            disabled: busy(),
                            onclick: {
                                let backend = backend.clone();
                                move |_| {
                                    let backend = backend.clone();
                                    busy.set(true);
                                    failure.set(None);
                                    spawn(async move {
                                        match backend.debrid_accounts().await {
                                            Ok(list) => {
                                                if let Some(vm) = list.into_iter().find(|a| a.service == service) {
                                                    snack.show(if vm.connected { t("debrid.test_ok") } else { t("debrid.test_failed") });
                                                    on_changed.call(vm);
                                                }
                                            }
                                            Err(e) => failure.set(Some(e)),
                                        }
                                        busy.set(false);
                                    });
                                }
                            },
                        }
                        Button {
                            label: t("debrid.remove"),
                            icon: "trash".to_string(),
                            variant: ButtonVariant::Text,
                            size: ButtonSize::Sm,
                            disabled: busy(),
                            onclick: move |_| submit.call(None),
                        }
                    }
                }
            }
            if let Some(e) = failure() {
                ErrorState { error: e, compact: true, title: Some(tf("debrid.save_failed", &[("service", label)])),
                    on_retry: move |_| {
                        let v = key.peek().trim().to_owned();
                        if !v.is_empty() {
                            submit.call(Some(v));
                        }
                    },
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premium_dates() {
        assert_eq!(premium_date(Some(1_700_000_000)).as_deref(), Some("2023-11-14"));
        assert_eq!(premium_date(Some(0)), None);
        assert_eq!(premium_date(None), None);
        for s in DebridServiceVm::ALL {
            assert!(!key_page(s).is_empty());
        }
    }
}
