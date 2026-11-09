//! Airing calendar page `/calendar` (wave B, worker **b**; checklist `home.calendar.schedule`, `home.calendar.my-shows`,
//! `home.calendar.reminders`; classic `pages/base/CalendarPage.tsx`): the week grid by local day
//! (`components::AiringCalendar` with `compact: false`, `data-calendar-week`), All / My shows filter, previous / next
//! week, the reminders state (on / off + minutes, a shortcut to turn them on) and the "My shows" list with unfollow.
//! The reminders themselves are polled by Home (`Backend::reminders_due`).

use dioxus::prelude::*;

use crate::app::{Route, Services, Ui};
use crate::backend::MediaVm;
use crate::components::airing::{LocalClock, day_month, use_local_clock};
use crate::components::{
    AiringCalendar, Button, ButtonSize, ButtonVariant, EmptyState, ErrorState, IconButton, IconButtonSize,
    MediaCard, Segmented, SkeletonRow, use_snackbar,
};
use crate::i18n::{t, tf};
use crate::model::{SettingsAction, reduce_settings};

const WEEK: i64 = 7 * 86_400;

#[component]
pub fn CalendarPage() -> Element {
    let clock = use_local_clock();
    // Weeks away from the current one (negative = past).
    let mut shift = use_signal(|| 0_i64);
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
            div { class: "tk-page tk-calendar-page", "data-page": "calendar.title",
                {head}
                SkeletonRow { count: 7 }
            }
        };
    };
    let start = week_start(c, shift());
    let filter = if mine() { "mine" } else { "all" };
    rsx! {
        div { class: "tk-page tk-calendar-page", "data-page": "calendar.title",
            {head}
            div { class: "tk-calendar-page__bar",
                IconButton {
                    icon: "chevron-left".to_string(),
                    label: t("calendar.prev"),
                    onclick: move |_| shift -= 1,
                }
                span { class: "tk-calendar-page__week", "data-shift": "{shift}",
                    {tf("calendar.week_of", &[("date", &day_month(start + 43_200, c.offset_min))])}
                }
                IconButton {
                    icon: "chevron-right".to_string(),
                    label: t("calendar.next"),
                    onclick: move |_| shift += 1,
                }
                if shift() != 0 {
                    Button {
                        label: t("calendar.this_week"),
                        variant: ButtonVariant::Text,
                        size: ButtonSize::Sm,
                        onclick: move |_| shift.set(0),
                    }
                }
                span { class: "tk-calendar-page__spacer" }
                Segmented {
                    options: vec![("all".to_string(), t("calendar.filter.all")), ("mine".to_string(), t("calendar.filter.mine"))],
                    selected: filter.to_string(),
                    label: t("calendar.filter"),
                    small: true,
                    on_select: move |id: String| mine.set(id == "mine"),
                }
            }
            AiringCalendar { week_start_unix: start, compact: false, tracked_only: mine() }
            MyShows {}
        }
    }
}

/// Monday 00:00 local of the week `shift` weeks away from the current one.
fn week_start(c: LocalClock, shift: i64) -> i64 {
    c.week_monday() + shift * WEEK
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
}
