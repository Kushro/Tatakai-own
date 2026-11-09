//! Settings > System (wave A, worker **f**; checklist `system.autostart`, `system.tray.icon`, `system.reset-app`,
//! `account.setup-wizard`; classic `desktop/ipc/ipc-system.cjs`, `desktop/window/tray.cjs`, `DesktopSettings.tsx`):
//!
//! * start with the system (`Backend::autostart_enabled` / `set_autostart`, `data-autostart` = the real OS state)
//!   and "start minimized in the tray";
//! * tray icon and close-to-tray (`Settings::system`, applied live by `tk_ui::tray`);
//! * check for updates at start-up;
//! * "Open magnet links with Tatakai" (`Backend::magnet_handler_registered` / `set_magnet_handler`, worker i's
//!   backend; only when `system_caps().magnet_handler`, `data-magnet-handler`);
//! * "Run the setup assistant again" (`Route::SetupPage`);
//! * danger zone "Reset app" (`data-reset-app`): `ConfirmDialog` with a typed confirmation word and the "keep
//!   downloaded torrents" switch, then `Backend::reset_app`; the settings in memory become `Settings::default()`
//!   (not saved: the data directory stays empty until the wizard finishes) and the app shows `/setup`.
//!
//! Rows the platform cannot offer (`Backend::system_caps`) are hidden, not disabled.
//!
//! Split out of `screens/settings.rs` by the wave-A contract (docs/redesign/15-parity-wave-a-contract.md
//! section 2). The page shell (`mod.rs`: index, scroll spy, `SettingsPanel`, `SettingRow`) is frozen.

use dioxus::prelude::*;

use super::SettingRow;
use crate::app::{Route, Services, Ui};
use crate::components::{Button, ButtonSize, ButtonVariant, ConfirmDialog, ErrorState, SkeletonLines, Switch, TextField, use_snackbar};
use crate::i18n::{t, tf};
use crate::model::{Settings, SettingsAction, SystemPrefs, UiError, reduce_settings};

/// The typed confirmation matches the word shown in the dialog (case and surrounding spaces do not matter).
pub fn reset_word_matches(typed: &str, word: &str) -> bool {
    let typed = typed.trim();
    !typed.is_empty() && typed.to_lowercase() == word.trim().to_lowercase()
}

#[component]
pub fn SystemSection() -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let navigator = use_navigator();
    let caps = services.backend.system_caps();
    let prefs = ui.settings.read().system.clone();

    // Every switch builds the whole `SystemPrefs` and dispatches it (`SettingsAction::SetSystem`).
    let commit = use_callback({
        let backend = services.backend.clone();
        move |system: SystemPrefs| {
            let next = reduce_settings(&ui.settings.peek(), SettingsAction::SetSystem(system));
            ui.commit_settings(&backend, next);
        }
    });
    let with = move |f: fn(&mut SystemPrefs, bool)| {
        move |v: bool| {
            let mut s = ui.settings.peek().system.clone();
            f(&mut s, v);
            commit.call(s);
        }
    };

    rsx! {
        if caps.autostart {
            AutostartRows { tray: caps.tray }
        }
        if caps.tray {
            SettingRow { label: t("settings.system.tray"), hint: t("settings.system.tray_hint"),
                Switch {
                    checked: prefs.tray_icon,
                    label: t("settings.system.tray"),
                    on_toggle: with(|s, v| s.tray_icon = v),
                }
            }
            SettingRow { label: t("settings.system.close_to_tray"), hint: t("settings.system.close_to_tray_hint"),
                Switch {
                    checked: prefs.tray_icon && prefs.close_to_tray,
                    label: t("settings.system.close_to_tray"),
                    disabled: !prefs.tray_icon,
                    on_toggle: with(|s, v| s.close_to_tray = v),
                }
            }
        }
        SettingRow { label: t("settings.system.check_updates"), hint: t("settings.system.check_updates_hint"),
            Switch {
                checked: prefs.check_updates,
                label: t("settings.system.check_updates"),
                on_toggle: with(|s, v| s.check_updates = v),
            }
        }
        if caps.magnet_handler {
            MagnetRow {}
        }
        SettingRow { label: t("settings.system.rerun_setup"), hint: t("settings.system.rerun_setup_hint"),
            Button {
                label: t("settings.system.rerun_setup"),
                icon: "replay".to_string(),
                variant: ButtonVariant::Tonal,
                size: ButtonSize::Sm,
                onclick: move |_| {
                    navigator.push(Route::SetupPage {});
                },
            }
        }
        ResetZone {}
    }
}

/// "Start with the system" (real OS state) and "Start minimized in the tray".
#[component]
fn AutostartRows(tray: bool) -> Element {
    let services = use_context::<Services>();
    let backend = services.backend.clone();
    let ui = use_context::<Ui>();
    let mut enabled = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.autostart_enabled().await }
        }
    });
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<UiError>);
    let prefs = ui.settings.read().system.clone();

    let toggle = {
        let backend = backend.clone();
        move |on: bool| {
            if busy() {
                return;
            }
            busy.set(true);
            error.set(None);
            let backend = backend.clone();
            spawn(async move {
                match backend.set_autostart(on).await {
                    Ok(()) => {
                        let mut s = ui.settings.peek().system.clone();
                        s.start_with_system = on;
                        let next = reduce_settings(&ui.settings.peek(), SettingsAction::SetSystem(s));
                        ui.commit_settings(&backend, next);
                    }
                    Err(e) => error.set(Some(e)),
                }
                enabled.restart();
                busy.set(false);
            });
        }
    };
    let set_minimized = {
        let backend = backend.clone();
        move |on: bool| {
            let mut s = ui.settings.peek().system.clone();
            s.start_minimized = on;
            let next = reduce_settings(&ui.settings.peek(), SettingsAction::SetSystem(s));
            ui.commit_settings(&backend, next);
        }
    };

    let state = match &*enabled.read() {
        None => None,
        Some(Ok(v)) => Some(Ok(*v)),
        Some(Err(e)) => Some(Err(e.clone())),
    };
    let body = match state {
        None => rsx! {
            SkeletonLines { widths: vec![40] }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e, compact: true, on_retry: move |_| enabled.restart() }
        },
        Some(Ok(on)) => rsx! {
                div { "data-autostart": "{on}",
                    SettingRow { label: t("settings.system.autostart"), hint: t("settings.system.autostart_hint"),
                        Switch {
                            checked: on,
                            label: t("settings.system.autostart"),
                            disabled: busy(),
                            on_toggle: toggle,
                        }
                    }
                    if tray && on {
                        SettingRow { label: t("settings.system.start_minimized"), hint: t("settings.system.start_minimized_hint"),
                            Switch {
                                checked: prefs.start_minimized && prefs.tray_icon,
                                label: t("settings.system.start_minimized"),
                                disabled: !prefs.tray_icon,
                                on_toggle: set_minimized,
                            }
                        }
                    }
                }
        },
    };
    rsx! {
        {body}
        if let Some(e) = error() {
            ErrorState { error: e, compact: true, retry: false }
        }
    }
}

/// "Open magnet links with Tatakai" (worker i's backend).
#[component]
fn MagnetRow() -> Element {
    let services = use_context::<Services>();
    let backend = services.backend.clone();
    let mut registered = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.magnet_handler_registered().await }
        }
    });
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<UiError>);
    let toggle = {
        let backend = backend.clone();
        move |on: bool| {
            if busy() {
                return;
            }
            busy.set(true);
            error.set(None);
            let backend = backend.clone();
            spawn(async move {
                if let Err(e) = backend.set_magnet_handler(on).await {
                    error.set(Some(e));
                }
                registered.restart();
                busy.set(false);
            });
        }
    };
    let state = match &*registered.read() {
        None => None,
        Some(Ok(v)) => Some(Ok(*v)),
        Some(Err(e)) => Some(Err(e.clone())),
    };
    let body = match state {
        None => rsx! {
            SkeletonLines { widths: vec![40] }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e, compact: true, on_retry: move |_| registered.restart() }
        },
        Some(Ok(on)) => rsx! {
                div { "data-magnet-handler": "{on}",
                    SettingRow { label: t("settings.system.magnet_handler"), hint: t("settings.system.magnet_handler_hint"),
                        Switch {
                            checked: on,
                            label: t("settings.system.magnet_handler"),
                            disabled: busy(),
                            on_toggle: toggle,
                        }
                    }
                }
        },
    };
    rsx! {
        {body}
        if let Some(e) = error() {
            ErrorState { error: e, compact: true, retry: false }
        }
    }
}

/// Danger zone: "Reset app".
#[component]
fn ResetZone() -> Element {
    let services = use_context::<Services>();
    let backend = services.backend.clone();
    let ui = use_context::<Ui>();
    let navigator = use_navigator();
    let snack = use_snackbar();
    let mut open = use_signal(|| false);
    let mut keep_downloads = use_signal(|| true);
    let mut typed = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<UiError>);
    let word = t("settings.system.reset_word");

    let run = {
        let backend = backend.clone();
        let word = word.clone();
        move |_: ()| {
            if busy() || !reset_word_matches(&typed.peek(), &word) {
                return;
            }
            busy.set(true);
            error.set(None);
            let keep = keep_downloads();
            let backend = backend.clone();
            spawn(async move {
                match backend.reset_app(keep).await {
                    Ok(()) => {
                        // first-run state in memory; nothing is written until the wizard finishes
                        let mut settings = ui.settings;
                        settings.set(Settings::default());
                        open.set(false);
                        typed.set(String::new());
                        snack.show(t("settings.system.reset_done"));
                        navigator.replace(Route::SetupPage {});
                    }
                    Err(e) => error.set(Some(e)),
                }
                busy.set(false);
            });
        }
    };

    let can_confirm = !busy() && reset_word_matches(&typed(), &word);
    let prompt = tf("settings.system.reset_type", &[("word", &word)]);
    rsx! {
        div { class: "tk-danger", "data-reset-app": "{open()}",
            h3 { class: "tk-danger__title", {t("settings.system.danger")} }
            SettingRow { label: t("settings.system.reset"), hint: t("settings.system.reset_hint"),
                Button {
                    label: t("settings.system.reset"),
                    icon: "warning".to_string(),
                    variant: ButtonVariant::Danger,
                    size: ButtonSize::Sm,
                    onclick: move |_| {
                        error.set(None);
                        typed.set(String::new());
                        open.set(true);
                    },
                }
            }
        }
        if open() {
            ConfirmDialog {
                title: t("settings.system.reset_confirm"),
                body: t("settings.system.reset_body"),
                confirm_label: if busy() { t("settings.system.resetting") } else { t("settings.system.reset") },
                icon: "warning".to_string(),
                confirm_disabled: !can_confirm,
                on_confirm: run,
                on_cancel: move |_| {
                    if !busy() {
                        open.set(false);
                    }
                },
                div { class: "tk-danger__form", "data-reset-word": "{word}",
                    SettingRow { label: t("settings.system.reset_keep_downloads"),
                        Switch {
                            checked: keep_downloads(),
                            label: t("settings.system.reset_keep_downloads"),
                            disabled: busy(),
                            on_toggle: move |v: bool| keep_downloads.set(v),
                        }
                    }
                    TextField {
                        label: prompt,
                        value: typed(),
                        disabled: busy(),
                        oninput: move |v: String| typed.set(v),
                    }
                    if let Some(e) = error() {
                        ErrorState { error: e, compact: true, retry: false }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::reset_word_matches;

    #[test]
    fn the_reset_needs_the_typed_word() {
        assert!(reset_word_matches("RESET", "reset"));
        assert!(reset_word_matches("  borrar ", "BORRAR"));
        assert!(!reset_word_matches("", "reset"));
        assert!(!reset_word_matches("rese", "reset"));
    }
}
