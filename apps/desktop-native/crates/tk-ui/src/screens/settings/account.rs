//! Settings > Account: Supabase sign in / out and sync; wave A adds sign-up, the 6-digit verification code, password recovery and a persisted session (`Backend::sign_up`, `verify_email`, `request_password_reset`, `reset_password`).
//!
//! Split out of `screens/settings.rs` by the wave-A contract (docs/redesign/15-parity-wave-a-contract.md
//! section 2). Owner: worker **f**. The page shell (`mod.rs`: index, scroll spy, `SettingsPanel`, `SettingRow`) is frozen.

use dioxus::prelude::*;

use super::with_data_dir;
use crate::app::Services;
use crate::backend::AccountVm;
use crate::components::{Button, ButtonSize, ButtonVariant, ErrorState, SkeletonLines, TextField, use_snackbar};
use crate::i18n::{t, tf};
use crate::model::UiError;

#[component]
pub fn AccountSection() -> Element {
    let services = use_context::<Services>();
    let backend = services.backend.clone();
    let snack = use_snackbar();
    let mut account = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.account().await }
        }
    });
    let mut email = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<UiError>);
    let mut syncing = use_signal(|| false);
    let mut sync_error = use_signal(|| None::<UiError>);

    let sign_in = use_callback({
        let backend = backend.clone();
        move |_: ()| {
            if busy() {
                return;
            }
            let (e, p) = (email.peek().trim().to_owned(), password.peek().clone());
            if e.is_empty() || p.is_empty() {
                return;
            }
            busy.set(true);
            error.set(None);
            let backend = backend.clone();
            spawn(async move {
                match backend.sign_in(&e, &p).await {
                    Ok(_) => {
                        password.set(String::new());
                        account.restart();
                    }
                    Err(err) => error.set(Some(err)),
                }
                busy.set(false);
            });
        }
    });
    let sign_out = {
        let backend = backend.clone();
        move |_| {
            let backend = backend.clone();
            spawn(async move {
                let _ = backend.sign_out().await;
                account.restart();
            });
        }
    };
    let sync_now = {
        let backend = backend.clone();
        move |_| {
            if syncing() {
                return;
            }
            syncing.set(true);
            sync_error.set(None);
            let backend = backend.clone();
            spawn(async move {
                match backend.sync_now().await {
                    Ok(()) => snack.show(tf("account.synced", &[("when", &t("time.now"))])),
                    Err(e) => sync_error.set(Some(e)),
                }
                syncing.set(false);
            });
        }
    };

    let loaded: Option<Result<AccountVm, UiError>> = account.read().clone();

    match loaded {
        None => rsx! {
            SkeletonLines { widths: vec![60, 40] }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e, compact: true, on_retry: move |_| account.restart() }
        },
        Some(Ok(a)) if !a.configured => {
            let text = with_data_dir(&backend.data_dir_display(), "account.not_configured_at", "account.not_configured");
            rsx! {
                p { class: "tk-panel__hint", "{text}" }
            }
        }
        Some(Ok(AccountVm { email: Some(mail), .. })) => {
            let initial = mail.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
            rsx! {
                div { class: "tk-account",
                    div { class: "tk-account__avatar", "aria-hidden": "true", "{initial}" }
                    div { class: "tk-account__text",
                        div { class: "tk-title-md", {tf("account.signed_in_as", &[("email", &mail)])} }
                        div { class: "tk-body-sm tk-muted", {t("account.lists")} }
                    }
                    Button {
                        label: t("account.sync_now"),
                        icon: "sync".to_string(),
                        variant: ButtonVariant::Tonal,
                        size: ButtonSize::Sm,
                        disabled: syncing(),
                        onclick: sync_now,
                    }
                    Button {
                        label: t("account.sign_out"),
                        icon: "logout".to_string(),
                        variant: ButtonVariant::Outlined,
                        size: ButtonSize::Sm,
                        onclick: sign_out,
                    }
                }
                if let Some(e) = sync_error() {
                    ErrorState { error: e, compact: true, retry: false }
                }
            }
        }
        Some(Ok(_)) => rsx! {
            h3 { class: "tk-setting__label", {t("account.title")} }
            p { class: "tk-panel__hint", {t("account.body")} }
            div { class: "tk-form",
                TextField {
                    label: t("account.email"),
                    value: email(),
                    kind: "email".to_string(),
                    icon: "user".to_string(),
                    oninput: move |v: String| email.set(v),
                    onsubmit: move |_| sign_in.call(()),
                }
                TextField {
                    label: t("account.password"),
                    value: password(),
                    kind: "password".to_string(),
                    oninput: move |v: String| password.set(v),
                    onsubmit: move |_| sign_in.call(()),
                }
                if let Some(err) = error() {
                    p { class: "tk-form__error", role: "alert", {t(err.kind.key())} }
                }
                div {
                    Button {
                        label: if busy() { t("account.signing_in") } else { t("account.sign_in") },
                        disabled: busy(),
                        onclick: move |_| sign_in.call(()),
                    }
                }
            }
        },
    }
}
