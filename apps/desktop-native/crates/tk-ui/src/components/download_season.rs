//! "Download season" sheet (wave B, worker **h**; docs/redesign/14-parity-plan.md section 2). Mount point left by
//! the wave-A contract and mounted by worker c in `screens/details.rs`; wave B replaces the body (pick episodes,
//! language / server, auto-download new episodes). Props are final.

use dioxus::prelude::*;

use crate::components::{EmptyState, Sheet};
use crate::i18n::t;

#[component]
pub fn DownloadSeasonSheet(
    open: bool,
    /// `MediaVm::id` of the title.
    media_id: String,
    #[props(default)] episodes_total: Option<u32>,
    #[props(default)] on_close: EventHandler<()>,
) -> Element {
    let _ = (media_id, episodes_total);
    if !open {
        return rsx! {};
    }
    rsx! {
        Sheet { title: t("details.download"), on_close: move |_| on_close.call(()),
            EmptyState { icon: "download".to_string(), title: t("details.download"), body: t("common.placeholder") }
        }
    }
}
