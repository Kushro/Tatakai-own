//! `progress.rs`: wavy linear progress (the filled part is a sine wave, the rest a flat track).

use dioxus::prelude::*;

/// `fraction` in 0..=1.
#[component]
pub fn WavyProgress(fraction: f64, #[props(default)] label: String) -> Element {
    let pct = (fraction.clamp(0.0, 1.0) * 100.0).round();
    // 8 periods across a 200-unit viewBox; the clip rect reveals the filled part.
    let wave = "M0 6 Q 6.25 0 12.5 6 T 25 6 T 37.5 6 T 50 6 T 62.5 6 T 75 6 T 87.5 6 T 100 6 T 112.5 6 T 125 6 T 137.5 6 T 150 6 T 162.5 6 T 175 6 T 187.5 6 T 200 6";
    let clip_w = pct * 2.0;
    rsx! {
        div { class: "tk-wavy", role: "progressbar", "aria-valuenow": "{pct}", "aria-valuemin": "0", "aria-valuemax": "100", "aria-label": "{label}",
            svg { view_box: "0 0 200 12", preserve_aspect_ratio: "none", width: "100%", height: "12",
                defs {
                    clipPath { id: "tk-wavy-clip",
                        rect { x: "0", y: "0", width: "{clip_w}", height: "12" }
                    }
                }
                line { class: "tk-wavy__track", x1: "0", y1: "6", x2: "200", y2: "6" }
                path { class: "tk-wavy__fill", d: "{wave}", "clip-path": "url(#tk-wavy-clip)" }
            }
        }
    }
}
