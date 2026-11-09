//! "For you" page `/recommendations` (wave B, worker **b**; checklist `home.recs.for-you`, `home.recs.explanations`,
//! `home.recs.feedback`, `home.recs.atlas`; classic `RecommendationsPage.tsx`, `atlas/AnimeAtlasPanel.tsx`).
//!
//! Three tabs: **For you** (cards from `Backend::recommendations(page)` with the affinity percent `data-score`, the
//! reasons `data-reasons` rendered with `recs.reason.<kind>`, and Not interested / Skip / Like stored by
//! `recs_feedback`; dismissed cards leave at once and stay out after a reload), **Atlas** (`atlas_points` as an SVG
//! scatter drawn with tokens only, `data-atlas-points`; a dot opens its title) and **Your taste** (`taste_profile`).
//! [`RecsRow`] is the Home teaser of the same list.

use std::collections::{HashMap, HashSet};

use dioxus::prelude::*;

use crate::app::{Route, Services};
use crate::backend::{AtlasPointVm, RecommendationVm, RecsFeedbackVm, TasteProfileVm};
use crate::components::{
    Button, ButtonSize, ButtonVariant, EmptyState, ErrorState, LoadingIndicator, MediaCard, Segmented,
    SkeletonGrid, SkeletonLines, SkeletonRow, use_snackbar,
};
use crate::i18n::{t, tf, tn};
use crate::model::UiError;
use crate::screens::search::genre_label;

/// Tabs of the page (`data-tab`).
const TABS: [&str; 3] = ["for_you", "atlas", "profile"];
/// Recommendations on the Home teaser row.
const ROW_ITEMS: usize = 12;
/// Clusters with their own color on the Atlas (the rest share the outline color).
const ATLAS_COLORS: usize = 8;
/// Atlas canvas (viewBox units).
const ATLAS_W: f32 = 1000.0;
const ATLAS_H: f32 = 620.0;
const ATLAS_PAD: f32 = 32.0;

/// Affinity `0..=1` -> whole percent.
pub fn percent(score: f32) -> u32 {
    (score.clamp(0.0, 1.0) * 100.0).round() as u32
}

/// Display text of one reason token (`genre:Action` -> "You like Action"); `None` for an unknown kind.
pub fn reason_text(token: &str) -> Option<String> {
    let (kind, value) = token.split_once(':')?;
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    let shown = match kind {
        "genre" => genre_label(value),
        "studio" | "era" | "tag" | "score" => value.to_owned(),
        _ => return None,
    };
    Some(tf(&format!("recs.reason.{kind}"), &[("value", &shown)]))
}

/// Cluster -> color slot: the [`ATLAS_COLORS`] most common clusters get `0..ATLAS_COLORS`, the rest `ATLAS_COLORS`.
pub fn cluster_slots(points: &[AtlasPointVm]) -> HashMap<String, usize> {
    let mut count: HashMap<&str, usize> = HashMap::new();
    for p in points {
        *count.entry(p.cluster.as_str()).or_insert(0) += 1;
    }
    let mut order: Vec<(&str, usize)> = count.into_iter().collect();
    order.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));
    order
        .into_iter()
        .enumerate()
        .map(|(i, (c, _))| (c.to_owned(), i.min(ATLAS_COLORS)))
        .collect()
}

/// Atlas point -> viewBox coordinates.
fn atlas_xy(p: &AtlasPointVm) -> (f32, f32) {
    (
        ATLAS_PAD + p.x.clamp(0.0, 1.0) * (ATLAS_W - 2.0 * ATLAS_PAD),
        ATLAS_PAD + p.y.clamp(0.0, 1.0) * (ATLAS_H - 2.0 * ATLAS_PAD),
    )
}

/// Cards still to show: not dismissed, first occurrence of each id.
pub fn visible(list: &[RecommendationVm], hidden: &HashSet<String>) -> Vec<RecommendationVm> {
    let mut seen = HashSet::new();
    list.iter()
        .filter(|r| !hidden.contains(&r.media.id) && seen.insert(r.media.id.clone()))
        .cloned()
        .collect()
}

#[component]
pub fn RecommendationsPage() -> Element {
    let mut tab = use_signal(|| TABS[0].to_string());
    let options: Vec<(String, String)> = TABS
        .iter()
        .map(|id| ((*id).to_string(), t(&format!("recs.tab.{id}"))))
        .collect();
    let current = tab();
    let content = match current.as_str() {
        "atlas" => rsx! {
            Atlas {}
        },
        "profile" => rsx! {
            Taste {}
        },
        _ => rsx! {
            ForYou {}
        },
    };
    rsx! {
        div { class: "tk-page tk-recs", "data-page": "recs.title", "data-tab": "{current}",
            div { class: "tk-page__head",
                div {
                    h1 { class: "tk-page__title", {t("recs.title")} }
                    p { class: "tk-page__intro", {t("recs.intro")} }
                }
            }
            div { class: "tk-recs__tabs",
                Segmented { options, selected: current.clone(), label: t("recs.tabs"), on_select: move |id: String| tab.set(id) }
            }
            {content}
        }
    }
}

/// The "For you" grid with paging and feedback.
#[component]
fn ForYou() -> Element {
    let services = use_context::<Services>();
    let navigator = use_navigator();
    let mut first = use_resource({
        let backend = services.backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.recommendations(1).await }
        }
    });
    let more = use_signal(Vec::<RecommendationVm>::new);
    let next_page = use_signal(|| 2_u32);
    let loading_more = use_signal(|| false);
    let at_end = use_signal(|| false);
    let more_error = use_signal(|| None::<UiError>);
    let hidden = use_signal(HashSet::<String>::new);

    let load_more = use_callback({
        let backend = services.backend.clone();
        move |()| {
            if *loading_more.peek() {
                return;
            }
            let (mut more, mut next_page, mut loading_more, mut at_end, mut more_error) =
                (more, next_page, loading_more, at_end, more_error);
            loading_more.set(true);
            more_error.set(None);
            let page = *next_page.peek();
            let backend = backend.clone();
            spawn(async move {
                match backend.recommendations(page).await {
                    Ok(list) if list.is_empty() => at_end.set(true),
                    Ok(list) => {
                        more.write().extend(list);
                        next_page.set(page + 1);
                    }
                    Err(e) => more_error.set(Some(e)),
                }
                loading_more.set(false);
            });
        }
    });

    let body = match &*first.read() {
        None => rsx! {
            SkeletonGrid { count: 12 }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), on_retry: move |_| first.restart() }
        },
        Some(Ok(list)) if list.is_empty() => rsx! {
            EmptyState {
                icon: "star".to_string(),
                title: t("recs.empty.title"),
                body: t("recs.empty.body"),
                action_label: t("recs.empty.action"),
                on_action: move |_| {
                    navigator.push(Route::SearchPage {});
                },
            }
        },
        Some(Ok(list)) => {
            let mut all = list.clone();
            all.extend(more.read().iter().cloned());
            let cards = visible(&all, &hidden.read());
            rsx! {
                div { class: "tk-recs__grid", "data-results": "{cards.len()}",
                    for r in cards.iter().cloned() {
                        RecCard { key: "{r.media.id}", rec: r.clone(), hidden }
                    }
                }
                div { class: "tk-recs__more",
                    if let Some(e) = more_error() {
                        ErrorState { error: e, compact: true, on_retry: move |_| load_more.call(()) }
                    } else if at_end() {
                        p { class: "tk-muted", {t("recs.end")} }
                    } else if loading_more() {
                        LoadingIndicator { small: true }
                    } else {
                        Button { label: t("recs.more"), variant: ButtonVariant::Tonal, onclick: move |_| load_more.call(()) }
                    }
                }
            }
        }
    };
    body
}

/// One recommendation: poster, affinity, reasons and the feedback buttons.
#[component]
fn RecCard(
    rec: RecommendationVm,
    hidden: Signal<HashSet<String>>,
    #[props(default)] compact: bool,
) -> Element {
    let services = use_context::<Services>();
    let navigator = use_navigator();
    let snack = use_snackbar();
    let given = use_signal(|| None::<RecsFeedbackVm>);
    let pct = percent(rec.score);
    let tokens = rec.reasons.join("|");
    let reasons: Vec<String> = rec.reasons.iter().filter_map(|r| reason_text(r)).collect();
    let feedback_id = given().map_or("none", RecsFeedbackVm::id);
    let id = rec.media.id.clone();
    let send = {
        let (backend, id) = (services.backend.clone(), id.clone());
        move |fb: RecsFeedbackVm| {
            let (backend, id, mut given, mut hidden) = (backend.clone(), id.clone(), given, hidden);
            spawn(async move {
                match backend.recs_feedback(&id, fb).await {
                    Ok(()) => {
                        given.set(Some(fb));
                        if fb == RecsFeedbackVm::Like {
                            snack.show(t("recs.feedback.saved"));
                        } else {
                            hidden.write().insert(id);
                            snack.show(t("recs.feedback.hidden"));
                        }
                    }
                    Err(e) => snack.show(t(e.kind.key())),
                }
            });
        }
    };
    let open_id = id.clone();
    rsx! {
        article {
            class: "tk-rec",
            "data-media-id": "{rec.media.id}",
            "data-score": "{pct}",
            "data-reasons": "{tokens}",
            "data-feedback": "{feedback_id}",
            "data-compact": "{compact}",
            MediaCard {
                media: rec.media.clone(),
                hide_score: true,
                badge: tf("recs.match", &[("n", &pct.to_string())]),
                onclick: move |_| {
                    navigator.push(Route::DetailsPage { id: open_id.clone() });
                },
            }
            if !compact {
                ul { class: "tk-rec__reasons",
                    for (i , r) in reasons.into_iter().enumerate() {
                        li { key: "{i}", class: "tk-rec__reason", "{r}" }
                    }
                }
                div { class: "tk-rec__actions",
                    for fb in RecsFeedbackVm::ALL {
                        {
                            let mut send = send.clone();
                            let on = given() == Some(fb);
                            rsx! {
                                Button {
                                    key: "{fb.id()}",
                                    label: t(&format!("recs.feedback.{}", fb.id())),
                                    variant: if on { ButtonVariant::Tonal } else { ButtonVariant::Text },
                                    size: ButtonSize::Sm,
                                    onclick: move |_| send(fb),
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Home teaser: the first recommendations in a row, "See all" opens the page. Renders nothing without history.
#[component]
pub fn RecsRow() -> Element {
    let services = use_context::<Services>();
    let navigator = use_navigator();
    let mut recs = use_resource({
        let backend = services.backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.recommendations(1).await }
        }
    });
    let hidden = use_signal(HashSet::<String>::new);
    let body = match &*recs.read() {
        None => rsx! {
            SkeletonRow { count: 6 }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), compact: true, on_retry: move |_| recs.restart() }
        },
        Some(Ok(list)) if list.is_empty() => return rsx! {},
        Some(Ok(list)) => {
            let cards: Vec<RecommendationVm> = visible(list, &hidden.read())
                .into_iter()
                .take(ROW_ITEMS)
                .collect();
            rsx! {
                div { class: "tk-row",
                    for r in cards {
                        RecCard { key: "{r.media.id}", rec: r.clone(), hidden, compact: true }
                    }
                }
            }
        }
    };
    rsx! {
        section { class: "tk-section", "data-row": "recs",
            div { class: "tk-section__head",
                h2 { class: "tk-section__title", {t("recs.title")} }
                Button {
                    label: t("common.see_all"),
                    variant: ButtonVariant::Text,
                    size: ButtonSize::Sm,
                    onclick: move |_| {
                        navigator.push(Route::RecommendationsPage {});
                    },
                }
            }
            {body}
        }
    }
}

/// The Anime Atlas: seen titles + recommendations projected by genre.
#[component]
fn Atlas() -> Element {
    let services = use_context::<Services>();
    let navigator = use_navigator();
    let mut points = use_resource({
        let backend = services.backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.atlas_points().await }
        }
    });
    let body = match &*points.read() {
        None => rsx! {
            div { class: "tk-atlas", "aria-busy": "true", LoadingIndicator { contained: true } }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), on_retry: move |_| points.restart() }
        },
        Some(Ok(list)) if list.len() < 3 => rsx! {
            EmptyState { icon: "grid".to_string(), title: t("recs.atlas.empty"), body: t("recs.empty.body") }
        },
        Some(Ok(list)) => {
            let slots = cluster_slots(list);
            let mut legend: Vec<(String, usize)> = slots
                .iter()
                .filter(|(c, i)| **i < ATLAS_COLORS && !c.is_empty())
                .map(|(c, i)| (c.clone(), *i))
                .collect();
            legend.sort_by_key(|(_, i)| *i);
            let n = list.len();
            rsx! {
                figure { class: "tk-atlas", "data-atlas-points": "{n}",
                    svg {
                        class: "tk-atlas__map",
                        view_box: "0 0 {ATLAS_W} {ATLAS_H}",
                        role: "group",
                        "aria-label": t("recs.atlas.label"),
                        for (i , p) in list.iter().enumerate() {
                            {
                                let (x, y) = atlas_xy(p);
                                let slot = slots.get(&p.cluster).copied().unwrap_or(ATLAS_COLORS);
                                let id = p.media.id.clone();
                                let label = p.media.title.clone();
                                rsx! {
                                    circle {
                                        key: "{i}-{p.media.id}",
                                        class: "tk-atlas__dot tk-atlas__dot--c{slot}",
                                        cx: "{x:.1}",
                                        cy: "{y:.1}",
                                        r: "9",
                                        "tabindex": "0",
                                        "role": "button",
                                        "aria-label": "{label}",
                                        "data-cluster": "{p.cluster}",
                                        onclick: move |_| {
                                            navigator.push(Route::DetailsPage { id: id.clone() });
                                        },
                                        title { "{label}" }
                                    }
                                }
                            }
                        }
                    }
                    figcaption { class: "tk-atlas__legend",
                        p { class: "tk-muted", {t("recs.atlas.hint")} }
                        ul {
                            for (c , i) in legend {
                                li { key: "{c}", class: "tk-atlas__key",
                                    span { class: "tk-atlas__swatch tk-atlas__dot--c{i}", "aria-hidden": "true" }
                                    {genre_label(&c)}
                                }
                            }
                        }
                    }
                }
            }
        }
    };
    body
}

/// "Your taste": the weighted profile the recommendations come from.
#[component]
fn Taste() -> Element {
    let services = use_context::<Services>();
    let mut profile = use_resource({
        let backend = services.backend.clone();
        move || {
            let backend = backend.clone();
            async move { backend.taste_profile().await }
        }
    });
    match &*profile.read() {
        None => rsx! {
            SkeletonLines { widths: vec![80, 70, 60, 50] }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), on_retry: move |_| profile.restart() }
        },
        Some(Ok(p)) if p.sample_size == 0 => rsx! {
            EmptyState { icon: "star".to_string(), title: t("recs.empty.title"), body: t("recs.empty.body") }
        },
        Some(Ok(p)) => rsx! {
            TasteView { profile: p.clone() }
        },
    }
}

#[component]
fn TasteView(profile: TasteProfileVm) -> Element {
    let groups: Vec<(&str, Vec<(String, f32)>)> = vec![
        (
            "genres",
            profile.genres.iter().map(|(g, w)| (genre_label(g), *w)).collect(),
        ),
        ("studios", profile.studios.clone()),
        ("eras", profile.eras.clone()),
        ("themes", profile.themes.clone()),
    ];
    let sample = tn("recs.profile.sample", profile.sample_size as usize);
    let accuracy = profile
        .accuracy
        .map(|a| tf("recs.profile.accuracy", &[("n", &percent(a).to_string())]));
    rsx! {
        div { class: "tk-taste", "data-sample": "{profile.sample_size}",
            p { class: "tk-muted", "{sample}" }
            if let Some(a) = accuracy {
                p { class: "tk-taste__accuracy", "{a}" }
            }
            div { class: "tk-taste__groups",
                for (id , items) in groups {
                    section { key: "{id}", class: "tk-taste__group",
                        h3 { class: "tk-taste__title", {t(&format!("recs.profile.{id}"))} }
                        if items.is_empty() {
                            p { class: "tk-muted", {t("recs.profile.empty")} }
                        }
                        ul {
                            for (name , w) in items.into_iter().take(8) {
                                li { key: "{name}", class: "tk-taste__bar",
                                    span { class: "tk-taste__name", "{name}" }
                                    span { class: "tk-taste__track", "aria-hidden": "true",
                                        span { class: "tk-taste__fill", style: "width:{percent(w)}%" }
                                    }
                                    span { class: "tk-taste__pct tk-mono", "{percent(w)}%" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::MediaVm;

    fn media(id: &str) -> MediaVm {
        MediaVm {
            id: id.into(),
            title: id.into(),
            cover: None,
            color: None,
            subtitle: String::new(),
            score: None,
        }
    }

    #[test]
    fn percents_are_clamped() {
        assert_eq!(percent(0.876), 88);
        assert_eq!(percent(1.7), 100);
        assert_eq!(percent(-1.0), 0);
    }

    #[test]
    fn reasons_need_a_known_kind_and_a_value() {
        assert!(reason_text("studio:MAPPA").is_some());
        assert!(reason_text("bogus:x").is_none());
        assert!(reason_text("genre:").is_none());
        assert!(reason_text("nocolon").is_none());
    }

    #[test]
    fn dismissed_and_duplicate_cards_are_hidden() {
        let r = |id: &str| RecommendationVm {
            media: media(id),
            score: 0.5,
            reasons: vec![],
            atlas: None,
        };
        let hidden: HashSet<String> = ["2".to_owned()].into_iter().collect();
        let v = visible(&[r("1"), r("2"), r("1"), r("3")], &hidden);
        assert_eq!(
            v.iter().map(|x| x.media.id.as_str()).collect::<Vec<_>>(),
            ["1", "3"]
        );
    }

    #[test]
    fn common_clusters_get_their_own_color() {
        let p = |c: &str| AtlasPointVm {
            media: media(c),
            x: 0.5,
            y: 0.5,
            cluster: c.into(),
        };
        let mut pts: Vec<AtlasPointVm> = (0..10).map(|i| p(&format!("g{i}"))).collect();
        pts.push(p("g0"));
        let slots = cluster_slots(&pts);
        assert_eq!(slots["g0"], 0);
        // 10 clusters: the ATLAS_COLORS most common keep their own slot, the other 10 - ATLAS_COLORS share the last.
        assert_eq!(
            slots.values().filter(|s| **s == ATLAS_COLORS).count(),
            10 - ATLAS_COLORS
        );
        let (x, y) = atlas_xy(&AtlasPointVm {
            media: media("a"),
            x: 0.0,
            y: 1.0,
            cluster: String::new(),
        });
        assert_eq!((x, y), (ATLAS_PAD, ATLAS_H - ATLAS_PAD));
    }
}
