//! Airing calendar (wave B, worker **b**; checklist `home.calendar.schedule`, `home.calendar.my-shows`,
//! `home.calendar.reminders`; classic `CalendarPage.tsx`, `TrackShowButton.tsx`). One component for the block Home
//! and Library embed (`compact: true`: today + tomorrow) and the week grid of `/calendar` (`compact: false`):
//! `Backend::airing_schedule(week_start_unix)`, local-time columns, tracked shows highlighted (`data-tracked`),
//! "Follow" / "Unfollow" with `set_tracked`. Props are final; b replaces the body. Renders nothing until then.

use dioxus::prelude::*;

#[component]
pub fn AiringCalendar(
    /// Local midnight (unix seconds) of the first day shown.
    week_start_unix: i64,
    #[props(default)] compact: bool,
    /// Only the user's tracked shows (`home.calendar.my-shows` filter).
    #[props(default)] tracked_only: bool,
) -> Element {
    let _ = (week_start_unix, compact, tracked_only);
    rsx! {}
}
