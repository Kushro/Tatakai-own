//! Extensions: install `.kai` (drop zone, file chooser or path), enable/disable, remove, compatibility
//! pill with an explanatory banner, permission chips, hash / domains / date from
//! `Backend::extension_detail` and a collapsible per-extension log (`Backend::extension_logs`).

use std::time::{SystemTime, UNIX_EPOCH};

use dioxus::prelude::*;

use crate::app::Services;
use crate::backend::{ExtLogLevel, ExtensionVm};
use crate::components::{
    Banner, Button, ButtonSize, ButtonVariant, ConfirmDialog, EmptyState, ErrorState, IconButton, InfoChip,
    LoadingIndicator, Pill, SkeletonLines, Switch, TextField, Tone, use_snackbar,
};
use crate::i18n::{t, tf};
use crate::icons::Icon;
use crate::model::{CompatLevel, ErrorKind, PlatformVm, UiError, is_kai_path};

/// Permission chips shown before collapsing the rest into `+n`.
const PERMS_SHOWN: usize = 6;
const FILE_INPUT_ID: &str = "tk-kai-file";

fn tone(c: CompatLevel) -> Tone {
    match c {
        CompatLevel::Full => Tone::Ok,
        CompatLevel::Partial => Tone::Warn,
        CompatLevel::Unsupported => Tone::Err,
    }
}

/// Icon of a permission chip by its namespace (`network:domain:x`, `storage:*` ...).
pub fn perm_icon(perm: &str) -> &'static str {
    if perm.starts_with("network") {
        "language"
    } else if perm.starts_with("storage") {
        "storage"
    } else {
        "warning"
    }
}

/// `(i18n key, n)` for an age of `diff_secs` (negative ages count as now).
pub fn rel_bucket(diff_secs: i64) -> (&'static str, i64) {
    let d = diff_secs.max(0);
    match d {
        0..60 => ("time.now", 0),
        60..3_600 => ("time.min_ago", d / 60),
        3_600..86_400 => ("time.h_ago", d / 3_600),
        86_400..172_800 => ("time.yesterday", 1),
        _ => ("time.d_ago", d / 86_400),
    }
}

/// "5 min ago", "yesterday" ... (also used by Details for device last-seen).
pub fn rel_time(now_unix: i64, then_unix: i64) -> String {
    let (key, n) = rel_bucket(now_unix - then_unix);
    tf(key, &[("n", &n.to_string())])
}

fn now_unix() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Strip spaces and the quotes Windows adds with "Copy as path" (or a shell adds when pasting a path).
fn clean_path(p: &str) -> String {
    p.trim().trim_matches(['"', '\'']).to_owned()
}

/// Example path for the install field, in the OS's own form.
fn path_placeholder() -> &'static str {
    match PlatformVm::current() {
        PlatformVm::Windows => "C:\\...\\extension.kai",
        PlatformVm::Linux => "/home/…/extension.kai",
        PlatformVm::Mac => "/Users/…/extension.kai",
    }
}

#[component]
pub fn ExtensionsPage() -> Element {
    let services = use_context::<Services>();
    let snackbar = use_snackbar();
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
    let mut over = use_signal(|| false);
    let mut pending_remove = use_signal(|| None::<ExtensionVm>);
    // Toggle / remove failures, shown inline above the list.
    let mut action_error = use_signal(|| None::<UiError>);

    let install = use_callback({
        let backend = backend.clone();
        move |p: String| {
            let p = clean_path(&p);
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
                    Ok(ext) => {
                        path.set(String::new());
                        snackbar.show(tf("ext.install.ok", &[("name", &ext.name)]));
                        list.restart();
                    }
                    Err(e) => install_error.set(Some(e)),
                }
                busy.set(false);
            });
        }
    });

    let installed = match &*list.read() {
        Some(Ok(items)) => Some(items.len()),
        _ => None,
    };

    let body = match &*list.read() {
        None => rsx! { SkeletonLines { widths: vec![100, 100, 100] } },
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
                                action_error.set(None);
                                spawn(async move {
                                    if let Err(e) = backend.set_extension_enabled(&id, enabled).await {
                                        action_error.set(Some(e));
                                    }
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

    let drop_label = t("ext.install");
    rsx! {
        div { class: "tk-page tk-page--ext",
            div { class: "tk-page__head",
                div {
                    h1 { class: "tk-page__title", {t("ext.title")} }
                    p { class: "tk-page__intro tk-muted", {t("ext.intro")} }
                }
                if let Some(n) = installed {
                    Pill { text: tf("ext.installed", &[("n", &n.to_string())]) }
                }
            }
            section { class: "tk-panel tk-install",
                label {
                    class: "tk-drop",
                    r#for: "{FILE_INPUT_ID}",
                    role: "button",
                    tabindex: "0",
                    "aria-label": "{drop_label}",
                    "aria-disabled": "{busy()}",
                    "data-over": "{over()}",
                    ondragenter: move |e| {
                        e.prevent_default();
                        over.set(true);
                    },
                    ondragover: move |e| {
                        e.prevent_default();
                        if !*over.peek() {
                            over.set(true);
                        }
                    },
                    ondragleave: move |_| over.set(false),
                    ondrop: move |e| {
                        e.prevent_default();
                        over.set(false);
                        if busy() {
                            return;
                        }
                        // Desktop drops carry the real file path.
                        match e.data_transfer().files().first() {
                            Some(f) => install.call(f.path().to_string_lossy().into_owned()),
                            None => invalid.set(true),
                        }
                    },
                    onkeydown: move |e| {
                        let activate = match e.key() {
                            Key::Enter => true,
                            Key::Character(c) => c == " ",
                            _ => false,
                        };
                        if activate {
                            e.prevent_default();
                            let _ = document::eval(&format!("const i=document.getElementById('{FILE_INPUT_ID}');if(i){{i.click();}}"));
                        }
                    },
                    if busy() {
                        LoadingIndicator { small: true }
                    } else {
                        Icon { name: "download".to_string(), size: 32 }
                    }
                    b { "{drop_label}" }
                    span { class: "tk-drop__hint", {t("ext.install.drop")} }
                    input {
                        id: "{FILE_INPUT_ID}",
                        r#type: "file",
                        accept: ".kai",
                        hidden: true,
                        disabled: busy(),
                        onchange: move |e| {
                            if let Some(f) = e.files().first() {
                                install.call(f.path().to_string_lossy().into_owned());
                            }
                        },
                    }
                }
                div { class: "tk-install__or", {t("ext.install.or")} }
                div { class: "tk-install__path",
                    TextField {
                        label: t("ext.install.path"),
                        value: path(),
                        icon: "folder".to_string(),
                        mono: true,
                        error: invalid(),
                        hint: if invalid() { t("ext.install.invalid") } else { String::new() },
                        placeholder: path_placeholder().to_string(),
                        oninput: move |v: String| {
                            invalid.set(false);
                            path.set(v);
                        },
                        onsubmit: move |_| install.call(path.peek().clone()),
                    }
                    Button {
                        label: t("ext.install.go"),
                        variant: ButtonVariant::Tonal,
                        disabled: busy() || path().trim().is_empty(),
                        onclick: move |_| install.call(path.peek().clone()),
                    }
                }
                if invalid() && path().trim().is_empty() {
                    p { class: "tk-form__error", role: "alert", {t("ext.install.invalid")} }
                }
                if let Some(err) = install_error() {
                    ErrorState { error: err.clone(), compact: true, on_retry: move |_| install.call(path.peek().clone()) }
                }
            }
            if let Some(err) = action_error() {
                ErrorState { error: err.clone(), compact: true, on_retry: move |_| {
                    action_error.set(None);
                    list.restart();
                } }
            }
            {body}
            if let Some(ext) = pending_remove() {
                ConfirmDialog {
                    title: t("ext.remove.title"),
                    body: tf("ext.remove.body", &[("name", &ext.name)]),
                    confirm_label: t("common.remove"),
                    icon: "trash".to_string(),
                    on_cancel: move |_| pending_remove.set(None),
                    on_confirm: {
                        let backend = backend.clone();
                        move |_| {
                            let (backend, id, name) = (backend.clone(), ext.id.clone(), ext.name.clone());
                            pending_remove.set(None);
                            action_error.set(None);
                            spawn(async move {
                                match backend.remove_extension(&id).await {
                                    Ok(()) => snackbar.show(tf("ext.removed", &[("name", &name)])),
                                    Err(e) => action_error.set(Some(e)),
                                }
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
    let services = use_context::<Services>();
    let mut detail = use_resource({
        let (backend, id) = (services.backend.clone(), ext.id.clone());
        move || {
            let (backend, id) = (backend.clone(), id.clone());
            async move { backend.extension_detail(&id).await }
        }
    });
    let mut show_log = use_signal(|| false);

    let compat_label = t(&format!("compat.{}", ext.compat.id()));
    let can_enable = ext.compat.can_enable();
    let for_remove = ext.clone();
    let initial = ext.name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
    let version = tf("ext.version", &[("v", &ext.version)]);
    let log_label = if show_log() { t("ext.hide_logs") } else { t("ext.logs") };
    let now = now_unix();
    let hash_label = t("ext.hash");

    let foot = match &*detail.read() {
        None => rsx! {
            span { class: "tk-skel tk-skel--line tk-ext__footskel", "aria-hidden": "true" }
        },
        Some(Err(e)) => {
            let retry = e.kind != ErrorKind::NotWired;
            rsx! {
                ErrorState { error: e.clone(), compact: true, retry, on_retry: move |_| detail.restart() }
            }
        }
        Some(Ok(d)) => {
            let domains = d.domains.join(", ");
            rsx! {
                if let Some(ts) = d.updated_unix {
                    span { {tf("ext.updated", &[("when", &rel_time(now, ts))])} }
                }
                if !d.hash.is_empty() {
                    span { "{hash_label} " code { class: "tk-mono", "{d.hash}" } }
                }
                span { title: "{domains}", {tf("ext.domains", &[("n", &d.domains.len().to_string())])} }
                if !d.kind.is_empty() {
                    span { class: "tk-mono", "{d.kind}" }
                }
            }
        }
    };

    rsx! {
        li { class: "tk-ext", "data-compat": "{ext.compat.id()}",
            div { class: "tk-ext__head",
                div { class: "tk-ext__ico", "aria-hidden": "true", "{initial}" }
                div { class: "tk-ext__name",
                    b {
                        span { "{ext.name}" }
                        Pill { text: compat_label, tone: tone(ext.compat) }
                    }
                    span { class: "tk-ext__ver", "{version} · {ext.id}" }
                }
                div { class: "tk-ext__ctl",
                    span { class: "tk-ext__ctllabel", {t("ext.enabled")} }
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
            }
            div { class: "tk-ext__perms",
                span { class: "tk-ext__permlabel", {t("ext.permissions")} }
                if ext.permissions.is_empty() {
                    InfoChip { text: t("ext.no_permissions") }
                }
                for p in ext.permissions.iter().take(PERMS_SHOWN) {
                    InfoChip { key: "{p}", text: p.clone(), icon: perm_icon(p).to_string() }
                }
                if ext.permissions.len() > PERMS_SHOWN {
                    InfoChip { text: format!("+{}", ext.permissions.len() - PERMS_SHOWN) }
                }
            }
            if ext.compat == CompatLevel::Unsupported {
                Banner { text: t("ext.cannot_enable"), icon: "warning".to_string(), tone: Tone::Warn }
            }
            if ext.compat == CompatLevel::Partial {
                Banner { text: t("ext.compat.partial_hint"), icon: "info".to_string() }
            }
            div { class: "tk-ext__foot",
                {foot}
                Button {
                    label: log_label,
                    icon: "terminal".to_string(),
                    variant: ButtonVariant::Text,
                    size: ButtonSize::Xs,
                    class: "tk-ext__logbtn".to_string(),
                    onclick: move |_| show_log.set(!show_log()),
                }
            }
            if show_log() {
                ExtLog { id: ext.id.clone() }
            }
        }
    }
}

/// The extension's log, newest first. Mounted only while expanded, so it loads on open.
#[component]
fn ExtLog(id: String) -> Element {
    let services = use_context::<Services>();
    let mut logs = use_resource({
        let (backend, id) = (services.backend.clone(), id.clone());
        move || {
            let (backend, id) = (backend.clone(), id.clone());
            async move { backend.extension_logs(&id).await }
        }
    });
    let now = now_unix();
    let view = match &*logs.read() {
        None => rsx! { SkeletonLines { widths: vec![90, 70, 80] } },
        Some(Err(e)) => {
            let retry = e.kind != ErrorKind::NotWired;
            rsx! {
                ErrorState { error: e.clone(), compact: true, retry, on_retry: move |_| logs.restart() }
            }
        }
        Some(Ok(lines)) if lines.is_empty() => rsx! {
            p { class: "tk-muted tk-log__empty", {t("ext.logs_empty")} }
        },
        Some(Ok(lines)) => rsx! {
            pre { class: "tk-log",
                for (i , l) in lines.iter().enumerate() {
                    {
                        let level = match l.level {
                            ExtLogLevel::Ok => "ok",
                            ExtLogLevel::Warn => "warn",
                            ExtLogLevel::Err => "err",
                        };
                        let when = l.at_unix.map(|ts| rel_time(now, ts)).unwrap_or_default();
                        rsx! {
                            div { key: "{i}", class: "tk-log__line", "data-level": "{level}",
                                span { class: "tk-log__lvl", "[{level}] " }
                                "{l.op}: {l.message}"
                                if !when.is_empty() {
                                    span { class: "tk-log__at", " · {when}" }
                                }
                            }
                        }
                    }
                }
            }
        },
    };
    view
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_time_buckets() {
        assert_eq!(rel_bucket(-5), ("time.now", 0));
        assert_eq!(rel_bucket(59), ("time.now", 0));
        assert_eq!(rel_bucket(125), ("time.min_ago", 2));
        assert_eq!(rel_bucket(7_200), ("time.h_ago", 2));
        assert_eq!(rel_bucket(90_000), ("time.yesterday", 1));
        assert_eq!(rel_bucket(3 * 86_400 + 5), ("time.d_ago", 3));
    }

    #[test]
    fn permission_icons_and_paths() {
        assert_eq!(perm_icon("network:domain:example.org"), "language");
        assert_eq!(perm_icon("storage:kv"), "storage");
        assert_eq!(perm_icon("torrent:session"), "warning");
        assert_eq!(clean_path("  \"C:\\x\\toko.kai\" "), "C:\\x\\toko.kai");
    }
}
