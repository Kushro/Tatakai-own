//! Horizontal rows with prev / next arrows (2026-11 UI refresh, Home rows).
//!
//! The arrows appear while the row is hovered and scroll by about one viewport (`scrollBy` through
//! `document::eval`, smooth unless reduced motion). A small script, installed once per window, keeps
//! `data-prev` / `data-next` on the `.tk-rowx` wrapper up to date (scroll listener throttled with
//! `requestAnimationFrame`, a `ResizeObserver` and a `MutationObserver` per track): CSS uses them to show the
//! arrows and the faded right edge. No Rust handler runs on scroll.

use std::sync::atomic::{AtomicU32, Ordering};

use dioxus::prelude::*;

use crate::i18n::t;
use crate::icons::Icon;

static NEXT_ROW: AtomicU32 = AtomicU32::new(0);

/// Installs `window.__tkRows` once (idempotent).
const ROWS_JS: &str = r#"(() => {
  if (window.__tkRows) return;
  const upd = (el) => {
    const box = el.parentElement && el.parentElement.closest('.tk-rowx');
    if (!box) return;
    const max = el.scrollWidth - el.clientWidth;
    const prev = el.scrollLeft > 4 ? 'true' : 'false';
    const next = el.scrollLeft < max - 4 ? 'true' : 'false';
    if (box.dataset.prev !== prev) box.dataset.prev = prev;
    if (box.dataset.next !== next) box.dataset.next = next;
  };
  window.__tkRows = {
    init(id) {
      const el = document.getElementById(id);
      if (!el) return;
      if (el.__tkRow) { upd(el); return; }
      el.__tkRow = true;
      let raf = 0;
      el.addEventListener('scroll', () => {
        if (!raf) raf = requestAnimationFrame(() => { raf = 0; upd(el); });
      }, { passive: true });
      if (window.ResizeObserver) new ResizeObserver(() => upd(el)).observe(el);
      new MutationObserver(() => upd(el)).observe(el, { childList: true });
      requestAnimationFrame(() => upd(el));
    },
    scroll(id, dir) {
      const el = document.getElementById(id);
      if (!el) return;
      const reduce = document.documentElement.dataset.reduceMotion === 'true'
        || window.matchMedia('(prefers-reduced-motion: reduce)').matches;
      el.scrollBy({ left: dir * Math.max(el.clientWidth * 0.9, 160), behavior: reduce ? 'auto' : 'smooth' });
    },
  };
})();"#;

/// Element id for a row track, unique in this window.
pub fn use_row_id() -> String {
    use_hook(|| format!("tk-row-{}", NEXT_ROW.fetch_add(1, Ordering::Relaxed)))
}

/// Script that starts tracking the row `id` (ids come from [`use_row_id`]).
pub fn row_init_js(id: &str) -> String {
    format!("{ROWS_JS}\nwindow.__tkRows.init({});", js_str(id))
}

/// Script that scrolls the row `id` one viewport forward (`dir` > 0) or back.
pub fn row_scroll_js(id: &str, dir: i8) -> String {
    format!("{ROWS_JS}\nwindow.__tkRows.scroll({}, {});", js_str(id), dir.signum())
}

fn js_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_owned())
}

/// Wraps a scrolling track (the child element whose `id` is `track`) with the arrows and the edge fades.
#[component]
pub fn RowScroller(track: String, children: Element) -> Element {
    let (init_id, prev_id, next_id) = (track.clone(), track.clone(), track);
    rsx! {
        div {
            class: "tk-rowx",
            onmounted: move |_| {
                let _ = document::eval(&row_init_js(&init_id));
            },
            {children}
            button {
                class: "tk-rowx__nav",
                r#type: "button",
                "data-dir": "prev",
                tabindex: "-1",
                "aria-label": t("hero.row.prev"),
                title: t("hero.row.prev"),
                onclick: move |_| {
                    let _ = document::eval(&row_scroll_js(&prev_id, -1));
                },
                Icon { name: "chevron-left".to_string(), size: 22 }
            }
            button {
                class: "tk-rowx__nav",
                r#type: "button",
                "data-dir": "next",
                tabindex: "-1",
                "aria-label": t("hero.row.next"),
                title: t("hero.row.next"),
                onclick: move |_| {
                    let _ = document::eval(&row_scroll_js(&next_id, 1));
                },
                Icon { name: "chevron-right".to_string(), size: 22 }
            }
        }
    }
}

/// A `.tk-row` of cards with [`RowScroller`] arrows.
#[component]
pub fn ScrollRow(children: Element) -> Element {
    let id = use_row_id();
    rsx! {
        RowScroller { track: id.clone(),
            div { class: "tk-row", id: "{id}", {children} }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_quote_the_id_and_clamp_the_direction() {
        let js = row_scroll_js("tk-row-3", 5);
        assert!(js.ends_with("window.__tkRows.scroll(\"tk-row-3\", 1);"));
        let js = row_scroll_js("tk-row-3", -2);
        assert!(js.ends_with("window.__tkRows.scroll(\"tk-row-3\", -1);"));
        let js = row_init_js("a'b\"c");
        assert!(js.ends_with(r#"window.__tkRows.init("a'b\"c");"#));
        assert!(js.starts_with("(() => {"));
    }
}
