//! Settings > Manga reader (wave B, worker **l**; checklist `manga.settings.section`, `manga.reader.fit`,
//! `manga.reader.loading`, `manga.reader.behavior`, `manga.reader.keybinds`, `manga.title-language`,
//! `manga.chapters.providers` preferences; classic `panels/ReaderPanel.tsx`, `DisplayPanel.tsx`): the defaults of
//! `MangaReaderPrefs` (the shared `ReaderPrefsForm`: mode, fit, direction, background, width, gap, brightness,
//! panel crop, loading, prefetch, auto-scroll, zoom, chrome), preferred chapter language and source, automatic best
//! source, the manga title language (separate from the anime one), 18+ manga without asking, and the reader key
//! bindings (`data-reader-shortcut`). Everything through `SettingsAction::SetManga`.

use dioxus::prelude::*;

use super::{SettingRow, lang_seg};
use crate::app::{Services, Ui};
use crate::components::reader::prefs_form::ReaderPrefsForm;
use crate::components::{Button, ButtonSize, ButtonVariant, Kbd, SegOption, Segmented, Switch};
use crate::i18n::{t, tf};
use crate::model::manga::{MANGA_LANGS, MANGA_TITLE_LANGUAGES, MANGADEX_PROVIDER, ReaderActionVm, key_token};
use crate::model::{MangaPrefs, MangaReaderPrefs, SettingsAction};

/// Shown form of a key token (`arrowright` -> `→`, `space` -> the translated name, letters upper-case).
pub fn key_label(token: &str) -> String {
    match token {
        "arrowright" => "→".to_owned(),
        "arrowleft" => "←".to_owned(),
        "arrowup" => "↑".to_owned(),
        "arrowdown" => "↓".to_owned(),
        "space" => t("manga.key.space"),
        "pagedown" => "PgDn".to_owned(),
        "pageup" => "PgUp".to_owned(),
        "enter" => "Enter".to_owned(),
        other if other.chars().count() == 1 => other.to_uppercase(),
        other => {
            let mut c = other.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        }
    }
}

/// `prefs` with `action` back on its default key (an action that took the default meanwhile keeps its own).
pub fn reset_binding(prefs: &MangaReaderPrefs, action: ReaderActionVm) -> MangaReaderPrefs {
    let mut n = prefs.clone();
    n.keybinds.remove(action.id());
    n.sanitized()
}

#[component]
pub fn ReaderSection(commit_saved: Callback<SettingsAction>) -> Element {
    let ui = use_context::<Ui>();
    let services = use_context::<Services>();
    let s = (ui.settings)();
    let m = s.manga.clone();
    let commit = move |next: MangaPrefs| commit_saved.call(SettingsAction::SetManga(next));

    let backend = services.backend.clone();
    let exts = use_resource(move || {
        let backend = backend.clone();
        async move { backend.extensions().await }
    });
    let mut sources: Vec<SegOption> = vec![(String::new(), t("settings.reader.source_auto")), (MANGADEX_PROVIDER.to_owned(), "MangaDex".to_owned())];
    let mut sources_error = None;
    match &*exts.read() {
        Some(Ok(list)) => sources.extend(list.iter().filter(|e| e.enabled).map(|e| (e.id.clone(), e.name.clone()))),
        Some(Err(e)) => sources_error = Some(e.detail.clone()),
        None => {}
    }
    let current_source = m.preferred_source.clone().unwrap_or_default();
    if !current_source.is_empty() && !sources.iter().any(|(id, _)| *id == current_source) {
        sources.push((current_source.clone(), current_source.clone()));
    }
    let title_opts: Vec<SegOption> = MANGA_TITLE_LANGUAGES
        .iter()
        .map(|l| ((*l).to_owned(), if l.is_empty() { t("settings.reader.title_lang.app") } else { t(&format!("settings.reader.title_lang.{l}")) }))
        .collect();

    let (m1, m2, m3, m4, m5, m6) = (m.clone(), m.clone(), m.clone(), m.clone(), m.clone(), m.clone());
    rsx! {
        p { class: "tk-panel__hint", {t("settings.reader.hint")} }
        SettingRow { label: t("settings.reader.defaults"), hint: t("settings.reader.defaults_hint"), stack: true,
            ReaderPrefsForm {
                prefs: m.reader.clone(),
                on_change: move |r: MangaReaderPrefs| {
                    let mut n = m1.clone();
                    n.reader = r;
                    commit(n);
                },
            }
        }
        SettingRow { label: t("settings.reader.lang"), hint: t("settings.reader.lang_hint"),
            Segmented {
                options: lang_seg(&MANGA_LANGS),
                selected: m.preferred_lang.clone(),
                label: t("settings.reader.lang"),
                small: true,
                on_select: move |id: String| {
                    let mut n = m2.clone();
                    n.preferred_lang = id;
                    commit(n);
                },
            }
        }
        SettingRow { label: t("settings.reader.source"), hint: t("settings.reader.source_hint"), stack: true,
            div { "data-preferred-source": "{current_source}",
                Segmented {
                    options: sources,
                    selected: current_source.clone(),
                    label: t("settings.reader.source"),
                    small: true,
                    on_select: move |id: String| {
                        let mut n = m3.clone();
                        n.preferred_source = Some(id).filter(|x| !x.is_empty());
                        commit(n);
                    },
                }
                if let Some(e) = sources_error {
                    p { class: "tk-muted", {tf("settings.reader.source_error", &[("detail", &e)])} }
                }
            }
        }
        SettingRow { label: t("settings.reader.auto_best"), hint: t("settings.reader.auto_best_hint"),
            Switch {
                checked: m.auto_best_source,
                label: t("settings.reader.auto_best"),
                on_toggle: move |v: bool| {
                    let mut n = m4.clone();
                    n.auto_best_source = v;
                    commit(n);
                },
            }
        }
        SettingRow { label: t("settings.reader.title_lang"), hint: t("settings.reader.title_lang_hint"),
            div { "data-manga-title-language": "{m.title_language}",
                Segmented {
                    options: title_opts,
                    selected: m.title_language.clone(),
                    label: t("settings.reader.title_lang"),
                    small: true,
                    on_select: move |id: String| {
                        let mut n = m5.clone();
                        n.title_language = id;
                        commit(n);
                    },
                }
            }
        }
        SettingRow { label: t("settings.reader.adult"), hint: t("settings.reader.adult_hint"),
            Switch {
                checked: m.adult_ok,
                label: t("settings.reader.adult"),
                on_toggle: move |v: bool| {
                    let mut n = m6.clone();
                    n.adult_ok = v;
                    commit(n);
                },
            }
        }
        SettingRow { label: t("settings.reader.keys"), hint: t("settings.reader.keys_hint"), stack: true,
            KeyTable { prefs: m.clone(), on_change: move |n: MangaPrefs| commit(n) }
        }
    }
}

/// Rebindable reader keys (`manga.reader.keybinds`): press "Change", then the new key; Escape cancels.
#[component]
fn KeyTable(prefs: MangaPrefs, on_change: EventHandler<MangaPrefs>) -> Element {
    let mut capturing = use_signal(|| None::<ReaderActionVm>);
    let reader = prefs.reader.clone();
    let customized = !reader.keybinds.is_empty();
    let all_prefs = prefs.clone();
    rsx! {
        div { class: "tk-manga-keys", "data-reader-keys": "{ReaderActionVm::ALL.len()}",
            table { class: "tk-manga-keys__table",
                tbody {
                    for action in ReaderActionVm::ALL {
                        {
                            let key = reader.key_for(action);
                            let custom = reader.keybinds.contains_key(action.id());
                            let label = t(&format!("manga.reader.action.{}", action.id()));
                            let (p1, p2) = (prefs.clone(), prefs.clone());
                            rsx! {
                                tr {
                                    key: "{action.id()}",
                                    "data-reader-shortcut": "{action.id()}",
                                    "data-key": "{key}",
                                    "data-custom": "{custom}",
                                    td { class: "tk-manga-keys__action", "{label}" }
                                    td { class: "tk-manga-keys__key",
                                        if capturing() == Some(action) {
                                            input {
                                                class: "tk-manga-keys__capture",
                                                r#type: "text",
                                                readonly: true,
                                                "aria-label": tf("settings.reader.keys.press_for", &[("action", &label)]),
                                                placeholder: t("settings.reader.keys.press"),
                                                onmounted: move |e| {
                                                    spawn(async move {
                                                        let _ = e.set_focus(true).await;
                                                    });
                                                },
                                                onblur: move |_| capturing.set(None),
                                                onkeydown: move |e: KeyboardEvent| {
                                                    e.prevent_default();
                                                    e.stop_propagation();
                                                    let raw = e.key().to_string();
                                                    if raw == "Escape" {
                                                        capturing.set(None);
                                                        return;
                                                    }
                                                    if let Some(tok) = key_token(&raw) {
                                                        let mut n = p1.clone();
                                                        n.reader = n.reader.with_binding(action, &tok);
                                                        capturing.set(None);
                                                        on_change.call(n);
                                                    }
                                                },
                                            }
                                        } else {
                                            Kbd { keys: key_label(&key) }
                                        }
                                    }
                                    td { class: "tk-manga-keys__ops",
                                        Button {
                                            label: t("settings.reader.keys.change"),
                                            variant: ButtonVariant::Text,
                                            size: ButtonSize::Sm,
                                            onclick: move |_| capturing.set(Some(action)),
                                        }
                                        if custom {
                                            Button {
                                                label: t("settings.reader.keys.reset"),
                                                variant: ButtonVariant::Text,
                                                size: ButtonSize::Sm,
                                                onclick: move |_| {
                                                    let mut n = p2.clone();
                                                    n.reader = reset_binding(&n.reader, action);
                                                    on_change.call(n);
                                                },
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if customized {
                Button {
                    label: t("settings.reader.keys.reset_all"),
                    variant: ButtonVariant::Outlined,
                    size: ButtonSize::Sm,
                    onclick: move |_| {
                        let mut n = all_prefs.clone();
                        n.reader.keybinds.clear();
                        on_change.call(n);
                    },
                }
            }
            p { class: "tk-muted", {t("settings.reader.keys.reserved")} }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_labels_and_reset() {
        assert_eq!(key_label("arrowright"), "→");
        assert_eq!(key_label("n"), "N");
        assert_eq!(key_label("home"), "Home");
        let p = MangaReaderPrefs::default().with_binding(ReaderActionVm::Reload, "x");
        assert_eq!(p.key_for(ReaderActionVm::Reload), "x");
        let back = reset_binding(&p, ReaderActionVm::Reload);
        assert_eq!(back.key_for(ReaderActionVm::Reload), "r");
        assert!(back.keybinds.is_empty());
    }
}
