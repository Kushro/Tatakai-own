//! Settings: one page with sections (appearance, player, metadata priority, account, Link, about).

use dioxus::prelude::*;
use tk_player::ShaderPreset;

use crate::app::{Route, Services, Ui};
use crate::backend::AccountVm;
use crate::components::{Button, ButtonVariant, IconButton, Segmented, TextField};
use crate::i18n::{LangChoice, t, tf};
use crate::model::{
    Contrast, PALETTES, SettingsAction, ThemeMode, UiError, reduce_settings,
};

fn seg<I: Copy>(items: &[I], id: impl Fn(I) -> &'static str, key: &str) -> Vec<(String, String)> {
    items.iter().map(|i| (id(*i).to_owned(), t(&format!("{key}.{}", id(*i))))).collect()
}

#[component]
pub fn SettingsPage() -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let navigator = use_navigator();
    let backend = services.backend.clone();

    let commit = use_callback({
        let backend = backend.clone();
        move |action: SettingsAction| {
            let next = reduce_settings(&ui.settings.peek(), action);
            ui.commit_settings(&backend, next);
        }
    });

    let s = (ui.settings)();

    rsx! {
        div { class: "tk-page tk-page--settings",
            h1 { class: "tk-page__title", {t("settings.title")} }

            section { class: "tk-panel",
                h2 { class: "tk-panel__title", {t("settings.appearance")} }
                div { class: "tk-setting",
                    div { class: "tk-setting__label", {t("settings.palette")} }
                    div { class: "tk-swatches", role: "radiogroup", "aria-label": t("settings.palette"),
                        for p in PALETTES {
                            {
                                let label = t(&format!("palette.{}", p.id));
                                let selected = s.palette == p.id;
                                rsx! {
                                    button {
                                        key: "{p.id}",
                                        class: "tk-swatch",
                                        r#type: "button",
                                        role: "radio",
                                        "aria-checked": "{selected}",
                                        "aria-label": "{label}",
                                        title: "{label}",
                                        "data-selected": "{selected}",
                                        onclick: move |_| commit.call(SettingsAction::SetPalette(p.id.to_owned())),
                                        span { class: "tk-swatch__dot", style: "background-color:{p.seed}" }
                                        span { class: "tk-swatch__name", "{label}" }
                                    }
                                }
                            }
                        }
                    }
                }
                div { class: "tk-setting",
                    div { class: "tk-setting__label", {t("settings.mode")} }
                    Segmented {
                        options: seg(&ThemeMode::ALL, ThemeMode::id, "mode"),
                        selected: s.mode.id().to_owned(),
                        label: t("settings.mode"),
                        on_select: move |id: String| {
                            if let Some(m) = ThemeMode::ALL.into_iter().find(|m| m.id() == id) {
                                commit.call(SettingsAction::SetMode(m));
                            }
                        },
                    }
                }
                div { class: "tk-setting",
                    div { class: "tk-setting__label", {t("settings.contrast")} }
                    Segmented {
                        options: seg(&Contrast::ALL, Contrast::id, "contrast"),
                        selected: s.contrast.id().to_owned(),
                        label: t("settings.contrast"),
                        on_select: move |id: String| {
                            if let Some(c) = Contrast::ALL.into_iter().find(|c| c.id() == id) {
                                commit.call(SettingsAction::SetContrast(c));
                            }
                        },
                    }
                }
                div { class: "tk-setting",
                    div { class: "tk-setting__label", {t("settings.language")} }
                    Segmented {
                        options: seg(&LangChoice::ALL, LangChoice::id, "lang"),
                        selected: s.language.id().to_owned(),
                        label: t("settings.language"),
                        on_select: move |id: String| {
                            if let Some(l) = LangChoice::ALL.into_iter().find(|l| l.id() == id) {
                                commit.call(SettingsAction::SetLanguage(l));
                            }
                        },
                    }
                }
            }

            section { class: "tk-panel",
                h2 { class: "tk-panel__title", {t("settings.player")} }
                div { class: "tk-setting",
                    div { class: "tk-setting__label", {t("settings.player.preset")} }
                    p { class: "tk-setting__hint", {t("settings.player.preset_hint")} }
                    Segmented {
                        options: ShaderPreset::ALL
                            .iter()
                            .map(|p| (p.id().to_owned(), t(&format!("preset.{}", p.id()))))
                            .collect::<Vec<_>>(),
                        selected: s.shader_preset.clone(),
                        label: t("settings.player.preset"),
                        on_select: move |id: String| {
                            if let Some(p) = ShaderPreset::from_id(&id) {
                                commit.call(SettingsAction::SetPreset(p));
                            }
                        },
                    }
                }
            }

            section { class: "tk-panel",
                h2 { class: "tk-panel__title", {t("settings.metadata")} }
                p { class: "tk-setting__hint", {t("settings.metadata.hint")} }
                ol { class: "tk-priority",
                    for (i , provider) in s.provider_priority.iter().enumerate() {
                        {
                            let name = t(&format!("provider.{provider}"));
                            let last = i + 1 == s.provider_priority.len();
                            rsx! {
                                li { key: "{provider}", class: "tk-priority__row",
                                    span { class: "tk-priority__rank", "{i + 1}" }
                                    span { class: "tk-priority__name", "{name}" }
                                    IconButton {
                                        icon: "chevron-up".to_string(),
                                        label: tf("settings.metadata.up", &[("name", &name)]),
                                        disabled: i == 0,
                                        onclick: move |_| commit.call(SettingsAction::MoveProvider { index: i, delta: -1 }),
                                    }
                                    IconButton {
                                        icon: "chevron-down".to_string(),
                                        label: tf("settings.metadata.down", &[("name", &name)]),
                                        disabled: last,
                                        onclick: move |_| commit.call(SettingsAction::MoveProvider { index: i, delta: 1 }),
                                    }
                                }
                            }
                        }
                    }
                }
            }

            AccountSection {}

            section { class: "tk-panel",
                h2 { class: "tk-panel__title", {t("settings.link")} }
                p { class: "tk-setting__hint", {t("settings.link.hint")} }
                Button {
                    label: t("settings.link.manage"),
                    variant: ButtonVariant::Tonal,
                    icon: "link".to_string(),
                    onclick: move |_| {
                        navigator.push(Route::LinkPage {});
                    },
                }
            }

            section { class: "tk-panel",
                h2 { class: "tk-panel__title", {t("settings.about")} }
                p { class: "tk-setting__hint", {t("settings.about.body")} }
            }
        }
    }
}

#[component]
fn AccountSection() -> Element {
    let services = use_context::<Services>();
    let backend = services.backend.clone();
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

    let acct: Option<AccountVm> = account.read().as_ref().and_then(|r| r.as_ref().ok()).cloned();

    rsx! {
        section { class: "tk-panel",
            h2 { class: "tk-panel__title", {t("settings.account")} }
            match acct {
                None => rsx! {
                    p { class: "tk-setting__hint", {t("common.loading")} }
                },
                Some(a) if !a.configured => rsx! {
                    p { class: "tk-setting__hint", {t("account.not_configured")} }
                },
                Some(AccountVm { email: Some(mail), .. }) => rsx! {
                    p { class: "tk-setting__hint", {tf("account.signed_in_as", &[("email", &mail)])} }
                    Button { label: t("account.sign_out"), variant: ButtonVariant::Outlined, onclick: sign_out }
                },
                Some(_) => rsx! {
                    h3 { class: "tk-setting__label", {t("account.title")} }
                    p { class: "tk-setting__hint", {t("account.body")} }
                    div { class: "tk-form",
                        TextField {
                            label: t("account.email"),
                            value: email(),
                            kind: "email".to_string(),
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
                        Button {
                            label: if busy() { t("account.signing_in") } else { t("account.sign_in") },
                            disabled: busy(),
                            onclick: move |_| sign_in.call(()),
                        }
                    }
                },
            }
        }
    }
}
