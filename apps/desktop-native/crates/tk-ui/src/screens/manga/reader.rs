//! Manga reader `/manga/:id/read/:ext_id/:chapter_id` (worker **l**; checklist `manga.reader.modes`,
//! `manga.reader.fit`, `manga.reader.loading`, `manga.reader.navigation`, `manga.reader.behavior`,
//! `manga.reader.keybinds`, `manga.reader.provider-switch`, `manga.reader.prefetch`, `manga.reader.progress`,
//! `manga.download.offline-read`; classic `MangaReaderPage.tsx`, `components/reader/*`). Bare route
//! (`Route::is_bare`): the reader draws its own chrome. The chapter id is route-encoded
//! (`screens::manga::encode_chapter_id`).
//!
//! - Pages from `Backend::manga_pages` (offline copy first), loaded through the `tkpage` asset handler
//!   (`components::reader`). Modes paged / double page / vertical / webtoon (`data-mode`), fit (`data-fit`),
//!   direction (`data-direction`; RTL turns pages leftwards, keys and tap zones follow), background
//!   (`data-background`, tokens only), brightness, width, max width, gap, panel crop, zoom (Ctrl + wheel).
//! - Lazy (current page +- prefetch) or eager rendering, next chapter prefetched at 70 % (`data-prefetched-next`),
//!   continuous scroll into the next chapter (vertical / webtoon).
//! - Navigation: tap zones, rebindable keys (`MangaReaderPrefs::keybinds`, Settings > Manga reader), page rail,
//!   chapter navigator, previous / next chapter. Behaviour: auto-scroll (off with reduced motion), full screen on
//!   open, chrome hidden in full screen until the mouse moves, page number, progress bar, "Save page".
//! - Failed pages offer a retry and every other source of the chapter (`manga.reader.provider-switch`).
//! - Progress: every page change is saved (`set_read_progress`, `data-page`, `data-chapter`); the last page marks the
//!   chapter completed (AniList progress follows in the backend).

use std::collections::BTreeMap;
use std::time::Duration;

use dioxus::desktop::use_window;
use dioxus::prelude::*;
use serde::Deserialize;

use super::{decode_chapter_id, reader_route};
use crate::app::{Route, Services, Ui};
use crate::backend::{ChapterProviderVm, ChapterRefVm, PageVm, ReadProgressVm};
use crate::components::reader::chapters::ChapterNavigator;
use crate::components::reader::prefs_form::ReaderPrefsForm;
use crate::components::reader::rail::PageRail;
use crate::components::reader::{page_io, page_src, use_page_handler};
use crate::components::{Button, ButtonVariant, ErrorState, IconButton, LoadingIndicator, Sheet, use_snackbar};
use crate::i18n::{t, tf};
use crate::model::manga::{
    ReaderActionVm, ZOOM_MAX_PERCENT, ZOOM_MIN_PERCENT, chapter_label, key_token, merge_chapters, neighbour_chapter,
    pages_are_offline, render_window, same_chapter, should_prefetch_next, sources_of, spread_of, spreads,
};
use crate::model::{MangaReaderPrefs, ReadKindVm, ReaderDirectionVm, ReaderFitVm, ReaderModeVm, SettingsAction, reduce_settings};

/// Key / wheel / scroll events of the reader page (installed by [`READER_JS`]).
const READER_JS: &str = r#"
(() => {
  if (window.__tkReaderStop) window.__tkReaderStop();
  const ac = new AbortController();
  const editable = (el) => !!el && ((el.tagName === 'INPUT' && !['range', 'checkbox'].includes(el.type)) || el.tagName === 'TEXTAREA' || el.tagName === 'SELECT' || el.isContentEditable);
  window.addEventListener('keydown', (e) => {
    if (editable(e.target) || document.querySelector('.tk-sheet, .tk-dialog')) return;
    if ([' ', 'ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'PageDown', 'PageUp'].includes(e.key)) e.preventDefault();
    dioxus.send({ t: 'key', key: e.key, ctrl: e.ctrlKey || e.metaKey });
  }, { signal: ac.signal });
  window.addEventListener('wheel', (e) => {
    if (!(e.ctrlKey || e.metaKey) || !document.querySelector('.tk-reader[data-wheel-zoom="true"]')) return;
    e.preventDefault();
    dioxus.send({ t: 'zoom', d: e.deltaY < 0 ? 1 : -1 });
  }, { passive: false, signal: ac.signal });
  let timer = 0;
  document.addEventListener('scroll', (e) => {
    const el = e.target;
    if (!el || !el.classList || !el.classList.contains('tk-reader__scroll')) return;
    clearTimeout(timer);
    timer = setTimeout(() => {
      const mark = el.getBoundingClientRect().top + el.clientHeight * 0.35;
      let idx = 0;
      for (const p of el.querySelectorAll('[data-page-index]')) {
        if (p.getBoundingClientRect().top <= mark) idx = Number(p.dataset.pageIndex); else break;
      }
      const end = el.scrollTop + el.clientHeight >= el.scrollHeight - 8;
      dioxus.send({ t: 'scroll', i: idx, end });
    }, 90);
  }, { capture: true, passive: true, signal: ac.signal });
  window.__tkReaderStop = () => ac.abort();
})();
"#;

const STOP_JS: &str = "window.__tkReaderStop && window.__tkReaderStop(); window.__tkAuto && window.__tkAuto.stop();";

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "t")]
enum ReaderMsg {
    #[serde(rename = "key")]
    Key { key: String, ctrl: bool },
    #[serde(rename = "zoom")]
    Zoom { d: i32 },
    #[serde(rename = "scroll")]
    Scroll { i: usize, end: bool },
}

/// What a key press does: the bound action, with the arrow-style page turns mirrored in RTL paged modes (the
/// key bound to "next page" turns leftwards, like the tap zones).
pub fn resolve_key(prefs: &MangaReaderPrefs, key: &str) -> Option<ReaderActionVm> {
    let token = key_token(key)?;
    let action = prefs.action_for(&token)?;
    let paged = matches!(prefs.mode, ReaderModeVm::Paged | ReaderModeVm::DoublePage);
    let rtl = prefs.direction == ReaderDirectionVm::Rtl;
    Some(match action {
        ReaderActionVm::NextPage if paged && rtl => ReaderActionVm::PrevPage,
        ReaderActionVm::PrevPage if paged && rtl => ReaderActionVm::NextPage,
        a => a,
    })
}

/// Page after / before `current` in the paged modes (`None` = past the chapter's end / start).
pub fn step_page(mode: ReaderModeVm, pages: &[PageVm], current: usize, forward: bool) -> Option<usize> {
    if pages.is_empty() {
        return None;
    }
    if mode == ReaderModeVm::DoublePage {
        let sp = spreads(pages);
        let at = spread_of(&sp, current);
        return if forward { sp.get(at + 1).map(|s| s.0) } else { at.checked_sub(1).and_then(|i| sp.get(i)).map(|s| s.0) };
    }
    if forward { (current + 1 < pages.len()).then_some(current + 1) } else { current.checked_sub(1) }
}

/// The next mode of the "toggle mode" action.
fn next_mode(m: ReaderModeVm) -> ReaderModeVm {
    let all = ReaderModeVm::ALL;
    let i = all.iter().position(|x| *x == m).unwrap_or(0);
    all[(i + 1) % all.len()]
}

/// Which sheet is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Panel {
    Settings,
    Chapters,
    Sources,
}

#[component]
pub fn MangaReaderPage(id: String, ext_id: String, chapter_id: String) -> Element {
    let chapter = ChapterRefVm { ext_id: ext_id.clone(), chapter_id: decode_chapter_id(&chapter_id) };
    rsx! {
        ReaderView { key: "{ext_id}/{chapter_id}", media_id: id.clone(), chapter }
    }
}

#[component]
fn ReaderView(media_id: String, chapter: ChapterRefVm) -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let navigator = use_navigator();
    let snackbar = use_snackbar();
    let win = use_window();
    use_page_handler();

    let mut prefs = use_signal(|| ui.settings.peek().manga.reader.clone());
    let reduce = ui.settings.read().reduce_motion;
    let backend = services.backend.clone();

    let mut pages = use_resource({
        let (backend, chapter) = (backend.clone(), chapter.clone());
        move || {
            let (backend, chapter) = (backend.clone(), chapter.clone());
            async move { backend.manga_pages(&chapter).await }
        }
    });
    let chapters = use_resource({
        let (backend, id) = (backend.clone(), media_id.clone());
        move || {
            let (backend, id) = (backend.clone(), id.clone());
            async move { backend.manga_chapters(&id, false).await }
        }
    });
    let progress = use_resource({
        let (backend, id) = (backend.clone(), media_id.clone());
        move || {
            let (backend, id) = (backend.clone(), id.clone());
            async move { backend.read_progress(ReadKindVm::Manga, &id).await }
        }
    });
    let title = use_resource({
        let (backend, id) = (backend.clone(), media_id.clone());
        move || {
            let (backend, id) = (backend.clone(), id.clone());
            async move { backend.manga_details(&id).await.map(|d| d.media.title) }
        }
    });

    let mut current = use_signal(|| 0_usize);
    let mut placed = use_signal(|| false);
    let mut chrome = use_signal(|| true);
    let mut chrome_gen = use_signal(|| 0_u64);
    let mut panel = use_signal(|| None::<Panel>);
    let mut fullscreen = use_signal(|| false);
    let mut failed = use_signal(BTreeMap::<usize, u32>::new);
    let mut auto = use_signal(|| false);
    let mut next_pages = use_signal(|| None::<Vec<PageVm>>);
    let mut prefetched = use_signal(|| false);
    let mut leaving = use_signal(|| false);

    // ---- derived state ----
    let page_list: Vec<PageVm> = pages.read().as_ref().and_then(|r| r.as_ref().ok()).cloned().unwrap_or_default();
    let providers: Vec<ChapterProviderVm> = chapters.read().as_ref().and_then(|r| r.as_ref().ok()).cloned().unwrap_or_default();
    let manga_prefs = ui.settings.read().manga.clone();
    let merged = merge_chapters(&providers, &manga_prefs.preferred_lang, manga_prefs.preferred_source.as_deref());
    let rows: Vec<ReadProgressVm> = progress.read().as_ref().and_then(|r| r.as_ref().ok()).cloned().unwrap_or_default();
    let here = providers.iter().find_map(|p| p.chapters.iter().find(|c| c.reference == chapter).map(|c| (p.ext_id.clone(), p.ext_name.clone(), c.number)));
    let number = here
        .as_ref()
        .map(|h| h.2)
        .or_else(|| rows.iter().find(|r| r.chapter == chapter).map(|r| r.chapter_number))
        .unwrap_or(0.0);
    let provider_key = here.as_ref().map(|h| h.0.clone()).unwrap_or_else(|| chapter.ext_id.clone());
    let provider_name = here.as_ref().map(|h| h.1.clone()).unwrap_or_else(|| chapter.ext_id.clone());
    let next = neighbour_chapter(&merged, number, true).cloned();
    let prev = neighbour_chapter(&merged, number, false).cloned();
    let others: Vec<(String, String, ChapterRefVm)> = if number > 0.0 {
        sources_of(&providers, number).into_iter().filter(|(k, _, c)| *k != provider_key && c.reference != chapter).map(|(k, n, c)| (k, n, c.reference)).collect()
    } else {
        Vec::new()
    };
    let total = page_list.len();
    let p = prefs();

    // ---- initial position: where this chapter stopped ----
    {
        let chapter = chapter.clone();
        use_effect(move || {
            let n = pages.read().as_ref().and_then(|r| r.as_ref().ok()).map(Vec::len);
            let saved: Option<Vec<ReadProgressVm>> = progress.read().as_ref().map(|r| r.clone().unwrap_or_default());
            let (Some(n), Some(saved)) = (n, saved) else { return };
            if *placed.peek() {
                return;
            }
            let start = saved.iter().find(|r| r.chapter == chapter && !r.completed).map_or(0, |r| r.position as usize);
            let start = start.min(n.saturating_sub(1));
            placed.set(true);
            current.set(start);
            let mode = prefs.peek().mode;
            if start > 0 && matches!(mode, ReaderModeVm::Vertical | ReaderModeVm::Webtoon) {
                spawn(async move {
                    tokio::time::sleep(Duration::from_millis(150)).await;
                    let _ = document::eval(&format!("document.querySelector('.tk-reader [data-page-index=\"{start}\"]')?.scrollIntoView({{block:'start'}})"));
                });
            }
        });
    }

    // ---- progress: saved on every page change once placed ----
    {
        let backend = backend.clone();
        let (media_id, chapter) = (media_id.clone(), chapter.clone());
        use_effect(move || {
            let index = current();
            if !placed() {
                return;
            }
            let n = pages.read().as_ref().and_then(|r| r.as_ref().ok()).map_or(0, Vec::len);
            if n == 0 {
                return;
            }
            let number = chapters
                .read()
                .as_ref()
                .and_then(|r| r.as_ref().ok())
                .and_then(|ps| ps.iter().find_map(|p| p.chapters.iter().find(|c| c.reference == chapter).map(|c| c.number)))
                .or_else(|| {
                    progress.peek().as_ref().and_then(|r| r.as_ref().ok()).and_then(|rows| rows.iter().find(|r| r.chapter == chapter).map(|r| r.chapter_number))
                })
                .unwrap_or(0.0);
            let vm = ReadProgressVm {
                chapter: chapter.clone(),
                chapter_number: number,
                position: index as u32,
                total: Some(n as u32),
                completed: index + 1 >= n,
                updated_unix: 0,
            };
            let (backend, media_id) = (backend.clone(), media_id.clone());
            spawn(async move {
                if let Err(e) = backend.set_read_progress(ReadKindVm::Manga, &media_id, &vm).await {
                    tracing::debug!(detail = %e.detail, "saving reading progress failed");
                }
            });
        });
    }

    // ---- prefetch the next chapter at 70 % (`manga.reader.prefetch`) ----
    {
        let backend = backend.clone();
        let next_ref = next.as_ref().map(|n| n.chapter.reference.clone());
        use_effect(use_reactive!(|next_ref| {
            let index = current();
            let n = pages.read().as_ref().and_then(|r| r.as_ref().ok()).map_or(0, Vec::len);
            if *prefetched.peek() || !prefs.peek().prefetch_next_chapter || !should_prefetch_next(index, n) {
                return;
            }
            let Some(next_ref) = next_ref.clone() else { return };
            prefetched.set(true);
            let backend = backend.clone();
            spawn(async move {
                if let Ok(list) = backend.manga_pages(&next_ref).await {
                    next_pages.set(Some(list));
                }
            });
        }));
    }

    // ---- full screen on open, restored on leave ----
    {
        let win = win.clone();
        use_hook(move || {
            if ui.settings.peek().manga.reader.auto_fullscreen {
                win.set_fullscreen(true);
                fullscreen.set(true);
            }
        });
    }
    {
        let win = win.clone();
        use_drop(move || {
            let _ = document::eval(STOP_JS);
            if *fullscreen.peek() {
                win.set_fullscreen(false);
            }
        });
    }

    // ---- actions ----
    let open_chapter = use_callback({
        let media_id = media_id.clone();
        move |c: ChapterRefVm| {
            if *leaving.peek() {
                return;
            }
            leaving.set(true);
            navigator.replace(reader_route(&media_id, &c));
        }
    });
    let scroll_by = move |px: i64| {
        let behavior = if reduce { "instant" } else { "smooth" };
        let _ = document::eval(&format!("document.querySelector('.tk-reader__scroll')?.scrollBy({{top:{px},behavior:'{behavior}'}})"));
    };
    let jump = use_callback(move |i: usize| {
        current.set(i);
        if matches!(prefs.peek().mode, ReaderModeVm::Vertical | ReaderModeVm::Webtoon) {
            let _ = document::eval(&format!("document.querySelector('.tk-reader [data-page-index=\"{i}\"]')?.scrollIntoView({{block:'start'}})"));
        }
    });
    let act = use_callback({
        let win = win.clone();
        let next = next.clone();
        let prev = prev.clone();
        let page_list = page_list.clone();
        move |a: ReaderActionVm| {
            let pr = prefs.peek().clone();
            let vertical = matches!(pr.mode, ReaderModeVm::Vertical | ReaderModeVm::Webtoon);
            let cur = *current.peek();
            match a {
                ReaderActionVm::NextPage | ReaderActionVm::ScrollDown if vertical => {
                    scroll_by(if a == ReaderActionVm::NextPage { 700 } else { 160 });
                }
                ReaderActionVm::PrevPage | ReaderActionVm::ScrollUp if vertical => {
                    scroll_by(if a == ReaderActionVm::PrevPage { -700 } else { -160 });
                }
                ReaderActionVm::NextPage | ReaderActionVm::ScrollDown => match step_page(pr.mode, &page_list, cur, true) {
                    Some(i) => current.set(i),
                    None => match &next {
                        Some(n) => open_chapter.call(n.chapter.reference.clone()),
                        None => snackbar.show(t("manga.reader.last_chapter")),
                    },
                },
                ReaderActionVm::PrevPage | ReaderActionVm::ScrollUp => match step_page(pr.mode, &page_list, cur, false) {
                    Some(i) => current.set(i),
                    None => {
                        if let Some(p) = &prev {
                            open_chapter.call(p.chapter.reference.clone());
                        }
                    }
                },
                ReaderActionVm::NextChapter => match &next {
                    Some(n) => open_chapter.call(n.chapter.reference.clone()),
                    None => snackbar.show(t("manga.reader.last_chapter")),
                },
                ReaderActionVm::PrevChapter => match &prev {
                    Some(p) => open_chapter.call(p.chapter.reference.clone()),
                    None => snackbar.show(t("manga.reader.first_chapter")),
                },
                ReaderActionVm::Fullscreen => {
                    let on = !*fullscreen.peek();
                    fullscreen.set(on);
                    win.set_fullscreen(on);
                    chrome.set(true);
                }
                ReaderActionVm::ToggleMode => {
                    let mut n = pr.clone();
                    n.mode = next_mode(pr.mode);
                    prefs.set(n);
                }
                ReaderActionVm::ZoomIn | ReaderActionVm::ZoomOut => {
                    let mut n = pr.clone();
                    let step: i32 = if a == ReaderActionVm::ZoomIn { 10 } else { -10 };
                    n.fit = ReaderFitVm::Original;
                    n.zoom_percent = (i32::from(pr.zoom_percent) + step).clamp(i32::from(ZOOM_MIN_PERCENT), i32::from(ZOOM_MAX_PERCENT)) as u16;
                    prefs.set(n);
                }
                ReaderActionVm::Reload => {
                    failed.set(BTreeMap::new());
                    pages.restart();
                }
                ReaderActionVm::AutoScroll => {
                    if ui.settings.peek().reduce_motion {
                        snackbar.show(t("manga.reader.auto_reduced"));
                    } else {
                        let on = !*auto.peek();
                        auto.set(on);
                    }
                }
                ReaderActionVm::ToggleChrome => {
                    let on = !*chrome.peek();
                    chrome.set(on);
                }
            }
        }
    });

    // ---- key / wheel / scroll bridge ----
    {
        use_future(move || async move {
            let mut eval = document::eval(READER_JS);
            while let Ok(msg) = eval.recv::<ReaderMsg>().await {
                match msg {
                    ReaderMsg::Key { key, ctrl } => {
                        if key == "Escape" {
                            if *panel.peek() != None {
                                panel.set(None);
                            } else if *fullscreen.peek() {
                                act.call(ReaderActionVm::Fullscreen);
                            } else {
                                navigator.go_back();
                            }
                            continue;
                        }
                        if ctrl {
                            continue;
                        }
                        let pr = prefs.peek().clone();
                        if let Some(a) = resolve_key(&pr, &key) {
                            act.call(a);
                        }
                    }
                    ReaderMsg::Zoom { d } => {
                        if prefs.peek().zoom_with_wheel {
                            act.call(if d > 0 { ReaderActionVm::ZoomIn } else { ReaderActionVm::ZoomOut });
                        }
                    }
                    ReaderMsg::Scroll { i, end } => {
                        if *current.peek() != i {
                            current.set(i);
                        }
                        let n = pages.peek().as_ref().and_then(|r| r.as_ref().ok()).map_or(0, Vec::len);
                        if end && n > 0 && *current.peek() + 1 < n {
                            current.set(n - 1);
                        }
                        let pr = prefs.peek().clone();
                        if end && pr.continuous_scroll && matches!(pr.mode, ReaderModeVm::Vertical | ReaderModeVm::Webtoon) {
                            act.call(ReaderActionVm::NextChapter);
                        }
                    }
                }
            }
        });
    }

    // ---- auto-scroll: a rAF loop in vertical modes, a page timer in paged ones ----
    use_effect(move || {
        let on = auto();
        let pr = prefs();
        let vertical = matches!(pr.mode, ReaderModeVm::Vertical | ReaderModeVm::Webtoon);
        if on && vertical {
            let px = f32::from(pr.auto_scroll_speed) * 0.6;
            let _ = document::eval(&format!(
                "window.__tkAuto && window.__tkAuto.stop(); window.__tkAuto = (() => {{ let on = true; const step = () => {{ if (!on) return; const el = document.querySelector('.tk-reader__scroll'); if (el) el.scrollBy(0, {px}); requestAnimationFrame(step); }}; requestAnimationFrame(step); return {{ stop: () => {{ on = false; }} }}; }})();"
            ));
        } else {
            let _ = document::eval("window.__tkAuto && window.__tkAuto.stop();");
        }
    });
    use_future(move || async move {
        loop {
            let pr = prefs.peek().clone();
            let secs = 12_u64.saturating_sub(u64::from(pr.auto_scroll_speed)).max(2);
            tokio::time::sleep(Duration::from_secs(secs)).await;
            let pr = prefs.peek().clone();
            // `NextPage` of `act` is the logical forward step (RTL only mirrors keys and tap zones).
            if *auto.peek() && matches!(pr.mode, ReaderModeVm::Paged | ReaderModeVm::DoublePage) && panel.peek().is_none() {
                act.call(ReaderActionVm::NextPage);
            }
        }
    });

    // ---- chrome auto-hide in full screen ----
    let on_move = move |_| {
        if !*fullscreen.peek() || !prefs.peek().hide_chrome_fullscreen {
            return;
        }
        if !*chrome.peek() {
            chrome.set(true);
        }
        let g = *chrome_gen.peek() + 1;
        chrome_gen.set(g);
        spawn(async move {
            tokio::time::sleep(Duration::from_millis(2500)).await;
            if *chrome_gen.peek() == g && *fullscreen.peek() && panel.peek().is_none() {
                chrome.set(false);
            }
        });
    };

    // ---- page capture ----
    let capture = {
        let page_list = page_list.clone();
        let title_text = title.read().as_ref().and_then(|r| r.as_ref().ok()).cloned().unwrap_or_default();
        move |_| {
            let Some(page) = page_list.get(*current.peek()).cloned() else { return };
            let Some(io) = page_io() else {
                snackbar.show(t("manga.reader.capture_error"));
                return;
            };
            let stem = format!("{} - {} - {}", title_text, chapter_label(number), page.index + 1);
            spawn(async move {
                match io.save(page.url.clone(), page.headers.clone(), stem).await {
                    Ok(path) => snackbar.show(tf("manga.reader.captured", &[("path", &path)])),
                    Err(e) => snackbar.show(tf("manga.reader.capture_failed", &[("detail", &e)])),
                }
            });
        }
    };

    // ---- stage ----
    let vertical = matches!(p.mode, ReaderModeVm::Vertical | ReaderModeVm::Webtoon);
    let cur = current().min(total.saturating_sub(1));
    let width = ui.width;
    let tap = move |e: MouseEvent| {
        let pr = prefs.peek().clone();
        let w = width().max(1.0);
        let x = e.client_coordinates().x / w;
        let paged = matches!(pr.mode, ReaderModeVm::Paged | ReaderModeVm::DoublePage);
        if !pr.tap_zones || !paged || (0.33..=0.67).contains(&x) {
            let on = !*chrome.peek();
            chrome.set(on);
            return;
        }
        let left = x < 0.33;
        let rtl = pr.direction == ReaderDirectionVm::Rtl;
        let forward = left == rtl;
        act.call(if forward { ReaderActionVm::NextPage } else { ReaderActionVm::PrevPage });
    };
    let window_range = render_window(total, cur, p.prefetch_pages, p.eager_loading);
    let fails = failed();
    let page_img = move |i: usize, page: &PageVm| {
        let retry = fails.get(&i).copied().unwrap_or(0);
        let failed_now = fails.contains_key(&i);
        let src = page_src(page, retry);
        rsx! {
            div { key: "{i}", class: "tk-reader__page", "data-page-index": "{i}", "data-failed": "{failed_now}",
                img {
                    src: "{src}",
                    alt: tf("manga.reader.page_alt", &[("n", &(i + 1).to_string())]),
                    decoding: "async",
                    draggable: "false",
                    onerror: move |_| {
                        let mut f = failed.peek().clone();
                        f.entry(i).or_insert(0);
                        failed.set(f);
                    },
                    onload: move |_| {
                        if failed.peek().contains_key(&i) {
                            let mut f = failed.peek().clone();
                            f.remove(&i);
                            failed.set(f);
                        }
                    },
                }
                if failed_now {
                    div { class: "tk-reader__page-error",
                        span { {t("manga.reader.page_failed")} }
                        Button {
                            label: t("common.retry"),
                            variant: ButtonVariant::Tonal,
                            onclick: move |e: MouseEvent| {
                                e.stop_propagation();
                                let mut f = failed.peek().clone();
                                let n = f.entry(i).or_insert(0);
                                *n += 1;
                                failed.set(f);
                            },
                        }
                    }
                }
            }
        }
    };

    let end_of_chapter = match next.clone() {
        Some(n) => {
            let label = tf("manga.reader.next_chapter_n", &[("n", &chapter_label(n.number))]);
            let target = n.chapter.reference.clone();
            rsx! {
                Button {
                    label,
                    icon: "next".to_string(),
                    onclick: move |e: MouseEvent| {
                        e.stop_propagation();
                        open_chapter.call(target.clone());
                    },
                }
            }
        }
        None => rsx! {
            p { class: "tk-muted", {t("manga.reader.last_chapter")} }
        },
    };
    let state = pages.read().clone();
    let stage = match state {
        None => rsx! {
            div { class: "tk-reader__center", "data-pages": "loading", LoadingIndicator {} }
        },
        Some(Err(e)) => rsx! {
            div { class: "tk-reader__center", "data-pages": "error",
                ErrorState { error: e, on_retry: move |_| pages.restart(),
                    SourceSwitch { others: others.clone(), on_pick: move |c: ChapterRefVm| open_chapter.call(c) }
                }
            }
        },
        Some(Ok(list)) if list.is_empty() => rsx! {
            div { class: "tk-reader__center", "data-pages": "empty",
                ErrorState { error: crate::model::UiError::new(crate::model::ErrorKind::Other, t("manga.reader.no_pages")), on_retry: move |_| pages.restart(),
                    SourceSwitch { others: others.clone(), on_pick: move |c: ChapterRefVm| open_chapter.call(c) }
                }
            }
        },
        Some(Ok(list)) if vertical => rsx! {
            div { class: "tk-reader__scroll", onclick: tap,
                div { class: "tk-reader__column",
                    for (i , page) in list.iter().enumerate() {
                        if window_range.contains(&i) {
                            {page_img(i, page)}
                        } else {
                            div { key: "{i}", class: "tk-reader__page tk-reader__page--idle", "data-page-index": "{i}" }
                        }
                    }
                    div { class: "tk-reader__end", "data-chapter-end": "true", {end_of_chapter} }
                }
            }
        },
        Some(Ok(list)) => {
            let sp = spreads(&list);
            let at = spread_of(&sp, cur);
            let (a, b) = if p.mode == ReaderModeVm::DoublePage { sp.get(at).copied().unwrap_or((cur, None)) } else { (cur, None) };
            let preload: Vec<usize> = (cur + 1..(cur + 1 + usize::from(p.prefetch_pages)).min(list.len())).collect();
            rsx! {
                div { class: "tk-reader__paged", onclick: tap,
                    div { class: "tk-reader__spread", "data-spread": "{b.is_some()}",
                        if let Some(page) = list.get(a) {
                            {page_img(a, page)}
                        }
                        if let Some(bi) = b {
                            if let Some(page) = list.get(bi) {
                                {page_img(bi, page)}
                            }
                        }
                    }
                    div { class: "tk-reader__preload", "aria-hidden": "true",
                        for i in preload {
                            if let Some(page) = list.get(i) {
                                img { key: "{i}", src: page_src(page, 0), alt: "", loading: "eager" }
                            }
                        }
                    }
                }
            }
        }
    };

    let offline = pages_are_offline(&page_list);
    let title_text = title.read().as_ref().and_then(|r| r.as_ref().ok()).cloned().unwrap_or_default();
    let chapter_text = tf("manga.chapter.n", &[("n", &chapter_label(number))]);
    let counter = if total > 0 { format!("{} / {total}", cur + 1) } else { String::new() };
    let progress_pct = if total > 0 { ((cur + 1) as f64 / total as f64 * 100.0).round() } else { 0.0 };
    let style = format!(
        "--tk-reader-brightness: {}%; --tk-reader-width: {}%; --tk-reader-max: {}; --tk-reader-gap: {}px; --tk-reader-zoom: {};",
        p.brightness,
        p.width_percent,
        if p.max_width_px == 0 { "none".to_owned() } else { format!("{}px", p.max_width_px) },
        if p.mode == ReaderModeVm::Webtoon { 0 } else { p.gap_px },
        f32::from(p.zoom_percent) / 100.0,
    );
    let mode_label = t(&format!("manga.reader.mode.{}", next_mode(p.mode).id()));
    let media_for_back = media_id.clone();
    let chrome_on = chrome() || panel().is_some();
    let next_for_btn = next.clone();
    let prev_for_btn = prev.clone();
    let save_default = {
        let services = services.clone();
        move |_| {
            let s = ui.settings.peek().clone();
            let mut m = s.manga.clone();
            m.reader = prefs.peek().clone().sanitized();
            let n = reduce_settings(&s, SettingsAction::SetManga(m));
            ui.commit_settings(&services.backend, n);
            snackbar.show(t("manga.reader.saved_default"));
        }
    };

    let sheet = match panel() {
        Some(Panel::Settings) => rsx! {
            Sheet {
                title: t("manga.reader.settings"),
                on_close: move |_| panel.set(None),
                footer: rsx! {
                    Button { label: t("manga.reader.save_default"), variant: ButtonVariant::Tonal, onclick: save_default }
                },
                ReaderPrefsForm { prefs: p.clone(), on_change: move |n: MangaReaderPrefs| prefs.set(n) }
            }
        },
        Some(Panel::Chapters) => {
            let chapter = chapter.clone();
            rsx! {
                ChapterNavigator {
                    merged: merged.clone(),
                    current: number,
                    on_pick: move |m: crate::model::manga::MergedChapterVm| {
                        panel.set(None);
                        if !same_chapter(m.number, number) || m.chapter.reference != chapter {
                            open_chapter.call(m.chapter.reference.clone());
                        }
                    },
                    on_close: move |_| panel.set(None),
                }
            }
        }
        Some(Panel::Sources) => rsx! {
            Sheet { title: t("manga.reader.sources"), on_close: move |_| panel.set(None),
                p { class: "tk-muted", {tf("manga.reader.current_source", &[("name", &provider_name)])} }
                SourceSwitch {
                    others: others.clone(),
                    on_pick: move |c: ChapterRefVm| {
                        panel.set(None);
                        open_chapter.call(c);
                    },
                }
            }
        },
        None => rsx! {},
    };

    rsx! {
        div {
            class: "tk-reader",
            tabindex: "-1",
            style: "{style}",
            "data-mode": "{p.mode.id()}",
            "data-fit": "{p.fit.id()}",
            "data-direction": "{p.direction.id()}",
            "data-background": "{p.background.id()}",
            "data-panel-crop": "{p.panel_crop}",
            "data-page": "{cur + 1}",
            "data-total": "{total}",
            "data-chapter": "{chapter_label(number)}",
            "data-chapter-id": "{chapter.chapter_id}",
            "data-provider": "{provider_key}",
            "data-offline": "{offline}",
            "data-chrome": if chrome_on { "visible" } else { "hidden" },
            "data-fullscreen": "{fullscreen()}",
            "data-wheel-zoom": "{p.zoom_with_wheel}",
            "data-autoscroll": "{auto()}",
            "data-prefetched-next": "{next_pages().is_some()}",
            onmousemove: on_move,
            if chrome_on {
                header { class: "tk-reader__top",
                    IconButton {
                        icon: "back".to_string(),
                        label: t("common.back"),
                        onclick: move |_| {
                            if navigator.can_go_back() {
                                navigator.go_back();
                            } else {
                                navigator.replace(Route::MangaDetailsPage { id: media_for_back.clone() });
                            }
                        },
                    }
                    div { class: "tk-reader__titles",
                        span { class: "tk-reader__title", "{title_text}" }
                        span { class: "tk-reader__sub tk-muted",
                            "{chapter_text} · {provider_name}"
                            if offline {
                                " · "
                                {t("manga.reader.offline")}
                            }
                        }
                    }
                    if p.show_page_number && total > 0 {
                        span { class: "tk-reader__counter", "data-counter": "{cur + 1}", "{counter}" }
                    }
                    div { class: "tk-reader__actions",
                        IconButton { icon: "back".to_string(), label: t("manga.reader.prev_chapter"), disabled: prev_for_btn.is_none(), onclick: move |_| act.call(ReaderActionVm::PrevChapter) }
                        IconButton { icon: "next".to_string(), label: t("manga.reader.next_chapter"), disabled: next_for_btn.is_none(), onclick: move |_| act.call(ReaderActionVm::NextChapter) }
                        IconButton { icon: "grid".to_string(), label: tf("manga.reader.switch_mode", &[("mode", &mode_label)]), onclick: move |_| act.call(ReaderActionVm::ToggleMode) }
                        IconButton { icon: "library".to_string(), label: t("manga.reader.chapters"), onclick: move |_| panel.set(Some(Panel::Chapters)) }
                        IconButton { icon: "sync".to_string(), label: t("manga.reader.sources"), onclick: move |_| panel.set(Some(Panel::Sources)) }
                        if !reduce {
                            IconButton {
                                icon: if auto() { "pause".to_string() } else { "play".to_string() },
                                label: t("manga.reader.auto_scroll"),
                                toggle: true,
                                toggled: auto(),
                                onclick: move |_| act.call(ReaderActionVm::AutoScroll),
                            }
                        }
                        if p.show_capture_button {
                            IconButton { icon: "screenshot".to_string(), label: t("manga.reader.capture"), onclick: capture }
                        }
                        IconButton { icon: "fullscreen".to_string(), label: t("manga.reader.fullscreen"), toggle: true, toggled: fullscreen(), onclick: move |_| act.call(ReaderActionVm::Fullscreen) }
                        IconButton { icon: "tune".to_string(), label: t("manga.reader.settings"), onclick: move |_| panel.set(Some(Panel::Settings)) }
                    }
                }
            }
            main { class: "tk-reader__stage", {stage} }
            if chrome_on && total > 1 {
                footer { class: "tk-reader__bottom",
                    PageRail { pages: page_list.clone(), current: cur, on_jump: move |i: usize| jump.call(i) }
                }
            }
            if p.show_progress_bar && total > 0 {
                div { class: "tk-reader__progress", role: "progressbar", "aria-valuenow": "{progress_pct}", "aria-valuemin": "0", "aria-valuemax": "100",
                    span { class: "tk-reader__progress-fill", style: "width: {progress_pct}%" }
                }
            }
            if let Some(list) = next_pages() {
                div { class: "tk-reader__preload", "aria-hidden": "true",
                    for (i , page) in list.iter().take(usize::from(p.prefetch_pages).max(1)).enumerate() {
                        img { key: "{i}", src: page_src(page, 0), alt: "", loading: "eager" }
                    }
                }
            }
            {sheet}
        }
    }
}

/// Other sources of the current chapter (`manga.reader.provider-switch`, `data-source-switch`).
#[component]
fn SourceSwitch(others: Vec<(String, String, ChapterRefVm)>, on_pick: EventHandler<ChapterRefVm>) -> Element {
    if others.is_empty() {
        return rsx! {
            p { class: "tk-muted", "data-source-switch": "0", {t("manga.reader.no_other_sources")} }
        };
    }
    rsx! {
        div { class: "tk-reader__sources", "data-source-switch": "{others.len()}",
            p { class: "tk-muted", {t("manga.reader.try_source")} }
            for (key , name , reference) in others {
                Button {
                    key: "{key}-{reference.chapter_id}",
                    label: name.clone(),
                    icon: "sync".to_string(),
                    variant: ButtonVariant::Outlined,
                    onclick: move |_| on_pick.call(reference.clone()),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pages(n: usize) -> Vec<PageVm> {
        (0..n).map(|i| PageVm { index: i as u32, url: format!("u{i}"), headers: Vec::new(), width: Some(800), height: Some(1200) }).collect()
    }

    #[test]
    fn keys_follow_bindings_and_direction() {
        let rtl = MangaReaderPrefs::default();
        assert_eq!(rtl.direction, ReaderDirectionVm::Rtl);
        assert_eq!(resolve_key(&rtl, "ArrowLeft"), Some(ReaderActionVm::NextPage));
        assert_eq!(resolve_key(&rtl, "ArrowRight"), Some(ReaderActionVm::PrevPage));
        let ltr = MangaReaderPrefs { direction: ReaderDirectionVm::Ltr, ..MangaReaderPrefs::default() };
        assert_eq!(resolve_key(&ltr, "ArrowRight"), Some(ReaderActionVm::NextPage));
        let vertical = MangaReaderPrefs { mode: ReaderModeVm::Webtoon, ..MangaReaderPrefs::default() };
        assert_eq!(resolve_key(&vertical, "ArrowRight"), Some(ReaderActionVm::NextPage));
        assert_eq!(resolve_key(&rtl, "n"), Some(ReaderActionVm::NextChapter));
        assert_eq!(resolve_key(&rtl, "Shift"), None);
        let rebound = rtl.with_binding(ReaderActionVm::Reload, "x");
        assert_eq!(resolve_key(&rebound, "x"), Some(ReaderActionVm::Reload));
        assert_eq!(resolve_key(&rebound, "r"), None);
    }

    #[test]
    fn page_steps_in_single_and_double_modes() {
        let p = pages(5);
        assert_eq!(step_page(ReaderModeVm::Paged, &p, 0, true), Some(1));
        assert_eq!(step_page(ReaderModeVm::Paged, &p, 4, true), None);
        assert_eq!(step_page(ReaderModeVm::Paged, &p, 0, false), None);
        // spreads: (0), (1,2), (3,4)
        assert_eq!(step_page(ReaderModeVm::DoublePage, &p, 0, true), Some(1));
        assert_eq!(step_page(ReaderModeVm::DoublePage, &p, 2, true), Some(3));
        assert_eq!(step_page(ReaderModeVm::DoublePage, &p, 3, true), None);
        assert_eq!(step_page(ReaderModeVm::DoublePage, &p, 4, false), Some(1));
        assert_eq!(step_page(ReaderModeVm::Paged, &[], 0, true), None);
        assert_eq!(next_mode(ReaderModeVm::Webtoon), ReaderModeVm::Paged);
    }

    #[test]
    fn bridge_messages_parse() {
        let k: ReaderMsg = serde_json::from_str(r#"{"t":"key","key":"ArrowLeft","ctrl":false}"#).unwrap();
        assert_eq!(k, ReaderMsg::Key { key: "ArrowLeft".into(), ctrl: false });
        let s: ReaderMsg = serde_json::from_str(r#"{"t":"scroll","i":3,"end":true}"#).unwrap();
        assert_eq!(s, ReaderMsg::Scroll { i: 3, end: true });
        let z: ReaderMsg = serde_json::from_str(r#"{"t":"zoom","d":-1}"#).unwrap();
        assert_eq!(z, ReaderMsg::Zoom { d: -1 });
    }
}
