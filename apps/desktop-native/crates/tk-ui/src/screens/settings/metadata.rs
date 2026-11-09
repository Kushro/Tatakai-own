//! Settings > Metadata: provider priority, on / off per provider and their status, and the cache.
//!
//! Split out of `screens/settings.rs` by the wave-A contract (docs/redesign/15-parity-wave-a-contract.md
//! section 2). Wave B (worker **b**; checklist `settings.metadata.extra-providers`, `settings.metadata.provider-status`):
//! the 8 providers of `model::PROVIDERS` are listed in priority order (`data-provider`), each with a switch ("active" =
//! not in `HomePrefs::disabled_providers`; the last active one cannot be switched off) and its status from
//! `Backend::provider_health` (`data-provider-status="ok|error|cooling|off|discontinued"`: last result, the router's
//! pause after a failure with the minutes left, switched off, or dropped because the service no longer answers).

use dioxus::prelude::*;

use super::{SettingRow, provider_mono};
use crate::app::Ui;
use crate::backend::ProviderHealthVm;
use crate::components::{Button, ButtonSize, ButtonVariant, ErrorState, IconButton, IconButtonSize, Switch};
use crate::i18n::{t, tf};
use crate::model::SettingsAction;
use crate::model::home::NO_CLIENT_ID;

/// Display text of a provider status.
pub fn status_text(h: &ProviderHealthVm) -> String {
    match h.status() {
        "discontinued" => t("settings.metadata.status.discontinued"),
        "off" => t("settings.metadata.status.off"),
        "cooling" => tf(
            "settings.metadata.status.cooling",
            &[("n", &h.cooling_minutes().unwrap_or(1).to_string())],
        ),
        "error" => match h.last_error.as_deref() {
            Some(NO_CLIENT_ID) => t("settings.metadata.status.no_client_id"),
            Some(e) => tf("settings.metadata.status.error", &[("error", e)]),
            None => t("settings.metadata.status.ok"),
        },
        _ => t("settings.metadata.status.ok"),
    }
}

/// The health row of `id` with `enabled` taken from the settings just saved (the backend may lag one refresh).
pub fn health_for(list: &[ProviderHealthVm], id: &str, enabled: bool) -> Option<ProviderHealthVm> {
    list.iter().find(|h| h.id == id).cloned().map(|mut h| {
        h.enabled = enabled;
        h
    })
}

#[component]
pub fn MetadataSection(commit_saved: Callback<SettingsAction>) -> Element {
    let ui = use_context::<Ui>();
    let services = use_context::<crate::app::Services>();
    let s = (ui.settings)();
    let mut health = use_resource({
        let backend = services.backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.provider_health().await }
        }
    });
    let list: Vec<ProviderHealthVm> = match &*health.read() {
        Some(Ok(l)) => l.clone(),
        _ => Vec::new(),
    };
    let loading = health.read().is_none();
    let error = match &*health.read() {
        Some(Err(e)) => Some(e.clone()),
        _ => None,
    };
    let prefs = s.home.clone();
    let enabled_count = s
        .provider_priority
        .iter()
        .filter(|p| !prefs.is_provider_disabled(p))
        .count();
    rsx! {
        p { class: "tk-panel__hint", {t("settings.metadata.hint")} }
        p { class: "tk-panel__hint", {t("settings.metadata.extra_hint")} }
        ol { class: "tk-priority", "aria-busy": "{loading}",
            for (i , provider) in s.provider_priority.iter().enumerate() {
                {
                    let name = t(&format!("provider.{provider}"));
                    let sub = t(&format!("provider.{provider}.sub"));
                    let mono = provider_mono(provider);
                    let last = i + 1 == s.provider_priority.len();
                    let enabled = !prefs.is_provider_disabled(provider);
                    let status = health_for(&list, provider, enabled);
                    let (id, prefs) = (provider.clone(), prefs.clone());
                    // The last active provider stays on.
                    let lock = enabled && enabled_count <= 1;
                    rsx! {
                        li {
                            key: "{provider}",
                            class: "tk-priority__row",
                            "data-provider": "{provider}",
                            "data-enabled": "{enabled}",
                            span { class: "tk-priority__rank", "{i + 1}" }
                            span { class: "tk-priority__ico", "aria-hidden": "true", "{mono}" }
                            span { class: "tk-priority__name",
                                "{name}"
                                small { "{sub}" }
                                if let Some(h) = status {
                                    span {
                                        class: "tk-priority__status",
                                        "data-provider-status": "{h.status()}",
                                        title: h.last_error.clone().unwrap_or_default(),
                                        {status_text(&h)}
                                    }
                                } else if !enabled {
                                    span { class: "tk-priority__status", "data-provider-status": "off",
                                        {t("settings.metadata.status.off")}
                                    }
                                } else if loading {
                                    span { class: "tk-priority__status tk-muted", {t("settings.metadata.status.checking")} }
                                }
                            }
                            Switch {
                                checked: enabled,
                                disabled: lock,
                                label: tf("settings.metadata.enable", &[("name", &name)]),
                                on_toggle: move |v: bool| {
                                    commit_saved.call(SettingsAction::SetHome(prefs.clone().with_provider_enabled(&id, v)));
                                    health.restart();
                                },
                            }
                            IconButton {
                                icon: "chevron-up".to_string(),
                                label: tf("settings.metadata.up", &[("name", &name)]),
                                size: IconButtonSize::Sm,
                                disabled: i == 0,
                                onclick: move |_| commit_saved.call(SettingsAction::MoveProvider { index: i, delta: -1 }),
                            }
                            IconButton {
                                icon: "chevron-down".to_string(),
                                label: tf("settings.metadata.down", &[("name", &name)]),
                                size: IconButtonSize::Sm,
                                disabled: last,
                                onclick: move |_| commit_saved.call(SettingsAction::MoveProvider { index: i, delta: 1 }),
                            }
                        }
                    }
                }
            }
        }
        if let Some(e) = error {
            ErrorState { error: e, compact: true, on_retry: move |_| health.restart() }
        }
        SettingRow { label: t("settings.metadata.status.title"),
            Button {
                label: t("settings.metadata.status.refresh"),
                variant: ButtonVariant::Text,
                size: ButtonSize::Sm,
                icon: "refresh".to_string(),
                disabled: loading,
                onclick: move |_| health.restart(),
            }
        }
        SettingRow { label: t("settings.metadata.cache"), hint: t("settings.metadata.cache_hint") }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(id: &str) -> ProviderHealthVm {
        ProviderHealthVm {
            id: id.into(),
            enabled: true,
            cooling_down: false,
            cooling_secs: None,
            kinds: vec![],
            last_ok_unix: None,
            last_error: None,
            discontinued: false,
        }
    }

    #[test]
    fn saved_settings_override_the_enabled_flag() {
        let list = vec![h("mal"), h("kitsu")];
        let x = health_for(&list, "mal", false).unwrap();
        assert_eq!(x.status(), "off");
        assert!(health_for(&list, "tenrai", true).is_none());
        let mut c = h("tenrai");
        c.cooling_down = true;
        c.cooling_secs = Some(30);
        assert_eq!(health_for(&[c], "tenrai", true).unwrap().status(), "cooling");
    }
}
