//! "For you" page `/recommendations` (wave B, worker **b**; checklist `home.recs.for-you`, `home.recs.explanations`,
//! `home.recs.feedback`, `home.recs.atlas`; classic `RecommendationsPage.tsx`, `atlas/AnimeAtlasPanel.tsx`): cards from
//! `Backend::recommendations(page)` with the affinity percent (`data-score`) and reasons, feedback menu
//! (`recs_feedback`, `data-feedback`), the taste profile summary (`taste_profile`) and the Atlas tab (`atlas_points`,
//! an SVG scatter with tokens only; `data-atlas-points`). Contract placeholder: b replaces the body.

use dioxus::prelude::*;

use crate::components::EmptyState;
use crate::i18n::t;

#[component]
pub fn RecommendationsPage() -> Element {
    rsx! {
        div { class: "tk-page", "data-page": "recs.title",
            div { class: "tk-page__head",
                h1 { class: "tk-page__title", {t("recs.title")} }
            }
            EmptyState { icon: "star".to_string(), title: t("recs.title"), body: t("common.placeholder") }
        }
    }
}
