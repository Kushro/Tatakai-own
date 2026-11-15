//! Details: header with metadata chips, the next-episode countdown, the Watch/Continue split button (its chevron
//! opens the source picker), list / share / trailer actions; tabs Episodes / Info / Characters / Relations with a
//! sliding indicator (Episodes and Info mount at once, the other two when first opened); episodes in ranges of 100
//! with search and three views (`details/episodes.rs`: cards, list, compact), Watched / Filler / Not yet aired
//! pills; the relations graph or grouped list (`details/relations.rs`); the watch order of the franchise and AniList
//! recommendations under the tabs; the source summary and "send to device" side panel; a sticky floating toolbar;
//! the adult content gate. `DetailsView { embedded: true }` is the right pane of Search on wide windows (header
//! only). Wave A (worker c): extras come from `Backend::details_extra` / `watch_order` (`details/extra.rs`).

mod episodes;
mod extra;
mod relations;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dioxus::prelude::*;

use crate::app::{Route, Services, Ui};
use crate::backend::{
    AiringVm, DetailsExtraVm, DetailsVm, DeviceVm, EpisodeVm, LinkStatusVm, MediaFactsVm, SendOutcomeVm, SendProgress, SendStep, SendToDeviceVm, SourceVm,
    TrailerVm,
};
use crate::components::title_bar::ShellStatus;
use crate::components::{
    Button, ButtonSize, ButtonVariant, ConfirmDialog, DownloadSeasonSheet, EmptyState, ErrorState,
    FloatingToolbar, IconButton, IconButtonVariant, InfoChip, ListButton, LoadingIndicator, MediaCover, Menu, MenuItem, Pill,
    Reviews,
    Sheet, SkeletonLines, SourcePicker, SourceSummary, SourcesError, SplitButton, ToolbarSeparator, Tone,
    missing_pick_label, use_snackbar,
};
use crate::i18n::{t, tf, tn};
use crate::icons::Icon;
use crate::model::{UiError, format_time};
use crate::screens::extensions::rel_time;
pub(crate) use extra::{country_label, source_label};
use episodes::EpisodeList;
use extra::{AdultGate, CharactersTab, FactsSheet, Recommendations, WatchOrder, airing_label, share_url};
use relations::RelationsTab;

/// From this fraction an episode counts as watched ("Watched" pill).
pub const WATCHED_AT: f64 = 0.9;
/// Devices listed in the "Send to device" panel.
const SIDE_DEVICES: usize = 2;
/// Synopses longer than this get the More / Less toggle.
const CLAMP_CHARS: usize = 120;
/// Web view `localStorage` key (per-viewer convenience; the backend has no UI-state store). The episode view moved
/// to `Settings::look.episode_view` (2026-11 UI refresh); the old `tk.details.episodeView` key is ignored.
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
            Self::Related => "episodes.tab.relations",
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }

    /// Characters and Relations mount the first time they are opened (Characters loads every image).
    fn lazy(self) -> bool {
        matches!(self, Self::Characters | Self::Related)
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
                ..EpisodeVm::default()
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
/// "Watched", and neither does a row with the "mark watched" check (`with_check`: the filled check already says
/// it, BUG-073). `EpisodeVm` has no recap flag yet, so "Recap" is never produced.
pub fn episode_pills(e: &EpisodeVm, resume: u32, with_check: bool) -> Vec<(&'static str, Tone)> {
    let mut out = Vec::new();
    if e.is_filler {
        out.push(("details.filler", Tone::Warn));
    }
    if e.watched >= WATCHED_AT && e.number != resume && !with_check {
        out.push(("details.watched", Tone::Ok));
    }
    out
}

/// Reports whether the header's actions row (Watch / Continue) is inside the viewport, on every scroll (any
/// scroller, capture phase) and resize, only when it changes. No row (still loading) counts as hidden.
const HEADER_ACTIONS_JS: &str = r#"
if (window.__tkHeadActionsOff) window.__tkHeadActionsOff();
let last = null, queued = false;
const check = () => {
  queued = false;
  const el = document.querySelector('.tk-details[data-embedded="false"] .tk-dhead__actions');
  let v = false;
  if (el) { const r = el.getBoundingClientRect(); v = r.bottom > 0 && r.top < window.innerHeight && r.height > 0; }
  if (v !== last) { last = v; dioxus.send(v); }
};
const later = () => { if (!queued) { queued = true; requestAnimationFrame(check); } };
document.addEventListener('scroll', later, true);
window.addEventListener('resize', later);
const mo = new MutationObserver(later);
mo.observe(document.body, { childList: true, subtree: true });
// One watcher at a time: the next Details screen removes this one.
window.__tkHeadActionsOff = () => { document.removeEventListener('scroll', later, true); window.removeEventListener('resize', later); mo.disconnect(); };
check();
"#;

/// The requested device, else the one connected now seen most recently, else the one seen most recently (BUG-039:
/// never a tablet last seen 19 h ago over the phone that is connected), else the first paired one.
pub fn pick_device(list: &[DeviceVm], wanted: Option<&str>) -> Option<DeviceVm> {
    let latest = |online: bool| list.iter().filter(|d| !online || d.online).max_by_key(|d| d.last_seen_unix.unwrap_or(i64::MIN));
    wanted
        .and_then(|id| list.iter().find(|d| d.id == id))
        .or_else(|| latest(true))
        .or_else(|| list.iter().filter(|d| d.last_seen_unix.is_some()).max_by_key(|d| d.last_seen_unix))
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
    // BUG-130: the devices load on their own when the page opens: the shell's last Link answer (its poll follows the
    // server and who is online) or, before that, `link_peek()`, which never starts the server (no firewall prompt).
    let shell_link = try_use_context::<ShellStatus>().map(|s| s.link);
    let mut link = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            // Read (not peeked): the list follows the shell's poll.
            let polled = shell_link.and_then(|l| l());
            async move {
                match link_query(embedded, polled) {
                    LinkQuery::Skip => Ok(LinkStatusVm::default()),
                    LinkQuery::Known(st) => Ok(st),
                    LinkQuery::Ask => backend.link_peek().await,
                }
            }
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
    // Lazy panels already opened once (they stay mounted afterwards).
    let mut opened = use_signal(|| [false; 4]);
    let mut picker_open = use_signal(|| false);
    // BUG-078: the floating toolbar shows only once the header's Watch / Continue row has scrolled away.
    let mut header_actions_visible = use_signal(|| false);
    use_future(move || async move {
        let mut eval = document::eval(HEADER_ACTIONS_JS);
        while let Ok(v) = eval.recv::<bool>().await {
            if *header_actions_visible.peek() != v {
                header_actions_visible.set(v);
            }
        }
    });
    // Wave B mount point (worker h): the "Download season" sheet; the contract only opens / closes it.
    let mut download_open = use_signal(|| false);
    let mut picker_ep = use_signal(|| 1_u32);
    let mut picker_list = use_signal(|| None::<Result<Vec<SourceVm>, UiError>>);
    // BUG-142 parity: the sheet shows the last known list (maybe partial) until `Backend::sources` answers, and says
    // nothing of a missing pick meanwhile. Each open bumps the generation, so an older refresh does not end the wait.
    let mut picker_refresh_gen = use_signal(|| 0_u64);
    let mut picker_refreshing = use_signal(|| false);
    let mut send_target = use_signal(|| None::<DeviceVm>);
    let mut send_error = use_signal(|| None::<UiError>);
    // plan §9.1 / §11.4: "Se envió X porque…" when another source than the chosen one went to the phone
    let mut send_notice = use_signal(|| None::<String>);
    // BUG-107: the send in flight ("Enviando a moto g51… · Preparando la fuente…" + Cancelar); it took 25-28 s unseen
    let mut sending = use_signal(|| None::<Sending>);
    let cancel_send = use_callback(move |_: ()| {
        let Some(s) = sending.peek().clone() else { return };
        if s.progress.cancel() {
            sending.set(None);
            snackbar.show(t("details.send_cancelled"));
        }
    });

    // The ▾ of the split button and "See all sources" open the same picker. BUG-139: it opens at once on the list
    // Details already has (`known_sources`) and swaps in the fresh one when `sources` answers; with nothing known it
    // waits with the `src.resolving` line.
    let open_picker = use_callback({
        let (backend, id) = (backend.clone(), id.clone());
        move |ep: u32| {
            picker_ep.set(ep);
            picker_list.set(picker_seed(backend.known_sources(&id, ep)));
            let refresh = *picker_refresh_gen.peek() + 1;
            picker_refresh_gen.set(refresh);
            picker_refreshing.set(true);
            picker_open.set(true);
            let (backend, id) = (backend.clone(), id.clone());
            spawn(async move {
                let result = backend.sources(&id, ep).await;
                if *picker_refresh_gen.peek() == refresh {
                    let next = picker_refresh(picker_list.peek().clone(), result);
                    picker_list.set(next);
                    picker_refreshing.set(false);
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
                // Chosen right now: Manual mode plays it without asking again (BUG-052).
                *crate::screens::player::CONFIRMED_PICK.write() = Some((id.clone(), ep, source_id));
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

    // No paired device: go pair one (Link) instead of opening an empty dialog. BUG-039: when the Link status is not
    // loaded yet (or was never asked this session), ask for it now instead of jumping to the Link page.
    let ask_send = use_callback({
        let backend = backend.clone();
        move |device: Option<String>| {
            if sending.peek().is_some() {
                return;
            }
            let target = match &*link.read() {
                Some(Ok(st)) => pick_device(&st.devices, device.as_deref()),
                _ => None,
            };
            if let Some(d) = target {
                send_error.set(None);
                send_target.set(Some(d));
                return;
            }
            let backend = backend.clone();
            spawn(async move {
                match backend.link_status().await.ok().and_then(|st| pick_device(&st.devices, device.as_deref())) {
                    Some(d) => {
                        send_error.set(None);
                        send_target.set(Some(d));
                        link.restart();
                    }
                    None => {
                        navigator.push(Route::LinkPage {});
                    }
                }
            });
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
                    facts: extra_state.as_ref().and_then(|r| r.as_ref().ok()).map(|x| x.facts.clone()),
                }
            }
        }
    };

    let picker = if picker_open() {
        match picker_list() {
            None => rsx! {
                Sheet { title: t("src.title"), wide: true, on_close: move |_| picker_open.set(false),
                    div { class: "tk-details__sheetstate", role: "status", "data-resolving": "true",
                        LoadingIndicator {}
                        p { class: "tk-muted", {t("src.resolving")} }
                    }
                }
            },
            Some(Err(e)) => rsx! {
                Sheet { title: t("src.title"), wide: true, on_close: move |_| picker_open.set(false),
                    SourcesError { error: e.clone(), on_retry: move |_| open_picker.call(*picker_ep.peek()) }
                }
            },
            Some(Ok(list)) => rsx! {
                SourcePicker {
                    open: true,
                    missing_pick: missing_pick_label(
                        &list,
                        backend.picked_source_key(&id, *picker_ep.peek()).as_deref(),
                        picker_refreshing(),
                    ),
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

    // Manual watched / not watched (AniList-first progress): reload the episodes and the resume button after it.
    let is_provider = id.starts_with("ext:");
    let on_toggle = EventHandler::new({
        let (backend, id) = (backend.clone(), id.clone());
        move |(n, watched): (u32, bool)| {
            let (backend, id) = (backend.clone(), id.clone());
            spawn(async move {
                match backend.set_episode_watched(&id, n, watched).await {
                    Ok(()) => {
                        episodes.restart();
                        details.restart();
                    }
                    Err(e) => snackbar.show(tf("details.mark_failed", &[("detail", &e.detail)])),
                }
            });
        }
    });

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
                on_toggle: (!is_provider).then_some(on_toggle),
                fallback_image: vm.as_ref().and_then(|d| d.banner.clone().or_else(|| d.media.cover.clone())),
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
    // Only once the details (and so the episode to continue) are known: rendering it earlier resolved episode 1 first,
    // and that ~40 s resolution held the extension's single JS thread while the real episode waited behind it.
    let sources_panel = if has_source_ext && vm.is_some() {
        rsx! {
            // BUG-077: no outer "Stream source" title: the card's own "Episode sources" is the only heading (as on
            // Android); the old hint is the section's tooltip.
            section { class: "tk-details__sources", title: t("details.provider_hint"),
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
        // BUG-130: no paired device (not "not seen since pairing", which is about one device).
        Some(Ok(st)) if st.devices.is_empty() => rsx! {
            p { class: "tk-muted", {t("details.link.no_devices")} }
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
                        let this_busy = sending().as_ref().is_some_and(|s| s.device_id == d.id);
                        let kind = if this_busy {
                            t("details.sending")
                        } else if d.online {
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
                                disabled: sending().is_some(),
                                "aria-busy": "{this_busy}",
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
            let send_req = SendToDeviceVm {
                device_id: d.id.clone(),
                media_id: id.clone(),
                episode: watch_ep,
                position_secs: v.resume_secs,
                progress: SendProgress::default(),
            };
            let device_name = d.name.clone();
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
                        // BUG-107: a fresh progress per send; the card and a snackbar say so right away
                        let progress = SendProgress::new();
                        let req = SendToDeviceVm { progress: progress.clone(), ..send_req.clone() };
                        let device = device_name.clone();
                        send_notice.set(None);
                        send_error.set(None);
                        sending.set(Some(Sending {
                            progress: progress.clone(),
                            device_id: req.device_id.clone(),
                            device: device.clone(),
                            step: SendStep::Preparing,
                        }));
                        snackbar.show_action(tf("details.sending_to", &[("device", &device)]), t("common.cancel"), cancel_send);
                        let watcher = spawn({
                            let progress = progress.clone();
                            async move {
                                loop {
                                    progress.changed().await;
                                    let step = progress.step();
                                    let mut w = sending.write();
                                    match w.as_mut() {
                                        Some(s) if s.progress == progress => s.step = step,
                                        _ => break,
                                    }
                                }
                            }
                        });
                        spawn(async move {
                            let result = backend.send_to_device(&req).await;
                            watcher.cancel();
                            if sending.peek().as_ref().is_some_and(|s| s.progress == progress) {
                                sending.set(None);
                            }
                            if progress.is_cancelled() {
                                return;
                            }
                            match result {
                                Ok(outcome) => {
                                    let text = send_outcome_text(&outcome)
                                        .unwrap_or_else(|| tf("details.send_done", &[("device", &device)]));
                                    snackbar.show(text.clone());
                                    send_notice.set(Some(text));
                                }
                                Err(e) => send_error.set(Some(e)),
                            }
                        });
                    },
                }
            }
        }
        _ => rsx! {},
    };

    // UX review #8: the floating toolbar steps aside while a sheet or dialog is open (it covered their lists).
    // BUG-078: and while the header's own Watch / Continue button is on screen (it repeated it).
    let overlay_open = picker_open() || download_open() || send_target().is_some();
    let toolbar = match vm.as_ref() {
        Some(v) if !overlay_open && !header_actions_visible() => {
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
        _ => rsx! {},
    };

    let tablist_label = t("details.info");
    rsx! {
        div { class: "tk-details", "data-embedded": "false",
            {header}
            div { class: "tk-details__layout",
                div { class: "tk-section tk-details__main",
                    div {
                        class: "tk-dtabs",
                        role: "tablist",
                        "aria-label": "{tablist_label}",
                        style: "--tk-dtab-i: {tab().index()}; --tk-dtab-n: {Tab::ALL.len()}",
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
                                onclick: move |_| {
                                    if tb.lazy() && !opened.peek()[tb.index()] {
                                        opened.write()[tb.index()] = true;
                                    }
                                    tab.set(tb);
                                },
                                {t(tb.label_key())}
                            }
                        }
                        // One indicator for the row, slid under the active tab with a transform.
                        span { class: "tk-dtabs__indicator", "aria-hidden": "true" }
                    }
                    // Episodes and Info mount at once; Characters and Relations the first time they are opened, then
                    // stay mounted (only the active panel is visible).
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
                        if opened()[Tab::Characters.index()] || tab() == Tab::Characters {
                            CharactersTab { extra: extra_state.clone(), images, on_retry: move |_| extra.restart() }
                        }
                    }
                    div {
                        id: "tk-dtabpanel-related",
                        class: "tk-details__tab",
                        role: "tabpanel",
                        "aria-labelledby": "tk-dtab-related",
                        hidden: tab() != Tab::Related,
                        if opened()[Tab::Related.index()] || tab() == Tab::Related {
                            RelationsTab {
                                extra: extra_state.clone(),
                                order: order_state.clone(),
                                current: vm.as_ref().map(|v| v.media.clone()),
                                on_retry: move |_| extra.restart(),
                            }
                        }
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
                        if let Some(s) = sending() {
                            div { class: "tk-details__sending", role: "status", "aria-live": "polite",
                                LoadingIndicator { small: true }
                                span { class: "tk-details__sending-text",
                                    span { class: "tk-details__sending-title", {tf("details.sending_to", &[("device", &s.device)])} }
                                    span { class: "tk-muted", {send_step_text(s.step)} }
                                }
                                Button {
                                    label: t("common.cancel"),
                                    variant: ButtonVariant::Text,
                                    size: ButtonSize::Sm,
                                    disabled: !s.progress.can_cancel(),
                                    onclick: move |_| cancel_send.call(()),
                                }
                            }
                        }
                        if let Some(err) = send_error() {
                            ErrorState { error: err.clone(), compact: true, retry: false }
                        }
                        if let Some(note) = send_notice() {
                            p { class: "tk-muted tk-details__send-note", role: "status", "{note}" }
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
    /// Facts of `Backend::details_extra` (format, season, studio...) for the data tiles; `None` while loading.
    #[props(default)] facts: Option<MediaFactsVm>,
) -> Element {
    let mut clamp = use_signal(|| true);
    let mut more_open = use_signal(|| false);
    let watch_label = if vm.resume_episode > 0 {
        tf("details.continue", &[("n", &vm.resume_episode.to_string())])
    } else {
        tf("details.watch_ep", &[("n", "1")])
    };
    let size = if embedded { ButtonSize::Md } else { ButtonSize::Lg };
    let synopsis = vm.synopsis.clone().unwrap_or_else(|| t("details.no_synopsis"));
    let long = synopsis.chars().count() > CLAMP_CHARS;
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
    // UX review #2: a blurred banner behind the header (the classic's look), only with images allowed.
    let backdrop = (images && !embedded)
        // 0.3.2: the ani.zip fanart (full size) first, then AniList's banner strip, then the poster.
        .then(|| vm.fanart.clone().or_else(|| vm.banner.clone()).or_else(|| vm.media.cover.clone()))
        .flatten()
        .filter(|u| u.starts_with("https://") || u.starts_with("http://"))
        .map(|u| u.replace(['\'', '"', '(', ')', '\\'], ""));
    let tiles = meta_tiles(&vm, facts.as_ref());
    rsx! {
        div { class: "tk-dhead", "data-embedded": "{embedded}", "data-resume-secs": "{vm.resume_secs}",
            if let Some(src) = backdrop {
                div { class: "tk-dhead__backdrop", "aria-hidden": "true", style: "background-image: url('{src}')" }
            }
            div { class: "tk-dhead__cover", "data-images": "{images}",
                MediaCover { src: cover, color: vm.media.color.clone(), title: vm.media.title.clone() }
            }
            div { class: "tk-dhead__text",
                h1 { class: "tk-dhead__title", "{vm.media.title}" }
                div { class: "tk-chips",
                    if let Some(status) = vm.status.clone() {
                        Pill { text: status_label(&status) }
                    }
                    if !vm.media.subtitle.is_empty() {
                        InfoChip { text: vm.media.subtitle.clone() }
                    }
                    // The subtitle already says "2023 - 28 ep": no second episode-count chip (UX review).
                    if let Some(score) = vm.media.score {
                        // One score scale everywhere (UX review #4): the cards' "★ 8.4".
                        InfoChip { text: score_label(score), icon: "star".to_string() }
                    }
                    for g in vm.genres.clone() {
                        InfoChip { key: "{g}", text: crate::screens::search::genre_label(&g) }
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
                        {trailer_button}
                        // UX review #2: the rest of the actions in one menu instead of a second row of buttons.
                        Menu {
                            open: more_open(),
                            label: t("details.more_actions"),
                            on_close: move |_| more_open.set(false),
                            trigger: rsx! {
                                IconButton {
                                    icon: "more".to_string(),
                                    label: t("details.more_actions"),
                                    variant: IconButtonVariant::Outlined,
                                    onclick: move |_| more_open.toggle(),
                                }
                            },
                            MenuItem {
                                label: t("details.send_device"),
                                icon: "cast".to_string(),
                                on_select: move |_| {
                                    more_open.set(false);
                                    on_send.call(());
                                },
                            }
                            MenuItem {
                                label: t("details.download"),
                                icon: "download".to_string(),
                                on_select: move |_| {
                                    more_open.set(false);
                                    on_download.call(());
                                },
                            }
                            MenuItem {
                                label: t("details.share"),
                                icon: "link".to_string(),
                                on_select: move |_| {
                                    more_open.set(false);
                                    on_share.call(());
                                },
                            }
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
                if !embedded && !tiles.is_empty() {
                    // UX review #2: useful facts (format, season, studio, length, popularity) instead of tiles that
                    // repeated the chips ("Estado RELEASING", "Géneros 3").
                    div { class: "tk-dmeta",
                        for (k , v) in tiles {
                            MetaItem { key: "{k}", k: k.clone(), v }
                        }
                    }
                }
            }
        }
    }
}

/// AniList status (`RELEASING`, `FINISHED`...) in the UI language; unknown values pass through.
pub fn status_label(raw: &str) -> String {
    let key = format!("details.status.{}", raw.trim().to_ascii_lowercase());
    let text = t(&key);
    if text == key { raw.to_owned() } else { text }
}

/// AniList 0..=100 score as the cards show it: `8.4`.
pub fn score_label(score: u32) -> String {
    format!("{:.1}", f64::from(score) / 10.0)
}

/// Header data tiles: format, season, studio, episode length, popularity (only the known ones).
fn meta_tiles(vm: &DetailsVm, facts: Option<&MediaFactsVm>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let Some(f) = facts else {
        if let Some(n) = vm.episodes_total {
            out.push((t("details.episodes"), n.to_string()));
        }
        return out;
    };
    if let Some(fmt) = f.format.as_deref() {
        let key = format!("details.format.{}", fmt.to_ascii_lowercase());
        let label = t(&key);
        out.push((t("details.meta.format"), if label == key { fmt.to_owned() } else { label }));
    }
    if let (Some(season), Some(year)) = (f.season.as_deref(), f.season_year) {
        out.push((t("details.meta.season"), format!("{} {year}", t(&format!("details.season.{season}")))));
    }
    if let Some(studio) = f.studios.first() {
        out.push((t("details.studio"), studio.clone()));
    }
    if let Some(min) = f.duration_min {
        let eps = vm.episodes_total.map(|n| format!(" · {}", tn("details.episodes_count", n as usize))).unwrap_or_default();
        out.push((t("details.meta.length"), format!("{}{eps}", tf("details.meta.minutes", &[("n", &min.to_string())]))));
    }
    if let Some(p) = f.popularity {
        out.push((t("details.facts.popularity"), p.to_string()));
    }
    out
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

/// Plan §9.1 / §11.4: the notice after "Enviar a dispositivo" when another source than the chosen one went (never a
/// technical id: the source is named as the lists name it). `None` = the chosen source was sent.
/// BUG-107: a "Enviar a dispositivo" in flight.
#[derive(Debug, Clone, PartialEq)]
struct Sending {
    progress: SendProgress,
    device_id: String,
    device: String,
    step: SendStep,
}

/// BUG-107: the step line under "Enviando a <device>…".
pub(crate) fn send_step_text(step: SendStep) -> String {
    t(match step {
        SendStep::Preparing => "details.send_step_preparing",
        SendStep::Starting => "details.send_step_starting",
    })
}

pub(crate) fn send_outcome_text(o: &SendOutcomeVm) -> Option<String> {
    let name = o.sent_instead.as_deref()?.trim();
    let source = if name.is_empty() { t("details.send_other_source") } else { name.to_owned() };
    Some(tf(if o.torrent_failed { "details.send_other_torrent" } else { "details.send_other" }, &[("source", &source)]))
}

/// Where the "Send to device" card takes its Link status from (BUG-130).
#[derive(Debug, Clone, PartialEq)]
enum LinkQuery {
    /// The embedded pane shows no card.
    Skip,
    /// The shell already holds an answer (its poll, or the Link page).
    Known(LinkStatusVm),
    /// Ask `Backend::link_peek()` (never starts the server).
    Ask,
}

/// BUG-139: what the sources sheet shows the moment it opens: the last list Details got (`Backend::known_sources`),
/// or `None` (the indicator and `src.resolving`) when this episode was never listed. Pure.
fn picker_seed(known: Option<Vec<SourceVm>>) -> Option<Result<Vec<SourceVm>, UiError>> {
    known.filter(|l| !l.is_empty()).map(Ok)
}

/// BUG-139: the sheet once `Backend::sources` answers: the fresh list replaces the shown one; a failed refresh keeps a
/// list already on screen (it is the one Details shows) and only an empty sheet shows the error. Pure.
fn picker_refresh(
    shown: Option<Result<Vec<SourceVm>, UiError>>,
    fresh: Result<Vec<SourceVm>, UiError>,
) -> Option<Result<Vec<SourceVm>, UiError>> {
    match (shown, fresh) {
        (Some(Ok(list)), Err(e)) => {
            tracing::warn!(kind = ?e.kind, detail = %e.detail, "refreshing the sources sheet failed; keeping the known list");
            Some(Ok(list))
        }
        (_, fresh) => Some(fresh),
    }
}

fn link_query(embedded: bool, polled: Option<LinkStatusVm>) -> LinkQuery {
    match (embedded, polled) {
        (true, _) => LinkQuery::Skip,
        (false, Some(st)) => LinkQuery::Known(st),
        (false, None) => LinkQuery::Ask,
    }
}

#[cfg(test)]
mod picker_tests {
    use super::*;
    use crate::backend::{ReleaseTagsVm, SourceKindVm};
    use crate::model::ErrorKind;

    fn row(label: &str) -> SourceVm {
        SourceVm {
            id: format!("ext|torrent|{label}"),
            ext_id: "ext".to_owned(),
            ext_name: "Ext".to_owned(),
            kind: SourceKindVm::Torrent,
            label: label.to_owned(),
            quality: "1080p".to_owned(),
            container: None,
            subs: Vec::new(),
            dubs: Vec::new(),
            reliability: 0.8,
            note: None,
            latency_ms: None,
            size_bytes: None,
            seeders: None,
            leechers: None,
            info_hash: None,
            ext_enabled: true,
            score: 0.0,
            recommended: false,
            picked: false,
            tags: ReleaseTagsVm::default(),
            debrid: None,
            embed: false,
        }
    }
    fn labels(v: &Option<Result<Vec<SourceVm>, UiError>>) -> Option<Vec<String>> {
        v.as_ref().and_then(|r| r.as_ref().ok()).map(|l| l.iter().map(|s| s.label.clone()).collect())
    }

    #[test]
    fn the_sheet_opens_on_the_known_list_and_takes_the_fresh_one() {
        // BUG-139: Details already showed 34 sources; the sheet waited ~30 s on an indicator for the same list.
        let seeded = picker_seed(Some(vec![row("ToonsHub"), row("ASW")]));
        assert_eq!(labels(&seeded), Some(vec!["ToonsHub".to_owned(), "ASW".to_owned()]));
        let fresh = picker_refresh(seeded.clone(), Ok(vec![row("ASW"), row("Judas"), row("ToonsHub")]));
        assert_eq!(labels(&fresh), Some(vec!["ASW".to_owned(), "Judas".to_owned(), "ToonsHub".to_owned()]));
        // A failed refresh keeps what is on screen.
        let kept = picker_refresh(seeded, Err(UiError::new(ErrorKind::Other, "timeout")));
        assert_eq!(labels(&kept), Some(vec!["ToonsHub".to_owned(), "ASW".to_owned()]));
    }

    #[test]
    fn with_nothing_known_the_sheet_waits_and_shows_errors() {
        assert!(picker_seed(None).is_none());
        assert!(picker_seed(Some(Vec::new())).is_none());
        assert!(matches!(picker_refresh(None, Err(UiError::new(ErrorKind::Other, "x"))), Some(Err(_))));
        assert_eq!(labels(&picker_refresh(None, Ok(vec![row("ASW")]))), Some(vec!["ASW".to_owned()]));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_send_card_asks_for_the_devices_without_waiting_for_the_link_page() {
        // BUG-130: right after launch (the shell has no answer yet) Details asks `link_peek()` itself
        assert_eq!(link_query(false, None), LinkQuery::Ask);
        let st = LinkStatusVm { running: true, port: 47321, devices: Vec::new(), error: None };
        assert_eq!(link_query(false, Some(st.clone())), LinkQuery::Known(st.clone()));
        assert_eq!(link_query(true, Some(st)), LinkQuery::Skip);
        assert_eq!(link_query(true, None), LinkQuery::Skip);
        assert_ne!(t("details.link.no_devices"), t("link.never_seen"));
    }

    use super::*;

    fn ep(n: u32, watched: f64, filler: bool) -> EpisodeVm {
        EpisodeVm { number: n, title: None, image: None, air_date: None, is_filler: filler, watched, ..EpisodeVm::default() }
    }

    fn dev(id: &str, online: bool) -> DeviceVm {
        DeviceVm { id: id.into(), name: id.into(), platform: "android".into(), last_seen_unix: None, online, ..Default::default() }
    }

    #[test]
    fn send_notice_names_the_source_sent_instead() {
        // plan §9.1 / §11.4 (tests run in English: nothing in the test build switches the language)
        assert_eq!(send_outcome_text(&SendOutcomeVm::default()), None);
        let torrent = SendOutcomeVm { sent_instead: Some("anizone · 1080p".into()), torrent_failed: true };
        assert_eq!(send_outcome_text(&torrent).unwrap(), "Sent anizone · 1080p because the PC could not open the chosen torrent.");
        let web = SendOutcomeVm { sent_instead: Some(" ".into()), torrent_failed: false };
        assert_eq!(send_outcome_text(&web).unwrap(), "Sent another web source because the chosen source could not be sent.");
    }

    #[test]
    fn the_sending_title_wraps_instead_of_cutting_the_device_name() {
        // BUG-116: "Enviando a moto g51 5G…" beside "Cancelar" in the 360 px side column: two lines, never an ellipsis
        let css = include_str!("../../assets/css/details.css");
        let rule = css.lines().find(|l| l.starts_with(".tk-details__sending-title {")).expect("sending title rule");
        assert!(!rule.contains("nowrap") && !rule.contains("ellipsis"), "{rule}");
        assert!(rule.contains("-webkit-line-clamp: 2") && rule.contains("white-space: normal"), "{rule}");
        assert!(css.contains(".tk-details__sending > .tk-btn { flex: none; }"));
    }

    #[test]
    fn pills_skip_watched_on_the_resume_episode() {
        assert_eq!(episode_pills(&ep(3, 1.0, true), 5, false), vec![("details.filler", Tone::Warn), ("details.watched", Tone::Ok)]);
        assert!(episode_pills(&ep(5, 1.0, false), 5, false).is_empty());
        assert!(episode_pills(&ep(6, 0.5, false), 5, false).is_empty());
        // BUG-073: with the "mark watched" check the pill would repeat it.
        assert_eq!(episode_pills(&ep(3, 1.0, true), 5, true), vec![("details.filler", Tone::Warn)]);
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
    fn device_choice_prefers_wanted_then_online() {
        let list = [dev("a", false), dev("b", true)];
        assert_eq!(pick_device(&list, Some("a")).unwrap().id, "a");
        assert_eq!(pick_device(&list, None).unwrap().id, "b");
        assert_eq!(pick_device(&list, Some("zz")).unwrap().id, "b");
        assert_eq!(pick_device(&[dev("a", false)], None).unwrap().id, "a");
        assert!(pick_device(&[], None).is_none());
    }

    #[test]
    fn device_choice_never_prefers_a_stale_device_bug_039() {
        let mut tab = dev("tab", false);
        tab.last_seen_unix = Some(1_000);
        let mut moto = dev("moto", false);
        moto.last_seen_unix = Some(70_000);
        assert_eq!(pick_device(&[tab.clone(), moto.clone()], None).unwrap().id, "moto", "most recently seen when none is online");
        let mut moto_on = moto.clone();
        moto_on.online = true;
        let mut other_on = dev("other", true);
        other_on.last_seen_unix = Some(60_000);
        assert_eq!(pick_device(&[tab, other_on, moto_on], None).unwrap().id, "moto", "the online one seen last");
    }
}
