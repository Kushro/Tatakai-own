//! `media.rs`: MediaCover (13:18), MediaCard and EpisodeRow.

use dioxus::prelude::*;

use crate::backend::{EpisodeVm, MediaVm};
use crate::components::chips::Pill;
use crate::i18n::{t, tf};

/// Only `#RGB` / `#RRGGBB` pass: the value comes from a remote metadata provider and lands in a
/// `style` attribute.
pub fn safe_css_color(c: &str) -> Option<&str> {
    let hex = c.strip_prefix('#')?;
    ((hex.len() == 3 || hex.len() == 6) && hex.chars().all(|ch| ch.is_ascii_hexdigit())).then_some(c)
}

#[component]
pub fn MediaCover(
    #[props(default)] src: Option<String>,
    #[props(default)] color: Option<String>,
    title: String,
) -> Element {
    let bg = color
        .as_deref()
        .and_then(safe_css_color)
        .map(|c| format!("background-color:{c}"))
        .unwrap_or_default();
    rsx! {
        div { class: "tk-cover", style: "{bg}",
            if let Some(src) = src {
                img { src: "{src}", alt: "{title}", loading: "lazy", decoding: "async", draggable: "false" }
            }
        }
    }
}

#[component]
pub fn MediaCard(
    media: MediaVm,
    #[props(default)] selected: bool,
    /// 0.0..=1.0; drawn as a bar under the poster when > 0.
    #[props(default)] progress: f64,
    #[props(default)] badge: String,
    #[props(default)] onclick: EventHandler<MouseEvent>,
) -> Element {
    let bar = (progress * 100.0).round();
    rsx! {
        button {
            class: "tk-card",
            r#type: "button",
            "data-selected": "{selected}",
            "aria-label": "{media.title}",
            onclick: move |e| onclick.call(e),
            div { class: "tk-card__cover",
                MediaCover { src: media.cover.clone(), color: media.color.clone(), title: media.title.clone() }
                if progress > 0.0 {
                    div { class: "tk-card__progress",
                        div { style: "width:{bar}%" }
                    }
                }
                if !badge.is_empty() {
                    span { class: "tk-card__badge", "{badge}" }
                }
                if let Some(score) = media.score {
                    span { class: "tk-card__score", "{score}" }
                }
            }
            div { class: "tk-card__title", "{media.title}" }
            if !media.subtitle.is_empty() {
                div { class: "tk-card__sub", "{media.subtitle}" }
            }
        }
    }
}

#[component]
pub fn EpisodeRow(
    episode: EpisodeVm,
    #[props(default)] current: bool,
    #[props(default)] onclick: EventHandler<MouseEvent>,
) -> Element {
    let n = episode.number.to_string();
    let heading = episode.title.clone().unwrap_or_else(|| tf("details.episode", &[("n", &n)]));
    let aired = episode.air_date.clone().unwrap_or_default();
    let bar = (episode.watched * 100.0).round();
    rsx! {
        button {
            class: "tk-ep",
            r#type: "button",
            "data-current": "{current}",
            onclick: move |e| onclick.call(e),
            div { class: "tk-ep__thumb",
                if let Some(img) = episode.image.clone() {
                    img { src: "{img}", alt: "", loading: "lazy", decoding: "async", draggable: "false" }
                }
                if episode.watched > 0.0 {
                    div { class: "tk-card__progress",
                        div { style: "width:{bar}%" }
                    }
                }
            }
            div { class: "tk-ep__text",
                div { class: "tk-ep__num", "{n}" }
                div { class: "tk-ep__body",
                    div { class: "tk-ep__title", "{heading}" }
                    if !aired.is_empty() {
                        div { class: "tk-ep__sub", "{aired}" }
                    }
                }
                if episode.is_filler {
                    Pill { text: t("details.filler"), tone: "warn".to_string() }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::safe_css_color;

    #[test]
    fn only_hex_colors_pass() {
        assert_eq!(safe_css_color("#a1b2c3"), Some("#a1b2c3"));
        assert_eq!(safe_css_color("#abc"), Some("#abc"));
        assert_eq!(safe_css_color("red"), None);
        assert_eq!(safe_css_color("#12345"), None);
        assert_eq!(safe_css_color("#fff;background:url(x)"), None);
    }
}
