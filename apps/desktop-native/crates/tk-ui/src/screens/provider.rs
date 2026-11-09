//! Custom provider page `/provider/:ext_id` (wave B, worker **j**; checklist `ext.custom-providers.home`,
//! `ext.custom-providers.list`, `ext.custom-providers.bookmark`; classic `pages/custom/CustomHomePage.tsx`,
//! `CustomProvidersPage.tsx`): rows of `Backend::provider_home(ext_id)` ("See all" -> `provider_feed`), the provider's
//! search (`search_in_extension`), bookmarks (`provider_bookmarks`). Cards open `Route::DetailsPage` with the
//! `ext:<ext_id>:<key>` id (`Backend::provider_item` answers `details` / `episodes` / `playback` for those ids, see
//! `live/mod.rs`). `data-provider="<ext_id>"`. Contract placeholder: j replaces the body.

use dioxus::prelude::*;

use crate::components::EmptyState;
use crate::i18n::t;

#[component]
pub fn ProviderPage(ext_id: String) -> Element {
    let _ = (ext_id,);
    rsx! {
        div { class: "tk-page", "data-page": "provider_page.title",
            div { class: "tk-page__head",
                h1 { class: "tk-page__title", {t("provider_page.title")} }
            }
            EmptyState { icon: "extensions".to_string(), title: t("provider_page.title"), body: t("common.placeholder") }
        }
    }
}
