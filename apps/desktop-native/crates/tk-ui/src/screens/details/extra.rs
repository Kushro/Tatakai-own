//! Details extras (wave A, worker c; `Backend::details_extra` / `watch_order`): the full facts sheet, characters
//! with voice actors and staff, related titles, the watch order of the franchise, recommendations, the adult
//! content gate and the pure helpers behind the header (next-episode countdown, share link). Private to
//! `screens/details.rs`.

use dioxus::prelude::*;

use crate::app::Route;
use crate::backend::{CharacterRoleVm, DetailsExtraVm, MediaFactsVm, PersonVm, RelatedVm, TrailerVm, WatchOrderEntryVm};
use crate::components::{
    Button, ButtonVariant, EmptyState, ErrorState, InfoChip, MediaCard, MediaCover, Pill, SkeletonLines, SkeletonRow,
    Tone,
};
use crate::i18n::{t, tf, tn};
use crate::icons::Icon;
use crate::model::UiError;

pub type ExtraState = Option<Result<DetailsExtraVm, UiError>>;
pub type OrderState = Option<Result<Vec<WatchOrderEntryVm>, UiError>>;

// ---- pure helpers -------------------------------------------------------------------------------------------

/// `(days, hours, minutes)` left in `secs` (negative = 0).
pub fn countdown_parts(secs: i64) -> (u64, u64, u64) {
    let s = u64::try_from(secs.max(0)).unwrap_or(0);
    (s / 86_400, (s % 86_400) / 3_600, (s % 3_600) / 60)
}

/// "Episode 5 in 2d 4h" / "in 3h 12m" / "in 7m" / "airing now".
pub fn airing_label(episode: u32, at_unix: i64, now_unix: i64) -> String {
    let n = episode.to_string();
    if at_unix <= now_unix {
        return tf("details.next_airing.now", &[("n", &n)]);
    }
    let (d, h, m) = countdown_parts(at_unix - now_unix);
    let when = if d > 0 {
        tf("details.dur.dh", &[("d", &d.to_string()), ("h", &h.to_string())])
    } else if h > 0 {
        tf("details.dur.hm", &[("h", &h.to_string()), ("m", &m.to_string())])
    } else {
        tf("details.dur.m", &[("m", &m.max(1).to_string())])
    };
    tf("details.next_airing", &[("n", &n), ("when", &when)])
}

/// Public page of the title to share: AniList when its id is known, else MAL, else Kitsu.
pub fn share_url(id: &str, facts: Option<&MediaFactsVm>) -> String {
    if let Some(a) = facts.and_then(|f| f.anilist_id) {
        return format!("https://anilist.co/anime/{a}");
    }
    if let Some(rest) = id.strip_prefix("mal:") {
        return format!("https://myanimelist.net/anime/{rest}");
    }
    if let Some(rest) = id.strip_prefix("kitsu:") {
        return format!("https://kitsu.app/anime/{rest}");
    }
    format!("https://anilist.co/anime/{id}")
}

/// i18n key of an AniList relation type (`None` = show the raw value).
pub fn relation_key(relation: &str) -> Option<&'static str> {
    Some(match relation {
        "PREQUEL" => "details.relation.prequel",
        "SEQUEL" => "details.relation.sequel",
        "PARENT" => "details.relation.parent",
        "SIDE_STORY" => "details.relation.side_story",
        "CHARACTER" => "details.relation.character",
        "SUMMARY" => "details.relation.summary",
        "ALTERNATIVE" => "details.relation.alternative",
        "SPIN_OFF" => "details.relation.spin_off",
        "OTHER" => "details.relation.other",
        "SOURCE" => "details.relation.source",
        "COMPILATION" => "details.relation.compilation",
        "CONTAINS" => "details.relation.contains",
        "ADAPTATION" => "details.relation.adaptation",
        _ => return None,
    })
}

/// Display order of relation groups: the story line first, then adaptations, then the rest.
pub fn relation_rank(relation: &str) -> u8 {
    match relation {
        "PREQUEL" => 0,
        "SEQUEL" => 1,
        "PARENT" => 2,
        "SOURCE" | "ADAPTATION" => 3,
        "SIDE_STORY" => 4,
        "SPIN_OFF" => 5,
        "ALTERNATIVE" => 6,
        "SUMMARY" | "COMPILATION" | "CONTAINS" => 7,
        _ => 8,
    }
}

/// Relations sorted by [`relation_rank`] (stable inside a group).
pub fn sorted_relations(list: &[RelatedVm]) -> Vec<RelatedVm> {
    let mut out = list.to_vec();
    out.sort_by_key(|r| relation_rank(&r.relation));
    out
}

fn relation_label(relation: &str) -> String {
    relation_key(relation).map(t).unwrap_or_else(|| relation.replace('_', " "))
}

fn role_label(role: &str) -> String {
    match role {
        "MAIN" => t("details.role.main"),
        "SUPPORTING" => t("details.role.supporting"),
        "BACKGROUND" => t("details.role.background"),
        other => other.to_owned(),
    }
}

fn format_label(format: &str) -> String {
    match format {
        "TV" => t("details.format.tv"),
        "TV_SHORT" => t("details.format.tv_short"),
        "MOVIE" => t("details.format.movie"),
        "SPECIAL" => t("details.format.special"),
        "OVA" => t("details.format.ova"),
        "ONA" => t("details.format.ona"),
        "MUSIC" => t("details.format.music"),
        other => other.replace('_', " "),
    }
}

fn source_label(source: &str) -> String {
    match source {
        "ORIGINAL" => t("details.source.original"),
        "MANGA" => t("details.source.manga"),
        "LIGHT_NOVEL" => t("details.source.light_novel"),
        "VISUAL_NOVEL" => t("details.source.visual_novel"),
        "VIDEO_GAME" | "GAME" => t("details.source.game"),
        "NOVEL" => t("details.source.novel"),
        "WEB_NOVEL" => t("details.source.web_novel"),
        "DOUJINSHI" => t("details.source.doujinshi"),
        "ANIME" => t("details.source.anime"),
        "LIVE_ACTION" => t("details.source.live_action"),
        "COMIC" => t("details.source.comic"),
        "MULTIMEDIA_PROJECT" => t("details.source.multimedia"),
        "PICTURE_BOOK" => t("details.source.picture_book"),
        "OTHER" => t("details.source.other"),
        other => other.replace('_', " "),
    }
}

fn season_label(season: &str, year: Option<i32>) -> String {
    let name = match season {
        "winter" => t("details.season.winter"),
        "spring" => t("details.season.spring"),
        "summer" => t("details.season.summer"),
        "fall" => t("details.season.fall"),
        other => other.to_owned(),
    };
    match year {
        Some(y) => format!("{name} {y}"),
        None => name,
    }
}

fn country_label(code: &str) -> String {
    match code {
        "JP" => t("details.country.jp"),
        "KR" => t("details.country.kr"),
        "CN" => t("details.country.cn"),
        "TW" => t("details.country.tw"),
        other => other.to_owned(),
    }
}

/// `1234567` -> `1,234,567` (locale-neutral thin grouping).
pub fn group_digits(n: u32) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// `(label, value)` rows of the facts sheet, in display order; rows without data are left out.
pub fn fact_rows(f: &MediaFactsVm, episodes: Option<u32>, status: Option<&str>) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    if let Some(x) = &f.format {
        rows.push((t("details.format"), format_label(x)));
    }
    if let Some(n) = episodes {
        rows.push((t("details.episodes"), n.to_string()));
    }
    if let Some(d) = f.duration_min {
        rows.push((t("details.duration"), tf("details.min", &[("n", &d.to_string())])));
    }
    if let Some(s) = status {
        rows.push((t("details.status"), s.to_owned()));
    }
    let aired = match (&f.start_date, &f.end_date) {
        (Some(a), Some(b)) if a != b => Some(format!("{a} – {b}")),
        (Some(a), _) => Some(a.clone()),
        (None, Some(b)) => Some(b.clone()),
        (None, None) => None,
    };
    if let Some(a) = aired {
        rows.push((t("details.facts.aired"), a));
    }
    if let Some(s) = &f.season {
        rows.push((t("details.season"), season_label(s, f.season_year)));
    }
    if let Some(x) = &f.source {
        rows.push((t("details.facts.source"), source_label(x)));
    }
    if let Some(c) = &f.country {
        rows.push((t("details.facts.country"), country_label(c)));
    }
    if let Some(s) = f.mean_score {
        rows.push((t("details.facts.mean_score"), format!("{s}%")));
    }
    if let Some(p) = f.popularity {
        rows.push((t("details.facts.popularity"), group_digits(p)));
    }
    if let Some(v) = f.favourites {
        rows.push((t("details.facts.favourites"), group_digits(v)));
    }
    // Wave B (b): MAL rank and members from the external catalogues.
    if let Some(r) = f.rank {
        rows.push((t("details.facts.mal_rank"), format!("#{r}")));
    }
    if let Some(m) = f.members {
        rows.push((t("details.facts.members"), group_digits(m)));
    }
    rows
}

// ---- adult gate ---------------------------------------------------------------------------------------------

/// Shown instead of the page for an `isAdult` title until the viewer confirms (classic `AdultWarningGate`).
#[component]
pub fn AdultGate(title: String, on_back: EventHandler<()>, on_continue: EventHandler<()>, on_always: EventHandler<()>) -> Element {
    rsx! {
        section { class: "tk-panel tk-dadult", "data-adult-gate": "pending", role: "alertdialog", "aria-labelledby": "tk-dadult-title",
            div { class: "tk-dadult__icon", Icon { name: "shield".to_string(), size: 36 } }
            h2 { id: "tk-dadult-title", class: "tk-dadult__title", {t("details.adult.title")} }
            p { class: "tk-dadult__body", {tf("details.adult.body", &[("title", &title)])} }
            div { class: "tk-dadult__actions",
                Button { label: t("details.adult.back"), icon: "back".to_string(), variant: ButtonVariant::Text, onclick: move |_| on_back.call(()) }
                Button { label: t("details.adult.continue"), variant: ButtonVariant::Tonal, onclick: move |_| on_continue.call(()) }
                Button { label: t("details.adult.always"), variant: ButtonVariant::Filled, onclick: move |_| on_always.call(()) }
            }
        }
    }
}

// ---- Info tab: facts sheet ----------------------------------------------------------------------------------

#[component]
pub fn FactsSheet(
    extra: ExtraState,
    episodes: Option<u32>,
    status: Option<String>,
    on_retry: EventHandler<()>,
    on_trailer: EventHandler<TrailerVm>,
    /// External / streaming links open in the system browser (`Backend::open_url`).
    on_link: EventHandler<String>,
) -> Element {
    let navigator = use_navigator();
    match extra {
        None => rsx! { SkeletonLines { widths: vec![70, 55, 80, 40, 65, 50] } },
        Some(Err(e)) => rsx! { ErrorState { error: e, compact: true, on_retry: move |_| on_retry.call(()) } },
        Some(Ok(x)) => {
            let f = x.facts.clone();
            let rows = fact_rows(&f, episodes, status.as_deref());
            let facts_source = f.facts_sources.join(" ");
            let ids: Vec<(String, String)> = [
                (t("provider.anilist"), f.anilist_id),
                (t("provider.mal"), f.mal_id),
                (t("details.facts.kitsu"), f.kitsu_id),
            ]
            .into_iter()
            .filter_map(|(k, v)| v.map(|v| (k, format!("#{v}"))))
            .collect();
            let trailer_label = t("details.trailer");
            let trailer = match x.trailer.clone() {
                Some(tr) => {
                    let site = tr.site.clone();
                    let thumb = tr.thumbnail.clone();
                    rsx! {
                        section { class: "tk-panel tk-dtrailer",
                            h3 { class: "tk-panel__title", Icon { name: "film".to_string(), size: 20 } "{trailer_label}" }
                            // Thumbnail only: never autoplays (reduce motion or not); the click opens the watch page.
                            button {
                                class: "tk-dtrailer__thumb",
                                r#type: "button",
                                "data-trailer-thumb": "{site}",
                                "aria-label": "{trailer_label}",
                                onclick: move |_| on_trailer.call(tr.clone()),
                                if let Some(img) = thumb {
                                    img { src: "{img}", alt: "", loading: "lazy", decoding: "async", draggable: "false" }
                                }
                                span { class: "tk-dtrailer__play", Icon { name: "play".to_string(), size: 28 } }
                            }
                        }
                    }
                }
                None => rsx! {},
            };
            rsx! {
                section { class: "tk-panel tk-dfacts", "data-extra": "facts", "data-facts-source": "{facts_source}",
                    h3 { class: "tk-panel__title", Icon { name: "info".to_string(), size: 20 } {t("details.facts.title")} }
                    dl { class: "tk-dfacts__grid",
                        for (i , (k , v)) in rows.into_iter().enumerate() {
                            div { key: "{i}", class: "tk-dfacts__row",
                                dt { "{k}" }
                                dd { "{v}" }
                            }
                        }
                        if !f.studios.is_empty() {
                            div { class: "tk-dfacts__row",
                                dt { {t("details.studio")} }
                                dd { class: "tk-dfacts__links",
                                    for s in f.studios.clone() {
                                        {
                                            let name = s.clone();
                                            rsx! {
                                                button {
                                                    key: "{s}",
                                                    class: "tk-dfacts__link",
                                                    r#type: "button",
                                                    "data-studio": "{s}",
                                                    onclick: move |_| {
                                                        navigator.push(Route::StudioPage { name: name.clone() });
                                                    },
                                                    "{s}"
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        if !f.producers.is_empty() {
                            div { class: "tk-dfacts__row",
                                dt { {t("details.facts.producers")} }
                                dd { {f.producers.join(", ")} }
                            }
                        }
                        for (k , v) in ids {
                            div { key: "{k}", class: "tk-dfacts__row",
                                dt { "{k}" }
                                dd { class: "tk-mono", "{v}" }
                            }
                        }
                    }
                }
                {trailer}
                if !f.tags.is_empty() {
                    section { class: "tk-panel", "data-extra": "tags",
                        h3 { class: "tk-panel__title", {t("details.facts.tags")} }
                        div { class: "tk-chips",
                            for (name , rank) in f.tags.clone() {
                                InfoChip { key: "{name}", text: tf("details.facts.tag", &[("name", &name), ("n", &rank.to_string())]) }
                            }
                        }
                    }
                }
                if !f.synonyms.is_empty() {
                    section { class: "tk-panel", "data-extra": "titles",
                        h3 { class: "tk-panel__title", {t("details.facts.alt_titles")} }
                        ul { class: "tk-dfacts__titles",
                            for s in f.synonyms.clone() {
                                li { key: "{s}", "{s}" }
                            }
                        }
                    }
                }
                if !x.external_links.is_empty() {
                    section { class: "tk-panel", "data-extra": "links",
                        h3 { class: "tk-panel__title", Icon { name: "external".to_string(), size: 20 } {t("details.facts.links")} }
                        div { class: "tk-chips",
                            for (site , url) in x.external_links.clone() {
                                {
                                    let target = url.clone();
                                    rsx! {
                                        button {
                                            key: "{url}",
                                            class: "tk-dfacts__ext",
                                            r#type: "button",
                                            title: "{url}",
                                            onclick: move |_| on_link.call(target.clone()),
                                            Icon { name: "external".to_string(), size: 16 }
                                            "{site}"
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

// ---- Characters tab -----------------------------------------------------------------------------------------

#[component]
pub fn CharactersTab(extra: ExtraState, images: bool, on_retry: EventHandler<()>) -> Element {
    let navigator = use_navigator();
    match extra {
        None => rsx! { SkeletonLines { widths: vec![60, 60, 60, 60] } },
        Some(Err(e)) => rsx! { ErrorState { error: e, compact: true, on_retry: move |_| on_retry.call(()) } },
        Some(Ok(x)) if x.characters.is_empty() && x.staff.is_empty() => rsx! {
            EmptyState { icon: "user".to_string(), title: t("details.no_characters.title"), body: t("details.no_characters.body") }
        },
        Some(Ok(x)) => rsx! {
            if !x.characters.is_empty() {
                section { class: "tk-section", "data-extra": "characters",
                    h3 { class: "tk-section__title tk-section__title--sm", {t("details.characters")} }
                    div { class: "tk-dchars",
                        for c in x.characters.clone() {
                            {
                                let id = c.id.clone();
                                rsx! {
                                    CharacterRow {
                                        key: "{c.id}",
                                        character: c.clone(),
                                        images,
                                        onclick: move |_| {
                                            navigator.push(Route::CharacterPage { id: id.clone() });
                                        },
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if !x.staff.is_empty() {
                section { class: "tk-section", "data-extra": "staff",
                    h3 { class: "tk-section__title tk-section__title--sm", {t("details.staff")} }
                    div { class: "tk-dstaff",
                        for (i , p) in x.staff.clone().into_iter().enumerate() {
                            PersonChip { key: "{i}", person: p, images }
                        }
                    }
                }
            }
        },
    }
}

fn avatar(src: Option<String>, name: &str, images: bool) -> Element {
    let mono: String = name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
    rsx! {
        span { class: "tk-davatar", "aria-hidden": "true",
            if let Some(src) = src.filter(|_| images) {
                img { src: "{src}", alt: "", loading: "lazy", decoding: "async", draggable: "false" }
            } else {
                span { "{mono}" }
            }
        }
    }
}

#[component]
fn CharacterRow(character: CharacterRoleVm, images: bool, onclick: EventHandler<MouseEvent>) -> Element {
    let role = role_label(&character.role);
    let va = character.voice_actors.first().cloned();
    rsx! {
        div { class: "tk-dchar", "data-character-role": "{character.role}",
            button { class: "tk-dchar__who", r#type: "button", onclick: move |e| onclick.call(e),
                {avatar(character.image.clone(), &character.name, images)}
                span { class: "tk-dchar__text",
                    span { class: "tk-dchar__name", "{character.name}" }
                    span { class: "tk-dchar__sub", "{role}" }
                }
            }
            if let Some(v) = va {
                div { class: "tk-dchar__va", "data-voice-actor": "{v.id}",
                    span { class: "tk-dchar__text tk-dchar__text--end",
                        span { class: "tk-dchar__name", "{v.name}" }
                        span { class: "tk-dchar__sub", {v.role.clone().unwrap_or_default()} }
                    }
                    {avatar(v.image.clone(), &v.name, images)}
                }
            }
        }
    }
}

#[component]
fn PersonChip(person: PersonVm, images: bool) -> Element {
    rsx! {
        div { class: "tk-dperson",
            {avatar(person.image.clone(), &person.name, images)}
            span { class: "tk-dchar__text",
                span { class: "tk-dchar__name", "{person.name}" }
                if let Some(r) = person.role.clone() {
                    span { class: "tk-dchar__sub", "{r}" }
                }
            }
        }
    }
}

// ---- Related tab --------------------------------------------------------------------------------------------

#[component]
pub fn RelatedTab(extra: ExtraState, on_retry: EventHandler<()>) -> Element {
    let navigator = use_navigator();
    match extra {
        None => rsx! { SkeletonLines { widths: vec![70, 70, 70] } },
        Some(Err(e)) => rsx! { ErrorState { error: e, compact: true, on_retry: move |_| on_retry.call(()) } },
        Some(Ok(x)) if x.relations.is_empty() => rsx! {
            EmptyState { icon: "library".to_string(), title: t("details.no_related.title"), body: t("details.no_related.body") }
        },
        Some(Ok(x)) => rsx! {
            div { class: "tk-drelated", "data-extra": "relations",
                for r in sorted_relations(&x.relations) {
                    {
                        let label = relation_label(&r.relation);
                        let manga = r.kind == "manga";
                        let id = r.media.id.clone();
                        let body = rsx! {
                            span { class: "tk-drelated__cover",
                                MediaCover { src: r.media.cover.clone(), color: r.media.color.clone(), title: r.media.title.clone() }
                            }
                            span { class: "tk-drelated__text",
                                span { class: "tk-drelated__rel", "{label}" }
                                span { class: "tk-drelated__title", "{r.media.title}" }
                                span { class: "tk-chips",
                                    if manga {
                                        Pill { text: t("details.kind.manga"), tone: Tone::Primary, plain: true }
                                    }
                                    if !r.media.subtitle.is_empty() {
                                        span { class: "tk-drelated__sub", "{r.media.subtitle}" }
                                    }
                                }
                            }
                        };
                        if manga {
                            // Wave B (l, `manga.details.anime-link`): anime -> manga opens the manga details page.
                            rsx! {
                                button {
                                    key: "{r.media.id}",
                                    class: "tk-drelated__row",
                                    r#type: "button",
                                    "data-relation": "{r.relation}",
                                    "data-kind": "manga",
                                    onclick: move |_| {
                                        navigator.push(Route::MangaDetailsPage { id: id.clone() });
                                    },
                                    {body}
                                }
                            }
                        } else {
                            rsx! {
                                button {
                                    key: "{r.media.id}",
                                    class: "tk-drelated__row",
                                    r#type: "button",
                                    "data-relation": "{r.relation}",
                                    "data-kind": "anime",
                                    onclick: move |_| {
                                        navigator.push(Route::DetailsPage { id: id.clone() });
                                    },
                                    {body}
                                }
                            }
                        }
                    }
                }
            }
        },
    }
}

// ---- watch order + recommendations (below the tabs) ---------------------------------------------------------

#[component]
pub fn WatchOrder(order: OrderState, on_retry: EventHandler<()>) -> Element {
    let navigator = use_navigator();
    let head = |count: Option<usize>| {
        rsx! {
            div { class: "tk-section__head",
                h2 { class: "tk-section__title tk-section__title--sm", {t("details.watch_order")} }
                if let Some(n) = count {
                    span { class: "tk-section__count", {tn("details.watch_order.count", n)} }
                }
            }
        }
    };
    match order {
        None => rsx! {
            section { class: "tk-section", {head(None)} SkeletonRow { count: 5 } }
        },
        Some(Err(e)) => rsx! {
            section { class: "tk-section", {head(None)}
                ErrorState { error: e, compact: true, on_retry: move |_| on_retry.call(()) }
            }
        },
        // A title with no prequel, sequel or side story has no order to show.
        Some(Ok(list)) if list.len() <= 1 => rsx! {},
        Some(Ok(list)) => rsx! {
            section { class: "tk-section", "data-extra": "watch-order",
                {head(Some(list.len()))}
                div { class: "tk-row tk-dorder",
                    for e in list.clone() {
                        {
                            let id = e.media.id.clone();
                            let current = e.current;
                            let badge = if e.current {
                                t("details.watch_order.now")
                            } else {
                                match e.relation.as_str() {
                                    "SIDE_STORY" | "SUMMARY" => relation_label(&e.relation),
                                    _ => format!("#{}", e.position),
                                }
                            };
                            rsx! {
                                div {
                                    key: "{e.media.id}",
                                    class: "tk-dorder__item",
                                    "data-position": "{e.position}",
                                    "data-current": if e.current { "true" },
                                    MediaCard {
                                        media: e.media.clone(),
                                        selected: e.current,
                                        badge,
                                        hide_score: true,
                                        onclick: move |_| {
                                            if !current {
                                                navigator.push(Route::DetailsPage { id: id.clone() });
                                            }
                                        },
                                    }
                                }
                            }
                        }
                    }
                }
            }
        },
    }
}

#[component]
pub fn Recommendations(extra: ExtraState) -> Element {
    let navigator = use_navigator();
    let title = rsx! {
        div { class: "tk-section__head",
            h2 { class: "tk-section__title tk-section__title--sm", {t("details.recommendations")} }
        }
    };
    match extra {
        None => rsx! {
            section { class: "tk-section", {title} SkeletonRow { count: 6 } }
        },
        // The facts sheet already shows the error with Retry; one is enough.
        Some(Err(_)) => rsx! {},
        Some(Ok(x)) if x.recommendations.is_empty() => rsx! {
            section { class: "tk-section", "data-extra": "recommendations", "data-empty": "true",
                {title}
                p { class: "tk-muted", {t("details.no_recommendations")} }
            }
        },
        Some(Ok(x)) => rsx! {
            section { class: "tk-section", "data-extra": "recommendations",
                {title}
                div { class: "tk-row",
                    for m in x.recommendations.clone() {
                        {
                            let id = m.id.clone();
                            rsx! {
                                MediaCard {
                                    key: "{m.id}",
                                    media: m.clone(),
                                    onclick: move |_| {
                                        navigator.push(Route::DetailsPage { id: id.clone() });
                                    },
                                }
                            }
                        }
                    }
                }
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::MediaVm;

    fn rel(relation: &str) -> RelatedVm {
        RelatedVm {
            media: MediaVm {
                id: relation.into(),
                title: relation.into(),
                cover: None,
                color: None,
                subtitle: String::new(),
                score: None,
            },
            relation: relation.into(),
            kind: "anime".into(),
        }
    }

    #[test]
    fn countdown_splits_days_hours_minutes() {
        assert_eq!(countdown_parts(2 * 86_400 + 4 * 3_600 + 59), (2, 4, 0));
        assert_eq!(countdown_parts(3 * 3_600 + 12 * 60), (0, 3, 12));
        assert_eq!(countdown_parts(-5), (0, 0, 0));
    }

    #[test]
    fn share_prefers_anilist() {
        let f = MediaFactsVm { anilist_id: Some(21), ..MediaFactsVm::default() };
        assert_eq!(share_url("mal:21", Some(&f)), "https://anilist.co/anime/21");
        assert_eq!(share_url("mal:5114", None), "https://myanimelist.net/anime/5114");
        assert_eq!(share_url("kitsu:7", None), "https://kitsu.app/anime/7");
        assert_eq!(share_url("154587", None), "https://anilist.co/anime/154587");
    }

    #[test]
    fn relations_group_story_first() {
        let list = [rel("OTHER"), rel("SEQUEL"), rel("SOURCE"), rel("PREQUEL")];
        let ids: Vec<String> = sorted_relations(&list).into_iter().map(|r| r.relation).collect();
        assert_eq!(ids, vec!["PREQUEL", "SEQUEL", "SOURCE", "OTHER"]);
        assert_eq!(relation_key("SPIN_OFF"), Some("details.relation.spin_off"));
        assert_eq!(relation_key("NEW_KIND"), None);
    }

    #[test]
    fn fact_rows_skip_missing_data() {
        let full = MediaFactsVm {
            format: Some("TV".into()),
            source: Some("MANGA".into()),
            season: Some("fall".into()),
            season_year: Some(2023),
            duration_min: Some(24),
            start_date: Some("2023-09-29".into()),
            end_date: Some("2024-03-22".into()),
            country: Some("JP".into()),
            mean_score: Some(90),
            popularity: Some(400_000),
            favourites: Some(50_000),
            ..MediaFactsVm::default()
        };
        let rows = fact_rows(&full, Some(28), Some("Finished"));
        assert_eq!(rows.len(), 11);
        assert!(rows.iter().any(|(_, v)| v == "2023-09-29 – 2024-03-22"));
        assert!(rows.iter().any(|(_, v)| v == "400,000"));
        assert!(fact_rows(&MediaFactsVm::default(), None, None).is_empty());
        let same_day = MediaFactsVm { start_date: Some("2020-01-01".into()), end_date: Some("2020-01-01".into()), ..MediaFactsVm::default() };
        assert_eq!(fact_rows(&same_day, None, None)[0].1, "2020-01-01");
    }

    #[test]
    fn digits_are_grouped() {
        assert_eq!(group_digits(0), "0");
        assert_eq!(group_digits(999), "999");
        assert_eq!(group_digits(1_000), "1,000");
        assert_eq!(group_digits(1_234_567), "1,234,567");
    }
}
