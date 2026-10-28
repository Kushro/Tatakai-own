//! Extensions: install `.kai`, enable/disable, remove, compatibility badge and permissions.

use dioxus::prelude::*;

use crate::app::Services;
use crate::backend::ExtensionVm;
use crate::components::{
    Button, ButtonVariant, ConfirmDialog, EmptyState, ErrorState, IconButton, Pill, Switch, TextField,
};
use crate::i18n::{t, tf};
use crate::model::{CompatLevel, UiError, is_kai_path};

fn tone(c: CompatLevel) -> &'static str {
    match c {
        CompatLevel::Full => "ok",
        CompatLevel::Partial => "warn",
        CompatLevel::Unsupported => "bad",
    }
}

#[component]
pub fn ExtensionsPage() -> Element {
    let services = use_context::<Services>();
    let backend = services.backend.clone();
    let mut list = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.extensions().await }
        }
    });
    let mut path = use_signal(String::new);
    let mut install_error = use_signal(|| None::<UiError>);
    let mut invalid = use_signal(|| false);
    let mut busy = use_signal(|| false);
    let mut pending_remove = use_signal(|| None::<ExtensionVm>);

    let install = use_callback({
        let backend = backend.clone();
        move |p: String| {
            let p = p.trim().trim_matches('"').to_owned();
            if !is_kai_path(&p) {
                invalid.set(true);
                return;
            }
            invalid.set(false);
            install_error.set(None);
            busy.set(true);
            let backend = backend.clone();
            spawn(async move {
                match backend.install_extension(&p).await {
                    Ok(_) => {
                        path.set(String::new());
                        list.restart();
                    }
                    Err(e) => install_error.set(Some(e)),
                }
                busy.set(false);
            });
        }
    });

    let body = match &*list.read() {
        None => rsx! {
            div { class: "tk-skel tk-skel--list", "aria-hidden": "true" }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), on_retry: move |_| list.restart() }
        },
        Some(Ok(items)) if items.is_empty() => rsx! {
            EmptyState {
                icon: "extensions".to_string(),
                title: t("ext.empty.title"),
                body: t("ext.empty.body"),
            }
        },
        Some(Ok(items)) => rsx! {
            ul { class: "tk-extlist",
                for ext in items.clone() {
                    ExtensionRow {
                        key: "{ext.id}",
                        ext: ext.clone(),
                        on_toggle: {
                            let backend = backend.clone();
                            let id = ext.id.clone();
                            move |enabled: bool| {
                                let (backend, id) = (backend.clone(), id.clone());
                                spawn(async move {
                                    let _ = backend.set_extension_enabled(&id, enabled).await;
                                    list.restart();
                                });
                            }
                        },
                        on_remove: move |e: ExtensionVm| pending_remove.set(Some(e)),
                    }
                }
            }
        },
    };

    rsx! {
        div { class: "tk-page tk-page--ext",
            h1 { class: "tk-page__title", {t("ext.title")} }
            p { class: "tk-setting__hint", {t("ext.intro")} }
            section { class: "tk-panel",
                div { class: "tk-install",
                    label { class: "tk-btn tk-btn--filled tk-btn--md tk-filebtn",
                        span { {t("ext.install")} }
                        input {
                            r#type: "file",
                            accept: ".kai",
                            disabled: busy(),
                            onchange: move |e| {
                                if let Some(f) = e.files().first() {
                                    install.call(f.path().to_string_lossy().into_owned());
                                }
                            },
                        }
                    }
                    TextField {
                        label: t("ext.install.path"),
                        value: path(),
                        placeholder: "C:\\...\\extension.kai".to_string(),
                        oninput: move |v: String| path.set(v),
                        onsubmit: move |_| install.call(path.peek().clone()),
                    }
                    Button {
                        label: t("ext.install.go"),
                        variant: ButtonVariant::Tonal,
                        disabled: busy() || path().trim().is_empty(),
                        onclick: move |_| install.call(path.peek().clone()),
                    }
                }
                if invalid() {
                    p { class: "tk-form__error", role: "alert", {t("ext.install.invalid")} }
                }
                if let Some(err) = install_error() {
                    ErrorState { error: err.clone(), compact: true, on_retry: move |_| install.call(path.peek().clone()) }
                }
            }
            {body}
            if let Some(ext) = pending_remove() {
                ConfirmDialog {
                    title: t("ext.remove.title"),
                    body: tf("ext.remove.body", &[("name", &ext.name)]),
                    confirm_label: t("common.remove"),
                    on_cancel: move |_| pending_remove.set(None),
                    on_confirm: {
                        let backend = backend.clone();
                        move |_| {
                            let (backend, id) = (backend.clone(), ext.id.clone());
                            pending_remove.set(None);
                            spawn(async move {
                                let _ = backend.remove_extension(&id).await;
                                list.restart();
                            });
                        }
                    },
                }
            }
        }
    }
}

#[component]
fn ExtensionRow(
    ext: ExtensionVm,
    #[props(default)] on_toggle: EventHandler<bool>,
    #[props(default)] on_remove: EventHandler<ExtensionVm>,
) -> Element {
    let compat_label = t(&format!("compat.{}", ext.compat.id()));
    let can_enable = ext.compat.can_enable();
    let for_remove = ext.clone();
    rsx! {
        li { class: "tk-ext", "data-compat": "{ext.compat.id()}",
            div { class: "tk-ext__head",
                div { class: "tk-ext__name",
                    span { "{ext.name}" }
                    span { class: "tk-ext__ver", {tf("ext.version", &[("v", &ext.version)])} }
                }
                Pill { text: compat_label, tone: tone(ext.compat).to_string() }
                Switch {
                    checked: ext.enabled && can_enable,
                    label: t("ext.enabled"),
                    disabled: !can_enable,
                    on_toggle,
                }
                IconButton {
                    icon: "trash".to_string(),
                    label: t("ext.remove"),
                    onclick: move |_| on_remove.call(for_remove.clone()),
                }
            }
            div { class: "tk-ext__perms",
                span { class: "tk-ext__permlabel", {t("ext.permissions")} }
                if ext.permissions.is_empty() {
                    span { class: "tk-chip", {t("ext.no_permissions")} }
                }
                for p in ext.permissions.iter().take(8) {
                    span { key: "{p}", class: "tk-chip", "{p}" }
                }
                if ext.permissions.len() > 8 {
                    span { class: "tk-chip", "+{ext.permissions.len() - 8}" }
                }
            }
            if !can_enable {
                p { class: "tk-setting__hint", {t("ext.cannot_enable")} }
            }
        }
    }
}
