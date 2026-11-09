//! Chapter navigator sheet of the reader (worker **l**; checklist `manga.reader.navigation`; classic
//! `ReaderChapterNavigator`): the merged chapter list (best source per number, `model::manga::merge_chapters`), a
//! filter, the current chapter highlighted (`aria-current`), read / downloaded marks. Picking one opens it.

use dioxus::prelude::*;

use crate::components::{EmptyState, Sheet, TextField};
use crate::i18n::{t, tf};
use crate::icons::Icon;
use crate::model::manga::{MergedChapterVm, chapter_label, chapter_matches, same_chapter};

#[component]
pub fn ChapterNavigator(
    merged: Vec<MergedChapterVm>,
    current: f32,
    on_pick: EventHandler<MergedChapterVm>,
    on_close: EventHandler<()>,
) -> Element {
    let mut filter = use_signal(String::new);
    let needle = filter();
    let rows: Vec<MergedChapterVm> = merged
        .iter()
        .filter(|m| chapter_matches(&m.chapter, &needle))
        .cloned()
        .collect();
    rsx! {
        Sheet { title: t("manga.reader.chapters"), on_close: move |_| on_close.call(()),
            div { class: "tk-reader__navigator", "data-chapter-navigator": "{merged.len()}",
                TextField {
                    label: t("manga.chapters.filter"),
                    value: needle.clone(),
                    icon: "search".to_string(),
                    oninput: move |v: String| filter.set(v),
                }
                if rows.is_empty() {
                    EmptyState { icon: "grid".to_string(), title: t("manga.chapters.none_match") }
                } else {
                    ul { class: "tk-list tk-reader__navlist",
                        for m in rows {
                            {
                                let here = same_chapter(m.number, current);
                                let label = tf("manga.chapter.n", &[("n", &chapter_label(m.number))]);
                                let title = m.chapter.title.clone().unwrap_or_default();
                                let read = m.chapter.read >= 0.999;
                                let downloaded = m.chapter.downloaded;
                                let pick = m.clone();
                                rsx! {
                                    li { key: "{m.provider}-{m.chapter.reference.chapter_id}",
                                        button {
                                            class: "tk-reader__navitem",
                                            r#type: "button",
                                            "aria-current": if here { "true" } else { "false" },
                                            "data-read": "{read}",
                                            onclick: move |_| on_pick.call(pick.clone()),
                                            span { class: "tk-reader__navnum", "{label}" }
                                            span { class: "tk-reader__navtitle", "{title}" }
                                            if downloaded {
                                                Icon { name: "download".to_string(), size: 16 }
                                            }
                                            if read {
                                                Icon { name: "check".to_string(), size: 16 }
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
