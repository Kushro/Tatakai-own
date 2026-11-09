//! AniList reviews and the user's AniList activity for a title (wave B, worker **n**; checklist `details.reviews`;
//! classic `src/components/anime/RatingsSection.tsx`; plan 14 section 0.1 #10: social is AniList-only, there is no
//! own comments backend). Mounted by `screens/details.rs` under the tab panels. Props are final.
//!
//! * AniList's average score of the title (the AniList-backed `details` call, `data-anilist-score`);
//! * reviews (`Backend::anilist_reviews(media_id, page)`: author, score, summary, helpful votes), "Read more" loads
//!   the full body (`anilist_review`), paginated; `data-reviews`, `data-review-id`, `data-expanded`;
//! * "Write a review" opens the title on anilist.co (`open_url`; no editor of our own);
//! * with an AniList account, the viewer's own activity on the title (`anilist_activity(Media(id))`); without one,
//!   an invitation to connect (`data-social="disconnected"`).
//!
//! `Settings::social.{show_reviews_on_details, show_activity_on_details}` switch each half off. Rating the title
//! itself is the list editor of worker g (`lists.write.score`).

use dioxus::prelude::*;

use crate::app::{Services, Ui};
use crate::backend::{ActivityFeedVm, ListServiceVm, ReviewVm};
use crate::components::{
    Button, ButtonSize, ButtonVariant, EmptyState, ErrorState, IconButton, IconButtonSize, SkeletonLines,
    format_score, use_snackbar,
};
use crate::i18n::{t, tf};
use crate::model::UiError;
use crate::platform::now_unix;
use crate::screens::social::activity::ActivityList;
use crate::screens::social::{Avatar, Disconnected, paragraphs, rel_time};

/// AniList page size of `Page.reviews` (`tk_core::metadata::anilist::social::REVIEWS_PER_PAGE`): a shorter page is
/// the last one.
pub const REVIEWS_PAGE_SIZE: usize = 10;

/// Percent-encode a search term for an URL query (unreserved characters stay as they are).
pub fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Where "Write a review" leads: the AniList page of the title (its Reviews tab has AniList's editor). AniList ids
/// are the plain numeric UI ids; anything else (MAL / Kitsu ids) goes through AniList's search by title.
pub fn write_review_url(media_id: &str, title: Option<&str>) -> String {
    let id = media_id.trim();
    if !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) {
        return format!("https://anilist.co/anime/{id}");
    }
    match title.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => format!("https://anilist.co/search/anime?search={}", url_encode(t)),
        None => "https://anilist.co/search/anime".to_owned(),
    }
}

/// `items` + the next page without duplicates.
pub fn merge_reviews(items: &mut Vec<ReviewVm>, page: Vec<ReviewVm>) {
    for r in page {
        if !items.iter().any(|x| x.id == r.id) {
            items.push(r);
        }
    }
}

/// Average of the reviewers' scores (0..=100), when the title has no AniList score of its own.
pub fn reviewers_average(items: &[ReviewVm]) -> Option<u32> {
    let scores: Vec<u32> = items.iter().filter_map(|r| r.score).collect();
    if scores.is_empty() { None } else { Some(scores.iter().sum::<u32>() / scores.len() as u32) }
}

#[component]
pub fn Reviews(
    /// `MediaVm::id` of the title.
    media_id: String,
) -> Element {
    let ui = use_context::<Ui>();
    let services = use_context::<Services>();
    let prefs = ui.settings.read().social.clone();
    // AniList account connected (worker g's accounts); `None` while loading.
    let connected = use_resource({
        let backend = services.backend.clone();
        move || {
            let backend = backend.clone();
            async move {
                backend
                    .list_accounts()
                    .await
                    .map(|list| list.iter().any(|a| a.service == ListServiceVm::AniList && a.connected))
                    .unwrap_or(false)
            }
        }
    });
    if !prefs.show_reviews_on_details && !prefs.show_activity_on_details {
        return rsx! {};
    }
    let connected = *connected.read();
    rsx! {
        section { class: "tk-panel tk-social-reviews", "data-social-reviews": "{media_id}",
            if prefs.show_reviews_on_details {
                ReviewsBlock { key: "{media_id}", media_id: media_id.clone() }
            }
            match connected {
                None => rsx! {},
                Some(false) => rsx! {
                    Disconnected { compact: true }
                },
                Some(true) if prefs.show_activity_on_details => rsx! {
                    div { class: "tk-social-reviews__activity", "data-social": "connected",
                        h3 { class: "tk-panel__title", {t("reviews.activity.title")} }
                        ActivityList { key: "{media_id}", feed: ActivityFeedVm::Media(media_id.clone()), compact: true }
                    }
                },
                Some(true) => rsx! {},
            }
        }
    }
}

#[component]
fn ReviewsBlock(media_id: String) -> Element {
    let services = use_context::<Services>();
    let snack = use_snackbar();
    let mut items = use_signal(Vec::<ReviewVm>::new);
    let mut page = use_signal(|| 0_u32);
    let mut has_next = use_signal(|| false);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<UiError>);
    let mut loaded = use_signal(|| false);
    // AniList's average score and the title (for the "Write a review" search fallback).
    let details = use_resource({
        let backend = services.backend.clone();
        let id = media_id.clone();
        move || {
            let (backend, id) = (backend.clone(), id.clone());
            async move { backend.details(&id).await.ok().map(|d| (d.media.score, d.media.title)) }
        }
    });

    let load = use_callback({
        let backend = services.backend.clone();
        let id = media_id.clone();
        move |next: u32| {
            if *busy.peek() {
                return;
            }
            let (backend, id) = (backend.clone(), id.clone());
            busy.set(true);
            error.set(None);
            spawn(async move {
                match backend.anilist_reviews(&id, next).await {
                    Ok(list) => {
                        has_next.set(list.len() >= REVIEWS_PAGE_SIZE);
                        merge_reviews(&mut items.write(), list);
                        page.set(next);
                    }
                    Err(e) => error.set(Some(e)),
                }
                loaded.set(true);
                busy.set(false);
            });
        }
    });
    use_hook(move || load.call(1));

    let info = details.read().clone().flatten();
    let list = items.read().clone();
    let score = info.as_ref().and_then(|(s, _)| *s).or_else(|| reviewers_average(&list));
    let title = info.as_ref().map(|(_, t)| t.clone());
    let write_url = write_review_url(&media_id, title.as_deref());
    let write = {
        let backend = services.backend.clone();
        move |_| {
            let (backend, url) = (backend.clone(), write_url.clone());
            spawn(async move {
                if let Err(e) = backend.open_url(&url).await {
                    tracing::warn!(detail = %e.detail, "opening AniList failed");
                    snack.show(t("social.open_failed"));
                }
            });
        }
    };
    let err = error.read().clone();
    let score_attr = score.map(|s| s.to_string()).unwrap_or_default();

    rsx! {
        div { class: "tk-social-reviews__block", "data-reviews": "{list.len()}", "data-anilist-score": "{score_attr}",
            div { class: "tk-social-reviews__head",
                h3 { class: "tk-panel__title", {t("reviews.title")} }
                if let Some(s) = score {
                    span { class: "tk-social-reviews__score", "aria-label": "{t(\"reviews.average\")}",
                        b { {format_score(s)} }
                        span { class: "tk-muted", {t("reviews.average")} }
                    }
                }
                span { class: "tk-social-card__spacer" }
                Button { label: t("reviews.write"), icon: "edit".to_string(), variant: ButtonVariant::Text, size: ButtonSize::Sm, onclick: write }
            }
            if !loaded() && list.is_empty() {
                div { "aria-busy": "true", "data-loading": "true",
                    SkeletonLines { widths: vec![80, 65, 72] }
                }
            } else if let (true, Some(e)) = (list.is_empty(), err.clone()) {
                ErrorState { error: e, compact: true, on_retry: move |_| load.call(1) }
            } else if list.is_empty() {
                EmptyState { icon: "star".to_string(), title: t("reviews.empty.title"), body: t("reviews.empty.body") }
            } else {
                ul { class: "tk-social-reviews__list",
                    for r in list.iter().cloned() {
                        ReviewCard { key: "{r.id}", review: r.clone() }
                    }
                }
                if let Some(e) = err {
                    ErrorState { error: e, compact: true, on_retry: move |_| load.call(page() + 1) }
                } else if has_next() {
                    div { class: "tk-social__more",
                        Button {
                            label: if busy() { t("social.loading") } else { t("reviews.more") },
                            variant: ButtonVariant::Tonal,
                            size: ButtonSize::Sm,
                            disabled: busy(),
                            onclick: move |_| load.call(page() + 1),
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn ReviewCard(review: ReviewVm) -> Element {
    let services = use_context::<Services>();
    let snack = use_snackbar();
    let navigator = use_navigator();
    let r = review;
    let mut expanded = use_signal(|| false);
    let mut body = use_signal(|| r.body.clone());
    let mut loading = use_signal(|| false);
    let mut failed = use_signal(|| None::<UiError>);
    let id = r.id;

    let fetch = use_callback({
        let backend = services.backend.clone();
        move |()| {
            if *loading.peek() {
                return;
            }
            let backend = backend.clone();
            loading.set(true);
            failed.set(None);
            spawn(async move {
                match backend.anilist_review(id).await {
                    Ok(full) => body.set(full.body.or(Some(full.summary))),
                    Err(e) => failed.set(Some(e)),
                }
                loading.set(false);
            });
        }
    });
    let toggle = move |_| {
        let next = !expanded();
        expanded.set(next);
        if next && body.peek().is_none() {
            fetch.call(());
        }
    };
    let open = {
        let backend = services.backend.clone();
        let url = r.url.clone();
        move |_| {
            let (backend, url) = (backend.clone(), url.clone());
            spawn(async move {
                if let Err(e) = backend.open_url(&url).await {
                    tracing::warn!(detail = %e.detail, "opening AniList failed");
                    snack.show(t("social.open_failed"));
                }
            });
        }
    };
    let name = r.user.name.clone();
    let now = now_unix();
    let open_now = expanded();
    let full = body.read().clone();
    let fail = failed.read().clone();

    rsx! {
        li { class: "tk-social-review", "data-review-id": "{id}", "data-expanded": "{open_now}",
            div { class: "tk-social-card__head",
                button {
                    class: "tk-social-card__user",
                    r#type: "button",
                    onclick: move |_| {
                        navigator.push(crate::app::Route::UserPage { name: name.clone() });
                    },
                    Avatar { user: r.user.clone(), size: 32 }
                    b { "{r.user.name}" }
                }
                if let Some(s) = r.score {
                    span { class: "tk-social-review__score", "data-review-score": "{s}", {tf("reviews.score", &[("score", &s.to_string())])} }
                }
                span { class: "tk-muted tk-social-card__time", {rel_time(now, r.created_unix)} }
            }
            p { class: "tk-social-review__summary", "{r.summary}" }
            if open_now {
                div { class: "tk-social-review__body",
                    if let Some(text) = full {
                        for (i , para) in paragraphs(&text).into_iter().enumerate() {
                            p { key: "{i}", "{para}" }
                        }
                    } else if let Some(e) = fail {
                        ErrorState { error: e, compact: true, on_retry: move |_| fetch.call(()) }
                    } else {
                        SkeletonLines { widths: vec![90, 85, 70] }
                    }
                }
            }
            div { class: "tk-social-card__actions",
                Button {
                    label: if open_now { t("reviews.less") } else { t("reviews.read_more") },
                    icon: if open_now { "chevron-up".to_string() } else { "chevron-down".to_string() },
                    variant: ButtonVariant::Text,
                    size: ButtonSize::Sm,
                    onclick: toggle,
                }
                if r.rating_amount > 0 {
                    span { class: "tk-muted", "data-review-rating": "{r.rating}",
                        {tf("reviews.helpful", &[("n", &r.rating.max(0).to_string()), ("total", &r.rating_amount.to_string())])}
                    }
                }
                span { class: "tk-social-card__spacer" }
                IconButton { icon: "external".to_string(), label: t("social.open_anilist"), size: IconButtonSize::Sm, onclick: open }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::UserVm;

    fn review(id: u64, score: Option<u32>) -> ReviewVm {
        ReviewVm {
            id,
            user: UserVm { id: 1, name: "a".into(), avatar: None, url: String::new() },
            summary: "s".into(),
            body: None,
            score,
            rating: 3,
            rating_amount: 4,
            created_unix: 0,
            url: String::new(),
        }
    }

    #[test]
    fn write_review_links() {
        assert_eq!(write_review_url("21", None), "https://anilist.co/anime/21");
        assert_eq!(write_review_url("mal:5", Some("Cowboy Bebop")), "https://anilist.co/search/anime?search=Cowboy%20Bebop");
        assert_eq!(write_review_url("mal:5", None), "https://anilist.co/search/anime");
        assert_eq!(url_encode("a&b=c"), "a%26b%3Dc");
        assert_eq!(url_encode("Ñ"), "%C3%91");
    }

    #[test]
    fn review_pages_and_average() {
        let mut items = vec![review(1, Some(80))];
        merge_reviews(&mut items, vec![review(1, Some(80)), review(2, Some(60)), review(3, None)]);
        assert_eq!(items.len(), 3);
        assert_eq!(reviewers_average(&items), Some(70));
        assert_eq!(reviewers_average(&[]), None);
    }
}
