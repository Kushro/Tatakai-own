//! Placement math for the child window that mpv draws into (pure, testable).

use crate::types::Rect;

/// Child-window rectangle in physical parent-client pixels: `(x, y, width, height)`.
pub type PixelRect = (i32, i32, i32, i32);

/// CSS-pixel `rect` x `dpr` -> physical pixels with one pixel of slack on every side (the page's
/// surround covers the excess; a picture a pixel short of the hole would leave a hairline of desktop).
/// `None` when the rect is degenerate (smaller than one CSS pixel or not finite): hide the window.
pub fn child_rect(rect: Rect, dpr: f64) -> Option<PixelRect> {
    if ![rect.x, rect.y, rect.w, rect.h].iter().all(|v| v.is_finite()) || rect.w < 1.0 || rect.h < 1.0 {
        return None;
    }
    let s = if dpr.is_finite() && dpr > 0.0 { dpr } else { 1.0 };
    let (x0, y0) = ((rect.x * s).floor() as i32 - 1, (rect.y * s).floor() as i32 - 1);
    let (x1, y1) = (((rect.x + rect.w) * s).ceil() as i32 + 1, ((rect.y + rect.h) * s).ceil() as i32 + 1);
    Some((x0, y0, x1 - x0, y1 - y0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn scales_by_dpr_and_adds_a_pixel_of_slack() {
        assert_eq!(child_rect(r(100.0, 50.0, 800.0, 450.0), 1.0), Some((99, 49, 802, 452)));
        assert_eq!(child_rect(r(100.0, 50.0, 800.0, 450.0), 1.5), Some((149, 74, 1202, 677)));
    }

    #[test]
    fn degenerate_rects_hide_the_window() {
        assert_eq!(child_rect(r(0.0, 0.0, 0.5, 100.0), 1.0), None);
        assert_eq!(child_rect(r(0.0, 0.0, f64::NAN, 100.0), 1.0), None);
    }

    #[test]
    fn bad_dpr_falls_back_to_one() {
        assert_eq!(child_rect(r(10.0, 10.0, 10.0, 10.0), 0.0), child_rect(r(10.0, 10.0, 10.0, 10.0), 1.0));
        assert_eq!(child_rect(r(10.0, 10.0, 10.0, 10.0), f64::INFINITY), child_rect(r(10.0, 10.0, 10.0, 10.0), 1.0));
    }
}
