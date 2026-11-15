//! Airing calendar (wave B, worker **b**; checklist `home.calendar.schedule`, `home.calendar.my-shows`,
//! `home.calendar.reminders`, `details.track-show`; classic `CalendarPage.tsx`, `TrackShowButton.tsx`).
//!
//! * [`AiringCalendar`]: the block Home embeds (`compact: true`: the first two days, today + tomorrow) and the Week /
//!   Agenda views of `/calendar` (`compact: false`, 7 columns; `agenda: true` one row per day at every width, past
//!   days dimmed). `Backend::airing_schedule(week_start_unix)`, columns by **local** day (the offset comes from the
//!   web view, [`use_local_clock`]), tracked shows highlighted (`data-tracked`), follow / unfollow in place with
//!   `set_tracked`. `data-calendar-week="<unix>"` and `data-layout="compact|week|agenda"` on the grid.
//! * [`AiringMonth`]: the Month view (2026-11 UI refresh): Monday-first grid, poster stacks, a dot for the user's
//!   shows, the picked day's list; one `airing_schedule` per week row, fetched at once and merged ([`merge_weeks`]).
//! * [`AiringDay`]: the Day ("Today") view: a time line with hour marks, a "now" line and relative labels.
//! * [`TrackShowButton`]: the "Follow" button of Details (mounted there by the integrator): pins the series in the
//!   calendar and turns the airing reminders on (`Settings::home.airing_reminders`) the first time.
//!
//! Local time: the Rust side only knows UTC, so the app asks the web view for `Date.now()` and `getTimezoneOffset()`
//! once a minute through ONE shared clock ([`use_local_clock`]: a global signal fed by a single loop); day boundaries
//! ignore a DST change inside the week (one hour at most).

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::task::Poll;
use std::time::Duration;

use dioxus::dioxus_core::{ScopeId, consume_context_from_scope, provide_root_context, spawn_forever};
use dioxus::prelude::*;

use crate::app::{Route, Services, Ui};
use crate::backend::{AiringSlotVm, UiResult};
use crate::components::{
    Button, ButtonSize, ButtonVariant, EmptyState, ErrorState, IconButton, IconButtonSize, SkeletonLines,
    use_snackbar,
};
use crate::i18n::{t, tf, tn};
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

/// The shared local clock (`None` until the web view answered), fed by one loop per app ([`use_local_clock`]).
static CLOCK: GlobalSignal<Option<LocalClock>> = Signal::global(|| None);

/// Marker in the root scope: the clock loop of this app is running.
#[derive(Clone, Copy)]
struct ClockRunning;

/// The local clock, refreshed every minute (`None` until the web view answered). Every caller shares one signal and
/// one JS poll: the first call starts the loop (in the root scope, so it outlives the screen that started it).
pub fn use_local_clock() -> Signal<Option<LocalClock>> {
    use_hook(|| {
        let mut clock = CLOCK.signal();
        if consume_context_from_scope::<ClockRunning>(ScopeId::ROOT).is_none() {
            provide_root_context(ClockRunning);
            spawn_forever(async move {
                loop {
                    let mut eval = document::eval(CLOCK_JS);
                    if let Ok((now, offset_min)) = eval.recv::<(i64, i64)>().await {
                        let next = Some(LocalClock { now, offset_min });
                        if *clock.peek() != next {
                            clock.set(next);
                        }
                    }
                    tokio::time::sleep(Duration::from_secs(60)).await;
                }
            });
        }
        clock
    })
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

/// Days since 1970-01-01 of a civil date (inverse of [`civil_from_days`]).
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = i64::from(m);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Local day number (days since 1970-01-01 in local time) of `unix`.
pub fn local_day(unix: i64, offset_min: i64) -> i64 {
    (unix - offset_min * 60).div_euclid(DAY)
}

/// Local midnight (unix seconds) of a local day number.
pub fn day_start_unix(day: i64, offset_min: i64) -> i64 {
    day * DAY + offset_min * 60
}

/// `(year, month)` moved by `delta` months.
pub fn add_months(year: i64, month: u32, delta: i64) -> (i64, u32) {
    let idx = year * 12 + i64::from(month) - 1 + delta;
    (idx.div_euclid(12), (idx.rem_euclid(12) + 1) as u32)
}

/// Days in a month.
pub fn month_len(year: i64, month: u32) -> u32 {
    let (ny, nm) = add_months(year, month, 1);
    (days_from_civil(ny, nm, 1) - days_from_civil(year, month, 1)) as u32
}

/// Cells of a Monday-first month grid: whole weeks from the Monday on or before the 1st to the Sunday on or after
/// the last day (5 or 6 rows, 4 for a February starting on Monday).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonthGrid {
    /// Local day number of the first cell (a Monday).
    pub first: i64,
    /// Number of cells, a multiple of 7.
    pub cells: usize,
    /// Local day numbers of the 1st and the last day of the month.
    pub month_first: i64,
    pub month_last: i64,
}

impl MonthGrid {
    pub fn new(year: i64, month: u32) -> Self {
        let month_first = days_from_civil(year, month, 1);
        let month_last = month_first + i64::from(month_len(year, month)) - 1;
        // 1970-01-01 was a Thursday (Monday-first index 3).
        let lead = (month_first + 3).rem_euclid(7);
        let first = month_first - lead;
        let trail = 6 - (month_last + 3).rem_euclid(7);
        let cells = (month_last + trail - first + 1) as usize;
        Self { first, cells, month_first, month_last }
    }

    /// Local day number of cell `i`.
    pub fn day(&self, i: usize) -> i64 {
        self.first + i as i64
    }

    /// The day belongs to the month (not a leading / trailing day of the neighbours).
    pub fn in_month(&self, day: i64) -> bool {
        (self.month_first..=self.month_last).contains(&day)
    }

    pub fn contains(&self, day: i64) -> bool {
        (self.first..self.first + self.cells as i64).contains(&day)
    }

    /// Week starts (local Monday midnight, unix seconds) of every row: the `airing_schedule` weeks to fetch.
    pub fn weeks(&self, offset_min: i64) -> Vec<i64> {
        (0..self.cells / 7)
            .map(|w| day_start_unix(self.first + 7 * w as i64, offset_min))
            .collect()
    }
}

/// Weeks (starts, 7 days each) not wholly covered by an answer complete up to `complete_until` (0.3.1).
pub fn missing_weeks(weeks: &[i64], complete_until: i64) -> usize {
    weeks.iter().filter(|w| **w + 7 * DAY - 1 > complete_until).count()
}

/// Merge the answers of several weeks: duplicates (same title, episode and time) once, sorted by time. An error only
/// when every week failed (then the first one); a partial answer shows what arrived.
pub fn merge_weeks(results: Vec<UiResult<Vec<AiringSlotVm>>>) -> UiResult<Vec<AiringSlotVm>> {
    let mut first_err = None;
    let mut any_ok = false;
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for r in results {
        match r {
            Ok(list) => {
                any_ok = true;
                for s in list {
                    if seen.insert((s.media.id.clone(), s.episode, s.at_unix)) {
                        out.push(s);
                    }
                }
            }
            Err(e) => {
                first_err.get_or_insert(e);
            }
        }
    }
    match first_err {
        Some(e) if !any_ok => Err(e),
        _ => {
            out.sort_by(|a, b| a.at_unix.cmp(&b.at_unix).then_with(|| a.media.title.cmp(&b.media.title)));
            Ok(out)
        }
    }
}

/// Polls every future at once and returns their outputs in order (a small `join_all`; no extra crate).
pub async fn join_all<F: Future>(futs: Vec<F>) -> Vec<F::Output> {
    let mut futs: Vec<Pin<Box<F>>> = futs.into_iter().map(Box::pin).collect();
    let mut out: Vec<Option<F::Output>> = futs.iter().map(|_| None).collect();
    std::future::poll_fn(move |cx| {
        let mut pending = false;
        for (f, o) in futs.iter_mut().zip(out.iter_mut()) {
            if o.is_none() {
                match f.as_mut().poll(cx) {
                    Poll::Ready(v) => *o = Some(v),
                    Poll::Pending => pending = true,
                }
            }
        }
        if pending {
            Poll::Pending
        } else {
            Poll::Ready(out.iter_mut().filter_map(Option::take).collect())
        }
    })
    .await
}

/// Pixels per hour of the day time line.
pub const TIMELINE_HOUR_PX: i64 = 72;
/// Height of one event of the time line; events closer than this share the row side by side.
pub const TIMELINE_EVENT_PX: i64 = 60;

/// Hours `[from, to)` the time line shows: every event and "now" (seconds into the day), one hour of margin on each
/// side, within the day. `None` when there is nothing to show.
pub fn timeline_hours(events: &[i64], now: Option<i64>) -> Option<(i64, i64)> {
    let points: Vec<i64> = events.iter().copied().chain(now).map(|s| s.clamp(0, DAY - 1)).collect();
    let lo = points.iter().min()? / 3600;
    let hi = points.iter().max()? / 3600 + 1;
    Some(((lo - 1).max(0), (hi + 1).min(24)))
}

/// Vertical position (px) of `secs` into the day on a time line starting at hour `from`.
pub fn timeline_top(secs: i64, from_hour: i64) -> i64 {
    (secs - from_hour * 3600) * TIMELINE_HOUR_PX / 3600
}

/// Side-by-side lanes for events (seconds into the day, sorted) that would overlap: `(lane, lanes)` per event, where
/// `lanes` is the width of its group of overlapping events.
pub fn timeline_lanes(events: &[i64]) -> Vec<(usize, usize)> {
    let span = TIMELINE_EVENT_PX * 3600 / TIMELINE_HOUR_PX;
    let mut out = vec![(0, 1); events.len()];
    let mut i = 0;
    while i < events.len() {
        // A group: events chained by overlap.
        let mut ends: Vec<i64> = Vec::new();
        let mut group_end = i64::MIN;
        let mut j = i;
        while j < events.len() && (j == i || events[j] < group_end) {
            let lane = ends.iter().position(|e| *e <= events[j]).unwrap_or(ends.len());
            if lane == ends.len() {
                ends.push(0);
            }
            ends[lane] = events[j] + span;
            group_end = group_end.max(events[j] + span);
            out[j].0 = lane;
            j += 1;
        }
        for o in &mut out[i..j] {
            o.1 = ends.len();
        }
        i = j;
    }
    out
}

/// How far an airing is from now, for the time line labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relative {
    /// Aired less than a minute ago (or within the next minute).
    Now,
    /// Airs in `h` hours and `m` minutes.
    In { h: i64, m: i64 },
    /// Aired `h` hours and `m` minutes ago.
    Ago { h: i64, m: i64 },
}

pub fn relative(at: i64, now: i64) -> Relative {
    let diff = at - now;
    if diff.abs() < 60 {
        return Relative::Now;
    }
    // Upcoming rounds up (a show at 10:00 is "in 1 min" at 9:58:30), past rounds down.
    let mins = if diff > 0 { (diff + 59) / 60 } else { -diff / 60 };
    let (h, m) = (mins / 60, mins % 60);
    if diff > 0 { Relative::In { h, m } } else { Relative::Ago { h, m } }
}

/// i18n text of [`relative`]: "in 22 min", "in 1 h 52 min", "aired 1 h ago".
pub fn relative_text(r: Relative) -> String {
    match r {
        Relative::Now => t("schedule.rel.now"),
        Relative::In { h: 0, m } => tf("schedule.rel.in_min", &[("n", &m.to_string())]),
        Relative::In { h, m: 0 } => tf("schedule.rel.in_h", &[("h", &h.to_string())]),
        Relative::In { h, m } => tf("schedule.rel.in_hm", &[("h", &h.to_string()), ("m", &m.to_string())]),
        Relative::Ago { h: 0, m } => tf("schedule.rel.ago_min", &[("n", &m.to_string())]),
        Relative::Ago { h, .. } => tf("schedule.rel.ago_h", &[("h", &h.to_string())]),
    }
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
    /// One row per day with wide slots at every width (the Agenda view); past days dimmed. Without it the week is
    /// seven columns that still turn into rows below ~1500 px.
    #[props(default)]
    agenda: bool,
) -> Element {
    let services = use_context::<Services>();
    let clock = use_local_clock();
    let layout = if compact { "compact" } else if agenda { "agenda" } else { "week" };
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
            let slots = with_overrides(list, &overrides.read());
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
                        "data-layout": "{layout}",
                        "data-days": "{days}",
                        for (i , col) in cols.into_iter().enumerate() {
                            {
                                let day = week_start_unix + i as i64 * DAY;
                                // Midday avoids an off-by-one when DST moved the boundary by an hour.
                                let noon = day + DAY / 2;
                                let is_today = local_midnight(noon, clock.offset_min) == today;
                                let is_past = local_midnight(noon, clock.offset_min) < today;
                                let name = if compact && is_today {
                                    t("calendar.today")
                                } else if compact && local_midnight(noon, clock.offset_min) == today + DAY {
                                    t("calendar.tomorrow")
                                } else {
                                    t(&format!("calendar.day.{}", local_weekday(noon, clock.offset_min)))
                                };
                                let date = day_month(noon, clock.offset_min);
                                rsx! {
                                    section { key: "{day}", class: "tk-calendar__day", "data-day": "{i}", "data-today": "{is_today}", "data-past": "{is_past}",
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
fn SlotCard(
    slot: AiringSlotVm,
    offset_min: i64,
    overrides: Signal<HashMap<String, bool>>,
    /// "in 22 min" / "aired 1 h ago" next to the time (the Day view).
    #[props(default)]
    rel: Option<String>,
) -> Element {
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
                    span { class: "tk-calendar__time tk-mono",
                        "{time}"
                        if let Some(rel) = rel {
                            span { class: "tk-calendar__rel", " · {rel}" }
                        }
                    }
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

/// Slots with the follow / unfollow done in place applied.
fn with_overrides(list: &[AiringSlotVm], ov: &HashMap<String, bool>) -> Vec<AiringSlotVm> {
    list.iter()
        .map(|s| {
            let mut s = s.clone();
            if let Some(v) = ov.get(&s.media.id) {
                s.tracked = *v;
            }
            s
        })
        .collect()
}

/// Weekday name and `DD/MM` of a local day number ("Wednesday 11/11").
fn day_title(day: i64) -> String {
    let (_, m, d) = civil_from_days(day);
    tf(
        "schedule.day_title",
        &[("day", &t(&format!("calendar.day.{}", (day + 3).rem_euclid(7)))), ("date", &format!("{d:02}/{m:02}"))],
    )
}

/// Month view: a Monday-first grid with up to three posters per day, "+N" and a dot when one of the user's shows
/// airs; a click opens that day's list under the grid (beside it on wide windows). One `airing_range` call for the
/// whole grid (0.3.1). `data-calendar-month="YYYY-MM"`.
#[component]
pub fn AiringMonth(year: i64, month: u32, #[props(default)] tracked_only: bool) -> Element {
    let clock = use_local_clock();
    let Some(c) = clock() else {
        return rsx! {
            div { class: "tk-month", "aria-busy": "true", SkeletonLines { widths: vec![90, 70, 80] } }
        };
    };
    rsx! {
        MonthBody { year, month, tracked_only, offset_min: c.offset_min, today: local_day(c.now, c.offset_min) }
    }
}

#[component]
fn MonthBody(year: i64, month: u32, tracked_only: bool, offset_min: i64, today: i64) -> Element {
    let services = use_context::<Services>();
    let mut schedule = use_resource(use_reactive((&year, &month, &offset_min), {
        let backend = services.backend.clone();
        move |(y, m, off)| {
            let backend = backend.clone();
            async move {
                // 0.3.1: the whole grid as ONE range (AniList pages of 50 instead of one range per week); a partial
                // answer still says how many weeks are missing (QA 0.3).
                let weeks = MonthGrid::new(y, m).weeks(off);
                let (Some(&from), Some(&last)) = (weeks.first(), weeks.last()) else {
                    return Ok((Vec::new(), 0));
                };
                let to = last + 7 * DAY - 1;
                let r = backend.airing_range(from, to).await?;
                let failed = missing_weeks(&weeks, r.complete_until);
                merge_weeks(vec![Ok(r.slots)]).map(|list| (list, failed))
            }
        }
    }));
    let overrides = use_signal(HashMap::<String, bool>::new);
    let mut picked = use_signal(|| None::<i64>);
    let grid = MonthGrid::new(year, month);
    let key = format!("{year}-{month:02}");
    let list = match &*schedule.read() {
        None => {
            return rsx! {
                div { class: "tk-month", "aria-busy": "true", "data-calendar-month": "{key}",
                    SkeletonLines { widths: vec![90, 70, 80, 60] }
                }
            };
        }
        Some(Err(e)) => {
            return rsx! {
                ErrorState { error: e.clone(), compact: true, on_retry: move |_| schedule.restart() }
            };
        }
        Some(Ok((list, failed))) => (with_overrides(list, &overrides.read()), *failed),
    };
    let (list, failed) = list;
    let cols = columns(&list, day_start_unix(grid.first, offset_min), grid.cells, tracked_only);
    let selected = picked()
        .filter(|d| grid.contains(*d))
        .or_else(|| grid.in_month(today).then_some(today));
    let panel = selected.map(|day| {
        let col = cols[(day - grid.first) as usize].clone();
        let empty = if tracked_only { t("schedule.empty_day_mine") } else { t("calendar.empty_day") };
        rsx! {
            section { class: "tk-month__panel", "data-selected-day": "{day}", "aria-live": "polite",
                h3 { class: "tk-month__panel-title", {day_title(day)} }
                if col.is_empty() {
                    p { class: "tk-calendar__none tk-muted", "{empty}" }
                }
                div { class: "tk-month__list",
                    for s in col {
                        SlotCard { key: "{s.media.id}-{s.episode}-{s.at_unix}", slot: s.clone(), offset_min, overrides }
                    }
                }
            }
        }
    });
    rsx! {
        if failed > 0 {
            div { class: "tk-banner tk-banner--warn tk-month__partial", role: "status", "data-failed-weeks": "{failed}",
                span { class: "tk-banner__text", {tn("schedule.month.partial", failed)} }
                div { class: "tk-banner__actions",
                    Button {
                        label: t("common.retry"),
                        variant: ButtonVariant::Text,
                        size: ButtonSize::Sm,
                        onclick: move |_| schedule.restart(),
                    }
                }
            }
        }
        div { class: "tk-month", "data-calendar-month": "{key}", "data-has-panel": "{selected.is_some()}",
            div { class: "tk-month__grid", role: "group", "aria-label": "{key}",
                for i in 0..7 {
                    span { key: "dow-{i}", class: "tk-month__dow", "aria-hidden": "true", {t(&format!("schedule.dow.{i}"))} }
                }
                for (i , col) in cols.iter().enumerate() {
                    {
                        let day = grid.day(i);
                        let (_, _, d) = civil_from_days(day);
                        let n = col.len();
                        let mine = col.iter().any(|s| s.tracked);
                        let more = n.saturating_sub(3);
                        let label = format!("{}: {}", day_title(day), tn("schedule.episodes", n));
                        let posters: Vec<(String, Option<String>)> = col
                            .iter()
                            .take(3)
                            .map(|s| (format!("{}-{}-{}", s.media.id, s.episode, s.at_unix), s.media.cover.clone()))
                            .collect();
                        rsx! {
                            button {
                                key: "{day}",
                                class: "tk-month__cell",
                                r#type: "button",
                                "data-day": "{day}",
                                "data-out": "{!grid.in_month(day)}",
                                "data-today": "{day == today}",
                                "data-count": "{n}",
                                "data-tracked": "{mine}",
                                "aria-pressed": if selected == Some(day) { "true" } else { "false" },
                                "aria-label": "{label}",
                                onclick: move |_| picked.set(Some(day)),
                                span { class: "tk-month__num tk-mono", "{d}" }
                                if n > 0 {
                                    span { class: "tk-month__stack", "aria-hidden": "true",
                                        for (k , cover) in posters {
                                            if let Some(src) = cover {
                                                img { key: "{k}", class: "tk-month__poster", src: "{src}", alt: "", loading: "lazy" }
                                            } else {
                                                span { key: "{k}", class: "tk-month__poster tk-month__poster--empty" }
                                            }
                                        }
                                        if more > 0 {
                                            span { class: "tk-month__more tk-mono", {tf("schedule.more", &[("n", &more.to_string())])} }
                                        }
                                    }
                                }
                                if mine {
                                    span { class: "tk-month__dot", title: t("schedule.mine_dot") }
                                }
                            }
                        }
                    }
                }
            }
            if let Some(panel) = panel {
                {panel}
            } else {
                p { class: "tk-month__hint tk-muted", {t("schedule.pick_day")} }
            }
        }
    }
}

/// Scrolls the time line anchor ("now", or the first show of another day) into view when it is off screen.
const TIMELINE_SCROLL_JS: &str = "const el = document.querySelector('[data-timeline-anchor=\"true\"]'); if (el) { const r = el.getBoundingClientRect(); if (r.top < 0 || r.bottom > window.innerHeight) el.scrollIntoView({ block: 'center' }); }";

/// Day view: one day (local midnight `day_start_unix`) as a vertical time line with hour marks, the shows placed by
/// time (side by side when they overlap), a "now" line and "in 22 min" / "aired 1 h ago" labels.
/// `data-calendar-day="<unix>"`.
#[component]
pub fn AiringDay(day_start_unix: i64, #[props(default)] tracked_only: bool) -> Element {
    let services = use_context::<Services>();
    let clock = use_local_clock();
    let offset = clock().map(|c| c.offset_min);
    let mut schedule = use_resource(use_reactive((&day_start_unix, &offset), {
        let backend = services.backend.clone();
        move |(day, off)| {
            let backend = backend.clone();
            async move {
                match off {
                    Some(off) => Some(backend.airing_schedule(week_monday(day + DAY / 2, off)).await),
                    None => None,
                }
            }
        }
    }));
    let overrides = use_signal(HashMap::<String, bool>::new);
    let (Some(c), Some(Some(answer))) = (clock(), schedule.read().clone()) else {
        return rsx! {
            div { class: "tk-timeline", "aria-busy": "true", SkeletonLines { widths: vec![90, 70, 80, 60] } }
        };
    };
    let list = match answer {
        Ok(list) => with_overrides(&list, &overrides.read()),
        Err(e) => {
            return rsx! {
                ErrorState { error: e, compact: true, on_retry: move |_| schedule.restart() }
            };
        }
    };
    let col = columns(&list, day_start_unix, 1, tracked_only).pop().unwrap_or_default();
    if col.is_empty() {
        let title = if tracked_only { t("schedule.empty_day_mine") } else { t("schedule.empty_day") };
        return rsx! {
            div { class: "tk-timeline", "data-calendar-day": "{day_start_unix}", "data-empty": "true",
                EmptyState { icon: "calendar".to_string(), title }
            }
        };
    }
    let now_secs = (c.now >= day_start_unix && c.now < day_start_unix + DAY).then(|| c.now - day_start_unix);
    let secs: Vec<i64> = col.iter().map(|s| s.at_unix - day_start_unix).collect();
    let (from, to) = timeline_hours(&secs, now_secs).unwrap_or((0, 24));
    let lanes = timeline_lanes(&secs);
    let height = (to - from) * TIMELINE_HOUR_PX;
    rsx! {
        div {
            class: "tk-timeline",
            "data-calendar-day": "{day_start_unix}",
            "data-calendar-week": "{week_monday(day_start_unix + DAY / 2, c.offset_min)}",
            "data-hours": "{from}-{to}",
            style: "height: {height}px",
            for h in from..to {
                div { key: "h{h}", class: "tk-timeline__hour", style: "top: {(h - from) * TIMELINE_HOUR_PX}px",
                    span { class: "tk-timeline__label tk-mono", "{h:02}:00" }
                }
            }
            div { class: "tk-timeline__track",
                for (i , (s , (lane , width))) in col.iter().cloned().zip(lanes).enumerate() {
                    {
                        let anchor = now_secs.is_none() && i == 0;
                        let top = timeline_top(secs[i], from);
                        let rel = relative_text(relative(s.at_unix, c.now));
                        rsx! {
                            div {
                                key: "{s.media.id}-{s.episode}-{s.at_unix}",
                                class: "tk-timeline__event",
                                "data-past": "{s.at_unix <= c.now}",
                                "data-timeline-anchor": "{anchor}",
                                style: "top: {top}px; left: calc({lane} * 100% / {width}); width: calc(100% / {width} - 4px)",
                                onmounted: move |_| {
                                    if anchor {
                                        document::eval(TIMELINE_SCROLL_JS);
                                    }
                                },
                                SlotCard { slot: s.clone(), offset_min: c.offset_min, overrides, rel: Some(rel) }
                            }
                        }
                    }
                }
            }
            if let Some(n) = now_secs {
                div {
                    key: "now-{day_start_unix}",
                    class: "tk-timeline__now",
                    "data-timeline-anchor": "true",
                    style: "top: {timeline_top(n, from)}px",
                    onmounted: move |_| {
                        document::eval(TIMELINE_SCROLL_JS);
                    },
                    span { class: "tk-timeline__now-label", {t("schedule.now")} }
                }
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
                // Same as Android (FIX-A2): a bell, since following means airing reminders; the bookmark is the list.
                icon: if tracked { "bell-active".to_string() } else { "bell-plus".to_string() },
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
    use crate::model::UiError;

    /// 2026-11-09 00:00 UTC, a Monday.
    const MONDAY_UTC: i64 = 1_794_182_400;

    fn slot(id: &str, at: i64, tracked: bool) -> AiringSlotVm {
        AiringSlotVm {
            media: MediaVm { format: None, status: None, year: None,
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

    #[test]
    fn month_grids_are_monday_first_whole_weeks() {
        // November 2026: the 1st is a Sunday, the 30th a Monday -> Mon 26/10 .. Sun 06/12, 6 rows.
        let g = MonthGrid::new(2026, 11);
        assert_eq!(civil_from_days(g.first), (2026, 10, 26));
        assert_eq!(g.cells, 42);
        assert_eq!(civil_from_days(g.day(g.cells - 1)), (2026, 12, 6));
        assert!(!g.in_month(g.first) && g.in_month(g.month_first) && g.in_month(g.month_last));
        assert_eq!(civil_from_days(g.month_last), (2026, 11, 30));
        // February 2027 starts on Monday and has 28 days: exactly 4 rows.
        let f = MonthGrid::new(2027, 2);
        assert_eq!(f.first, f.month_first);
        assert_eq!(f.cells, 28);
        // 2024 leap year.
        assert_eq!(month_len(2024, 2), 29);
        assert_eq!(month_len(2026, 12), 31);
        assert_eq!(days_from_civil(2026, 11, 9), MONDAY_UTC / DAY);
        for d in [-800, -1, 0, 19_782, 20_000, 30_000] {
            let (y, m, dd) = civil_from_days(d);
            assert_eq!(days_from_civil(y, m, dd), d);
        }
    }

    #[test]
    fn missing_weeks_follow_the_complete_point() {
        let weeks = [0, 7 * DAY, 14 * DAY];
        assert_eq!(missing_weeks(&weeks, 21 * DAY - 1), 0);
        // Stopped inside the second week: it and the third are missing.
        assert_eq!(missing_weeks(&weeks, 9 * DAY), 2);
        assert_eq!(missing_weeks(&weeks, -1), 3);
    }

    #[test]
    fn month_navigation_wraps_years() {
        assert_eq!(add_months(2026, 11, 1), (2026, 12));
        assert_eq!(add_months(2026, 12, 1), (2027, 1));
        assert_eq!(add_months(2026, 1, -1), (2025, 12));
        assert_eq!(add_months(2026, 11, -23), (2024, 12));
    }

    #[test]
    fn weeks_overlapping_a_month_are_its_rows() {
        let g = MonthGrid::new(2026, 11);
        let w = g.weeks(0);
        assert_eq!(w.len(), 6);
        assert_eq!(w[0], day_start_unix(g.first, 0));
        assert_eq!(w[2], MONDAY_UTC);
        assert!(w.windows(2).all(|p| p[1] - p[0] == 7 * DAY));
        // Each is a local Monday midnight, also away from UTC.
        for off in [180, -540] {
            for s in g.weeks(off) {
                assert_eq!(local_weekday(s, off), 0);
                assert_eq!(local_midnight(s, off), s);
            }
        }
    }

    #[test]
    fn merged_weeks_dedupe_and_survive_a_failed_week() {
        let a = slot("a", MONDAY_UTC + 10, false);
        let b = slot("b", MONDAY_UTC + 5, true);
        let merged = merge_weeks(vec![
            Ok(vec![a.clone(), b.clone()]),
            Err(UiError::not_wired("x")),
            Ok(vec![a.clone()]),
        ])
        .unwrap();
        assert_eq!(merged.iter().map(|s| s.media.id.as_str()).collect::<Vec<_>>(), ["b", "a"]);
        assert!(merge_weeks(vec![Err(UiError::not_wired("x")), Err(UiError::not_wired("y"))]).is_err());
        assert_eq!(merge_weeks(Vec::new()).unwrap(), Vec::new());
    }

    #[test]
    fn join_all_keeps_the_order() {
        let fut = join_all((1..=3).map(std::future::ready).collect());
        let mut fut = std::pin::pin!(fut);
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        assert_eq!(fut.as_mut().poll(&mut cx), Poll::Ready(vec![1, 2, 3]));
    }

    #[test]
    fn timeline_range_and_positions() {
        let h = 3600;
        // Shows at 10:00 and 13:30, now 12:05: 09:00 .. 15:00.
        assert_eq!(timeline_hours(&[10 * h, 13 * h + 1800], Some(12 * h + 300)), Some((9, 15)));
        // Clamped to the day.
        assert_eq!(timeline_hours(&[0, 23 * h + 59 * 60], None), Some((0, 24)));
        assert_eq!(timeline_hours(&[], None), None);
        assert_eq!(timeline_hours(&[], Some(8 * h)), Some((7, 10)));
        assert_eq!(timeline_top(10 * h + 1800, 9), TIMELINE_HOUR_PX * 3 / 2);
        assert_eq!(timeline_top(9 * h, 9), 0);
    }

    #[test]
    fn overlapping_shows_share_the_row() {
        let h = 3600;
        // 10:00 and 10:15 overlap (an event is 50 min tall), 10:55 overlaps 10:15 only (reuses lane 0), 12:00 alone.
        let lanes = timeline_lanes(&[10 * h, 10 * h + 900, 10 * h + 3300, 12 * h]);
        assert_eq!(lanes, vec![(0, 2), (1, 2), (0, 2), (0, 1)]);
        assert_eq!(timeline_lanes(&[5 * h, 5 * h, 5 * h]), vec![(0, 3), (1, 3), (2, 3)]);
        assert!(timeline_lanes(&[]).is_empty());
    }

    #[test]
    fn relative_times() {
        let now = MONDAY_UTC;
        assert_eq!(relative(now + 30, now), Relative::Now);
        assert_eq!(relative(now + 22 * 60, now), Relative::In { h: 0, m: 22 });
        assert_eq!(relative(now + 90, now), Relative::In { h: 0, m: 2 });
        assert_eq!(relative(now + 112 * 60, now), Relative::In { h: 1, m: 52 });
        assert_eq!(relative(now - 61 * 60, now), Relative::Ago { h: 1, m: 1 });
        assert_eq!(relative(now - 5 * 60 - 30, now), Relative::Ago { h: 0, m: 5 });
        // English catalog (tests never switch the global language).
        assert_eq!(relative_text(Relative::In { h: 0, m: 22 }), "in 22 min");
        assert_eq!(relative_text(Relative::In { h: 1, m: 52 }), "in 1 h 52 min");
        assert_eq!(relative_text(Relative::In { h: 2, m: 0 }), "in 2 h");
        assert_eq!(relative_text(Relative::Ago { h: 1, m: 10 }), "aired 1 h ago");
        assert_eq!(relative_text(Relative::Ago { h: 0, m: 5 }), "aired 5 min ago");
        assert_eq!(relative_text(Relative::Now), "airing now");
    }
}
