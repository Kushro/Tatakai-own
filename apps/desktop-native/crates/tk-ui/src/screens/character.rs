//! Character page (wave A, worker **a**; classic `/character/:name`): portrait, names, bio facts (gender, age,
//! birthday, blood type, favourites), description, and the anime the character appears in with their voice actors
//! (`Backend::character`). Route `/character/:id` (AniList character id). Opened from Search ("Characters" type),
//! the characters feed of Browse and the Characters tab of Details (worker c).
//!
//! Verifiable state (docs/redesign/15 section 5): the page root carries `data-character-id`, the appearances list
//! `data-appearances` (count) and every voice actor `data-voice-actor` (AniList staff id).

use dioxus::prelude::*;

use crate::app::{Route, Services};
use crate::backend::{CharacterAppearanceVm, CharacterVm, PersonVm};
use crate::components::{
    Button, ButtonSize, ButtonVariant, Chip, EmptyState, ErrorState, IconButton, MediaCover, Pill, SkeletonLines, Tone,
};
use crate::i18n::{lang, lookup, t, tf, tn};
use crate::model::SKELETON_DELAY_MS;
use crate::screens::search::month_name;

/// Descriptions longer than this start collapsed behind "Show more".
const DESCRIPTION_FOLD: usize = 600;

/// Display text of an AniList fuzzy birthday in the current UI language (`character.birthday.*`); `None` when the
/// day or month alone would be meaningless. tk-app builds `CharacterVm::birthday` with it.
pub fn birthday_text(year: Option<i32>, month: Option<u32>, day: Option<u32>) -> Option<String> {
    let l = lang();
    let month_text = month.and_then(month_name);
    let day = day.filter(|d| (1..=31).contains(d));
    let year = year.filter(|y| (1..=9999).contains(y));
    let fill = |key: &str, m: &str, d: Option<u32>, y: Option<i32>| {
        let mut s = lookup(l, key).replace("{month}", m);
        if let Some(d) = d {
            s = s.replace("{day}", &d.to_string());
        }
        if let Some(y) = y {
            s = s.replace("{year}", &y.to_string());
        }
        s
    };
    match (month_text, day, year) {
        (Some(m), Some(d), Some(y)) => Some(fill("character.birthday.dmy", &m, Some(d), Some(y))),
        (Some(m), Some(d), None) => Some(fill("character.birthday.dm", &m, Some(d), None)),
        (Some(m), None, Some(y)) => Some(fill("character.birthday.my", &m, None, Some(y))),
        (Some(m), None, None) => Some(m),
        (None, _, Some(y)) => Some(y.to_string()),
        _ => None,
    }
}

/// i18n key of an appearance role (`MAIN | SUPPORTING | BACKGROUND`).
pub fn role_key(role: &str) -> String {
    match role.trim().to_ascii_lowercase().as_str() {
        r @ ("main" | "supporting" | "background") => format!("character.role.{r}"),
        _ => "character.role.background".to_owned(),
    }
}

/// Voice actor languages present in `appearances`, first-seen order (filter chips).
pub fn voice_languages(appearances: &[CharacterAppearanceVm]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for a in appearances {
        for va in &a.voice_actors {
            if let Some(l) = va.role.as_deref().map(str::trim).filter(|l| !l.is_empty())
                && !out.iter().any(|x| x == l)
            {
                out.push(l.to_owned());
            }
        }
    }
    out
}

/// The voice actors of `a` in `language` (all of them with `None`).
pub fn actors_in<'a>(a: &'a CharacterAppearanceVm, language: Option<&str>) -> Vec<&'a PersonVm> {
    a.voice_actors.iter().filter(|p| language.is_none_or(|l| p.role.as_deref() == Some(l))).collect()
}

/// Paragraphs of a description (blank lines split them).
pub fn paragraphs(text: &str) -> Vec<String> {
    text.split("\n\n").map(|p| p.trim().to_owned()).filter(|p| !p.is_empty()).collect()
}

#[component]
pub fn CharacterPage(id: String) -> Element {
    // Keyed so moving from one character to another remounts the view (it loads once per mount).
    rsx! {
        {std::iter::once(id).map(|id| rsx! {
            CharacterView { key: "{id}", id: id.clone() }
        })}
    }
}

#[component]
fn CharacterView(id: String) -> Element {
    let services = use_context::<Services>();
    let navigator = use_navigator();
    let mut character = use_resource({
        let (backend, id) = (services.backend.clone(), id.clone());
        move || {
            let (backend, id) = (backend.clone(), id.clone());
            async move { backend.character(&id).await }
        }
    });
    // Skeletons only after SKELETON_DELAY_MS (fast answers never flash one).
    let mut show_skeleton = use_signal(|| false);
    use_future(move || async move {
        tokio::time::sleep(std::time::Duration::from_millis(SKELETON_DELAY_MS)).await;
        show_skeleton.set(true);
    });

    let back = rsx! {
        IconButton {
            icon: "back".to_string(),
            label: t("common.back"),
            onclick: move |_| {
                navigator.go_back();
            },
        }
    };

    let state = character.read().clone();
    match state {
        None => rsx! {
            div { class: "tk-page tk-character", "data-character-id": "{id}", "data-loading": "true", "aria-busy": "true",
                div { class: "tk-page__head", {back} h1 { class: "tk-page__title", {t("character.title")} } }
                if show_skeleton() {
                    div { class: "tk-character__hero",
                        div { class: "tk-character__portrait tk-skel" }
                        SkeletonLines { widths: vec![60, 40, 80, 70] }
                    }
                }
            }
        },
        Some(Err(e)) => rsx! {
            div { class: "tk-page tk-character", "data-character-id": "{id}", "data-error": "true",
                div { class: "tk-page__head", {back} h1 { class: "tk-page__title", {t("character.title")} } }
                ErrorState { error: e, on_retry: move |_| character.restart() }
            }
        },
        Some(Ok(c)) => rsx! {
            CharacterBody { character: c, back }
        },
    }
}

#[component]
fn CharacterBody(character: CharacterVm, back: Element) -> Element {
    let navigator = use_navigator();
    let mut expanded = use_signal(|| false);
    let mut language = use_signal(|| None::<String>);
    let c = character;
    let languages = voice_languages(&c.appearances);
    let selected_lang = language.read().clone().filter(|l| languages.contains(l));
    let description = c.description.clone().unwrap_or_default();
    let long = description.chars().count() > DESCRIPTION_FOLD;
    let shown_text = if long && !expanded() {
        let cut: String = description.chars().take(DESCRIPTION_FOLD).collect();
        format!("{}…", cut.trim_end())
    } else {
        description.clone()
    };

    let mut facts: Vec<(String, String)> = Vec::new();
    if let Some(g) = &c.gender {
        facts.push((t("character.gender"), g.clone()));
    }
    if let Some(a) = &c.age {
        facts.push((t("character.age"), a.clone()));
    }
    if let Some(b) = &c.birthday {
        facts.push((t("character.birthday"), b.clone()));
    }
    if let Some(b) = &c.blood_type {
        facts.push((t("character.blood_type"), b.clone()));
    }
    if let Some(f) = c.favourites {
        facts.push((t("character.favourites"), f.to_string()));
    }
    let count = c.appearances.len();
    let alt = tf("character.portrait", &[("name", &c.name)]);

    rsx! {
        div { class: "tk-page tk-character", "data-character-id": "{c.id}",
            div { class: "tk-page__head", {back} h1 { class: "tk-page__title", "{c.name}" } }
            section { class: "tk-character__hero",
                div { class: "tk-character__portrait",
                    if let Some(src) = c.image.clone() {
                        img { src: "{src}", alt: "{alt}", loading: "lazy" }
                    } else {
                        MediaCover { src: None, color: None, title: c.name.clone() }
                    }
                }
                div { class: "tk-character__info",
                    if let Some(n) = c.native_name.clone() {
                        p { class: "tk-character__native", lang: "ja", "{n}" }
                    }
                    if !c.alternative_names.is_empty() {
                        div { class: "tk-character__alts", "aria-label": "{t(\"character.alternative_names\")}",
                            span { class: "tk-muted", {t("character.alternative_names")} }
                            div { class: "tk-chips",
                                for n in c.alternative_names.clone() {
                                    Pill { key: "{n}", text: n.clone(), plain: true }
                                }
                            }
                        }
                    }
                    if !facts.is_empty() {
                        dl { class: "tk-character__facts", "data-facts": "{facts.len()}",
                            for (k , v) in facts {
                                div { key: "{k}", class: "tk-character__fact",
                                    dt { "{k}" }
                                    dd { "{v}" }
                                }
                            }
                        }
                    }
                }
            }
            section { class: "tk-section tk-character__about",
                h2 { class: "tk-section__title tk-section__title--sm", {t("character.about")} }
                if description.is_empty() {
                    p { class: "tk-muted", {t("character.no_description")} }
                } else {
                    div { class: "tk-character__desc", "data-expanded": "{expanded()}",
                        for (i , para) in paragraphs(&shown_text).into_iter().enumerate() {
                            p { key: "{i}", "{para}" }
                        }
                    }
                    if long {
                        Button {
                            label: if expanded() { t("character.show_less") } else { t("character.show_more") },
                            variant: ButtonVariant::Text,
                            size: ButtonSize::Sm,
                            onclick: move |_| {
                                let next = !expanded();
                                expanded.set(next);
                            },
                        }
                    }
                }
            }
            section { class: "tk-section tk-character__media",
                div { class: "tk-section__head",
                    h2 { class: "tk-section__title tk-section__title--sm", {tn("character.appearances", count)} }
                }
                if languages.len() > 1 {
                    div { class: "tk-chips", role: "group", "aria-label": "{t(\"character.voice_language\")}",
                        Chip {
                            label: t("character.voice_all"),
                            on: selected_lang.is_none(),
                            onclick: move |_| language.set(None),
                        }
                        for l in languages.clone() {
                            {
                                let pick = l.clone();
                                let on = selected_lang.as_deref() == Some(l.as_str());
                                rsx! {
                                    Chip { key: "{l}", label: l.clone(), on, onclick: move |_| language.set(Some(pick.clone())) }
                                }
                            }
                        }
                    }
                }
                if c.appearances.is_empty() {
                    div { "data-appearances": "0",
                        EmptyState { icon: "film".to_string(), title: t("character.no_appearances.title"), body: t("character.no_appearances.body") }
                    }
                } else {
                    ul { class: "tk-character__list", "data-appearances": "{count}",
                        for a in c.appearances.clone() {
                            {
                                let media_id = a.media.id.clone();
                                let actors: Vec<PersonVm> = actors_in(&a, selected_lang.as_deref()).into_iter().cloned().collect();
                                rsx! {
                                    li { key: "{a.media.id}", class: "tk-character__row", "data-media-id": "{a.media.id}",
                                        button {
                                            class: "tk-character__title",
                                            r#type: "button",
                                            onclick: move |_| {
                                                navigator.push(Route::DetailsPage { id: media_id.clone() });
                                            },
                                            div { class: "tk-character__cover",
                                                MediaCover { src: a.media.cover.clone(), color: a.media.color.clone(), title: a.media.title.clone() }
                                            }
                                            div { class: "tk-character__titletext",
                                                b { "{a.media.title}" }
                                                if !a.media.subtitle.is_empty() {
                                                    span { class: "tk-muted", "{a.media.subtitle}" }
                                                }
                                                span {
                                                    Pill { text: t(&role_key(&a.role)), tone: if a.role.eq_ignore_ascii_case("main") { Tone::Primary } else { Tone::default() } }
                                                }
                                            }
                                        }
                                        if actors.is_empty() {
                                            p { class: "tk-character__noactors tk-muted", {t("character.no_voice_actors")} }
                                        } else {
                                            ul { class: "tk-character__actors", "aria-label": "{t(\"character.voice_actors\")}",
                                                for p in actors {
                                                    VoiceActor { key: "{p.id}", person: p.clone() }
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
    }
}

#[component]
fn VoiceActor(person: PersonVm) -> Element {
    let p = person;
    rsx! {
        li { class: "tk-character__actor", "data-voice-actor": "{p.id}",
            span { class: "tk-character__avatar",
                if let Some(src) = p.image.clone() {
                    img { src: "{src}", alt: "", loading: "lazy" }
                } else {
                    span { "aria-hidden": "true", {crate::components::monogram(&p.name)} }
                }
            }
            span { class: "tk-character__actorname",
                b { "{p.name}" }
                if let Some(r) = p.role.clone() {
                    span { class: "tk-muted", "{r}" }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::MediaVm;

    fn person(id: &str, lang: &str) -> PersonVm {
        PersonVm { id: id.into(), name: format!("P{id}"), image: None, role: Some(lang.into()) }
    }

    fn appearance(actors: Vec<PersonVm>) -> CharacterAppearanceVm {
        CharacterAppearanceVm {
            media: MediaVm { id: "21".into(), title: "One Piece".into(), cover: None, color: None, subtitle: String::new(), score: None },
            role: "MAIN".into(),
            voice_actors: actors,
        }
    }

    #[test]
    fn birthday_needs_a_month() {
        assert_eq!(birthday_text(None, None, Some(5)), None);
        assert_eq!(birthday_text(None, None, None), None);
        assert_eq!(birthday_text(Some(1990), None, None).as_deref(), Some("1990"));
        let dm = birthday_text(None, Some(5), Some(5)).unwrap();
        assert!(dm.contains('5') && !dm.contains('{'), "{dm}");
        let dmy = birthday_text(Some(1999), Some(12), Some(24)).unwrap();
        assert!(dmy.contains("1999") && dmy.contains("24") && !dmy.contains('{'), "{dmy}");
        assert_eq!(birthday_text(None, Some(13), Some(1)), None);
    }

    #[test]
    fn roles_languages_and_paragraphs() {
        assert_eq!(role_key("MAIN"), "character.role.main");
        assert_eq!(role_key("Supporting"), "character.role.supporting");
        assert_eq!(role_key("?"), "character.role.background");
        let list = vec![appearance(vec![person("1", "Japanese"), person("2", "English")]), appearance(vec![person("3", "Japanese")])];
        assert_eq!(voice_languages(&list), vec!["Japanese".to_owned(), "English".to_owned()]);
        assert_eq!(actors_in(&list[0], Some("English")).len(), 1);
        assert_eq!(actors_in(&list[0], None).len(), 2);
        assert_eq!(paragraphs("a\n\n\n\nb\n\n "), vec!["a".to_owned(), "b".to_owned()]);
    }
}
