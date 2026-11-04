//! `progress.rs`: M3E wavy linear progress. The filled part is a sine wave clipped to `fraction`; when
//! `active` the wave travels (CSS translate of exactly one wavelength, so it loops seamlessly). The
//! rest is a flat track with a gap and a stop dot. Reduced motion freezes the phase in CSS.

use std::sync::atomic::{AtomicU64, Ordering};

use dioxus::prelude::*;

const W: f64 = 400.0;
const H: f64 = 14.0;
/// Must match the `tk-wave` keyframes translate in app.css.
const WAVELENGTH: f64 = 36.0;
const AMPLITUDE: f64 = 3.5;

/// Sine path from x=0 to `W + WAVELENGTH` (the extra period is what scrolls into view).
pub fn wave_path(amplitude: f64) -> String {
    let mut d = String::new();
    let mut x = 0.0;
    while x <= W + WAVELENGTH {
        let y = H / 2.0 + (x / WAVELENGTH * std::f64::consts::TAU).sin() * amplitude;
        d.push_str(&format!("{}{x:.0} {y:.2}", if x == 0.0 { 'M' } else { 'L' }));
        x += 3.0;
    }
    d
}

fn next_id() -> u64 {
    static N: AtomicU64 = AtomicU64::new(0);
    N.fetch_add(1, Ordering::Relaxed)
}

/// `fraction` in 0..=1.
#[component]
pub fn WavyProgress(
    fraction: f64,
    #[props(default)] label: String,
    /// Animate the wave (buffering, downloading). Inactive draws a flat bar.
    #[props(default = true)] active: bool,
    #[props(default)] thin: bool,
) -> Element {
    let clip_id = use_hook(|| format!("tk-wavy-clip-{}", next_id()));
    let f = fraction.clamp(0.0, 1.0);
    let pct = (f * 100.0).round();
    let end = f * W;
    let d = wave_path(if active { AMPLITUDE } else { 0.0 });
    let track_start = (end + 10.0).min(W - 2.0);
    rsx! {
        div {
            class: "tk-wavy",
            "data-active": "{active}",
            "data-thin": "{thin}",
            role: "progressbar",
            "aria-valuenow": "{pct}",
            "aria-valuemin": "0",
            "aria-valuemax": "100",
            "aria-label": "{label}",
            svg { view_box: "0 0 {W} {H}", preserve_aspect_ratio: "none", "aria-hidden": "true",
                defs {
                    clipPath { id: "{clip_id}",
                        rect { x: "0", y: "0", width: "{end}", height: "{H}" }
                    }
                }
                if track_start < W - 2.0 {
                    line { class: "tk-wavy__track", x1: "{track_start}", y1: "{H / 2.0}", x2: "{W - 2.0}", y2: "{H / 2.0}" }
                }
                g { "clip-path": "url(#{clip_id})",
                    path { class: "tk-wavy__fill", d: "{d}" }
                }
                circle { class: "tk-wavy__stop", cx: "{W - 2.0}", cy: "{H / 2.0}", r: "2" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wave_covers_one_extra_period_and_flat_when_inactive() {
        let d = wave_path(AMPLITUDE);
        assert!(d.starts_with("M0 7.00"));
        let last_x: f64 = d.rsplit('L').next().unwrap().split(' ').next().unwrap().parse().unwrap();
        assert!(last_x >= W + WAVELENGTH - 3.0);
        assert!(wave_path(0.0).split('L').all(|p| p.trim_end().ends_with("7.00")));
    }
}
