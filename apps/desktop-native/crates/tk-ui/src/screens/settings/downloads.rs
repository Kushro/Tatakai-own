//! Settings > Downloads (wave B, worker **h**; checklist `downloads.location`, `downloads.prefs`,
//! `downloads.storage-limit`, `downloads.auto.profiles`, `downloads.auto.schedule`; classic `DesktopSettings.tsx`,
//! `StorageSettingsPanel.tsx`, `AutomationProfiles.tsx`): folder (`data-download-dir`, typed path or a native folder
//! picker through `<input type="file" webkitdirectory>`, reset to the default, open), concurrent / speed / storage
//! limit (`data-storage-limit-reached`), preferred server + audio, auto-download toggle + profile + interval +
//! "Check now" / "Simulate" (`auto_download_check_now`, `data-auto-download`), delete-after-watching, library
//! folders. Every change is one `SettingsAction::SetDownloads` with the whole struct.

use std::time::Duration;

use dioxus::prelude::*;

use super::{SettingRow, lang_seg};
use crate::app::{Services, Ui};
use crate::backend::{AutoDownloadRunVm, BackendRef, DownloadStorageVm, ExtensionVm, ListStatusVm, UiResult};
use crate::components::{
    Button, ButtonSize, ButtonVariant, EmptyState, ErrorState, IconButton, IconButtonSize, LoadingIndicator, Segmented,
    SkeletonLines, Switch, TextField, WavyProgress,
};
use crate::i18n::{t, tf, tn};
use crate::model::downloads::{
    AUTO_MAX_PER_TITLE, AutoProfileVm, CONCURRENT_MAX, CONCURRENT_MIN, DownloadPrefs, OPEN_ROOT_FOLDER, format_bytes,
    format_when, have_episodes, is_aired, plan_new_episodes, storage_fraction, storage_limit_reached,
};
use crate::model::{DUB_LANGS, SettingsAction, UiError, format_unix_utc};
use crate::screens::downloads::AutoCheckNow;

const DIR_INPUT_ID: &str = "tk-set-download-dir";
const LIB_INPUT_ID: &str = "tk-set-library-dir";
/// Auto-download intervals offered (minutes).
pub const AUTO_INTERVALS: [u16; 6] = [15, 30, 60, 180, 720, 1440];
/// "Delete after watching" choices (days, 0 = never).
pub const DELETE_DAYS: [u16; 5] = [0, 1, 7, 14, 30];

/// `"1024"` -> 1024; empty = 0; anything else = `None`.
pub fn parse_number(text: &str) -> Option<u32> {
    let t = text.trim();
    if t.is_empty() { Some(0) } else { t.parse::<u32>().ok() }
}

/// Library folders with `dir` added once (trimmed); unchanged for a blank path.
pub fn with_dir(dirs: &[String], dir: &str) -> Vec<String> {
    let dir = dir.trim();
    let mut out = dirs.to_vec();
    if !dir.is_empty() && !out.iter().any(|d| d == dir) {
        out.push(dir.to_owned());
    }
    out
}

/// One line of the "Simulate" result: `(title, episodes that would be queued)`.
pub type SimLine = (String, Vec<u32>);

/// What the auto-downloader would queue now (no side effects): the same rule as the backend pass, computed from
/// the Watching list, the episode lists and the queue.
async fn simulate(backend: &BackendRef) -> UiResult<Vec<SimLine>> {
    let entries = backend.list_entries(Some(ListStatusVm::Watching)).await?;
    let rows = backend.downloads().await.unwrap_or_default();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let today = format_unix_utc(now).get(..10).unwrap_or_default().to_owned();
    let mut out = Vec::new();
    for e in entries {
        let Ok(eps) = backend.episodes(&e.media.id).await else { continue };
        let aired: Vec<u32> = eps.iter().filter(|x| is_aired(x.air_date.as_deref(), &today)).map(|x| x.number).collect();
        let have = have_episodes(&rows, &e.media.id);
        let plan = plan_new_episodes(e.progress, &aired, &have, AUTO_MAX_PER_TITLE);
        if !plan.is_empty() {
            out.push((e.media.title.clone(), plan));
        }
    }
    Ok(out)
}

#[component]
pub fn DownloadsSection(commit_saved: Callback<SettingsAction>) -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let backend = services.backend.clone();
    let prefs = ui.settings.read().downloads.clone();
    let set = move |p: DownloadPrefs| commit_saved.call(SettingsAction::SetDownloads(p.sanitized()));
    // The saved prefs as a value (no read guard may be alive while `set` writes the settings signal).
    let cur = move || ui.settings.peek().downloads.clone();

    // Re-read when the folder / cap change: the backend applies the prefs on save, hence the short wait.
    let mut storage = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            let key = {
                let s = ui.settings.read();
                (s.downloads.dir.clone(), s.downloads.storage_limit_gib)
            };
            async move {
                drop(key);
                tokio::time::sleep(Duration::from_millis(300)).await;
                backend.download_storage().await
            }
        }
    });
    let exts = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.extensions().await }
        }
    });
    let mut last_run = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.auto_download_last_run().await }
        }
    });
    let mut dir_draft = use_signal(|| None::<String>);
    let mut speed_draft = use_signal(|| None::<String>);
    let mut limit_draft = use_signal(|| None::<String>);
    let mut lib_draft = use_signal(String::new);
    let mut sim = use_signal(|| None::<UiResult<Vec<SimLine>>>);
    let mut sim_busy = use_signal(|| false);
    let mut open_error = use_signal(|| None::<UiError>);

    let run_sim = use_callback({
        let backend = backend.clone();
        move |_: ()| {
            if sim_busy() {
                return;
            }
            sim_busy.set(true);
            let backend = backend.clone();
            spawn(async move {
                sim.set(Some(simulate(&backend).await));
                sim_busy.set(false);
            });
        }
    });
    let open_dir = use_callback({
        let backend = backend.clone();
        move |_: ()| {
            let backend = backend.clone();
            spawn(async move {
                open_error.set(backend.download_open_folder(OPEN_ROOT_FOLDER).await.err());
            });
        }
    });

    // ---- folder ----
    let shown_dir = match &*storage.read() {
        Some(Ok(s)) => s.dir.clone(),
        _ => prefs.dir.clone(),
    };
    let dir_text = dir_draft().unwrap_or_else(|| prefs.dir.clone());
    let dir_row = rsx! {
        SettingRow { label: t("settings.downloads.dir"), hint: t("settings.downloads.dir_hint"), stack: true,
            div { class: "tk-dlset__dir", "data-download-dir": "{shown_dir}",
                span { class: "tk-dlset__dir-path", title: "{shown_dir}", "{shown_dir}" }
                div { class: "tk-dlset__dir-edit",
                    TextField {
                        label: t("settings.downloads.dir_field"),
                        value: dir_text,
                        icon: "folder".to_string(),
                        mono: true,
                        placeholder: t("settings.downloads.dir_default"),
                        oninput: move |v: String| dir_draft.set(Some(v)),
                        onsubmit: move |_| {
                            if let Some(d) = dir_draft() {
                                set(DownloadPrefs { dir: d, ..cur() });
                                dir_draft.set(None);
                            }
                        },
                    }
                    Button {
                        label: t("common.save"),
                        variant: ButtonVariant::Tonal,
                        size: ButtonSize::Sm,
                        disabled: dir_draft().is_none(),
                        onclick: move |_| {
                            if let Some(d) = dir_draft() {
                                set(DownloadPrefs { dir: d, ..cur() });
                                dir_draft.set(None);
                            }
                        },
                    }
                    Button {
                        label: t("settings.downloads.pick"),
                        icon: "folder".to_string(),
                        variant: ButtonVariant::Outlined,
                        size: ButtonSize::Sm,
                        onclick: move |_| {
                            let _ = document::eval(&format!("const i=document.getElementById('{DIR_INPUT_ID}');if(i){{i.click();}}"));
                        },
                    }
                    Button {
                        label: t("settings.downloads.reset_dir"),
                        variant: ButtonVariant::Text,
                        size: ButtonSize::Sm,
                        disabled: prefs.dir.is_empty(),
                        onclick: move |_| {
                            dir_draft.set(None);
                            set(DownloadPrefs { dir: String::new(), ..cur() });
                        },
                    }
                    IconButton {
                        icon: "external".to_string(),
                        label: t("downloads.open_folder"),
                        size: IconButtonSize::Sm,
                        onclick: move |_| open_dir.call(()),
                    }
                    input {
                        id: "{DIR_INPUT_ID}",
                        r#type: "file",
                        directory: true,
                        hidden: true,
                        onchange: move |e| {
                            if let Some(f) = e.files().first() {
                                dir_draft.set(None);
                                set(DownloadPrefs { dir: f.path().to_string_lossy().into_owned(), ..cur() });
                            }
                        },
                    }
                }
                if let Some(e) = open_error() {
                    ErrorState { error: e, compact: true, retry: false }
                }
            }
        }
    };

    // ---- limits ----
    let concurrent_opts: Vec<(String, String)> = (CONCURRENT_MIN..=CONCURRENT_MAX).map(|n| (n.to_string(), n.to_string())).collect();
    let speed_text = speed_draft().unwrap_or_else(|| prefs.speed_limit_kib.to_string());
    let speed_bad = parse_number(&speed_text).is_none();
    let limit_text = limit_draft().unwrap_or_else(|| prefs.storage_limit_gib.to_string());
    let limit_bad = parse_number(&limit_text).is_none();
    let storage_view = match &*storage.read() {
        None => rsx! { SkeletonLines { widths: vec![50] } },
        Some(Err(e)) => rsx! { ErrorState { error: e.clone(), compact: true, on_retry: move |_| storage.restart() } },
        Some(Ok(s)) => rsx! { StorageUse { storage: s.clone() } },
    };

    // ---- server / audio ----
    let mut server_opts = vec![("auto".to_owned(), t("downloads.season.server_auto"))];
    let ext_list: Vec<ExtensionVm> = match &*exts.read() {
        Some(Ok(v)) => v.clone(),
        _ => Vec::new(),
    };
    server_opts.extend(ext_list.iter().filter(|e| e.enabled).map(|e| (e.id.clone(), e.name.clone())));
    if let Some(s) = prefs.preferred_server.as_ref().filter(|s| !server_opts.iter().any(|(id, _)| id == *s)) {
        server_opts.push((s.clone(), s.clone()));
    }
    let server_sel = prefs.preferred_server.clone().unwrap_or_else(|| "auto".to_owned());

    // ---- auto ----
    let profile_opts: Vec<(String, String)> =
        AutoProfileVm::ALL.iter().map(|p| (p.id().to_owned(), t(&format!("downloads.profile.{}", p.id())))).collect();
    let interval_opts: Vec<(String, String)> =
        AUTO_INTERVALS.iter().map(|m| (m.to_string(), interval_label(*m))).collect();
    let delete_opts: Vec<(String, String)> = DELETE_DAYS
        .iter()
        .map(|d| (d.to_string(), if *d == 0 { t("settings.downloads.delete_never") } else { tn("settings.downloads.delete_days", *d as usize) }))
        .collect();
    let last_line = match &*last_run.read() {
        Some(Ok(Some(r))) => run_line(r),
        Some(Ok(None)) => t("settings.downloads.auto_never"),
        Some(Err(e)) => e.detail.clone(),
        None => String::new(),
    };
    let sim_view = match sim() {
        None if sim_busy() => rsx! { LoadingIndicator { small: true } },
        None => rsx! {},
        Some(Err(e)) => rsx! { ErrorState { error: e, compact: true, on_retry: move |_| run_sim.call(()) } },
        Some(Ok(lines)) if lines.is_empty() => rsx! {
            EmptyState { icon: "check".to_string(), title: t("settings.downloads.sim_empty") }
        },
        Some(Ok(lines)) => rsx! {
            ul { class: "tk-dlset__sim", role: "list", "data-simulated": "{lines.len()}",
                for (title, eps) in lines.iter().cloned() {
                    li { key: "{title}",
                        span { class: "tk-dlset__sim-title", "{title}" }
                        span { class: "tk-dlset__sim-eps",
                            {tf("settings.downloads.sim_eps", &[("eps", &eps.iter().map(|e| e.to_string()).collect::<Vec<_>>().join(", "))])}
                        }
                    }
                }
            }
        },
    };

    // ---- library folders ----
    let lib_dirs = prefs.library_dirs.clone();

    rsx! {
        p { class: "tk-panel__hint", {t("settings.downloads.hint")} }
        {dir_row}
        SettingRow { label: t("settings.downloads.concurrent"), hint: t("settings.downloads.concurrent_hint"),
            Segmented {
                options: concurrent_opts,
                selected: prefs.concurrent.to_string(),
                label: t("settings.downloads.concurrent"),
                small: true,
                on_select: move |id: String| {
                    if let Ok(n) = id.parse::<u8>() {
                        set(DownloadPrefs { concurrent: n, ..cur() });
                    }
                },
            }
        }
        SettingRow { label: t("settings.downloads.speed"), hint: t("settings.downloads.speed_hint"),
            div { class: "tk-dlset__num",
                TextField {
                    label: t("settings.downloads.speed_field"),
                    value: speed_text,
                    kind: "number".to_string(),
                    mono: true,
                    error: speed_bad,
                    oninput: move |v: String| speed_draft.set(Some(v)),
                    onsubmit: move |_| {
                        if let Some(n) = speed_draft().as_deref().and_then(parse_number) {
                            set(DownloadPrefs { speed_limit_kib: n, ..cur() });
                            speed_draft.set(None);
                        }
                    },
                }
                Button {
                    label: t("common.save"),
                    variant: ButtonVariant::Tonal,
                    size: ButtonSize::Sm,
                    disabled: speed_draft().is_none() || speed_bad,
                    onclick: move |_| {
                        if let Some(n) = speed_draft().as_deref().and_then(parse_number) {
                            set(DownloadPrefs { speed_limit_kib: n, ..cur() });
                            speed_draft.set(None);
                        }
                    },
                }
            }
        }
        SettingRow { label: t("settings.downloads.limit"), hint: t("settings.downloads.limit_hint"), stack: true,
            div { class: "tk-dlset__num",
                TextField {
                    label: t("settings.downloads.limit_field"),
                    value: limit_text,
                    kind: "number".to_string(),
                    mono: true,
                    error: limit_bad,
                    oninput: move |v: String| limit_draft.set(Some(v)),
                    onsubmit: move |_| {
                        if let Some(n) = limit_draft().as_deref().and_then(parse_number) {
                            set(DownloadPrefs { storage_limit_gib: n, ..cur() });
                            limit_draft.set(None);
                        }
                    },
                }
                Button {
                    label: t("common.save"),
                    variant: ButtonVariant::Tonal,
                    size: ButtonSize::Sm,
                    disabled: limit_draft().is_none() || limit_bad,
                    onclick: move |_| {
                        if let Some(n) = limit_draft().as_deref().and_then(parse_number) {
                            set(DownloadPrefs { storage_limit_gib: n, ..cur() });
                            limit_draft.set(None);
                        }
                    },
                }
            }
            {storage_view}
        }
        SettingRow { label: t("settings.downloads.delete_watched"), hint: t("settings.downloads.delete_watched_hint"),
            Segmented {
                options: delete_opts,
                selected: prefs.delete_watched_days.to_string(),
                label: t("settings.downloads.delete_watched"),
                small: true,
                on_select: move |id: String| {
                    if let Ok(n) = id.parse::<u16>() {
                        set(DownloadPrefs { delete_watched_days: n, ..cur() });
                    }
                },
            }
        }
        SettingRow { label: t("settings.downloads.server"), hint: t("settings.downloads.server_hint"), stack: true,
            div { "data-preferred-server": "{server_sel}",
                Segmented {
                    options: server_opts,
                    selected: server_sel.clone(),
                    label: t("settings.downloads.server"),
                    small: true,
                    on_select: move |id: String| {
                        let server = Some(id).filter(|s| s != "auto");
                        set(DownloadPrefs { preferred_server: server, ..cur() });
                    },
                }
            }
        }
        SettingRow { label: t("settings.downloads.audio"), hint: t("settings.downloads.audio_hint"),
            div { "data-preferred-audio": "{prefs.preferred_audio}",
                Segmented {
                    options: lang_seg(&DUB_LANGS),
                    selected: prefs.preferred_audio.clone(),
                    label: t("settings.downloads.audio"),
                    small: true,
                    on_select: move |id: String| set(DownloadPrefs { preferred_audio: id, ..cur() }),
                }
            }
        }
        h3 { class: "tk-dlset__group", {t("settings.downloads.auto_group")} }
        SettingRow { label: t("settings.downloads.auto"), hint: t("settings.downloads.auto_hint"),
            div { "data-auto-enabled": "{prefs.auto_new_episodes}",
                Switch {
                    checked: prefs.auto_new_episodes,
                    label: t("settings.downloads.auto"),
                    on_toggle: move |v: bool| set(DownloadPrefs { auto_new_episodes: v, ..cur() }),
                }
            }
        }
        SettingRow { label: t("settings.downloads.profile"), hint: t("settings.downloads.profile_hint"), stack: true,
            div { "data-auto-profile": prefs.auto_profile.id(),
                Segmented {
                    options: profile_opts,
                    selected: prefs.auto_profile.id().to_owned(),
                    label: t("settings.downloads.profile"),
                    on_select: move |id: String| {
                        set(DownloadPrefs { auto_profile: AutoProfileVm::from_id(&id), ..cur() });
                    },
                }
            }
        }
        SettingRow { label: t("settings.downloads.interval"), hint: t("settings.downloads.interval_hint"),
            Segmented {
                options: interval_opts,
                selected: prefs.auto_check_minutes.to_string(),
                label: t("settings.downloads.interval"),
                small: true,
                on_select: move |id: String| {
                    if let Ok(m) = id.parse::<u16>() {
                        set(DownloadPrefs { auto_check_minutes: m, ..cur() });
                    }
                },
            }
        }
        SettingRow { label: t("settings.downloads.check"), hint: last_line, stack: true,
            div { class: "tk-dlset__auto-actions",
                AutoCheckNow { on_done: move |_| last_run.restart() }
                Button {
                    label: t("settings.downloads.simulate"),
                    icon: "filter".to_string(),
                    variant: ButtonVariant::Text,
                    disabled: sim_busy(),
                    onclick: move |_| run_sim.call(()),
                }
            }
            {sim_view}
        }
        h3 { class: "tk-dlset__group", {t("settings.downloads.library_group")} }
        SettingRow { label: t("settings.downloads.library"), hint: t("settings.downloads.library_hint"), stack: true,
            div { class: "tk-dlset__libs", "data-library-dirs": "{lib_dirs.len()}",
                if lib_dirs.is_empty() {
                    p { class: "tk-dlset__libs-empty", {t("settings.downloads.library_empty")} }
                }
                ul { class: "tk-dlset__lib-list", role: "list",
                    for d in lib_dirs.iter().cloned() {
                        li { key: "{d}", class: "tk-dlset__lib",
                            span { class: "tk-dlset__lib-path", title: "{d}", "{d}" }
                            IconButton {
                                icon: "trash".to_string(),
                                label: t("common.remove"),
                                size: IconButtonSize::Sm,
                                onclick: {
                                    let d = d.clone();
                                    move |_| {
                                        let mut p = ui.settings.peek().downloads.clone();
                                        p.library_dirs.retain(|x| *x != d);
                                        set(p);
                                    }
                                },
                            }
                        }
                    }
                }
                div { class: "tk-dlset__lib-add",
                    TextField {
                        label: t("settings.downloads.library_add"),
                        value: lib_draft(),
                        icon: "folder".to_string(),
                        mono: true,
                        oninput: move |v: String| lib_draft.set(v),
                        onsubmit: move |_| {
                            let p = ui.settings.peek().downloads.clone();
                            let dirs = with_dir(&p.library_dirs, &lib_draft.peek());
                            set(DownloadPrefs { library_dirs: dirs, ..p });
                            lib_draft.set(String::new());
                        },
                    }
                    Button {
                        label: t("settings.downloads.library_add_btn"),
                        icon: "plus".to_string(),
                        variant: ButtonVariant::Tonal,
                        size: ButtonSize::Sm,
                        disabled: lib_draft().trim().is_empty(),
                        onclick: move |_| {
                            let p = ui.settings.peek().downloads.clone();
                            let dirs = with_dir(&p.library_dirs, &lib_draft.peek());
                            set(DownloadPrefs { library_dirs: dirs, ..p });
                            lib_draft.set(String::new());
                        },
                    }
                    Button {
                        label: t("settings.downloads.pick"),
                        icon: "folder".to_string(),
                        variant: ButtonVariant::Outlined,
                        size: ButtonSize::Sm,
                        onclick: move |_| {
                            let _ = document::eval(&format!("const i=document.getElementById('{LIB_INPUT_ID}');if(i){{i.click();}}"));
                        },
                    }
                    input {
                        id: "{LIB_INPUT_ID}",
                        r#type: "file",
                        directory: true,
                        hidden: true,
                        onchange: move |e| {
                            if let Some(f) = e.files().first() {
                                let p = ui.settings.peek().downloads.clone();
                                let dirs = with_dir(&p.library_dirs, &f.path().to_string_lossy());
                                set(DownloadPrefs { library_dirs: dirs, ..p });
                            }
                        },
                    }
                }
            }
        }
    }
}

/// `15 min`, `3 h`, `24 h`.
fn interval_label(minutes: u16) -> String {
    if minutes < 60 {
        tf("settings.downloads.minutes", &[("n", &minutes.to_string())])
    } else {
        tf("settings.downloads.hours", &[("n", &(minutes / 60).to_string())])
    }
}

/// "Last check: <when> · N checked · M queued".
fn run_line(r: &AutoDownloadRunVm) -> String {
    tf(
        "settings.downloads.auto_last",
        &[("when", &format_when(r.ran_unix)), ("checked", &r.checked.to_string()), ("enqueued", &r.enqueued.to_string())],
    )
}

/// Disk use against the cap.
#[component]
fn StorageUse(storage: DownloadStorageVm) -> Element {
    let reached = storage_limit_reached(storage.used_bytes, storage.limit_bytes);
    let used = format_bytes(storage.used_bytes);
    let line = match storage.limit_bytes {
        Some(l) => tf("downloads.storage.used_of", &[("used", &used), ("limit", &format_bytes(l))]),
        None => tf("downloads.storage.used", &[("used", &used)]),
    };
    let limit_attr = storage.limit_bytes.map(|l| l.to_string()).unwrap_or_default();
    rsx! {
        div {
            class: "tk-dlset__storage",
            "data-storage-used": "{storage.used_bytes}",
            "data-storage-limit": "{limit_attr}",
            "data-storage-limit-reached": "{reached}",
            span { "{line}" }
            if let Some(f) = storage_fraction(&storage) {
                WavyProgress { fraction: f, label: line.clone(), active: false, thin: true }
            }
            if reached {
                p { class: "tk-dlset__storage-warn", role: "status", {t("downloads.storage.reached")} }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_and_dirs() {
        assert_eq!(parse_number(""), Some(0));
        assert_eq!(parse_number(" 512 "), Some(512));
        assert_eq!(parse_number("x"), None);
        let dirs = vec!["/a".to_owned()];
        assert_eq!(with_dir(&dirs, " /b "), vec!["/a".to_owned(), "/b".to_owned()]);
        assert_eq!(with_dir(&dirs, "/a"), dirs);
        assert_eq!(with_dir(&dirs, "  "), dirs);
    }
}
