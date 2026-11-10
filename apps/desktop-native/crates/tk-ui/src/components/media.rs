//! `media.rs`: MediaCover (13:18 with a monogram placeholder), MediaCard, ContinueCard, EpisodeRow and the
//! home HeroCarousel (docs/redesign/07 section 2; prototype `ui.js::cover/card`, `screens.js::home`).

use std::time::Duration;

use dioxus::prelude::*;

use crate::app::Ui;
use crate::backend::{ContinueVm, EpisodeVm, MediaVm};
use crate::components::button::{Button, ButtonVariant};
use crate::components::chips::{Pill, Tone};
use crate::i18n::{t, tf};
use crate::icons::Icon;

/// Hero autoplay period (prototype: 9 s).
pub const HERO_AUTOPLAY_MS: u64 = 9000;

/// Sends `prefers-reduced-motion: reduce` now and on every change.
const REDUCED_MOTION_JS: &str = r#"
const q = window.matchMedia('(prefers-reduced-motion: reduce)');
dioxus.send(q.matches);
q.addEventListener('change', e => dioxus.send(e.matches));
"#;

/// Only `#RGB` / `#RRGGBB` pass: the value comes from a remote metadata provider and lands in a
/// `style` attribute.
pub fn safe_css_color(c: &str) -> Option<&str> {
    let hex = c.strip_prefix('#')?;
    ((hex.len() == 3 || hex.len() == 6) && hex.chars().all(|ch| ch.is_ascii_hexdigit())).then_some(c)
}

/// `--tk-cover-c:<color>` for the placeholder gradient, empty when the color is missing or unsafe
/// (CSS then falls back to a container token).
fn cover_color_var(color: Option<&str>) -> String {
    color.and_then(safe_css_color).map(|c| format!("--tk-cover-c:{c}")).unwrap_or_default()
}

/// Placeholder letters drawn big on the cover art: initials of the first two words ("Frieren: Beyond"
/// -> "FB"), or the first two characters of a single word.
pub fn monogram(title: &str) -> String {
    let words: Vec<&str> = title.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    match words.as_slice() {
        [] => String::new(),
        [one] => one.chars().take(2).collect::<String>().to_uppercase(),
        [a, b, ..] => a.chars().take(1).chain(b.chars().take(1)).collect::<String>().to_uppercase(),
    }
}

/// AniList-style 0..=100 score as the prototype shows it (`84` -> `8.4`).
pub fn format_score(score: u32) -> String {
    format!("{:.1}", f64::from(score.min(100)) / 10.0)
}

/// Poster (13:18). The art layer (poster color gradient + monogram) is always drawn and the image, if
/// any, covers it, so a slow or missing poster never leaves a blank box.
#[component]
pub fn MediaCover(
    #[props(default)] src: Option<String>,
    #[props(default)] color: Option<String>,
    title: String,
    /// Top-left label ("EP 5").
    #[props(default)] badge: String,
    /// 0..=100; drawn top-right as `8.4` with a star.
    #[props(default)] score: Option<u32>,
    /// 0.0..=1.0; drawn as a bar at the bottom when set.
    #[props(default)] progress: Option<f64>,
    /// Bottom-left caption over the fog ("Episode 5").
    #[props(default)] ep: String,
    /// Overrides the monogram (episode thumbnails show the episode number).
    #[props(default)] mono: Option<String>,
) -> Element {
    let style = cover_color_var(color.as_deref());
    let mono = mono.unwrap_or_else(|| monogram(&title));
    let bar = progress.map(|p| (p.clamp(0.0, 1.0) * 100.0).round());
    let score = score.map(format_score);
    rsx! {
        div { class: "tk-cover", style: "{style}",
            div { class: "tk-cover__art", "data-mono": "{mono}" }
            if let Some(src) = src {
                img { src: "{src}", alt: "{title}", loading: "lazy", decoding: "async", draggable: "false" }
            }
            div { class: "tk-cover__fog" }
            if !badge.is_empty() {
                span { class: "tk-cover__badge", "{badge}" }
            }
            if let Some(score) = score {
                span { class: "tk-cover__score",
                    Icon { name: "star".to_string(), size: 12 }
                    "{score}"
                }
            }
            if !ep.is_empty() {
                span { class: "tk-cover__ep", "{ep}" }
            }
            if let Some(bar) = bar {
                div { class: "tk-cover__progress",
                    i { style: "width:{bar}%" }
                }
            }
        }
    }
}

#[component]
pub fn MediaCard(
    media: MediaVm,
    #[props(default)] selected: bool,
    /// 0.0..=1.0; drawn as a bar on the poster when > 0.
    #[props(default)] progress: f64,
    #[props(default)] badge: String,
    /// Replaces `media.subtitle` ("Episode 3 · 12:40").
    #[props(default)] sub: String,
    /// 16:9 landscape card (260 px) instead of a 13:18 poster (150 px).
    #[props(default)] wide: bool,
    /// Hide the score even when `media.score` is set.
    #[props(default)] hide_score: bool,
    #[props(default)] onclick: EventHandler<MouseEvent>,
) -> Element {
    let sub = if sub.is_empty() { media.subtitle.clone() } else { sub };
    let progress = (progress > 0.0).then_some(progress);
    let score = if hide_score { None } else { media.score };
    rsx! {
        button {
            class: "tk-card",
            r#type: "button",
            "data-selected": "{selected}",
            "data-wide": "{wide}",
            "aria-label": "{media.title}",
            onclick: move |e| onclick.call(e),
            MediaCover {
                src: media.cover.clone(),
                color: media.color.clone(),
                title: media.title.clone(),
                badge,
                score,
                progress,
            }
            div { class: "tk-card__title", "{media.title}" }
            if !sub.is_empty() {
                div { class: "tk-card__sub", "{sub}" }
            }
        }
    }
}

/// "Continue watching" tile: small poster with progress, title, `Episode n · m min left`, a bar and a
/// play affordance. The whole tile is one button (`onclick` usually opens the player).
#[component]
pub fn ContinueCard(item: ContinueVm, #[props(default)] onclick: EventHandler<MouseEvent>) -> Element {
    let ep = tf("home.continue_episode", &[("n", &item.episode.to_string())]);
    let frac = item.fraction();
    let left = item
        .duration_secs
        .filter(|d| *d > item.progress_secs)
        .map(|d| (f64::from(d - item.progress_secs) / 60.0).round().to_string())
        .map(|m| tf("home.left", &[("m", &m)]));
    let sub = match left {
        Some(l) => format!("{ep} · {l}"),
        None => ep.clone(),
    };
    let bar = (frac * 100.0).round();
    let label = format!("{} · {ep}", item.media.title);
    rsx! {
        button { class: "tk-cont", r#type: "button", "aria-label": "{label}", onclick: move |e| onclick.call(e),
            MediaCover {
                src: item.media.cover.clone(),
                color: item.media.color.clone(),
                title: item.media.title.clone(),
                progress: Some(frac),
            }
            div { class: "tk-cont__body",
                div { class: "tk-cont__title", "{item.media.title}" }
                div { class: "tk-cont__sub", "{sub}" }
                div { class: "tk-cont__bar",
                    i { style: "width:{bar}%" }
                }
            }
            span { class: "tk-cont__play", "aria-hidden": "true",
                Icon { name: "play".to_string(), size: 20 }
            }
        }
    }
}

/// One episode line: 16:9 thumb (image, or poster-color art with the episode number), title with pills,
/// a sub line, an optional duration and a trailing icon.
///
/// Defaults reproduce the details list: title `"{n}. {title}"` (or `Episode n`), sub = air date, the
/// filler pill when `episode.is_filler`, and a progress bar on the thumb when `episode.watched > 0`.
#[component]
pub fn EpisodeRow(
    episode: EpisodeVm,
    /// Highlighted (resume episode); also sets `aria-current`.
    #[props(default)] current: bool,
    /// Thumbnail URL; `None` uses `episode.image`, then the placeholder art.
    #[props(default)] thumb: Option<String>,
    /// Poster color (`MediaVm::color`) for the placeholder art.
    #[props(default)] color: Option<String>,
    /// Extra pills after the title (`(text, tone)`, drawn plain), e.g. Recap / Watched. Filler is automatic.
    #[props(default)] pills: Vec<(String, Tone)>,
    /// Right-aligned mono text ("24:00"); empty hides it.
    #[props(default)] duration: String,
    /// Overrides the title line.
    #[props(default)] title: String,
    /// Overrides the sub line (default: `episode.air_date`).
    #[props(default)] sub: String,
    /// Icon name at the end of the row ("chevron-right").
    #[props(default)] trailing_icon: String,
    #[props(default)] onclick: EventHandler<MouseEvent>,
) -> Element {
    let n = episode.number.to_string();
    let heading = if !title.is_empty() {
        title
    } else {
        match &episode.title {
            Some(name) => format!("{n}. {name}"),
            None => tf("details.episode", &[("n", &n)]),
        }
    };
    let sub = if sub.is_empty() { episode.air_date.clone().unwrap_or_default() } else { sub };
    let image = thumb.or_else(|| episode.image.clone());
    let style = cover_color_var(color.as_deref());
    let bar = (episode.watched.clamp(0.0, 1.0) * 100.0).round();
    rsx! {
        button {
            class: "tk-ep",
            r#type: "button",
            "data-current": "{current}",
            "aria-current": "{current}",
            onclick: move |e| onclick.call(e),
            div { class: "tk-ep__thumb", style: "{style}",
                div { class: "tk-cover__art", "data-mono": "{n}" }
                if let Some(img) = image {
                    img { src: "{img}", alt: "", loading: "lazy", decoding: "async", draggable: "false" }
                }
                if episode.watched > 0.0 {
                    div { class: "tk-cover__progress",
                        i { style: "width:{bar}%" }
                    }
                }
            }
            div { class: "tk-ep__body",
                div { class: "tk-ep__title",
                    span { class: "tk-ep__name", "{heading}" }
                    if episode.is_filler {
                        Pill { text: t("details.filler"), tone: Tone::Warn, plain: true }
                    }
                    for (i , (text , tone)) in pills.into_iter().enumerate() {
                        Pill { key: "{i}", text, tone, plain: true }
                    }
                }
                if !sub.is_empty() {
                    div { class: "tk-ep__sub", "{sub}" }
                }
            }
            if !duration.is_empty() {
                span { class: "tk-ep__dur tk-num", "{duration}" }
            }
            if !trailing_icon.is_empty() {
                span { class: "tk-ep__end",
                    Icon { name: trailing_icon.clone(), size: 20 }
                }
            }
        }
    }
}

/// Multi-browse hero: the front slide is wide, the rest are narrow previews. Clicking a preview (or a
/// dot, or Left/Right) brings it to the front; clicking the front slide or its buttons calls `on_open`.
///
/// Autoplay rotates every [`HERO_AUTOPLAY_MS`] and pauses while hovered or focused, while Settings >
/// Reduce motion is on, or while the OS asks for reduced motion. `autoplay` and the item count are read
/// at mount: key the carousel by its items so a new list remounts it.
#[component]
pub fn HeroCarousel(
    items: Vec<MediaVm>,
    /// Label after the rank ("#1 Trending now") and accessible name of the carousel.
    #[props(default)] kicker: String,
    #[props(default = true)] autoplay: bool,
    #[props(default = true)] dots: bool,
    /// Primary button text (default `details.watch`; manga and novels pass "Leer", UX review #14).
    #[props(default)] cta: String,
    /// Called with `MediaVm::id`.
    #[props(default)] on_open: EventHandler<String>,
) -> Element {
    let cta = if cta.is_empty() { t("details.watch") } else { cta };
    let mut active = use_signal(|| 0_usize);
    let mut paused = use_signal(|| false);
    let mut os_reduced = use_signal(|| false);
    let ui = try_use_context::<Ui>();
    let n = items.len();

    use_future(move || async move {
        let mut eval = document::eval(REDUCED_MOTION_JS);
        while let Ok(v) = eval.recv::<bool>().await {
            os_reduced.set(v);
        }
    });

    use_future(move || async move {
        if !autoplay || n < 2 {
            return;
        }
        loop {
            tokio::time::sleep(Duration::from_millis(HERO_AUTOPLAY_MS)).await;
            let reduce = ui.is_some_and(|u| u.settings.peek().reduce_motion);
            if reduce || *paused.peek() || *os_reduced.peek() {
                continue;
            }
            let next = (*active.peek() + 1) % n;
            active.set(next);
        }
    });

    if n == 0 {
        return rsx! {};
    }
    let cur = active() % n;
    // Rotate so the active slide comes first (the layout makes the first slide wide).
    let slides: Vec<(bool, usize, MediaVm)> =
        (cur..n).chain(0..cur).enumerate().map(|(pos, k)| (pos == 0, k, items[k].clone())).collect();
    let dot_list: Vec<(usize, bool, String)> =
        (0..n).map(|k| (k, k == cur, tf("home.hero.slide", &[("n", &(k + 1).to_string())]))).collect();

    rsx! {
        div { class: "tk-hero-wrap",
            div {
                class: "tk-hero",
                role: "region",
                "aria-roledescription": t("home.hero.carousel"),
                "aria-label": "{kicker}",
                tabindex: "0",
                onmouseenter: move |_| paused.set(true),
                onmouseleave: move |_| paused.set(false),
                onfocusin: move |_| paused.set(true),
                onfocusout: move |_| paused.set(false),
                onkeydown: move |e| match e.key() {
                    Key::ArrowRight => active.set((cur + 1) % n),
                    Key::ArrowLeft => active.set((cur + n - 1) % n),
                    _ => {}
                },
                for (front , k , m) in slides {
                    HeroSlide {
                        key: "{m.id}",
                        media: m.clone(),
                        rank: k + 1,
                        kicker: kicker.clone(),
                        cta: cta.clone(),
                        front,
                        on_front: move |_| active.set(k),
                        on_open,
                    }
                }
            }
            if dots && n > 1 {
                div { class: "tk-hero__dots",
                    for (k , is_cur , label) in dot_list {
                        button {
                            key: "{k}",
                            class: "tk-hero__dot",
                            r#type: "button",
                            "aria-current": "{is_cur}",
                            "aria-label": "{label}",
                            title: "{label}",
                            onclick: move |_| active.set(k),
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn HeroSlide(
    media: MediaVm,
    rank: usize,
    kicker: String,
    cta: String,
    front: bool,
    on_front: EventHandler<()>,
    on_open: EventHandler<String>,
) -> Element {
    let style = cover_color_var(media.color.as_deref());
    let mono = monogram(&media.title);
    let (id_slide, id_watch, id_info) = (media.id.clone(), media.id.clone(), media.id.clone());
    rsx! {
        article {
            class: "tk-hero__slide",
            "data-front": "{front}",
            style: "{style}",
            onclick: move |_| {
                if front {
                    on_open.call(id_slide.clone());
                } else {
                    on_front.call(());
                }
            },
            div { class: "tk-cover__art", "data-mono": "{mono}" }
            if let Some(src) = media.cover.clone() {
                img { class: "tk-hero__img", src: "{src}", alt: "", loading: "lazy", decoding: "async", draggable: "false" }
            }
            div { class: "tk-hero__shade" }
            div { class: "tk-hero__body",
                div { class: "tk-hero__kicker",
                    Icon { name: "bolt".to_string(), size: 16 }
                    "#{rank} {kicker}"
                }
                h2 { class: "tk-hero__title", "{media.title}" }
                if !media.subtitle.is_empty() {
                    p { class: "tk-hero__sub", "{media.subtitle}" }
                }
                div { class: "tk-hero__actions",
                    Button {
                        label: cta.clone(),
                        icon: "play".to_string(),
                        onclick: move |e: MouseEvent| {
                            e.stop_propagation();
                            on_open.call(id_watch.clone());
                        },
                    }
                    Button {
                        label: t("common.details"),
                        icon: "info".to_string(),
                        variant: ButtonVariant::Tonal,
                        onclick: move |e: MouseEvent| {
                            e.stop_propagation();
                            on_open.call(id_info.clone());
                        },
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{format_score, monogram, safe_css_color};

    #[test]
    fn only_hex_colors_pass() {
        assert_eq!(safe_css_color("#a1b2c3"), Some("#a1b2c3"));
        assert_eq!(safe_css_color("#abc"), Some("#abc"));
        assert_eq!(safe_css_color("red"), None);
        assert_eq!(safe_css_color("#12345"), None);
        assert_eq!(safe_css_color("#fff;background:url(x)"), None);
    }

    #[test]
    fn monogram_takes_initials() {
        assert_eq!(monogram("Frieren: Beyond Journey's End"), "FB");
        assert_eq!(monogram("dandadan"), "DA");
        assert_eq!(monogram("  "), "");
        assert_eq!(monogram("86"), "86");
    }

    #[test]
    fn score_is_one_decimal() {
        assert_eq!(format_score(84), "8.4");
        assert_eq!(format_score(100), "10.0");
        assert_eq!(format_score(250), "10.0");
    }
}
