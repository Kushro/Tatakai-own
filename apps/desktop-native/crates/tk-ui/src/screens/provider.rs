//! Custom provider page `/provider/:ext_id` (wave B, worker **j**; checklist `ext.custom-providers.home`,
//! `ext.custom-providers.list`, `ext.custom-providers.bookmark`; classic `pages/custom/CustomHomePage.tsx`,
//! `CustomProvidersPage.tsx`, `CustomInfoPage.tsx`): rows of `Backend::provider_home(ext_id)` ("See all" ->
//! `provider_feed`), the provider's search (`search_in_extension`), and the saved titles of this provider
//! (`provider_bookmarks`). Cards open `Route::DetailsPage` with the `ext:<ext_id>:<key>` id (`Backend::provider_item`
//! answers `details` / `episodes` / `playback` for those ids through `live/mod.rs`), or the manga / novel details for
//! reading providers. `data-provider="<ext_id>"`.
//!
//! [`ProviderBookmarkButton`] ("Save" / "Saved" + "Mark as completed", `data-bookmarked`) is mounted by the Details
//! page for `ext:` ids (integrator, `details.rs`) and on this page's saved row.

use dioxus::prelude::*;

use crate::app::{Route, Services};
use crate::backend::{CustomProviderVm, MediaVm, SectionVm};
use crate::components::{
    Button, ButtonSize, ButtonVariant, EmptyState, ErrorState, LoadingIndicator, MediaCard, SkeletonRow,
    TextField, use_snackbar,
};
use crate::i18n::{t, tf};
use crate::model::ext::{provider_id_parts, provider_row_title};
use crate::model::UiError;
use crate::screens::extensions::provider_route;

/// The kind cards of this extension open with: the only kind its providers have (`anime` when mixed or unknown).
pub fn provider_kind(providers: &[CustomProviderVm]) -> Option<String> {
    let first = providers.first()?.kind.clone();
    providers.iter().all(|p| p.kind == first).then_some(first)
}

#[component]
pub fn ProviderPage(ext_id: String) -> Element {
    // Keyed so another provider's page starts fresh.
    rsx! {
        ProviderBody { key: "{ext_id}", ext_id: ext_id.clone() }
    }
}

#[component]
fn ProviderBody(ext_id: String) -> Element {
    let services = use_context::<Services>();
    let navigator = use_navigator();
    let backend = services.backend.clone();
    let mut providers = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.custom_providers().await }
        }
    });
    let mut home = use_resource({
        let (backend, id) = (backend.clone(), ext_id.clone());
        move || {
            let (backend, id) = (backend.clone(), id.clone());
            async move { backend.provider_home(&id).await }
        }
    });
    let mut bookmarks = use_resource({
        let backend = backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.provider_bookmarks().await }
        }
    });
    let mut query = use_signal(String::new);
    let mut results = use_signal(|| None::<Result<Vec<MediaVm>, UiError>>);
    let mut searching = use_signal(|| false);

    let mine: Vec<CustomProviderVm> = match &*providers.read() {
        Some(Ok(list)) => list.iter().filter(|p| p.ext_id == ext_id).cloned().collect(),
        _ => Vec::new(),
    };
    let ext_name = mine.first().map(|p| p.ext_name.clone()).unwrap_or_else(|| ext_id.clone());
    let kind = provider_kind(&mine);
    let can_search = mine.iter().any(|p| p.has_search);

    let search = use_callback({
        let (backend, id) = (backend.clone(), ext_id.clone());
        move |_: ()| {
            let q = query.peek().trim().to_owned();
            if q.is_empty() {
                results.set(None);
                return;
            }
            let (backend, id) = (backend.clone(), id.clone());
            searching.set(true);
            spawn(async move {
                results.set(Some(backend.search_in_extension(&id, &q, 1).await));
                searching.set(false);
            });
        }
    });

    let rows = match &*home.read() {
        None => rsx! {
            SkeletonRow { count: 6 }
            SkeletonRow { count: 6 }
        },
        Some(Err(e)) => rsx! { ErrorState { error: e.clone(), on_retry: move |_| home.restart() } },
        Some(Ok(list)) if list.is_empty() => rsx! {
            EmptyState { icon: "extensions".to_string(), title: t("provider_page.empty.title"), body: t("provider_page.empty.body") }
        },
        Some(Ok(list)) => rsx! {
            for s in list.clone() {
                ProviderRow {
                    key: "{s.title_key}",
                    ext_id: ext_id.clone(),
                    ext_name: ext_name.clone(),
                    kind: kind.clone(),
                    section: s.clone(),
                    on_retry: move |_| home.restart(),
                }
            }
        },
    };

    let search_view = match results() {
        None => rsx! {},
        Some(Err(e)) => rsx! { ErrorState { error: e, compact: true, on_retry: move |_| search.call(()) } },
        Some(Ok(items)) if items.is_empty() => rsx! {
            EmptyState { icon: "search".to_string(), title: t("provider_page.search.none") }
        },
        Some(Ok(items)) => rsx! {
            div { class: "tk-grid", "data-results": "true", "data-origin": "ext:{ext_id}",
                for m in items {
                    {
                        let (id, k) = (m.id.clone(), kind.clone());
                        rsx! {
                            MediaCard {
                                key: "{m.id}",
                                media: m.clone(),
                                onclick: move |_| {
                                    navigator.push(provider_route(&id, k.as_deref()));
                                },
                            }
                        }
                    }
                }
            }
        },
    };

    let prefix = format!("ext:{ext_id}:");
    let saved: Vec<MediaVm> = match &*bookmarks.read() {
        Some(Ok(list)) => list.iter().filter(|m| m.id.starts_with(&prefix)).cloned().collect(),
        _ => Vec::new(),
    };
    let providers_err = match &*providers.read() {
        Some(Err(e)) => Some(e.clone()),
        _ => None,
    };
    let saved_failed = match &*bookmarks.read() {
        Some(Err(e)) => Some(e.clone()),
        _ => None,
    };

    rsx! {
        div { class: "tk-page tk-provider", "data-page": "provider_page.title", "data-provider": "{ext_id}",
            div { class: "tk-page__head",
                div {
                    h1 { class: "tk-page__title", "{ext_name}" }
                    p { class: "tk-page__intro tk-muted", {tf("provider_page.intro", &[("name", &ext_name)])} }
                }
                Button {
                    label: t("provider_page.back"),
                    icon: "back".to_string(),
                    variant: ButtonVariant::Text,
                    onclick: move |_| {
                        navigator.push(Route::ExtensionsPage {});
                    },
                }
            }
            if let Some(e) = providers_err {
                ErrorState { error: e, compact: true, on_retry: move |_| providers.restart() }
            }
            if can_search {
                section { class: "tk-panel tk-provider__search",
                    div { class: "tk-install__path",
                        TextField {
                            label: tf("provider_page.search", &[("name", &ext_name)]),
                            value: query(),
                            icon: "search".to_string(),
                            oninput: move |v: String| query.set(v),
                            onsubmit: move |_| search.call(()),
                        }
                        Button {
                            label: t("provider_page.search.go"),
                            variant: ButtonVariant::Tonal,
                            disabled: searching() || query().trim().is_empty(),
                            onclick: move |_| search.call(()),
                        }
                    }
                    if searching() {
                        LoadingIndicator { small: true }
                    } else {
                        {search_view}
                    }
                }
            }
            if !saved.is_empty() {
                section { class: "tk-section", "data-provider-bookmarks": "true",
                    div { class: "tk-section__head",
                        h2 { class: "tk-section__title", {t("provider_page.saved")} }
                    }
                    div { class: "tk-row",
                        for m in saved {
                            {
                                let (id, k) = (m.id.clone(), kind.clone());
                                rsx! {
                                    MediaCard {
                                        key: "{m.id}",
                                        media: m.clone(),
                                        onclick: move |_| {
                                            navigator.push(provider_route(&id, k.as_deref()));
                                        },
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if let Some(e) = saved_failed {
                ErrorState { error: e, compact: true, on_retry: move |_| bookmarks.restart() }
            }
            {rows}
        }
    }
}

/// One provider row; "See all" loads the whole row (`provider_feed`) as a grid.
#[component]
fn ProviderRow(
    ext_id: String,
    ext_name: String,
    kind: Option<String>,
    section: SectionVm,
    #[props(default)] on_retry: EventHandler<()>,
) -> Element {
    let services = use_context::<Services>();
    let navigator = use_navigator();
    let title = provider_row_title(&section.title_key, &ext_name);
    let mut expanded = use_signal(|| None::<Result<Vec<MediaVm>, UiError>>);
    let mut loading = use_signal(|| false);

    let see_all = {
        let (backend, id, row) = (services.backend.clone(), ext_id.clone(), title.clone());
        move |_: MouseEvent| {
            if expanded.peek().is_some() {
                expanded.set(None);
                return;
            }
            let (backend, id, row) = (backend.clone(), id.clone(), row.clone());
            loading.set(true);
            spawn(async move {
                expanded.set(Some(backend.provider_feed(&id, &row, 1).await.map(|p| p.items)));
                loading.set(false);
            });
        }
    };

    let items = match (&section.items, expanded()) {
        (_, Some(Ok(all))) => Ok((all, true)),
        (_, Some(Err(e))) => Err(e),
        (Ok(list), None) => Ok((list.clone(), false)),
        (Err(e), None) => Err(e.clone()),
    };
    let see_all_label = if expanded().is_some() { t("provider_page.see_less") } else { t("common.see_all") };
    let body = match items {
        Err(e) => rsx! { ErrorState { error: e, compact: true, on_retry: move |_| on_retry.call(()) } },
        Ok((list, _)) if list.is_empty() => rsx! { p { class: "tk-section__empty tk-muted", {t("provider_page.row_empty")} } },
        Ok((list, grid)) => rsx! {
            div { class: "{row_class(grid)}",
                for m in list {
                    {
                        let (id, k) = (m.id.clone(), kind.clone());
                        rsx! {
                            MediaCard {
                                key: "{m.id}",
                                media: m.clone(),
                                onclick: move |_| {
                                    navigator.push(provider_route(&id, k.as_deref()));
                                },
                            }
                        }
                    }
                }
            }
        },
    };
    let has_items = section.items.as_ref().is_ok_and(|v| !v.is_empty());

    rsx! {
        section { class: "tk-section", "data-row": "{title}",
            div { class: "tk-section__head",
                h2 { class: "tk-section__title", "{title}" }
                if has_items {
                    if loading() {
                        LoadingIndicator { small: true }
                    } else {
                        Button { label: see_all_label, variant: ButtonVariant::Text, size: ButtonSize::Sm, onclick: see_all }
                    }
                }
            }
            {body}
        }
    }
}

fn row_class(grid: bool) -> &'static str {
    if grid { "tk-grid" } else { "tk-row" }
}

/// "Save" / "Saved" for a provider title plus "Mark as completed" once saved (`ext.custom-providers.bookmark`).
/// Renders nothing for ids that are not provider titles. Completing writes the last episode as watched, which also
/// marks the bookmark completed (tk-app `ext_catalog::record_progress`).
#[component]
pub fn ProviderBookmarkButton(media_id: String) -> Element {
    let services = use_context::<Services>();
    let snackbar = use_snackbar();
    let parts = provider_id_parts(&media_id);
    let mut state = use_resource({
        let (backend, id) = (services.backend.clone(), media_id.clone());
        move || {
            let (backend, id) = (backend.clone(), id.clone());
            async move { backend.provider_bookmarks().await.map(|list| list.into_iter().find(|m| m.id == id)) }
        }
    });
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<UiError>);
    let Some((ext_id, _source, item_key)) = parts else { return rsx! {} };

    let saved = match &*state.read() {
        Some(Ok(found)) => Some(found.clone()),
        _ => None,
    };
    let loading = state.read().is_none();
    let bookmarked = saved.as_ref().is_some_and(|m| m.is_some());
    let completed = saved.as_ref().and_then(|m| m.as_ref()).is_some_and(|m| m.subtitle.starts_with('✓'));

    let toggle = {
        let (backend, ext_id, item_key) = (services.backend.clone(), ext_id.clone(), item_key.clone());
        move |_: MouseEvent| {
            let (backend, ext_id, item_key) = (backend.clone(), ext_id.clone(), item_key.clone());
            let next = !bookmarked;
            busy.set(true);
            error.set(None);
            spawn(async move {
                match backend.set_provider_bookmark(&ext_id, &item_key, next).await {
                    Ok(()) => snackbar.show(if next { t("provider_page.bookmark.saved") } else { t("provider_page.bookmark.removed") }),
                    Err(e) => error.set(Some(e)),
                }
                busy.set(false);
                state.restart();
            });
        }
    };
    let complete = {
        let (backend, id) = (services.backend.clone(), media_id.clone());
        move |_: MouseEvent| {
            let (backend, id) = (backend.clone(), id.clone());
            busy.set(true);
            error.set(None);
            spawn(async move {
                let r = match backend.episodes(&id).await {
                    Ok(eps) => match eps.iter().map(|e| e.number).max() {
                        Some(last) => backend.record_progress(&id, last, 100, Some(100)).await,
                        None => Ok(()),
                    },
                    Err(e) => Err(e),
                };
                match r {
                    Ok(()) => snackbar.show(t("provider_page.bookmark.completed")),
                    Err(e) => error.set(Some(e)),
                }
                busy.set(false);
                state.restart();
            });
        }
    };

    let label = if bookmarked { t("provider_page.bookmark.saved_state") } else { t("provider_page.bookmark.save") };
    rsx! {
        div { class: "tk-provider__bookmark", "data-bookmarked": "{bookmarked}", "data-completed": "{completed}",
            Button {
                label,
                icon: "bookmark".to_string(),
                variant: if bookmarked { ButtonVariant::Tonal } else { ButtonVariant::Outlined },
                size: ButtonSize::Sm,
                disabled: busy() || loading,
                onclick: toggle,
            }
            if bookmarked && !completed {
                Button {
                    label: t("provider_page.bookmark.complete"),
                    icon: "check".to_string(),
                    variant: ButtonVariant::Text,
                    size: ButtonSize::Sm,
                    disabled: busy(),
                    onclick: complete,
                }
            }
            if let Some(e) = error() {
                ErrorState { error: e, compact: true, retry: false }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(kind: &str) -> CustomProviderVm {
        CustomProviderVm { ext_id: "e".into(), ext_name: "E".into(), name: kind.into(), kind: kind.into(), has_home: true, has_search: true }
    }

    #[test]
    fn provider_kind_is_the_only_kind() {
        assert_eq!(provider_kind(&[p("manga"), p("manga")]).as_deref(), Some("manga"));
        assert_eq!(provider_kind(&[p("manga"), p("anime")]), None);
        assert_eq!(provider_kind(&[]), None);
    }
}
