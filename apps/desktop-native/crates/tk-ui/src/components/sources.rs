//! `sources.rs`: SourceRow, SourcePicker (sheet) and SourceSummary (Details side panel), docs/redesign/07
//! section 3.4. Props are the wave 1 contract (docs/redesign/08); the bodies are placeholders that
//! wave 1 E replaces.
//!
//! Unlike the other components, `SourceSummary` loads its own data: it reads the backend from the
//! `Services` context (`use_context::<Services>().backend`, a `BackendRef`) and the prefs from the `Ui`
//! context (`ui.settings.read().sources`). Its resource does not track prop changes, so callers key it
//! per episode: `SourceSummary { key: "{id}-{ep}", .. }`.

use dioxus::prelude::*;

use crate::app::Services;
use crate::backend::SourceVm;
use crate::components::button::{Button, ButtonVariant};
use crate::components::chips::{Pill, Tone};
use crate::components::sheet::Sheet;
use crate::components::state::ErrorState;
use crate::i18n::t;
use crate::model::SourcePrefsVm;

/// One source (radio row). `on_pick` receives `source.id`. Disabled when the extension is off.
#[component]
pub fn SourceRow(source: SourceVm, selected: bool, on_pick: EventHandler<String>) -> Element {
    let id = source.id.clone();
    let kind = source.kind.id();
    let disabled = !source.ext_enabled;
    rsx! {
        button {
            class: "tk-srow",
            r#type: "button",
            role: "radio",
            "aria-checked": "{selected}",
            "data-kind": "{kind}",
            disabled,
            onclick: move |_| on_pick.call(id.clone()),
            span { class: "tk-srow__main",
                span { class: "tk-srow__title",
                    b { "{source.ext_name} · {source.label}" }
                    if source.recommended {
                        Pill { text: t("src.recommended"), tone: Tone::Primary }
                    }
                }
            }
        }
    }
}

/// All sources of an episode in a side sheet, already ranked (`sources[0]` is the recommended one).
/// Render it always; it shows nothing while `open` is false. Picking calls `on_pick(id)`; the caller
/// closes it.
#[component]
pub fn SourcePicker(
    open: bool,
    sources: Vec<SourceVm>,
    prefs: SourcePrefsVm,
    on_pick: EventHandler<String>,
    on_close: EventHandler<()>,
) -> Element {
    if !open {
        return rsx! {};
    }
    let sub = prefs.sub_lang.to_uppercase();
    rsx! {
        Sheet { title: t("src.title"), wide: true, on_close: move |_| on_close.call(()),
            p { class: "tk-muted", "{sub}" }
            div { class: "tk-slist", role: "radiogroup",
                for s in sources.iter() {
                    SourceRow { key: "{s.id}", source: s.clone(), selected: s.picked, on_pick: move |id: String| on_pick.call(id) }
                }
            }
        }
    }
}

/// Details side panel: the recommended (or picked) source, "Chosen by the system" and "See all sources".
/// `on_play(id)` plays a source; `on_open_picker` asks the parent to open the `SourcePicker`.
#[component]
pub fn SourceSummary(media_id: String, episode: u32, on_play: EventHandler<String>, on_open_picker: EventHandler<()>) -> Element {
    let services = use_context::<Services>();
    let mut sources = use_resource({
        let (backend, id) = (services.backend.clone(), media_id.clone());
        move || {
            let (backend, id) = (backend.clone(), id.clone());
            async move { backend.sources(&id, episode).await }
        }
    });
    let body = match &*sources.read() {
        None => rsx! {
            div { class: "tk-skel tk-skel--line", "aria-hidden": "true" }
        },
        Some(Err(e)) => rsx! {
            ErrorState { error: e.clone(), compact: true, on_retry: move |_| sources.restart() }
        },
        Some(Ok(list)) => match list.first() {
            None => rsx! {
                p { class: "tk-muted", {t("src.none")} }
            },
            Some(best) => rsx! {
                SourceRow { source: best.clone(), selected: true, on_pick: move |id: String| on_play.call(id) }
                Button { label: t("src.title"), variant: ButtonVariant::Tonal, block: true, onclick: move |_| on_open_picker.call(()) }
            },
        },
    };
    rsx! {
        div { class: "tk-panel tk-srcsum", {body} }
    }
}
