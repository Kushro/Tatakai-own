//! Details > Episodes (2026-11 UI refresh): three views following `Settings::look.episode_view`.
//! - Cards: two columns of large cards (16:9 thumbnail with runtime and rating, `N. Title`, synopsis, relative date,
//!   progress bar, "Continue" mark on the resume episode, watched toggle on hover / focus).
//! - List: one row per episode with a small thumbnail, relative date and runtime on the right.
//! - Compact: a dense grid of `N. Title · date` tiles.
//!
//! The in-page switch saves the choice to `Settings::look` (it replaced the `tk.details.episodeView` localStorage
//! key). Ranges of 100, the search and the pills (filler, watched, unaired) work the same in every view.

use dioxus::prelude::*;

use super::extra::countdown_parts;
use super::{WATCHED_AT, days_from_civil, episode_pills, filter_episodes, is_unaired, parse_air_unix};
use crate::app::{Route, Services, Ui};
use crate::backend::{AiringVm, EpisodeVm};
use crate::components::{EmptyState, IconButton, IconButtonSize, IconButtonVariant, Pill, Segmented, TextField, Tone};
use crate::i18n::{t, tf, tn};
use crate::icons::Icon;
use crate::model::{EPISODE_RANGE, EpisodeView, SettingsAction, episode_ranges, range_index_for, reduce_settings};

const SECS_PER_DAY: i64 = 86_400;

// ---- pure helpers -------------------------------------------------------------------------------------------

/// Unit of a relative date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelUnit {
    Days,
    Weeks,
    Months,
    Years,
}

/// A calendar day relative to today: "today", "yesterday", "in 6 days", "1 month ago"...
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelDay {
    Today,
    Yesterday,
    Tomorrow,
    Ago(RelUnit, u32),
    In(RelUnit, u32),
}

/// Local day number (days since 1970-01-01) of an air date. A bare date (`2024-10-05`) is that calendar day as is;
/// a date with a time is shifted to the viewer's zone (`offset_secs` east of UTC) first.
pub fn air_day(text: &str, offset_secs: i64) -> Option<i64> {
    let text = text.trim();
    let at = parse_air_unix(text)?;
    if text.len() <= 10 {
        let (y, m, d) = (text.get(..4)?.parse().ok()?, text.get(5..7)?.parse().ok()?, text.get(8..10)?.parse().ok()?);
        return Some(days_from_civil(y, m, d));
    }
    Some((at + offset_secs).div_euclid(SECS_PER_DAY))
}

/// Local day number of a unix time (`offset_secs` east of UTC).
pub fn local_day(unix: i64, offset_secs: i64) -> i64 {
    (unix + offset_secs).div_euclid(SECS_PER_DAY)
}

/// `day` relative to `today` (both local day numbers): 0 today, ±1 yesterday / tomorrow, then days up to 6, weeks up
/// to 29 days, months (30 days) up to 364 days, then years. Pure.
pub fn relative_day(day: i64, today: i64) -> RelDay {
    let diff = today - day;
    let n = diff.unsigned_abs();
    let (unit, count) = match n {
        0 => return RelDay::Today,
        1 => return if diff > 0 { RelDay::Yesterday } else { RelDay::Tomorrow },
        2..=6 => (RelUnit::Days, n),
        7..=29 => (RelUnit::Weeks, n / 7),
        30..=364 => (RelUnit::Months, n / 30),
        _ => (RelUnit::Years, n / 365),
    };
    let count = u32::try_from(count).unwrap_or(u32::MAX);
    if diff > 0 { RelDay::Ago(unit, count) } else { RelDay::In(unit, count) }
}

/// [`RelDay`] in the UI language ("hace 1 mes", "en 6 días", "ayer").
pub fn relative_day_label(r: RelDay) -> String {
    let unit = |u: RelUnit| match u {
        RelUnit::Days => "days",
        RelUnit::Weeks => "weeks",
        RelUnit::Months => "months",
        RelUnit::Years => "years",
    };
    match r {
        RelDay::Today => t("episodes.rel.today"),
        RelDay::Yesterday => t("episodes.rel.yesterday"),
        RelDay::Tomorrow => t("episodes.rel.tomorrow"),
        RelDay::Ago(u, n) => tn(&format!("episodes.rel.{}_ago", unit(u)), n as usize),
        RelDay::In(u, n) => tn(&format!("episodes.rel.in_{}", unit(u)), n as usize),
    }
}

/// "in 2 d 4 h" / "in 3 h 12 min" / "in 7 min" until `at_unix` (the next airing episode's countdown).
fn countdown_label(at_unix: i64, now_unix: i64) -> String {
    let (d, h, m) = countdown_parts(at_unix - now_unix);
    let when = if d > 0 {
        tf("details.dur.dh", &[("d", &d.to_string()), ("h", &h.to_string())])
    } else if h > 0 {
        tf("details.dur.hm", &[("h", &h.to_string()), ("m", &m.to_string())])
    } else {
        tf("details.dur.m", &[("m", &m.max(1).to_string())])
    };
    tf("episodes.in", &[("when", &when)])
}

/// The date line of an episode: the countdown for the next airing one (when it is still ahead), else the air date
/// relative to today. `None` without a usable date.
pub fn episode_when(e: &EpisodeVm, next: Option<AiringVm>, now_unix: i64, offset_secs: i64) -> Option<String> {
    if let Some(n) = next.filter(|n| n.episode == e.number && n.at_unix > now_unix) {
        return Some(countdown_label(n.at_unix, now_unix));
    }
    let day = air_day(e.air_date.as_deref()?, offset_secs)?;
    Some(relative_day_label(relative_day(day, local_day(now_unix, offset_secs))))
}

/// `★ 9.64` text of a 0..10 rating (two decimals, as the providers give it).
pub fn rating_text(r: f32) -> String {
    format!("{:.2}", r)
}

/// Episode views in switch order with their icons.
const VIEWS: [(EpisodeView, &str); 3] =
    [(EpisodeView::Cards, "grid"), (EpisodeView::List, "list"), (EpisodeView::Compact, "menu")];

// ---- components ---------------------------------------------------------------------------------------------

/// A connected button group like `Segmented` with an icon before each label (the views of Episodes / Relations).
#[component]
pub fn IconSegmented(
    /// `(id, label, icon)`.
    options: Vec<(String, String, String)>,
    selected: String,
    label: String,
    #[props(default)] on_select: EventHandler<String>,
) -> Element {
    let ids: Vec<String> = options.iter().map(|o| o.0.clone()).collect();
    let current = ids.iter().position(|id| *id == selected).unwrap_or(0);
    rsx! {
        div {
            class: "tk-seg tk-seg--sm tk-dviews",
            role: "radiogroup",
            "aria-label": "{label}",
            onkeydown: move |e| {
                let len = ids.len();
                if len == 0 {
                    return;
                }
                let target = match e.key() {
                    Key::ArrowRight | Key::ArrowDown => Some((current + 1) % len),
                    Key::ArrowLeft | Key::ArrowUp => Some((current + len - 1) % len),
                    Key::Home => Some(0),
                    Key::End => Some(len - 1),
                    _ => None,
                };
                if let Some(i) = target {
                    e.prevent_default();
                    on_select.call(ids[i].clone());
                }
            },
            for (i , (id , text , icon)) in options.into_iter().enumerate() {
                button {
                    key: "{id}",
                    class: "tk-seg__item",
                    r#type: "button",
                    role: "radio",
                    "data-view-option": "{id}",
                    "aria-checked": if id == selected { "true" } else { "false" },
                    tabindex: if i == current { "0" } else { "-1" },
                    onclick: {
                        let id = id.clone();
                        move |_| on_select.call(id.clone())
                    },
                    Icon { name: icon, size: 18 }
                    span { "{text}" }
                }
            }
        }
    }
}

#[component]
pub fn EpisodeList(
    id: String,
    episodes: Vec<EpisodeVm>,
    resume: u32,
    #[props(default)] next_airing: Option<AiringVm>,
    #[props(default)] now: i64,
    #[props(default = true)] images: bool,
    /// Manual watched / not-watched toggle of an episode (`(number, watched)`); `None` hides the buttons.
    #[props(default)] on_toggle: Option<EventHandler<(u32, bool)>>,
    /// Series artwork for episodes without their own thumbnail (banner, else cover).
    #[props(default)] fallback_image: Option<String>,
) -> Element {
    let navigator = use_navigator();
    let ui = use_context::<Ui>();
    let services = use_context::<Services>();
    let mut query = use_signal(String::new);
    // The viewer's zone (seconds east of UTC), so "today" / "yesterday" follow the local calendar.
    let mut offset = use_signal(|| 0_i64);
    use_future(move || async move {
        let js = "try{return -new Date().getTimezoneOffset()*60}catch(e){return 0}";
        if let Ok(v) = document::eval(js).join::<i64>().await {
            offset.set(v);
        }
    });
    let active_view = ui.settings.read().look.episode_view;

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
    let view_options: Vec<(String, String, String)> =
        VIEWS.iter().map(|(v, icon)| (v.id().to_owned(), t(&v.label_key()), (*icon).to_owned())).collect();
    let shown_count = shown.len();
    let offset_now = offset();

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
            IconSegmented {
                options: view_options,
                selected: active_view.id().to_string(),
                label: t("details.view"),
                on_select: move |v: String| {
                    let Some(x) = EpisodeView::from_id(&v) else { return };
                    let cur = ui.settings.peek().clone();
                    if cur.look.episode_view == x {
                        return;
                    }
                    let mut look = cur.look.clone();
                    look.episode_view = x;
                    ui.commit_settings(&services.backend, reduce_settings(&cur, SettingsAction::SetLook(look)));
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
                    // BUG-073: an unaired episode with a date already says when in its date line.
                    if unaired && e.air_date.is_none() {
                        pills.push((t("details.unaired"), Tone::Primary));
                    }
                    let with_check = on_toggle.is_some() && !unaired;
                    pills.extend(episode_pills(&e, resume, with_check).into_iter().map(|(k, tone)| (t(k), tone)));
                    let seen = e.watched >= WATCHED_AT;
                    let when = episode_when(&e, next_airing, now, offset_now);
                    rsx! {
                        div { key: "{n}", class: "tk-dep-wrap", "data-watched": "{seen}", "data-unaired": "{unaired}", "data-current": "{n == resume}",
                            DetailsEpisode {
                                episode: e.clone(),
                                view: active_view,
                                fallback_image: fallback_image.clone(),
                                current: n == resume,
                                pills,
                                unaired,
                                images,
                                when,
                                onclick: move |_| {
                                    if !unaired {
                                        navigator.push(Route::PlayerPage { id: id.clone(), episode: n });
                                    }
                                },
                            }
                            if let Some(toggle) = on_toggle.filter(|_| !unaired) {
                                IconButton {
                                    icon: "check".to_string(),
                                    label: if seen { tf("details.mark_unwatched", &[("n", &n.to_string())]) } else { tf("details.mark_watched", &[("n", &n.to_string())]) },
                                    variant: if seen { IconButtonVariant::Filled } else { IconButtonVariant::Standard },
                                    size: IconButtonSize::Sm,
                                    toggle: true,
                                    toggled: seen,
                                    class: "tk-dep__mark".to_string(),
                                    onclick: move |_| toggle.call((n, !seen)),
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// One episode in the active view. Unaired episodes are disabled.
#[component]
fn DetailsEpisode(
    episode: EpisodeVm,
    view: EpisodeView,
    current: bool,
    pills: Vec<(String, Tone)>,
    #[props(default)] unaired: bool,
    #[props(default = true)] images: bool,
    #[props(default)] fallback_image: Option<String>,
    /// Relative date / countdown line.
    #[props(default)] when: Option<String>,
    #[props(default)] onclick: EventHandler<MouseEvent>,
) -> Element {
    let n = episode.number;
    // No episode thumbnail: the series artwork, dimmed, with the number on top (a bare grey box read as "broken").
    let (thumb, fallback) = match episode.image.clone() {
        Some(i) => (Some(i), false),
        None => (fallback_image.clone(), true),
    };
    let heading = match episode.title.clone() {
        Some(title) => format!("{n}. {title}"),
        None => tf("details.episode", &[("n", &n.to_string())]),
    };
    // The absolute date stays in the tooltip.
    let day = episode.air_date.clone().map(|d| d.get(..10).unwrap_or(d.as_str()).to_owned());
    let tip = match (&day, unaired) {
        (Some(d), true) => format!("{} · {}", tf("details.unaired_title", &[("title", &heading)]), tf("details.unaired_on", &[("d", d)])),
        (None, true) => tf("details.unaired_title", &[("title", &heading)]),
        (Some(d), false) => format!("{heading} · {}", tf("details.aired", &[("d", d)])),
        (None, false) => heading.clone(),
    };
    let in_progress = episode.watched > 0.0 && episode.watched < WATCHED_AT;
    let bar = (episode.watched * 100.0).round();
    let runtime = episode.runtime_min.filter(|m| *m > 0).map(|m| tf("episodes.runtime", &[("n", &m.to_string())]));
    let rating = episode.rating.filter(|r| r.is_finite() && *r > 0.0).map(rating_text);
    let summary = episode.summary.clone().filter(|s| !s.trim().is_empty());
    let compact = view == EpisodeView::Compact;

    let thumb_el = rsx! {
        div { class: "tk-dep__thumb", "data-fallback": "{fallback}",
            if let Some(img) = thumb.filter(|_| images) {
                img { src: "{img}", alt: "", loading: "lazy", decoding: "async", draggable: "false" }
                if fallback {
                    span { class: "tk-dep__badge", "aria-hidden": "true", "{n}" }
                }
            } else {
                span { class: "tk-dep__mono", "aria-hidden": "true", "{n}" }
            }
            if view == EpisodeView::Cards {
                if let Some(r) = runtime.clone() {
                    span { class: "tk-dep__k", "data-k": "runtime", "{r}" }
                }
                if let Some(r) = rating.clone() {
                    span { class: "tk-dep__k", "data-k": "rating",
                        Icon { name: "star".to_string(), size: 12 }
                        "{r}"
                    }
                }
            } else if in_progress {
                div { class: "tk-dep__progress",
                    div { style: "width:{bar}%" }
                }
            }
        }
    };
    let title_el = rsx! {
        div { class: "tk-dep__title",
            span { class: "tk-dep__name", "{heading}" }
            for (text , tone) in pills {
                Pill { key: "{text}", text: text.clone(), tone, plain: true }
            }
        }
    };

    rsx! {
        button {
            class: "tk-dep",
            r#type: "button",
            title: "{tip}",
            "aria-current": "{current}",
            "data-layout": "{view.id()}",
            "data-compact": "{compact}",
            "data-unaired": "{unaired}",
            "data-episode": "{n}",
            disabled: unaired,
            "aria-disabled": "{unaired}",
            onclick: move |e| onclick.call(e),
            match view {
                EpisodeView::Cards => rsx! {
                    {thumb_el}
                    div { class: "tk-dep__body",
                        {title_el}
                        if let Some(s) = summary {
                            p { class: "tk-dep__syn", "{s}" }
                        }
                        if in_progress {
                            div { class: "tk-dep__bar", role: "presentation",
                                div { style: "width:{bar}%" }
                            }
                        }
                        div { class: "tk-dep__when",
                            span { {when.clone().unwrap_or_default()} }
                            if current {
                                span { class: "tk-dep__resume", Icon { name: "play".to_string(), size: 14 } {t("episodes.resume")} }
                            }
                        }
                    }
                },
                EpisodeView::List => rsx! {
                    {thumb_el}
                    div { class: "tk-dep__body",
                        {title_el}
                        if current {
                            div { class: "tk-dep__sub tk-dep__resume", {t("episodes.resume")} }
                        }
                    }
                    div { class: "tk-dep__meta",
                        if let Some(w) = when.clone() {
                            span { "{w}" }
                        }
                        if let Some(r) = runtime.clone() {
                            span { class: "tk-dep__runtime", "{r}" }
                        }
                    }
                },
                EpisodeView::Compact => rsx! {
                    div { class: "tk-dep__body",
                        {title_el}
                        if let Some(w) = when.clone() {
                            span { class: "tk-dep__sub", "{w}" }
                        }
                    }
                    if in_progress {
                        div { class: "tk-dep__bar", role: "presentation",
                            div { style: "width:{bar}%" }
                        }
                    }
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = SECS_PER_DAY;

    #[test]
    fn relative_days_cover_past_and_future() {
        let today = 20_000;
        assert_eq!(relative_day(today, today), RelDay::Today);
        assert_eq!(relative_day(today - 1, today), RelDay::Yesterday);
        assert_eq!(relative_day(today + 1, today), RelDay::Tomorrow);
        assert_eq!(relative_day(today - 3, today), RelDay::Ago(RelUnit::Days, 3));
        assert_eq!(relative_day(today + 6, today), RelDay::In(RelUnit::Days, 6));
        assert_eq!(relative_day(today - 7, today), RelDay::Ago(RelUnit::Weeks, 1));
        assert_eq!(relative_day(today + 14, today), RelDay::In(RelUnit::Weeks, 2));
        assert_eq!(relative_day(today - 29, today), RelDay::Ago(RelUnit::Weeks, 4));
        assert_eq!(relative_day(today - 30, today), RelDay::Ago(RelUnit::Months, 1));
        assert_eq!(relative_day(today - 364, today), RelDay::Ago(RelUnit::Months, 12));
        assert_eq!(relative_day(today - 365, today), RelDay::Ago(RelUnit::Years, 1));
        assert_eq!(relative_day(today + 800, today), RelDay::In(RelUnit::Years, 2));
    }

    #[test]
    fn relative_labels_read_naturally() {
        // Tests run in English.
        assert_eq!(relative_day_label(RelDay::Today), "today");
        assert_eq!(relative_day_label(RelDay::Yesterday), "yesterday");
        assert_eq!(relative_day_label(RelDay::Ago(RelUnit::Months, 1)), "1 month ago");
        assert_eq!(relative_day_label(RelDay::Ago(RelUnit::Weeks, 3)), "3 weeks ago");
        assert_eq!(relative_day_label(RelDay::In(RelUnit::Days, 6)), "in 6 days");
        assert_eq!(relative_day_label(RelDay::In(RelUnit::Weeks, 1)), "in 1 week");
    }

    #[test]
    fn bare_dates_are_calendar_days_and_times_follow_the_zone() {
        // 2023-09-29 is day 19_629.
        assert_eq!(air_day("2023-09-29", 0), Some(19_629));
        assert_eq!(air_day("2023-09-29", -5 * 3_600), Some(19_629), "a bare date never moves");
        // 23:30 UTC is already the next day in UTC+2 and still the same day in UTC-3.
        assert_eq!(air_day("2023-09-29T23:30:00Z", 2 * 3_600), Some(19_630));
        assert_eq!(air_day("2023-09-29T23:30:00Z", -3 * 3_600), Some(19_629));
        assert_eq!(air_day("soon", 0), None);
        assert_eq!(local_day(19_629 * DAY + 3_600, -2 * 3_600), 19_628);
    }

    #[test]
    fn episode_when_prefers_the_countdown_of_the_next_airing() {
        let now = 19_629 * DAY + 12 * 3_600; // 2023-09-29 12:00 UTC
        let mut e = EpisodeVm { number: 8, air_date: Some("2023-10-05".into()), ..EpisodeVm::default() };
        assert_eq!(episode_when(&e, None, now, 0).as_deref(), Some("in 6 days"));
        let next = Some(AiringVm { episode: 8, at_unix: now + 2 * DAY + 4 * 3_600 });
        let countdown = episode_when(&e, next, now, 0).unwrap();
        assert!(countdown.starts_with("in ") && countdown.contains('2') && countdown.contains('4'), "{countdown}");
        e.air_date = Some("2023-09-28".into());
        assert_eq!(episode_when(&e, None, now, 0).as_deref(), Some("yesterday"));
        e.air_date = None;
        assert_eq!(episode_when(&e, None, now, 0), None);
    }

    #[test]
    fn ratings_keep_two_decimals() {
        assert_eq!(rating_text(9.64), "9.64");
        assert_eq!(rating_text(10.0), "10.00");
        assert_eq!(rating_text(5.349_9), "5.35");
    }
}
