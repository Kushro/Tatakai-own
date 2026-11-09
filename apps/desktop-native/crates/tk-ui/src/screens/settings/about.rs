//! Settings > About (wave A, worker **f**; checklist `system.update-check`, `system.licenses`, `system.open-logs`,
//! `system.changelog`; classic `AboutPanel.tsx`, `desktop/services/update-manager.cjs` notice only):
//!
//! * version, Link protocol, license;
//! * "Check for updates" (`Backend::check_updates`, GitHub Releases of `Kushro/Tatakai-own`):
//!   `data-update-state` = `idle | checking | latest | newer | error`; a newer release offers "Open" (release page,
//!   `Backend::open_url`), "Release notes" (same sheet as the licenses, `UpdateCheckVm::notes`) and "Skip this
//!   version" (`Settings::system.skipped_version`, which silences the start-up notice of `tk_ui::tray`);
//! * "Licenses": sheet with `THIRD_PARTY_NOTICES.md` (`Backend::licenses_text`);
//! * "Open logs" (`Backend::open_logs`: today's log file in the system viewer).
//!
//! Split out of `screens/settings.rs` by the wave-A contract (docs/redesign/15-parity-wave-a-contract.md
//! section 2). The page shell (`mod.rs`: index, scroll spy, `SettingsPanel`, `SettingRow`) is frozen.

use dioxus::prelude::*;

use crate::app::{Services, Ui};
use crate::backend::UpdateCheckVm;
use crate::components::{Button, ButtonSize, ButtonVariant, ErrorState, LoadingIndicator, Sheet, use_snackbar};
use crate::i18n::{t, tf};
use crate::model::{SettingsAction, UiError, format_unix_utc, reduce_settings};

/// State of the update check.
#[derive(Debug, Clone, PartialEq)]
pub enum UpdateState {
    Idle,
    Checking,
    Done(UpdateCheckVm),
    Failed(UiError),
}

impl UpdateState {
    /// `data-update-state` value.
    pub fn id(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Checking => "checking",
            Self::Done(vm) if vm.newer => "newer",
            Self::Done(_) => "latest",
            Self::Failed(_) => "error",
        }
    }
}

/// What the sheet shows.
#[derive(Debug, Clone, PartialEq)]
enum SheetKind {
    Licenses,
    Notes { version: String, text: String },
}

#[component]
pub fn AboutSection() -> Element {
    let services = use_context::<Services>();
    let backend = services.backend.clone();
    let ui = use_context::<Ui>();
    let snack = use_snackbar();
    let version = format!("{} · tk-app (Dioxus 0.7)", env!("CARGO_PKG_VERSION"));
    let mut update = use_signal(|| UpdateState::Idle);
    let mut sheet = use_signal(|| None::<SheetKind>);
    let mut licenses = use_signal(|| None::<Result<String, UiError>>);
    let mut error = use_signal(|| None::<UiError>);
    let skipped = ui.settings.read().system.skipped_version.clone();

    let check = {
        let backend = backend.clone();
        move |_: MouseEvent| {
            if *update.peek() == UpdateState::Checking {
                return;
            }
            update.set(UpdateState::Checking);
            let backend = backend.clone();
            spawn(async move {
                match backend.check_updates().await {
                    Ok(vm) => update.set(UpdateState::Done(vm)),
                    Err(e) => update.set(UpdateState::Failed(e)),
                }
            });
        }
    };
    let open_licenses = {
        let backend = backend.clone();
        move |_: MouseEvent| {
            sheet.set(Some(SheetKind::Licenses));
            if matches!(*licenses.peek(), Some(Ok(_))) {
                return;
            }
            licenses.set(None);
            let backend = backend.clone();
            spawn(async move {
                let r = backend.licenses_text().await;
                licenses.set(Some(r));
            });
        }
    };
    let open_logs = {
        let backend = backend.clone();
        move |_: MouseEvent| {
            error.set(None);
            let backend = backend.clone();
            spawn(async move {
                match backend.open_logs().await {
                    Ok(()) => snack.show(t("settings.about.logs_opened")),
                    Err(e) => error.set(Some(e)),
                }
            });
        }
    };
    let open_url = use_callback({
        let backend = backend.clone();
        move |url: String| {
            let backend = backend.clone();
            spawn(async move {
                if let Err(e) = backend.open_url(&url).await {
                    error.set(Some(e));
                }
            });
        }
    });
    let skip = use_callback({
        let backend = backend.clone();
        move |tag: String| {
            let mut system = ui.settings.peek().system.clone();
            system.skipped_version = Some(tag);
            let next = reduce_settings(&ui.settings.peek(), SettingsAction::SetSystem(system));
            ui.commit_settings(&backend, next);
            snack.show(t("update.skipped"));
        }
    });

    let state = update();
    let result = match &state {
        UpdateState::Idle => rsx! {},
        UpdateState::Checking => rsx! {
            LoadingIndicator { small: true }
        },
        UpdateState::Failed(e) => rsx! {
            ErrorState { error: e.clone(), compact: true, title: t("update.failed"), retry: false }
        },
        UpdateState::Done(vm) if vm.newer => {
            let tag = vm.latest.clone().unwrap_or_default();
            let line = tf("update.available", &[("version", &tag)]);
            let url = vm.url.clone();
            let notes = vm.notes.clone();
            let is_skipped = skipped.as_deref().is_some_and(|s| s.trim_start_matches(['v', 'V']) == tag.trim_start_matches(['v', 'V']));
            let tag_for_skip = tag.clone();
            let tag_for_notes = tag.clone();
            rsx! {
                div { class: "tk-update", "data-latest": "{tag}",
                    p { class: "tk-update__line", "{line}" }
                    div { class: "tk-panel__row",
                        if let Some(url) = url {
                            Button {
                                label: t("update.open"),
                                icon: "external".to_string(),
                                variant: ButtonVariant::Tonal,
                                size: ButtonSize::Sm,
                                onclick: move |_| open_url.call(url.clone()),
                            }
                        }
                        if let Some(text) = notes {
                            Button {
                                label: t("update.notes"),
                                icon: "info".to_string(),
                                variant: ButtonVariant::Text,
                                size: ButtonSize::Sm,
                                onclick: move |_| {
                                    sheet.set(Some(SheetKind::Notes { version: tag_for_notes.clone(), text: text.clone() }));
                                },
                            }
                        }
                        if is_skipped {
                            span { class: "tk-setting-row__hint", {t("update.skipped")} }
                        } else {
                            Button {
                                label: t("update.skip"),
                                variant: ButtonVariant::Text,
                                size: ButtonSize::Sm,
                                onclick: move |_| skip.call(tag_for_skip.clone()),
                            }
                        }
                    }
                }
            }
        }
        UpdateState::Done(vm) => {
            let checked = tf("update.checked_at", &[("when", &format_unix_utc(vm.checked_unix))]);
            rsx! {
                div { class: "tk-update",
                    p { class: "tk-update__line", {t("settings.about.uptodate")} }
                    p { class: "tk-setting-row__hint", "{checked}" }
                }
            }
        }
    };

    let sheet_view = match sheet() {
        None => rsx! {},
        Some(kind) => {
            let (title, body) = match kind {
                SheetKind::Licenses => {
                    let body = match licenses() {
                        None => rsx! {
                            LoadingIndicator {}
                        },
                        Some(Err(e)) => rsx! {
                            ErrorState { error: e, compact: true, retry: false }
                        },
                        Some(Ok(text)) => rsx! {
                            pre { class: "tk-licenses", "data-licenses": "true", "{text}" }
                        },
                    };
                    (t("settings.about.licenses"), body)
                }
                SheetKind::Notes { version, text } => {
                    let body = rsx! {
                        pre { class: "tk-licenses", "data-release-notes": "true", "{text}" }
                    };
                    (tf("update.notes_title", &[("version", &version)]), body)
                }
            };
            rsx! {
                Sheet { title, wide: true, on_close: move |_| sheet.set(None), {body} }
            }
        }
    };

    rsx! {
        div { class: "tk-about",
            p { {t("settings.about.body")} }
            dl {
                dt { "Tatakai Next" }
                dd { "{version}" }
                dt { "Tatakai-Link" }
                dd { "v1 · openapi.v1.json" }
                dt { {t("settings.about.license")} }
                dd { "AGPL-3.0-or-later" }
            }
        }
        div { class: "tk-panel__row", "data-update-state": "{state.id()}",
            Button {
                label: t("settings.about.check_updates"),
                icon: "update".to_string(),
                variant: ButtonVariant::Tonal,
                size: ButtonSize::Sm,
                disabled: state == UpdateState::Checking,
                onclick: check,
            }
            Button {
                label: t("settings.about.licenses"),
                icon: "external".to_string(),
                variant: ButtonVariant::Text,
                size: ButtonSize::Sm,
                onclick: open_licenses,
            }
            Button {
                label: t("settings.about.logs"),
                icon: "terminal".to_string(),
                variant: ButtonVariant::Text,
                size: ButtonSize::Sm,
                onclick: open_logs,
            }
        }
        {result}
        if let Some(e) = error() {
            ErrorState { error: e, compact: true, retry: false }
        }
        {sheet_view}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vm(newer: bool) -> UpdateCheckVm {
        UpdateCheckVm { current: "0.1.0".into(), latest: Some("v0.2.0".into()), newer, url: None, notes: None, checked_unix: 0 }
    }

    #[test]
    fn the_state_ids_match_the_acceptance_values() {
        assert_eq!(UpdateState::Idle.id(), "idle");
        assert_eq!(UpdateState::Checking.id(), "checking");
        assert_eq!(UpdateState::Done(vm(true)).id(), "newer");
        assert_eq!(UpdateState::Done(vm(false)).id(), "latest");
        assert_eq!(UpdateState::Failed(UiError::not_wired("x")).id(), "error");
    }
}
