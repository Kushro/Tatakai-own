//! Details: header with metadata chips, the next-episode countdown, the Watch/Continue split button (its chevron
//! opens the source picker), list / share / trailer actions; tabs Episodes / Info / Characters / Related (all
//! panels stay in the DOM, only the active one is visible); episodes in ranges of 100 with search, thumbnail or
//! compact view, Watched / Filler / Not yet aired pills; the watch order of the franchise and AniList
//! recommendations under the tabs; the source summary and "send to device" side panel; a sticky floating toolbar;
//! the adult content gate. `DetailsView { embedded: true }` is the right pane of Search on wide windows (header
//! only). Wave A (worker c): extras come from `Backend::details_extra` / `watch_order` (`details/extra.rs`).

mod extra;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dioxus::prelude::*;

use crate::app::{Route, Services, Ui};
use crate::backend::{AiringVm, DetailsExtraVm, DetailsVm, DeviceVm, EpisodeVm, LinkStatusVm, SendToDeviceVm, SourceVm, TrailerVm};
use crate::components::title_bar::ShellStatus;
use crate::components::{
    Button, ButtonSize, ButtonVariant, ConfirmDialog, DownloadSeasonSheet, EmptyState, ErrorState,
    FloatingToolbar, IconButton, IconButtonVariant, InfoChip, ListButton, LoadingIndicator, MediaCover, Pill, Reviews,
    Segmented, Sheet, SkeletonLines, SourcePicker, SourceSummary, SplitButton, TextField, ToolbarSeparator, Tone,
    use_snackbar,
};
use crate::i18n::{t, tf, tn};
use crate::icons::Icon;
use crate::model::{EPISODE_RANGE, UiError, episode_ranges, format_time, range_index_for};
use crate::screens::extensions::rel_time;
use extra::{
    AdultGate, CharactersTab, FactsSheet, Recommendations, RelatedTab, WatchOrder, airing_label, share_url,
};

/// From this fraction an episode counts as watched ("Watched" pill).
pub const WATCHED_AT: f64 = 0.9;
/// Devices listed in the "Send to device" panel.
const SIDE_DEVICES: usize = 2;
/// Synopses longer than this get the More / Less toggle.
const CLAMP_CHARS: usize = 120;
/// Web view `localStorage` keys (per-viewer conveniences; the backend has no UI-state store).
const VIEW_KEY: &str = "tk.details.episodeView";
const ADULT_KEY: &str = "tk.details.adultOk";
/// The next-episode countdown refreshes this often.
const CLOCK_TICK: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Episodes,
    Info,
    Characters,
    Related,
}

impl Tab {
    const ALL: [Tab; 4] = [Self::Episodes, Self::Info, Self::Characters, Self::Related];

    fn id(self) -> &'static str {
        match self {
            Self::Episodes => "episodes",
            Self::Info => "info",
            Self::Characters => "characters",
            Self::Related => "related",
        }
    }

    fn label_key(self) -> &'static str {
        match self {
            Self::Episodes => "details.episodes",
            Self::Info => "details.info",
            Self::Characters => "details.tab.characters",
            Self::Related => "details.related",
        }
    }
}

/// Episode list layout (remembered per viewer).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpisodeView {
    Thumbnails,
    Compact,
}

impl EpisodeView {
    pub fn id(self) -> &'static str {
        match self {
            Self::Thumbnails => "thumbnails",
            Self::Compact => "compact",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "thumbnails" => Some(Self::Thumbnails),
            "compact" => Some(Self::Compact),
            _ => None,
        }
    }
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// `YYYY-MM-DD` of a unix time (UTC).
pub fn unix_to_date(secs: i64) -> String {
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    format!("{y:04}-{m:02}-{d:02}")
}

/// Air date of an episode as unix seconds: `2024-10-05` (start of that day, UTC), `2024-10-05T14:30:00Z`,
/// `2024-10-05 14:30` or with a `+09:00` offset. `None` when it does not start with a full date.
pub fn parse_air_unix(text: &str) -> Option<i64> {
    let text = text.trim();
    let num = |s: &str| -> Option<i64> { (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse().ok()).flatten() };
    let date = text.get(..10)?;
    let mut parts = date.split('-');
    let (y, m, d) = (num(parts.next()?)?, num(parts.next()?)?, num(parts.next()?)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || parts.next().is_some() {
        return None;
    }
    let mut secs = days_from_civil(y, m, d) * 86_400;
    let rest = &text[10..];
    if let Some(time) = rest.strip_prefix('T').or_else(|| rest.strip_prefix(' ')) {
        let (clock, offset) = match time.find(['Z', 'z', '+', '-']) {
            Some(i) => (&time[..i], &time[i..]),
            None => (time, ""),
        };
        let mut hms = clock.split(':');
        let h = num(hms.next()?)?;
        let mi = hms.next().map_or(Some(0), num)?;
        let sec = hms.next().map_or(Some(0), |s| num(s.split('.').next().unwrap_or("0")))?;
        secs += h * 3_600 + mi * 60 + sec;
        if let Some(sign) = offset.chars().next().filter(|c| *c == '+' || *c == '-') {
            let o = offset[1..].replace(':', "");
            let (oh, om) = (num(o.get(..2)?)?, o.get(2..4).map_or(Some(0), num)?);
            let delta = oh * 3_600 + om * 60;
            secs += if sign == '+' { -delta } else { delta };
        }
    }
    Some(secs)
}

/// Not aired yet: at or after the next airing episode, or an air date still in the future.
pub fn is_unaired(e: &EpisodeVm, next: Option<AiringVm>, now_unix: i64) -> bool {
    if next.is_some_and(|n| e.number >= n.episode) {
        return true;
    }
    e.air_date.as_deref().and_then(parse_air_unix).is_some_and(|at| at > now_unix)
}

/// The episode list plus a schedule-only row for the next airing episode when the providers do not list it yet.
pub fn with_next_airing(mut eps: Vec<EpisodeVm>, next: Option<AiringVm>) -> Vec<EpisodeVm> {
    if let Some(n) = next {
        let max = eps.iter().map(|e| e.number).max().unwrap_or(0);
        if n.episode == max + 1 {
            eps.push(EpisodeVm {
                number: n.episode,
                title: None,
                image: None,
                air_date: Some(unix_to_date(n.at_unix)),
                is_filler: false,
                watched: 0.0,
            });
        }
    }
    eps
}

/// Episodes matching the search: a number matches that episode (and titles containing it); text matches titles,
/// case-insensitively. An empty query keeps everything.
pub fn filter_episodes(eps: &[EpisodeVm], query: &str) -> Vec<EpisodeVm> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return eps.to_vec();
    }
    let number: Option<u32> = q.parse().ok();
    eps.iter()
        .filter(|e| {
            number.is_some_and(|n| e.number == n) || e.title.as_ref().is_some_and(|t| t.to_lowercase().contains(&q))
        })
        .cloned()
        .collect()
}

fn ls_set(key: &str, value: &str) {
    let k = serde_json::to_string(key).unwrap_or_default();
    let v = serde_json::to_string(value).unwrap_or_default();
    let _ = document::eval(&format!("try{{localStorage.setItem({k},{v})}}catch(e){{}}"));
}

async fn ls_get(key: &str) -> Option<String> {
    let k = serde_json::to_string(key).unwrap_or_default();
    let js = format!("try{{return localStorage.getItem({k})}}catch(e){{return null}}");
    document::eval(&js).join::<Option<String>>().await.ok().flatten()
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

    // Wave A (c): facts, people, relations, recommendations, trailer, next airing; and the season chain.
    let mut extra = use_resource({
        let (backend, id) = (backend.clone(), id.clone());
        move || {
            let (backend, id) = (backend.clone(), id.clone());
            async move { if embedded { Ok(DetailsExtraVm::default()) } else { backend.details_extra(&id).await } }
        }
    });
    let mut order = use_resource({
        let (backend, id) = (backend.clone(), id.clone());
        move || {
            let (backend, id) = (backend.clone(), id.clone());
            async move { if embedded { Ok(Vec::new()) } else { backend.watch_order(&id).await } }
        }
    });
    // Adult gate (classic `AdultWarningGate`): "Always show" is remembered in the web view, "Show this time" only
    // for this page.
    let mut adult_ok = use_signal(|| false);
    let mut adult_session = use_signal(|| false);
    use_future(move || async move {
        if ls_get(ADULT_KEY).await.as_deref() == Some("1") {
            adult_ok.set(true);
        }
    });
    // Clock of the next-episode countdown and of the "not yet aired" marks.
    let mut now = use_signal(now_unix);
    use_future(move || async move {
        loop {
            tokio::time::sleep(CLOCK_TICK).await;
            now.set(now_unix());
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

    let open_trailer = use_callback({
        let backend = backend.clone();
        move |tr: TrailerVm| {
            let backend = backend.clone();
            spawn(async move {
                if let Err(e) = backend.open_url(&tr.url()).await {
                    tracing::warn!(detail = %e.detail, "opening the trailer failed");
                    snackbar.show(t("details.trailer.failed"));
                }
            });
        }
    });
    let open_link = use_callback({
        let backend = backend.clone();
        move |url: String| {
            let backend = backend.clone();
            spawn(async move {
                if let Err(e) = backend.open_url(&url).await {
                    tracing::warn!(detail = %e.detail, "opening an external link failed");
                    snackbar.show(t("details.link.failed"));
                }
            });
        }
    });
    // Share = copy the public page (AniList, else MAL / Kitsu) to the clipboard.
    let share = use_callback({
        let id = id.clone();
        move |_: ()| {
            let facts = match &*extra.peek() {
                Some(Ok(x)) => Some(x.facts.clone()),
                _ => None,
            };
            let url = share_url(&id, facts.as_ref());
            let arg = serde_json::to_string(&url).unwrap_or_else(|_| "\"\"".into());
            let _ = document::eval(&format!("navigator.clipboard&&navigator.clipboard.writeText({arg});"));
            snackbar.show(t("details.share.copied"));
        }
    });

    let vm: Option<DetailsVm> = details.read().as_ref().and_then(|r| r.as_ref().ok()).cloned();
    let resume = vm.as_ref().map(|d| d.resume_episode).unwrap_or(0);
    let watch_ep = resume.max(1);
    let extra_state: Option<Result<DetailsExtraVm, UiError>> = (*extra.read()).clone();
    let order_state = (*order.read()).clone();
    let adult = matches!(&extra_state, Some(Ok(x)) if x.facts.adult);
    // Wave B (b, `settings.privacy.mature-content`): the "Show" mode skips the 18+ gate.
    let allowed = adult_ok()
        || adult_session()
        || ui.settings.read().home.mature_content == crate::model::home::MatureContentVm::Show;
    // Images wait until we know the title is not adult (or the viewer accepted); an error never hides them.
    let images = match &extra_state {
        None => false,
        Some(Ok(x)) => !x.facts.adult || allowed,
        Some(Err(_)) => true,
    };
    let (next_airing, trailer) = match &extra_state {
        Some(Ok(x)) => (x.next_airing, x.trailer.clone()),
        _ => (None, None),
    };
    let now_ts = now();

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
                    images,
                    next_airing,
                    trailer: trailer.clone(),
                    now: now_ts,
                    menu_open: picker_open(),
                    on_trailer: move |tr: TrailerVm| open_trailer.call(tr),
                    on_share: move |_| share.call(()),
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

    if adult && !allowed {
        let title = vm.as_ref().map(|v| v.media.title.clone()).unwrap_or_default();
        return rsx! {
            div { class: "tk-details", "data-embedded": "false", "data-adult": "true",
                AdultGate {
                    title,
                    on_back: move |_| {
                        navigator.go_back();
                    },
                    on_continue: move |_| adult_session.set(true),
                    on_always: move |_| {
                        ls_set(ADULT_KEY, "1");
                        adult_ok.set(true);
                    },
                }
            }
        };
    }

    let episodes_panel = match &*episodes.read() {
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
            EpisodeList {
                id: id.clone(),
                episodes: with_next_airing(eps.clone(), next_airing),
                resume,
                next_airing,
                now: now_ts,
                images,
            }
        },
    };
    let info_panel = match &*details.read() {
        None => rsx! { SkeletonLines {} },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), compact: true, on_retry: move |_| details.restart() }
        },
        Some(Ok(d)) => rsx! {
            InfoTab { vm: d.clone() }
            FactsSheet {
                extra: extra_state.clone(),
                episodes: d.episodes_total,
                status: d.status.clone(),
                on_retry: move |_| extra.restart(),
                on_trailer: move |tr: TrailerVm| open_trailer.call(tr),
                on_link: move |url: String| open_link.call(url),
            }
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
            // Wave B (worker k, `link.send-to-device`): `Backend::send_to_device` over Tatakai Link.
            let send_backend = backend.clone();
            let send_req = SendToDeviceVm { device_id: d.id.clone(), media_id: id.clone(), episode: watch_ep, position_secs: v.resume_secs };
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
                        let backend = send_backend.clone();
                        let req = send_req.clone();
                        spawn(async move {
                            if let Err(e) = backend.send_to_device(&req).await {
                                send_error.set(Some(e));
                            }
                        });
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
                                "data-tab": "{tb.id()}",
                                "aria-selected": if tab() == tb { "true" } else { "false" },
                                "aria-controls": "tk-dtabpanel-{tb.id()}",
                                onclick: move |_| tab.set(tb),
                                {t(tb.label_key())}
                            }
                        }
                    }
                    // Every panel stays mounted (the verifier and in-page find see all of them); only one is visible.
                    div {
                        id: "tk-dtabpanel-episodes",
                        class: "tk-details__tab",
                        role: "tabpanel",
                        "aria-labelledby": "tk-dtab-episodes",
                        hidden: tab() != Tab::Episodes,
                        {episodes_panel}
                    }
                    div {
                        id: "tk-dtabpanel-info",
                        class: "tk-details__tab",
                        role: "tabpanel",
                        "aria-labelledby": "tk-dtab-info",
                        hidden: tab() != Tab::Info,
                        {info_panel}
                    }
                    div {
                        id: "tk-dtabpanel-characters",
                        class: "tk-details__tab",
                        role: "tabpanel",
                        "aria-labelledby": "tk-dtab-characters",
                        hidden: tab() != Tab::Characters,
                        CharactersTab { extra: extra_state.clone(), images, on_retry: move |_| extra.restart() }
                    }
                    div {
                        id: "tk-dtabpanel-related",
                        class: "tk-details__tab",
                        role: "tabpanel",
                        "aria-labelledby": "tk-dtab-related",
                        hidden: tab() != Tab::Related,
                        RelatedTab { extra: extra_state.clone(), on_retry: move |_| extra.restart() }
                    }
                    WatchOrder { order: order_state.clone(), on_retry: move |_| order.restart() }
                    Recommendations { extra: extra_state.clone() }
                    // Wave B mount point (worker n): AniList reviews + the user's AniList activity on this title
                    // (renders nothing until then; plan 14 section 0.1 #10: no own comments backend).
                    Reviews { media_id: id.clone() }
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
    /// False while the adult flag is unknown or the viewer has not accepted it: the poster shows its art only.
    #[props(default = true)] images: bool,
    #[props(default)] next_airing: Option<AiringVm>,
    #[props(default)] trailer: Option<TrailerVm>,
    #[props(default)] now: i64,
    menu_open: bool,
    #[props(default)] on_watch: EventHandler<()>,
    #[props(default)] on_menu: EventHandler<()>,
    #[props(default)] on_send: EventHandler<()>,
    #[props(default)] on_download: EventHandler<()>,
    #[props(default)] on_trailer: EventHandler<TrailerVm>,
    #[props(default)] on_share: EventHandler<()>,
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
    let cover = if images { vm.media.cover.clone() } else { None };
    let airing = next_airing.map(|n| (n, airing_label(n.episode, n.at_unix, now)));
    // A plain button (not `Button`) so it can carry `data-trailer` for the verifier.
    let trailer_button = match trailer.filter(|_| !embedded) {
        Some(tr) => {
            let site = tr.site.clone();
            let label = t("details.trailer");
            rsx! {
                button {
                    class: "tk-btn tk-btn--outlined tk-btn--md",
                    r#type: "button",
                    "data-trailer": "{site}",
                    onclick: move |_| on_trailer.call(tr.clone()),
                    Icon { name: "film".to_string(), size: 20 }
                    span { "{label}" }
                }
            }
        }
        None => rsx! {},
    };
    rsx! {
        div { class: "tk-dhead", "data-embedded": "{embedded}", "data-resume-secs": "{vm.resume_secs}",
            div { class: "tk-dhead__cover", "data-images": "{images}",
                MediaCover { src: cover, color: vm.media.color.clone(), title: vm.media.title.clone() }
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
                if let Some((n, label)) = airing {
                    div {
                        class: "tk-dhead__airing",
                        "data-next-airing": "{n.episode}",
                        "data-airing-at": "{n.at_unix}",
                        role: "status",
                        Icon { name: "clock".to_string(), size: 18 }
                        span { "{label}" }
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
                        // Wave B (b, `details.track-show`): follow the title in the airing calendar.
                        crate::components::airing::TrackShowButton { media_id: vm.media.id.clone() }
                        // Wave B (j, `ext.custom-providers.bookmark`): Save / Mark as completed for provider titles.
                        if vm.media.id.starts_with("ext:") {
                            crate::screens::provider::ProviderBookmarkButton { media_id: vm.media.id.clone() }
                        }
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
                        {trailer_button}
                        IconButton {
                            icon: "link".to_string(),
                            label: t("details.share"),
                            variant: IconButtonVariant::Outlined,
                            onclick: move |_| on_share.call(()),
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

/// Info tab, top: full synopsis and genres (the facts sheet follows, `extra::FactsSheet`).
#[component]
fn InfoTab(vm: DetailsVm) -> Element {
    let synopsis = vm.synopsis.clone().unwrap_or_else(|| t("details.no_synopsis"));
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
        }
    }
}

#[component]
fn EpisodeList(
    id: String,
    episodes: Vec<EpisodeVm>,
    resume: u32,
    #[props(default)] next_airing: Option<AiringVm>,
    #[props(default)] now: i64,
    #[props(default = true)] images: bool,
) -> Element {
    let navigator = use_navigator();
    let mut query = use_signal(String::new);
    let mut view = use_signal(|| None::<EpisodeView>);
    use_future(move || async move {
        if let Some(v) = ls_get(VIEW_KEY).await.as_deref().and_then(EpisodeView::from_id) {
            view.set(Some(v));
        }
    });
    // Like the classic: thumbnails when the list has any, else compact, until the viewer picks one.
    let has_thumbs = episodes.iter().any(|e| e.image.is_some());
    let active_view = view().unwrap_or(if has_thumbs { EpisodeView::Thumbnails } else { EpisodeView::Compact });
    let compact = active_view == EpisodeView::Compact;

    let max = episodes.iter().map(|e| e.number).max().unwrap_or(0);
    let ranges = episode_ranges(max, EPISODE_RANGE);
    let mut range = use_signal(|| if resume > 0 { range_index_for(resume, EPISODE_RANGE) } else { 0 });
    let active = range().min(ranges.len().saturating_sub(1));
    let (from, to) = ranges.get(active).copied().unwrap_or((1, max));
    let q = query();
    let searching = !q.trim().is_empty();
    // A search looks through every episode, not only the selected range.
    let shown: Vec<EpisodeVm> = if searching {
        filter_episodes(&episodes, &q)
    } else {
        episodes.iter().filter(|e| e.number >= from && e.number <= to).cloned().collect()
    };
    let options: Vec<(String, String)> = ranges
        .iter()
        .enumerate()
        .map(|(i, (a, b))| (i.to_string(), tf("details.range", &[("from", &a.to_string()), ("to", &b.to_string())])))
        .collect();
    let view_options: Vec<(String, String)> = vec![
        (EpisodeView::Thumbnails.id().to_owned(), t("details.view.thumbnails")),
        (EpisodeView::Compact.id().to_owned(), t("details.view.compact")),
    ];
    let shown_count = shown.len();

    rsx! {
        div { class: "tk-deps__tools",
            div { class: "tk-deps__search", "data-episode-search": "{searching}",
                TextField {
                    label: t("details.ep_search"),
                    value: q.clone(),
                    kind: "search".to_string(),
                    placeholder: t("details.ep_search.placeholder"),
                    icon: "search".to_string(),
                    oninput: move |v: String| query.set(v),
                }
            }
            Segmented {
                options: view_options,
                selected: active_view.id().to_string(),
                label: t("details.view"),
                small: true,
                on_select: move |v: String| {
                    if let Some(x) = EpisodeView::from_id(&v) {
                        view.set(Some(x));
                        ls_set(VIEW_KEY, x.id());
                    }
                },
            }
        }
        if ranges.len() > 1 && !searching {
            Segmented {
                options,
                selected: active.to_string(),
                label: t("details.episodes"),
                small: true,
                on_select: move |v: String| range.set(v.parse().unwrap_or(0)),
            }
        }
        if searching && shown.is_empty() {
            EmptyState {
                icon: "search".to_string(),
                title: t("details.ep_search.none.title"),
                body: t("details.ep_search.none.body"),
            }
        }
        div { class: "tk-list tk-deps", "data-view": "{active_view.id()}", "data-shown": "{shown_count}",
            for e in shown {
                {
                    let (id, n) = (id.clone(), e.number);
                    let unaired = is_unaired(&e, next_airing, now);
                    let mut pills: Vec<(String, Tone)> = Vec::new();
                    if unaired {
                        pills.push((t("details.unaired"), Tone::Primary));
                    }
                    pills.extend(episode_pills(&e, resume).into_iter().map(|(k, tone)| (t(k), tone)));
                    rsx! {
                        DetailsEpisodeRow {
                            key: "{n}",
                            episode: e.clone(),
                            current: n == resume,
                            pills,
                            unaired,
                            compact,
                            images,
                            onclick: move |_| {
                                if !unaired {
                                    navigator.push(Route::PlayerPage { id: id.clone(), episode: n });
                                }
                            },
                        }
                    }
                }
            }
        }
    }
}

/// Episode row of this screen (thumb with progress, `n. title`, pills, air date). Compact rows drop the thumb;
/// unaired rows are disabled.
#[component]
fn DetailsEpisodeRow(
    episode: EpisodeVm,
    current: bool,
    pills: Vec<(String, Tone)>,
    #[props(default)] unaired: bool,
    #[props(default)] compact: bool,
    #[props(default = true)] images: bool,
    #[props(default)] onclick: EventHandler<MouseEvent>,
) -> Element {
    let n = episode.number;
    let heading = match episode.title.clone() {
        Some(title) => format!("{n}. {title}"),
        None => tf("details.episode", &[("n", &n.to_string())]),
    };
    let aired = episode.air_date.clone().map(|d| {
        let day = d.get(..10).unwrap_or(d.as_str()).to_owned();
        if unaired { tf("details.unaired_on", &[("d", &day)]) } else { tf("details.aired", &[("d", &day)]) }
    });
    let in_progress = episode.watched > 0.0 && episode.watched < WATCHED_AT;
    let bar = (episode.watched * 100.0).round();
    let tip = if unaired { tf("details.unaired_title", &[("title", &heading)]) } else { heading.clone() };
    rsx! {
        button {
            class: "tk-dep",
            r#type: "button",
            title: "{tip}",
            "aria-current": "{current}",
            "data-compact": "{compact}",
            "data-unaired": "{unaired}",
            "data-episode": "{n}",
            disabled: unaired,
            "aria-disabled": "{unaired}",
            onclick: move |e| onclick.call(e),
            if compact {
                span { class: "tk-dep__num", "aria-hidden": "true", "{n}" }
            } else {
                div { class: "tk-dep__thumb",
                    if let Some(img) = episode.image.clone().filter(|_| images) {
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
    fn air_dates_parse_with_and_without_time() {
        assert_eq!(parse_air_unix("1970-01-02"), Some(86_400));
        assert_eq!(parse_air_unix("2023-09-29T14:00:00Z"), Some(1_695_996_000));
        assert_eq!(parse_air_unix("2023-09-29T23:00:00+09:00"), Some(1_695_996_000));
        assert_eq!(parse_air_unix("2023-09-29 14:00"), Some(1_695_996_000));
        assert_eq!(parse_air_unix("2023-13-01"), None);
        assert_eq!(parse_air_unix("soon"), None);
        assert_eq!(unix_to_date(1_695_996_000), "2023-09-29");
        assert_eq!(unix_to_date(0), "1970-01-01");
    }

    #[test]
    fn unaired_uses_next_airing_and_future_dates() {
        let mut e = ep(5, 0.0, false);
        let next = Some(AiringVm { episode: 5, at_unix: 2_000_000_000 });
        assert!(is_unaired(&e, next, 1_900_000_000));
        assert!(!is_unaired(&ep(4, 0.0, false), next, 1_900_000_000));
        e.air_date = Some("2030-01-01".into());
        assert!(is_unaired(&e, None, 1_700_000_000));
        assert!(!is_unaired(&e, None, 1_900_000_000));
        assert!(!is_unaired(&ep(1, 0.0, false), None, 0));
    }

    #[test]
    fn next_airing_adds_one_schedule_row() {
        let eps = vec![ep(1, 0.0, false), ep(2, 0.0, false)];
        let next = Some(AiringVm { episode: 3, at_unix: 1_695_996_000 });
        let out = with_next_airing(eps.clone(), next);
        assert_eq!(out.len(), 3);
        assert_eq!(out[2].air_date.as_deref(), Some("2023-09-29"));
        // Already listed or far ahead: unchanged.
        assert_eq!(with_next_airing(eps.clone(), Some(AiringVm { episode: 2, at_unix: 0 })).len(), 2);
        assert_eq!(with_next_airing(eps, Some(AiringVm { episode: 9, at_unix: 0 })).len(), 2);
    }

    #[test]
    fn episode_search_matches_number_or_title() {
        let mut a = ep(12, 0.0, false);
        a.title = Some("The Journey Begins".into());
        let mut b = ep(112, 0.0, false);
        b.title = Some("Twelve Swords".into());
        let list = vec![a, b, ep(3, 0.0, false)];
        let n: Vec<u32> = filter_episodes(&list, "12").iter().map(|e| e.number).collect();
        assert_eq!(n, vec![12]);
        let n: Vec<u32> = filter_episodes(&list, "journey").iter().map(|e| e.number).collect();
        assert_eq!(n, vec![12]);
        assert_eq!(filter_episodes(&list, "  ").len(), 3);
        assert!(filter_episodes(&list, "nothing").is_empty());
    }

    #[test]
    fn episode_view_ids_round_trip() {
        for v in [EpisodeView::Thumbnails, EpisodeView::Compact] {
            assert_eq!(EpisodeView::from_id(v.id()), Some(v));
        }
        assert_eq!(EpisodeView::from_id("grid"), None);
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
