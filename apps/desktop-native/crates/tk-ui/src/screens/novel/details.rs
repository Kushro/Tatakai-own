//! Novel details `/novels/:id` (worker **m**; checklist `novel.details`, `novel.details.check-sources`,
//! `novel.chapters`, `novel.readlist`, `novel.download.chapters`; classic `NovelPage.tsx`,
//! `NovelChapterProviderRows.tsx`, `NovelDownloadModal.tsx`, `useNovelReadlist.ts`): `Backend::novel_details(id)`
//! (synopsis, metadata, authors), chapters per provider with the translator filter (`novel_chapters`,
//! `data-provider`, `data-translator`), "Check all sources" (`novel_check_sources`, `data-sources-checked`), the
//! read list (`read_list_entry` / `set_read_list_entry` with `ReadKindVm::Novel`, `data-read-status`) and the
//! chapter download selection (`novel_download_chapters`).

use std::collections::HashSet;

use dioxus::prelude::*;

use crate::app::{Route, Services};
use crate::backend::{ChapterProviderVm, ChapterRefVm, ChapterVm, ListEntryUpdateVm, ListStatusVm, NovelDetailsVm};
use crate::components::{
    Button, ButtonSize, ButtonVariant, Chip, EmptyState, ErrorState, IconButton, LoadingIndicator, MediaCover, Pill,
    SkeletonLines, Tone, use_snackbar,
};
use crate::i18n::{t, tf};
use crate::icons::Icon;
use crate::model::novel::chapter_label;
use crate::model::{ReadKindVm, UiError};

/// `data-read-status` / select value of a list status (`none` = not listed).
pub fn read_status_id(s: Option<ListStatusVm>) -> &'static str {
    s.map_or("none", ListStatusVm::id)
}

/// `YYYY-MM-DD` of a unix time (UTC; enough for a release date).
pub fn date_of(unix: i64) -> String {
    let days = unix.div_euclid(86_400);
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{m:02}-{d:02}", if m <= 2 { y + 1 } else { y })
}

/// First chapter (lowest number) across providers: the "Start reading" target.
pub fn first_chapter(providers: &[ChapterProviderVm]) -> Option<ChapterRefVm> {
    providers
        .iter()
        .flat_map(|p| p.chapters.iter())
        .min_by(|a, b| a.number.total_cmp(&b.number))
        .map(|c| c.reference.clone())
}

#[component]
pub fn NovelDetailsPage(id: String) -> Element {
    let services = use_context::<Services>();
    let navigator = use_navigator();
    let id_sig = use_signal(|| id.clone());
    let mut details = use_resource({
        let backend = services.backend.clone();
        move || {
            let backend = backend.clone();
            let id = id_sig();
            async move { backend.novel_details(&id).await }
        }
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
    let body = match &*details.read() {
        None => rsx! {
            div { class: "tk-novel-details__skel", "aria-busy": "true",
                div { class: "tk-skel tk-skel--hero", "aria-hidden": "true" }
                SkeletonLines { widths: vec![70, 90, 60] }
            }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), on_retry: move |_| details.restart() }
        },
        Some(Ok(vm)) => rsx! {
            DetailsBody { key: "{vm.media.id}", vm: vm.clone() }
        },
    };
    rsx! {
        div { class: "tk-page tk-novel-details", "data-page": "novel.details", "data-novel-id": "{id}",
            div { class: "tk-page__head", {back} }
            {body}
        }
    }
}

#[component]
fn DetailsBody(vm: NovelDetailsVm) -> Element {
    let navigator = use_navigator();
    let id = vm.media.id.clone();
    let mut translator = use_signal(|| None::<String>);
    let mut refresh = use_signal(|| 0_u32);
    let mut expanded = use_signal(|| false);
    let services = use_context::<Services>();

    let mut chapters = use_resource({
        let backend = services.backend.clone();
        let id = id.clone();
        move || {
            let backend = backend.clone();
            let id = id.clone();
            let tl = translator();
            let fresh = refresh() > 0;
            async move { backend.novel_chapters(&id, tl.as_deref(), fresh).await }
        }
    });
    let providers: Vec<ChapterProviderVm> = chapters.read().as_ref().and_then(|r| r.as_ref().ok()).cloned().unwrap_or_default();
    // Translators known from the details (cached lists) plus those of the loaded chapters.
    let mut translators: Vec<String> = vm.translators.clone();
    for c in providers.iter().flat_map(|p| p.chapters.iter()) {
        if let Some(g) = &c.group
            && !translators.iter().any(|x| x.eq_ignore_ascii_case(g))
        {
            translators.push(g.clone());
        }
    }
    let first = first_chapter(&providers);
    let resume = vm.resume.clone();
    let read_target = resume.as_ref().map(|r| r.chapter.clone()).or(first.clone());
    let resume_label = match &resume {
        Some(r) if r.chapter_number > 0.0 => tf("novel.details.continue_n", &[("n", &chapter_label(r.chapter_number))]),
        Some(_) => t("novel.details.continue"),
        None => t("novel.details.start"),
    };
    let synopsis = vm.synopsis.clone().unwrap_or_default();
    let meta: Vec<(String, String)> = [
        vm.status.as_ref().map(|s| (t("novel.details.status"), t(&format!("novel.media_status.{}", s.to_ascii_lowercase())))),
        vm.chapters_total.map(|n| (t("novel.details.chapters"), n.to_string())),
        vm.volumes_total.map(|n| (t("novel.details.volumes"), n.to_string())),
        (!vm.authors.is_empty()).then(|| (t("novel.details.authors"), vm.authors.join(", "))),
    ]
    .into_iter()
    .flatten()
    .collect();
    let tl_now = translator();
    let tl_attr = tl_now.clone().unwrap_or_else(|| "all".into());
    let banner_style = vm.banner.as_ref().map(|b| format!("background-image:url('{}')", b.replace('\'', "%27"))).unwrap_or_default();
    let open_id = id.clone();

    rsx! {
        section { class: "tk-novel-details__hero", "data-adult": "{vm.adult}",
            div { class: "tk-novel-details__banner", style: "{banner_style}", "aria-hidden": "true" }
            div { class: "tk-novel-details__cover",
                MediaCover { src: vm.media.cover.clone(), color: vm.media.color.clone(), title: vm.media.title.clone(), score: vm.media.score }
            }
            div { class: "tk-novel-details__info",
                h1 { class: "tk-novel-details__title", "{vm.media.title}" }
                if !vm.media.subtitle.is_empty() {
                    p { class: "tk-muted", "{vm.media.subtitle}" }
                }
                div { class: "tk-novel-details__genres",
                    if vm.adult {
                        Pill { text: t("novel.details.adult"), tone: Tone::Warn }
                    }
                    for g in vm.genres.clone() {
                        Pill { key: "{g}", text: g.clone() }
                    }
                }
                dl { class: "tk-novel-details__meta",
                    for (k , v) in meta {
                        div { key: "{k}",
                            dt { "{k}" }
                            dd { "{v}" }
                        }
                    }
                }
                div { class: "tk-novel-details__actions",
                    if let Some(target) = read_target.clone() {
                        Button {
                            label: resume_label.clone(),
                            icon: "play".to_string(),
                            variant: ButtonVariant::Filled,
                            onclick: move |_| {
                                navigator.push(Route::NovelReaderPage {
                                    id: open_id.clone(),
                                    ext_id: target.ext_id.clone(),
                                    chapter_id: target.chapter_id.clone(),
                                });
                            },
                        }
                    }
                    ReadListControl { media_id: id.clone() }
                    CheckSources { media_id: id.clone() }
                }
            }
        }
        if !synopsis.is_empty() {
            section { class: "tk-section tk-novel-details__synopsis", "data-expanded": "{expanded()}",
                h2 { class: "tk-section__title", {t("novel.details.synopsis")} }
                p { class: "tk-novel-details__text", "{synopsis}" }
                if synopsis.chars().count() > 420 {
                    Button {
                        label: if expanded() { t("common.less") } else { t("common.more") },
                        variant: ButtonVariant::Text,
                        size: ButtonSize::Sm,
                        onclick: move |_| expanded.toggle(),
                    }
                }
            }
        }
        section { class: "tk-section tk-novel-details__chapters",
            div { class: "tk-section__head",
                h2 { class: "tk-section__title", {t("novel.chapters.title")} }
                IconButton {
                    icon: "refresh".to_string(),
                    label: t("novel.chapters.refresh"),
                    onclick: move |_| {
                        refresh += 1;
                        chapters.restart();
                    },
                }
            }
            if !translators.is_empty() {
                div { class: "tk-novel-details__translators", role: "group", "aria-label": t("novel.chapters.translator"), "data-translator": "{tl_attr}",
                    Chip { label: t("novel.chapters.all_translators"), on: tl_now.is_none(), onclick: move |_| translator.set(None) }
                    for name in translators.clone() {
                        {
                            let on = tl_now.as_deref().is_some_and(|x| x.eq_ignore_ascii_case(&name));
                            let pick = name.clone();
                            rsx! {
                                span { key: "{name}", "data-translator": "{name}",
                                    Chip { label: name.clone(), on, onclick: move |_| translator.set(Some(pick.clone())) }
                                }
                            }
                        }
                    }
                }
            }
            match &*chapters.read() {
                None => rsx! {
                    div { class: "tk-novel-details__loading", role: "status",
                        LoadingIndicator {}
                        p { class: "tk-muted", {t("novel.chapters.loading")} }
                    }
                },
                Some(Err(e)) => rsx! {
                    ErrorState { error: e.clone(), on_retry: move |_| chapters.restart() }
                },
                Some(Ok(list)) if list.is_empty() => rsx! {
                    EmptyState { icon: "extensions".to_string(), title: t("novel.chapters.none.title"), body: t("novel.chapters.none.body") }
                },
                Some(Ok(list)) => rsx! {
                    ChapterLists { media_id: id.clone(), providers: list.clone(), on_changed: move |_| chapters.restart() }
                },
            }
        }
    }
}

/// Read list status of the novel (AniList manga list mirror through the shared `read_*` calls).
#[component]
fn ReadListControl(media_id: String) -> Element {
    let services = use_context::<Services>();
    let snack = use_snackbar();
    let mid = use_signal(|| media_id.clone());
    let mut entry = use_resource({
        let backend = services.backend.clone();
        move || {
            let backend = backend.clone();
            let id = mid();
            async move { backend.read_list_entry(ReadKindVm::Novel, &id).await }
        }
    });
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<UiError>);
    let current = entry.read().as_ref().and_then(|r| r.as_ref().ok()).cloned().flatten().map(|e| e.status);
    let status_id = read_status_id(current);
    let loading = entry.read().is_none();
    let read_err = entry.read().as_ref().and_then(|r| r.as_ref().err()).cloned();
    let on_change = {
        let backend = services.backend.clone();
        move |e: FormEvent| {
            let v = e.value();
            let backend = backend.clone();
            let id = mid.peek().clone();
            busy.set(true);
            error.set(None);
            spawn(async move {
                let res = match ListStatusVm::ALL.into_iter().find(|s| s.id() == v) {
                    Some(s) => {
                        let up = ListEntryUpdateVm { status: Some(s), ..ListEntryUpdateVm::default() };
                        backend.set_read_list_entry(ReadKindVm::Novel, &id, &up).await.map(|_| ())
                    }
                    None => backend.remove_read_list_entry(ReadKindVm::Novel, &id).await,
                };
                busy.set(false);
                match res {
                    Ok(()) => {
                        snack.show(t("novel.readlist.saved"));
                        entry.restart();
                    }
                    Err(e) => error.set(Some(e)),
                }
            });
        }
    };
    rsx! {
        div { class: "tk-novel-readlist", "data-read-status": "{status_id}",
            label { class: "tk-novel__filter",
                span { class: "tk-novel__filter-label", {t("novel.readlist.label")} }
                select { value: "{status_id}", disabled: busy() || loading, onchange: on_change,
                    option { value: "none", selected: status_id == "none", {t("novel.readlist.none")} }
                    for s in ListStatusVm::ALL {
                        option { key: "{s.id()}", value: "{s.id()}", selected: status_id == s.id(), {t(&format!("novel.status.{}", s.id()))} }
                    }
                }
            }
            if busy() || loading {
                LoadingIndicator { small: true }
            }
            if let Some(e) = error() {
                ErrorState { error: e, compact: true, retry: false, title: t("novel.readlist.failed") }
            } else if let Some(e) = read_err {
                ErrorState { error: e, compact: true, title: t("novel.readlist.failed"), on_retry: move |_| entry.restart() }
            }
        }
    }
}

/// "Check all sources": asks every novel extension (full scan) and shows the chapters each provider has.
#[component]
fn CheckSources(media_id: String) -> Element {
    let services = use_context::<Services>();
    let mut running = use_signal(|| false);
    let mut result = use_signal(|| None::<Result<Vec<(String, u32)>, UiError>>);
    let run = {
        let backend = services.backend.clone();
        let id = media_id.clone();
        move |_| {
            if *running.peek() {
                return;
            }
            let backend = backend.clone();
            let id = id.clone();
            running.set(true);
            spawn(async move {
                let res = backend.novel_check_sources(&id).await;
                running.set(false);
                result.set(Some(res));
            });
        }
    };
    let checked = match &*result.read() {
        Some(Ok(list)) => list.iter().filter(|(_, n)| *n > 0).count().to_string(),
        _ => String::new(),
    };
    rsx! {
        div { class: "tk-novel-check", "data-sources-checked": "{checked}", "data-running": "{running()}",
            Button {
                label: t("novel.check.run"),
                icon: "sync".to_string(),
                variant: ButtonVariant::Tonal,
                disabled: running(),
                onclick: run,
            }
            if running() {
                span { class: "tk-novel-check__busy", role: "status",
                    LoadingIndicator { small: true }
                    span { class: "tk-muted", {t("novel.check.running")} }
                }
            }
            match result() {
                Some(Ok(list)) if list.is_empty() => rsx! {
                    p { class: "tk-muted", {t("novel.check.none")} }
                },
                Some(Ok(list)) => rsx! {
                    ul { class: "tk-novel-check__list",
                        for (name , n) in list {
                            li { key: "{name}", "data-provider": "{name}", "data-count": "{n}",
                                span { "{name}" }
                                Pill { text: tf("novel.check.count", &[("n", &n.to_string())]), tone: if n > 0 { Tone::Ok } else { Tone::Neutral } }
                            }
                        }
                    }
                },
                Some(Err(e)) => rsx! {
                    ErrorState { error: e, compact: true, title: t("novel.check.failed"), retry: false }
                },
                None => rsx! {},
            }
        }
    }
}

/// Chapter lists per provider with selection for download.
#[component]
fn ChapterLists(media_id: String, providers: Vec<ChapterProviderVm>, on_changed: EventHandler<()>) -> Element {
    let services = use_context::<Services>();
    let snack = use_snackbar();
    let mut selecting = use_signal(|| false);
    let mut selected = use_signal(HashSet::<ChapterRefVm>::new);
    let mut downloading = use_signal(|| false);
    let mut dl_error = use_signal(|| None::<UiError>);
    let count = selected.read().len();
    let download = {
        let backend = services.backend.clone();
        let id = media_id.clone();
        move |_| {
            let refs: Vec<ChapterRefVm> = selected.peek().iter().cloned().collect();
            if refs.is_empty() || *downloading.peek() {
                return;
            }
            let backend = backend.clone();
            let id = id.clone();
            downloading.set(true);
            dl_error.set(None);
            spawn(async move {
                let total = refs.len();
                let res = backend.novel_download_chapters(&id, &refs).await;
                downloading.set(false);
                match res {
                    Ok(n) => {
                        snack.show(tf("novel.download.done", &[("n", &n.to_string()), ("total", &total.to_string())]));
                        selected.set(HashSet::new());
                        selecting.set(false);
                        on_changed.call(());
                    }
                    Err(e) => dl_error.set(Some(e)),
                }
            });
        }
    };
    rsx! {
        div { class: "tk-novel-chapters", "data-selecting": "{selecting()}",
            div { class: "tk-novel-chapters__tools",
                if selecting() {
                    span { class: "tk-muted", role: "status", {tf("novel.download.selected", &[("n", &count.to_string())])} }
                    Button {
                        label: t("novel.download.run"),
                        icon: "download".to_string(),
                        variant: ButtonVariant::Filled,
                        size: ButtonSize::Sm,
                        disabled: count == 0 || downloading(),
                        onclick: download,
                    }
                    Button {
                        label: t("common.cancel"),
                        variant: ButtonVariant::Text,
                        size: ButtonSize::Sm,
                        onclick: move |_| {
                            selecting.set(false);
                            selected.set(HashSet::new());
                        },
                    }
                    if downloading() {
                        LoadingIndicator { small: true }
                    }
                } else {
                    Button {
                        label: t("novel.download.select"),
                        icon: "download".to_string(),
                        variant: ButtonVariant::Outlined,
                        size: ButtonSize::Sm,
                        onclick: move |_| selecting.set(true),
                    }
                }
            }
            if let Some(e) = dl_error() {
                ErrorState { error: e, compact: true, retry: false, title: t("novel.download.failed") }
            }
            for p in providers {
                ProviderRow { key: "{p.ext_id}-{p.ext_name}", media_id: media_id.clone(), provider: p.clone(), selecting: selecting(), selected }
            }
        }
    }
}

#[component]
fn ProviderRow(media_id: String, provider: ChapterProviderVm, selecting: bool, selected: Signal<HashSet<ChapterRefVm>>) -> Element {
    let mut open = use_signal(|| true);
    let mut newest_first = use_signal(|| false);
    let mut chapters: Vec<ChapterVm> = provider.chapters.clone();
    if newest_first() {
        chapters.reverse();
    }
    let n = provider.chapters.len();
    let lang = provider.lang.clone().unwrap_or_default();
    let all_refs: Vec<ChapterRefVm> = provider.chapters.iter().map(|c| c.reference.clone()).collect();
    rsx! {
        div { class: "tk-novel-provider", "data-provider": "{provider.ext_name}", "data-ext": "{provider.ext_id}", "data-open": "{open()}",
            div { class: "tk-novel-provider__head",
                button { class: "tk-novel-provider__toggle", r#type: "button", "aria-expanded": "{open()}", onclick: move |_| open.toggle(),
                    Icon { name: if open() { "chevron-down".to_string() } else { "chevron-right".to_string() }, size: 20 }
                    b { "{provider.ext_name}" }
                }
                if !lang.is_empty() {
                    Pill { text: lang.to_uppercase(), plain: true }
                }
                span { class: "tk-muted", {tf("novel.chapters.count", &[("n", &n.to_string())])} }
                IconButton {
                    icon: "sort".to_string(),
                    label: t("novel.chapters.sort"),
                    toggle: true,
                    toggled: newest_first(),
                    onclick: move |_| newest_first.toggle(),
                }
                if selecting && n > 0 {
                    Button {
                        label: t("novel.download.select_all"),
                        variant: ButtonVariant::Text,
                        size: ButtonSize::Sm,
                        onclick: move |_| {
                            let refs = all_refs.clone();
                            let mut sel = selected;
                            sel.with_mut(|s| s.extend(refs));
                        },
                    }
                }
            }
            if let Some(e) = provider.error.clone() {
                ErrorState { error: e, compact: true, retry: false, title: t("novel.chapters.provider_failed") }
            }
            if open() && !chapters.is_empty() {
                ul { class: "tk-novel-provider__list",
                    for c in chapters {
                        ChapterRow { key: "{c.reference.chapter_id}", media_id: media_id.clone(), chapter: c.clone(), selecting, selected }
                    }
                }
            }
        }
    }
}

#[component]
fn ChapterRow(media_id: String, chapter: ChapterVm, selecting: bool, selected: Signal<HashSet<ChapterRefVm>>) -> Element {
    let navigator = use_navigator();
    let r = chapter.reference.clone();
    let is_sel = selected.read().contains(&r);
    let label = tf("novel.chapter.n", &[("n", &chapter_label(chapter.number))]);
    let title = chapter.title.clone().unwrap_or_default();
    let date = chapter.published_unix.map(date_of).unwrap_or_default();
    let bar = (chapter.read * 100.0).round();
    let read_state = if chapter.read >= 0.99 { "read" } else if chapter.read > 0.0 { "partial" } else { "unread" };
    rsx! {
        li { class: "tk-novel-chapter", "data-read": "{read_state}", "data-downloaded": "{chapter.downloaded}", "data-chapter": "{chapter.number}",
            button {
                class: "tk-novel-chapter__open",
                r#type: "button",
                "aria-pressed": if selecting { if is_sel { "true" } else { "false" } } else { "" },
                onclick: move |_| {
                    if selecting {
                        let r = r.clone();
                        let mut sel = selected;
                        sel.with_mut(|s| {
                            if !s.remove(&r) {
                                s.insert(r);
                            }
                        });
                    } else {
                        navigator.push(Route::NovelReaderPage { id: media_id.clone(), ext_id: r.ext_id.clone(), chapter_id: r.chapter_id.clone() });
                    }
                },
                if selecting {
                    span { class: "tk-novel-chapter__check", "data-checked": "{is_sel}",
                        if is_sel { Icon { name: "check".to_string(), size: 16 } }
                    }
                }
                span { class: "tk-novel-chapter__num", "{label}" }
                if !title.is_empty() {
                    span { class: "tk-novel-chapter__title", "{title}" }
                }
                if let Some(g) = chapter.group.clone() {
                    span { class: "tk-novel-chapter__group tk-muted", "{g}" }
                }
                if !date.is_empty() {
                    span { class: "tk-novel-chapter__date tk-muted", "{date}" }
                }
                if chapter.downloaded {
                    span { class: "tk-novel-chapter__offline", title: t("novel.chapters.offline"), Icon { name: "offline".to_string(), size: 16 } }
                }
                if chapter.read > 0.0 && chapter.read < 0.99 {
                    span { class: "tk-novel-chapter__bar", "aria-hidden": "true",
                        i { style: "width:{bar}%" }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_status_ids() {
        assert_eq!(date_of(0), "1970-01-01");
        assert_eq!(date_of(1_714_521_600), "2024-05-01");
        assert_eq!(date_of(951_782_400), "2000-02-29");
        assert_eq!(read_status_id(None), "none");
        assert_eq!(read_status_id(Some(ListStatusVm::Watching)), "watching");
    }

    #[test]
    fn first_chapter_is_the_lowest_number() {
        let ch = |n: f32, id: &str| ChapterVm {
            reference: ChapterRefVm { ext_id: "x".into(), chapter_id: id.into() },
            number: n,
            volume: None,
            title: None,
            lang: None,
            group: None,
            published_unix: None,
            pages: None,
            read: 0.0,
            downloaded: false,
        };
        let p = vec![
            ChapterProviderVm { ext_id: "x".into(), ext_name: "a".into(), lang: None, chapters: vec![ch(2.0, "b"), ch(1.0, "a")], error: None },
            ChapterProviderVm { ext_id: "x".into(), ext_name: "b".into(), lang: None, chapters: vec![ch(3.0, "c")], error: None },
        ];
        assert_eq!(first_chapter(&p).map(|r| r.chapter_id), Some("a".into()));
        assert_eq!(first_chapter(&[]), None);
    }
}
