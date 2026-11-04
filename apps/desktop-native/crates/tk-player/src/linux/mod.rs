//! Linux video surface (docs/redesign/09 section 3.2, owner A): libmpv's render API (OpenGL) drawing into a
//! `gtk::GLArea` placed UNDER the WebKitGTK web view inside a `gtk::Overlay`, built from the target's `gtk::Box`.
//! The web view gets a transparent background at widget level (`webkit_web_view_set_background_color` with alpha
//! 0) while a surface is attached; the toplevel window stays opaque (a transparent toplevel flickers under
//! WebKitGTK on Wayland). X11 and Wayland both go through GTK; only the GL loader differs (EGL on Wayland, GLX on
//! X11, which is what GTK 3 uses for `GLArea` on each backend).
//!
//! Widget tree after the first attach (tao's default vbox keeps its place in the window):
//!
//! ```text
//! gtk::Box (tao default vbox)
//! └─ gtk::Overlay "tk-video-overlay"
//!    ├─ gtk::Box            main child: empty, sizes the overlay like the web view did
//!    ├─ gtk::GLArea         overlay 0: mpv draws here; allocation = the page's hole (get-child-position)
//!    └─ webkit2gtk::WebView overlay 1: fills the overlay, transparent background, gets all the input
//! ```
//!
//! The overlay stays for the life of the window; each `Player` adds its own `GLArea` and removes it on drop.
//! The area is sized to the hole (not the whole window), so mpv's own OSD / uosc land inside the hole and the
//! coordinates `Player::forward_mouse` sends match the render surface.
//!
//! Contract with `mpv_player.rs`: [`GlSurface::attach`] runs after the base options and overrides `vo` (`libmpv`);
//! [`GlSurface::keep_core`] then gets a clone of the `Arc<Mpv>` so the core is only destroyed after the render
//! context is freed, even if the last `Player` reference goes away off the UI thread. GTK objects are `!Send`:
//! they live in a UI-thread `thread_local!` keyed by surface id, so `GlSurface` (and `Player`) stay `Send + Sync`.
//! With [`EmbedTarget::none`] or when GL is unavailable, `attach` is a no-op and mpv opens its own window.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ffi::{CStr, c_char, c_int, c_uint, c_void};
use std::ptr;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::ThreadId;

use gtk::glib;
use gtk::prelude::*;
use libloading::os::unix::Library;
use libmpv2::Mpv;
use webkit2gtk::WebViewExt;

use crate::embed::EmbedTarget;
use crate::rect::child_rect;
use crate::types::{PlayerError, Rect};

const OVERLAY_NAME: &str = "tk-video-overlay";

// --- libmpv render API (render.h / render_gl.h). libmpv2 4's `RenderContext` frees its update-callback box
// before `mpv_render_context_free` (a race with mpv's threads) and cannot pass the native display, so the few
// calls we need are declared here (libmpv2-sys already links libmpv).

/// Opaque `mpv_render_context`.
type MpvRenderContext = c_void;

#[repr(C)]
struct RenderParam {
    type_: c_uint,
    data: *mut c_void,
}

#[repr(C)]
struct OpenGlInitParams {
    get_proc_address: Option<unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void>,
    get_proc_address_ctx: *mut c_void,
}

#[repr(C)]
struct OpenGlFbo {
    fbo: c_int,
    w: c_int,
    h: c_int,
    internal_format: c_int,
}

const PARAM_INVALID: c_uint = 0;
const PARAM_API_TYPE: c_uint = 1;
const PARAM_OPENGL_INIT_PARAMS: c_uint = 2;
const PARAM_OPENGL_FBO: c_uint = 3;
const PARAM_FLIP_Y: c_uint = 4;
const PARAM_X11_DISPLAY: c_uint = 8;
const PARAM_WL_DISPLAY: c_uint = 9;
const PARAM_BLOCK_FOR_TARGET_TIME: c_uint = 12;

/// `GL_DRAW_FRAMEBUFFER_BINDING` (= `GL_FRAMEBUFFER_BINDING`): the FBO `GLArea` bound before `render`.
const GL_DRAW_FRAMEBUFFER_BINDING: c_uint = 0x8CA6;

#[link(name = "mpv")]
unsafe extern "C" {
    fn mpv_render_context_create(res: *mut *mut MpvRenderContext, mpv: *mut c_void, params: *mut RenderParam) -> c_int;
    fn mpv_render_context_set_update_callback(
        ctx: *mut MpvRenderContext,
        callback: Option<unsafe extern "C" fn(*mut c_void)>,
        callback_ctx: *mut c_void,
    );
    fn mpv_render_context_render(ctx: *mut MpvRenderContext, params: *mut RenderParam) -> c_int;
    fn mpv_render_context_free(ctx: *mut MpvRenderContext);
    fn mpv_error_string(error: c_int) -> *const c_char;
}

fn mpv_error(code: c_int) -> String {
    // SAFETY: mpv_error_string returns a static string for any code.
    let s = unsafe { mpv_error_string(code) };
    if s.is_null() {
        format!("mpv error {code}")
    } else {
        // SAFETY: non-NULL, NUL-terminated, static.
        unsafe { CStr::from_ptr(s) }.to_string_lossy().into_owned()
    }
}

// --- GL function loader

type GetProcFn = unsafe extern "C" fn(*const c_char) -> *mut c_void;
type GetIntegervFn = unsafe extern "C" fn(c_uint, *mut c_int);

/// GL entry points for mpv and for us. The GDK backend never changes within a process, so one static is enough.
struct GlProcs {
    get_proc: Option<GetProcFn>,
    get_integerv: Option<GetIntegervFn>,
    // Searched directly when the loader returns NULL for core symbols (old EGL, some GLX setups).
    fallback: Vec<Library>,
    _loader: Option<Library>,
}

impl GlProcs {
    fn load(wayland: bool) -> Self {
        // SAFETY: plain system GL libraries; GTK has already loaded them, so their initialisers already ran.
        let open = |name: &str| unsafe { Library::new(name) }.ok();
        let (loader, symbol): (Option<Library>, &[u8]) = if wayland {
            (open("libEGL.so.1").or_else(|| open("libEGL.so")), b"eglGetProcAddress\0")
        } else {
            (open("libGLX.so.0").or_else(|| open("libGL.so.1")), b"glXGetProcAddressARB\0")
        };
        // SAFETY: both symbols have the `GetProcFn` signature.
        let get_proc = loader.as_ref().and_then(|l| unsafe { l.get::<GetProcFn>(symbol) }.ok().map(|s| *s));
        let fallback = ["libGL.so.1", "libOpenGL.so.0", "libGLESv2.so.2"].into_iter().filter_map(open).collect();
        let mut procs = Self { get_proc, get_integerv: None, fallback, _loader: loader };
        let p = procs.lookup(c"glGetIntegerv");
        if !p.is_null() {
            // SAFETY: glGetIntegerv(GLenum, GLint*).
            procs.get_integerv = Some(unsafe { std::mem::transmute::<*mut c_void, GetIntegervFn>(p) });
        }
        procs
    }

    fn lookup(&self, name: &CStr) -> *mut c_void {
        if let Some(f) = self.get_proc {
            // SAFETY: `name` is NUL-terminated; the loader returns NULL for unknown names.
            let p = unsafe { f(name.as_ptr()) };
            if !p.is_null() {
                return p;
            }
        }
        for lib in &self.fallback {
            // SAFETY: looked up as an opaque pointer, never called here.
            if let Ok(sym) = unsafe { lib.get::<*mut c_void>(name.to_bytes_with_nul()) } {
                let p = *sym;
                if !p.is_null() {
                    return p;
                }
            }
        }
        ptr::null_mut()
    }
}

static GL_PROCS: OnceLock<GlProcs> = OnceLock::new();

fn gl_procs(wayland: bool) -> &'static GlProcs {
    GL_PROCS.get_or_init(|| GlProcs::load(wayland))
}

unsafe extern "C" fn get_proc_address(_ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    let Some(procs) = GL_PROCS.get() else { return ptr::null_mut() };
    if name.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: mpv passes a NUL-terminated function name.
    procs.lookup(unsafe { CStr::from_ptr(name) })
}

/// `wl_display*` (Wayland) or X11 `Display*` behind the GDK display, for mpv's hwdec interop. NULL if unknown.
fn native_display(display: &gtk::gdk::Display, wayland: bool) -> *mut c_void {
    let symbol: &[u8] =
        if wayland { b"gdk_wayland_display_get_wl_display\0" } else { b"gdk_x11_display_get_xdisplay\0" };
    // Resolved at run time from the already loaded libgdk-3, so a GTK built without one backend still links.
    let this = Library::this();
    // SAFETY: both functions take the GdkDisplay* of their own backend and return a borrowed pointer.
    match unsafe { this.get::<unsafe extern "C" fn(*mut c_void) -> *mut c_void>(symbol) } {
        Ok(f) => unsafe { f(display.as_ptr() as *mut c_void) },
        Err(_) => ptr::null_mut(),
    }
}

// --- UI-thread state

struct Surface {
    overlay: gtk::Overlay,
    webview: webkit2gtk::WebView,
    area: gtk::GLArea,
    /// The render context; NULL once freed (in the area's `unrealize`, while its GL context is still current).
    render: Rc<Cell<*mut MpvRenderContext>>,
    /// Allocation of the area inside the overlay, GTK logical px. `None` = hidden.
    geom: Rc<Cell<Option<(i32, i32, i32, i32)>>>,
}

thread_local! {
    static SURFACES: RefCell<HashMap<u64, Surface>> = RefCell::new(HashMap::new());
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// mpv's update callback (any mpv thread): ask the GTK main loop to redraw. Must not call mpv from here.
unsafe extern "C" fn on_update(ctx: *mut c_void) {
    let id = ctx as usize as u64;
    glib::idle_add_once(move || {
        SURFACES.with(|m| {
            if let Ok(m) = m.try_borrow()
                && let Some(s) = m.get(&id)
                && s.area.is_visible()
            {
                s.area.queue_render();
            }
        });
    });
}

fn render_frame(area: &gtk::GLArea, render: &Cell<*mut MpvRenderContext>) {
    let ctx = render.get();
    if ctx.is_null() {
        return;
    }
    let scale = area.scale_factor().max(1);
    let (w, h) = (area.allocated_width() * scale, area.allocated_height() * scale);
    if w <= 0 || h <= 0 {
        return;
    }
    let mut fbo: c_int = 0;
    if let Some(get_integerv) = GL_PROCS.get().and_then(|p| p.get_integerv) {
        // SAFETY: GLArea made its context current and bound its framebuffer before emitting `render`.
        unsafe { get_integerv(GL_DRAW_FRAMEBUFFER_BINDING, &mut fbo) };
    }
    let mut target = OpenGlFbo { fbo, w, h, internal_format: 0 };
    let mut flip: c_int = 1;
    // Never block the GTK main loop waiting for the frame's display time.
    let mut block: c_int = 0;
    let mut params = [
        RenderParam { type_: PARAM_OPENGL_FBO, data: (&mut target as *mut OpenGlFbo).cast() },
        RenderParam { type_: PARAM_FLIP_Y, data: (&mut flip as *mut c_int).cast() },
        RenderParam { type_: PARAM_BLOCK_FOR_TARGET_TIME, data: (&mut block as *mut c_int).cast() },
        RenderParam { type_: PARAM_INVALID, data: ptr::null_mut() },
    ];
    // SAFETY: valid context, its GL context current, params INVALID-terminated and alive for the call.
    let err = unsafe { mpv_render_context_render(ctx, params.as_mut_ptr()) };
    if err < 0 {
        tracing::debug!(error = %mpv_error(err), "mpv render failed");
    }
}

fn set_transparent(webview: &webkit2gtk::WebView, transparent: bool) {
    let alpha = if transparent { 0.0 } else { 1.0 };
    webview.set_background_color(&gtk::gdk::RGBA::new(0.0, 0.0, 0.0, alpha));
}

/// The overlay holding the web view, creating it (and re-parenting the web view) on first use.
fn overlay_for(vbox: &gtk::Box) -> Result<(gtk::Overlay, webkit2gtk::WebView), String> {
    for child in vbox.children() {
        if let Ok(overlay) = child.downcast::<gtk::Overlay>()
            && overlay.widget_name().as_str() == OVERLAY_NAME
        {
            let webview = overlay
                .children()
                .into_iter()
                .find_map(|c| c.downcast::<webkit2gtk::WebView>().ok())
                .ok_or("video overlay without a web view")?;
            return Ok((overlay, webview));
        }
    }

    let (pos, webview) = vbox
        .children()
        .into_iter()
        .enumerate()
        .find_map(|(i, c)| c.downcast::<webkit2gtk::WebView>().ok().map(|w| (i, w)))
        .ok_or("no web view in the window box")?;
    let (expand, fill, padding, pack) = vbox.query_child_packing(&webview);

    let overlay = gtk::Overlay::new();
    overlay.set_widget_name(OVERLAY_NAME);
    // Main child: sizes the overlay; overlay children never affect its size request.
    let base = gtk::Box::new(gtk::Orientation::Vertical, 0);
    overlay.add(&base);
    overlay.connect_get_child_position(|_, widget| {
        SURFACES.with(|m| {
            let m = m.try_borrow().ok()?;
            let s = m.values().find(|s| s.area.upcast_ref::<gtk::Widget>() == widget)?;
            s.geom.get().map(|(x, y, w, h)| gtk::Rectangle::new(x, y, w, h))
        })
    });

    // `webview` keeps a strong reference while it is out of any container.
    vbox.remove(&webview);
    overlay.add_overlay(&webview);
    vbox.pack_start(&overlay, expand, fill, padding);
    vbox.set_child_packing(&overlay, expand, fill, padding, pack);
    vbox.reorder_child(&overlay, pos as i32);
    base.show();
    webview.show();
    overlay.show();
    webview.grab_focus();
    Ok((overlay, webview))
}

/// Builds the GL area and the render context; returns the surface id.
fn build(mpv: &Mpv, vbox: &gtk::Box) -> Result<u64, String> {
    let display = vbox.display();
    let wayland = match display.type_().name() {
        "GdkWaylandDisplay" => true,
        "GdkX11Display" => false,
        other => return Err(format!("unsupported GDK backend {other}")),
    };
    let procs = gl_procs(wayland);
    if procs.get_proc.is_none() && procs.fallback.is_empty() {
        return Err("no OpenGL loader (libEGL / libGL) found".into());
    }

    let (overlay, webview) = overlay_for(vbox)?;
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let render: Rc<Cell<*mut MpvRenderContext>> = Rc::new(Cell::new(ptr::null_mut()));
    let geom: Rc<Cell<Option<(i32, i32, i32, i32)>>> = Rc::new(Cell::new(None));

    let area = gtk::GLArea::new();
    area.set_widget_name("tk-video-gl");
    area.set_has_alpha(false);
    // auto-render: GTK redraws (resize, the web view repainting above) re-render the last frame as well.
    area.set_auto_render(true);
    area.set_no_show_all(true);
    area.set_visible(false);
    overlay.add_overlay(&area);
    overlay.reorder_overlay(&area, 0);
    overlay.set_overlay_pass_through(&area, true);
    {
        let render = render.clone();
        area.connect_render(move |area, _| {
            render_frame(area, &render);
            glib::Propagation::Stop
        });
    }
    {
        let render = render.clone();
        area.connect_unrealize(move |area| {
            let ctx = render.replace(ptr::null_mut());
            if !ctx.is_null() {
                area.make_current();
                // SAFETY: the area's GL context (the one the render context was created with) is current.
                unsafe { mpv_render_context_free(ctx) };
            }
        });
    }

    let fail = |area: &gtk::GLArea, msg: String| -> Result<u64, String> {
        // Removing the area unrealizes it, which frees the render context if one was created.
        overlay.remove(area);
        Err(msg)
    };
    area.realize();
    area.make_current();
    if let Some(e) = area.error() {
        return fail(&area, format!("GL context: {e}"));
    }
    if area.context().is_none() {
        return fail(&area, "GL context: none".into());
    }

    let native = native_display(&display, wayland);
    let mut init = OpenGlInitParams { get_proc_address: Some(get_proc_address), get_proc_address_ctx: ptr::null_mut() };
    let mut params = vec![
        RenderParam { type_: PARAM_API_TYPE, data: c"opengl".as_ptr() as *mut c_void },
        RenderParam { type_: PARAM_OPENGL_INIT_PARAMS, data: (&mut init as *mut OpenGlInitParams).cast() },
    ];
    if !native.is_null() {
        params.push(RenderParam { type_: if wayland { PARAM_WL_DISPLAY } else { PARAM_X11_DISPLAY }, data: native });
    }
    params.push(RenderParam { type_: PARAM_INVALID, data: ptr::null_mut() });
    let mut ctx: *mut MpvRenderContext = ptr::null_mut();
    // SAFETY: the area's GL context is current; params are INVALID-terminated and outlive the call; the GDK
    // display (and its native display) lives as long as the process.
    let err = unsafe { mpv_render_context_create(&mut ctx, mpv.ctx.as_ptr().cast(), params.as_mut_ptr()) };
    if err < 0 || ctx.is_null() {
        return fail(&area, format!("mpv render context: {}", mpv_error(err)));
    }
    render.set(ctx);
    if let Err(e) = mpv.set_property("vo", "libmpv") {
        return fail(&area, format!("vo=libmpv refused: {e}"));
    }
    // SAFETY: `ctx` is valid; the callback only posts to the GTK main loop; the id travels as the pointer value.
    unsafe { mpv_render_context_set_update_callback(ctx, Some(on_update), id as usize as *mut c_void) };

    set_transparent(&webview, true);
    let backend = if wayland { "wayland" } else { "x11" };
    tracing::info!(id, backend, "embedded mpv GL surface ready");
    SURFACES.with(|m| {
        m.borrow_mut().insert(id, Surface { overlay, webview, area, render, geom });
    });
    Ok(id)
}

/// Remove a surface (UI thread); its render context is freed through `unrealize`.
fn release(id: u64) {
    let Some(s) = SURFACES.with(|m| m.borrow_mut().remove(&id)) else { return };
    let ctx = s.render.get();
    if !ctx.is_null() {
        // SAFETY: valid context; mpv stops calling back into a surface that is going away.
        unsafe { mpv_render_context_set_update_callback(ctx, None, ptr::null_mut()) };
    }
    s.overlay.remove(&s.area);
    // Still set = the area was not realized (window already gone): free without a current context rather than
    // keep it past the core.
    let ctx = s.render.replace(ptr::null_mut());
    if !ctx.is_null() {
        // SAFETY: valid context, freed exactly once (the cell is now NULL).
        unsafe { mpv_render_context_free(ctx) };
    }
    if SURFACES.with(|m| m.borrow().is_empty()) {
        set_transparent(&s.webview, false);
    }
}

pub(crate) struct GlSurface {
    /// 0 = not embedded (mpv has its own window).
    id: u64,
    ui: Option<ThreadId>,
    /// Keeps the mpv core alive until the render context is gone (see [`GlSurface::keep_core`]).
    core: Mutex<Option<Arc<Mpv>>>,
}

impl GlSurface {
    fn inactive() -> Self {
        Self { id: 0, ui: None, core: Mutex::new(None) }
    }

    /// Set up the GL widget in `target` and the mpv render context. Call on the UI thread.
    /// Never fails the player: without GL (or without a GTK target) mpv keeps its own window.
    pub(crate) fn attach(mpv: &Mpv, target: &EmbedTarget) -> Result<Self, PlayerError> {
        let Some(vbox) = target.gtk_box() else { return Ok(Self::inactive()) };
        match build(mpv, vbox) {
            Ok(id) => Ok(Self { id, ui: Some(std::thread::current().id()), core: Mutex::new(None) }),
            Err(e) => {
                tracing::warn!(error = %e, "embedded video unavailable; mpv opens its own window");
                Ok(Self::inactive())
            }
        }
    }

    /// Hold a reference to the core so `mpv_terminate_destroy` runs after the render context is freed, wherever
    /// the last `Player` reference is dropped. Call once, right after the `Arc<Mpv>` exists.
    pub(crate) fn keep_core(&self, mpv: Arc<Mpv>) {
        if self.id != 0
            && let Ok(mut c) = self.core.lock()
        {
            *c = Some(mpv);
        }
    }

    /// Show the picture at `rect` (CSS px relative to the web view viewport, `dpr` = devicePixelRatio) or hide it.
    /// Returns whether the picture is shown. Call on the UI thread.
    pub(crate) fn place(&self, rect: Option<Rect>, dpr: f64) -> bool {
        if self.id == 0 || self.ui != Some(std::thread::current().id()) {
            return false;
        }
        SURFACES.with(|m| {
            let Ok(m) = m.try_borrow() else { return false };
            let Some(s) = m.get(&self.id) else { return false };
            // CSS px -> GTK logical px: devicePixelRatio already includes the GDK scale; what remains is zoom.
            let scale = f64::from(s.webview.scale_factor().max(1));
            let zoom = if dpr.is_finite() && dpr > 0.0 { dpr / scale } else { 1.0 };
            match rect.and_then(|r| child_rect(r, zoom)) {
                Some(g) => {
                    let changed = s.geom.get() != Some(g);
                    s.geom.set(Some(g));
                    if !s.area.is_visible() {
                        s.area.set_visible(true);
                    }
                    if changed {
                        s.overlay.queue_resize();
                    }
                    s.area.queue_render();
                    true
                }
                None => {
                    s.geom.set(None);
                    if s.area.is_visible() {
                        s.area.set_visible(false);
                    }
                    false
                }
            }
        })
    }
}

impl Drop for GlSurface {
    fn drop(&mut self) {
        if self.id == 0 {
            return;
        }
        let id = self.id;
        let core = self.core.get_mut().ok().and_then(|c| c.take());
        if self.ui == Some(std::thread::current().id()) {
            release(id);
            drop(core);
        } else {
            // Off the UI thread: the GTK main loop frees the surface, then lets go of the core.
            glib::idle_add_once(move || {
                release(id);
                drop(core);
            });
        }
    }
}
