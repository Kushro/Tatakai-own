//! Comments of a title / episode (wave B, worker **n**; docs/redesign/14-parity-plan.md section 2, Supabase).
//! Mount point left by the wave-A contract and mounted by worker c in `screens/details.rs` (title scope) and by
//! the player owner later (episode scope). Renders nothing until wave B. Props are final.

use dioxus::prelude::*;

#[component]
pub fn Comments(
    /// `MediaVm::id` of the title.
    media_id: String,
    /// `None` = comments about the whole title.
    #[props(default)] episode: Option<u32>,
) -> Element {
    let _ = (media_id, episode);
    rsx! {}
}
