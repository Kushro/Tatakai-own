//! Details: header with metadata chips and the Watch/Continue split button (its chevron opens the
//! source picker), tabs Episodes / Info / Related, episodes in ranges of 100 with Watched / Filler
//! pills, the source summary and "send to device" side panel, and a sticky floating toolbar.
//! `DetailsView { embedded: true }` is the right pane of Search on wide windows (header only).

use std::time::{SystemTime, UNIX_EPOCH};

use dioxus::prelude::*;

use crate::app::{Route, Services, Ui};
use crate::backend::{DetailsVm, DeviceVm, EpisodeVm, LinkStatusVm, SourceVm};
use crate::components::title_bar::ShellStatus;
use crate::components::{
    Button, ButtonSize, ButtonVariant, Comments, ConfirmDialog, DownloadSeasonSheet, EmptyState, ErrorState,
    FloatingToolbar, IconButton, IconButtonVariant, InfoChip, ListButton, LoadingIndicator, MediaCover, Pill, Segmented,
    Sheet, SkeletonLines, SourcePicker, SourceSummary, SplitButton, ToolbarSeparator, Tone, use_snackbar,
};
use crate::i18n::{t, tf, tn};
use crate::icons::Icon;
use crate::model::{EPISODE_RANGE, UiError, episode_ranges, format_time, range_index_for};
use crate::screens::extensions::rel_time;

/// From this fraction an episode counts as watched ("Watched" pill).
pub const WATCHED_AT: f64 = 0.9;
/// Devices listed in the "Send to device" panel.
const SIDE_DEVICES: usize = 2;
/// Synopses longer than this get the More / Less toggle.
const CLAMP_CHARS: usize = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Episodes,
    Info,
    Related,
}

impl Tab {
    const ALL: [Tab; 3] = [Self::Episodes, Self::Info, Self::Related];

    fn id(self) -> &'static str {
        match self {
            Self::Episodes => "episodes",
            Self::Info => "info",
            Self::Related => "related",
        }
    }

    fn label_key(self) -> &'static str {
        match self {
            Self::Episodes => "details.episodes",
            Self::Info => "details.info",
            Self::Related => "details.related",
        }
    }
}

/// `(i18n key, tone)` of the pills of an episode row. The current (resume) episode never shows
/// "Watched". `EpisodeVm` has no recap flag yet, so "Recap" is never produced.
pub fn episode_pills(e: &EpisodeVm, resume: u32) -> Vec<(&'static str, Tone)> {
    let mut out = Vec::new();
    if e.is_filler {
        out.push(("details.filler", Tone::Warn));
    }
    if e.watched >= WATCHED_AT && e.number != resume {
        out.push(("details.watched", Tone::Ok));
    }
    out
}

/// The requested device, else the first one connected now, else the first paired one.
pub fn pick_device(list: &[DeviceVm], wanted: Option<&str>) -> Option<DeviceVm> {
    wanted
        .and_then(|id| list.iter().find(|d| d.id == id))
        .or_else(|| list.iter().find(|d| d.online))
        .or_else(|| list.first())
        .cloned()
}

fn now_unix() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

#[component]
pub fn DetailsPage(id: String) -> Element {
    rsx! {
        div { class: "tk-page tk-page--details",
            DetailsView { key: "{id}", id: id.clone(), embedded: false }
        }
    }
}

#[component]
pub fn DetailsView(id: String, #[props(default)] embedded: bool) -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let navigator = use_navigator();
    let snackbar = use_snackbar();
    let backend = services.backend.clone();

    let mut details = use_resource({
        let (backend, id) = (backend.clone(), id.clone());
        move || {
            let (backend, id) = (backend.clone(), id.clone());
            async move { backend.details(&id).await }
        }
    });
    // The embedded pane only shows the header: skip the other calls there.
    let mut episodes = use_resource({
        let (backend, id) = (backend.clone(), id.clone());
        move || {
            let (backend, id) = (backend.clone(), id.clone());
            async move { if embedded { Ok(Vec::new()) } else { backend.episodes(&id).await } }
        }
    });
    let exts = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move { if embedded { Ok(Vec::new()) } else { backend.extensions().await } }
        }
    });
    // tk-app starts the Link server on `link_status()` (firewall prompt): only ask once a Link screen has
    // started it this session (the shell then holds a status); otherwise the panel offers the Link page.
    let shell_link = try_use_context::<ShellStatus>().map(|s| s.link);
    let mut link = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            let started = shell_link.is_some_and(|l| l.peek().is_some());
            async move { if embedded || !started { Ok(LinkStatusVm::default()) } else { backend.link_status().await } }
        }
    });

    let mut tab = use_signal(|| Tab::Episodes);
    let mut picker_open = use_signal(|| false);
    // Wave B mount point (worker h): the "Download season" sheet; the contract only opens / closes it.
    let mut download_open = use_signal(|| false);
    let mut picker_ep = use_signal(|| 1_u32);
    let mut picker_list = use_signal(|| None::<Result<Vec<SourceVm>, UiError>>);
    let mut send_target = use_signal(|| None::<DeviceVm>);
    let mut send_error = use_signal(|| None::<UiError>);

    // The ▾ of the split button and "See all sources" open the same picker with a fresh list.
    let open_picker = use_callback({
        let (backend, id) = (backend.clone(), id.clone());
        move |ep: u32| {
            picker_ep.set(ep);
            picker_list.set(None);
            picker_open.set(true);
            let (backend, id) = (backend.clone(), id.clone());
            spawn(async move {
                let result = backend.sources(&id, ep).await;
                if *picker_ep.peek() == ep {
                    picker_list.set(Some(result));
                }
            });
        }
    });

    // Remember the choice for this episode, then play it (the player reads `picked`).
    let play_source = use_callback({
        let (backend, id) = (backend.clone(), id.clone());
        move |(ep, source_id): (u32, String)| {
            picker_open.set(false);
            let (backend, id) = (backend.clone(), id.clone());
            spawn(async move {
                if let Err(e) = backend.set_picked_source(&id, ep, Some(&source_id)).await {
                    tracing::warn!(detail = %e.detail, "saving the picked source failed");
                }
                navigator.push(Route::PlayerPage { id, episode: ep });
            });
        }
    });

    // Torrent rows of the picker: "Download for later" (a background download; Torrents > Downloads lists it and
    // holds the warning / switch when background downloads are still off).
    let open_downloads = use_callback(move |_: ()| {
        navigator.push(Route::TorrentDownloadsPage {});
    });
    let download_source = use_callback({
        let (backend, id) = (backend.clone(), id.clone());
        move |(ep, source_id): (u32, String)| {
            let (backend, id) = (backend.clone(), id.clone());
            spawn(async move {
                match backend.torrent_add_download(&source_id, &id, Some(ep)).await {
                    Ok(_) => snackbar.show_action(t("src.download.started"), t("src.download.open"), open_downloads),
                    Err(e) => {
                        tracing::warn!(detail = %e.detail, "starting a background download failed");
                        snackbar.show_action(t("src.download.failed"), t("src.download.open"), open_downloads);
                    }
                }
            });
        }
    });

    // No paired device: go pair one (Link) instead of opening an empty dialog.
    let ask_send = use_callback(move |device: Option<String>| {
        let target = match &*link.read() {
            Some(Ok(st)) => pick_device(&st.devices, device.as_deref()),
            _ => None,
        };
        match target {
            Some(d) => {
                send_error.set(None);
                send_target.set(Some(d));
            }
            None => {
                navigator.push(Route::LinkPage {});
            }
        }
    });

    let vm: Option<DetailsVm> = details.read().as_ref().and_then(|r| r.as_ref().ok()).cloned();
    let resume = vm.as_ref().map(|d| d.resume_episode).unwrap_or(0);
    let watch_ep = resume.max(1);

    let header = match &*details.read() {
        None => rsx! {
            div { class: "tk-skel tk-skel--header", "aria-hidden": "true" }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), on_retry: move |_| details.restart() }
        },
        Some(Ok(d)) => {
            let play_id = id.clone();
            rsx! {
                DetailsHeader {
                    vm: d.clone(),
                    embedded,
                    menu_open: picker_open(),
                    on_watch: move |_| {
                        navigator.push(Route::PlayerPage { id: play_id.clone(), episode: watch_ep });
                    },
                    on_menu: move |_| open_picker.call(watch_ep),
                    on_send: move |_| ask_send.call(None),
                    on_download: move |_| download_open.set(true),
                }
            }
        }
    };

    let picker = if picker_open() {
        match picker_list() {
            None => rsx! {
                Sheet { title: t("src.title"), wide: true, on_close: move |_| picker_open.set(false),
                    div { class: "tk-details__sheetstate", LoadingIndicator {} }
                }
            },
            Some(Err(e)) => rsx! {
                Sheet { title: t("src.title"), wide: true, on_close: move |_| picker_open.set(false),
                    ErrorState { error: e.clone(), on_retry: move |_| open_picker.call(*picker_ep.peek()) }
                }
            },
            Some(Ok(list)) => rsx! {
                SourcePicker {
                    open: true,
                    sources: list,
                    prefs: ui.settings.read().sources.clone(),
                    on_pick: move |sid: String| play_source.call((*picker_ep.peek(), sid)),
                    on_close: move |_| picker_open.set(false),
                    on_download: move |sid: String| download_source.call((*picker_ep.peek(), sid)),
                }
            },
        }
    } else {
        rsx! {}
    };

    if embedded {
        let page_id = id.clone();
        return rsx! {
            div { class: "tk-details", "data-embedded": "true",
                {header}
                Button {
                    label: t("common.details"),
                    trailing_icon: "chevron-right".to_string(),
                    variant: ButtonVariant::Text,
                    onclick: move |_| {
                        navigator.push(Route::DetailsPage { id: page_id.clone() });
                    },
                }
                {picker}
            }
        };
    }

    let tab_body = match tab() {
        Tab::Episodes => match &*episodes.read() {
            None => rsx! {
                div { class: "tk-skel tk-skel--list", "aria-hidden": "true" }
            },
            Some(Err(e)) => rsx! {
                ErrorState { error: e.clone(), compact: true, on_retry: move |_| episodes.restart() }
            },
            Some(Ok(eps)) if eps.is_empty() => rsx! {
                EmptyState {
                    icon: "film".to_string(),
                    title: t("details.no_episodes.title"),
                    body: t("details.no_episodes.body"),
                }
            },
            Some(Ok(eps)) => rsx! {
                EpisodeList { id: id.clone(), episodes: eps.clone(), resume }
            },
        },
        Tab::Info => match &*details.read() {
            None => rsx! { SkeletonLines {} },
            Some(Err(e)) => rsx! {
                ErrorState { error: e.clone(), compact: true, on_retry: move |_| details.restart() }
            },
            Some(Ok(d)) => rsx! { InfoTab { vm: d.clone() } },
        },
        // No backend call for related titles in the wave 1 contract.
        Tab::Related => rsx! {
            ErrorState { error: UiError::not_wired("related"), retry: false }
        },
    };

    // Side panel: sources need at least one usable extension (unknown while loading: assume yes).
    let has_source_ext = match &*exts.read() {
        Some(Ok(list)) => list.iter().any(|e| e.enabled && e.compat.can_enable()),
        _ => true,
    };
    let sources_panel = if has_source_ext {
        rsx! {
            section { class: "tk-details__sources",
                h3 { class: "tk-panel__title", Icon { name: "extensions".to_string(), size: 20 } {t("details.provider")} }
                p { class: "tk-muted tk-details__hint", {t("details.provider_hint")} }
                SourceSummary {
                    key: "{id}-{watch_ep}",
                    media_id: id.clone(),
                    episode: watch_ep,
                    on_play: move |sid: String| play_source.call((watch_ep, sid)),
                    on_open_picker: move |_| open_picker.call(watch_ep),
                }
            }
        }
    } else {
        rsx! {
            section { class: "tk-panel",
                EmptyState {
                    icon: "extensions".to_string(),
                    title: t("details.no_extension.title"),
                    body: t("details.no_extension.body"),
                    action_label: t("details.go_extensions"),
                    on_action: move |_| {
                        navigator.push(Route::ExtensionsPage {});
                    },
                }
            }
        }
    };

    let now = now_unix();
    let devices_body = match &*link.read() {
        None => rsx! { SkeletonLines { widths: vec![70, 50] } },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), compact: true, on_retry: move |_| link.restart() }
        },
        Some(Ok(st)) if st.devices.is_empty() => rsx! {
            p { class: "tk-muted", {t("link.never_seen")} }
            Button {
                label: t("nav.link"),
                icon: "link".to_string(),
                variant: ButtonVariant::Tonal,
                onclick: move |_| {
                    navigator.push(Route::LinkPage {});
                },
            }
        },
        Some(Ok(st)) => rsx! {
            div { class: "tk-list",
                for d in st.devices.iter().take(SIDE_DEVICES).cloned() {
                    {
                        let kind = if d.online {
                            t("link.online")
                        } else if let Some(ts) = d.last_seen_unix {
                            tf("link.last_seen", &[("when", &rel_time(now, ts))])
                        } else {
                            t("link.never_seen")
                        };
                        let icon = if d.platform.eq_ignore_ascii_case("android") || d.platform.eq_ignore_ascii_case("ios") { "phone" } else { "desktop" };
                        let dev_id = d.id.clone();
                        rsx! {
                            button {
                                key: "{d.id}",
                                class: "tk-drel",
                                r#type: "button",
                                "data-online": "{d.online}",
                                onclick: move |_| ask_send.call(Some(dev_id.clone())),
                                span { class: "tk-drel__ico", Icon { name: icon.to_string(), size: 20 } }
                                span { class: "tk-drel__text",
                                    span { class: "tk-drel__kind", "{kind}" }
                                    span { class: "tk-drel__title", "{d.name}" }
                                }
                                Icon { name: "chevron-right".to_string(), size: 20 }
                            }
                        }
                    }
                }
            }
        },
    };

    let send_dialog = match (send_target(), vm.as_ref()) {
        (Some(d), Some(v)) => {
            let ep = tf("details.episode", &[("n", &watch_ep.to_string())]);
            rsx! {
                ConfirmDialog {
                    title: t("details.send_device"),
                    body: format!("{} · {ep} → {}", v.media.title, d.name),
                    confirm_label: t("details.send_device"),
                    icon: "cast".to_string(),
                    danger: false,
                    on_cancel: move |_| send_target.set(None),
                    on_confirm: move |_| {
                        send_target.set(None);
                        // The contract has no "send to device" call yet (integration note).
                        send_error.set(Some(UiError::not_wired("send_to_device")));
                    },
                }
            }
        }
        _ => rsx! {},
    };

    let toolbar = match vm.as_ref() {
        Some(v) => {
            let label = if resume > 0 {
                tf("details.continue", &[("n", &resume.to_string())])
            } else {
                tf("details.watch_ep", &[("n", "1")])
            };
            let play_id = id.clone();
            rsx! {
                FloatingToolbar { label: v.media.title.clone(),
                    Button {
                        label,
                        icon: "play".to_string(),
                        onclick: move |_| {
                            navigator.push(Route::PlayerPage { id: play_id.clone(), episode: watch_ep });
                        },
                    }
                    ToolbarSeparator {}
                    // Wave A mount point (worker g): the list button loads its own entry.
                    ListButton { media_id: id.clone(), episodes_total: v.episodes_total, compact: true }
                    IconButton { icon: "cast".to_string(), label: t("details.send_device"), onclick: move |_| ask_send.call(None) }
                    IconButton { icon: "download".to_string(), label: t("details.download"), onclick: move |_| download_open.set(true) }
                }
            }
        }
        None => rsx! {},
    };

    let tablist_label = t("details.info");
    rsx! {
        div { class: "tk-details", "data-embedded": "false",
            {header}
            div { class: "tk-details__layout",
                div { class: "tk-section tk-details__main",
                    div { class: "tk-dtabs", role: "tablist", "aria-label": "{tablist_label}",
                        for tb in Tab::ALL {
                            button {
                                key: "{tb.id()}",
                                id: "tk-dtab-{tb.id()}",
                                class: "tk-dtab",
                                r#type: "button",
                                role: "tab",
                                "aria-selected": if tab() == tb { "true" } else { "false" },
                                "aria-controls": "tk-dtabpanel",
                                onclick: move |_| tab.set(tb),
                                {t(tb.label_key())}
                            }
                        }
                    }
                    div {
                        id: "tk-dtabpanel",
                        class: "tk-details__tab",
                        role: "tabpanel",
                        "aria-labelledby": "tk-dtab-{tab().id()}",
                        {tab_body}
                    }
                    // Wave B mount point (worker n): comments about the title (renders nothing until then).
                    Comments { media_id: id.clone() }
                }
                aside { class: "tk-details__side",
                    {sources_panel}
                    section { class: "tk-panel tk-details__send",
                        h3 { class: "tk-panel__title", Icon { name: "link".to_string(), size: 20 } {t("details.send_device")} }
                        {devices_body}
                        if let Some(err) = send_error() {
                            ErrorState { error: err.clone(), compact: true, retry: false }
                        }
                    }
                }
            }
            {toolbar}
            {send_dialog}
            {picker}
            DownloadSeasonSheet {
                open: download_open(),
                media_id: id.clone(),
                episodes_total: vm.as_ref().and_then(|v| v.episodes_total),
                on_close: move |_| download_open.set(false),
            }
        }
    }
}

#[component]
fn DetailsHeader(
    vm: DetailsVm,
    embedded: bool,
    menu_open: bool,
    #[props(default)] on_watch: EventHandler<()>,
    #[props(default)] on_menu: EventHandler<()>,
    #[props(default)] on_send: EventHandler<()>,
    #[props(default)] on_download: EventHandler<()>,
) -> Element {
    let mut clamp = use_signal(|| true);
    let watch_label = if vm.resume_episode > 0 {
        tf("details.continue", &[("n", &vm.resume_episode.to_string())])
    } else {
        tf("details.watch_ep", &[("n", "1")])
    };
    let size = if embedded { ButtonSize::Md } else { ButtonSize::Lg };
    let synopsis = vm.synopsis.clone().unwrap_or_else(|| t("details.no_synopsis"));
    let long = synopsis.chars().count() > CLAMP_CHARS;
    let count = vm.episodes_total.map(|n| tn("details.episodes_count", n as usize));
    let resume_at = (vm.resume_episode > 0 && vm.resume_secs > 0)
        .then(|| tf("details.resume_at", &[("t", &format_time(f64::from(vm.resume_secs)))]));
    let more_label = if clamp() { t("common.more") } else { t("common.less") };
    let clamped = clamp() && long;
    rsx! {
        div { class: "tk-dhead", "data-embedded": "{embedded}",
            div { class: "tk-dhead__cover",
                MediaCover { src: vm.media.cover.clone(), color: vm.media.color.clone(), title: vm.media.title.clone() }
            }
            div { class: "tk-dhead__text",
                h1 { class: "tk-dhead__title", "{vm.media.title}" }
                div { class: "tk-chips",
                    if let Some(status) = vm.status.clone() {
                        Pill { text: status }
                    }
                    if !vm.media.subtitle.is_empty() {
                        InfoChip { text: vm.media.subtitle.clone() }
                    }
                    if let Some(c) = count.clone() {
                        InfoChip { text: c }
                    }
                    if let Some(score) = vm.media.score {
                        InfoChip { text: tf("details.score", &[("n", &score.to_string())]), icon: "star".to_string() }
                    }
                    for g in vm.genres.clone() {
                        InfoChip { key: "{g}", text: g.clone() }
                    }
                }
                div { class: "tk-dhead__actions",
                    SplitButton {
                        label: watch_label,
                        menu_label: t("details.choose_source"),
                        icon: "play".to_string(),
                        size,
                        expanded: menu_open,
                        onclick: move |_| on_watch.call(()),
                        on_menu: move |_| on_menu.call(()),
                    }
                    if !embedded {
                        // Wave A mount point (worker g): `components/list_button.rs` loads its own entry.
                        ListButton { media_id: vm.media.id.clone(), episodes_total: vm.episodes_total }
                        Button {
                            label: t("details.send_device"),
                            icon: "cast".to_string(),
                            variant: ButtonVariant::Outlined,
                            onclick: move |_| on_send.call(()),
                        }
                        IconButton {
                            icon: "download".to_string(),
                            label: t("details.download"),
                            variant: IconButtonVariant::Outlined,
                            onclick: move |_| on_download.call(()),
                        }
                    }
                    if let Some(r) = resume_at {
                        span { class: "tk-muted tk-dhead__resume", "{r}" }
                    }
                }
                div { class: "tk-dhead__syn",
                    p { class: "tk-dhead__synopsis", "data-clamp": "{clamped}", "{synopsis}" }
                    if long {
                        Button {
                            label: more_label,
                            variant: ButtonVariant::Text,
                            size: ButtonSize::Xs,
                            onclick: move |_| clamp.set(!clamp()),
                        }
                    }
                }
                if !embedded {
                    div { class: "tk-dmeta",
                        if let Some(status) = vm.status.clone() {
                            MetaItem { k: t("details.status"), v: status }
                        }
                        if let Some(n) = vm.episodes_total {
                            MetaItem { k: t("details.episodes"), v: n.to_string() }
                        }
                        if !vm.genres.is_empty() {
                            MetaItem { k: t("details.genres"), v: vm.genres.len().to_string() }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn MetaItem(k: String, v: String, #[props(default)] mono: bool) -> Element {
    rsx! {
        div { class: "tk-dmeta__item",
            span { class: "tk-dmeta__k", "{k}" }
            span { class: "tk-dmeta__v", "data-mono": "{mono}", "{v}" }
        }
    }
}

/// Info tab: full synopsis, genres and the provider id.
#[component]
fn InfoTab(vm: DetailsVm) -> Element {
    let synopsis = vm.synopsis.clone().unwrap_or_else(|| t("details.no_synopsis"));
    let (provider, pid) = match vm.media.id.strip_prefix("mal:") {
        Some(rest) => (t("provider.mal"), rest.to_owned()),
        None => (t("provider.anilist"), vm.media.id.clone()),
    };
    rsx! {
        div { class: "tk-section",
            section { class: "tk-panel",
                h3 { class: "tk-panel__title", {t("details.synopsis")} }
                p { class: "tk-dhead__synopsis", "{synopsis}" }
            }
            if !vm.genres.is_empty() {
                section { class: "tk-panel",
                    h3 { class: "tk-panel__title", {t("details.genres")} }
                    div { class: "tk-chips",
                        for g in vm.genres.clone() {
                            InfoChip { key: "{g}", text: g.clone() }
                        }
                    }
                }
            }
            div { class: "tk-dmeta",
                MetaItem { k: provider, v: format!("#{pid}"), mono: true }
            }
        }
    }
}

#[component]
fn EpisodeList(id: String, episodes: Vec<EpisodeVm>, resume: u32) -> Element {
    let navigator = use_navigator();
    let max = episodes.iter().map(|e| e.number).max().unwrap_or(0);
    let ranges = episode_ranges(max, EPISODE_RANGE);
    let mut range = use_signal(|| if resume > 0 { range_index_for(resume, EPISODE_RANGE) } else { 0 });
    let active = range().min(ranges.len().saturating_sub(1));
    let (from, to) = ranges.get(active).copied().unwrap_or((1, max));
    let shown: Vec<EpisodeVm> = episodes.iter().filter(|e| e.number >= from && e.number <= to).cloned().collect();
    let options: Vec<(String, String)> = ranges
        .iter()
        .enumerate()
        .map(|(i, (a, b))| (i.to_string(), tf("details.range", &[("from", &a.to_string()), ("to", &b.to_string())])))
        .collect();

    rsx! {
        if ranges.len() > 1 {
            Segmented {
                options,
                selected: active.to_string(),
                label: t("details.episodes"),
                small: true,
                on_select: move |v: String| range.set(v.parse().unwrap_or(0)),
            }
        }
        div { class: "tk-list tk-deps",
            for e in shown {
                {
                    let (id, n) = (id.clone(), e.number);
                    rsx! {
                        DetailsEpisodeRow {
                            key: "{n}",
                            episode: e.clone(),
                            current: n == resume,
                            pills: episode_pills(&e, resume).into_iter().map(|(k, tone)| (t(k), tone)).collect::<Vec<_>>(),
                            onclick: move |_| {
                                navigator.push(Route::PlayerPage { id: id.clone(), episode: n });
                            },
                        }
                    }
                }
            }
        }
    }
}

/// Episode row of this screen (thumb with progress, `n. title`, pills, air date).
#[component]
fn DetailsEpisodeRow(
    episode: EpisodeVm,
    current: bool,
    pills: Vec<(String, Tone)>,
    #[props(default)] onclick: EventHandler<MouseEvent>,
) -> Element {
    let n = episode.number;
    let heading = match episode.title.clone() {
        Some(title) => format!("{n}. {title}"),
        None => tf("details.episode", &[("n", &n.to_string())]),
    };
    let aired = episode.air_date.clone().map(|d| tf("details.aired", &[("d", &d)]));
    let in_progress = episode.watched > 0.0 && episode.watched < WATCHED_AT;
    let bar = (episode.watched * 100.0).round();
    rsx! {
        button {
            class: "tk-dep",
            r#type: "button",
            "aria-current": "{current}",
            onclick: move |e| onclick.call(e),
            div { class: "tk-dep__thumb",
                if let Some(img) = episode.image.clone() {
                    img { src: "{img}", alt: "", loading: "lazy", decoding: "async", draggable: "false" }
                } else {
                    span { class: "tk-dep__mono", "aria-hidden": "true", "{n}" }
                }
                if in_progress {
                    div { class: "tk-dep__progress",
                        div { style: "width:{bar}%" }
                    }
                }
            }
            div { class: "tk-dep__body",
                div { class: "tk-dep__title",
                    span { "{heading}" }
                    for (text , tone) in pills {
                        Pill { key: "{text}", text: text.clone(), tone, plain: true }
                    }
                }
                if let Some(a) = aired {
                    div { class: "tk-dep__sub", "{a}" }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ep(n: u32, watched: f64, filler: bool) -> EpisodeVm {
        EpisodeVm { number: n, title: None, image: None, air_date: None, is_filler: filler, watched }
    }

    fn dev(id: &str, online: bool) -> DeviceVm {
        DeviceVm { id: id.into(), name: id.into(), platform: "android".into(), last_seen_unix: None, online }
    }

    #[test]
    fn pills_skip_watched_on_the_resume_episode() {
        assert_eq!(episode_pills(&ep(3, 1.0, true), 5), vec![("details.filler", Tone::Warn), ("details.watched", Tone::Ok)]);
        assert!(episode_pills(&ep(5, 1.0, false), 5).is_empty());
        assert!(episode_pills(&ep(6, 0.5, false), 5).is_empty());
    }

    #[test]
    fn device_choice_prefers_wanted_then_online() {
        let list = [dev("a", false), dev("b", true)];
        assert_eq!(pick_device(&list, Some("a")).unwrap().id, "a");
        assert_eq!(pick_device(&list, None).unwrap().id, "b");
        assert_eq!(pick_device(&list, Some("zz")).unwrap().id, "b");
        assert_eq!(pick_device(&[dev("a", false)], None).unwrap().id, "a");
        assert!(pick_device(&[], None).is_none());
    }
}
