//! Airing calendar page `/calendar` (wave B, worker **b**; checklist `home.calendar.schedule`, `home.calendar.my-shows`,
//! `home.calendar.reminders`; classic `pages/base/CalendarPage.tsx`): week grid by local day (`components::AiringCalendar`
//! with `compact: false`), "My shows" filter, follow / unfollow, previous / next week; the reminders snackbar comes from
//! `Backend::reminders_due` polled by Home (b). `data-calendar-week="<unix>"`, `data-tracked` per slot. Contract placeholder:
//! b replaces the body.

use dioxus::prelude::*;

use crate::components::EmptyState;
use crate::i18n::t;

#[component]
pub fn CalendarPage() -> Element {
    rsx! {
        div { class: "tk-page", "data-page": "calendar.title",
            div { class: "tk-page__head",
                h1 { class: "tk-page__title", {t("calendar.title")} }
            }
            EmptyState { icon: "calendar".to_string(), title: t("calendar.title"), body: t("common.placeholder") }
        }
    }
}
