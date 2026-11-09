//! "Download season" sheet (wave B, worker **h**; checklist `downloads.season-modal`, `downloads.prefs`; classic
//! `components/anime/SeasonDownloadModal.tsx`). Mounted by worker c in `screens/details.rs`. Pick episodes (grid,
//! "all missing", typed ranges `1-12, 15`), audio language and server (both remembered in `Settings::downloads`),
//! see how many are already downloaded (`data-already-downloaded`), turn on auto-download of new episodes, and queue
//! them with `Backend::download_enqueue_many`. Props are final.

use std::collections::BTreeSet;

use dioxus::prelude::*;

use crate::app::{Route, Services, Ui};
use crate::backend::{DownloadRequestVm, EpisodeVm, ExtensionVm, UiResult};
use crate::components::{
    Banner, Button, ButtonSize, ButtonVariant, EmptyState, ErrorState, LoadingIndicator, Segmented, Sheet, Switch,
    TextField, Tone, use_snackbar,
};
use crate::i18n::{t, tf, tn};
use crate::model::downloads::{have_episodes, is_aired, parse_episode_ranges};
use crate::model::{DUB_LANGS, UiError, format_unix_utc};

/// One cell of the episode grid.
#[derive(Debug, Clone, PartialEq)]
pub struct SeasonEpisode {
    pub number: u32,
    pub title: Option<String>,
    pub aired: bool,
    pub downloaded: bool,
}

/// Grid cells from the episode list (or `1..=episodes_total` when the list is empty).
pub fn season_episodes(eps: &[EpisodeVm], episodes_total: Option<u32>, have: &[u32], today: &str) -> Vec<SeasonEpisode> {
    let mut out: Vec<SeasonEpisode> = if eps.is_empty() {
        (1..=episodes_total.unwrap_or(0).min(5000))
            .map(|n| SeasonEpisode { number: n, title: None, aired: true, downloaded: have.contains(&n) })
            .collect()
    } else {
        eps.iter()
            .map(|e| SeasonEpisode {
                number: e.number,
                title: e.title.clone(),
                aired: is_aired(e.air_date.as_deref(), today),
                downloaded: have.contains(&e.number),
            })
            .collect()
    };
    out.sort_by_key(|e| e.number);
    out.dedup_by_key(|e| e.number);
    out
}

/// Aired episodes that are not downloaded yet ("Select all missing").
pub fn missing(cells: &[SeasonEpisode]) -> BTreeSet<u32> {
    cells.iter().filter(|c| c.aired && !c.downloaded).map(|c| c.number).collect()
}

fn today() -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    format_unix_utc(now).get(..10).unwrap_or_default().to_owned()
}

#[component]
pub fn DownloadSeasonSheet(
    open: bool,
    /// `MediaVm::id` of the title.
    media_id: String,
    #[props(default)] episodes_total: Option<u32>,
    #[props(default)] on_close: EventHandler<()>,
) -> Element {
    if !open {
        return rsx! {};
    }
    rsx! {
        SeasonSheetBody { media_id: media_id, episodes_total: episodes_total, on_close: on_close }
    }
}

type SeasonData = (UiResult<Vec<EpisodeVm>>, Vec<u32>, Vec<ExtensionVm>);

#[component]
fn SeasonSheetBody(media_id: String, episodes_total: Option<u32>, on_close: EventHandler<()>) -> Element {
    let services = use_context::<Services>();
    let ui = use_context::<Ui>();
    let navigator = use_navigator();
    let snack = use_snackbar();
    let backend = services.backend.clone();

    let mut data = use_resource({
        let (backend, id) = (backend.clone(), media_id.clone());
        move || {
            let (backend, id) = (backend.clone(), id.clone());
            async move {
                let eps = backend.episodes(&id).await;
                let rows = backend.downloads().await.unwrap_or_default();
                let exts = backend.extensions().await.unwrap_or_default();
                let out: SeasonData = (eps, have_episodes(&rows, &id), exts);
                out
            }
        }
    });
    let mut selected = use_signal(BTreeSet::<u32>::new);
    let mut range_text = use_signal(String::new);
    let mut lang = use_signal(|| ui.settings.peek().downloads.preferred_audio.clone());
    let mut server = use_signal(|| ui.settings.peek().downloads.preferred_server.clone());
    let mut busy = use_signal(|| false);
    let mut error = use_signal(|| None::<UiError>);

    let open_downloads = use_callback(move |_: ()| {
        navigator.push(Route::DownloadsPage {});
    });

    let confirm = use_callback({
        let (backend, id) = (backend.clone(), media_id.clone());
        move |_: ()| {
            let eps: Vec<u32> = selected.peek().iter().copied().collect();
            if eps.is_empty() || busy() {
                return;
            }
            busy.set(true);
            error.set(None);
            // Remember the language / server for the next downloads (`downloads.prefs`) before queueing: the backend
            // reads the saved prefs.
            let mut next = ui.settings.peek().clone();
            next.downloads.preferred_audio = lang.peek().clone();
            next.downloads.preferred_server = server.peek().clone();
            next.downloads = next.downloads.sanitized();
            let changed = *ui.settings.peek() != next;
            let req = DownloadRequestVm { audio_lang: Some(next.downloads.preferred_audio.clone()), ..DownloadRequestVm::default() };
            let (backend, id) = (backend.clone(), id.clone());
            spawn(async move {
                if changed {
                    let mut settings = ui.settings;
                    settings.set(next.clone());
                    if let Err(e) = backend.save_settings(&next).await {
                        tracing::warn!(detail = %e.detail, "saving the download prefs failed");
                    }
                }
                match backend.download_enqueue_many(&id, &eps, &req).await {
                    Ok(ids) => {
                        snack.show_action(tn("downloads.season.queued", ids.len()), t("downloads.season.open"), open_downloads);
                        on_close.call(());
                    }
                    Err(e) => error.set(Some(e)),
                }
                busy.set(false);
            });
        }
    });

    let toggle_auto = use_callback({
        let backend = backend.clone();
        move |on: bool| {
            let mut next = ui.settings.peek().clone();
            next.downloads.auto_new_episodes = on;
            ui.commit_settings(&backend, next);
        }
    });

    let content = match &*data.read() {
        None => rsx! {
            div { class: "tk-dlseason__loading", "aria-busy": "true", LoadingIndicator {} }
        },
        Some((eps, have, exts)) => {
            let list: &[EpisodeVm] = match eps {
                Ok(v) => v,
                Err(_) => &[],
            };
            let cells = season_episodes(list, episodes_total, have, &today());
            match eps {
                Err(e) if cells.is_empty() => rsx! {
                    ErrorState { error: e.clone(), title: Some(t("downloads.season.error")), on_retry: move |_| data.restart() }
                },
                _ if cells.is_empty() => rsx! {
                    EmptyState { icon: "download".to_string(), title: t("downloads.season.empty") }
                },
                _ => {
                    let already = cells.iter().filter(|c| c.downloaded).count();
                    let max = cells.last().map(|c| c.number);
                    let miss = missing(&cells);
                    let downloaded: BTreeSet<u32> = cells.iter().filter(|c| c.downloaded).map(|c| c.number).collect();
                    let sel = selected();
                    let mut server_opts = vec![("auto".to_owned(), t("downloads.season.server_auto"))];
                    server_opts.extend(exts.iter().filter(|e| e.enabled).map(|e| (e.id.clone(), e.name.clone())));
                    let server_sel = server().unwrap_or_else(|| "auto".to_owned());
                    let lang_opts: Vec<(String, String)> = DUB_LANGS.iter().map(|l| ((*l).to_owned(), l.to_uppercase())).collect();
                    let auto_on = ui.settings.read().downloads.auto_new_episodes;
                    let (miss_a, down_a, down_b) = (miss.clone(), downloaded.clone(), downloaded);
                    rsx! {
                        p { class: "tk-dlseason__already", "data-already-downloaded": "{already}",
                            {tn("downloads.season.already", already)}
                        }
                        div { class: "tk-dlseason__quick",
                            Button {
                                label: t("downloads.season.select_missing"),
                                variant: ButtonVariant::Tonal,
                                size: ButtonSize::Sm,
                                disabled: miss.is_empty(),
                                onclick: move |_| selected.set(miss_a.clone()),
                            }
                            Button {
                                label: t("downloads.season.clear"),
                                variant: ButtonVariant::Text,
                                size: ButtonSize::Sm,
                                disabled: sel.is_empty(),
                                onclick: move |_| selected.set(BTreeSet::new()),
                            }
                            div { class: "tk-dlseason__range",
                                TextField {
                                    label: t("downloads.season.range"),
                                    value: range_text(),
                                    placeholder: "1-12, 15".to_string(),
                                    hint: t("downloads.season.range_hint"),
                                    oninput: move |v: String| range_text.set(v),
                                    onsubmit: move |_| {
                                        let picked: BTreeSet<u32> = parse_episode_ranges(&range_text.peek(), max)
                                            .into_iter()
                                            .filter(|e| !down_b.contains(e))
                                            .collect();
                                        selected.set(picked);
                                    },
                                }
                                Button {
                                    label: t("downloads.season.apply"),
                                    variant: ButtonVariant::Outlined,
                                    size: ButtonSize::Sm,
                                    onclick: move |_| {
                                        let picked: BTreeSet<u32> = parse_episode_ranges(&range_text.peek(), max)
                                            .into_iter()
                                            .filter(|e| !down_a.contains(e))
                                            .collect();
                                        selected.set(picked);
                                    },
                                }
                            }
                        }
                        div {
                            class: "tk-dlseason__eps",
                            role: "group",
                            "aria-label": t("downloads.season.episodes"),
                            "data-episodes-selectable": "true",
                            for c in cells.iter().cloned() {
                                button {
                                    key: "{c.number}",
                                    class: "tk-dlseason__ep",
                                    r#type: "button",
                                    "data-episode": "{c.number}",
                                    "data-downloaded": "{c.downloaded}",
                                    "data-aired": "{c.aired}",
                                    "aria-pressed": "{sel.contains(&c.number)}",
                                    disabled: c.downloaded,
                                    title: c.title.clone().unwrap_or_default(),
                                    onclick: move |_| {
                                        let mut s = selected.peek().clone();
                                        if !s.remove(&c.number) {
                                            s.insert(c.number);
                                        }
                                        selected.set(s);
                                    },
                                    "{c.number}"
                                }
                            }
                        }
                        div { class: "tk-dlseason__opts",
                            div { class: "tk-dlseason__opt",
                                span { class: "tk-dlseason__opt-label", {t("downloads.season.language")} }
                                Segmented {
                                    options: lang_opts,
                                    selected: lang(),
                                    label: t("downloads.season.language"),
                                    small: true,
                                    on_select: move |id: String| lang.set(id),
                                }
                            }
                            div { class: "tk-dlseason__opt",
                                span { class: "tk-dlseason__opt-label", {t("downloads.season.server")} }
                                Segmented {
                                    options: server_opts,
                                    selected: server_sel,
                                    label: t("downloads.season.server"),
                                    small: true,
                                    on_select: move |id: String| server.set(Some(id).filter(|s| s != "auto")),
                                }
                            }
                            div { class: "tk-dlseason__opt tk-dlseason__opt--switch",
                                div { class: "tk-dlseason__opt-text",
                                    span { class: "tk-dlseason__opt-label", {t("downloads.season.auto")} }
                                    span { class: "tk-dlseason__opt-hint", {t("downloads.season.auto_hint")} }
                                }
                                Switch { checked: auto_on, label: t("downloads.season.auto"), on_toggle: move |v: bool| toggle_auto.call(v) }
                            }
                        }
                        if eps.is_err() {
                            Banner { text: t("downloads.season.fallback"), icon: "info".to_string(), tone: Tone::Neutral }
                        }
                    }
                }
            }
        }
    };

    let n = selected().len();
    let ready = matches!(&*data.read(), Some(_));
    rsx! {
        Sheet {
            title: t("downloads.season.title"),
            wide: true,
            on_close: move |_| on_close.call(()),
            footer: rsx! {
                Button { label: t("common.cancel"), variant: ButtonVariant::Text, onclick: move |_| on_close.call(()) }
                Button {
                    label: if n == 0 { t("downloads.season.confirm_none") } else { tn("downloads.season.confirm", n) },
                    icon: "download".to_string(),
                    disabled: busy() || n == 0 || !ready,
                    onclick: move |_| confirm.call(()),
                }
            },
            div { class: "tk-dlseason", "data-selected-count": "{n}",
                {content}
                if busy() {
                    div { class: "tk-dlseason__busy", LoadingIndicator { small: true } span { {tf("downloads.season.queueing", &[("n", &n.to_string())])} } }
                }
                if let Some(e) = error() {
                    ErrorState { error: e, compact: true, retry: false, title: Some(t("downloads.season.failed")) }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ep(n: u32, date: Option<&str>) -> EpisodeVm {
        EpisodeVm { number: n, title: None, image: None, air_date: date.map(str::to_owned), is_filler: false, watched: 0.0 }
    }

    #[test]
    fn grid_cells() {
        let eps = vec![ep(2, Some("2026-01-08")), ep(1, Some("2026-01-01")), ep(3, Some("2027-01-01"))];
        let cells = season_episodes(&eps, None, &[1], "2026-11-09");
        assert_eq!(cells.iter().map(|c| c.number).collect::<Vec<_>>(), vec![1, 2, 3]);
        assert!(cells[0].downloaded);
        assert!(!cells[2].aired);
        assert_eq!(missing(&cells).into_iter().collect::<Vec<_>>(), vec![2]);
        let fallback = season_episodes(&[], Some(3), &[], "2026-11-09");
        assert_eq!(fallback.len(), 3);
        assert!(season_episodes(&[], None, &[], "2026-11-09").is_empty());
    }
}
