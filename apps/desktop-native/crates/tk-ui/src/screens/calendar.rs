//! Airing calendar page `/calendar` (wave B, worker **b**; checklist `home.calendar.schedule`, `home.calendar.my-shows`,
//! `home.calendar.reminders`; classic `pages/base/CalendarPage.tsx`), a rail destination (`NavDest::Calendar`).
//!
//! Four views following `Settings::look.calendar_view` (default Week), switched and saved by the icon segmented
//! control next to the navigation (`data-calendar-view`): **Week** (`AiringCalendar`, seven columns that turn into
//! rows below ~1500 px, `data-calendar-week`), **Agenda** (one row per day at every width), **Month**
//! (`AiringMonth`) and **Day** (`AiringDay`, a time line). Each view has its own previous / next (week, month, day)
//! and a way back to the current one. All / My shows filter, the reminders state (on / off + minutes, a shortcut to
//! turn them on) and the "My shows" list with unfollow. The reminders themselves are polled by Home
//! (`Backend::reminders_due`).

use dioxus::prelude::*;

use crate::app::{Route, Services, Ui};
use crate::backend::MediaVm;
use crate::components::airing::{
    LocalClock, add_months, civil_from_days, day_month, day_start_unix, local_day, local_weekday, use_local_clock,
};
use crate::components::{
    AiringCalendar, AiringDay, AiringMonth, Button, ButtonSize, ButtonVariant, EmptyState, ErrorState, IconButton, IconButtonSize,
    MediaCard, Segmented, SkeletonRow, use_snackbar,
};
use crate::i18n::{t, tf};
use crate::icons::Icon;
use crate::model::{CalendarView, SettingsAction, reduce_settings};

const WEEK: i64 = 7 * 86_400;

#[component]
pub fn CalendarPage() -> Element {
    let ui = use_context::<Ui>();
    let clock = use_local_clock();
    let view = ui.settings.read().look.calendar_view;
    // Weeks / months / days away from the current one (negative = past), one per kind of view.
    let shift = use_signal(|| 0_i64);
    let month_shift = use_signal(|| 0_i64);
    let day_shift = use_signal(|| 0_i64);
    let mut mine = use_signal(|| false);

    let head = rsx! {
        div { class: "tk-page__head tk-calendar-page__head",
            div {
                h1 { class: "tk-page__title", {t("calendar.title")} }
                p { class: "tk-page__intro", {t("calendar.intro")} }
            }
            RemindersState {}
        }
    };
    let Some(c) = clock() else {
        return rsx! {
            div { class: "tk-page tk-calendar-page", "data-page": "calendar.title", "data-view": "{view.id()}",
                {head}
                SkeletonRow { count: 7 }
            }
        };
    };
    let filter = if mine() { "mine" } else { "all" };
    let tracked_only = mine();
    let (y, m) = month_of(c, month_shift());
    let day = day_start(c, day_shift());
    let start = week_start(c, shift());

    // Navigation: (prev label, next label, current label, reset label, shift signal, data-shift).
    let (prev, next, label, reset, mut nav) = match view {
        CalendarView::Month => (
            t("schedule.prev_month"),
            t("schedule.next_month"),
            month_label(y, m),
            t("schedule.this_month"),
            month_shift,
        ),
        CalendarView::Day => (
            t("schedule.prev_day"),
            t("schedule.next_day"),
            day_label(c, day, day_shift()),
            t("schedule.back_today"),
            day_shift,
        ),
        CalendarView::Week | CalendarView::Agenda => (
            t("calendar.prev"),
            t("calendar.next"),
            tf("calendar.week_of", &[("date", &day_month(start + 43_200, c.offset_min))]),
            t("calendar.this_week"),
            shift,
        ),
    };
    let body = match view {
        CalendarView::Week => rsx! {
            AiringCalendar { week_start_unix: start, compact: false, tracked_only }
        },
        CalendarView::Agenda => rsx! {
            AiringCalendar { week_start_unix: start, compact: false, tracked_only, agenda: true }
        },
        CalendarView::Month => rsx! {
            AiringMonth { year: y, month: m, tracked_only }
        },
        CalendarView::Day => rsx! {
            AiringDay { day_start_unix: day, tracked_only }
        },
    };
    rsx! {
        div { class: "tk-page tk-calendar-page", "data-page": "calendar.title", "data-view": "{view.id()}",
            {head}
            div { class: "tk-calendar-page__bar",
                div { class: "tk-calendar-page__nav",
                    IconButton { icon: "chevron-left".to_string(), label: prev, onclick: move |_| nav -= 1 }
                    span { class: "tk-calendar-page__week", "data-shift": "{nav}", "aria-live": "polite", "{label}" }
                    IconButton { icon: "chevron-right".to_string(), label: next, onclick: move |_| nav += 1 }
                    if nav() != 0 {
                        Button {
                            label: reset,
                            variant: ButtonVariant::Text,
                            size: ButtonSize::Sm,
                            onclick: move |_| nav.set(0),
                        }
                    }
                }
                ViewSwitch { current: view }
                span { class: "tk-calendar-page__spacer" }
                Segmented {
                    options: vec![("all".to_string(), t("calendar.filter.all")), ("mine".to_string(), t("calendar.filter.mine"))],
                    selected: filter.to_string(),
                    label: t("calendar.filter"),
                    small: true,
                    on_select: move |id: String| mine.set(id == "mine"),
                }
            }
            {body}
            MyShows {}
        }
    }
}

/// Monday 00:00 local of the week `shift` weeks away from the current one.
fn week_start(c: LocalClock, shift: i64) -> i64 {
    c.week_monday() + shift * WEEK
}

/// `(year, month)` `shift` months away from the current one.
fn month_of(c: LocalClock, shift: i64) -> (i64, u32) {
    let (y, m, _) = civil_from_days(local_day(c.now, c.offset_min));
    add_months(y, m, shift)
}

/// Local midnight of the day `shift` days away from today.
fn day_start(c: LocalClock, shift: i64) -> i64 {
    day_start_unix(local_day(c.now, c.offset_min) + shift, c.offset_min)
}

fn month_label(y: i64, m: u32) -> String {
    tf("schedule.month_label", &[("month", &t(&format!("schedule.month.{m}"))), ("year", &y.to_string())])
}

/// "Today · Sunday 15/11" for yesterday / today / tomorrow, "Friday 20/11" otherwise.
fn day_label(c: LocalClock, day: i64, shift: i64) -> String {
    let (_, m, d) = civil_from_days(local_day(day + 43_200, c.offset_min));
    let name = t(&format!("calendar.day.{}", local_weekday(day + 43_200, c.offset_min)));
    let plain = tf("schedule.day_title", &[("day", &name), ("date", &format!("{d:02}/{m:02}"))]);
    let rel = match shift {
        -1 => t("schedule.yesterday"),
        0 => t("calendar.today"),
        1 => t("calendar.tomorrow"),
        _ => return plain,
    };
    tf("schedule.day_nav", &[("rel", &rel), ("day", &plain)])
}

fn view_icon(v: CalendarView) -> &'static str {
    match v {
        CalendarView::Week => "view-week",
        CalendarView::Agenda => "view-agenda",
        CalendarView::Month => "view-month",
        CalendarView::Day => "view-day",
    }
}

/// Week / Agenda / Month / Today with icons; picking one switches and saves `Settings::look.calendar_view`.
#[component]
fn ViewSwitch(current: CalendarView) -> Element {
    let ui = use_context::<Ui>();
    let services = use_context::<Services>();
    let set = use_callback(move |v: CalendarView| {
        let cur = ui.settings.peek().clone();
        if cur.look.calendar_view == v {
            return;
        }
        let mut look = cur.look.clone();
        look.calendar_view = v;
        ui.commit_settings(&services.backend, reduce_settings(&cur, SettingsAction::SetLook(look)));
    });
    let all = CalendarView::ALL;
    let at = all.iter().position(|v| *v == current).unwrap_or(0);
    rsx! {
        div {
            class: "tk-seg tk-seg--sm tk-calendar-views",
            role: "radiogroup",
            "aria-label": t("look.calendar"),
            "data-calendar-view": "{current.id()}",
            onkeydown: move |e| {
                let step = match e.key() {
                    Key::ArrowRight | Key::ArrowDown => 1,
                    Key::ArrowLeft | Key::ArrowUp => all.len() - 1,
                    _ => return,
                };
                e.prevent_default();
                set.call(all[(at + step) % all.len()]);
            },
            for (i , v) in all.iter().copied().enumerate() {
                button {
                    key: "{v.id()}",
                    class: "tk-seg__item tk-calendar-views__item",
                    r#type: "button",
                    role: "radio",
                    "data-view": "{v.id()}",
                    "aria-checked": if v == current { "true" } else { "false" },
                    tabindex: if i == at { "0" } else { "-1" },
                    title: t(&v.label_key()),
                    onclick: move |_| set.call(v),
                    Icon { name: view_icon(v).to_string(), size: 18 }
                    span { class: "tk-calendar-views__label", {t(&v.label_key())} }
                }
            }
        }
    }
}

/// "Reminders N min before" or "off" with a button that turns them on.
#[component]
fn RemindersState() -> Element {
    let ui = use_context::<Ui>();
    let services = use_context::<Services>();
    let prefs = ui.settings.read().home.clone();
    if prefs.airing_reminders {
        return rsx! {
            p { class: "tk-calendar-page__reminders tk-muted", "data-reminders": "on",
                {tf("calendar.reminders.on", &[("n", &prefs.reminder_minutes.to_string())])}
            }
        };
    }
    rsx! {
        div { class: "tk-calendar-page__reminders", "data-reminders": "off",
            span { class: "tk-muted", {t("calendar.reminders.off")} }
            Button {
                label: t("calendar.reminders.turn_on"),
                variant: ButtonVariant::Tonal,
                size: ButtonSize::Sm,
                onclick: move |_| {
                    let cur = ui.settings.peek().clone();
                    let mut home = cur.home.clone();
                    home.airing_reminders = true;
                    ui.commit_settings(&services.backend, reduce_settings(&cur, SettingsAction::SetHome(home)));
                },
            }
        }
    }
}

/// The tracked shows, each with an unfollow button.
#[component]
fn MyShows() -> Element {
    let services = use_context::<Services>();
    let navigator = use_navigator();
    let snack = use_snackbar();
    let mut shows = use_resource({
        let backend = services.backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.tracked_shows().await }
        }
    });
    let body = match &*shows.read() {
        None => rsx! {
            SkeletonRow { count: 6 }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), compact: true, on_retry: move |_| shows.restart() }
        },
        Some(Ok(list)) if list.is_empty() => rsx! {
            EmptyState { icon: "bookmark".to_string(), title: t("calendar.my_shows"), body: t("calendar.my_shows.empty") }
        },
        Some(Ok(list)) => rsx! {
            div { class: "tk-row tk-calendar-page__mine",
                for m in list.clone() {
                    {
                        let backend = services.backend.clone();
                        rsx! {
                            MyShow {
                                key: "{m.id}",
                                media: m.clone(),
                                on_open: move |id: String| {
                                    navigator.push(Route::DetailsPage { id });
                                },
                                on_unfollow: move |id: String| {
                                    let (backend, mut shows) = (backend.clone(), shows);
                                    spawn(async move {
                                        match backend.set_tracked(&id, false).await {
                                            Ok(()) => {
                                                snack.show(t("home.track.removed"));
                                                shows.restart();
                                            }
                                            Err(e) => snack.show(t(e.kind.key())),
                                        }
                                    });
                                },
                            }
                        }
                    }
                }
            }
        },
    };
    rsx! {
        section { class: "tk-section", "data-my-shows": "",
            div { class: "tk-section__head",
                h2 { class: "tk-section__title", {t("calendar.my_shows")} }
            }
            {body}
        }
    }
}

#[component]
fn MyShow(media: MediaVm, on_open: EventHandler<String>, on_unfollow: EventHandler<String>) -> Element {
    let (open_id, drop_id) = (media.id.clone(), media.id.clone());
    rsx! {
        div { class: "tk-calendar-page__show", "data-tracked": "true",
            MediaCard { media: media.clone(), onclick: move |_| on_open.call(open_id.clone()) }
            IconButton {
                icon: "close".to_string(),
                label: tf("home.track.unfollow", &[("title", &media.title)]),
                size: IconButtonSize::Sm,
                class: "tk-calendar-page__unfollow".to_string(),
                onclick: move |_| on_unfollow.call(drop_id.clone()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weeks_shift_from_this_monday() {
        // Wednesday 2026-11-11 12:00 UTC, UTC clock.
        let c = LocalClock {
            now: 1_794_182_400 + 2 * 86_400 + 43_200,
            offset_min: 0,
        };
        assert_eq!(week_start(c, 0), 1_794_182_400);
        assert_eq!(week_start(c, -1), 1_794_182_400 - WEEK);
        assert_eq!(week_start(c, 2), 1_794_182_400 + 2 * WEEK);
    }

    #[test]
    fn months_and_days_shift_from_today() {
        // Sunday 2026-11-15 22:30 local in Buenos Aires (UTC-3) = Monday 01:30 UTC.
        let c = LocalClock { now: 1_794_182_400 + 7 * 86_400 + 5_400, offset_min: 180 };
        assert_eq!(month_of(c, 0), (2026, 11));
        assert_eq!(month_of(c, 2), (2027, 1));
        let today = day_start(c, 0);
        assert_eq!(today, c.today());
        assert_eq!(day_start(c, 1) - today, 86_400);
        assert_eq!(day_start(c, -3), today - 3 * 86_400);
    }
}
