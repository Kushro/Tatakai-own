//! Manga (wave B, worker **l**; checklist `manga.*`; classic `pages/manga/*`): [`home`] (`/manga`: hero, rows,
//! genres, followed updates, 18+ zone, search with filters, the read list), [`details`] (`/manga/:id`) and
//! [`reader`] (`/manga/:id/read/:ext_id/:chapter_id`, bare route: no rail). l owns every file here. Data:
//! `Backend::manga_*`, `read_*` (shared with novels, m consumes them read-only).
//!
//! Shared pieces for other screens (mounted by the integrator, see the l report):
//! - [`open_search`]: the "Manga" type of Search hands its query to the manga home (`/manga` has no query segment;
//!   the query waits in [`take_pending_query`]).
//! - [`ReadListPanel`]: the read list by status (`manga.readlist`), for Library > Manga / Novels.
//! - [`MangaCardItem`]: a manga card that honours `Settings::home.mature_content` (`data-blurred`, confirm before
//!   opening an 18+ title, `manga.adult-gate`).

pub mod details;
pub mod home;
pub mod reader;

use std::sync::Mutex;

use base64::Engine;
use dioxus::prelude::*;
use dioxus::router::Navigator;

use crate::app::{Route, Services, Ui};
use crate::backend::{ChapterRefVm, ListEntryVm, ListStatusVm, MediaVm};
use crate::components::{ConfirmDialog, EmptyState, ErrorState, MediaCard, SegOption, Segmented, SkeletonGrid};
use crate::i18n::{t, tf};
use crate::model::Settings;
use crate::model::home::MatureContentVm;
use crate::model::manga::is_adult_card;
use crate::model::{ReadKindVm, SettingsAction, reduce_settings};

/// Prefix of an encoded chapter id in a reader route.
const ENCODED: &str = "c.";

/// Route-safe form of an opaque chapter id (extension ids may contain `/`, `:`, `?`...).
pub fn encode_chapter_id(id: &str) -> String {
    format!("{ENCODED}{}", base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(id.as_bytes()))
}

/// Inverse of [`encode_chapter_id`]; a plain id passes through.
pub fn decode_chapter_id(s: &str) -> String {
    s.strip_prefix(ENCODED)
        .and_then(|b| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(b).ok())
        .and_then(|v| String::from_utf8(v).ok())
        .unwrap_or_else(|| s.to_owned())
}

/// The reader route of a chapter.
pub fn reader_route(media_id: &str, chapter: &ChapterRefVm) -> Route {
    Route::MangaReaderPage {
        id: media_id.to_owned(),
        ext_id: chapter.ext_id.clone(),
        chapter_id: encode_chapter_id(&chapter.chapter_id),
    }
}

static PENDING_QUERY: Mutex<Option<String>> = Mutex::new(None);

/// Open the manga home with `query` in its search box (Search's "Manga" type; `manga.route`).
pub fn open_search(navigator: Navigator, query: &str) {
    if let Ok(mut q) = PENDING_QUERY.lock() {
        *q = Some(query.trim().to_owned()).filter(|q| !q.is_empty());
    }
    navigator.push(Route::MangaHomePage {});
}

/// The query handed over by [`open_search`] (taken once).
pub fn take_pending_query() -> Option<String> {
    PENDING_QUERY.lock().ok().and_then(|mut q| q.take())
}

/// 18+ handling of a manga card / page: `show`, `blur` or `warn` (mature content setting), with the reader's
/// "always allow" (`MangaPrefs::adult_ok`) lifting the warning.
pub fn adult_mode(s: &Settings) -> MatureContentVm {
    match s.home.mature_content {
        MatureContentVm::Warn if s.manga.adult_ok => MatureContentVm::Show,
        m => m,
    }
}

/// Persist "always show 18+ manga" (`MangaPrefs::adult_ok`).
pub fn allow_adult_always(ui: Ui, services: &Services) {
    let s = ui.settings.peek().clone();
    let mut m = s.manga.clone();
    m.adult_ok = true;
    let next = reduce_settings(&s, SettingsAction::SetManga(m));
    ui.commit_settings(&services.backend, next);
}

/// Manga card honouring the mature-content setting: blurred (`data-blurred`) or behind a confirmation.
#[component]
pub fn MangaCardItem(media: MediaVm, #[props(default)] progress: f64, #[props(default)] sub: String) -> Element {
    let ui = use_context::<Ui>();
    let services = use_context::<Services>();
    let navigator = use_navigator();
    let mut confirm = use_signal(|| false);
    let adult = is_adult_card(&media);
    let mode = adult_mode(&ui.settings.read());
    let blurred = adult && mode == MatureContentVm::Blur;
    let id = media.id.clone();
    let open = {
        let id = id.clone();
        move || {
            navigator.push(Route::MangaDetailsPage { id: id.clone() });
        }
    };
    let open2 = open.clone();
    rsx! {
        div {
            class: "tk-manga-card",
            "data-kind": "manga",
            "data-media-id": "{id}",
            "data-adult": "{adult}",
            "data-blurred": "{blurred}",
            MediaCard {
                media: media.clone(),
                progress,
                sub,
                onclick: move |_| {
                    if adult && mode == MatureContentVm::Warn {
                        confirm.set(true);
                    } else {
                        open();
                    }
                },
            }
            if confirm() {
                ConfirmDialog {
                    title: t("manga.adult.title"),
                    body: tf("manga.adult.body", &[("title", &media.title)]),
                    confirm_label: t("manga.adult.continue"),
                    icon: "warning".to_string(),
                    danger: false,
                    on_confirm: move |_| {
                        confirm.set(false);
                        open2();
                    },
                    on_cancel: move |_| confirm.set(false),
                    button {
                        class: "tk-btn tk-btn--text tk-btn--sm",
                        r#type: "button",
                        onclick: {
                            let services = services.clone();
                            move |_| {
                                allow_adult_always(ui, &services);
                                confirm.set(false);
                            }
                        },
                        {t("manga.adult.always")}
                    }
                }
            }
        }
    }
}

/// `data-read-status` of a list status (manga wording: reading / rereading).
pub fn read_status_attr(s: Option<ListStatusVm>) -> &'static str {
    match s {
        None => "none",
        Some(ListStatusVm::Watching) => "reading",
        Some(ListStatusVm::Planning) => "planning",
        Some(ListStatusVm::Completed) => "completed",
        Some(ListStatusVm::Paused) => "paused",
        Some(ListStatusVm::Dropped) => "dropped",
        Some(ListStatusVm::Repeating) => "rereading",
    }
}

/// The read list by status (`manga.readlist`): Reading / Planning / Completed / Paused / Dropped / Rereading tabs over
/// `Backend::read_list_entries(kind, status)`. `kind` = manga or novel (Library mounts both).
#[component]
pub fn ReadListPanel(kind: ReadKindVm) -> Element {
    let services = use_context::<Services>();
    let navigator = use_navigator();
    let mut status = use_signal(|| ListStatusVm::Watching);
    let backend = services.backend.clone();
    let mut entries = use_resource(move || {
        let backend = backend.clone();
        let st = status();
        async move { backend.read_list_entries(kind, Some(st)).await }
    });
    let options: Vec<SegOption> =
        ListStatusVm::ALL.iter().map(|s| (s.id().to_owned(), t(&format!("manga.status.{}", s.id())))).collect();
    let body = match &*entries.read() {
        None => rsx! {
            SkeletonGrid { count: 8 }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), on_retry: move |_| entries.restart() }
        },
        Some(Ok(list)) if list.is_empty() => rsx! {
            EmptyState {
                icon: "bookmark".to_string(),
                title: t("manga.list.empty.title"),
                body: t("manga.list.empty.body"),
            }
        },
        Some(Ok(list)) => {
            let list: Vec<ListEntryVm> = list.clone();
            rsx! {
                div { class: "tk-grid tk-manga-grid", "data-read-list": "{status().id()}",
                    for e in list {
                        {
                            let id = e.media.id.clone();
                            let progress = e.episodes_total.filter(|t| *t > 0).map_or(0.0, |t| f64::from(e.progress) / f64::from(t));
                            let sub = match e.episodes_total {
                                Some(total) => tf("manga.list.progress_of", &[("n", &e.progress.to_string()), ("total", &total.to_string())]),
                                None => tf("manga.list.progress", &[("n", &e.progress.to_string())]),
                            };
                            let pending = e.pending;
                            rsx! {
                                div {
                                    key: "{id}",
                                    class: "tk-manga-card",
                                    "data-kind": "{kind.id()}",
                                    "data-read-status": "{read_status_attr(Some(e.status))}",
                                    "data-pending": "{pending}",
                                    MediaCard {
                                        media: e.media.clone(),
                                        progress,
                                        sub,
                                        onclick: move |_| {
                                            if kind == ReadKindVm::Manga {
                                                navigator.push(Route::MangaDetailsPage { id: id.clone() });
                                            } else {
                                                navigator.push(Route::NovelDetailsPage { id: id.clone() });
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
    };
    rsx! {
        section { class: "tk-section tk-manga-list", "data-read-kind": "{kind.id()}",
            Segmented {
                options,
                selected: status().id().to_owned(),
                label: t("manga.list.title"),
                small: true,
                on_select: move |id: String| {
                    if let Some(s) = ListStatusVm::from_id(&id) {
                        status.set(s);
                    }
                },
            }
            {body}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chapter_ids_round_trip() {
        for id in ["mangadex:abc", "https://site/x/y?z=1", "ch 1 / 2", ""] {
            assert_eq!(decode_chapter_id(&encode_chapter_id(id)), id);
            assert!(!encode_chapter_id(id).contains('/'));
        }
        assert_eq!(decode_chapter_id("plain"), "plain");
    }

    #[test]
    fn adult_modes_and_status_attrs() {
        let mut s = Settings::default();
        s.home.mature_content = MatureContentVm::Warn;
        assert_eq!(adult_mode(&s), MatureContentVm::Warn);
        s.manga.adult_ok = true;
        assert_eq!(adult_mode(&s), MatureContentVm::Show);
        s.home.mature_content = MatureContentVm::Blur;
        assert_eq!(adult_mode(&s), MatureContentVm::Blur);
        assert_eq!(read_status_attr(Some(ListStatusVm::Watching)), "reading");
        assert_eq!(read_status_attr(None), "none");
    }

    #[test]
    fn pending_query_is_taken_once() {
        if let Ok(mut q) = PENDING_QUERY.lock() {
            *q = Some("berserk".into());
        }
        assert_eq!(take_pending_query().as_deref(), Some("berserk"));
        assert_eq!(take_pending_query(), None);
    }
}
