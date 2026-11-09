//! Airing calendar (wave B, worker **b**; checklist `home.calendar.schedule`, `home.calendar.my-shows`,
//! `home.calendar.reminders`, `details.track-show`; classic `CalendarPage.tsx`, `TrackShowButton.tsx`).
//!
//! * [`AiringCalendar`]: the block Home embeds (`compact: true`: the first two days, today + tomorrow) and the week grid
//!   of `/calendar` (`compact: false`, 7 columns). `Backend::airing_schedule(week_start_unix)`, columns by **local** day
//!   (the offset comes from the web view, [`use_local_clock`]), tracked shows highlighted (`data-tracked`), follow /
//!   unfollow in place with `set_tracked`. `data-calendar-week="<unix>"` on the grid.
//! * [`TrackShowButton`]: the "Follow" button of Details (mounted there by the integrator): pins the series in the
//!   calendar and turns the airing reminders on (`Settings::home.airing_reminders`) the first time.
//!
//! Local time: the Rust side only knows UTC, so the calendar asks the web view for `Date.now()` and
//! `getTimezoneOffset()` once a minute; day boundaries ignore a DST change inside the week (one hour at most).

use std::collections::HashMap;
use std::time::Duration;

use dioxus::prelude::*;

use crate::app::{Route, Services, Ui};
use crate::backend::AiringSlotVm;
use crate::components::{
    Button, ButtonSize, ButtonVariant, EmptyState, ErrorState, IconButton, IconButtonSize, SkeletonLines,
    use_snackbar,
};
use crate::i18n::{t, tf};
use crate::model::{SettingsAction, reduce_settings};

/// Sends `[unix seconds, getTimezoneOffset() minutes]`.
const CLOCK_JS: &str = "dioxus.send([Math.floor(Date.now() / 1000), new Date().getTimezoneOffset()]);";
const DAY: i64 = 86_400;

/// "Now" and the local offset as the web view knows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalClock {
    pub now: i64,
    /// `Date.getTimezoneOffset()`: minutes to add to local time to get UTC (UTC-3 = 180).
    pub offset_min: i64,
}

impl LocalClock {
    /// Local midnight of today, as unix seconds.
    pub fn today(&self) -> i64 {
        local_midnight(self.now, self.offset_min)
    }

    /// Local midnight of this week's Monday.
    pub fn week_monday(&self) -> i64 {
        week_monday(self.now, self.offset_min)
    }
}

/// The local clock, refreshed every minute (`None` until the web view answered).
pub fn use_local_clock() -> Signal<Option<LocalClock>> {
    let mut clock = use_signal(|| None::<LocalClock>);
    use_future(move || async move {
        loop {
            let mut eval = document::eval(CLOCK_JS);
            if let Ok((now, offset_min)) = eval.recv::<(i64, i64)>().await {
                clock.set(Some(LocalClock { now, offset_min }));
            }
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    });
    clock
}

// ---------------------------------------------------------------------------------------------
// Pure time helpers (tested)
// ---------------------------------------------------------------------------------------------

/// Unix seconds of the local midnight starting the day of `unix`.
pub fn local_midnight(unix: i64, offset_min: i64) -> i64 {
    let local = unix - offset_min * 60;
    local.div_euclid(DAY) * DAY + offset_min * 60
}

/// Local weekday of `unix`, 0 = Monday.
pub fn local_weekday(unix: i64, offset_min: i64) -> u8 {
    // 1970-01-01 was a Thursday (index 3).
    (((unix - offset_min * 60).div_euclid(DAY) + 3).rem_euclid(7)) as u8
}

/// Local midnight of the Monday of the week of `unix`.
pub fn week_monday(unix: i64, offset_min: i64) -> i64 {
    local_midnight(unix, offset_min) - i64::from(local_weekday(unix, offset_min)) * DAY
}

/// `HH:MM` local time of `unix`.
pub fn hhmm(unix: i64, offset_min: i64) -> String {
    let secs = (unix - offset_min * 60).rem_euclid(DAY);
    format!("{:02}:{:02}", secs / 3600, secs % 3600 / 60)
}

/// Civil date `(year, month, day)` of a count of days since 1970-01-01 (H. Hinnant's algorithm).
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// `DD/MM` local date of `unix`.
pub fn day_month(unix: i64, offset_min: i64) -> String {
    let (_, m, d) = civil_from_days((unix - offset_min * 60).div_euclid(DAY));
    format!("{d:02}/{m:02}")
}

/// Column of `at` in a grid starting at `start` (local midnight), `None` outside `0..days`.
pub fn column_of(at: i64, start: i64, days: usize) -> Option<usize> {
    let i = (at - start).div_euclid(DAY);
    usize::try_from(i).ok().filter(|i| *i < days)
}

/// Slots split into `days` columns, each sorted by time; `tracked_only` keeps My shows.
pub fn columns(
    slots: &[AiringSlotVm],
    start: i64,
    days: usize,
    tracked_only: bool,
) -> Vec<Vec<AiringSlotVm>> {
    let mut cols = vec![Vec::new(); days];
    for s in slots.iter().filter(|s| !tracked_only || s.tracked) {
        if let Some(i) = column_of(s.at_unix, start, days) {
            cols[i].push(s.clone());
        }
    }
    for c in &mut cols {
        c.sort_by(|a, b| {
            a.at_unix
                .cmp(&b.at_unix)
                .then_with(|| a.media.title.cmp(&b.media.title))
        });
    }
    cols
}

// ---------------------------------------------------------------------------------------------
// Components
// ---------------------------------------------------------------------------------------------

#[component]
pub fn AiringCalendar(
    /// Local midnight (unix seconds) of the first day shown.
    week_start_unix: i64,
    #[props(default)] compact: bool,
    /// Only the user's tracked shows (`home.calendar.my-shows` filter).
    #[props(default)]
    tracked_only: bool,
) -> Element {
    let services = use_context::<Services>();
    let clock = use_local_clock();
    let mut schedule = use_resource(use_reactive((&week_start_unix,), {
        let backend = services.backend.clone();
        move |(week,)| {
            let backend = backend.clone();
            async move { backend.airing_schedule(week).await }
        }
    }));
    // Follow / unfollow done here, so a toggle does not reload the whole week.
    let overrides = use_signal(HashMap::<String, bool>::new);

    let Some(clock) = clock() else {
        return rsx! {
            div { class: "tk-calendar", "aria-busy": "true", SkeletonLines { widths: vec![90, 70, 80] } }
        };
    };
    let days = if compact { 2 } else { 7 };
    let body = match &*schedule.read() {
        None => rsx! {
            div { class: "tk-calendar", "aria-busy": "true", "data-compact": "{compact}",
                SkeletonLines { widths: vec![90, 70, 80, 60] }
            }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), compact: true, on_retry: move |_| schedule.restart() }
        },
        Some(Ok(list)) => {
            let ov = overrides.read().clone();
            let slots: Vec<AiringSlotVm> = list
                .iter()
                .map(|s| {
                    let mut s = s.clone();
                    if let Some(v) = ov.get(&s.media.id) {
                        s.tracked = *v;
                    }
                    s
                })
                .collect();
            let cols = columns(&slots, week_start_unix, days, tracked_only);
            if cols.iter().all(Vec::is_empty) && !compact {
                let title = if tracked_only {
                    t("calendar.empty_mine")
                } else {
                    t("calendar.empty")
                };
                rsx! {
                    div { class: "tk-calendar", "data-calendar-week": "{week_start_unix}", "data-empty": "true",
                        EmptyState { icon: "calendar".to_string(), title }
                    }
                }
            } else {
                let today = clock.today();
                rsx! {
                    div {
                        class: "tk-calendar",
                        "data-calendar-week": "{week_start_unix}",
                        "data-compact": "{compact}",
                        "data-days": "{days}",
                        for (i , col) in cols.into_iter().enumerate() {
                            {
                                let day = week_start_unix + i as i64 * DAY;
                                // Midday avoids an off-by-one when DST moved the boundary by an hour.
                                let noon = day + DAY / 2;
                                let is_today = local_midnight(noon, clock.offset_min) == today;
                                let name = if compact && is_today {
                                    t("calendar.today")
                                } else if compact && local_midnight(noon, clock.offset_min) == today + DAY {
                                    t("calendar.tomorrow")
                                } else {
                                    t(&format!("calendar.day.{}", local_weekday(noon, clock.offset_min)))
                                };
                                let date = day_month(noon, clock.offset_min);
                                rsx! {
                                    section { key: "{day}", class: "tk-calendar__day", "data-day": "{i}", "data-today": "{is_today}",
                                        h3 { class: "tk-calendar__day-head",
                                            span { class: "tk-calendar__day-name", "{name}" }
                                            span { class: "tk-calendar__day-date tk-mono", "{date}" }
                                        }
                                        if col.is_empty() {
                                            p { class: "tk-calendar__none tk-muted", {t("calendar.empty_day")} }
                                        }
                                        for s in col {
                                            SlotCard { key: "{s.media.id}-{s.episode}-{s.at_unix}", slot: s.clone(), offset_min: clock.offset_min, overrides }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    };
    body
}

/// One airing slot: poster, local time, title, episode and the follow toggle.
#[component]
fn SlotCard(slot: AiringSlotVm, offset_min: i64, overrides: Signal<HashMap<String, bool>>) -> Element {
    let services = use_context::<Services>();
    let navigator = use_navigator();
    let snack = use_snackbar();
    let busy = use_signal(|| false);
    let time = hhmm(slot.at_unix, offset_min);
    let ep = (slot.episode > 0).then(|| tf("calendar.ep", &[("n", &slot.episode.to_string())]));
    let id = slot.media.id.clone();
    let tracked = slot.tracked;
    let label = if tracked {
        tf("home.track.unfollow", &[("title", &slot.media.title)])
    } else {
        tf("home.track.follow_title", &[("title", &slot.media.title)])
    };
    let toggle = {
        let (backend, id) = (services.backend.clone(), id.clone());
        move |_| {
            if *busy.peek() {
                return;
            }
            let (mut busy, mut overrides) = (busy, overrides);
            busy.set(true);
            let (backend, id) = (backend.clone(), id.clone());
            spawn(async move {
                match backend.set_tracked(&id, !tracked).await {
                    Ok(()) => {
                        overrides.write().insert(id, !tracked);
                        snack.show(if tracked {
                            t("home.track.removed")
                        } else {
                            t("home.track.followed")
                        });
                    }
                    Err(e) => snack.show(t(e.kind.key())),
                }
                busy.set(false);
            });
        }
    };
    rsx! {
        div { class: "tk-calendar__slot", "data-tracked": "{tracked}", "data-media-id": "{slot.media.id}",
            button {
                class: "tk-calendar__open",
                r#type: "button",
                "aria-label": "{slot.media.title}",
                onclick: move |_| {
                    navigator.push(Route::DetailsPage { id: id.clone() });
                },
                if let Some(src) = slot.media.cover.clone() {
                    img { class: "tk-calendar__poster", src: "{src}", alt: "", loading: "lazy" }
                } else {
                    span { class: "tk-calendar__poster tk-calendar__poster--empty", "aria-hidden": "true" }
                }
                span { class: "tk-calendar__text",
                    span { class: "tk-calendar__time tk-mono", "{time}" }
                    span { class: "tk-calendar__title", "{slot.media.title}" }
                    if let Some(ep) = ep {
                        span { class: "tk-calendar__ep", "{ep}" }
                    }
                }
            }
            IconButton {
                icon: if tracked { "check".to_string() } else { "bookmark".to_string() },
                label,
                size: IconButtonSize::Sm,
                toggle: true,
                toggled: tracked,
                disabled: busy(),
                onclick: toggle,
            }
        }
    }
}

/// "Follow" on Details (`details.track-show`): pins the title in the calendar; the first follow also turns the
/// airing reminders on. `data-tracked` on the wrapper.
#[component]
pub fn TrackShowButton(media_id: String) -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let snack = use_snackbar();
    let state = use_resource(use_reactive((&media_id,), {
        let backend = services.backend.clone();
        move |(id,)| {
            let backend = backend.clone();
            async move { backend.is_tracked(&id).await }
        }
    }));
    let local = use_signal(|| None::<bool>);
    let busy = use_signal(|| false);
    let loaded = match &*state.read() {
        Some(Ok(v)) => Some(*v),
        _ => None,
    };
    let tracked = local().or(loaded).unwrap_or(false);
    let loading = loaded.is_none() && local().is_none() && state.read().is_none();
    let onclick = {
        let (backend, id) = (services.backend.clone(), media_id.clone());
        move |_| {
            if *busy.peek() {
                return;
            }
            let (mut busy, mut local) = (busy, local);
            busy.set(true);
            let (backend, id) = (backend.clone(), id.clone());
            spawn(async move {
                match backend.set_tracked(&id, !tracked).await {
                    Ok(()) => {
                        local.set(Some(!tracked));
                        if tracked {
                            snack.show(t("home.track.removed"));
                        } else {
                            let cur = ui.settings.peek().clone();
                            if !cur.home.airing_reminders {
                                let mut home = cur.home.clone();
                                home.airing_reminders = true;
                                ui.commit_settings(
                                    &backend,
                                    reduce_settings(&cur, SettingsAction::SetHome(home)),
                                );
                            }
                            snack.show(t("home.track.followed"));
                        }
                    }
                    Err(e) => snack.show(t(e.kind.key())),
                }
                busy.set(false);
            });
        }
    };
    rsx! {
        span { class: "tk-track", "data-tracked": "{tracked}", "data-track-show": "{media_id}", title: t("home.track.hint"),
            Button {
                label: if tracked { t("home.track.following") } else { t("home.track.follow") },
                variant: if tracked { ButtonVariant::Tonal } else { ButtonVariant::Outlined },
                size: ButtonSize::Md,
                icon: if tracked { "check".to_string() } else { "calendar".to_string() },
                disabled: busy() || loading,
                onclick,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::MediaVm;

    /// 2026-11-09 00:00 UTC, a Monday.
    const MONDAY_UTC: i64 = 1_794_182_400;

    fn slot(id: &str, at: i64, tracked: bool) -> AiringSlotVm {
        AiringSlotVm {
            media: MediaVm {
                id: id.into(),
                title: id.into(),
                cover: None,
                color: None,
                subtitle: String::new(),
                score: None,
            },
            episode: 1,
            at_unix: at,
            tracked,
        }
    }

    #[test]
    fn local_days_follow_the_offset() {
        // Buenos Aires (UTC-3, offset 180): Monday 00:00 UTC is Sunday 21:00 local.
        assert_eq!(local_weekday(MONDAY_UTC, 180), 6);
        assert_eq!(local_midnight(MONDAY_UTC, 180), MONDAY_UTC - DAY + 3 * 3600);
        assert_eq!(hhmm(MONDAY_UTC, 180), "21:00");
        // Tokyo (UTC+9, offset -540): Monday 09:00 local.
        assert_eq!(local_weekday(MONDAY_UTC, -540), 0);
        assert_eq!(hhmm(MONDAY_UTC, -540), "09:00");
        assert_eq!(week_monday(MONDAY_UTC + 3 * DAY, 0), MONDAY_UTC);
        assert_eq!(week_monday(MONDAY_UTC, 180), MONDAY_UTC - 7 * DAY + 3 * 3600);
    }

    #[test]
    fn civil_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(MONDAY_UTC / DAY), (2026, 11, 9));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        assert_eq!(day_month(MONDAY_UTC, 0), "09/11");
        // 2024-02-29
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
    }

    #[test]
    fn slots_fall_into_their_local_column() {
        let start = MONDAY_UTC;
        let slots = vec![
            slot("b", start + DAY + 100, false),
            slot("a", start + DAY + 50, true),
            slot("x", start + 8 * DAY, true),
            slot("y", start - 1, false),
        ];
        let cols = columns(&slots, start, 7, false);
        assert_eq!(cols.len(), 7);
        assert_eq!(
            cols[1].iter().map(|s| s.media.id.as_str()).collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert!(cols[0].is_empty());
        let mine = columns(&slots, start, 7, true);
        assert_eq!(mine[1].len(), 1);
        assert_eq!(columns(&slots, start, 2, false).len(), 2);
        assert_eq!(column_of(start + 7 * DAY, start, 7), None);
    }
}
