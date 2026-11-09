//! Novel reader `/novels/:id/read/:ext_id/:chapter_id` (worker **m**; checklist `novel.reader.*`, `novel.tts.*`,
//! `novel.reader.offline`, `novel.reader.progress`; classic `NovelReaderPage.tsx`, `NovelReaderPanel.tsx`). Bare
//! route (no rail).
//!
//! - Text: `Backend::novel_content(&ChapterRefVm)` (offline copy first, `data-offline`), rendered with the
//!   `Settings::novel` typography (`data-font-size`, `data-theme`, CSS variables, `@font-face` of the custom font
//!   served by the `tk-novel` asset handler).
//! - Flow: continuous scroll appends `NovelContentVm::next` near the end (`data-chapter` follows the chapter in
//!   view); without it an end-of-chapter card links to the next one. Progress bar, percent and chapter number in the
//!   top bar, fullscreen, keep-awake (Screen Wake Lock), in-place settings sheet.
//! - Progress: a small scroll observer reports the first visible paragraph (`data-paragraph`); it is saved through
//!   the shared `set_read_progress(ReadKindVm::Novel, ..)` (throttled, and on leaving) and restored on open.
//! - Narration: the bar reads paragraph after paragraph with `tts_speak` (clip played by the web view, or the
//!   native system voice), highlights it (`data-tts-paragraph`), pauses / resumes / stops (`data-tts-state`),
//!   changes speed and voice, and continues into the next chapter when `TtsPrefs::auto_advance` is on.

use std::time::{Duration, Instant};

use dioxus::core::{Task, spawn_forever};
use dioxus::desktop::use_window;
use dioxus::prelude::*;
use serde::Deserialize;

use super::assets::{
    self, PAUSE_JS, RESUME_JS, STOP_JS, js_str, play_clip_js, use_novel_assets, wake_lock_js,
};
use super::controls::{FlowControls, TypographyControls, commit_novel};
use crate::app::{Route, Services, Ui};
use crate::backend::{BackendRef, ChapterRefVm, NovelContentVm, ReadProgressVm, TtsRequestVm, TtsVoiceVm};
use crate::components::{
    Button, ButtonVariant, EmptyState, ErrorState, IconButton, LoadingIndicator, Sheet, SkeletonLines,
};
use crate::i18n::{t, tf};
use crate::model::novel::{chapter_label, chapter_number_of};
use crate::model::{NovelPrefs, ReadKindVm, TtsProviderVm, UiError};

/// Narration speeds offered in the bar (percent).
pub const SPEEDS: [u8; 7] = [50, 75, 100, 125, 150, 175, 200];
/// Saved positions at most this often while scrolling.
const SAVE_EVERY: Duration = Duration::from_millis(1500);

/// `data-tts-state` of the narration bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TtsState {
    Idle,
    Loading,
    Playing,
    Paused,
}

impl TtsState {
    pub fn id(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Loading => "loading",
            Self::Playing => "playing",
            Self::Paused => "paused",
        }
    }
}

/// What the scroll observer reports: chapter index, first visible paragraph, near the end.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
struct ScrollMsg {
    c: i64,
    p: u32,
    end: bool,
}

/// Scroll observer of `#tk-novel-scroll` (throttled; ends when the reader is gone).
const SCROLL_JS: &str = r#"
const root = document.getElementById('tk-novel-scroll');
if (!root) { return; }
let last = '';
const report = () => {
  const top = root.getBoundingClientRect().top + 8;
  let cur = null;
  for (const p of root.querySelectorAll('[data-p]')) {
    if (p.getBoundingClientRect().bottom > top) { cur = p; break; }
  }
  const end = root.scrollTop + root.clientHeight >= root.scrollHeight - 900;
  const msg = cur ? { c: Number(cur.dataset.c), p: Number(cur.dataset.p), end } : { c: -1, p: 0, end };
  const k = JSON.stringify(msg);
  if (k !== last) { last = k; dioxus.send(msg); }
};
let timer = null;
root.addEventListener('scroll', () => {
  if (timer) { return; }
  timer = setTimeout(() => { timer = null; report(); }, 160);
}, { passive: true });
setTimeout(report, 400);
await new Promise((resolve) => {
  const iv = setInterval(() => { if (!root.isConnected) { clearInterval(iv); resolve(); } }, 2000);
});
"#;

/// Run as a plain script when the reader goes away: stop the clip and release the wake lock.
const LEAVE_JS: &str = "(() => { const a = window.__tkNovelAudio; if (a) { try { a.pause(); a.dispatchEvent(new Event('tkstop')); } catch (e) {} } \
if (window.__tkWake) { try { window.__tkWake.release(); } catch (e) {} window.__tkWake = null; } })();";

/// Paragraph progress in percent (`p` of `count`, 0-based).
pub fn percent(p: u32, count: usize) -> u32 {
    if count == 0 {
        return 0;
    }
    (((u64::from(p) + 1) * 100) / count as u64).min(100) as u32
}

/// Position after `pos` in the loaded chapters (`None` at the end of the last one).
pub fn next_position(lens: &[usize], pos: (usize, usize)) -> Option<(usize, usize)> {
    let (c, p) = pos;
    if lens.get(c).is_some_and(|n| p + 1 < *n) {
        return Some((c, p + 1));
    }
    (c + 1..lens.len()).find(|i| lens[*i] > 0).map(|i| (i, 0))
}

/// The inline style of the text column.
pub fn column_style(prefs: &NovelPrefs) -> String {
    format!(
        "--tk-novel-font:{};--tk-novel-size:{}px;--tk-novel-lh:{};--tk-novel-gap:{}em;--tk-novel-width:{}px;--tk-novel-align:{}",
        prefs.font_stack(),
        prefs.font_size,
        f32::from(prefs.line_height) / 100.0,
        f32::from(prefs.paragraph_gap) / 10.0,
        prefs.max_width,
        if prefs.justify { "justify" } else { "start" }
    )
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

fn scroll_to_js(c: usize, p: usize, smooth: bool) -> String {
    format!(
        "const el = document.querySelector('[data-c=\"{c}\"][data-p=\"{p}\"]'); if (el) {{ el.scrollIntoView({{ block: '{block}', behavior: '{behavior}' }}); }}",
        block = if smooth { "center" } else { "start" },
        behavior = if smooth { "smooth" } else { "auto" }
    )
}

#[component]
pub fn NovelReaderPage(id: String, ext_id: String, chapter_id: String) -> Element {
    let k = format!("{id}/{ext_id}/{chapter_id}");
    rsx! {
        ReaderLoader { key: "{k}", id, ext_id, chapter_id }
    }
}

#[component]
fn ReaderLoader(id: String, ext_id: String, chapter_id: String) -> Element {
    let services = use_context::<Services>();
    let navigator = use_navigator();
    use_novel_assets();
    let reference = ChapterRefVm {
        ext_id: ext_id.clone(),
        chapter_id: chapter_id.clone(),
    };
    let mut first = use_resource({
        let backend = services.backend.clone();
        let id = id.clone();
        let reference = reference.clone();
        move || {
            let backend = backend.clone();
            let id = id.clone();
            let reference = reference.clone();
            async move {
                let content = backend.novel_content(&reference).await?;
                // A missing position only means "start at the top".
                let saved = backend
                    .read_progress(ReadKindVm::Novel, &id)
                    .await
                    .unwrap_or_default();
                let at = saved
                    .iter()
                    .find(|p| p.chapter == reference && !p.completed)
                    .map(|p| p.position);
                Ok::<_, UiError>((content, at))
            }
        }
    });
    let close_id = id.clone();
    let body = match &*first.read() {
        None => rsx! {
            div { class: "tk-novel-reader__loading", role: "status",
                LoadingIndicator {}
                SkeletonLines { widths: vec![90, 95, 80, 92, 70] }
            }
        },
        Some(Err(e)) => rsx! {
            div { class: "tk-novel-reader__error",
                ErrorState { error: e.clone(), on_retry: move |_| first.restart(),
                    Button {
                        label: t("novel.reader.back"),
                        variant: ButtonVariant::Text,
                        onclick: move |_| {
                            navigator.replace(Route::NovelDetailsPage { id: close_id.clone() });
                        },
                    }
                }
            }
        },
        Some(Ok((content, _))) if content.paragraphs.is_empty() => rsx! {
            EmptyState { icon: "bookmark".to_string(), title: t("novel.reader.empty.title"), body: t("novel.reader.empty.body") }
        },
        Some(Ok((content, at))) => rsx! {
            ReaderView { media_id: id.clone(), first: content.clone(), resume: *at }
        },
    };
    rsx! {
        div { class: "tk-novel-reader-page", {body} }
    }
}

/// Append the chapter after the last loaded one; `false` when there is none or it failed.
async fn append_next(
    backend: BackendRef,
    mut chapters: Signal<Vec<NovelContentVm>>,
    mut loading: Signal<bool>,
    mut error: Signal<Option<UiError>>,
) -> bool {
    if *loading.peek() {
        return false;
    }
    let Some(next) = chapters.peek().last().and_then(|c| c.next.clone()) else {
        return false;
    };
    if chapters.peek().iter().any(|c| c.reference == next) {
        return false;
    }
    loading.set(true);
    error.set(None);
    let res = backend.novel_content(&next).await;
    loading.set(false);
    match res {
        Ok(c) if !c.paragraphs.is_empty() => {
            chapters.with_mut(|v| v.push(c));
            true
        }
        Ok(_) => false,
        Err(e) => {
            error.set(Some(e));
            false
        }
    }
}

/// Where the reader is: the chapter in view, its paragraph count and the first visible paragraph.
#[derive(Debug, Clone, PartialEq)]
struct Spot {
    chapter: ChapterRefVm,
    total: u32,
    paragraph: u32,
}

fn spot_of(chapters: &[NovelContentVm], c: usize, p: u32) -> Option<Spot> {
    chapters.get(c).map(|ch| Spot {
        chapter: ch.reference.clone(),
        total: ch.paragraphs.len() as u32,
        paragraph: p,
    })
}

/// Save a reading position (shared `read_progress`).
async fn save_position(backend: BackendRef, media_id: String, spot: Spot) -> Result<(), UiError> {
    let (total, p) = (spot.total, spot.paragraph);
    let progress = ReadProgressVm {
        chapter_number: chapter_number_of(&spot.chapter.chapter_id).unwrap_or(0.0),
        chapter: spot.chapter.clone(),
        position: p,
        total: Some(total),
        completed: total > 0 && p + 1 >= total,
        updated_unix: now_unix(),
    };
    backend
        .set_read_progress(ReadKindVm::Novel, &media_id, &progress)
        .await
}

#[component]
fn ReaderView(media_id: String, first: NovelContentVm, resume: Option<u32>) -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let navigator = use_navigator();
    let win = use_window();
    let backend = services.backend.clone();
    let prefs = ui.settings.read().novel.clone();
    let reduce_motion = ui.settings.read().reduce_motion;

    let chapters = use_signal(|| vec![first.clone()]);
    let mut current = use_signal(|| (0_usize, resume.unwrap_or(0)));
    let next_loading = use_signal(|| false);
    let next_error = use_signal(|| None::<UiError>);
    let mut sheet = use_signal(|| false);
    let mut fullscreen = use_signal(|| false);
    let mut save_error = use_signal(|| false);
    let mut wake_ok = use_signal(|| None::<bool>);
    let last_saved =
        use_hook(|| std::rc::Rc::new(std::cell::Cell::new((usize::MAX, u32::MAX, Instant::now()))));
    // Mirror of the position for `use_drop` (signals may already be gone when it runs).
    let last_spot = use_hook(|| {
        std::rc::Rc::new(std::cell::RefCell::new(spot_of(
            std::slice::from_ref(&first),
            0,
            resume.unwrap_or(0),
        )))
    });

    // Narration.
    let tts_state = use_signal(|| TtsState::Idle);
    let tts_at = use_signal(|| None::<(usize, usize)>);
    let tts_native = use_signal(|| false);
    let tts_task = use_signal(|| None::<Task>);
    let tts_error = use_signal(|| None::<UiError>);

    // Restore the saved paragraph once the first chapter is on screen.
    use_effect(move || {
        if let Some(p) = resume.filter(|p| *p > 0) {
            spawn(async move {
                tokio::time::sleep(Duration::from_millis(120)).await;
                let _ = document::eval(&scroll_to_js(0, p as usize, false));
            });
        }
    });

    // Scroll observer: position, progress saving, continuous loading.
    use_future({
        let backend = backend.clone();
        let media_id = media_id.clone();
        let last_saved = last_saved.clone();
        let last_spot = last_spot.clone();
        move || {
            let backend = backend.clone();
            let media_id = media_id.clone();
            let last_saved = last_saved.clone();
            let last_spot = last_spot.clone();
            async move {
                let mut ev = document::eval(SCROLL_JS);
                while let Ok(msg) = ev.recv::<ScrollMsg>().await {
                    if msg.c >= 0 {
                        let c = msg.c as usize;
                        current.set((c, msg.p));
                        let spot = spot_of(&chapters.peek(), c, msg.p);
                        *last_spot.borrow_mut() = spot.clone();
                        let (lc, lp, at) = last_saved.get();
                        if (lc != c || lp != msg.p) && (lc != c || at.elapsed() >= SAVE_EVERY) {
                            last_saved.set((c, msg.p, Instant::now()));
                            if let Some(spot) = spot {
                                let ok = save_position(backend.clone(), media_id.clone(), spot)
                                    .await
                                    .is_ok();
                                save_error.set(!ok);
                            }
                        }
                    }
                    let continuous = ui.settings.peek().novel.flow.continuous;
                    if msg.end && continuous {
                        append_next(backend.clone(), chapters, next_loading, next_error).await;
                    }
                }
            }
        }
    });

    // Keep the screen on while reading (when enabled).
    let keep_awake = prefs.flow.keep_awake;
    use_effect(use_reactive((&keep_awake,), move |(keep_awake,)| {
        spawn(async move {
            let mut ev = document::eval(&wake_lock_js(keep_awake));
            if let Ok(ok) = ev.recv::<bool>().await {
                wake_ok.set(Some(ok));
            }
        });
    }));

    // Leaving: save the last position, stop narration, release the wake lock, leave fullscreen.
    // (The narration task is owned by this scope and ends with it.)
    use_drop({
        let backend = backend.clone();
        let media_id = media_id.clone();
        let win = win.clone();
        let last_spot = last_spot.clone();
        move || {
            let spot = last_spot.borrow().clone();
            let b = backend.clone();
            spawn_forever(async move {
                if let Some(spot) = spot {
                    let _ = save_position(b.clone(), media_id, spot).await;
                }
                let _ = b.tts_stop().await;
            });
            let _ = win.webview.evaluate_script(LEAVE_JS);
            win.set_fullscreen(false);
        }
    });

    let list = chapters();
    let (cur_c, cur_p) = current();
    let cur_chapter = list.get(cur_c).or(list.first()).cloned();
    let count = cur_chapter.as_ref().map_or(0, |c| c.paragraphs.len());
    let pct = percent(cur_p, count);
    let number = cur_chapter
        .as_ref()
        .and_then(|c| chapter_number_of(&c.reference.chapter_id));
    let chapter_text = number
        .map(|n| tf("novel.chapter.n", &[("n", &chapter_label(n))]))
        .unwrap_or_default();
    let title = cur_chapter.as_ref().map(|c| c.title.clone()).unwrap_or_default();
    let offline = cur_chapter.as_ref().is_some_and(|c| c.offline);
    let chapter_attr = cur_chapter
        .as_ref()
        .map(|c| c.reference.chapter_id.clone())
        .unwrap_or_default();
    let number_attr = number.map(chapter_label).unwrap_or_default();
    let tts_now = tts_at();
    let highlight = prefs.tts.highlight;
    let tts_par = tts_now.map(|(_, p)| p.to_string()).unwrap_or_default();
    let tts_ch = tts_now.map(|(c, _)| c.to_string()).unwrap_or_default();
    let style = column_style(&prefs);
    let custom_font = (prefs.font == "custom")
        .then(|| prefs.custom_font_path.clone())
        .flatten()
        .and_then(|p| assets::register("font", std::path::Path::new(&p)));
    let font_css = custom_font.map(|url| {
        format!(
            "@font-face {{ font-family: TkNovelCustom; src: url({}); font-display: swap; }}",
            js_str(&url)
        )
    });
    let last = list.last().cloned();
    let continuous = prefs.flow.continuous;
    let prev_ref = cur_chapter.as_ref().and_then(|c| c.prev.clone());
    let next_ref = cur_chapter.as_ref().and_then(|c| c.next.clone());
    let (has_prev, has_next) = (prev_ref.is_some(), next_ref.is_some());
    let win_key = win.clone();
    let win_fs = win.clone();
    let data_dir = use_hook({
        let backend = backend.clone();
        move || backend.data_dir_display()
    });

    let go = {
        let media_id = media_id.clone();
        move |r: ChapterRefVm| {
            navigator.replace(Route::NovelReaderPage {
                id: media_id.clone(),
                ext_id: r.ext_id,
                chapter_id: r.chapter_id,
            });
        }
    };
    let go_prev = go.clone();
    let go_next = go.clone();
    let go_end = go.clone();
    let close_id = media_id.clone();
    let on_prefs = {
        let backend = backend.clone();
        move |next: NovelPrefs| commit_novel(ui, &backend, next)
    };
    let on_prefs2 = on_prefs.clone();
    let start_tts = {
        let backend = backend.clone();
        move |from: (usize, usize)| {
            start_narration(
                backend.clone(),
                ui,
                chapters,
                next_loading,
                next_error,
                tts_state,
                tts_at,
                tts_native,
                tts_task,
                tts_error,
                from,
                reduce_motion,
            );
        }
    };
    let start_at_click = start_tts.clone();

    rsx! {
        div {
            class: "tk-novel-reader",
            "data-theme": "{prefs.theme.id()}",
            "data-font-size": "{prefs.font_size}",
            "data-font": "{prefs.font}",
            "data-chapter": "{number_attr}",
            "data-chapter-id": "{chapter_attr}",
            "data-paragraph": "{cur_p}",
            "data-offline": "{offline}",
            "data-fullscreen": "{fullscreen()}",
            "data-tts-state": "{tts_state().id()}",
            "data-tts-paragraph": "{tts_par}",
            "data-tts-chapter": "{tts_ch}",
            "data-progress-saved": if save_error() { "error" } else { "ok" },
            tabindex: "-1",
            onkeydown: move |e| {
                if e.key() == Key::Escape {
                    if *sheet.peek() {
                        sheet.set(false);
                    } else if *fullscreen.peek() {
                        fullscreen.set(false);
                        win_key.set_fullscreen(false);
                    }
                }
            },
            if let Some(css) = font_css {
                style { "{css}" }
            }
            header { class: "tk-novel-reader__bar",
                IconButton {
                    icon: "back".to_string(),
                    label: t("novel.reader.back"),
                    onclick: move |_| {
                        navigator.replace(Route::NovelDetailsPage { id: close_id.clone() });
                    },
                }
                div { class: "tk-novel-reader__heading",
                    if prefs.flow.show_chapter && !chapter_text.is_empty() {
                        span { class: "tk-novel-reader__chapter", "{chapter_text}" }
                    }
                    if !title.is_empty() {
                        span { class: "tk-novel-reader__title", "{title}" }
                    }
                }
                if offline {
                    span { class: "tk-novel-reader__offline", title: t("novel.reader.offline"), {t("novel.reader.offline")} }
                }
                if prefs.flow.show_percent {
                    span { class: "tk-novel-reader__pct", "data-percent": "{pct}", "{pct}%" }
                }
                IconButton {
                    icon: "chevron-left".to_string(),
                    label: t("novel.reader.prev"),
                    disabled: !has_prev,
                    onclick: move |_| {
                        if let Some(r) = prev_ref.clone() {
                            go_prev(r);
                        }
                    },
                }
                IconButton {
                    icon: "chevron-right".to_string(),
                    label: t("novel.reader.next"),
                    disabled: !has_next,
                    onclick: move |_| {
                        if let Some(r) = next_ref.clone() {
                            go_next(r);
                        }
                    },
                }
                IconButton {
                    icon: "fullscreen".to_string(),
                    label: t("novel.reader.fullscreen"),
                    toggle: true,
                    toggled: fullscreen(),
                    onclick: move |_| {
                        let on = !*fullscreen.peek();
                        fullscreen.set(on);
                        win_fs.set_fullscreen(on);
                    },
                }
                IconButton {
                    icon: "tune".to_string(),
                    label: t("novel.reader.settings"),
                    onclick: move |_| sheet.set(true),
                }
            }
            if prefs.flow.show_progress {
                div { class: "tk-novel-reader__progress", role: "progressbar", "aria-valuemin": "0", "aria-valuemax": "100", "aria-valuenow": "{pct}",
                    i { style: "width:{pct}%" }
                }
            }
            div { id: "tk-novel-scroll", class: "tk-novel-reader__scroll",
                article { class: "tk-novel-reader__column", style: "{style}",
                    for (ci , ch) in list.iter().cloned().enumerate() {
                        section {
                            key: "{ch.reference.chapter_id}",
                            class: "tk-novel-reader__chapter-block",
                            "data-c-root": "{ci}",
                            "data-count": "{ch.paragraphs.len()}",
                            if ci > 0 {
                                {
                                    let n = chapter_number_of(&ch.reference.chapter_id).map(chapter_label).unwrap_or_default();
                                    let heading = if n.is_empty() { ch.title.clone() } else { tf("novel.chapter.n", &[("n", &n)]) };
                                    rsx! { h2 { class: "tk-novel-reader__divider", "{heading}" } }
                                }
                            } else if !ch.title.is_empty() {
                                h1 { class: "tk-novel-reader__h1", "{ch.title}" }
                            }
                            for (pi , text) in ch.paragraphs.iter().cloned().enumerate() {
                                {
                                    let speaking = highlight && tts_now == Some((ci, pi));
                                    let start = start_at_click.clone();
                                    let playing = tts_state() != TtsState::Idle;
                                    rsx! {
                                        p {
                                            key: "{pi}",
                                            class: "tk-novel-reader__p",
                                            "data-c": "{ci}",
                                            "data-p": "{pi}",
                                            "data-speaking": "{speaking}",
                                            ondoubleclick: move |_| {
                                                if playing {
                                                    start((ci, pi));
                                                }
                                            },
                                            "{text}"
                                        }
                                    }
                                }
                            }
                        }
                    }
                    footer { class: "tk-novel-reader__end",
                        if next_loading() {
                            div { role: "status", LoadingIndicator { small: true } span { class: "tk-muted", {t("novel.reader.loading_next")} } }
                        } else if let Some(e) = next_error() {
                            ErrorState {
                                error: e,
                                compact: true,
                                on_retry: {
                                    let backend = backend.clone();
                                    move |_| {
                                        let backend = backend.clone();
                                        spawn(async move {
                                            append_next(backend, chapters, next_loading, next_error).await;
                                        });
                                    }
                                },
                            }
                        } else if let Some(n) = last.as_ref().and_then(|l| l.next.clone()) {
                            if continuous {
                                p { class: "tk-muted", {t("novel.reader.keep_scrolling")} }
                            } else {
                                Button {
                                    label: t("novel.reader.next_chapter"),
                                    icon: "chevron-right".to_string(),
                                    variant: ButtonVariant::Tonal,
                                    onclick: move |_| go_end(n.clone()),
                                }
                            }
                        } else {
                            p { class: "tk-muted", {t("novel.reader.the_end")} }
                        }
                    }
                }
            }
            TtsBar {
                state: tts_state(),
                error: tts_error(),
                on_play: {
                    let start = start_tts.clone();
                    move |_| {
                        let from = tts_at.peek().unwrap_or(*current.peek());
                        let from = (from.0, from.1 as usize);
                        start(from);
                    }
                },
                on_pause: move |_| pause_narration(tts_state, tts_native, tts_task),
                on_resume: {
                    let start = start_tts.clone();
                    move |_| {
                        if *tts_native.peek() {
                            if let Some(at) = *tts_at.peek() {
                                start(at);
                            }
                        } else {
                            let _ = document::eval(RESUME_JS);
                            let mut st = tts_state;
                            st.set(TtsState::Playing);
                        }
                    }
                },
                on_stop: {
                    let backend = backend.clone();
                    move |_| stop_narration(backend.clone(), tts_state, tts_at, tts_task)
                },
            }
            if sheet() {
                Sheet { title: t("novel.reader.settings"), on_close: move |_| sheet.set(false),
                    div { class: "tk-novel-reader__sheet",
                        h3 { class: "tk-novel-ctl__title", {t("novel.settings.typography")} }
                        TypographyControls { prefs: prefs.clone(), data_dir: data_dir.clone(), on_change: on_prefs }
                        h3 { class: "tk-novel-ctl__title", {t("novel.settings.flow")} }
                        FlowControls { prefs: prefs.clone(), wake_supported: wake_ok(), on_change: on_prefs2 }
                    }
                }
            }
        }
    }
}

fn pause_narration(mut state: Signal<TtsState>, native: Signal<bool>, mut task: Signal<Option<Task>>) {
    if *native.peek() {
        // The OS voice cannot pause: stop it and resume from the same paragraph.
        if let Some(t) = task.peek().as_ref().copied() {
            t.cancel();
        }
        task.set(None);
    } else {
        let _ = document::eval(PAUSE_JS);
    }
    state.set(TtsState::Paused);
}

fn stop_narration(
    backend: BackendRef,
    mut state: Signal<TtsState>,
    mut at: Signal<Option<(usize, usize)>>,
    mut task: Signal<Option<Task>>,
) {
    if let Some(t) = task.peek().as_ref().copied() {
        t.cancel();
    }
    task.set(None);
    let _ = document::eval(STOP_JS);
    spawn(async move {
        let _ = backend.tts_stop().await;
    });
    state.set(TtsState::Idle);
    at.set(None);
}

/// Read from `from` on, paragraph after paragraph (and into the next chapter with auto-advance).
#[allow(clippy::too_many_arguments)]
fn start_narration(
    backend: BackendRef,
    ui: Ui,
    chapters: Signal<Vec<NovelContentVm>>,
    next_loading: Signal<bool>,
    next_error: Signal<Option<UiError>>,
    mut state: Signal<TtsState>,
    mut at: Signal<Option<(usize, usize)>>,
    mut native: Signal<bool>,
    mut task: Signal<Option<Task>>,
    mut error: Signal<Option<UiError>>,
    from: (usize, usize),
    reduce_motion: bool,
) {
    if let Some(t) = task.peek().as_ref().copied() {
        t.cancel();
    }
    let _ = document::eval(STOP_JS);
    error.set(None);
    let handle = spawn(async move {
        let mut pos = Some(from);
        while let Some((c, p)) = pos {
            let text = chapters
                .peek()
                .get(c)
                .and_then(|ch| ch.paragraphs.get(p).cloned());
            let Some(text) = text else { break };
            let tts = ui.settings.peek().novel.tts.clone();
            at.set(Some((c, p)));
            if tts.highlight {
                let _ = document::eval(&scroll_to_js(c, p, !reduce_motion));
            }
            let system = tts.provider == TtsProviderVm::System;
            state.set(if system {
                TtsState::Playing
            } else {
                TtsState::Loading
            });
            let req = TtsRequestVm {
                text,
                provider: tts.provider,
                voice: None,
                rate: tts.rate,
            };
            match backend.tts_speak(&req).await {
                Ok(vm) => match vm.clip {
                    Some(clip) => {
                        native.set(false);
                        let url = assets::register("clip", &clip.path).unwrap_or(clip.url.clone());
                        // Synthesize the next paragraph while this one plays (cached by the backend).
                        let lens: Vec<usize> = chapters.peek().iter().map(|ch| ch.paragraphs.len()).collect();
                        if !system
                            && let Some((nc, np)) = next_position(&lens, (c, p))
                            && let Some(next_text) = chapters
                                .peek()
                                .get(nc)
                                .and_then(|ch| ch.paragraphs.get(np).cloned())
                        {
                            let b = backend.clone();
                            let req = TtsRequestVm {
                                text: next_text,
                                provider: tts.provider,
                                voice: None,
                                rate: tts.rate,
                            };
                            spawn(async move {
                                let _ = b.tts_speak(&req).await;
                            });
                        }
                        if *state.peek() != TtsState::Paused {
                            state.set(TtsState::Playing);
                        }
                        let mut ev = document::eval(&play_clip_js(&url));
                        match ev.recv::<String>().await.as_deref() {
                            Ok("ended") => {}
                            Ok("stopped") => break,
                            _ => {
                                error.set(Some(UiError::new(
                                    crate::model::ErrorKind::Other,
                                    t("novel.tts.play_failed"),
                                )));
                                break;
                            }
                        }
                    }
                    None => native.set(true),
                },
                Err(e) => {
                    error.set(Some(e));
                    break;
                }
            }
            let lens: Vec<usize> = chapters.peek().iter().map(|ch| ch.paragraphs.len()).collect();
            pos = next_position(&lens, (c, p));
            if pos.is_none()
                && tts.auto_advance
                && append_next(backend.clone(), chapters, next_loading, next_error).await
            {
                let lens: Vec<usize> = chapters.peek().iter().map(|ch| ch.paragraphs.len()).collect();
                pos = next_position(&lens, (c, p));
            }
        }
        state.set(TtsState::Idle);
        at.set(None);
        task.set(None);
    });
    task.set(Some(handle));
}

/// Narration bar: play / pause / resume / stop, speed and voice (`novel.tts.controls`).
#[component]
fn TtsBar(
    state: TtsState,
    error: Option<UiError>,
    on_play: EventHandler<()>,
    on_pause: EventHandler<()>,
    on_resume: EventHandler<()>,
    on_stop: EventHandler<()>,
) -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let tts = ui.settings.read().novel.tts.clone();
    let provider = tts.provider;
    let voices = use_resource({
        let backend = services.backend.clone();
        move || {
            let backend = backend.clone();
            let p = ui.settings.read().novel.tts.provider;
            async move { backend.tts_voices(p).await }
        }
    });
    let voice_list: Vec<TtsVoiceVm> = voices
        .read()
        .as_ref()
        .and_then(|r| r.as_ref().ok())
        .cloned()
        .unwrap_or_default();
    let voices_err = voices.read().as_ref().and_then(|r| r.as_ref().err()).cloned();
    let rate = tts.rate;
    let voice_now = tts.voice.clone();
    let commit = {
        let backend = services.backend.clone();
        move |next: crate::model::TtsPrefs| {
            let mut all = ui.settings.peek().novel.clone();
            all.tts = next;
            commit_novel(ui, &backend, all);
        }
    };
    let commit_rate = commit.clone();
    let commit_voice = commit.clone();
    let tts_rate = tts.clone();
    let tts_voice = tts.clone();
    rsx! {
        div { class: "tk-novel-tts", role: "toolbar", "aria-label": t("novel.tts.bar"), "data-tts-state": "{state.id()}", "data-provider": "{provider.id()}",
            match state {
                TtsState::Idle => rsx! {
                    Button { label: t("novel.tts.read_aloud"), icon: "volume".to_string(), variant: ButtonVariant::Tonal, onclick: move |_| on_play.call(()) }
                },
                TtsState::Loading | TtsState::Playing => rsx! {
                    IconButton { icon: "pause".to_string(), label: t("novel.tts.pause"), onclick: move |_| on_pause.call(()) }
                    IconButton { icon: "close".to_string(), label: t("novel.tts.stop"), onclick: move |_| on_stop.call(()) }
                    if state == TtsState::Loading {
                        LoadingIndicator { small: true }
                    }
                },
                TtsState::Paused => rsx! {
                    IconButton { icon: "play".to_string(), label: t("novel.tts.resume"), onclick: move |_| on_resume.call(()) }
                    IconButton { icon: "close".to_string(), label: t("novel.tts.stop"), onclick: move |_| on_stop.call(()) }
                },
            }
            span { class: "tk-novel-tts__provider tk-muted", {t(&format!("novel.tts.provider.{}", provider.id()))} }
            label { class: "tk-novel-tts__ctl",
                span { class: "tk-novel-tts__label", {t("novel.tts.speed")} }
                select {
                    "aria-label": t("novel.tts.speed"),
                    "data-tts-rate": "{rate}",
                    value: "{rate}",
                    onchange: move |e| {
                        if let Ok(n) = e.value().parse::<u8>() {
                            let mut next = tts_rate.clone();
                            next.rate = n;
                            commit_rate(next);
                        }
                    },
                    for s in SPEEDS {
                        option { key: "{s}", value: "{s}", selected: s == rate, {format!("{:.2}x", f32::from(s) / 100.0)} }
                    }
                }
            }
            label { class: "tk-novel-tts__ctl",
                span { class: "tk-novel-tts__label", {t("novel.tts.voice")} }
                select {
                    "aria-label": t("novel.tts.voice"),
                    value: "{voice_now}",
                    disabled: voice_list.is_empty(),
                    onchange: move |e| {
                        let mut next = tts_voice.clone();
                        next.voice = e.value();
                        commit_voice(next);
                    },
                    option { value: "", selected: voice_now.is_empty(), {t("novel.tts.voice_default")} }
                    for v in voice_list.clone() {
                        option { key: "{v.id}", value: "{v.id}", selected: v.id == voice_now, "{v.name}" }
                    }
                }
            }
            if let Some(e) = error {
                ErrorState { error: e, compact: true, retry: false, title: t("novel.tts.failed") }
            } else if let Some(e) = voices_err {
                span { class: "tk-novel-tts__warn tk-muted", title: "{e.detail}", {t("novel.tts.voices_failed")} }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_and_positions() {
        assert_eq!(percent(0, 0), 0);
        assert_eq!(percent(0, 4), 25);
        assert_eq!(percent(3, 4), 100);
        assert_eq!(percent(9, 4), 100);
        let lens = [2, 0, 3];
        assert_eq!(next_position(&lens, (0, 0)), Some((0, 1)));
        assert_eq!(next_position(&lens, (0, 1)), Some((2, 0)));
        assert_eq!(next_position(&lens, (2, 2)), None);
        assert_eq!(TtsState::Paused.id(), "paused");
    }

    #[test]
    fn column_style_has_every_variable() {
        let s = column_style(&NovelPrefs {
            justify: true,
            ..NovelPrefs::default()
        });
        for v in [
            "--tk-novel-font:",
            "--tk-novel-size:18px",
            "--tk-novel-lh:1.6",
            "--tk-novel-gap:0.8em",
            "--tk-novel-width:760px",
            "justify",
        ] {
            assert!(s.contains(v), "{v} missing in {s}");
        }
        assert!(scroll_to_js(1, 2, false).contains("[data-c=\"1\"][data-p=\"2\"]"));
    }
}
