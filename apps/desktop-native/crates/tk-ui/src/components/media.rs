//! `media.rs`: MediaCover (13:18 with a monogram placeholder), MediaCard, ContinueCard, EpisodeRow and the
//! home HeroCarousel (docs/redesign/07 section 2; prototype `ui.js::cover/card`, `screens.js::home`).
//!
//! 2026-11 UI refresh: `MediaCard` renders the parts of the three card styles (`:root[data-cards]`: classic,
//! detailed, minimal) and media.css hides per style; the carousel keeps a fixed DOM order and moves only with
//! transform / opacity; [`HeroSegments`] is the timed progress bar shared with the spotlight (its CSS animation
//! drives autoplay, so hover / focus / a hidden window pause it without any Rust timer).

use dioxus::prelude::*;

use crate::app::Ui;
use crate::backend::{ContinueVm, EpisodeVm, MediaVm};
use crate::components::button::{Button, ButtonVariant};
use crate::components::chips::{Pill, Tone};
use crate::i18n::{t, tf, tn};
use crate::screens::search::genre_label;
use crate::icons::Icon;

/// Hero autoplay period when no settings are in context (prototype: 9 s).
pub const HERO_AUTOPLAY_MS: u64 = 9000;
/// Name of the `@keyframes` of the active progress segment (media.css); its `animationend` advances the hero.
const SEG_FILL_ANIMATION: &str = "tk-seg-fill";
/// Narrow previews visible next to the carousel stage.
const CAROUSEL_PEEKS: usize = 3;

/// Sends `[prefers-reduced-motion, document hidden]` now and on every change.
const HERO_MOTION_JS: &str = r#"
const q = window.matchMedia('(prefers-reduced-motion: reduce)');
const send = () => { try { dioxus.send([q.matches, document.visibilityState === 'hidden']); } catch (e) {} };
send();
q.addEventListener('change', send);
document.addEventListener('visibilitychange', send);
"#;

/// Only `#RGB` / `#RRGGBB` pass: the value comes from a remote metadata provider and lands in a
/// `style` attribute.
pub fn safe_css_color(c: &str) -> Option<&str> {
    let hex = c.strip_prefix('#')?;
    ((hex.len() == 3 || hex.len() == 6) && hex.chars().all(|ch| ch.is_ascii_hexdigit())).then_some(c)
}

/// `--tk-cover-c:<color>` for the placeholder gradient, empty when the color is missing or unsafe
/// (CSS then falls back to a container token).
pub(crate) fn cover_color_var(color: Option<&str>) -> String {
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

/// Status of a title for the card dot (`data-status`): AniList `MediaStatus` grouped in four.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusKind {
    Releasing,
    Finished,
    Upcoming,
    Other,
}

impl StatusKind {
    pub fn id(self) -> &'static str {
        match self {
            Self::Releasing => "releasing",
            Self::Finished => "finished",
            Self::Upcoming => "upcoming",
            Self::Other => "other",
        }
    }
}

/// `None` when the status is unknown (no dot is drawn).
pub fn status_kind(status: Option<&str>) -> Option<StatusKind> {
    let s = status.map(str::trim).filter(|s| !s.is_empty())?;
    Some(match s.to_ascii_uppercase().as_str() {
        "RELEASING" => StatusKind::Releasing,
        "FINISHED" => StatusKind::Finished,
        "NOT_YET_RELEASED" => StatusKind::Upcoming,
        _ => StatusKind::Other,
    })
}

/// `NOT_YET_RELEASED` -> "Not yet released" (labels without an i18n key).
fn humanize(raw: &str) -> String {
    let lower = raw.trim().replace('_', " ").to_lowercase();
    let mut chars = lower.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// `prefix.<raw lowercased>` from the catalog, else the raw value made readable.
fn enum_label(prefix: &str, raw: &str) -> String {
    let key = format!("{prefix}.{}", raw.trim().to_ascii_lowercase());
    let text = t(&key);
    if text == key { humanize(raw) } else { text }
}

/// AniList status (`RELEASING`, ...) as shown to the user (Details keys `details.status.*`).
pub fn status_label(raw: &str) -> String {
    enum_label("details.status", raw)
}

/// AniList format (`TV`, `TV_SHORT`, `MOVIE`, ...) as shown to the user (Details keys `details.format.*`).
pub fn format_label(raw: &str) -> String {
    enum_label("details.format", raw)
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

/// A poster card. Every card style's parts are in the DOM and media.css shows them per `:root[data-cards]`:
/// `classic` = title + sub line; `detailed` = status dot + title + "year … format" meta line (the sub line too when
/// the caller passed its own `sub`); `minimal` = poster only, the title slides up over it on hover / focus.
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
    /// 0.3.2: "Episode 5 · 2 h ago" of the episode rows, drawn prominently by the large / xl cards.
    #[props(default)] episode_line: String,
    #[props(default)] onclick: EventHandler<MouseEvent>,
) -> Element {
    let custom_sub = !sub.is_empty();
    let sub = if custom_sub { sub } else { media.subtitle.clone() };
    let progress = (progress > 0.0).then_some(progress);
    let score = if hide_score { None } else { media.score };
    let kind = status_kind(media.status.as_deref());
    let status_text = kind.and(media.status.as_deref()).map(status_label).unwrap_or_default();
    let format = media.format.as_deref().map(str::trim).filter(|f| !f.is_empty()).map(format_label);
    let year = media.year.filter(|y| *y > 0);
    let has_meta = format.is_some() || year.is_some();
    // Large / xl cards (`:root[data-card-size]`): facts line, score, genres and the synopsis when the row has it.
    let facts = card_facts(&status_text, format.as_deref(), year, media.episodes);
    let score_text = score.filter(|s| *s > 0).map(format_score);
    let genres: Vec<String> = media.genres.iter().take(CARD_GENRES).map(|g| genre_label(g)).collect();
    let synopsis = media.synopsis.as_deref().map(plain_synopsis).filter(|s| !s.is_empty());
    // The status is part of the accessible name (the dot itself is decorative inside the button).
    let label = if status_text.is_empty() {
        media.title.clone()
    } else {
        format!("{} · {status_text}", media.title)
    };
    rsx! {
        button {
            class: "tk-card",
            r#type: "button",
            "data-selected": "{selected}",
            "data-wide": "{wide}",
            "data-meta": "{has_meta}",
            "data-sub": if custom_sub { "custom" } else { "default" },
            "data-epline": "{!episode_line.is_empty()}",
            "aria-label": "{label}",
            onclick: move |e| onclick.call(e),
            div { class: "tk-card__poster",
                MediaCover {
                    src: media.cover.clone(),
                    color: media.color.clone(),
                    title: media.title.clone(),
                    badge,
                    score,
                    progress,
                }
                div { class: "tk-card__over", "aria-hidden": "true",
                    span { "{media.title}" }
                }
            }
            // `display: contents` for the poster sizes (the parts stay flex items of the card); the right column of
            // the large / xl horizontal cards.
            div { class: "tk-card__body",
            div { class: "tk-card__title",
                if let Some(k) = kind {
                    span {
                        class: "tk-card__dot",
                        "data-status": k.id(),
                        role: "img",
                        "aria-label": "{status_text}",
                        title: "{status_text}",
                    }
                }
                "{media.title}"
            }
            if !sub.is_empty() {
                div { class: "tk-card__sub", "{sub}" }
            }
            if has_meta {
                div { class: "tk-card__meta", "aria-hidden": "true",
                    span { class: "tk-card__year",
                        if let Some(y) = year {
                            Icon { name: "calendar".to_string(), size: 14 }
                            "{y}"
                        }
                    }
                    span { class: "tk-card__fmt",
                        if let Some(f) = format {
                            "{f}"
                            TvGlyph {}
                        }
                    }
                }
            }
            div { class: "tk-card__more",
                if !episode_line.is_empty() {
                    div { class: "tk-card__epline", "{episode_line}" }
                }
                if !facts.is_empty() || score_text.is_some() {
                    div { class: "tk-card__facts",
                        if let Some(k) = kind {
                            span { class: "tk-card__dot", "data-status": k.id(), "aria-hidden": "true" }
                        }
                        span { class: "tk-card__factstext", "{facts}" }
                        if let Some(sc) = score_text {
                            span { class: "tk-card__score",
                                Icon { name: "star".to_string(), size: 14 }
                                "{sc}"
                            }
                        }
                    }
                }
                if !genres.is_empty() {
                    ul { class: "tk-card__genres", "aria-hidden": "true",
                        for g in genres {
                            li { key: "{g}", "{g}" }
                        }
                    }
                }
                if let Some(syn) = synopsis {
                    p { class: "tk-card__syn", "{syn}" }
                }
            }
            }
        }
    }
}

/// Genre chips of a large / xl card.
const CARD_GENRES: usize = 3;

/// "Releasing · TV · 2026 · 12 ep" of the large / xl cards; empty parts are skipped.
pub fn card_facts(status: &str, format: Option<&str>, year: Option<i32>, episodes: Option<u32>) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !status.trim().is_empty() {
        parts.push(status.trim().to_owned());
    }
    if let Some(f) = format.map(str::trim).filter(|f| !f.is_empty()) {
        parts.push(f.to_owned());
    }
    if let Some(y) = year.filter(|y| *y > 0) {
        parts.push(y.to_string());
    }
    if let Some(n) = episodes.filter(|n| *n > 0) {
        parts.push(tn("hero.chip.episodes", n as usize));
    }
    parts.join(" · ")
}

/// Provider synopses can carry `<br>` / `<i>` tags: the card shows plain text.
pub fn plain_synopsis(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut in_tag = false;
    for c in raw.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// "2 h ago" / "3 d ago" of an aired episode, for the episode rows' cards.
pub fn aired_ago(aired_unix: i64, now_unix: i64) -> String {
    let secs = (now_unix - aired_unix).max(0);
    match secs {
        s if s < 60 => t("hero.ago.now"),
        s if s < 3600 => tf("hero.ago.min", &[("n", &(s / 60).to_string())]),
        s if s < 86_400 => tf("hero.ago.h", &[("n", &(s / 3600).to_string())]),
        s => tf("hero.ago.d", &[("n", &(s / 86_400).to_string())]),
    }
}

/// Small "screen" glyph after the format (not in `icons.rs`; local to the card).
#[component]
fn TvGlyph() -> Element {
    rsx! {
        svg {
            class: "tk-icon",
            view_box: "0 0 24 24",
            width: "14",
            height: "14",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "2",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            "aria-hidden": "true",
            rect { x: "3", y: "7", width: "18", height: "13", rx: "2" }
            path { d: "M8 3l4 4 4-4" }
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

/// OS reduced-motion preference and window visibility, tracked for the hero autoplay.
#[derive(Clone, Copy)]
pub struct HeroMotion {
    pub os_reduced: Signal<bool>,
    pub hidden: Signal<bool>,
}

/// [`HeroMotion`] of the calling component (one small listener script per mount).
pub fn use_hero_motion() -> HeroMotion {
    let mut os_reduced = use_signal(|| false);
    let mut hidden = use_signal(|| false);
    use_future(move || async move {
        let mut eval = document::eval(HERO_MOTION_JS);
        while let Ok((reduced, gone)) = eval.recv::<(bool, bool)>().await {
            if *os_reduced.peek() != reduced {
                os_reduced.set(reduced);
            }
            if *hidden.peek() != gone {
                hidden.set(gone);
            }
        }
    });
    HeroMotion { os_reduced, hidden }
}

/// Autoplay period of a hero with `n` slides, `None` when it must stay still: autoplay off in the props or in
/// `Settings::look`, a single slide, Settings > Reduce motion, or the OS asking for reduced motion.
pub fn hero_autoplay(ui: Option<Ui>, allowed: bool, n: usize, os_reduced: bool) -> Option<u64> {
    if !allowed || n < 2 || os_reduced {
        return None;
    }
    match ui {
        Some(u) => {
            let s = u.settings.read();
            if s.reduce_motion {
                None
            } else {
                s.look.hero_autoplay_ms()
            }
        }
        None => Some(HERO_AUTOPLAY_MS),
    }
}

/// Index after `cur` (wrapping), `None` with fewer than two slides.
pub fn next_index(cur: usize, n: usize) -> Option<usize> {
    (n > 1).then(|| (cur + 1) % n)
}

/// Index before `cur` (wrapping), `None` with fewer than two slides.
pub fn prev_index(cur: usize, n: usize) -> Option<usize> {
    (n > 1).then(|| (cur + n - 1) % n)
}

/// Timed progress segments under a hero. The active segment fills over `period_ms` with a CSS animation
/// (`transform: scaleX`); its `animationend` calls `on_done`. Hover / focus inside the hero and `paused` (window
/// hidden) freeze it through `animation-play-state`; with `period_ms = None` it is a plain static indicator.
#[component]
pub fn HeroSegments(
    n: usize,
    active: usize,
    period_ms: Option<u64>,
    #[props(default)] paused: bool,
    on_pick: EventHandler<usize>,
    #[props(default)] on_done: EventHandler<()>,
) -> Element {
    let run = period_ms.is_some();
    let dur = period_ms.unwrap_or(HERO_AUTOPLAY_MS);
    rsx! {
        div {
            class: "tk-segs",
            "data-run": "{run}",
            "data-paused": "{paused}",
            style: "--tk-seg-dur:{dur}ms",
            for k in 0..n {
                {
                    let label = tf("home.hero.slide", &[("n", &(k + 1).to_string())]);
                    let is_cur = k == active;
                    rsx! {
                        button {
                            key: "{k}",
                            class: "tk-segs__seg",
                            r#type: "button",
                            "aria-current": "{is_cur}",
                            "data-done": "{k < active}",
                            "aria-label": "{label}",
                            title: "{label}",
                            onclick: move |_| on_pick.call(k),
                            span {
                                class: "tk-segs__fill",
                                onanimationend: move |e: AnimationEvent| {
                                    if is_cur && e.data().animation_name() == SEG_FILL_ANIMATION {
                                        on_done.call(());
                                    }
                                },
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Multi-browse hero: a wide stage with the active slide and a strip of narrow previews. The DOM order never
/// changes: slides are stacked in the stage and cross-fade (opacity), the preview strip is a track moved with
/// `transform`. Clicking a preview (or a segment, or Left/Right) brings it to the stage; clicking the stage or
/// its buttons calls `on_open`.
///
/// Autoplay follows `Settings::look` (period, on/off) and stops under Reduce motion (app or OS); hovering or
/// focusing the hero and hiding the window pause it ([`HeroSegments`]). Key the carousel by its items so a new
/// list remounts it.
#[component]
pub fn HeroCarousel(
    items: Vec<MediaVm>,
    /// Wide artwork per item (banner); a missing entry uses the cover.
    #[props(default)]
    art: Vec<Option<String>>,
    /// Label after the rank ("#1 Trending now") and accessible name of the carousel.
    #[props(default)]
    kicker: String,
    #[props(default = true)] autoplay: bool,
    #[props(default = true)] dots: bool,
    /// Primary button text (default `details.watch`; manga and novels pass "Leer", UX review #14).
    #[props(default)]
    cta: String,
    /// Called with `MediaVm::id`.
    #[props(default)]
    on_open: EventHandler<String>,
) -> Element {
    let cta = if cta.is_empty() { t("details.watch") } else { cta };
    let mut active = use_signal(|| 0_usize);
    let motion = use_hero_motion();
    let ui = try_use_context::<Ui>();
    let n = items.len();
    if n == 0 {
        return rsx! {};
    }
    let period = hero_autoplay(ui, autoplay, n, (motion.os_reduced)());
    let cur = active() % n;
    let next = next_index(cur, n).unwrap_or(cur);
    let off = (cur + 1).min(n.saturating_sub(CAROUSEL_PEEKS));
    let advance = move |_: ()| {
        let c = *active.peek();
        if let Some(k) = next_index(c, n) {
            active.set(k);
        }
    };

    rsx! {
        div { class: "tk-hero-wrap",
            div {
                class: "tk-hero",
                role: "region",
                "aria-roledescription": t("home.hero.carousel"),
                "aria-label": "{kicker}",
                tabindex: "0",
                onkeydown: move |e| match e.key() {
                    Key::ArrowRight => active.set(next_index(cur, n).unwrap_or(cur)),
                    Key::ArrowLeft => active.set(prev_index(cur, n).unwrap_or(cur)),
                    _ => {}
                },
                div { class: "tk-hero__stage",
                    for (k , m) in items.iter().enumerate() {
                        HeroSlide {
                            key: "{m.id}",
                            media: m.clone(),
                            art: art.get(k).cloned().flatten(),
                            rank: k + 1,
                            kicker: kicker.clone(),
                            cta: cta.clone(),
                            active: k == cur,
                            eager: k == cur || k == next,
                            on_open,
                        }
                    }
                }
                if n > 1 {
                    div { class: "tk-hero__strip", "aria-hidden": "true",
                        div { class: "tk-hero__track", style: "--tk-hero-off:{off}",
                            for (k , m) in items.iter().enumerate() {
                                {
                                    let style = cover_color_var(m.color.as_deref());
                                    let mono = monogram(&m.title);
                                    rsx! {
                                        button {
                                            key: "{m.id}",
                                            class: "tk-hero__peek",
                                            r#type: "button",
                                            tabindex: "-1",
                                            "data-active": "{k == cur}",
                                            style: "{style}",
                                            onclick: move |_| active.set(k),
                                            div { class: "tk-cover__art", "data-mono": "{mono}" }
                                            if let Some(src) = m.cover.clone() {
                                                img {
                                                    class: "tk-hero__img",
                                                    src: "{src}",
                                                    alt: "",
                                                    loading: "lazy",
                                                    decoding: "async",
                                                    draggable: "false",
                                                }
                                            }
                                            div { class: "tk-hero__peek-shade" }
                                            span { class: "tk-hero__peek-title", "{m.title}" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if dots && n > 1 {
                HeroSegments {
                    n,
                    active: cur,
                    period_ms: period,
                    paused: (motion.hidden)(),
                    on_pick: move |k| active.set(k),
                    on_done: advance,
                }
            }
        }
    }
}

#[component]
fn HeroSlide(
    media: MediaVm,
    art: Option<String>,
    rank: usize,
    kicker: String,
    cta: String,
    active: bool,
    eager: bool,
    on_open: EventHandler<String>,
) -> Element {
    let style = cover_color_var(media.color.as_deref());
    let mono = monogram(&media.title);
    let (id_slide, id_watch, id_info) = (media.id.clone(), media.id.clone(), media.id.clone());
    let src = art.or_else(|| media.cover.clone());
    rsx! {
        article {
            class: "tk-hero__slide",
            "data-active": "{active}",
            "aria-hidden": "{!active}",
            style: "{style}",
            onclick: move |_| on_open.call(id_slide.clone()),
            div { class: "tk-cover__art", "data-mono": "{mono}" }
            if let Some(src) = src {
                img {
                    class: "tk-hero__img",
                    src: "{src}",
                    alt: "",
                    loading: if eager { "eager" } else { "lazy" },
                    decoding: "async",
                    draggable: "false",
                }
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
    #[test]
    fn large_card_facts_and_text() {
        assert_eq!(card_facts("Releasing", Some("TV"), Some(2026), Some(12)), format!("Releasing · TV · 2026 · {}", tn("hero.chip.episodes", 12)));
        assert_eq!(card_facts("", None, Some(0), Some(0)), "");
        assert_eq!(card_facts(" ", Some(" "), Some(2024), None), "2024");
        assert_eq!(plain_synopsis("A <i>quiet</i> mage.<br><br>\n Next  line"), "A quiet mage. Next line");
        assert_eq!(aired_ago(1000, 1030), t("hero.ago.now"));
        assert_eq!(aired_ago(0, 125 * 60), tf("hero.ago.h", &[("n", "2")]));
        assert_eq!(aired_ago(0, 3 * 86_400 + 5), tf("hero.ago.d", &[("n", "3")]));
        assert_eq!(aired_ago(100, 50), t("hero.ago.now"));
    }

    use super::*;

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

    #[test]
    fn status_maps_to_a_dot_kind() {
        assert_eq!(status_kind(Some("RELEASING")), Some(StatusKind::Releasing));
        assert_eq!(status_kind(Some("finished")), Some(StatusKind::Finished));
        assert_eq!(status_kind(Some("NOT_YET_RELEASED")), Some(StatusKind::Upcoming));
        assert_eq!(status_kind(Some("HIATUS")), Some(StatusKind::Other));
        assert_eq!(status_kind(Some(" ")), None);
        assert_eq!(status_kind(None), None);
        let ids: Vec<&str> = [StatusKind::Releasing, StatusKind::Finished, StatusKind::Upcoming, StatusKind::Other]
            .iter()
            .map(|k| k.id())
            .collect();
        assert_eq!(ids, ["releasing", "finished", "upcoming", "other"]);
    }

    #[test]
    fn labels_reuse_details_keys_and_humanize_unknowns() {
        assert_eq!(format_label("TV_SHORT"), t("details.format.tv_short"));
        assert_eq!(format_label("MOVIE"), t("details.format.movie"));
        assert_eq!(status_label("NOT_YET_RELEASED"), t("details.status.not_yet_released"));
        assert_eq!(format_label("WEIRD_THING"), "Weird thing");
        assert_eq!(humanize(""), "");
    }

    #[test]
    fn hero_indices_wrap_and_need_two_slides() {
        assert_eq!(next_index(0, 3), Some(1));
        assert_eq!(next_index(2, 3), Some(0));
        assert_eq!(prev_index(0, 3), Some(2));
        assert_eq!(next_index(0, 1), None);
        assert_eq!(prev_index(0, 0), None);
    }

    #[test]
    fn autoplay_stops_for_single_slides_and_reduced_motion() {
        assert_eq!(hero_autoplay(None, true, 5, false), Some(HERO_AUTOPLAY_MS));
        assert_eq!(hero_autoplay(None, false, 5, false), None);
        assert_eq!(hero_autoplay(None, true, 1, false), None);
        assert_eq!(hero_autoplay(None, true, 5, true), None);
    }
}
