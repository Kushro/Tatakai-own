//! Settings > Data & storage (wave A, worker **f**; checklist `system.storage-usage`, `system.clear-cache`,
//! `system.open-folder`, `system.export-history`, `system.clear-watch-history`, `system.metadata-cache-control`,
//! `history.link-sync.status`; classic `StorageSettingsPanel.tsx`, `PrivacyPanel.tsx`, `AccountPanel.tsx`):
//!
//! * storage bar (`Backend::storage_usage`, `data-storage-bytes`): caches, torrents, logs, extensions, free space;
//! * "Clear cache" (`clear_cache`, snackbar with the bytes freed), "Open folder" (`open_data_folder`), "Export
//!   history" (`export_history`, JSON = camelCase `HistoryEntry`, or CSV; snackbar with the path);
//! * metadata cache: size, "Empty" and the TTL selector (`Settings::system.metadata_cache_ttl_hours`);
//! * watch history over Tatakai Link: when a paired phone last synced (`data-history-sync-unix`) and which phone,
//!   "Sync now" (re-reads the state: the PC is the server, phones pull / push when they open the app or close the
//!   player), and without a paired phone a short explanation with a link to the Link page;
//! * "Clear watch history" with a confirmation (`clear_watch_history`).
//!
//! Split out of `screens/settings.rs` by the wave-A contract (docs/redesign/15-parity-wave-a-contract.md
//! section 2). The page shell (`mod.rs`: index, scroll spy, `SettingsPanel`, `SettingRow`) is frozen.

use std::time::{SystemTime, UNIX_EPOCH};

use dioxus::prelude::*;

use super::{SettingRow, with_data_dir};
use crate::app::{Route, Services, Ui};
use crate::backend::{DeviceVm, ExportFormatVm, LinkStatusVm, StorageUsageVm};
use crate::components::{
    Button, ButtonSize, ButtonVariant, ConfirmDialog, ErrorState, Pill, Segmented, SkeletonLines, Tone, use_snackbar,
};
use crate::i18n::{t, tf, tn};
use crate::model::system::METADATA_TTL_CHOICES;
use crate::model::{SettingsAction, UiError, format_unix_utc, reduce_settings};
use crate::player_state::format_bytes;

/// Parts of the storage bar: `(id, bytes)`; ids are the `settings.data.usage.<id>` keys and `data-part` values.
pub fn usage_parts(u: &StorageUsageVm) -> [(&'static str, u64); 4] {
    [("cache", u.cache_bytes), ("torrents", u.torrents_bytes), ("logs", u.logs_bytes), ("extensions", u.extensions_bytes)]
}

/// Bytes the app uses in total.
pub fn used_bytes(u: &StorageUsageVm) -> u64 {
    usage_parts(u).iter().fold(0u64, |acc, (_, b)| acc.saturating_add(*b))
}

/// Width (percent, 0..=100) of a part in the bar: against used + free when the free space is known, else against
/// the used total.
pub fn part_percent(bytes: u64, u: &StorageUsageVm) -> f64 {
    let total = used_bytes(u).saturating_add(u.free_bytes.unwrap_or(0));
    if total == 0 { 0.0 } else { (bytes as f64 / total as f64 * 100.0).clamp(0.0, 100.0) }
}

/// The paired phone seen most recently (the one that most likely made the last history request).
pub fn latest_device(status: &LinkStatusVm) -> Option<&DeviceVm> {
    status.devices.iter().max_by_key(|d| (d.online, d.last_seen_unix.unwrap_or(i64::MIN)))
}

/// Label of a TTL choice: whole days as `settings.data.ttl_days`, else `settings.data.ttl_hours`.
pub fn ttl_label(hours: u16) -> String {
    match hours {
        h if h % 24 == 0 => tn("settings.data.ttl_days", usize::from(h / 24)),
        h => tn("settings.data.ttl_hours", usize::from(h)),
    }
}

fn now_unix() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// "5 min ago" for recent times, the UTC date otherwise.
fn when(ts: i64) -> String {
    if now_unix() - ts < 2_592_000 { crate::screens::extensions::rel_time(now_unix(), ts) } else { format_unix_utc(ts) }
}

#[component]
pub fn DataSection() -> Element {
    let services = use_context::<Services>();
    let backend = services.backend.clone();
    let data_hint = with_data_dir(&backend.data_dir_display(), "settings.data.hint_at", "settings.data.hint");
    rsx! {
        p { class: "tk-panel__hint", "{data_hint}" }
        SettingRow { label: t("settings.data.telemetry"), hint: t("settings.data.telemetry_hint"),
            Pill { text: t("settings.data.telemetry_none"), tone: Tone::Ok }
        }
        StorageBlock {}
        MetadataCacheRow {}
        HistorySyncRow {}
        ClearHistoryRow {}
    }
}

/// Usage bar + Clear cache / Open folder / Export history.
#[component]
fn StorageBlock() -> Element {
    let services = use_context::<Services>();
    let backend = services.backend.clone();
    let snack = use_snackbar();
    let mut usage = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.storage_usage().await }
        }
    });
    let mut busy = use_signal(|| None::<&'static str>);
    let mut error = use_signal(|| None::<UiError>);

    let clear = {
        let backend = backend.clone();
        move |_: MouseEvent| {
            if busy().is_some() {
                return;
            }
            busy.set(Some("clear"));
            error.set(None);
            let backend = backend.clone();
            spawn(async move {
                match backend.clear_cache().await {
                    Ok(freed) => snack.show(tf("settings.data.cleared_bytes", &[("size", &format_bytes(freed))])),
                    Err(e) => error.set(Some(e)),
                }
                usage.restart();
                busy.set(None);
            });
        }
    };
    let open_folder = {
        let backend = backend.clone();
        move |_: MouseEvent| {
            error.set(None);
            let backend = backend.clone();
            spawn(async move {
                if let Err(e) = backend.open_data_folder().await {
                    error.set(Some(e));
                }
            });
        }
    };
    let export = use_callback({
        let backend = backend.clone();
        move |format: ExportFormatVm| {
            if busy().is_some() {
                return;
            }
            busy.set(Some("export"));
            error.set(None);
            let backend = backend.clone();
            spawn(async move {
                match backend.export_history(format).await {
                    Ok(path) => snack.show(tf("settings.data.exported", &[("path", &path.display().to_string())])),
                    Err(e) => error.set(Some(e)),
                }
                busy.set(None);
            });
        }
    });

    let bar = match &*usage.read() {
        None => rsx! {
            SkeletonLines { widths: vec![90, 40] }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), compact: true, on_retry: move |_| usage.restart() }
        },
        Some(Ok(u)) => {
            let used = used_bytes(u);
            let used_text = tf("settings.data.used", &[("size", &format_bytes(used))]);
            let free_text = u.free_bytes.map(|f| tf("settings.data.free", &[("size", &format_bytes(f))]));
            let free = u.free_bytes.unwrap_or(0);
            let parts: Vec<(&'static str, u64, String)> =
                usage_parts(u).iter().map(|(id, b)| (*id, *b, format!("width:{:.2}%", part_percent(*b, u)))).collect();
            rsx! {
                div {
                    class: "tk-storage",
                    "data-storage-bytes": "{used}",
                    "data-free-bytes": "{free}",
                    div { class: "tk-storage__head",
                        b { "{used_text}" }
                        if let Some(free) = free_text {
                            span { class: "tk-storage__free", "{free}" }
                        }
                    }
                    div {
                        class: "tk-storage__bar",
                        role: "img",
                        "aria-label": "{used_text}",
                        for (id , _ , style) in parts.iter().cloned() {
                            span {
                                key: "{id}",
                                class: "tk-storage__seg",
                                "data-part": "{id}",
                                style: "{style}",
                            }
                        }
                    }
                    ul { class: "tk-storage__legend",
                        for (id , bytes , _) in parts.iter().cloned() {
                            li { key: "{id}", "data-part": "{id}",
                                span { class: "tk-storage__dot" }
                                span { {t(&format!("settings.data.usage.{id}"))} }
                                span { class: "tk-storage__size", {format_bytes(bytes)} }
                            }
                        }
                    }
                }
            }
        }
    };

    rsx! {
        {bar}
        div { class: "tk-panel__row",
            Button {
                label: t("settings.data.clear_cache"),
                icon: "trash".to_string(),
                variant: ButtonVariant::Outlined,
                size: ButtonSize::Sm,
                disabled: busy().is_some(),
                onclick: clear,
            }
            Button {
                label: t("settings.data.open_folder"),
                icon: "folder".to_string(),
                variant: ButtonVariant::Text,
                size: ButtonSize::Sm,
                onclick: open_folder,
            }
            Button {
                label: t("settings.data.export"),
                icon: "download".to_string(),
                variant: ButtonVariant::Text,
                size: ButtonSize::Sm,
                disabled: busy().is_some(),
                onclick: move |_| export.call(ExportFormatVm::Json),
            }
            Button {
                label: t("settings.data.export_csv"),
                variant: ButtonVariant::Text,
                size: ButtonSize::Sm,
                disabled: busy().is_some(),
                onclick: move |_| export.call(ExportFormatVm::Csv),
            }
        }
        if let Some(e) = error() {
            ErrorState { error: e, compact: true, retry: false }
        }
    }
}

/// Metadata cache: size, Empty, TTL.
#[component]
fn MetadataCacheRow() -> Element {
    let services = use_context::<Services>();
    let backend = services.backend.clone();
    let ui = use_context::<Ui>();
    let snack = use_snackbar();
    let mut stats = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.metadata_cache_usage().await }
        }
    });
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<UiError>);
    let ttl = ui.settings.read().system.metadata_cache_ttl_hours;

    let set_ttl = {
        let backend = services.backend.clone();
        move |id: String| {
            let Ok(hours) = id.parse::<u16>() else { return };
            let mut system = ui.settings.peek().system.clone();
            system.metadata_cache_ttl_hours = hours;
            let next = reduce_settings(&ui.settings.peek(), SettingsAction::SetSystem(system));
            ui.commit_settings(&backend, next);
            snack.show(t("settings.saved"));
        }
    };
    let empty = {
        let backend = backend.clone();
        move |_: MouseEvent| {
            if busy() {
                return;
            }
            busy.set(true);
            error.set(None);
            let backend = backend.clone();
            spawn(async move {
                match backend.clear_metadata_cache().await {
                    Ok(freed) => snack.show(tf("settings.data.cleared_bytes", &[("size", &format_bytes(freed))])),
                    Err(e) => error.set(Some(e)),
                }
                stats.restart();
                busy.set(false);
            });
        }
    };

    let size = match &*stats.read() {
        None => rsx! {
            SkeletonLines { widths: vec![30] }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), compact: true, on_retry: move |_| stats.restart() }
        },
        Some(Ok((rows, bytes))) => {
            let text = tf("settings.data.metadata_size", &[("size", &format_bytes(*bytes)), ("n", &rows.to_string())]);
            rsx! {
                span { class: "tk-meta-cache__size", "data-metadata-cache-bytes": "{bytes}", "{text}" }
            }
        }
    };
    let options: Vec<(String, String)> = METADATA_TTL_CHOICES.iter().map(|h| (h.to_string(), ttl_label(*h))).collect();

    rsx! {
        SettingRow { label: t("settings.data.metadata"), hint: t("settings.data.metadata_hint"), stack: true,
            div { class: "tk-meta-cache", "data-metadata-ttl-hours": "{ttl}",
                {size}
                Button {
                    label: t("settings.data.metadata_empty"),
                    icon: "trash".to_string(),
                    variant: ButtonVariant::Text,
                    size: ButtonSize::Sm,
                    disabled: busy(),
                    onclick: empty,
                }
            }
            div { class: "tk-meta-cache__ttl",
                span { class: "tk-setting-row__hint", {t("settings.data.metadata_ttl")} }
                Segmented {
                    options,
                    selected: ttl.to_string(),
                    label: t("settings.data.metadata_ttl"),
                    small: true,
                    on_select: set_ttl,
                }
            }
            if let Some(e) = error() {
                ErrorState { error: e, compact: true, retry: false }
            }
        }
    }
}

/// Watch history over Tatakai Link: last sync, device, "Sync now", or how to pair a phone.
#[component]
fn HistorySyncRow() -> Element {
    let services = use_context::<Services>();
    let backend = services.backend.clone();
    let navigator = use_navigator();
    let snack = use_snackbar();
    let mut state = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move {
                let status = backend.link_status().await?;
                let last = backend.history_last_sync_unix().await?;
                Ok::<_, UiError>((status, last))
            }
        }
    });

    let body = match &*state.read() {
        None => rsx! {
            SkeletonLines { widths: vec![60, 30] }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), compact: true, on_retry: move |_| state.restart() }
        },
        Some(Ok((status, last))) => {
            if status.devices.is_empty() {
                rsx! {
                    div { class: "tk-history-sync", "data-history-sync-unix": "0", "data-paired": "false",
                        p { class: "tk-setting-row__hint", {t("settings.data.sync_no_device")} }
                        Button {
                            label: t("settings.data.sync_pair"),
                            icon: "link".to_string(),
                            variant: ButtonVariant::Tonal,
                            size: ButtonSize::Sm,
                            onclick: move |_| {
                                navigator.push(Route::LinkPage {});
                            },
                        }
                    }
                }
            } else {
                let device = latest_device(status).map(|d| d.name.clone()).unwrap_or_default();
                let line = match last {
                    Some(ts) => tf("settings.data.sync_last", &[("when", &when(*ts)), ("device", &device)]),
                    None => t("settings.data.sync_never"),
                };
                let n = tn("settings.data.sync_devices", status.devices.len());
                let ts = last.unwrap_or(0);
                rsx! {
                    div { class: "tk-history-sync", "data-history-sync-unix": "{ts}", "data-paired": "true",
                        p { class: "tk-history-sync__line", "{line}" }
                        p { class: "tk-setting-row__hint", "{n}" }
                        Button {
                            label: t("settings.data.sync_now"),
                            icon: "sync".to_string(),
                            variant: ButtonVariant::Tonal,
                            size: ButtonSize::Sm,
                            onclick: move |_| {
                                state.restart();
                                snack.show(t("settings.data.sync_now_hint"));
                            },
                        }
                    }
                }
            }
        }
    };

    rsx! {
        SettingRow { label: t("settings.data.sync"), hint: t("settings.data.sync_hint"), stack: true, {body} }
    }
}

/// "Clear watch history" with a confirmation.
#[component]
fn ClearHistoryRow() -> Element {
    let services = use_context::<Services>();
    let backend = services.backend.clone();
    let snack = use_snackbar();
    let mut confirm = use_signal(|| false);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<UiError>);

    let run = {
        let backend = backend.clone();
        move |_: ()| {
            confirm.set(false);
            busy.set(true);
            error.set(None);
            let backend = backend.clone();
            spawn(async move {
                match backend.clear_watch_history().await {
                    Ok(n) => snack.show(tn("settings.data.history_cleared", n as usize)),
                    Err(e) => error.set(Some(e)),
                }
                busy.set(false);
            });
        }
    };

    rsx! {
        SettingRow { label: t("settings.data.clear_history"), hint: t("settings.data.clear_history_hint"),
            Button {
                label: t("settings.data.clear_history_action"),
                icon: "history".to_string(),
                variant: ButtonVariant::Outlined,
                size: ButtonSize::Sm,
                disabled: busy(),
                onclick: move |_| confirm.set(true),
            }
        }
        if let Some(e) = error() {
            ErrorState { error: e, compact: true, retry: false }
        }
        if confirm() {
            ConfirmDialog {
                title: t("settings.data.clear_history_confirm"),
                body: t("settings.data.clear_history_body"),
                confirm_label: t("settings.data.clear_history_action"),
                icon: "history".to_string(),
                on_confirm: run,
                on_cancel: move |_| confirm.set(false),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage() -> StorageUsageVm {
        StorageUsageVm {
            data_dir: "/d".into(),
            cache_bytes: 100,
            torrents_bytes: 300,
            logs_bytes: 50,
            extensions_bytes: 50,
            free_bytes: Some(500),
        }
    }

    #[test]
    fn the_bar_adds_up_against_used_plus_free() {
        let u = usage();
        assert_eq!(used_bytes(&u), 500);
        assert!((part_percent(300, &u) - 30.0).abs() < 1e-9);
        let no_free = StorageUsageVm { free_bytes: None, ..usage() };
        assert!((part_percent(300, &no_free) - 60.0).abs() < 1e-9);
        assert_eq!(part_percent(0, &StorageUsageVm::default()), 0.0);
        assert_eq!(usage_parts(&u).map(|(id, _)| id), ["cache", "torrents", "logs", "extensions"]);
    }

    #[test]
    fn the_latest_device_prefers_online_then_most_recent() {
        let d = |id: &str, seen: Option<i64>, online: bool| DeviceVm {
            id: id.into(),
            name: id.into(),
            platform: "android".into(),
            last_seen_unix: seen,
            online,
        };
        let mut s = LinkStatusVm { running: true, port: 1, devices: vec![d("a", Some(10), false), d("b", Some(20), false)], error: None };
        assert_eq!(latest_device(&s).unwrap().id, "b");
        s.devices.push(d("c", None, true));
        assert_eq!(latest_device(&s).unwrap().id, "c");
        assert!(latest_device(&LinkStatusVm::default()).is_none());
    }
}
