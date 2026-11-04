//! Linux video surface (docs/redesign/09 section 3.2, owner A): libmpv's render API (OpenGL) drawing into a
//! `gtk::GLArea` placed UNDER the WebKitGTK web view inside a `gtk::Overlay`, built from the target's `gtk::Box`.
//! The web view gets a transparent background at widget level (`webkit_web_view_set_background_color` with alpha
//! 0) while a hole is open; the toplevel window stays opaque (a transparent toplevel flickers under WebKitGTK on
//! Wayland). X11 and Wayland both go through GTK, so nothing here is backend specific.
//!
//! Contract with `mpv_player.rs`: [`GlSurface::attach`] runs after the base options and may override `vo`
//! (`libmpv`); the field is declared before `mpv` in `Player` so the render context is freed before
//! `mpv_terminate_destroy`. GTK objects are `!Send`: keep them in `glib::thread_guard::ThreadGuard` (or another
//! UI-thread-only holder) so `Player` stays `Send + Sync`; every method here is called on the UI thread.
//! With [`EmbedTarget::none`] (or before this lands) `attach` is a no-op and mpv opens its own window.

use libmpv2::Mpv;

use crate::embed::EmbedTarget;
use crate::types::{PlayerError, Rect};

pub(crate) struct GlSurface {}

impl GlSurface {
    /// Set up the GL widget in `target` and the mpv render context. Call on the UI thread.
    pub(crate) fn attach(mpv: &Mpv, target: &EmbedTarget) -> Result<Self, PlayerError> {
        // Contract stub: no embedding yet, mpv keeps its own window.
        let _ = (mpv, target.gtk_box());
        Ok(Self {})
    }

    /// Show the picture at `rect` (CSS px relative to the web view viewport, `dpr` = devicePixelRatio) or hide it.
    /// Returns whether the picture is shown.
    pub(crate) fn place(&self, rect: Option<Rect>, dpr: f64) -> bool {
        let _ = (rect, dpr);
        false
    }
}
