//! Manga details `/manga/:id` (worker **l**; checklist `manga.details`, `manga.details.anime-link`,
//! `manga.chapters.providers`, `manga.chapters.auto-best`, `manga.chapters.tools`, `manga.readlist`,
//! `manga.download.chapters`, `manga.adult-gate`; classic `MangaPage.tsx`, `MangaChapterProviderRows.tsx`,
//! `MangaDownloadModal.tsx`).
//!
//! - Header: cover, title, status / format / chapters / score chips, synopsis, "Read" / "Continue chapter N"
//!   (best source of the chapter), the read-list status chips (`data-read-status`) with the chapters read, the
//!   anime adaptation (`data-anime-link`), "Download".
//! - Chapters tab: one source chip per provider (`data-provider`, the best one `data-best="true"`, failed ones
//!   `data-provider-error`) plus "Best source" (merged list, one row per chapter with its best source for the
//!   preferred language / source); tools: sort asc / desc (`data-sort`), filter, group by volume
//!   (`data-group-volume`), Refresh (bypasses the cache) and Clear cache.
//! - Info (synopsis, facts incl. MangaUpdates publisher / serialisation / publication status, cross-site ratings,
//!   tags, authors, alternative titles, links), Characters, Related (series tree: manga and anime relations,
//!   recommendations).
//! - 18+ titles: a gate with Continue / Always allow when `Settings::home.mature_content` is `warn`
//!   (`data-adult="true"`), a blurred cover with `blur`.

use std::collections::BTreeSet;

use dioxus::prelude::*;

use super::{MangaCardItem, adult_mode, allow_adult_always, read_status_attr, reader_route};
use crate::app::{Route, Services, Ui};
use crate::backend::{ChapterProviderVm, ChapterRefVm, ChapterVm, ListEntryUpdateVm, ListEntryVm, ListStatusVm, MangaDetailsVm, RelatedVm};
use crate::components::{
    Button, ButtonSize, ButtonVariant, Chip, EmptyState, ErrorState, IconButton, IconButtonVariant, InfoChip,
    MediaCover, Pill, Sheet, SkeletonLines, SkeletonRow, Switch, TextField, Tone, use_snackbar,
};
use crate::i18n::{t, tf};
use crate::icons::Icon;
use crate::model::home::MatureContentVm;
use crate::model::manga::{
    MergedChapterVm, best_provider, chapter_label, chapter_matches, group_by_volume, merge_chapters,
};
use crate::model::{ReadKindVm, UiError};

/// Synopsis length above which it is clamped with "More".
const SYNOPSIS_CLAMP: usize = 420;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Chapters,
    Info,
    Characters,
    Related,
}

impl Tab {
    const ALL: [Tab; 4] = [Tab::Chapters, Tab::Info, Tab::Characters, Tab::Related];

    fn id(self) -> &'static str {
        match self {
            Self::Chapters => "chapters",
            Self::Info => "info",
            Self::Characters => "characters",
            Self::Related => "related",
        }
    }
}

/// `YYYY-MM-DD` (UTC) of a unix time (civil-from-days, no time crate in tk-ui).
pub fn ymd(unix: i64) -> String {
    let z = unix.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// A label for a key whose translation may be missing (AniList values we do not translate fall back raw).
fn label_or(key: &str, raw: &str) -> String {
    let text = t(key);
    if text == key { raw.to_owned() } else { text }
}

/// Chapters of the view ("best source" merged rows or one provider), sorted and filtered.
fn visible_rows(providers: &[ChapterProviderVm], selected: &str, lang: &str, source: Option<&str>, asc: bool, filter: &str) -> Vec<MergedChapterVm> {
    let mut rows: Vec<MergedChapterVm> = if selected.is_empty() {
        merge_chapters(providers, lang, source)
    } else {
        providers
            .iter()
            .find(|p| p.ext_id == selected)
            .map(|p| {
                p.chapters
                    .iter()
                    .map(|c| MergedChapterVm { number: c.number, chapter: c.clone(), provider: p.ext_id.clone(), sources: 1 })
                    .collect()
            })
            .unwrap_or_default()
    };
    rows.retain(|r| chapter_matches(&r.chapter, filter));
    if !asc {
        rows.reverse();
    }
    rows
}

#[component]
pub fn MangaDetailsPage(id: String) -> Element {
    rsx! {
        div { class: "tk-page tk-page--details tk-manga-details",
            MangaDetailsView { key: "{id}", id: id.clone() }
        }
    }
}

#[component]
fn MangaDetailsView(id: String) -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let navigator = use_navigator();
    let backend = services.backend.clone();

    let mut details = use_resource({
        let (backend, id) = (backend.clone(), id.clone());
        move || {
            let (backend, id) = (backend.clone(), id.clone());
            async move { backend.manga_details(&id).await }
        }
    });
    let mut force = use_signal(|| false);
    let mut chapters = use_resource({
        let (backend, id) = (backend.clone(), id.clone());
        move || {
            let (backend, id) = (backend.clone(), id.clone());
            let refresh = *force.peek();
            if refresh {
                force.set(false);
            }
            async move { backend.manga_chapters(&id, refresh).await }
        }
    });
    let mut entry = use_resource({
        let (backend, id) = (backend.clone(), id.clone());
        move || {
            let (backend, id) = (backend.clone(), id.clone());
            async move { backend.read_list_entry(ReadKindVm::Manga, &id).await }
        }
    });
    let mut adult_session = use_signal(|| false);
    let mut tab = use_signal(|| Tab::Chapters);
    let mut download_open = use_signal(|| false);

    let state = details.read().clone();
    let vm = match state {
        None => {
            return rsx! {
                div { class: "tk-details", "aria-busy": "true",
                    div { class: "tk-skel tk-skel--header", "aria-hidden": "true" }
                    SkeletonLines {}
                    SkeletonRow { count: 6 }
                }
            };
        }
        Some(Err(e)) => {
            return rsx! {
                ErrorState { error: e, on_retry: move |_| details.restart() }
            };
        }
        Some(Ok(vm)) => vm,
    };

    let mode = adult_mode(&ui.settings.read());
    if vm.adult && mode == MatureContentVm::Warn && !adult_session() {
        let title = vm.media.title.clone();
        let services2 = services.clone();
        return rsx! {
            div { class: "tk-details tk-manga__gate", "data-adult": "true",
                EmptyState { icon: "warning".to_string(), title: t("manga.adult.title"), body: tf("manga.adult.body", &[("title", &title)]),
                    div { class: "tk-manga__gate-actions",
                        Button { label: t("common.back"), variant: ButtonVariant::Text, onclick: move |_| { navigator.go_back(); } }
                        Button { label: t("manga.adult.always"), variant: ButtonVariant::Outlined, onclick: move |_| {
                            allow_adult_always(ui, &services2);
                            adult_session.set(true);
                        } }
                        Button { label: t("manga.adult.continue"), onclick: move |_| adult_session.set(true) }
                    }
                }
            }
        };
    }

    let providers_state = chapters.read().clone();
    let providers: Vec<ChapterProviderVm> = providers_state.as_ref().and_then(|r| r.as_ref().ok()).cloned().unwrap_or_default();
    let prefs = ui.settings.read().manga.clone();
    let merged = merge_chapters(&providers, &prefs.preferred_lang, prefs.preferred_source.as_deref());
    let chapters_loading = providers_state.is_none();
    let chapters_body = match providers_state {
        None => rsx! {
            div { class: "tk-skel tk-skel--list", "aria-hidden": "true", "data-chapters": "loading" }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e, compact: true, on_retry: move |_| chapters.restart() }
        },
        Some(Ok(list)) => rsx! {
            ChaptersPanel {
                media_id: id.clone(),
                providers: list,
                on_refresh: move |_| {
                    force.set(true);
                    chapters.restart();
                },
                on_reload: move |_| chapters.restart(),
            }
        },
    };
    let blurred = vm.adult && mode == MatureContentVm::Blur;
    let tablist = t("manga.details.tabs");
    rsx! {
        div { class: "tk-details", "data-adult": "{vm.adult}", "data-blurred": "{blurred}", "data-manga-id": "{id}",
            Header {
                vm: vm.clone(),
                merged: merged.clone(),
                chapters_loading,
                entry: entry.read().clone(),
                on_entry: move |_| entry.restart(),
                on_download: move |_| download_open.set(true),
            }
            div { class: "tk-details__layout",
                div { class: "tk-section tk-details__main",
                    div { class: "tk-dtabs", role: "tablist", "aria-label": "{tablist}",
                        for tb in Tab::ALL {
                            button {
                                key: "{tb.id()}",
                                class: "tk-dtab",
                                r#type: "button",
                                role: "tab",
                                "data-tab": "{tb.id()}",
                                "aria-selected": if tab() == tb { "true" } else { "false" },
                                onclick: move |_| tab.set(tb),
                                {t(&format!("manga.details.tab.{}", tb.id()))}
                            }
                        }
                    }
                    div { class: "tk-details__tab", role: "tabpanel", hidden: tab() != Tab::Chapters,
                        {chapters_body}
                    }
                    div { class: "tk-details__tab", role: "tabpanel", hidden: tab() != Tab::Info,
                        InfoPanel { vm: vm.clone() }
                    }
                    div { class: "tk-details__tab", role: "tabpanel", hidden: tab() != Tab::Characters,
                        CharactersPanel { vm: vm.clone() }
                    }
                    div { class: "tk-details__tab", role: "tabpanel", hidden: tab() != Tab::Related,
                        RelatedPanel { vm: vm.clone() }
                    }
                }
            }
            if download_open() {
                DownloadSheet {
                    media_id: id.clone(),
                    merged: merged.clone(),
                    on_close: move |_| download_open.set(false),
                    on_done: move |_| {
                        download_open.set(false);
                        chapters.restart();
                    },
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Header
// ---------------------------------------------------------------------------------------------

#[component]
fn Header(
    vm: MangaDetailsVm,
    merged: Vec<MergedChapterVm>,
    chapters_loading: bool,
    entry: Option<Result<Option<ListEntryVm>, UiError>>,
    on_entry: EventHandler<()>,
    on_download: EventHandler<()>,
) -> Element {
    let navigator = use_navigator();
    let mut clamp = use_signal(|| true);
    let id = vm.media.id.clone();
    let synopsis = vm.synopsis.clone().unwrap_or_else(|| t("manga.details.no_synopsis"));
    let long = synopsis.chars().count() > SYNOPSIS_CLAMP;
    let clamped = long && clamp();

    // "Continue chapter N" at the stored position, else the first chapter's best source.
    let resume = vm.resume.clone();
    let (read_label, target): (String, Option<ChapterRefVm>) = match (&resume, merged.first()) {
        (Some(r), _) if !r.completed => (tf("manga.details.continue", &[("n", &chapter_label(r.chapter_number))]), Some(r.chapter.clone())),
        (Some(r), _) => match crate::model::manga::neighbour_chapter(&merged, r.chapter_number, true) {
            Some(next) => (tf("manga.details.read_n", &[("n", &chapter_label(next.number))]), Some(next.chapter.reference.clone())),
            None => (tf("manga.details.reread", &[("n", &chapter_label(r.chapter_number))]), Some(r.chapter.clone())),
        },
        (None, Some(first)) => (tf("manga.details.read_n", &[("n", &chapter_label(first.number))]), Some(first.chapter.reference.clone())),
        (None, None) if chapters_loading => (t("manga.details.loading_chapters"), None),
        (None, None) => (t("manga.details.no_chapters"), None),
    };
    let status = vm.status.clone().map(|s| label_or(&format!("manga.pub.{s}"), &s));
    let format = vm.format.clone().map(|f| label_or(&format!("manga.format.{}", f.to_ascii_lowercase()), &f));
    let no_target = target.is_none();
    let rid = id.clone();
    let anime_link = vm.anime_id.clone().map(|aid| {
        let open_id = aid.clone();
        rsx! {
            div { class: "tk-manga__anime-link", "data-anime-link": "{aid}",
                Button {
                    label: t("manga.details.anime"),
                    icon: "film".to_string(),
                    variant: ButtonVariant::Tonal,
                    onclick: move |_| {
                        navigator.push(Route::DetailsPage { id: open_id.clone() });
                    },
                }
            }
        }
    });
    rsx! {
        div { class: "tk-dhead tk-manga__head-card",
            div { class: "tk-dhead__cover tk-manga__cover",
                MediaCover { src: vm.media.cover.clone(), color: vm.media.color.clone(), title: vm.media.title.clone() }
            }
            div { class: "tk-dhead__text",
                h1 { class: "tk-dhead__title", "{vm.media.title}" }
                div { class: "tk-chips",
                    if let Some(s) = status {
                        Pill { text: s }
                    }
                    if let Some(f) = format {
                        InfoChip { text: f }
                    }
                    if let Some(n) = vm.chapters_total {
                        InfoChip { text: tf("manga.details.chapters_n", &[("n", &n.to_string())]) }
                    }
                    if let Some(n) = vm.volumes_total {
                        InfoChip { text: tf("manga.details.volumes_n", &[("n", &n.to_string())]) }
                    }
                    if let Some(sc) = vm.media.score {
                        InfoChip { text: tf("manga.details.score", &[("n", &sc.to_string())]), icon: "star".to_string() }
                    }
                    if vm.adult {
                        Pill { text: "18+".to_string(), tone: Tone::Warn }
                    }
                }
                if !vm.authors.is_empty() {
                    p { class: "tk-muted tk-manga__authors", "{vm.authors.join(\" · \")}" }
                }
                div { class: "tk-dhead__actions",
                    Button {
                        label: read_label,
                        icon: "play".to_string(),
                        disabled: no_target,
                        onclick: move |_| {
                            if let Some(c) = &target {
                                navigator.push(reader_route(&rid, c));
                            }
                        },
                    }
                    Button {
                        label: t("manga.details.download"),
                        icon: "download".to_string(),
                        variant: ButtonVariant::Outlined,
                        disabled: merged.is_empty(),
                        onclick: move |_| on_download.call(()),
                    }
                    {anime_link}
                }
                ReadStatus { media_id: id.clone(), chapters_total: vm.chapters_total, entry, on_change: on_entry }
                div { class: "tk-dhead__syn",
                    p { class: "tk-dhead__synopsis", "data-clamp": "{clamped}", "{synopsis}" }
                    if long {
                        Button {
                            label: if clamp() { t("common.more") } else { t("common.less") },
                            variant: ButtonVariant::Text,
                            size: ButtonSize::Xs,
                            onclick: move |_| clamp.set(!clamp()),
                        }
                    }
                }
            }
        }
    }
}

/// Read-list status chips + chapters read (`manga.readlist`, `data-read-status`).
#[component]
fn ReadStatus(media_id: String, chapters_total: Option<u32>, entry: Option<Result<Option<ListEntryVm>, UiError>>, on_change: EventHandler<()>) -> Element {
    let services = use_context::<Services>();
    let snackbar = use_snackbar();
    let mut busy = use_signal(|| false);
    let current: Option<ListEntryVm> = entry.as_ref().and_then(|r| r.as_ref().ok()).cloned().flatten();
    let status = current.as_ref().map(|e| e.status);
    let progress = current.as_ref().map_or(0, |e| e.progress);
    let save = use_callback({
        let backend = services.backend.clone();
        let id = media_id.clone();
        move |update: Option<ListEntryUpdateVm>| {
            let (backend, id) = (backend.clone(), id.clone());
            busy.set(true);
            spawn(async move {
                let r = match &update {
                    Some(u) => backend.set_read_list_entry(ReadKindVm::Manga, &id, u).await.map(|_| ()),
                    None => backend.remove_read_list_entry(ReadKindVm::Manga, &id).await,
                };
                busy.set(false);
                if let Err(e) = r {
                    snackbar.show(tf("manga.list.error", &[("detail", &e.detail)]));
                }
                on_change.call(());
            });
        }
    });
    let progress_text = match chapters_total {
        Some(total) => tf("manga.list.progress_of", &[("n", &progress.to_string()), ("total", &total.to_string())]),
        None => tf("manga.list.progress", &[("n", &progress.to_string())]),
    };
    rsx! {
        div {
            class: "tk-manga__status",
            "data-read-status": "{read_status_attr(status)}",
            "data-pending": "{current.as_ref().is_some_and(|e| e.pending)}",
            "aria-busy": "{busy()}",
            if entry.is_none() {
                SkeletonLines { widths: vec![60] }
            } else {
                if let Some(Err(e)) = entry.clone() {
                    ErrorState { error: e, compact: true, on_retry: move |_| on_change.call(()) }
                }
                div { class: "tk-chips", role: "group", "aria-label": t("manga.list.title"),
                    for s in ListStatusVm::ALL {
                        Chip {
                            key: "{s.id()}",
                            label: t(&format!("manga.status.{}", s.id())),
                            on: status == Some(s),
                            disabled: busy(),
                            onclick: move |_| save.call(Some(ListEntryUpdateVm { status: Some(s), ..ListEntryUpdateVm::default() })),
                        }
                    }
                    if status.is_some() {
                        IconButton {
                            icon: "close".to_string(),
                            label: t("manga.list.remove"),
                            disabled: busy(),
                            onclick: move |_| save.call(None),
                        }
                    }
                }
                if status.is_some() {
                    div { class: "tk-manga__progress", "data-read-progress": "{progress}",
                        span { class: "tk-muted", "{progress_text}" }
                        IconButton {
                            icon: "plus".to_string(),
                            label: t("manga.list.plus_one"),
                            variant: IconButtonVariant::Outlined,
                            disabled: busy(),
                            onclick: move |_| save.call(Some(ListEntryUpdateVm { progress: Some(progress + 1), ..ListEntryUpdateVm::default() })),
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Chapters
// ---------------------------------------------------------------------------------------------

#[component]
fn ChaptersPanel(media_id: String, providers: Vec<ChapterProviderVm>, on_refresh: EventHandler<()>, on_reload: EventHandler<()>) -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let navigator = use_navigator();
    let snackbar = use_snackbar();
    let prefs = ui.settings.read().manga.clone();
    let mut selected = use_signal(|| if prefs.auto_best_source { String::new() } else { prefs.preferred_source.clone().unwrap_or_default() });
    let mut asc = use_signal(|| false);
    let mut by_volume = use_signal(|| false);
    let mut filter = use_signal(String::new);
    let mut clearing = use_signal(|| false);

    let best = best_provider(&providers, &prefs.preferred_lang, prefs.preferred_source.as_deref());
    let failed: Vec<&ChapterProviderVm> = providers.iter().filter(|p| p.error.is_some()).collect();
    let any = providers.iter().any(|p| !p.chapters.is_empty());
    // A provider chosen earlier that vanished falls back to the merged view.
    let sel = {
        let s = selected();
        if s.is_empty() || providers.iter().any(|p| p.ext_id == s) { s } else { String::new() }
    };
    let rows = visible_rows(&providers, &sel, &prefs.preferred_lang, prefs.preferred_source.as_deref(), asc(), &filter());
    let groups = if by_volume() { group_by_volume(&rows, |r| r.chapter.volume) } else { vec![(None, rows.clone())] };

    let clear = {
        let backend = services.backend.clone();
        let id = media_id.clone();
        move |_| {
            let (backend, id) = (backend.clone(), id.clone());
            clearing.set(true);
            spawn(async move {
                match backend.read_clear_cache(ReadKindVm::Manga, &id).await {
                    Ok(()) => snackbar.show(t("manga.chapters.cache_cleared")),
                    Err(e) => snackbar.show(tf("manga.chapters.cache_error", &[("detail", &e.detail)])),
                }
                clearing.set(false);
                on_reload.call(());
            });
        }
    };

    rsx! {
        div { class: "tk-manga__chapters", "data-chapters": "{rows.len()}", "data-providers": "{providers.len()}",
            div { class: "tk-chips tk-manga__providers", role: "group", "aria-label": t("manga.chapters.sources"),
                Chip {
                    label: t("manga.chapters.best"),
                    icon: "bolt".to_string(),
                    on: sel.is_empty(),
                    onclick: move |_| selected.set(String::new()),
                }
                for (i , p) in providers.iter().enumerate() {
                    {
                        let key = p.ext_id.clone();
                        let label = match &p.lang {
                            Some(l) => format!("{} · {} · {}", p.ext_name, l.to_uppercase(), p.chapters.len()),
                            None => format!("{} · {}", p.ext_name, p.chapters.len()),
                        };
                        let pick = key.clone();
                        let is_best = best == Some(i);
                        let error = p.error.is_some();
                        rsx! {
                            span {
                                key: "{key}",
                                class: "tk-manga__provider",
                                "data-provider": "{key}",
                                "data-best": "{is_best}",
                                "data-provider-error": "{error}",
                                Chip {
                                    label,
                                    icon: if is_best { "star".to_string() } else if error { "warning".to_string() } else { String::new() },
                                    on: sel == key,
                                    disabled: error,
                                    onclick: move |_| selected.set(pick.clone()),
                                }
                            }
                        }
                    }
                }
            }
            if !failed.is_empty() {
                ul { class: "tk-manga__provider-errors",
                    for p in failed {
                        li { key: "{p.ext_id}", class: "tk-muted", "data-provider-failed": "{p.ext_id}",
                            Icon { name: "warning".to_string(), size: 16 }
                            {tf("manga.chapters.provider_failed", &[("name", &p.ext_name), ("detail", &p.error.as_ref().map(|e| e.detail.clone()).unwrap_or_default())])}
                        }
                    }
                }
            }
            div { class: "tk-manga__tools", "data-sort": if asc() { "asc" } else { "desc" },
                div { class: "tk-manga__filter-field",
                    TextField { label: t("manga.chapters.filter"), value: String::new(), icon: "search".to_string(), oninput: move |v: String| filter.set(v) }
                }
                Button {
                    label: if asc() { t("manga.chapters.sort_asc") } else { t("manga.chapters.sort_desc") },
                    icon: "sort".to_string(),
                    variant: ButtonVariant::Text,
                    class: "tk-manga__sort",
                    onclick: move |_| asc.set(!asc()),
                }
                label { class: "tk-manga__switch", "data-group-volume": "{by_volume()}",
                    Switch { checked: by_volume(), label: t("manga.chapters.group_volume"), on_toggle: move |v: bool| by_volume.set(v) }
                    span { {t("manga.chapters.group_volume")} }
                }
                Button { label: t("manga.chapters.refresh"), icon: "refresh".to_string(), variant: ButtonVariant::Text, onclick: move |_| on_refresh.call(()) }
                Button { label: t("manga.chapters.clear_cache"), icon: "trash".to_string(), variant: ButtonVariant::Text, disabled: clearing(), onclick: clear }
            }
            if !any {
                EmptyState {
                    icon: "extensions".to_string(),
                    title: t("manga.chapters.empty.title"),
                    body: t("manga.chapters.empty.body"),
                    action_label: t("manga.chapters.empty.action"),
                    on_action: move |_| {
                        navigator.push(Route::ExtensionsPage {});
                    },
                }
            } else if rows.is_empty() {
                EmptyState { icon: "search".to_string(), title: t("manga.chapters.none_match") }
            } else {
                for (vol , list) in groups {
                    div { key: "{vol:?}", class: "tk-manga__volume", "data-volume": "{vol.map(|v| v.to_string()).unwrap_or_default()}",
                        if by_volume() {
                            h4 { class: "tk-manga__volume-title",
                                {match vol { Some(v) => tf("manga.chapters.volume_n", &[("n", &v.to_string())]), None => t("manga.chapters.no_volume") }}
                            }
                        }
                        ul { class: "tk-list tk-manga__chapter-list",
                            for r in list {
                                ChapterRow {
                                    key: "{r.provider}-{r.chapter.reference.chapter_id}",
                                    media_id: media_id.clone(),
                                    row: r.clone(),
                                    provider_name: providers.iter().find(|p| p.ext_id == r.provider).map(|p| p.ext_name.clone()).unwrap_or_default(),
                                    on_changed: move |_| on_reload.call(()),
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
fn ChapterRow(media_id: String, row: MergedChapterVm, provider_name: String, on_changed: EventHandler<()>) -> Element {
    let services = use_context::<Services>();
    let navigator = use_navigator();
    let snackbar = use_snackbar();
    let ch: ChapterVm = row.chapter.clone();
    let label = tf("manga.chapter.n", &[("n", &chapter_label(row.number))]);
    let title = ch.title.clone().filter(|t| !t.trim().is_empty() && *t != label);
    let mut meta: Vec<String> = Vec::new();
    if let Some(l) = &ch.lang {
        meta.push(l.to_uppercase());
    }
    if let Some(g) = &ch.group {
        meta.push(g.clone());
    }
    if let Some(at) = ch.published_unix {
        meta.push(ymd(at));
    }
    if row.sources > 1 {
        meta.push(tf("manga.chapters.sources_n", &[("n", &row.sources.to_string())]));
    }
    if !provider_name.is_empty() {
        meta.push(provider_name.clone());
    }
    let read = ch.read >= 0.999;
    let reference = ch.reference.clone();
    let remove = {
        let backend = services.backend.clone();
        let reference = reference.clone();
        move |e: MouseEvent| {
            e.stop_propagation();
            let (backend, reference) = (backend.clone(), reference.clone());
            spawn(async move {
                match backend.read_remove_offline(ReadKindVm::Manga, &reference).await {
                    Ok(()) => snackbar.show(t("manga.download.removed")),
                    Err(e) => snackbar.show(tf("manga.download.error", &[("detail", &e.detail)])),
                }
                on_changed.call(());
            });
        }
    };
    rsx! {
        li {
            class: "tk-manga__chapter",
            "data-chapter": "{chapter_label(row.number)}",
            "data-read": "{read}",
            "data-downloaded": "{ch.downloaded}",
            "data-chapter-provider": "{row.provider}",
            button {
                class: "tk-manga__chapter-open",
                r#type: "button",
                onclick: move |_| {
                    navigator.push(reader_route(&media_id, &reference));
                },
                span { class: "tk-manga__chapter-num", "{label}" }
                span { class: "tk-manga__chapter-body",
                    if let Some(tt) = title {
                        span { class: "tk-manga__chapter-title", "{tt}" }
                    }
                    span { class: "tk-manga__chapter-meta tk-muted", "{meta.join(\" · \")}" }
                }
                if ch.read > 0.0 && !read {
                    span { class: "tk-manga__chapter-progress", style: "--tk-manga-read: {(ch.read * 100.0).round()}%" }
                }
                if read {
                    Icon { name: "check".to_string(), size: 18 }
                }
            }
            if ch.downloaded {
                IconButton { icon: "trash".to_string(), label: t("manga.download.remove"), onclick: remove }
            }
        }
    }
}

/// Chapter selection for offline reading (`manga.download.chapters`, `data-chapters-selectable`).
#[component]
fn DownloadSheet(media_id: String, merged: Vec<MergedChapterVm>, on_close: EventHandler<()>, on_done: EventHandler<()>) -> Element {
    let services = use_context::<Services>();
    let navigator = use_navigator();
    let snackbar = use_snackbar();
    let mut picked = use_signal(BTreeSet::<usize>::new);
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<UiError>);
    let open_downloads = use_callback(move |_: ()| {
        navigator.push(Route::DownloadsPage {});
    });
    let rows: Vec<MergedChapterVm> = merged.iter().filter(|m| m.number > 0.0).cloned().collect();
    let n = picked().len();
    let all = rows.len();
    let unread: BTreeSet<usize> = rows.iter().enumerate().filter(|(_, r)| r.chapter.read < 0.999 && !r.chapter.downloaded).map(|(i, _)| i).collect();
    let refs: Vec<ChapterRefVm> = picked().iter().filter_map(|i| rows.get(*i)).map(|r| r.chapter.reference.clone()).collect();
    let go = {
        let backend = services.backend.clone();
        let id = media_id.clone();
        move |_| {
            let (backend, id, refs) = (backend.clone(), id.clone(), refs.clone());
            busy.set(true);
            error.set(None);
            spawn(async move {
                match backend.manga_download_chapters(&id, &refs).await {
                    Ok(count) => {
                        snackbar.show_action(
                            tf("manga.download.queued", &[("n", &count.to_string())]),
                            t("manga.download.open"),
                            open_downloads,
                        );
                        on_done.call(());
                    }
                    Err(e) => error.set(Some(e)),
                }
                busy.set(false);
            });
        }
    };
    let footer = rsx! {
        Button { label: t("common.cancel"), variant: ButtonVariant::Text, onclick: move |_| on_close.call(()) }
        Button {
            label: tf("manga.download.go", &[("n", &n.to_string())]),
            icon: "download".to_string(),
            disabled: n == 0 || busy(),
            onclick: go,
        }
    };
    rsx! {
        Sheet { title: t("manga.download.title"), wide: true, on_close: move |_| on_close.call(()), footer,
            div { class: "tk-manga__download", "data-chapters-selectable": "{all}", "data-selected": "{n}",
                if let Some(e) = error() {
                    ErrorState { error: e, compact: true, retry: false }
                }
                if rows.is_empty() {
                    EmptyState { icon: "download".to_string(), title: t("manga.download.none") }
                } else {
                    div { class: "tk-manga__download-tools",
                        Button { label: t("manga.download.all"), variant: ButtonVariant::Text, onclick: move |_| picked.set((0..all).collect()) }
                        Button { label: t("manga.download.unread"), variant: ButtonVariant::Text, onclick: move |_| picked.set(unread.clone()) }
                        Button { label: t("manga.download.none_sel"), variant: ButtonVariant::Text, onclick: move |_| picked.set(BTreeSet::new()) }
                    }
                    ul { class: "tk-list tk-manga__download-list",
                        for (i , r) in rows.iter().enumerate() {
                            li { key: "{i}", class: "tk-manga__download-row", "data-downloaded": "{r.chapter.downloaded}",
                                label {
                                    input {
                                        r#type: "checkbox",
                                        checked: picked().contains(&i),
                                        onchange: move |e| {
                                            let mut s = picked.peek().clone();
                                            if e.checked() {
                                                s.insert(i);
                                            } else {
                                                s.remove(&i);
                                            }
                                            picked.set(s);
                                        },
                                    }
                                    span { class: "tk-manga__chapter-num", {tf("manga.chapter.n", &[("n", &chapter_label(r.number))])} }
                                    span { class: "tk-muted", "{r.chapter.title.clone().unwrap_or_default()}" }
                                    if r.chapter.downloaded {
                                        Pill { text: t("manga.download.done"), tone: Tone::Ok }
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

// ---------------------------------------------------------------------------------------------
// Info / characters / related
// ---------------------------------------------------------------------------------------------

#[component]
fn InfoPanel(vm: MangaDetailsVm) -> Element {
    let services = use_context::<Services>();
    let snackbar = use_snackbar();
    let synopsis = vm.synopsis.clone().unwrap_or_else(|| t("manga.details.no_synopsis"));
    rsx! {
        div { class: "tk-section tk-manga__info",
            section { class: "tk-panel",
                h3 { class: "tk-panel__title", {t("manga.details.synopsis")} }
                p { class: "tk-dhead__synopsis", "{synopsis}" }
            }
            if !vm.facts.is_empty() {
                section { class: "tk-panel", "data-facts": "{vm.facts.len()}",
                    h3 { class: "tk-panel__title", {t("manga.details.facts")} }
                    dl { class: "tk-manga__facts",
                        for (k , v) in vm.facts.clone() {
                            div { key: "{k}", class: "tk-manga__fact", "data-fact": "{k}",
                                dt { {label_or(&format!("manga.details.fact.{k}"), &k)} }
                                dd { "{v}" }
                            }
                        }
                    }
                }
            }
            if !vm.ratings.is_empty() {
                section { class: "tk-panel",
                    h3 { class: "tk-panel__title", {t("manga.details.ratings")} }
                    div { class: "tk-chips",
                        for (site , score) in vm.ratings.clone() {
                            span { key: "{site}", "data-rating-site": "{site}", "data-rating": "{score}",
                                InfoChip { text: format!("{site} · {score}%"), icon: "star".to_string() }
                            }
                        }
                    }
                }
            }
            if !vm.genres.is_empty() || !vm.tags.is_empty() {
                section { class: "tk-panel",
                    h3 { class: "tk-panel__title", {t("manga.details.tags")} }
                    div { class: "tk-chips",
                        for g in vm.genres.clone() {
                            Pill { key: "g-{g}", text: crate::screens::search::genre_label(&g), tone: Tone::Primary }
                        }
                        for (tag , rank) in vm.tags.clone() {
                            span { key: "t-{tag}", "data-tag-rank": "{rank}", InfoChip { text: tag.clone() } }
                        }
                    }
                }
            }
            if !vm.authors.is_empty() {
                section { class: "tk-panel",
                    h3 { class: "tk-panel__title", {t("manga.details.authors")} }
                    ul { class: "tk-manga__plain-list",
                        for a in vm.authors.clone() {
                            li { key: "{a}", "{a}" }
                        }
                    }
                }
            }
            if !vm.synonyms.is_empty() {
                section { class: "tk-panel",
                    h3 { class: "tk-panel__title", {t("manga.details.synonyms")} }
                    p { class: "tk-muted", "{vm.synonyms.join(\" · \")}" }
                }
            }
            if !vm.external.is_empty() {
                section { class: "tk-panel",
                    h3 { class: "tk-panel__title", {t("manga.details.links")} }
                    div { class: "tk-chips",
                        for (site , url) in vm.external.clone() {
                            {
                                let backend = services.backend.clone();
                                let target = url.clone();
                                rsx! {
                                    Chip {
                                        key: "{url}",
                                        label: site.clone(),
                                        icon: "external".to_string(),
                                        kind: crate::components::ChipKind::Assist,
                                        onclick: move |_| {
                                            let (backend, url) = (backend.clone(), target.clone());
                                            spawn(async move {
                                                if let Err(e) = backend.open_url(&url).await {
                                                    snackbar.show(tf("manga.details.link_error", &[("detail", &e.detail)]));
                                                }
                                            });
                                        },
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if !vm.source_ids.is_empty() {
                p { class: "tk-muted tk-manga__ids",
                    for (site , sid) in vm.source_ids.clone() {
                        span { key: "{site}", "data-source-id": "{site}", "{site}: {sid}" }
                    }
                }
            }
        }
    }
}

#[component]
fn CharactersPanel(vm: MangaDetailsVm) -> Element {
    let navigator = use_navigator();
    if vm.characters.is_empty() {
        return rsx! {
            EmptyState { icon: "user".to_string(), title: t("manga.details.no_characters") }
        };
    }
    rsx! {
        div { class: "tk-manga__characters", "data-characters": "{vm.characters.len()}",
            for c in vm.characters.clone() {
                {
                    let id = c.id.clone();
                    let role = label_or(&format!("manga.role.{}", c.role.to_ascii_lowercase()), &c.role);
                    rsx! {
                        button {
                            key: "{c.id}",
                            class: "tk-manga__character",
                            r#type: "button",
                            onclick: move |_| {
                                navigator.push(Route::CharacterPage { id: id.clone() });
                            },
                            MediaCover { src: c.image.clone(), title: c.name.clone() }
                            span { class: "tk-manga__character-name", "{c.name}" }
                            span { class: "tk-muted", "{role}" }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn RelatedPanel(vm: MangaDetailsVm) -> Element {
    let navigator = use_navigator();
    let empty = vm.relations.is_empty() && vm.recommendations.is_empty();
    if empty {
        return rsx! {
            EmptyState { icon: "library".to_string(), title: t("manga.details.no_related") }
        };
    }
    rsx! {
        div { class: "tk-section",
            if !vm.relations.is_empty() {
                section { class: "tk-panel", "data-relations": "{vm.relations.len()}",
                    h3 { class: "tk-panel__title", {t("manga.details.series")} }
                    ul { class: "tk-list tk-manga__relations",
                        for r in vm.relations.clone() {
                            RelationRow { key: "{r.media.id}", rel: r.clone(), on_open: move |r: RelatedVm| {
                                if r.kind == "anime" {
                                    navigator.push(Route::DetailsPage { id: r.media.id.clone() });
                                } else {
                                    navigator.push(Route::MangaDetailsPage { id: r.media.id.clone() });
                                }
                            } }
                        }
                    }
                }
            }
            if !vm.recommendations.is_empty() {
                section { class: "tk-panel",
                    h3 { class: "tk-panel__title", {t("manga.details.recommendations")} }
                    div { class: "tk-row",
                        for m in vm.recommendations.clone() {
                            MangaCardItem { key: "{m.id}", media: m.clone() }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn RelationRow(rel: RelatedVm, on_open: EventHandler<RelatedVm>) -> Element {
    let relation = label_or(&format!("details.relation.{}", rel.relation), &rel.relation);
    let kind = if rel.kind == "anime" { t("manga.details.kind_anime") } else { t("manga.details.kind_manga") };
    let icon = if rel.kind == "anime" { "film" } else { "grid" };
    let r = rel.clone();
    rsx! {
        li { class: "tk-drel", "data-relation-kind": "{rel.kind}", "data-relation": "{rel.relation}",
            button { class: "tk-manga__relation", r#type: "button", onclick: move |_| on_open.call(r.clone()),
                span { class: "tk-drel__ico", Icon { name: icon.to_string(), size: 20 } }
                span { class: "tk-drel__text",
                    span { class: "tk-drel__kind", "{relation} · {kind}" }
                    span { class: "tk-drel__title", "{rel.media.title}" }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::manga::read_fraction;

    #[test]
    fn dates_are_civil() {
        assert_eq!(ymd(0), "1970-01-01");
        assert_eq!(ymd(86_400 * 365), "1971-01-01");
        assert_eq!(ymd(1_700_000_000), "2023-11-14");
        assert_eq!(ymd(951_782_400), "2000-02-29");
    }

    #[test]
    fn rows_follow_selection_sort_and_filter() {
        let ch = |ext: &str, id: &str, n: f32| ChapterVm {
            reference: ChapterRefVm { ext_id: ext.into(), chapter_id: id.into() },
            number: n,
            volume: None,
            title: None,
            lang: Some("es".into()),
            group: None,
            published_unix: None,
            pages: None,
            read: 0.0,
            downloaded: false,
        };
        let providers = vec![
            ChapterProviderVm { ext_id: "a".into(), ext_name: "A".into(), lang: None, chapters: vec![ch("a", "1", 1.0), ch("a", "2", 2.0)], error: None },
            ChapterProviderVm { ext_id: "b".into(), ext_name: "B".into(), lang: None, chapters: vec![ch("b", "3", 3.0)], error: None },
        ];
        let merged = visible_rows(&providers, "", "es", None, true, "");
        assert_eq!(merged.iter().map(|r| r.number).collect::<Vec<_>>(), vec![1.0, 2.0, 3.0]);
        let desc = visible_rows(&providers, "", "es", None, false, "");
        assert_eq!(desc[0].number, 3.0);
        let only_b = visible_rows(&providers, "b", "es", None, true, "");
        assert_eq!(only_b.len(), 1);
        assert_eq!(visible_rows(&providers, "", "es", None, true, "2").len(), 1);
        assert_eq!(read_fraction(&crate::backend::ReadProgressVm {
            chapter: ChapterRefVm { ext_id: "a".into(), chapter_id: "1".into() },
            chapter_number: 1.0,
            position: 0,
            total: None,
            completed: true,
            updated_unix: 0,
        }), 1.0);
    }
}
