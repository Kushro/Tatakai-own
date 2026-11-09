//! AniList reviews and the user's AniList activity for a title (wave B, worker **n**; plan 14 section 0.1 #10:
//! social is AniList-only, there is no own comments backend). Mount point left by the wave-A contract and mounted
//! by worker c in `screens/details.rs` under the tab panel (checklist `details.reviews`, formerly the comments
//! mount). Renders nothing until wave B. Props are final.
//!
//! Wave B body: `Backend::anilist_reviews(media_id, page)` (AniList `Page(reviews(mediaId))`: summary, score,
//! author, rating) and, with an AniList account connected (worker g's `list_accounts`), the user's own activity
//! on the title (`Activity(mediaId, userId)`) with a "post status" field. Everything read-only without an account.

use dioxus::prelude::*;

#[component]
pub fn Reviews(
    /// `MediaVm::id` of the title.
    media_id: String,
) -> Element {
    let _ = media_id;
    rsx! {}
}
