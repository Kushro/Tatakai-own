//! "Add to list" button of the details page (wave A, worker **g**; docs/redesign/15-parity-wave-a-contract.md
//! section 4). Mounted by worker c's `screens/details.rs` with these exact props; g owns the body.
//!
//! Behaviour to implement: loads `Backend::list_entry(media_id)` (like `SourceSummary` loads its own data), shows
//! the current status (`lists.status.<id>`) or "Add to list", opens a menu with the six `ListStatusVm` statuses,
//! progress +/- and score, writes with `Backend::set_list_entry` / `remove_list_entry` (offline queue), and shows
//! a "Connect AniList / MAL" hint (Settings > Accounts) when no service is connected. `data-list-status` on the
//! root element carries the current status id (or `none`) for the verifier.

use dioxus::prelude::*;

use crate::components::{Button, ButtonVariant, IconButton};
use crate::i18n::t;

#[component]
pub fn ListButton(
    /// `MediaVm::id` of the title.
    media_id: String,
    #[props(default)] episodes_total: Option<u32>,
    /// Icon-only variant for the floating toolbar.
    #[props(default)] compact: bool,
) -> Element {
    let _ = (media_id, episodes_total);
    // Contract stub: no backend call yet; the owner replaces the whole body.
    if compact {
        return rsx! {
            span { "data-list-status": "none",
                IconButton { icon: "bookmark".to_string(), label: t("details.add_list"), disabled: true }
            }
        };
    }
    rsx! {
        span { "data-list-status": "none",
            Button { label: t("details.add_list"), icon: "bookmark".to_string(), variant: ButtonVariant::Tonal, disabled: true }
        }
    }
}
