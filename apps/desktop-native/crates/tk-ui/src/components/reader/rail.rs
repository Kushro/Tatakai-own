//! Thumbnail strip of a chapter's pages (worker **l**; checklist `manga.reader.navigation`; classic
//! `ReaderPageRail.tsx`). Thumbnails load lazily through the `tkpage` handler; the current page is
//! `aria-current="page"` and scrolled into view by the reader.

use dioxus::prelude::*;

use super::page_src;
use crate::backend::PageVm;
use crate::i18n::tf;

#[component]
pub fn PageRail(pages: Vec<PageVm>, current: usize, on_jump: EventHandler<usize>) -> Element {
    rsx! {
        nav { class: "tk-reader__rail", "data-rail": "{pages.len()}", "aria-label": tf("manga.reader.rail", &[("n", &pages.len().to_string())]),
            for (i , p) in pages.iter().enumerate() {
                button {
                    key: "{i}",
                    class: "tk-reader__thumb",
                    r#type: "button",
                    "data-rail-page": "{i + 1}",
                    "aria-current": if i == current { "page" } else { "false" },
                    "aria-label": tf("manga.reader.go_page", &[("n", &(i + 1).to_string())]),
                    onclick: move |_| on_jump.call(i),
                    img { src: page_src(p, 0), loading: "lazy", decoding: "async", alt: "" }
                    span { class: "tk-reader__thumb-num", "{i + 1}" }
                }
            }
        }
    }
}
