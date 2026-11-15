//! Home spotlight (2026-11 UI refresh, `HomeLayout::Spotlight`): a full-bleed banner of the trending titles.
//!
//! Every slide is rendered once and stacked; the active one fades in (opacity) and its art drifts slowly (Ken
//! Burns, `transform: scale`), so a slide change never re-lays out the page. Each slide shows the title, fact chips
//! (episodes, format, status, season, score %), the synopsis with genre chips, a primary action (continue / watch
//! / details), the list button and an info button. The timed segments at the bottom ([`HeroSegments`]) drive the
//! autoplay with a CSS animation, so hover, focus and a hidden window pause it without a Rust timer.

use dioxus::prelude::*;

use std::collections::HashMap;

use crate::app::{Route, Services, Ui};
use crate::backend::{HeroArtVm, HeroVm, MediaVm};
use crate::components::button::{Button, IconButton, IconButtonVariant};
use crate::components::list_button::ListButton;
use crate::components::media::{
    HeroSegments, StatusKind, cover_color_var, format_label, hero_autoplay, monogram, next_index, prev_index,
    status_kind, status_label, use_hero_motion,
};
use crate::i18n::{t, tf, tn};
use crate::screens::search::genre_label;

/// Genre chips under the synopsis.
const MAX_GENRES: usize = 4;

/// Slides of the spotlight: `hero` when the backend sent it, else the first `max` cards of `fallback` (the first
/// Home section) with the facts a card carries.
pub fn hero_slides(hero: &[HeroVm], fallback: &[MediaVm], max: usize) -> Vec<HeroVm> {
    if !hero.is_empty() {
        return hero.iter().take(max).cloned().collect();
    }
    fallback
        .iter()
        .take(max)
        .map(|m| HeroVm {
            media: m.clone(),
            status: m.status.clone(),
            year: m.year,
            format: m.format.clone(),
            ..HeroVm::default()
        })
        .collect()
}

/// Backdrop of a slide (0.3.2), as `data-art`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeroArt {
    /// ani.zip / TVDB fanart (1920x1080 class): shown as is.
    Fanart,
    /// AniList `bannerImage` (1900x400 class): a short strip, so the low-res guard applies (bottom-anchored crop,
    /// a little blur and a stronger scrim instead of a blown-up sharp image).
    Banner,
    /// The poster, blurred behind the text.
    Cover,
}

impl HeroArt {
    pub fn id(self) -> &'static str {
        match self {
            Self::Fanart => "fanart",
            Self::Banner => "banner",
            Self::Cover => "cover",
        }
    }
}

/// Backdrop of a slide: the fanart, else the AniList banner, else the cover; `None` = the monogram art only.
pub fn hero_backdrop(h: &HeroVm) -> Option<(HeroArt, String)> {
    let some = |v: &Option<String>| v.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
    some(&h.fanart)
        .map(|u| (HeroArt::Fanart, u))
        .or_else(|| some(&h.banner).map(|u| (HeroArt::Banner, u)))
        .or_else(|| some(&h.media.cover).map(|u| (HeroArt::Cover, u)))
}

/// Slides that still have no ani.zip artwork (asked through `Backend::hero_art` after Home rendered).
pub fn art_wanted(slides: &[HeroVm]) -> Vec<String> {
    slides.iter().filter(|h| h.fanart.is_none() && h.logo.is_none()).map(|h| h.media.id.clone()).collect()
}

/// `h` with the artwork that arrived for it (fields already set stay).
pub fn with_art(h: &HeroVm, art: Option<&HeroArtVm>) -> HeroVm {
    let mut out = h.clone();
    if let Some(a) = art {
        if out.fanart.is_none() {
            out.fanart = a.fanart.clone();
        }
        if out.logo.is_none() {
            out.logo = a.logo.clone();
        }
    }
    out
}

/// "Fall 2026"; the season or the year alone when only one is known.
pub fn season_label(season: Option<&str>, year: Option<i32>) -> Option<String> {
    let season = season
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            let key = format!("details.season.{}", s.to_ascii_lowercase());
            let text = t(&key);
            if text == key { s.to_owned() } else { text }
        });
    let year = year.filter(|y| *y > 0);
    match (season, year) {
        (Some(s), Some(y)) => Some(format!("{s} {y}")),
        (Some(s), None) => Some(s),
        (None, Some(y)) => Some(y.to_string()),
        (None, None) => None,
    }
}

/// A fact chip of a slide; `kind` is its `data-kind` (CSS colors the score).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeroChip {
    pub kind: &'static str,
    pub text: String,
}

/// Fact chips in Hayase order: episodes, format, status, season + year, score as a percentage (0..=100).
pub fn hero_chips(h: &HeroVm) -> Vec<HeroChip> {
    let mut out = Vec::new();
    let mut push = |kind, text: String| out.push(HeroChip { kind, text });
    if let Some(n) = h.episodes.filter(|n| *n > 0) {
        push("episodes", tn("hero.chip.episodes", n as usize));
    }
    let nonempty = |v: Option<&String>| v.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
    if let Some(f) = nonempty(h.format.as_ref()).or_else(|| nonempty(h.media.format.as_ref())) {
        push("format", format_label(&f));
    }
    if let Some(s) = nonempty(h.status.as_ref()).or_else(|| nonempty(h.media.status.as_ref())) {
        push("status", status_label(&s));
    }
    if let Some(s) = season_label(h.season.as_deref(), h.year.or(h.media.year)) {
        push("season", s);
    }
    if let Some(score) = h.media.score.filter(|s| *s > 0) {
        push("score", format!("{}%", score.min(100)));
    }
    out
}

/// Primary button of a slide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeroAction {
    /// "Continue ep. N": the title is in Continue watching.
    Continue(u32),
    /// "Watch now": episode 1.
    Watch,
    /// "Details": not aired yet.
    Details,
}

pub fn hero_action(status: Option<&str>, resume_episode: Option<u32>) -> HeroAction {
    if let Some(ep) = resume_episode.filter(|e| *e > 0) {
        return HeroAction::Continue(ep);
    }
    if status_kind(status) == Some(StatusKind::Upcoming) {
        HeroAction::Details
    } else {
        HeroAction::Watch
    }
}

#[component]
pub fn Spotlight(
    slides: Vec<HeroVm>,
    /// `(MediaVm::id, episode)` of Continue watching: those slides offer "Continue ep. N".
    #[props(default)]
    resume: Vec<(String, u32)>,
    /// Accessible name of the region.
    #[props(default)]
    label: String,
) -> Element {
    let mut active = use_signal(|| 0_usize);
    let motion = use_hero_motion();
    let ui = try_use_context::<Ui>();
    // 0.3.2: ani.zip artwork of the slides that lack it, once per mount; the slides swap when it arrives.
    let mut art = use_signal(HashMap::<String, HeroArtVm>::new);
    let services = try_use_context::<Services>();
    let wanted = art_wanted(&slides);
    use_hook(move || {
        if let (Some(services), false) = (services, wanted.is_empty()) {
            spawn(async move {
                if let Ok(found) = services.backend.hero_art(wanted).await {
                    art.set(found.into_iter().map(|a| (a.id.clone(), a)).collect());
                }
            });
        }
    });
    let n = slides.len();
    if n == 0 {
        return rsx! {};
    }
    let period = hero_autoplay(ui, true, n, (motion.os_reduced)());
    let hidden = (motion.hidden)();
    let cur = active() % n;
    let next = next_index(cur, n).unwrap_or(cur);
    let advance = move |_: ()| {
        let c = *active.peek();
        if let Some(k) = next_index(c, n) {
            active.set(k);
        }
    };
    rsx! {
        section {
            class: "tk-spot",
            role: "region",
            "aria-roledescription": t("home.hero.carousel"),
            "aria-label": "{label}",
            tabindex: "0",
            "data-count": "{n}",
            onkeydown: move |e| match e.key() {
                Key::ArrowRight => active.set(next_index(cur, n).unwrap_or(cur)),
                Key::ArrowLeft => active.set(prev_index(cur, n).unwrap_or(cur)),
                _ => {}
            },
            for (k , h) in slides.iter().enumerate() {
                SpotSlide {
                    key: "{h.media.id}",
                    slide: with_art(h, art.read().get(&h.media.id)),
                    active: k == cur,
                    eager: k == cur || k == next,
                    resume: resume.iter().find(|(id, _)| *id == h.media.id).map(|(_, ep)| *ep),
                }
            }
            if n > 1 {
                div { class: "tk-spot__segs",
                    HeroSegments {
                        n,
                        active: cur,
                        period_ms: period,
                        paused: hidden,
                        on_pick: move |k| active.set(k),
                        on_done: advance,
                    }
                }
            }
        }
    }
}

#[component]
fn SpotSlide(slide: HeroVm, active: bool, eager: bool, resume: Option<u32>) -> Element {
    let navigator = use_navigator();
    let m = slide.media.clone();
    let style = cover_color_var(m.color.as_deref());
    let mono = monogram(&m.title);
    let backdrop = hero_backdrop(&slide);
    let art_kind = backdrop.as_ref().map_or(HeroArt::Cover, |(k, _)| *k);
    let src = backdrop.map(|(_, u)| u);
    // The clear logo replaces the text title until it fails to load (then the text comes back).
    let mut logo_failed = use_signal(|| false);
    let logo = slide.logo.clone().filter(|l| !l.trim().is_empty() && !logo_failed());
    let has_logo = logo.is_some();
    let chips = hero_chips(&slide);
    let status = slide.status.clone().or_else(|| m.status.clone());
    let action = hero_action(status.as_deref(), resume);
    let (cta, cta_icon) = match action {
        HeroAction::Continue(ep) => (tf("hero.action.continue", &[("n", &ep.to_string())]), "play"),
        HeroAction::Watch => (t("hero.action.watch"), "play"),
        HeroAction::Details => (t("hero.action.details"), "info"),
    };
    let genres: Vec<String> = slide.genres.iter().take(MAX_GENRES).map(|g| genre_label(g)).collect();
    let synopsis = slide.synopsis.clone().filter(|s| !s.trim().is_empty());
    let id = m.id.clone();
    let open = move |id: String| {
        navigator.push(Route::DetailsPage { id });
    };
    let (id_art, id_title, id_info, id_cta) = (id.clone(), id.clone(), id.clone(), id.clone());
    rsx! {
        article {
            class: "tk-spot__slide",
            "data-active": "{active}",
            "data-art": art_kind.id(),
            "data-logo": "{has_logo}",
            "aria-hidden": "{!active}",
            style: "{style}",
            div { class: "tk-spot__art", onclick: move |_| open(id_art.clone()),
                div { class: "tk-cover__art", "data-mono": "{mono}" }
                if let Some(src) = src {
                    img {
                        class: "tk-spot__img",
                        src: "{src}",
                        alt: "",
                        loading: if eager { "eager" } else { "lazy" },
                        decoding: "async",
                        draggable: "false",
                    }
                }
            }
            div { class: "tk-spot__scrim" }
            div { class: "tk-spot__body",
                div { class: "tk-spot__main",
                    h2 { class: "tk-spot__title",
                        button {
                            r#type: "button",
                            tabindex: if active { "0" } else { "-1" },
                            onclick: move |_| open(id_title.clone()),
                            // QA 0.3: the clamp lives on a span (WebKit ignores -webkit-box on <button>, the title
                            // wrapped one word per line at min-content width).
                            if let Some(logo) = logo {
                                img {
                                    class: "tk-spot__logo",
                                    src: "{logo}",
                                    alt: "{m.title}",
                                    loading: if eager { "eager" } else { "lazy" },
                                    decoding: "async",
                                    draggable: "false",
                                    onerror: move |_| logo_failed.set(true),
                                }
                            } else {
                                span { class: "tk-spot__title-text", "{m.title}" }
                            }
                        }
                    }
                    if !chips.is_empty() {
                        ul { class: "tk-spot__chips",
                            for c in chips {
                                li { key: "{c.kind}", class: "tk-spot__chip", "data-kind": c.kind, "{c.text}" }
                            }
                        }
                    }
                    div { class: "tk-spot__actions",
                        Button {
                            label: cta,
                            icon: cta_icon.to_string(),
                            class: "tk-spot__cta".to_string(),
                            onclick: move |_| {
                                let id = id_cta.clone();
                                match action {
                                    HeroAction::Continue(episode) => {
                                        navigator.push(Route::PlayerPage { id, episode });
                                    }
                                    HeroAction::Watch => {
                                        navigator.push(Route::PlayerPage { id, episode: 1 });
                                    }
                                    HeroAction::Details => {
                                        navigator.push(Route::DetailsPage { id });
                                    }
                                }
                            },
                        }
                        // Only the visible slide asks for its list entry.
                        if active {
                            ListButton { media_id: id.clone(), episodes_total: slide.episodes, compact: true }
                        }
                        IconButton {
                            icon: "info".to_string(),
                            label: t("hero.action.info"),
                            variant: IconButtonVariant::Tonal,
                            class: "tk-spot__info".to_string(),
                            onclick: move |_| open(id_info.clone()),
                        }
                    }
                }
                if synopsis.is_some() || !genres.is_empty() {
                    div { class: "tk-spot__aside",
                        if let Some(s) = synopsis {
                            p { class: "tk-spot__syn", "{s}" }
                        }
                        if !genres.is_empty() {
                            ul { class: "tk-spot__genres",
                                for g in genres {
                                    li { key: "{g}", class: "tk-spot__chip", "data-kind": "genre", "{g}" }
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

    fn media(id: &str) -> MediaVm {
        MediaVm {
            id: id.to_owned(),
            title: id.to_uppercase(),
            status: Some("RELEASING".into()),
            format: Some("TV".into()),
            year: Some(2026),
            ..MediaVm::default()
        }
    }

    #[test]
    fn slides_prefer_the_hero_and_fall_back_to_cards() {
        let hero = vec![HeroVm { media: media("a"), banner: Some("b.jpg".into()), ..HeroVm::default() }];
        let cards = vec![media("x"), media("y"), media("z")];
        let s = hero_slides(&hero, &cards, 5);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].banner.as_deref(), Some("b.jpg"));
        let s = hero_slides(&[], &cards, 2);
        assert_eq!(s.iter().map(|h| h.media.id.as_str()).collect::<Vec<_>>(), ["x", "y"]);
        // The card facts carry over.
        assert_eq!(s[0].status.as_deref(), Some("RELEASING"));
        assert_eq!(s[0].year, Some(2026));
        assert!(hero_slides(&[], &[], 5).is_empty());
    }

    #[test]
    fn season_label_combines_what_is_known() {
        assert_eq!(season_label(Some("FALL"), Some(2026)), Some(format!("{} 2026", t("details.season.fall"))));
        assert_eq!(season_label(None, Some(2024)), Some("2024".into()));
        assert_eq!(season_label(Some("winter"), None), Some(t("details.season.winter")));
        assert_eq!(season_label(Some("  "), Some(0)), None);
        assert_eq!(season_label(Some("MONSOON"), Some(2020)), Some("MONSOON 2020".into()));
    }

    #[test]
    fn chips_follow_the_order_and_skip_unknowns() {
        let h = HeroVm {
            media: MediaVm { score: Some(78), ..media("a") },
            episodes: Some(12),
            status: Some("RELEASING".into()),
            season: Some("FALL".into()),
            year: Some(2026),
            format: Some("TV".into()),
            ..HeroVm::default()
        };
        let chips = hero_chips(&h);
        let kinds: Vec<&str> = chips.iter().map(|c| c.kind).collect();
        assert_eq!(kinds, ["episodes", "format", "status", "season", "score"]);
        assert_eq!(chips[0].text, tn("hero.chip.episodes", 12));
        assert_eq!(chips[1].text, t("details.format.tv"));
        assert_eq!(chips[2].text, t("details.status.releasing"));
        assert_eq!(chips[4].text, "78%");

        // Nothing but the card facts: format / status / year come from the media.
        let bare = HeroVm { media: media("b"), ..HeroVm::default() };
        let kinds: Vec<&str> = hero_chips(&bare).iter().map(|c| c.kind).collect();
        assert_eq!(kinds, ["format", "status", "season"]);
        let empty = HeroVm { media: MediaVm::default(), episodes: Some(0), ..HeroVm::default() };
        assert!(hero_chips(&empty).is_empty());
    }

    #[test]
    fn backdrop_prefers_fanart_then_banner_then_cover() {
        let cover = MediaVm { cover: Some("c.jpg".into()), ..media("a") };
        let h = HeroVm { media: cover.clone(), banner: Some("b.jpg".into()), fanart: Some("f.jpg".into()), ..HeroVm::default() };
        assert_eq!(hero_backdrop(&h), Some((HeroArt::Fanart, "f.jpg".into())));
        let h = HeroVm { fanart: Some(" ".into()), ..h };
        assert_eq!(hero_backdrop(&h), Some((HeroArt::Banner, "b.jpg".into())));
        let h = HeroVm { banner: None, ..h };
        assert_eq!(hero_backdrop(&h), Some((HeroArt::Cover, "c.jpg".into())));
        assert_eq!(hero_backdrop(&HeroVm::default()), None);
        assert_eq!(HeroArt::Banner.id(), "banner");
    }

    #[test]
    fn late_art_fills_only_what_is_missing() {
        let slides = vec![
            HeroVm { media: media("a"), ..HeroVm::default() },
            HeroVm { media: media("b"), fanart: Some("stored.jpg".into()), ..HeroVm::default() },
        ];
        assert_eq!(art_wanted(&slides), ["a"]);
        let art = HeroArtVm { id: "a".into(), fanart: Some("f.jpg".into()), logo: Some("l.png".into()) };
        let a = with_art(&slides[0], Some(&art));
        assert_eq!((a.fanart.as_deref(), a.logo.as_deref()), (Some("f.jpg"), Some("l.png")));
        let b = with_art(&slides[1], Some(&HeroArtVm { fanart: Some("new.jpg".into()), ..art }));
        assert_eq!(b.fanart.as_deref(), Some("stored.jpg"));
        assert_eq!(with_art(&slides[0], None), slides[0]);
    }

    #[test]
    fn primary_action_depends_on_progress_and_status() {
        assert_eq!(hero_action(Some("RELEASING"), Some(6)), HeroAction::Continue(6));
        assert_eq!(hero_action(Some("NOT_YET_RELEASED"), Some(2)), HeroAction::Continue(2));
        assert_eq!(hero_action(Some("NOT_YET_RELEASED"), None), HeroAction::Details);
        assert_eq!(hero_action(Some("FINISHED"), Some(0)), HeroAction::Watch);
        assert_eq!(hero_action(None, None), HeroAction::Watch);
    }
}
