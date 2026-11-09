//! Home: greeting, hero carousel, continue watching and feed rows (prototype `screens.js::home`).
//! Each row degrades on its own: a failed row shows an inline `ErrorState` whose Retry reloads only that
//! row. Extension rows (`SectionVm::title_key` = `ext:<extension name>`, the `customHome` row) and rows
//! below the first few mount their content only when scrolled into view.
//!
//! Wave B (worker **b**, docs/redesign/16 section 4; checklist `home.*`): behind the `Settings::home` toggles, the
//! torrent activity banner (`data-torrent-active`), new episodes of the Watching list (`data-row="followed"`),
//! latest episodes (`data-row="latest"`, `data-episode`), the Top 10 with Today / Week / Month (`data-row="top10"`,
//! `data-range`, `data-rank`), "Upcoming" / "Most favourited" (rows of `Backend::home`, `data-row`), "Because you
//! watched" (`data-row="because"`), the airing block (today + tomorrow), the recommendations teaser
//! (`data-row="recs"`), the genre cloud (`data-genre-cloud`, chip -> `#/browse/genre:<name>`) and one more genre row
//! each time the end of the page is reached (`data-row="genre:<name>"`). While Home is open, `reminders_due` is
//! polled every minute (`home.calendar.reminders`): a snackbar, a system notification when the web view allows it,
//! and a card per reminder (`data-reminder`).

use std::time::Duration;

use dioxus::prelude::*;

use crate::app::{Route, Services, Ui};
use crate::backend::{AiringSlotVm, HomeVm, LatestEpisodeVm, MediaVm, SectionVm, TopRangeVm};
use crate::components::airing::{hhmm, use_local_clock};
use crate::components::{
    AiringCalendar, Button, ButtonSize, ButtonVariant, Chip, ChipKind, ContinueCard, EmptyState, ErrorState,
    HeroCarousel, IconButton, IconButtonSize, InfoChip, MediaCard, Segmented, SkeletonRow, use_snackbar,
};
use crate::i18n::{t, tf, tn};
use crate::model::{ErrorKind, UiError};
use crate::player_state::format_bytes;
use crate::screens::recs::RecsRow;
use crate::screens::search::genre_label;

/// `title_key` prefix of an extension row; the rest is the extension's display name.
pub const EXT_ROW_PREFIX: &str = "ext:";
/// Hero slides taken from the first section.
const HERO_ITEMS: usize = 5;
/// Rows rendered right away; the rest wait until visible.
const EAGER_ROWS: usize = 3;
/// A lazy row mounts anyway after this long (in case the visibility event never arrives).
const LAZY_FALLBACK_MS: u64 = 6000;
/// The local hour for the greeting (the Rust side only knows UTC).
const HOUR_JS: &str = "dioxus.send(new Date().getHours());";
/// `reminders_due` poll period while Home is open.
const REMINDER_POLL_SECS: u64 = 60;
/// Torrent banner refresh period.
const TORRENT_POLL_SECS: u64 = 5;
/// Cards of the "latest" / "followed" rows.
const EPISODE_ROW_ITEMS: usize = 24;

/// `data-row` value and "See all" feed (`Route::BrowsePage`) of a `Backend::home` section.
pub fn row_meta(title_key: &str) -> (String, Option<String>) {
    match title_key {
        "home.row.upcoming" => ("upcoming".to_owned(), Some("upcoming".to_owned())),
        "home.row.most_favorite" => ("most-favorite".to_owned(), None),
        k if k.starts_with(EXT_ROW_PREFIX) => (k.to_owned(), None),
        k => {
            let feed = k.strip_prefix("feed.").unwrap_or(k).to_owned();
            (feed.clone(), Some(feed))
        }
    }
}

/// `Route::BrowsePage` feed of a genre (served by worker a's `feed()` as `FeedId::Genre`).
pub fn genre_feed_id(genre: &str) -> String {
    format!("genre:{genre}")
}

/// Text of a reminder: "airs at HH:MM" while it is ahead, "is out" once aired.
fn reminder_text(slot: &AiringSlotVm, now: i64, offset_min: i64) -> String {
    let n = slot.episode.to_string();
    if slot.at_unix > now {
        tf(
            "home.reminder.soon",
            &[
                ("title", &slot.media.title),
                ("n", &n),
                ("time", &hhmm(slot.at_unix, offset_min)),
            ],
        )
    } else {
        tf("home.reminder.out", &[("title", &slot.media.title), ("n", &n)])
    }
}

/// A system notification through the web view (`Notification` API), when the user allows it. Best effort.
fn notify_js(title: &str, body: &str) -> String {
    let (title, body) = (
        serde_json::to_string(title).unwrap_or_default(),
        serde_json::to_string(body).unwrap_or_default(),
    );
    format!(
        "try {{ if (window.Notification) {{ const go = () => new Notification({title}, {{ body: {body} }}); \
         if (Notification.permission === 'granted') go(); \
         else if (Notification.permission !== 'denied') Notification.requestPermission().then(p => {{ if (p === 'granted') go(); }}); }} }} catch (e) {{}}"
    )
}

#[component]
pub fn HomePage() -> Element {
    let services = use_context::<Services>();
    let backend = services.backend.clone();
    let mut home = use_resource(move || {
        let backend = backend.clone();
        async move { backend.home().await }
    });

    let body = match &*home.read() {
        None => rsx! {
            HomeSkeleton {}
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), on_retry: move |_| home.restart() }
        },
        Some(Ok(vm)) => rsx! {
            HomeContent { vm: vm.clone(), on_retry: move |_| home.restart() }
        },
    };

    rsx! {
        div { class: "tk-page tk-page--home",
            HomeHead {}
            ReminderPoller {}
            {body}
        }
    }
}

/// Greeting by local time of day + the active metadata sources.
#[component]
fn HomeHead() -> Element {
    let ui = use_context::<Ui>();
    let mut hour = use_signal(|| None::<u32>);
    use_future(move || async move {
        let mut eval = document::eval(HOUR_JS);
        if let Ok(h) = eval.recv::<u32>().await {
            hour.set(Some(h));
        }
    });
    let title = match hour() {
        Some(h) if h < 12 => t("home.greeting.morning"),
        Some(h) if h < 19 => t("home.greeting.afternoon"),
        Some(_) => t("home.greeting.evening"),
        None => t("nav.home"),
    };
    // The first (preferred) source gets a check mark.
    let providers: Vec<(usize, String, String)> = ui
        .settings
        .read()
        .provider_priority
        .iter()
        .enumerate()
        .map(|(i, p)| {
            (
                i,
                t(&format!("provider.{p}")),
                if i == 0 {
                    "check".to_string()
                } else {
                    String::new()
                },
            )
        })
        .collect();
    rsx! {
        div { class: "tk-page__head tk-home__greet",
            div {
                h1 { class: "tk-page__title", "{title}" }
                p { class: "tk-page__intro", {t("search.hint")} }
            }
            div { class: "tk-home__sources", "aria-label": t("home.sources"),
                for (i , name , icon) in providers {
                    InfoChip { key: "{i}", text: name, icon }
                }
            }
        }
    }
}

#[component]
fn HomeSkeleton() -> Element {
    rsx! {
        section { class: "tk-section", "aria-busy": "true",
            div { class: "tk-skel tk-skel--hero", "aria-hidden": "true" }
        }
        section { class: "tk-section", "aria-busy": "true",
            div { class: "tk-skel tk-skel--line tk-home__skel-title", "aria-hidden": "true" }
            SkeletonRow { count: 8 }
        }
        section { class: "tk-section", "aria-busy": "true",
            div { class: "tk-skel tk-skel--line tk-home__skel-title", "aria-hidden": "true" }
            SkeletonRow { count: 8 }
        }
    }
}

#[component]
fn HomeContent(vm: HomeVm, on_retry: EventHandler<()>) -> Element {
    let navigator = use_navigator();
    // Before any early return: hooks keep their order.
    let ui = use_context::<Ui>();
    let hero: Vec<MediaVm> = vm
        .sections
        .first()
        .and_then(|s| s.items.as_ref().ok())
        .map(|items| items.iter().take(HERO_ITEMS).cloned().collect())
        .unwrap_or_default();
    let hero_key = hero.iter().map(|m| m.id.as_str()).collect::<Vec<_>>().join(",");
    // The first section feeds the hero (as in the prototype); it is still listed if the hero is empty.
    let rows: Vec<SectionVm> = vm
        .sections
        .iter()
        .skip(usize::from(!hero.is_empty()))
        .cloned()
        .collect();
    // A failed row still counts: it shows its own error.
    let has_rows = rows
        .iter()
        .any(|s| !matches!(&s.items, Ok(items) if items.is_empty()));

    if hero.is_empty() && vm.continue_watching.is_empty() && !has_rows {
        return rsx! {
            EmptyState {
                icon: "film".to_string(),
                title: t("home.empty.title"),
                body: t("home.empty.body"),
                action_label: t("common.retry"),
                on_action: move |_| on_retry.call(()),
            }
        };
    }

    let p = ui.settings.read().home.clone();

    rsx! {
        if !hero.is_empty() {
            section { class: "tk-section",
                HeroCarousel {
                    key: "{hero_key}",
                    items: hero.clone(),
                    kicker: t("feed.trending"),
                    on_open: move |id: String| {
                        navigator.push(Route::DetailsPage { id });
                    },
                }
            }
        }
        if !vm.continue_watching.is_empty() {
            section { class: "tk-section",
                div { class: "tk-section__head",
                    h2 { class: "tk-section__title", {t("home.continue")} }
                    Button {
                        label: t("common.see_all"),
                        variant: ButtonVariant::Text,
                        size: ButtonSize::Sm,
                        onclick: move |_| {
                            navigator.push(Route::LibraryPage {});
                        },
                    }
                }
                div { class: "tk-row",
                    for c in vm.continue_watching.clone() {
                        {
                            let (id, episode) = (c.media.id.clone(), c.episode);
                            rsx! {
                                ContinueCard {
                                    key: "{c.media.id}",
                                    item: c.clone(),
                                    onclick: move |_| {
                                        navigator.push(Route::PlayerPage { id: id.clone(), episode });
                                    },
                                }
                            }
                        }
                    }
                }
            }
        }
        if p.show_torrent_activity {
            TorrentBanner {}
        }
        if p.show_latest_episodes {
            FollowedRow {}
            LatestRow {}
        }
        if p.show_top10 {
            Top10Block {}
        }
        for (i , s) in rows.into_iter().enumerate() {
            {
                let lazy = i >= EAGER_ROWS || s.title_key.starts_with(EXT_ROW_PREFIX);
                rsx! {
                    SectionRow { key: "{s.title_key}", section: s.clone(), lazy }
                }
            }
        }
        if p.show_because_you_watched {
            BecauseRows {}
        }
        if p.show_calendar {
            CalendarBlock {}
        }
        if p.show_recommendations {
            RecsRow {}
        }
        if p.show_genre_cloud {
            GenreCloud {}
        }
        if p.infinite_genre_rows {
            InfiniteGenreRows {}
        }
    }
}

/// One feed row. Retry re-runs `Backend::home()` and keeps only this row's result, so the other rows
/// never flash. `lazy` rows show a skeleton until they scroll into view.
#[component]
fn SectionRow(section: SectionVm, #[props(default)] lazy: bool) -> Element {
    let services = use_context::<Services>();
    let navigator = use_navigator();
    let mut seen = use_signal(|| !lazy);
    let retried = use_signal(|| None::<Result<Vec<MediaVm>, UiError>>);
    let retrying = use_signal(|| false);

    use_future(move || async move {
        if *seen.peek() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(LAZY_FALLBACK_MS)).await;
        if !*seen.peek() {
            seen.set(true);
        }
    });

    let title_key = section.title_key.clone();
    let retry = move |_: ()| {
        let backend = services.backend.clone();
        let key = title_key.clone();
        let (mut retried, mut retrying) = (retried, retrying);
        retrying.set(true);
        spawn(async move {
            let items = match backend.home().await {
                Ok(vm) => vm
                    .sections
                    .into_iter()
                    .find(|s| s.title_key == key)
                    .map(|s| s.items)
                    .unwrap_or_else(|| Ok(Vec::new())),
                Err(e) => Err(e),
            };
            retried.set(Some(items));
            retrying.set(false);
        });
    };

    let ext_name = section.title_key.strip_prefix(EXT_ROW_PREFIX).map(str::to_owned);
    let title = match &ext_name {
        Some(name) => tf("home.ext_row", &[("name", name)]),
        None => t(&section.title_key),
    };
    let is_ext = ext_name.is_some();
    let (row_id, feed) = row_meta(&section.title_key);
    let dated = section.title_key == "home.row.upcoming";
    let items = retried().unwrap_or_else(|| section.items.clone());
    let busy = !seen() || retrying();

    let body = if busy {
        rsx! {
            SkeletonRow { count: 6 }
        }
    } else {
        match items {
            Ok(list) if list.is_empty() => rsx! {
                p { class: "tk-section__empty tk-muted", {t("home.empty.title")} }
            },
            Ok(list) => rsx! {
                div { class: "tk-row",
                    for m in list {
                        {
                            let id = m.id.clone();
                            // Upcoming rows carry the expected start date in `subtitle`.
                            let sub = if dated && !m.subtitle.is_empty() {
                                tf("home.row.upcoming.date", &[("date", &m.subtitle)])
                            } else {
                                String::new()
                            };
                            rsx! {
                                MediaCard {
                                    key: "{m.id}",
                                    media: m.clone(),
                                    sub,
                                    onclick: move |_| {
                                        navigator.push(Route::DetailsPage { id: id.clone() });
                                    },
                                }
                            }
                        }
                    }
                }
            },
            Err(e) => {
                let cause = if e.kind == ErrorKind::RateLimited {
                    t("home.row_error")
                } else {
                    String::new()
                };
                rsx! {
                    ErrorState { error: e, compact: true, cause, on_retry: retry }
                }
            }
        }
    };

    rsx! {
        section {
            class: "tk-section",
            "aria-busy": "{busy}",
            "data-row": "{row_id}",
            onvisible: move |e| {
                if !*seen.peek() && e.is_intersecting().unwrap_or(false) {
                    seen.set(true);
                }
            },
            div { class: "tk-section__head",
                h2 { class: "tk-section__title",
                    "{title}"
                    if is_ext {
                        small { class: "tk-section__count tk-mono", ".kai" }
                    }
                }
                if let Some(feed) = feed.filter(|_| !is_ext) {
                    Button {
                        label: t("common.see_all"),
                        variant: ButtonVariant::Text,
                        size: ButtonSize::Sm,
                        onclick: move |_| {
                            navigator.push(Route::BrowsePage { feed: feed.clone() });
                        },
                    }
                }
            }
            {body}
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Wave B blocks (worker b)
// ---------------------------------------------------------------------------------------------

/// Polls `reminders_due` once a minute while Home is open and shows each new reminder once.
#[component]
fn ReminderPoller() -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let snack = use_snackbar();
    let navigator = use_navigator();
    let clock = use_local_clock();
    let due = use_signal(Vec::<AiringSlotVm>::new);
    use_future({
        let backend = services.backend.clone();
        move || {
            let backend = backend.clone();
            let mut due = due;
            async move {
                loop {
                    if ui.settings.peek().home.airing_reminders {
                        match backend.reminders_due().await {
                            Ok(list) if !list.is_empty() => {
                                let (now, offset) = clock.peek().map_or((0, 0), |c| (c.now, c.offset_min));
                                for s in &list {
                                    let text = reminder_text(s, now, offset);
                                    let _ = document::eval(&notify_js(&t("home.reminder.title"), &text));
                                }
                                snack.show(reminder_text(&list[0], now, offset));
                                due.write().extend(list);
                            }
                            Ok(_) => {}
                            Err(e) => tracing::debug!(detail = %e.detail, "reminders_due failed"),
                        }
                    }
                    tokio::time::sleep(Duration::from_secs(REMINDER_POLL_SECS)).await;
                }
            }
        }
    });
    let list = due();
    if list.is_empty() {
        return rsx! {};
    }
    let (now, offset) = clock().map_or((0, 0), |c| (c.now, c.offset_min));
    rsx! {
        section { class: "tk-home__reminders", role: "status", "aria-label": t("home.reminder.title"),
            for (i , s) in list.into_iter().enumerate() {
                {
                    let text = reminder_text(&s, now, offset);
                    let (id, episode) = (s.media.id.clone(), s.episode);
                    let mut due = due;
                    rsx! {
                        div { key: "{s.media.id}-{s.episode}", class: "tk-home__reminder", "data-reminder": "{s.media.id}:{s.episode}",
                            span { class: "tk-home__reminder-text", "{text}" }
                            Button {
                                label: t("home.reminder.open"),
                                variant: ButtonVariant::Text,
                                size: ButtonSize::Sm,
                                onclick: move |_| {
                                    navigator.push(Route::PlayerPage { id: id.clone(), episode: episode.max(1) });
                                },
                            }
                            IconButton {
                                icon: "close".to_string(),
                                label: t("home.reminder.dismiss"),
                                size: IconButtonSize::Sm,
                                onclick: move |_| {
                                    if i < due.peek().len() {
                                        due.write().remove(i);
                                    }
                                },
                            }
                        }
                    }
                }
            }
        }
    }
}

/// "N torrents downloading" with the speeds, refreshed every few seconds; nothing when idle.
#[component]
fn TorrentBanner() -> Element {
    let services = use_context::<Services>();
    let navigator = use_navigator();
    let mut activity = use_resource({
        let backend = services.backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.torrent_activity().await }
        }
    });
    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_secs(TORRENT_POLL_SECS)).await;
            activity.restart();
        }
    });
    match &*activity.read() {
        Some(Ok(a)) if a.active > 0 => {
            let text = tn("home.torrent.active", a.active as usize);
            let speed = tf(
                "home.torrent.speed",
                &[
                    ("down", &format_bytes(a.down_bps)),
                    ("up", &format_bytes(a.up_bps)),
                ],
            );
            rsx! {
                section { class: "tk-home__torrents", "data-torrent-active": "{a.active}",
                    span { class: "tk-home__torrents-text", "{text}" }
                    span { class: "tk-home__torrents-speed tk-mono", "{speed}" }
                    Button {
                        label: t("home.torrent.open"),
                        variant: ButtonVariant::Tonal,
                        size: ButtonSize::Sm,
                        icon: "magnet".to_string(),
                        onclick: move |_| {
                            navigator.push(Route::TorrentOverviewPage {});
                        },
                    }
                }
            }
        }
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), compact: true, on_retry: move |_| activity.restart() }
        },
        _ => rsx! {},
    }
}

/// A row of aired episodes (latest / followed): cards with the episode number (`data-episode`).
#[component]
fn EpisodeCards(items: Vec<LatestEpisodeVm>, play: bool) -> Element {
    let navigator = use_navigator();
    rsx! {
        div { class: "tk-row",
            for e in items.into_iter().take(EPISODE_ROW_ITEMS) {
                {
                    let (id, episode) = (e.media.id.clone(), e.episode);
                    let ep = tf("home.row.latest.ep", &[("n", &episode.to_string())]);
                    rsx! {
                        div { key: "{e.media.id}-{e.episode}", class: "tk-home__ep", "data-episode": "{e.episode}",
                            MediaCard {
                                media: e.media.clone(),
                                badge: tf("calendar.ep", &[("n", &episode.to_string())]),
                                sub: ep,
                                onclick: move |_| {
                                    if play {
                                        navigator.push(Route::PlayerPage { id: id.clone(), episode });
                                    } else {
                                        navigator.push(Route::DetailsPage { id: id.clone() });
                                    }
                                },
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn LatestRow() -> Element {
    let services = use_context::<Services>();
    let mut latest = use_resource({
        let backend = services.backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.latest_episodes(1).await }
        }
    });
    let body = match &*latest.read() {
        None => rsx! {
            SkeletonRow { count: 6 }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), compact: true, on_retry: move |_| latest.restart() }
        },
        Some(Ok(list)) if list.is_empty() => rsx! {
            p { class: "tk-section__empty tk-muted", {t("home.row.empty")} }
        },
        Some(Ok(list)) => rsx! {
            EpisodeCards { items: list.clone(), play: false }
        },
    };
    rsx! {
        section { class: "tk-section", "data-row": "latest",
            div { class: "tk-section__head",
                h2 { class: "tk-section__title", {t("home.row.latest")} }
            }
            {body}
        }
    }
}

/// New episodes of the Watching list since the previous visit; hidden when there are none.
#[component]
fn FollowedRow() -> Element {
    let services = use_context::<Services>();
    let mut followed = use_resource({
        let backend = services.backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.followed_updates().await }
        }
    });
    let body = match &*followed.read() {
        None => return rsx! {},
        Some(Ok(list)) if list.is_empty() => return rsx! {},
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), compact: true, on_retry: move |_| followed.restart() }
        },
        Some(Ok(list)) => rsx! {
            EpisodeCards { items: list.clone(), play: true }
        },
    };
    rsx! {
        section { class: "tk-section", "data-row": "followed",
            div { class: "tk-section__head",
                h2 { class: "tk-section__title",
                    {t("home.row.followed")}
                    small { class: "tk-section__count", {t("home.row.followed.hint")} }
                }
            }
            {body}
        }
    }
}

#[component]
fn Top10Block() -> Element {
    let services = use_context::<Services>();
    let navigator = use_navigator();
    let mut range = use_signal(TopRangeVm::default);
    let mut top = use_resource({
        let backend = services.backend.clone();
        move || {
            let backend = backend.clone();
            let r = range();
            async move { backend.top10(r).await }
        }
    });
    let current = range();
    let options: Vec<(String, String)> = TopRangeVm::ALL
        .iter()
        .map(|r| (r.id().to_string(), t(&format!("home.top.{}", r.id()))))
        .collect();
    let body = match &*top.read() {
        None => rsx! {
            SkeletonRow { count: 5 }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), compact: true, on_retry: move |_| top.restart() }
        },
        Some(Ok(list)) if list.is_empty() => rsx! {
            p { class: "tk-section__empty tk-muted", {t("home.row.empty")} }
        },
        Some(Ok(list)) => rsx! {
            ol { class: "tk-top10", "data-range": "{current.id()}",
                for (i , m) in list.iter().take(10).enumerate() {
                    {
                        let id = m.id.clone();
                        let rank = i + 1;
                        let label = tf("home.top.rank", &[("n", &rank.to_string()), ("title", &m.title)]);
                        rsx! {
                            li { key: "{m.id}", class: "tk-top10__item", "data-rank": "{rank}", "aria-label": "{label}",
                                span { class: "tk-top10__rank", "aria-hidden": "true", "{rank}" }
                                MediaCard {
                                    media: m.clone(),
                                    onclick: move |_| {
                                        navigator.push(Route::DetailsPage { id: id.clone() });
                                    },
                                }
                            }
                        }
                    }
                }
            }
        },
    };
    rsx! {
        section { class: "tk-section", "data-row": "top10",
            div { class: "tk-section__head",
                h2 { class: "tk-section__title", {t("home.top10")} }
                Segmented {
                    options,
                    selected: current.id().to_string(),
                    label: t("home.top10.range"),
                    small: true,
                    on_select: move |id: String| {
                        if let Some(r) = TopRangeVm::ALL.into_iter().find(|r| r.id() == id) {
                            range.set(r);
                        }
                    },
                }
            }
            {body}
        }
    }
}

/// "Because you watched <title>" rows (up to 3 seeds from the history); nothing without history.
#[component]
fn BecauseRows() -> Element {
    let services = use_context::<Services>();
    let navigator = use_navigator();
    let mut rows = use_resource({
        let backend = services.backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.because_you_watched().await }
        }
    });
    match &*rows.read() {
        None => rsx! {
            section { class: "tk-section", "aria-busy": "true", SkeletonRow { count: 6 } }
        },
        Some(Err(e)) => rsx! {
            section { class: "tk-section", "data-row": "because",
                ErrorState { error: e.clone(), compact: true, on_retry: move |_| rows.restart() }
            }
        },
        Some(Ok(list)) => rsx! {
            for b in list.clone() {
                section { key: "{b.seed.id}", class: "tk-section", "data-row": "because", "data-seed": "{b.seed.id}",
                    div { class: "tk-section__head",
                        h2 { class: "tk-section__title", {tf("home.row.because", &[("title", &b.seed.title)])} }
                    }
                    div { class: "tk-row",
                        for m in b.items.clone() {
                            {
                                let id = m.id.clone();
                                rsx! {
                                    MediaCard {
                                        key: "{m.id}",
                                        media: m.clone(),
                                        onclick: move |_| {
                                            navigator.push(Route::DetailsPage { id: id.clone() });
                                        },
                                    }
                                }
                            }
                        }
                    }
                }
            }
        },
    }
}

/// Today + tomorrow of the airing calendar, with a link to the full week.
#[component]
fn CalendarBlock() -> Element {
    let navigator = use_navigator();
    let clock = use_local_clock();
    rsx! {
        section { class: "tk-section", "data-row": "calendar",
            div { class: "tk-section__head",
                h2 { class: "tk-section__title", {t("home.row.calendar")} }
                Button {
                    label: t("home.calendar.open"),
                    variant: ButtonVariant::Text,
                    size: ButtonSize::Sm,
                    onclick: move |_| {
                        navigator.push(Route::CalendarPage {});
                    },
                }
            }
            if let Some(c) = clock() {
                AiringCalendar { week_start_unix: c.today(), compact: true }
            } else {
                SkeletonRow { count: 4 }
            }
        }
    }
}

/// Genre chips; a chip opens Browse with that genre.
#[component]
fn GenreCloud() -> Element {
    let services = use_context::<Services>();
    let navigator = use_navigator();
    let mut genres = use_resource({
        let backend = services.backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.genres().await }
        }
    });
    let body = match &*genres.read() {
        None => rsx! {
            div { class: "tk-skel tk-skel--line tk-home__skel-title", "aria-hidden": "true" }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), compact: true, on_retry: move |_| genres.restart() }
        },
        Some(Ok(list)) if list.is_empty() => rsx! {
            p { class: "tk-section__empty tk-muted", {t("home.row.empty")} }
        },
        Some(Ok(list)) => rsx! {
            div { class: "tk-home__genres", "data-genre-cloud": "{list.len()}",
                for g in list.clone() {
                    {
                        let feed = genre_feed_id(&g);
                        rsx! {
                            Chip {
                                key: "{g}",
                                label: genre_label(&g),
                                kind: ChipKind::Assist,
                                onclick: move |_| {
                                    navigator.push(Route::BrowsePage { feed: feed.clone() });
                                },
                            }
                        }
                    }
                }
            }
        },
    };
    rsx! {
        section { class: "tk-section", "data-row": "genres",
            div { class: "tk-section__head",
                h2 { class: "tk-section__title", {t("home.genres")} }
            }
            {body}
        }
    }
}

/// One more genre row each time the end of Home becomes visible, until the genre list runs out.
#[component]
fn InfiniteGenreRows() -> Element {
    let services = use_context::<Services>();
    let genres = use_resource({
        let backend = services.backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.genres().await }
        }
    });
    let mut shown = use_signal(|| 0_usize);
    let list: Vec<String> = match &*genres.read() {
        Some(Ok(l)) => l.clone(),
        _ => Vec::new(),
    };
    let total = list.len();
    let count = shown().min(total);
    rsx! {
        for g in list.into_iter().take(count) {
            GenreRow { key: "{g}", genre: g.clone() }
        }
        if count < total {
            div {
                class: "tk-home__more",
                "aria-label": t("home.genres.loading_more"),
                onvisible: move |e| {
                    if e.is_intersecting().unwrap_or(false) && *shown.peek() < total {
                        shown += 1;
                    }
                },
            }
        }
    }
}

#[component]
fn GenreRow(genre: String) -> Element {
    let services = use_context::<Services>();
    let navigator = use_navigator();
    let mut page = use_resource(use_reactive((&genre,), {
        let backend = services.backend.clone();
        move |(g,)| {
            let backend = backend.clone();
            async move { backend.genre_feed(&g, 1).await }
        }
    }));
    let feed = genre_feed_id(&genre);
    let body = match &*page.read() {
        None => rsx! {
            SkeletonRow { count: 6 }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), compact: true, on_retry: move |_| page.restart() }
        },
        Some(Ok(p)) if p.items.is_empty() => rsx! {
            p { class: "tk-section__empty tk-muted", {t("home.row.empty")} }
        },
        Some(Ok(p)) => rsx! {
            div { class: "tk-row",
                for m in p.items.clone() {
                    {
                        let id = m.id.clone();
                        rsx! {
                            MediaCard {
                                key: "{m.id}",
                                media: m.clone(),
                                onclick: move |_| {
                                    navigator.push(Route::DetailsPage { id: id.clone() });
                                },
                            }
                        }
                    }
                }
            }
        },
    };
    rsx! {
        section { class: "tk-section", "data-row": "genre:{genre}",
            div { class: "tk-section__head",
                h2 { class: "tk-section__title", {genre_label(&genre)} }
                Button {
                    label: t("common.see_all"),
                    variant: ButtonVariant::Text,
                    size: ButtonSize::Sm,
                    onclick: move |_| {
                        navigator.push(Route::BrowsePage { feed: feed.clone() });
                    },
                }
            }
            {body}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_map_to_data_row_and_feed() {
        assert_eq!(
            row_meta("feed.trending"),
            ("trending".to_owned(), Some("trending".to_owned()))
        );
        assert_eq!(
            row_meta("home.row.upcoming"),
            ("upcoming".to_owned(), Some("upcoming".to_owned()))
        );
        assert_eq!(
            row_meta("home.row.most_favorite"),
            ("most-favorite".to_owned(), None)
        );
        assert_eq!(row_meta("ext:Toko").1, None);
        assert_eq!(genre_feed_id("Slice of Life"), "genre:Slice of Life");
    }

    #[test]
    fn notification_script_escapes_text() {
        let js = notify_js("A \"quoted\" title", "line\nbreak");
        assert!(js.contains(r#""A \"quoted\" title""#));
        assert!(js.contains(r#""line\nbreak""#));
    }
}
