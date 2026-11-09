//! Search by image (wave A, worker **a**; classic trace.moe search): drop / pick an image (or a short clip) or paste
//! its URL, a confidence slider, and the matching scenes with their preview, episode and timestamp
//! (`Backend::search_by_image`). Each match opens the title or plays the episode from that minute (the position is
//! written to the history first, which is where the player resumes from). Route `/search/image`.
//!
//! Verifiable state (docs/redesign/15 section 5): the URL field is `[data-image-url]`, the results list
//! `[data-image-results]` and every match `[data-match][data-similarity]` (0.0..=1.0).

use base64::Engine;
use dioxus::prelude::*;

use crate::app::{Route, Services};
use crate::backend::{ImageMatchVm, ImageQueryVm};
use crate::components::{
    Button, ButtonSize, ButtonVariant, EmptyState, ErrorState, IconButton, LoadingIndicator, MediaCover, Pill, Tone,
};
use crate::i18n::{t, tf, tn};
use crate::icons::Icon;
use crate::model::{ErrorKind, UiError};

const FILE_INPUT_ID: &str = "tk-imgsearch-file";
/// trace.moe rejects larger uploads.
pub const MAX_IMAGE_BYTES: usize = 25 * 1024 * 1024;
/// Default minimum similarity shown (trace.moe: below ~87 % results are usually wrong).
pub const DEFAULT_MIN_SIMILARITY: u8 = 85;
/// Inline previews of picked images are skipped above this size (a data URL of the whole file).
const PREVIEW_MAX_BYTES: usize = 6 * 1024 * 1024;

/// MIME type of a picked file by extension (`None` = not something trace.moe takes).
pub fn mime_for(name: &str) -> Option<&'static str> {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase())?;
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" | "jfif" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "avif" => "image/avif",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        _ => return None,
    })
}

/// A pasted image URL, trimmed; `None` unless it is http(s).
pub fn valid_url(s: &str) -> Option<String> {
    let s = s.trim();
    ((s.starts_with("https://") || s.starts_with("http://")) && s.len() > 8 && !s.contains(char::is_whitespace))
        .then(|| s.to_owned())
}

/// `83.0` -> `1:23`, `3723.4` -> `1:02:03`.
pub fn clock(secs: f64) -> String {
    let s = if secs.is_finite() && secs > 0.0 { secs.floor() as u64 } else { 0 };
    let (h, m, s) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

/// Matches at or above `min_percent`, best first (the backend already sorts; kept stable).
pub fn visible_matches(all: &[ImageMatchVm], min_percent: u8) -> Vec<ImageMatchVm> {
    let min = f32::from(min_percent.min(100)) / 100.0;
    all.iter().filter(|m| m.similarity + f32::EPSILON >= min).cloned().collect()
}

/// A picked file, ready to send.
#[derive(Debug, Clone, PartialEq)]
struct Picked {
    name: String,
    mime: String,
    data: Vec<u8>,
    /// `data:` URL for the preview (small images only).
    preview: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum Phase {
    Idle,
    Loading,
    Ready(Vec<ImageMatchVm>),
    Failed(UiError),
}

fn local_error(msg: String) -> UiError {
    UiError::new(ErrorKind::Other, msg)
}

#[component]
pub fn ImageSearchPage() -> Element {
    let services = use_context::<Services>();
    let navigator = use_navigator();
    let mut url = use_signal(String::new);
    let mut picked = use_signal(|| None::<Picked>);
    let mut pick_error = use_signal(|| None::<String>);
    let mut min_similarity = use_signal(|| DEFAULT_MIN_SIMILARITY);
    let mut phase = use_signal(|| Phase::Idle);
    let mut generation = use_signal(|| 0_u64);
    let mut over = use_signal(|| false);
    let mut opening = use_signal(|| None::<String>);

    let take_file = use_callback(move |(name, bytes): (String, Result<Vec<u8>, String>)| {
        pick_error.set(None);
        let data = match bytes {
            Ok(d) => d,
            Err(e) => {
                pick_error.set(Some(tf("imgsearch.read_failed", &[("detail", &e)])));
                return;
            }
        };
        let Some(mime) = mime_for(&name) else {
            pick_error.set(Some(tf("imgsearch.unsupported", &[("name", &name)])));
            return;
        };
        if data.is_empty() {
            pick_error.set(Some(tf("imgsearch.read_failed", &[("detail", &name)])));
            return;
        }
        if data.len() > MAX_IMAGE_BYTES {
            pick_error.set(Some(t("imgsearch.too_big")));
            return;
        }
        let preview = (mime.starts_with("image/") && data.len() <= PREVIEW_MAX_BYTES)
            .then(|| format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(&data)));
        picked.set(Some(Picked { name, mime: mime.to_owned(), data, preview }));
        phase.set(Phase::Idle);
    });

    let run = use_callback({
        let backend = services.backend.clone();
        move |_: ()| {
            let query = match (picked.peek().clone(), valid_url(&url.peek())) {
                (Some(p), _) => ImageQueryVm::Bytes { data: p.data, mime: p.mime },
                (None, Some(u)) => ImageQueryVm::Url(u),
                (None, None) => {
                    phase.set(Phase::Failed(local_error(t("imgsearch.need_input"))));
                    return;
                }
            };
            let g = *generation.peek() + 1;
            generation.set(g);
            phase.set(Phase::Loading);
            let backend = backend.clone();
            spawn(async move {
                let r = backend.search_by_image(query).await;
                if *generation.peek() == g {
                    phase.set(match r {
                        Ok(list) => Phase::Ready(list),
                        Err(e) => Phase::Failed(e),
                    });
                }
            });
        }
    });

    // Play the matched episode from the scene: write the position, then open the player (it resumes there).
    let play_at = use_callback({
        let backend = services.backend.clone();
        move |(id, episode, secs): (String, u32, u32)| {
            opening.set(Some(id.clone()));
            let backend = backend.clone();
            spawn(async move {
                if let Err(e) = backend.record_progress(&id, episode, secs, None).await {
                    tracing::warn!(detail = %e.detail, "saving the scene position failed");
                }
                opening.set(None);
                navigator.push(Route::PlayerPage { id, episode });
            });
        }
    });

    let current = phase.read().clone();
    let busy = current == Phase::Loading;
    let file = picked.read().clone();
    let url_text = url.read().clone();
    let can_search = !busy && (file.is_some() || valid_url(&url_text).is_some());
    let min = min_similarity();
    let drop_label = t("imgsearch.drop");
    let phase_attr = match &current {
        Phase::Idle => "idle",
        Phase::Loading => "loading",
        Phase::Ready(_) => "ready",
        Phase::Failed(_) => "error",
    };

    rsx! {
        div { class: "tk-page tk-imgsearch", "data-phase": "{phase_attr}",
            div { class: "tk-page__head",
                IconButton {
                    icon: "back".to_string(),
                    label: t("common.back"),
                    onclick: move |_| {
                        navigator.go_back();
                    },
                }
                h1 { class: "tk-page__title", {t("imgsearch.title")} }
            }
            p { class: "tk-muted tk-imgsearch__intro", {t("imgsearch.intro")} }
            section { class: "tk-panel tk-imgsearch__input",
                label {
                    class: "tk-drop tk-imgsearch__drop",
                    r#for: "{FILE_INPUT_ID}",
                    role: "button",
                    tabindex: "0",
                    "aria-label": "{drop_label}",
                    "data-over": "{over()}",
                    "data-picked": "{file.is_some()}",
                    ondragenter: move |e| {
                        e.prevent_default();
                        over.set(true);
                    },
                    ondragover: move |e| {
                        e.prevent_default();
                        if !*over.peek() {
                            over.set(true);
                        }
                    },
                    ondragleave: move |_| over.set(false),
                    ondrop: move |e| {
                        e.prevent_default();
                        over.set(false);
                        match e.data_transfer().files().first().cloned() {
                            Some(f) => {
                                spawn(async move {
                                    let bytes = f.read_bytes().await.map(|b| b.to_vec()).map_err(|e| e.to_string());
                                    take_file.call((f.name(), bytes));
                                });
                            }
                            None => pick_error.set(Some(t("imgsearch.drop_invalid"))),
                        }
                    },
                    onkeydown: move |e| {
                        let activate = match e.key() {
                            Key::Enter => true,
                            Key::Character(c) => c == " ",
                            _ => false,
                        };
                        if activate {
                            e.prevent_default();
                            let _ = document::eval(&format!("const i=document.getElementById('{FILE_INPUT_ID}');if(i){{i.click();}}"));
                        }
                    },
                    match file.clone() {
                        Some(p) => rsx! {
                            if let Some(src) = p.preview.clone() {
                                img { class: "tk-imgsearch__preview", src: "{src}", alt: "{p.name}" }
                            } else {
                                Icon { name: "film".to_string(), size: 32 }
                            }
                            b { "{p.name}" }
                        },
                        None => rsx! {
                            Icon { name: "upload".to_string(), size: 32 }
                            b { "{drop_label}" }
                            span { class: "tk-drop__hint", {t("imgsearch.drop_hint")} }
                        },
                    }
                    input {
                        id: "{FILE_INPUT_ID}",
                        r#type: "file",
                        accept: "image/*,video/mp4,video/webm",
                        hidden: true,
                        onchange: move |e| {
                            if let Some(f) = e.files().first().cloned() {
                                spawn(async move {
                                    let bytes = f.read_bytes().await.map(|b| b.to_vec()).map_err(|e| e.to_string());
                                    take_file.call((f.name(), bytes));
                                });
                            }
                        },
                    }
                }
                if file.is_some() {
                    Button {
                        label: t("imgsearch.clear_file"),
                        variant: ButtonVariant::Text,
                        size: ButtonSize::Sm,
                        icon: "close".to_string(),
                        onclick: move |_| {
                            picked.set(None);
                            phase.set(Phase::Idle);
                        },
                    }
                }
                if let Some(err) = pick_error() {
                    p { class: "tk-imgsearch__error", role: "alert", "{err}" }
                }
                div { class: "tk-install__or", {t("imgsearch.or")} }
                label { class: "tk-field",
                    span { class: "tk-field__label", {t("imgsearch.url")} }
                    span { class: "tk-field__box",
                        Icon { name: "link".to_string(), size: 20 }
                        input {
                            class: "tk-field__input",
                            r#type: "url",
                            "data-image-url": "true",
                            placeholder: "https://…",
                            autocomplete: "off",
                            spellcheck: "false",
                            disabled: file.is_some(),
                            oninput: move |e| url.set(e.value()),
                            onkeydown: move |e| {
                                if e.key() == Key::Enter {
                                    run.call(());
                                }
                            },
                        }
                    }
                    if file.is_some() {
                        span { class: "tk-field__hint", {t("imgsearch.url_disabled")} }
                    }
                }
                div { class: "tk-imgsearch__controls",
                    label { class: "tk-imgsearch__slider",
                        span { {tf("imgsearch.min_similarity", &[("n", &min.to_string())])} }
                        input {
                            r#type: "range",
                            min: "50",
                            max: "100",
                            step: "1",
                            value: "{min}",
                            "data-min-similarity": "{min}",
                            "aria-label": "{t(\"imgsearch.min_similarity_label\")}",
                            oninput: move |e| {
                                if let Ok(v) = e.value().parse::<u8>() {
                                    min_similarity.set(v.clamp(50, 100));
                                }
                            },
                        }
                    }
                    Button {
                        label: t("imgsearch.search"),
                        icon: "search".to_string(),
                        disabled: !can_search,
                        onclick: move |_| run.call(()),
                    }
                }
            }
            match current {
                Phase::Idle => rsx! {
                    EmptyState { icon: "camera".to_string(), title: t("imgsearch.empty.title"), body: t("imgsearch.empty.body") }
                },
                Phase::Loading => rsx! {
                    div { class: "tk-imgsearch__loading", role: "status",
                        LoadingIndicator {}
                        p { class: "tk-muted", {t("imgsearch.searching")} }
                    }
                },
                Phase::Failed(e) => rsx! {
                    ErrorState { error: e, on_retry: move |_| run.call(()) }
                },
                Phase::Ready(all) => {
                    let shown = visible_matches(&all, min);
                    let hidden = all.len() - shown.len();
                    rsx! {
                        if shown.is_empty() {
                            div { "data-image-results": "0",
                                EmptyState {
                                    icon: "search".to_string(),
                                    title: t("imgsearch.none.title"),
                                    body: if hidden > 0 { tn("imgsearch.none.hidden", hidden) } else { t("imgsearch.none.body") },
                                    action_label: if hidden > 0 { t("imgsearch.show_all") } else { String::new() },
                                    on_action: move |_| min_similarity.set(50),
                                }
                            }
                        } else {
                            p { class: "tk-search__count tk-muted", role: "status", {tn("imgsearch.count", shown.len())} }
                            if hidden > 0 {
                                p { class: "tk-muted", {tn("imgsearch.hidden", hidden)} }
                            }
                            div { class: "tk-imgsearch__results", "data-image-results": "{shown.len()}",
                                for (i , m) in shown.into_iter().enumerate() {
                                    MatchCard {
                                        key: "{i}-{m.media.id}",
                                        index: i,
                                        item: m.clone(),
                                        opening: opening.read().as_deref() == Some(m.media.id.as_str()),
                                        on_details: move |id: String| {
                                            navigator.push(Route::DetailsPage { id });
                                        },
                                        on_play: move |args: (String, u32, u32)| play_at.call(args),
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

#[component]
fn MatchCard(
    index: usize,
    item: ImageMatchVm,
    opening: bool,
    on_details: EventHandler<String>,
    on_play: EventHandler<(String, u32, u32)>,
) -> Element {
    let m = item;
    let percent = (m.similarity * 100.0).round().clamp(0.0, 100.0) as u32;
    let similarity = format!("{:.3}", m.similarity);
    let tone = if percent >= 90 { Tone::Primary } else { Tone::default() };
    let scene = format!("{} – {}", clock(m.from_secs), clock(m.to_secs));
    let when = match m.episode {
        Some(ep) => tf("imgsearch.scene.episode", &[("ep", &ep.to_string()), ("time", &scene)]),
        None => tf("imgsearch.scene.movie", &[("time", &scene)]),
    };
    let details_id = m.media.id.clone();
    let play_id = m.media.id.clone();
    let start = m.from_secs.max(0.0).floor() as u32;
    // A movie / single-episode match plays episode 1.
    let episode = m.episode.unwrap_or(1).max(1);
    let play_label = tf("imgsearch.play_at", &[("time", &clock(m.from_secs))]);
    rsx! {
        article { class: "tk-imgsearch__match", "data-match": "{index}", "data-similarity": "{similarity}", "data-media-id": "{m.media.id}",
            div { class: "tk-imgsearch__scene",
                if let Some(v) = m.preview_video.clone() {
                    video {
                        src: "{v}",
                        poster: m.preview_image.clone().unwrap_or_default(),
                        controls: true,
                        muted: true,
                        preload: "none",
                        playsinline: true,
                    }
                } else if let Some(img_src) = m.preview_image.clone() {
                    img { src: "{img_src}", alt: "", loading: "lazy" }
                } else {
                    MediaCover { src: m.media.cover.clone(), color: m.media.color.clone(), title: m.media.title.clone() }
                }
            }
            div { class: "tk-imgsearch__meta",
                div { class: "tk-imgsearch__titlebar",
                    b { class: "tk-imgsearch__title", "{m.media.title}" }
                    Pill { text: format!("{percent}%"), tone }
                }
                span { class: "tk-muted", "{when}" }
                div { class: "tk-imgsearch__actions",
                    Button {
                        label: t("imgsearch.open_details"),
                        variant: ButtonVariant::Tonal,
                        size: ButtonSize::Sm,
                        onclick: move |_| on_details.call(details_id.clone()),
                    }
                    Button {
                        label: play_label,
                        icon: "play".to_string(),
                        size: ButtonSize::Sm,
                        disabled: opening,
                        onclick: move |_| on_play.call((play_id.clone(), episode, start)),
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

    fn hit(sim: f32) -> ImageMatchVm {
        ImageMatchVm {
            media: MediaVm { id: "21".into(), title: "x".into(), cover: None, color: None, subtitle: String::new(), score: None },
            episode: Some(1),
            from_secs: 1.0,
            to_secs: 2.0,
            similarity: sim,
            preview_image: None,
            preview_video: None,
        }
    }

    #[test]
    fn mime_and_url_checks() {
        assert_eq!(mime_for("Shot.PNG"), Some("image/png"));
        assert_eq!(mime_for("a.jpeg"), Some("image/jpeg"));
        assert_eq!(mime_for("clip.webm"), Some("video/webm"));
        assert_eq!(mime_for("doc.pdf"), None);
        assert_eq!(mime_for("noext"), None);
        assert_eq!(valid_url("  https://i.imgur.com/x.png "), Some("https://i.imgur.com/x.png".into()));
        assert_eq!(valid_url("ftp://x"), None);
        assert_eq!(valid_url("https://a b"), None);
        assert_eq!(valid_url("https://"), None);
    }

    #[test]
    fn clock_and_threshold() {
        assert_eq!(clock(83.9), "1:23");
        assert_eq!(clock(3723.4), "1:02:03");
        assert_eq!(clock(-5.0), "0:00");
        assert_eq!(clock(f64::NAN), "0:00");
        let all = vec![hit(0.97), hit(0.85), hit(0.6)];
        assert_eq!(visible_matches(&all, 85).len(), 2);
        assert_eq!(visible_matches(&all, 50).len(), 3);
        assert_eq!(visible_matches(&all, 100).len(), 0);
    }
}
