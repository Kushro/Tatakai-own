//! Details > Relations (2026-11 UI refresh): two views following `Settings::look.relations_view`.
//! - Graph: a pan / zoom canvas. The main chain (`Backend::watch_order`, already loaded by the page) runs left to
//!   right with solid SEQUEL / PREQUEL edges; the other relations of the open title (`DetailsExtraVm::relations`)
//!   branch above and below it in rows grouped by type, with dashed edges. No extra network requests. The layout is
//!   pure Rust ([`layout_graph`]); pan, zoom, fit, center and the full-screen overlay are one small inline script
//!   ([`GRAPH_JS`]) attached once to the canvas (it only updates a CSS transform: no Rust work per event).
//! - List: `extra::RelatedTab`, grouped by relation type.

use std::collections::HashSet;

use dioxus::prelude::*;

use super::episodes::IconSegmented;
use super::extra::{ExtraState, OrderState, RelatedTab, format_label, relation_label, relation_rank};
use crate::app::{Route, Services, Ui};
use crate::backend::{MediaVm, RelatedVm, WatchOrderEntryVm};
use crate::components::{EmptyState, ErrorState, MediaCover, SkeletonLines};
use crate::i18n::t;
use crate::icons::Icon;
use crate::model::{RelationsView, SettingsAction, reduce_settings};

/// Node card size (px, world units).
pub const NODE_W: f64 = 208.0;
pub const NODE_H: f64 = 76.0;
/// Horizontal step of the main chain and of a branch row.
const MAIN_STEP: f64 = NODE_W + 96.0;
const ROW_STEP_X: f64 = NODE_W + 24.0;
/// Vertical step between rows (room for the edge labels).
const ROW_STEP_Y: f64 = NODE_H + 76.0;
/// Most nodes in one branch row; a bigger group wraps onto another row further out.
const ROW_MAX: usize = 5;
/// Empty margin around the world.
const PAD: f64 = 40.0;
/// Id of the canvas element the script attaches to (one Details page at a time).
const CANVAS_ID: &str = "tk-relgraph";

#[derive(Debug, Clone, PartialEq)]
pub struct GNode {
    pub media: MediaVm,
    /// `manga` opens the manga details page.
    pub manga: bool,
    pub current: bool,
    /// Relation to the open title (empty for the main chain).
    pub relation: String,
    /// Top-left corner (world px).
    pub x: f64,
    pub y: f64,
}

impl GNode {
    pub fn center(&self) -> (f64, f64) {
        (self.x + NODE_W / 2.0, self.y + NODE_H / 2.0)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct GEdge {
    pub from: usize,
    pub to: usize,
    pub relation: String,
    /// Main story line (solid, primary) vs other relation (dashed).
    pub main: bool,
    /// SVG path (`M … C …`).
    pub path: String,
    /// Center of the label chip.
    pub label_at: (f64, f64),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct GraphLayout {
    pub nodes: Vec<GNode>,
    pub edges: Vec<GEdge>,
    /// World size, padding included (what "fit" fits).
    pub width: f64,
    pub height: f64,
    /// Center of the open title's node ("center on this title").
    pub focus: (f64, f64),
}

/// True for the story-line relations drawn solid.
fn is_main_relation(rel: &str) -> bool {
    matches!(rel, "SEQUEL" | "PREQUEL" | "")
}

/// Point of a cubic bezier at `t`.
fn bezier(p: [(f64, f64); 4], t: f64) -> (f64, f64) {
    let u = 1.0 - t;
    let (b0, b1, b2, b3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    (
        b0 * p[0].0 + b1 * p[1].0 + b2 * p[2].0 + b3 * p[3].0,
        b0 * p[0].1 + b1 * p[1].1 + b2 * p[2].1 + b3 * p[3].1,
    )
}

/// Left-to-right edge between two chain nodes: right middle of `a` to left middle of `b`; label in the middle.
pub fn horizontal_edge(a: &GNode, b: &GNode) -> (String, (f64, f64)) {
    let (x1, y1) = (a.x + NODE_W, a.y + NODE_H / 2.0);
    let (x2, y2) = (b.x, b.y + NODE_H / 2.0);
    let mx = (x1 + x2) / 2.0;
    let path = format!("M{x1:.1} {y1:.1} C{mx:.1} {y1:.1} {mx:.1} {y2:.1} {x2:.1} {y2:.1}");
    (path, bezier([(x1, y1), (mx, y1), (mx, y2), (x2, y2)], 0.5))
}

/// Vertical edge from the open title to a branch node above or below it; the label sits near the branch node (so
/// the chips of one row do not pile up over the source).
pub fn vertical_edge(src: &GNode, dst: &GNode) -> (String, (f64, f64)) {
    let above = dst.y < src.y;
    let x1 = src.x + NODE_W / 2.0;
    let y1 = if above { src.y } else { src.y + NODE_H };
    let x2 = dst.x + NODE_W / 2.0;
    let y2 = if above { dst.y + NODE_H } else { dst.y };
    let my = (y1 + y2) / 2.0;
    let path = format!("M{x1:.1} {y1:.1} C{x1:.1} {my:.1} {x2:.1} {my:.1} {x2:.1} {y2:.1}");
    (path, bezier([(x1, y1), (x1, my), (x2, my), (x2, y2)], 0.75))
}

/// `(min_x, min_y, max_x, max_y)` of the node boxes; zeros when empty.
pub fn bounds(nodes: &[GNode]) -> (f64, f64, f64, f64) {
    if nodes.is_empty() {
        return (0.0, 0.0, 0.0, 0.0);
    }
    nodes.iter().fold((f64::MAX, f64::MAX, f64::MIN, f64::MIN), |(a, b, c, d), n| {
        (a.min(n.x), b.min(n.y), c.max(n.x + NODE_W), d.max(n.y + NODE_H))
    })
}

/// Lays out the franchise graph. `chain` is the watch order (empty while loading or on error); `relations` the
/// open title's relations; `current` the open title. Without a usable chain, the prequels / sequels among
/// `relations` form it around the open title. Pure.
pub fn layout_graph(chain: &[WatchOrderEntryVm], relations: &[RelatedVm], current: &MediaVm) -> GraphLayout {
    // Main line: (media, relation to the previous one, current).
    let mut main: Vec<(MediaVm, String, bool)> = Vec::new();
    if chain.len() > 1 && chain.iter().any(|e| e.current) {
        let mut sorted = chain.to_vec();
        sorted.sort_by_key(|e| e.position);
        main = sorted.into_iter().map(|e| (e.media, e.relation, e.current)).collect();
    } else {
        let anime = |r: &&RelatedVm| r.kind != "manga";
        for r in relations.iter().filter(|r| r.relation == "PREQUEL").filter(anime) {
            main.push((r.media.clone(), String::new(), false));
        }
        main.push((current.clone(), "SEQUEL".to_owned(), true));
        for r in relations.iter().filter(|r| r.relation == "SEQUEL").filter(anime) {
            main.push((r.media.clone(), "SEQUEL".to_owned(), false));
        }
        if let Some(first) = main.first_mut() {
            first.1.clear();
        }
    }
    let cur_idx = main.iter().position(|m| m.2).unwrap_or(0);

    let mut nodes: Vec<GNode> = main
        .iter()
        .enumerate()
        .map(|(i, (media, _, cur))| GNode {
            media: media.clone(),
            manga: false,
            current: *cur,
            relation: String::new(),
            x: i as f64 * MAIN_STEP,
            y: 0.0,
        })
        .collect();
    let mut edges: Vec<GEdge> = Vec::new();
    for i in 1..main.len() {
        let rel = if main[i].1.is_empty() { "SEQUEL".to_owned() } else { main[i].1.clone() };
        let (path, label_at) = horizontal_edge(&nodes[i - 1], &nodes[i]);
        edges.push(GEdge { from: i - 1, to: i, main: is_main_relation(&rel), relation: rel, path, label_at });
    }

    // Branches: every other relation (not already on the line), grouped by type.
    let mut seen: HashSet<String> = main.iter().map(|m| m.0.id.clone()).collect();
    let mut groups: Vec<(String, Vec<&RelatedVm>)> = Vec::new();
    for r in relations {
        if !seen.insert(r.media.id.clone()) {
            continue;
        }
        match groups.iter_mut().find(|g| g.0 == r.relation) {
            Some(g) => g.1.push(r),
            None => groups.push((r.relation.clone(), vec![r])),
        }
    }
    groups.sort_by(|a, b| relation_rank(&a.0).cmp(&relation_rank(&b.0)).then_with(|| a.0.cmp(&b.0)));

    let (cx, _) = nodes[cur_idx].center();
    let (mut rows_above, mut rows_below) = (0_u32, 0_u32);
    for (gi, (_, items)) in groups.iter().enumerate() {
        let up = gi % 2 == 0;
        for row in items.chunks(ROW_MAX) {
            let level = if up {
                rows_above += 1;
                -f64::from(rows_above)
            } else {
                rows_below += 1;
                f64::from(rows_below)
            };
            let row_w = row.len() as f64 * ROW_STEP_X - (ROW_STEP_X - NODE_W);
            let x0 = cx - row_w / 2.0;
            for (j, r) in row.iter().enumerate() {
                nodes.push(GNode {
                    media: r.media.clone(),
                    manga: r.kind == "manga",
                    current: false,
                    relation: r.relation.clone(),
                    x: x0 + j as f64 * ROW_STEP_X,
                    y: level * ROW_STEP_Y,
                });
                let to = nodes.len() - 1;
                let (path, label_at) = vertical_edge(&nodes[cur_idx], &nodes[to]);
                edges.push(GEdge { from: cur_idx, to, relation: r.relation.clone(), main: false, path, label_at });
            }
        }
    }

    // Shift everything so the world starts at (PAD, PAD), then rebuild the edges at their final place.
    let (min_x, min_y, max_x, max_y) = bounds(&nodes);
    let (dx, dy) = (PAD - min_x, PAD - min_y);
    for n in &mut nodes {
        n.x += dx;
        n.y += dy;
    }
    for e in &mut edges {
        let (a, b) = (&nodes[e.from], &nodes[e.to]);
        let (path, at) = if a.y == b.y { horizontal_edge(a, b) } else { vertical_edge(a, b) };
        e.path = path;
        e.label_at = at;
    }
    GraphLayout {
        focus: nodes[cur_idx].center(),
        width: max_x - min_x + 2.0 * PAD,
        height: max_y - min_y + 2.0 * PAD,
        nodes,
        edges,
    }
}

/// Pan / zoom / fit / center / full screen of the graph canvas. `__ID__` is replaced by the canvas id. Reads the
/// world size and the focus point from the `data-*` of `.tk-graph__plane` (set by Rust) and writes only the
/// transform of `.tk-graph__world` (never touched by Rust, so a re-render does not undo it).
const GRAPH_JS: &str = r#"
const g = document.getElementById("__ID__");
if (g && !g.__tkGraph) {
  g.__tkGraph = true;
  const MIN = 0.2, MAX = 2.5;
  let s = 1, tx = 0, ty = 0, touched = false, moved = false, drag = null, pinch = null;
  const pts = new Map();
  const world = () => g.querySelector('.tk-graph__world');
  const dims = () => { const p = g.querySelector('.tk-graph__plane'); return p ? { w: +p.dataset.w, h: +p.dataset.h, cx: +p.dataset.cx, cy: +p.dataset.cy } : null; };
  const apply = () => { const w = world(); if (w) w.style.transform = `translate(${tx}px, ${ty}px) scale(${s})`; };
  const clamp = (v) => Math.max(MIN, Math.min(MAX, v));
  const animate = () => {
    if (document.documentElement.dataset.reduceMotion === 'true') return;
    g.dataset.animate = 'true'; clearTimeout(g.__tkAnim); g.__tkAnim = setTimeout(() => delete g.dataset.animate, 320);
  };
  const fit = () => {
    const d = dims(), W = g.clientWidth, H = g.clientHeight; if (!d || !W || !H || !d.w || !d.h) return;
    s = clamp(Math.min(1.1, W / d.w, H / d.h)); tx = (W - d.w * s) / 2; ty = (H - d.h * s) / 2; apply();
  };
  const center = () => {
    const d = dims(), W = g.clientWidth, H = g.clientHeight; if (!d || !W) return;
    s = 1; tx = W / 2 - d.cx; ty = H / 2 - d.cy; apply();
  };
  const zoomAt = (px, py, k) => { const ns = clamp(s * k); tx = px - (px - tx) * (ns / s); ty = py - (py - ty) * (ns / s); s = ns; apply(); };
  const setFull = (on) => {
    if (on) g.dataset.fullscreen = 'true'; else delete g.dataset.fullscreen;
    requestAnimationFrame(() => { fit(); const c = g.querySelector(on ? '[data-graph="close"]' : '[data-graph="full"]'); if (c) c.focus(); });
  };
  g.addEventListener('pointerdown', (e) => {
    if (e.pointerType === 'mouse' && e.button !== 0) return;
    if (e.target.closest('.tk-graph__tools, .tk-graph__legend, [data-graph="close"]')) return;
    pts.set(e.pointerId, { x: e.clientX, y: e.clientY });
    if (pts.size === 1) { moved = false; drag = { x: e.clientX - tx, y: e.clientY - ty, sx: e.clientX, sy: e.clientY }; }
    else if (pts.size === 2) { const [a, b] = [...pts.values()]; pinch = { d: Math.hypot(a.x - b.x, a.y - b.y) || 1, s }; drag = null; }
  });
  g.addEventListener('pointermove', (e) => {
    if (!pts.has(e.pointerId)) return;
    pts.set(e.pointerId, { x: e.clientX, y: e.clientY });
    if (pinch && pts.size === 2) {
      const [a, b] = [...pts.values()], r = g.getBoundingClientRect();
      zoomAt((a.x + b.x) / 2 - r.left, (a.y + b.y) / 2 - r.top, (pinch.s * Math.hypot(a.x - b.x, a.y - b.y) / pinch.d) / s);
      moved = touched = true; return;
    }
    if (!drag) return;
    if (!moved) {
      if (Math.hypot(e.clientX - drag.sx, e.clientY - drag.sy) < 5) return;
      moved = touched = true; g.dataset.dragging = 'true';
      try { g.setPointerCapture(e.pointerId); } catch (_) {}
    }
    tx = e.clientX - drag.x; ty = e.clientY - drag.y; apply();
  });
  const up = (e) => { pts.delete(e.pointerId); if (pts.size < 2) pinch = null; if (pts.size === 0) { drag = null; delete g.dataset.dragging; } };
  g.addEventListener('pointerup', up);
  g.addEventListener('pointercancel', up);
  // A drag that ends over a node must not open it (capture phase: the app never sees that click).
  g.addEventListener('click', (e) => { if (moved) { moved = false; e.preventDefault(); e.stopPropagation(); } }, true);
  g.addEventListener('wheel', (e) => {
    e.preventDefault(); touched = true;
    const r = g.getBoundingClientRect(), dy = e.deltaY * (e.deltaMode === 1 ? 16 : 1);
    zoomAt(e.clientX - r.left, e.clientY - r.top, Math.exp(-dy * (e.ctrlKey ? 0.01 : 0.0015)));
  }, { passive: false });
  g.addEventListener('click', (e) => {
    const b = e.target.closest('[data-graph]'); if (!b || !g.contains(b)) return;
    const a = b.dataset.graph, W = g.clientWidth, H = g.clientHeight;
    if (a === 'in' || a === 'out') { touched = true; animate(); zoomAt(W / 2, H / 2, a === 'in' ? 1.25 : 0.8); }
    else if (a === 'fit') { touched = false; animate(); fit(); }
    else if (a === 'center') { touched = true; animate(); center(); }
    else if (a === 'full') setFull(g.dataset.fullscreen !== 'true');
    else if (a === 'close') setFull(false);
  });
  // Tabbing to a node outside the view brings it in.
  g.addEventListener('focusin', (e) => {
    const n = e.target.closest('.tk-gnode'); if (!n) return;
    const r = n.getBoundingClientRect(), v = g.getBoundingClientRect();
    if (r.left < v.left || r.right > v.right || r.top < v.top || r.bottom > v.bottom) {
      tx += v.left + v.width / 2 - (r.left + r.width / 2); ty += v.top + v.height / 2 - (r.top + r.height / 2); touched = true; animate(); apply();
    }
  });
  const onKey = (e) => {
    if (!g.isConnected) { document.removeEventListener('keydown', onKey); return; }
    if (e.key === 'Escape' && g.dataset.fullscreen === 'true') { e.preventDefault(); setFull(false); }
  };
  document.addEventListener('keydown', onKey);
  // New data (the watch order arrives after the relations) or a new size (the tab was hidden): fit again until the
  // viewer moves the graph; after that only keep the transform on a re-created world.
  const refresh = () => { if (touched) apply(); else fit(); };
  new MutationObserver(refresh).observe(g, { subtree: true, childList: true, attributes: true, attributeFilter: ['data-w', 'data-h'] });
  if (window.ResizeObserver) new ResizeObserver(refresh).observe(g);
  fit();
}
"#;

fn save_view(ui: Ui, services: &Services, v: RelationsView) {
    let cur = ui.settings.peek().clone();
    if cur.look.relations_view == v {
        return;
    }
    let mut look = cur.look.clone();
    look.relations_view = v;
    ui.commit_settings(&services.backend, reduce_settings(&cur, SettingsAction::SetLook(look)));
}

/// The Relations tab: view switch, then the graph or the grouped list.
#[component]
pub fn RelationsTab(extra: ExtraState, order: OrderState, current: Option<MediaVm>, on_retry: EventHandler<()>) -> Element {
    let ui = use_context::<Ui>();
    let services = use_context::<Services>();
    let view = ui.settings.read().look.relations_view;
    let options: Vec<(String, String, String)> = [(RelationsView::Graph, "graph"), (RelationsView::List, "list")]
        .iter()
        .map(|(v, icon)| (v.id().to_owned(), t(&v.label_key()), (*icon).to_owned()))
        .collect();
    let switch = rsx! {
        div { class: "tk-drelations__tools",
            IconSegmented {
                options,
                selected: view.id().to_string(),
                label: t("episodes.relations.view"),
                on_select: move |v: String| {
                    if let Some(x) = RelationsView::from_id(&v) {
                        save_view(ui, &services, x);
                    }
                },
            }
        }
    };
    let body = match (&extra, view) {
        (_, RelationsView::List) => rsx! { RelatedTab { extra: extra.clone(), on_retry: move |_| on_retry.call(()) } },
        (None, _) => rsx! { SkeletonLines { widths: vec![70, 70, 70] } },
        (Some(Err(e)), _) => rsx! { ErrorState { error: e.clone(), compact: true, on_retry: move |_| on_retry.call(()) } },
        (Some(Ok(x)), RelationsView::Graph) => {
            let chain: Vec<WatchOrderEntryVm> = match &order {
                Some(Ok(list)) => list.clone(),
                _ => Vec::new(),
            };
            if x.relations.is_empty() && chain.len() <= 1 {
                rsx! {
                    EmptyState { icon: "library".to_string(), title: t("details.no_related.title"), body: t("details.no_related.body") }
                }
            } else {
                let me = current
                    .clone()
                    .or_else(|| chain.iter().find(|e| e.current).map(|e| e.media.clone()))
                    .unwrap_or_default();
                rsx! { RelationsGraph { layout: layout_graph(&chain, &x.relations, &me) } }
            }
        }
    };
    rsx! {
        div { class: "tk-drelations", "data-relations-view": "{view.id()}",
            {switch}
            {body}
        }
    }
}

/// Bottom line of a node: status while airing / announced, else the `2024 - 12 ep` line.
fn node_facts(m: &MediaVm, manga: bool) -> (String, String) {
    let format = if manga {
        t("details.kind.manga")
    } else {
        m.format.as_deref().map(format_label).unwrap_or_default()
    };
    let right = match m.status.as_deref() {
        Some(s @ ("RELEASING" | "NOT_YET_RELEASED" | "HIATUS" | "CANCELLED")) => super::status_label(s),
        _ if !m.subtitle.is_empty() => m.subtitle.clone(),
        Some(s) => super::status_label(s),
        None => String::new(),
    };
    (format, right)
}

#[component]
fn RelationsGraph(layout: GraphLayout) -> Element {
    let navigator = use_navigator();
    let (w, h) = (layout.width.ceil(), layout.height.ceil());
    let (fx, fy) = layout.focus;
    let tool = |action: &'static str, icon: &'static str, key: &'static str| {
        let label = t(key);
        rsx! {
            button { class: "tk-graph__tool", r#type: "button", "data-graph": action, "aria-label": "{label}", title: "{label}",
                Icon { name: icon.to_string(), size: 18 }
            }
        }
    };
    rsx! {
        div {
            id: CANVAS_ID,
            class: "tk-graph",
            role: "group",
            "aria-label": t("episodes.graph.label"),
            "data-extra": "relations",
            onmounted: move |_| {
                let _ = document::eval(&GRAPH_JS.replace("__ID__", CANVAS_ID));
            },
            div { class: "tk-graph__world",
                div {
                    class: "tk-graph__plane",
                    style: "width:{w}px;height:{h}px",
                    "data-w": "{w}",
                    "data-h": "{h}",
                    "data-cx": "{fx:.0}",
                    "data-cy": "{fy:.0}",
                    svg { class: "tk-graph__edges", width: "{w}", height: "{h}", "aria-hidden": "true",
                        for (i , e) in layout.edges.iter().enumerate() {
                            path { key: "{i}", d: "{e.path}", "data-main": "{e.main}" }
                        }
                    }
                    for (i , e) in layout.edges.iter().enumerate() {
                        span {
                            key: "l{i}",
                            class: "tk-graph__label",
                            "data-main": "{e.main}",
                            "aria-hidden": "true",
                            style: "left:{e.label_at.0:.0}px;top:{e.label_at.1:.0}px",
                            {relation_label(&e.relation)}
                        }
                    }
                    for n in layout.nodes.iter().cloned() {
                        {
                            let (format, facts) = node_facts(&n.media, n.manga);
                            let id = n.media.id.clone();
                            let (manga, current) = (n.manga, n.current);
                            rsx! {
                                button {
                                    key: "{n.media.id}",
                                    class: "tk-gnode",
                                    r#type: "button",
                                    style: "left:{n.x:.0}px;top:{n.y:.0}px",
                                    title: "{n.media.title}",
                                    "aria-current": if current { "true" } else { "false" },
                                    "data-kind": if manga { "manga" } else { "anime" },
                                    "data-relation": "{n.relation}",
                                    onclick: move |_| {
                                        if current {
                                            return;
                                        }
                                        if manga {
                                            navigator.push(Route::MangaDetailsPage { id: id.clone() });
                                        } else {
                                            navigator.push(Route::DetailsPage { id: id.clone() });
                                        }
                                    },
                                    span { class: "tk-gnode__cover",
                                        MediaCover { src: n.media.cover.clone(), color: n.media.color.clone(), title: n.media.title.clone() }
                                    }
                                    span { class: "tk-gnode__text",
                                        span { class: "tk-gnode__title", "{n.media.title}" }
                                        span { class: "tk-gnode__facts",
                                            span { "{format}" }
                                            span { "{facts}" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            div { class: "tk-graph__legend", "aria-hidden": "true",
                span { "data-main": "true", i {} {t("episodes.graph.legend.main")} }
                span { "data-main": "false", i {} {t("episodes.graph.legend.other")} }
            }
            div { class: "tk-graph__tools", role: "toolbar", "aria-label": t("episodes.graph.tools"),
                {tool("in", "zoom-in", "episodes.graph.zoom_in")}
                {tool("out", "zoom-out", "episodes.graph.zoom_out")}
                {tool("fit", "fit", "episodes.graph.fit")}
                {tool("center", "dot", "episodes.graph.center")}
                {tool("full", "fullscreen", "episodes.graph.fullscreen")}
            }
            button {
                class: "tk-graph__close",
                r#type: "button",
                "data-graph": "close",
                "aria-label": t("episodes.graph.close"),
                title: t("episodes.graph.close"),
                Icon { name: "close".to_string(), size: 20 }
            }
            p { class: "tk-graph__hint", "aria-hidden": "true", {t("episodes.graph.hint")} }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn media(id: &str) -> MediaVm {
        MediaVm { id: id.into(), title: id.into(), ..MediaVm::default() }
    }
    fn entry(id: &str, position: u32, relation: &str, current: bool) -> WatchOrderEntryVm {
        WatchOrderEntryVm { media: media(id), position, relation: relation.into(), current }
    }
    fn rel(id: &str, relation: &str, kind: &str) -> RelatedVm {
        RelatedVm { media: media(id), relation: relation.into(), kind: kind.into() }
    }
    fn node<'a>(l: &'a GraphLayout, id: &str) -> &'a GNode {
        l.nodes.iter().find(|n| n.media.id == id).unwrap()
    }

    #[test]
    fn the_chain_runs_left_to_right_with_solid_edges() {
        let chain = [entry("s2", 2, "SEQUEL", true), entry("s1", 1, "", false), entry("s3", 3, "SEQUEL", false)];
        let l = layout_graph(&chain, &[], &media("s2"));
        let ids: Vec<&str> = l.nodes.iter().map(|n| n.media.id.as_str()).collect();
        assert_eq!(ids, ["s1", "s2", "s3"], "sorted by position");
        assert!(node(&l, "s1").x < node(&l, "s2").x && node(&l, "s2").x < node(&l, "s3").x);
        assert!(l.nodes.iter().all(|n| n.y == PAD));
        assert_eq!(l.edges.len(), 2);
        assert!(l.edges.iter().all(|e| e.main && e.relation == "SEQUEL"));
        assert!(node(&l, "s2").current);
        assert_eq!(l.focus, node(&l, "s2").center());
        // Bounds: padding on every side.
        assert_eq!(node(&l, "s1").x, PAD);
        assert_eq!(l.width, 2.0 * MAIN_STEP + NODE_W + 2.0 * PAD);
        assert_eq!(l.height, NODE_H + 2.0 * PAD);
    }

    #[test]
    fn other_relations_branch_above_and_below_grouped_by_type() {
        let chain = [entry("s1", 1, "", false), entry("s2", 2, "SEQUEL", true)];
        let rels = [
            rel("s1", "PREQUEL", "anime"), // already on the line: not repeated
            rel("ova", "SIDE_STORY", "anime"),
            rel("mg", "SOURCE", "manga"),
            rel("ova2", "SIDE_STORY", "anime"),
            rel("alt", "ALTERNATIVE", "anime"),
        ];
        let l = layout_graph(&chain, &rels, &media("s2"));
        assert_eq!(l.nodes.len(), 6);
        let main_y = node(&l, "s2").y;
        // Rank order: SOURCE (3) above, SIDE_STORY (4) below, ALTERNATIVE (6) above, further out.
        assert!(node(&l, "mg").y < main_y);
        assert!(node(&l, "ova").y > main_y && node(&l, "ova2").y == node(&l, "ova").y);
        assert!(node(&l, "alt").y < node(&l, "mg").y);
        assert!(node(&l, "mg").manga);
        // One dashed edge per branch node, from the open title.
        let s2 = l.nodes.iter().position(|n| n.media.id == "s2").unwrap();
        let branch: Vec<&GEdge> = l.edges.iter().filter(|e| !e.main).collect();
        assert_eq!(branch.len(), 4);
        assert!(branch.iter().all(|e| e.from == s2));
        // A row is centered under / over the open title.
        let row_mid = (node(&l, "ova").center().0 + node(&l, "ova2").center().0) / 2.0;
        assert!((row_mid - node(&l, "s2").center().0).abs() < 0.01);
        // Everything is inside the bounds.
        assert!(l.nodes.iter().all(|n| n.x >= PAD && n.y >= PAD && n.x + NODE_W <= l.width && n.y + NODE_H <= l.height));
    }

    #[test]
    fn without_a_chain_prequels_and_sequels_form_it() {
        let rels = [rel("next", "SEQUEL", "anime"), rel("prev", "PREQUEL", "anime"), rel("sp", "SPIN_OFF", "anime")];
        let l = layout_graph(&[], &rels, &media("me"));
        assert!(node(&l, "prev").x < node(&l, "me").x && node(&l, "me").x < node(&l, "next").x);
        assert_eq!(node(&l, "prev").y, node(&l, "me").y);
        assert_ne!(node(&l, "sp").y, node(&l, "me").y);
        assert_eq!(l.edges.iter().filter(|e| e.main).count(), 2);
        // Alone: one node, no edges.
        let solo = layout_graph(&[], &[], &media("me"));
        assert_eq!(solo.nodes.len(), 1);
        assert!(solo.edges.is_empty());
        assert_eq!(solo.focus, (PAD + NODE_W / 2.0, PAD + NODE_H / 2.0));
    }

    #[test]
    fn big_groups_wrap_into_more_rows() {
        let rels: Vec<RelatedVm> = (0..7).map(|i| rel(&format!("c{i}"), "CHARACTER", "anime")).collect();
        let l = layout_graph(&[], &rels, &media("me"));
        let ys: HashSet<i64> = l.nodes.iter().filter(|n| !n.current).map(|n| n.y as i64).collect();
        assert_eq!(ys.len(), 2, "5 + 2");
    }

    #[test]
    fn edges_anchor_on_the_node_sides() {
        let a = GNode { media: media("a"), manga: false, current: false, relation: String::new(), x: 0.0, y: 0.0 };
        let b = GNode { x: 300.0, ..a.clone() };
        let (p, at) = horizontal_edge(&a, &b);
        assert!(p.starts_with(&format!("M{:.1} {:.1}", NODE_W, NODE_H / 2.0)), "{p}");
        assert!(p.ends_with(&format!("300.0 {:.1}", NODE_H / 2.0)), "{p}");
        assert_eq!(at, ((NODE_W + 300.0) / 2.0, NODE_H / 2.0));
        // Below: from the bottom of the source to the top of the target; the label sits near the target.
        let c = GNode { y: 200.0, x: 100.0, ..a.clone() };
        let (p, at) = vertical_edge(&a, &c);
        assert!(p.starts_with(&format!("M{:.1} {:.1}", NODE_W / 2.0, NODE_H)), "{p}");
        assert!(p.ends_with(&format!("{:.1} 200.0", 100.0 + NODE_W / 2.0)), "{p}");
        assert!(at.0 > (NODE_W / 2.0 + 100.0 + NODE_W / 2.0) / 2.0 && at.1 > (NODE_H + 200.0) / 2.0);
        // Above: from the top of the source to the bottom of the target.
        let d = GNode { y: -200.0, ..a.clone() };
        let (p, _) = vertical_edge(&a, &d);
        assert!(p.starts_with(&format!("M{:.1} 0.0", NODE_W / 2.0)) && p.ends_with(&format!("{:.1}", -200.0 + NODE_H)), "{p}");
        assert_eq!(bounds(&[a, d]), (0.0, -200.0, NODE_W, NODE_H));
        assert_eq!(bounds(&[]), (0.0, 0.0, 0.0, 0.0));
    }
}
