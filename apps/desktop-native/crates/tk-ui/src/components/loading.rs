//! `loading.rs`: M3E loading indicator. One SVG path morphs through 7 rounded shapes with SMIL
//! `<animate attributeName="d">` while the SVG rotates (CSS). Every shape has the same 48 points so the
//! interpolation is valid (same algorithm as the prototype's `ui.js::blob`). Reduced motion stops the
//! rotation in CSS; WebView2 supports SMIL.

use std::sync::OnceLock;

use dioxus::prelude::*;

use crate::i18n::t;

const POINTS: usize = 48;
/// (lobes, amplitude) of each shape, in animation order.
const SHAPES: [(u32, f64); 7] = [(4, 0.25), (7, 0.12), (9, 0.09), (5, 0.2), (8, 0.11), (6, 0.15), (12, 0.06)];

/// Closed path of a circle of radius 36 (viewBox -50 -50 100 100) whose radius is modulated by
/// `cos(lobes * angle)`.
pub fn blob(lobes: u32, amp: f64) -> String {
    let mut d = String::with_capacity(POINTS * 12);
    for i in 0..POINTS {
        let a = i as f64 / POINTS as f64 * std::f64::consts::TAU;
        let r = 36.0 * (1.0 + amp * (a * f64::from(lobes)).cos());
        d.push(if i == 0 { 'M' } else { 'L' });
        d.push_str(&format!("{:.1} {:.1}", a.cos() * r, a.sin() * r));
    }
    d.push('Z');
    d
}

/// `(first shape, SMIL values list, keySplines list)`.
fn morph() -> &'static (String, String, String) {
    static M: OnceLock<(String, String, String)> = OnceLock::new();
    M.get_or_init(|| {
        let shapes: Vec<String> = SHAPES.iter().map(|&(n, a)| blob(n, a)).collect();
        let mut values = shapes.clone();
        values.push(shapes[0].clone());
        let splines = vec![".4 0 .2 1"; shapes.len()].join(";");
        (shapes[0].clone(), values.join(";"), splines)
    })
}

#[component]
pub fn LoadingIndicator(#[props(default)] contained: bool, #[props(default)] small: bool) -> Element {
    let label = t("common.loading");
    let (first, values, splines) = morph();
    rsx! {
        div {
            class: "tk-loading",
            "data-contained": "{contained}",
            "data-small": "{small}",
            role: "progressbar",
            "aria-label": "{label}",
            svg { view_box: "-50 -50 100 100", "aria-hidden": "true",
                path { d: "{first}", fill: "currentColor",
                    animate {
                        "attributeName": "d",
                        dur: "5.6s",
                        "repeatCount": "indefinite",
                        "calcMode": "spline",
                        "keySplines": "{splines}",
                        values: "{values}",
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shape_has_the_same_number_of_commands() {
        for &(n, a) in &SHAPES {
            let d = blob(n, a);
            assert!(d.starts_with('M') && d.ends_with('Z'));
            assert_eq!(d.matches('L').count(), POINTS - 1, "{n} lobes");
        }
        let (_, values, splines) = morph();
        assert_eq!(values.split(';').count(), SHAPES.len() + 1);
        assert_eq!(splines.split(';').count(), SHAPES.len());
    }
}
