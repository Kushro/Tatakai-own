//! Settings > Accounts: AniList and MyAnimeList connect / disconnect (`Backend::list_connect`), username + avatar, pending changes, last sync, import / export, auto progress and primary service (`SettingsAction::SetLists`).
//!
//! This is the ONLY account UI of the app (plan 14 section 0.1 #9: no own login; the former Supabase "Account"
//! section is gone). Without a configured client id (`TATAKAI_ANILIST_CLIENT_ID` / `TATAKAI_MAL_CLIENT_ID` at build
//! time, or in the environment of a developer build) "Connect" is disabled with the reason. OAuth failures arrive
//! as `ErrorKind::Auth` with a fixed detail code (tk-app `live/lists.rs`) that this section translates
//! (`accounts.error.<code>`): cancelled, invalid state, timeout and expired token read differently. Watch history
//! is not an account feature: it stays local and syncs with the phone over Tatakai Link (Settings > Link).
//!
//! After connecting, a sheet offers the first sync (`lists.import-export`): import the remote list (on
//! disagreement the service wins), export the local list (this PC wins) or merge (disagreements become conflicts
//! to review). Verifier hooks: `[data-service][data-connected]`, `[data-pending-changes]`, `[data-last-sync]`.
//!
//! Split out of `screens/settings.rs` by the wave-A contract (docs/redesign/15-parity-wave-a-contract.md
//! section 2). Owner: worker **g**. The page shell (`mod.rs`: index, scroll spy, `SettingsPanel`, `SettingRow`) is frozen.

use dioxus::core::Task;
use dioxus::prelude::*;

use super::SettingRow;
use crate::app::{Services, Ui};
use crate::backend::{ConflictSideVm, ExportFormatVm, ListAccountVm};
use crate::components::list_button::ConflictSheet;
use crate::components::{
    Button, ButtonSize, ButtonVariant, ConfirmDialog, ErrorState, LoadingIndicator, Segmented, Sheet, SkeletonLines,
    Switch, use_snackbar,
};
use crate::i18n::{t, tf, tn};
use crate::icons::Icon;
use crate::model::{ErrorKind, ListPrefsVm, ListServiceVm, SettingsAction, UiError, reduce_settings};
use crate::platform::now_unix;

/// OAuth error codes the backend sends (`UiError::detail` of `ErrorKind::Auth`), each with `accounts.error.<code>`.
const AUTH_CODES: [&str; 10] = [
    "no_client_id",
    "port",
    "browser",
    "timeout",
    "denied",
    "state",
    "invalid_callback",
    "exchange",
    "expired",
    "not_connected",
];

/// Values of the "count as watched at" control.
const PROGRESS_AT: [u8; 5] = [70, 80, 85, 90, 95];

/// Environment variable with the OAuth client id of `service`.
pub fn client_id_var(service: ListServiceVm) -> &'static str {
    match service {
        ListServiceVm::AniList => "TATAKAI_ANILIST_CLIENT_ID",
        ListServiceVm::Mal => "TATAKAI_MAL_CLIENT_ID",
    }
}

/// Whether this build can connect `service` (same rule as `tk_core::lists::client_id`: runtime variable, else the
/// value baked in at build time).
pub fn client_configured(service: ListServiceVm) -> bool {
    let baked = match service {
        ListServiceVm::AniList => option_env!("TATAKAI_ANILIST_CLIENT_ID"),
        ListServiceVm::Mal => option_env!("TATAKAI_MAL_CLIENT_ID"),
    };
    let set = |v: &str| !v.trim().is_empty();
    std::env::var(client_id_var(service)).is_ok_and(|v| set(&v)) || baked.is_some_and(set)
}

/// i18n key (and args) explaining an account error.
pub fn auth_error_key(e: &UiError) -> (String, Option<String>) {
    if e.kind != ErrorKind::Auth {
        return (e.kind.key().to_owned(), None);
    }
    if let Some(rest) = e.detail.strip_prefix("provider:") {
        return ("accounts.error.provider".to_owned(), Some(rest.to_owned()));
    }
    match AUTH_CODES.iter().find(|c| **c == e.detail) {
        Some(code) => (format!("accounts.error.{code}"), None),
        None => (e.kind.key().to_owned(), None),
    }
}

fn auth_error_text(e: &UiError, service: ListServiceVm) -> String {
    let (key, arg) = auth_error_key(e);
    tf(&key, &[("var", client_id_var(service)), ("error", arg.as_deref().unwrap_or_default()), ("service", service.label())])
}

/// "now" / "5 min ago" / "yesterday" / "3 days ago" for the last sync.
fn ago(now: i64, then: i64) -> String {
    let m = (now - then).abs() / 60;
    if m < 1 {
        t("time.now")
    } else if m < 60 {
        tf("time.min_ago", &[("n", &m.to_string())])
    } else if m < 60 * 24 {
        tf("time.h_ago", &[("n", &((m + 30) / 60).to_string())])
    } else if m < 60 * 48 {
        t("time.yesterday")
    } else {
        tf("time.d_ago", &[("n", &((m + 720) / 1440).to_string())])
    }
}

/// How the first sync after connecting settles disagreements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstSync {
    /// Remote list into this PC: on disagreement the service wins.
    Import,
    /// This PC's list to the service: on disagreement this PC wins.
    Export,
    /// Both ways; disagreements stay as conflicts for the user.
    Merge,
}

impl FirstSync {
    pub const ALL: [FirstSync; 3] = [Self::Import, Self::Export, Self::Merge];

    pub fn id(self) -> &'static str {
        match self {
            Self::Import => "import",
            Self::Export => "export",
            Self::Merge => "merge",
        }
    }

    /// Side every conflict is resolved to after the sync (`None` = the user reviews them).
    pub fn side(self) -> Option<ConflictSideVm> {
        match self {
            Self::Import => Some(ConflictSideVm::Remote),
            Self::Export => Some(ConflictSideVm::Local),
            Self::Merge => None,
        }
    }
}

#[component]
pub fn AccountsSection() -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let snack = use_snackbar();

    let mut accounts = use_resource({
        let backend = services.backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.list_accounts().await }
        }
    });
    let mut conflicts = use_resource({
        let backend = services.backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.list_conflicts().await }
        }
    });
    let connecting = use_signal(|| None::<(ListServiceVm, Task)>);
    let connect_error = use_signal(|| None::<(ListServiceVm, UiError)>);
    let mut first_sync = use_signal(|| None::<ListServiceVm>);
    let first_busy = use_signal(|| false);
    let first_error = use_signal(|| None::<UiError>);
    let mut confirm_disconnect = use_signal(|| None::<ListServiceVm>);
    let syncing = use_signal(|| false);
    let sync_error = use_signal(|| None::<UiError>);
    let mut conflicts_open = use_signal(|| false);
    let busy_file = use_signal(|| false);
    let import_errors = use_signal(Vec::<String>::new);

    let connect = use_callback({
        let backend = services.backend.clone();
        move |s: ListServiceVm| {
            let (mut connecting, mut connect_error, mut accounts, mut first_sync) =
                (connecting, connect_error, accounts, first_sync);
            if connecting.peek().is_some() {
                return;
            }
            connect_error.set(None);
            let backend = backend.clone();
            let task = spawn(async move {
                match backend.list_connect(s).await {
                    Ok(_) => first_sync.set(Some(s)),
                    Err(e) => connect_error.set(Some((s, e))),
                }
                connecting.set(None);
                accounts.restart();
            });
            connecting.set(Some((s, task)));
        }
    });
    let cancel_connect = use_callback(move |_: ()| {
        let mut connecting = connecting;
        let current = *connecting.peek();
        if let Some((_, task)) = current {
            task.cancel();
            connecting.set(None);
        }
    });
    let disconnect = use_callback({
        let backend = services.backend.clone();
        move |s: ListServiceVm| {
            let mut accounts = accounts;
            let backend = backend.clone();
            spawn(async move {
                match backend.list_disconnect(s).await {
                    Ok(()) => snack.show(tf("accounts.disconnected", &[("service", s.label())])),
                    Err(e) => snack.show(format!("{} {}", t("accounts.disconnect_failed"), t(e.kind.key()))),
                }
                accounts.restart();
            });
        }
    });
    let sync = use_callback({
        let backend = services.backend.clone();
        move |_: ()| {
            let (mut syncing, mut sync_error, mut accounts, mut conflicts, mut conflicts_open) =
                (syncing, sync_error, accounts, conflicts, conflicts_open);
            if *syncing.peek() {
                return;
            }
            syncing.set(true);
            sync_error.set(None);
            let backend = backend.clone();
            spawn(async move {
                match backend.list_sync_now().await {
                    Ok(r) => {
                        if r.errors.is_empty() {
                            snack.show(tf(
                                "accounts.sync_done",
                                &[("pushed", &r.pushed.to_string()), ("pulled", &r.pulled.to_string())],
                            ));
                        } else {
                            snack.show(tf("lists.sync_partial", &[("errors", &r.errors.join("; "))]));
                        }
                        if r.conflicts > 0 {
                            conflicts_open.set(true);
                        }
                    }
                    Err(e) => sync_error.set(Some(e)),
                }
                syncing.set(false);
                accounts.restart();
                conflicts.restart();
            });
        }
    });
    let run_first_sync = use_callback({
        let backend = services.backend.clone();
        move |mode: FirstSync| {
            let (mut first_busy, mut first_error, mut first_sync, mut accounts, mut conflicts, mut conflicts_open) =
                (first_busy, first_error, first_sync, accounts, conflicts, conflicts_open);
            if *first_busy.peek() {
                return;
            }
            first_busy.set(true);
            first_error.set(None);
            let backend = backend.clone();
            spawn(async move {
                match backend.list_sync_now().await {
                    Ok(r) => {
                        let mut open_review = r.conflicts > 0;
                        if let Some(side) = mode.side()
                            && r.conflicts > 0
                        {
                            for c in backend.list_conflicts().await.unwrap_or_default() {
                                if let Err(e) = backend.resolve_list_conflict(c.id, side).await {
                                    tracing::warn!(detail = %e.detail, "resolving a first-sync conflict failed");
                                }
                            }
                            open_review = false;
                        }
                        snack.show(tf(
                            "accounts.sync_done",
                            &[("pushed", &r.pushed.to_string()), ("pulled", &r.pulled.to_string())],
                        ));
                        first_sync.set(None);
                        if open_review {
                            conflicts_open.set(true);
                        }
                    }
                    Err(e) => first_error.set(Some(e)),
                }
                first_busy.set(false);
                accounts.restart();
                conflicts.restart();
            });
        }
    });
    let import = use_callback({
        let backend = services.backend.clone();
        move |path: String| {
            let (mut busy_file, mut import_errors, mut accounts) = (busy_file, import_errors, accounts);
            if *busy_file.peek() || path.trim().is_empty() {
                return;
            }
            busy_file.set(true);
            import_errors.set(Vec::new());
            let backend = backend.clone();
            spawn(async move {
                match backend.import_list(&path).await {
                    Ok(r) => {
                        let text = tf(
                            "accounts.imported",
                            &[("n", &r.imported.to_string()), ("skipped", &r.skipped.to_string())],
                        );
                        import_errors.set(r.errors);
                        snack.show_action(text, t("lists.sync_now"), sync);
                    }
                    Err(e) => import_errors.set(vec![format!("{} {}", t(e.kind.key()), e.detail)]),
                }
                busy_file.set(false);
                accounts.restart();
            });
        }
    });
    let export = use_callback({
        let backend = services.backend.clone();
        move |format: ExportFormatVm| {
            let mut busy_file = busy_file;
            if *busy_file.peek() {
                return;
            }
            busy_file.set(true);
            let backend = backend.clone();
            spawn(async move {
                match backend.export_list(format).await {
                    Ok(path) => snack.show(tf("accounts.exported", &[("path", &path.display().to_string())])),
                    Err(e) => snack.show(format!("{} {}", t("accounts.export_failed"), t(e.kind.key()))),
                }
                busy_file.set(false);
            });
        }
    });
    let open_profile = use_callback({
        let backend = services.backend.clone();
        move |url: String| {
            let backend = backend.clone();
            spawn(async move {
                if let Err(e) = backend.open_url(&url).await {
                    snack.show(t(e.kind.key()));
                }
            });
        }
    });

    let prefs = ui.settings.read().lists.clone();
    let set_prefs = use_callback({
        let backend = services.backend.clone();
        move |p: ListPrefsVm| {
            let next = reduce_settings(&ui.settings.peek(), SettingsAction::SetLists(p));
            ui.commit_settings(&backend, next);
            snack.show(t("settings.saved"));
        }
    });

    let now = now_unix();
    let cards = match &*accounts.read() {
        None => rsx! { div { "aria-busy": "true", SkeletonLines { widths: vec![80, 60] } } },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), compact: true, retry: e.kind != ErrorKind::NotWired, on_retry: move |_| accounts.restart() }
        },
        Some(Ok(list)) => rsx! {
            div { class: "tk-accounts",
                for a in list.clone() {
                    AccountCard {
                        key: "{a.service.id()}",
                        account: a.clone(),
                        now,
                        connecting: connecting().is_some_and(|(s, _)| s == a.service),
                        other_connecting: connecting().is_some_and(|(s, _)| s != a.service),
                        error: connect_error().filter(|(s, _)| *s == a.service).map(|(_, e)| e),
                        on_connect: move |s: ListServiceVm| connect.call(s),
                        on_cancel: move |_| cancel_connect.call(()),
                        on_disconnect: move |s: ListServiceVm| confirm_disconnect.set(Some(s)),
                        on_profile: move |u: String| open_profile.call(u),
                    }
                }
            }
        },
    };
    let any_connected = matches!(&*accounts.read(), Some(Ok(list)) if list.iter().any(|a| a.connected));
    let pending_total: u32 = match &*accounts.read() {
        Some(Ok(list)) => list.iter().map(|a| a.pending_changes).sum(),
        _ => 0,
    };
    let n_conflicts = match &*conflicts.read() {
        Some(Ok(c)) => c.len(),
        _ => 0,
    };
    let progress_options: Vec<(String, String)> =
        PROGRESS_AT.iter().map(|p| (p.to_string(), format!("{p} %"))).collect();
    let primary_options: Vec<(String, String)> = std::iter::once(("auto".to_owned(), t("accounts.primary.auto")))
        .chain(ListServiceVm::ALL.iter().map(|s| (s.id().to_owned(), s.label().to_owned())))
        .collect();
    let primary_selected = prefs.primary.map_or("auto", ListServiceVm::id).to_string();
    let (p1, p2, p3, p4) = (prefs.clone(), prefs.clone(), prefs.clone(), prefs.clone());

    rsx! {
        p { class: "tk-panel__hint", {t("accounts.hint")} }
        {cards}
        div { class: "tk-accounts__sync", "data-pending-changes": "{pending_total}",
            SettingRow {
                label: t("lists.sync_now"),
                hint: if any_connected { tn("accounts.pending", pending_total as usize) } else { t("lists.not_connected") },
                Button {
                    label: t("lists.sync_now"),
                    icon: "sync".to_string(),
                    variant: ButtonVariant::Tonal,
                    disabled: !any_connected || syncing(),
                    class: if syncing() { "tk-lib__sync--busy".to_string() } else { String::new() },
                    onclick: move |_| sync.call(()),
                }
            }
            if let Some(e) = sync_error() {
                ErrorState { error: e, compact: true, title: Some(t("lists.sync_now")), on_retry: move |_| sync.call(()) }
            }
            if n_conflicts > 0 {
                SettingRow { label: tn("lists.conflicts.count", n_conflicts), hint: t("accounts.conflicts_hint"),
                    Button {
                        label: t("lists.conflicts.resolve"),
                        icon: "warning".to_string(),
                        variant: ButtonVariant::Outlined,
                        onclick: move |_| conflicts_open.set(true),
                    }
                }
            }
        }
        SettingRow { label: t("accounts.import"), hint: t("accounts.import_hint"),
            label { class: "tk-btn tk-btn--outlined tk-btn--md tk-accounts__file", "aria-disabled": "{busy_file()}",
                Icon { name: "upload".to_string(), size: 18 }
                span { {t("accounts.import_pick")} }
                input {
                    r#type: "file",
                    accept: ".xml,.json",
                    hidden: true,
                    disabled: busy_file(),
                    onchange: move |e| {
                        if let Some(f) = e.files().first() {
                            import.call(f.path().to_string_lossy().into_owned());
                        }
                    },
                }
            }
            if busy_file() {
                LoadingIndicator { small: true }
            }
        }
        if !import_errors().is_empty() {
            ul { class: "tk-accounts__errors", role: "status",
                for (i , e) in import_errors().into_iter().enumerate() {
                    li { key: "{i}", "{e}" }
                }
            }
        }
        SettingRow { label: t("accounts.export"), hint: t("accounts.export_hint"),
            for f in ExportFormatVm::ALL {
                Button {
                    key: "{f.id()}",
                    label: t(&format!("accounts.export.{}", f.id())),
                    icon: "download".to_string(),
                    variant: ButtonVariant::Outlined,
                    disabled: busy_file(),
                    onclick: move |_| export.call(f),
                }
            }
        }
        SettingRow { label: t("accounts.auto_progress"), hint: t("accounts.auto_progress_hint"),
            Switch {
                checked: prefs.auto_progress,
                label: t("accounts.auto_progress"),
                on_toggle: move |v: bool| set_prefs.call(ListPrefsVm { auto_progress: v, ..p1.clone() }),
            }
        }
        SettingRow { label: t("accounts.progress_at"), hint: t("accounts.progress_at_hint"),
            Segmented {
                options: progress_options,
                selected: prefs.progress_at.to_string(),
                label: t("accounts.progress_at"),
                small: true,
                on_select: move |v: String| {
                    if let Ok(at) = v.parse::<u8>() {
                        set_prefs.call(ListPrefsVm { progress_at: at, ..p2.clone() });
                    }
                },
            }
        }
        SettingRow { label: t("accounts.write_back"), hint: t("accounts.write_back_hint"),
            Switch {
                checked: prefs.write_back,
                label: t("accounts.write_back"),
                on_toggle: move |v: bool| set_prefs.call(ListPrefsVm { write_back: v, ..p3.clone() }),
            }
        }
        SettingRow { label: t("accounts.primary"), hint: t("accounts.primary_hint"),
            Segmented {
                options: primary_options,
                selected: primary_selected,
                label: t("accounts.primary"),
                small: true,
                on_select: move |v: String| set_prefs.call(ListPrefsVm { primary: ListServiceVm::from_id(&v), ..p4.clone() }),
            }
        }
        p { class: "tk-panel__hint tk-accounts__history", {t("accounts.history_note")} }

        if let Some(s) = first_sync() {
            Sheet { title: tf("accounts.first.title", &[("service", s.label())]), on_close: move |_| first_sync.set(None),
                div { class: "tk-firstsync", "data-first-sync": "{s.id()}",
                    p { class: "tk-panel__hint", {tf("accounts.first.body", &[("service", s.label())])} }
                    for mode in FirstSync::ALL {
                        button {
                            key: "{mode.id()}",
                            class: "tk-firstsync__opt",
                            r#type: "button",
                            "data-mode": "{mode.id()}",
                            disabled: first_busy(),
                            onclick: move |_| run_first_sync.call(mode),
                            span { class: "tk-firstsync__label", {t(&format!("accounts.first.{}", mode.id()))} }
                            span { class: "tk-firstsync__hint tk-muted",
                                {tf(&format!("accounts.first.{}_hint", mode.id()), &[("service", s.label())])}
                            }
                        }
                    }
                    if first_busy() {
                        LoadingIndicator {}
                    }
                    if let Some(e) = first_error() {
                        ErrorState { error: e, compact: true, retry: false }
                    }
                    Button { label: t("accounts.first.later"), variant: ButtonVariant::Text, onclick: move |_| first_sync.set(None) }
                }
            }
        }
        if let Some(s) = confirm_disconnect() {
            ConfirmDialog {
                title: tf("accounts.disconnect_title", &[("service", s.label())]),
                body: t("accounts.disconnect_body"),
                confirm_label: t("accounts.disconnect"),
                icon: "logout".to_string(),
                on_confirm: move |_| {
                    confirm_disconnect.set(None);
                    disconnect.call(s);
                },
                on_cancel: move |_| confirm_disconnect.set(None),
            }
        }
        if conflicts_open() {
            ConflictSheet {
                on_close: move |_| conflicts_open.set(false),
                on_changed: move |_| {
                    conflicts.restart();
                    accounts.restart();
                },
            }
        }
    }
}

#[component]
fn AccountCard(
    account: ListAccountVm,
    now: i64,
    connecting: bool,
    other_connecting: bool,
    error: Option<UiError>,
    on_connect: EventHandler<ListServiceVm>,
    on_cancel: EventHandler<()>,
    on_disconnect: EventHandler<ListServiceVm>,
    on_profile: EventHandler<String>,
) -> Element {
    let s = account.service;
    let configured = client_configured(s);
    let name = account.username.clone().unwrap_or_else(|| t("accounts.unknown_user"));
    let last = match account.last_sync_unix {
        Some(at) => tf("accounts.last_sync", &[("when", &ago(now, at))]),
        None => t("accounts.never_synced"),
    };
    let last_attr = account.last_sync_unix.map_or(String::new(), |v| v.to_string());
    let profile = account.profile_url.clone();
    let initials: String = s.label().chars().filter(|c| c.is_uppercase()).take(2).collect();
    rsx! {
        div {
            class: "tk-account",
            "data-service": "{s.id()}",
            "data-connected": "{account.connected}",
            "data-connecting": "{connecting}",
            "data-pending-changes": "{account.pending_changes}",
            "data-last-sync": "{last_attr}",
            div { class: "tk-account__avatar", "aria-hidden": "true",
                if let Some(src) = account.avatar.clone().filter(|_| account.connected) {
                    img { src: "{src}", alt: "" }
                } else {
                    span { "{initials}" }
                }
            }
            div { class: "tk-account__text",
                div { class: "tk-account__service", "{s.label()}" }
                if account.connected {
                    div { class: "tk-account__user", "{name}" }
                    div { class: "tk-account__meta tk-muted",
                        "{last}"
                        " · "
                        {tn("accounts.pending", account.pending_changes as usize)}
                    }
                } else if connecting {
                    div { class: "tk-account__meta tk-muted", {tf("accounts.waiting_browser", &[("service", s.label())])} }
                } else if !configured {
                    div { class: "tk-account__meta tk-account__reason", role: "note",
                        {tf("accounts.error.no_client_id", &[("var", client_id_var(s)), ("service", s.label())])}
                    }
                } else {
                    div { class: "tk-account__meta tk-muted", {t("accounts.not_connected")} }
                }
                if let Some(e) = &error {
                    div { class: "tk-account__error", role: "alert", "data-error": "{e.detail}", {auth_error_text(e, s)} }
                }
            }
            div { class: "tk-account__actions",
                if account.connected {
                    if let Some(url) = profile {
                        Button {
                            label: t("accounts.profile"),
                            icon: "external".to_string(),
                            variant: ButtonVariant::Text,
                            size: ButtonSize::Sm,
                            onclick: move |_| on_profile.call(url.clone()),
                        }
                    }
                    Button {
                        label: t("accounts.disconnect"),
                        icon: "logout".to_string(),
                        variant: ButtonVariant::Outlined,
                        size: ButtonSize::Sm,
                        onclick: move |_| on_disconnect.call(s),
                    }
                } else if connecting {
                    LoadingIndicator { small: true }
                    Button { label: t("common.cancel"), variant: ButtonVariant::Text, size: ButtonSize::Sm, onclick: move |_| on_cancel.call(()) }
                } else {
                    Button {
                        label: tf("accounts.connect", &[("service", s.label())]),
                        icon: "user".to_string(),
                        variant: ButtonVariant::Filled,
                        size: ButtonSize::Sm,
                        disabled: !configured || other_connecting,
                        onclick: move |_| on_connect.call(s),
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_codes_map_to_their_own_keys() {
        let e = |d: &str| UiError::new(ErrorKind::Auth, d);
        assert_eq!(auth_error_key(&e("timeout")).0, "accounts.error.timeout");
        assert_eq!(auth_error_key(&e("denied")).0, "accounts.error.denied");
        assert_eq!(auth_error_key(&e("state")).0, "accounts.error.state");
        assert_eq!(auth_error_key(&e("expired")).0, "accounts.error.expired");
        assert_eq!(
            auth_error_key(&e("provider:server_error")),
            ("accounts.error.provider".to_owned(), Some("server_error".to_owned()))
        );
        assert_eq!(auth_error_key(&e("weird")).0, "error.auth");
        assert_eq!(auth_error_key(&UiError::new(ErrorKind::Offline, "x")).0, "error.offline");
    }

    #[test]
    fn every_auth_code_has_a_text() {
        for code in AUTH_CODES.iter().copied().chain(["provider"]) {
            let key = format!("accounts.error.{code}");
            assert_ne!(t(&key), key, "missing {key}");
        }
    }

    #[test]
    fn first_sync_modes() {
        assert_eq!(FirstSync::Import.side(), Some(ConflictSideVm::Remote));
        assert_eq!(FirstSync::Export.side(), Some(ConflictSideVm::Local));
        assert_eq!(FirstSync::Merge.side(), None);
        assert_eq!(client_id_var(ListServiceVm::Mal), "TATAKAI_MAL_CLIENT_ID");
    }
}
