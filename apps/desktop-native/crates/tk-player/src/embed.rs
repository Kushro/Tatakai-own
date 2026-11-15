//! Where the embedded player draws (docs/redesign/09 section 3.2). Opaque to tk-ui: `tk_ui::platform::embed_target()`
//! builds it from the Dioxus / tao window on the UI thread and hands it to `PlayerHost::get_or_init`.
//!
//! * Windows: the main window's HWND; mpv gets a child `STATIC` window under WebView2 (`surface.rs`, unchanged).
//! * Linux: the `gtk::Box` tao creates as the window's only child and wry packs WebKitGTK's web view into
//!   (`WindowExtUnix::default_vbox`). The Linux surface (`linux/`) re-parents the web view into a `gtk::Overlay`
//!   over a `gtk::GLArea` that mpv's render API draws into; the web view gets a transparent background at widget
//!   level (the toplevel stays opaque). GTK objects are not `Send`: build and use this only on the UI thread.
//! * Elsewhere / no window: [`EmbedTarget::none`]; mpv then opens its own window (the pre-wave-2 Linux behaviour).

use std::fmt;

#[derive(Clone)]
enum Inner {
    None,
    #[cfg(windows)]
    Hwnd(isize),
    #[cfg(target_os = "linux")]
    Gtk(gtk::Box),
}

#[derive(Clone)]
pub struct EmbedTarget {
    inner: Inner,
}

impl EmbedTarget {
    /// No host window: mpv uses its own window.
    pub fn none() -> Self {
        Self { inner: Inner::None }
    }

    /// The main window's HWND (`WindowExtWindows::hwnd`). 0 behaves like [`EmbedTarget::none`].
    #[cfg(windows)]
    pub fn from_hwnd(hwnd: isize) -> Self {
        if hwnd == 0 { Self::none() } else { Self { inner: Inner::Hwnd(hwnd) } }
    }

    /// tao's default vbox (`WindowExtUnix::default_vbox`), which holds the WebKitGTK web view.
    #[cfg(target_os = "linux")]
    pub fn from_gtk_box(vbox: &gtk::Box) -> Self {
        Self { inner: Inner::Gtk(vbox.clone()) }
    }

    pub fn is_none(&self) -> bool {
        matches!(self.inner, Inner::None)
    }

    /// The parent HWND, 0 when there is none.
    #[cfg(windows)]
    pub fn hwnd(&self) -> isize {
        match self.inner {
            Inner::Hwnd(h) => h,
            Inner::None => 0,
        }
    }

    #[cfg(target_os = "linux")]
    pub fn gtk_box(&self) -> Option<&gtk::Box> {
        match &self.inner {
            Inner::Gtk(b) => Some(b),
            Inner::None => None,
        }
    }
}

impl EmbedTarget {
    /// Refresh rate (Hz) of the monitor the app window is on, when the platform reports it (frame generation caps its
    /// multiplier with it, BUG-036). Linux: GDK's monitor of the toplevel window; elsewhere `None` for now.
    pub fn refresh_hz(&self) -> Option<f64> {
        #[cfg(target_os = "linux")]
        if let Inner::Gtk(b) = &self.inner {
            use gtk::prelude::*;
            let window = b.toplevel()?.window()?;
            let monitor = window.display().monitor_at_window(&window)?;
            let mhz = monitor.refresh_rate();
            return (mhz > 0).then(|| f64::from(mhz) / 1000.0);
        }
        None
    }

    /// The monitor the app window is on: `(model, index)` from GDK. On X11 GDK's model is the RandR output name
    /// (`DP-1`) and the index follows the RandR monitor order, which is what mpv's `--screen-name` / `--screen` use.
    /// `None` when the platform does not report it.
    pub fn monitor(&self) -> Option<(Option<String>, Option<u32>)> {
        #[cfg(target_os = "linux")]
        if let Inner::Gtk(b) = &self.inner {
            use gtk::prelude::*;
            let window = b.toplevel()?.window()?;
            let display = window.display();
            let monitor = display.monitor_at_window(&window)?;
            let index = (0..display.n_monitors()).find(|&i| display.monitor(i).as_ref() == Some(&monitor)).and_then(|i| u32::try_from(i).ok());
            let model = monitor.model().map(|m| m.to_string()).filter(|m| !m.is_empty());
            return Some((model, index));
        }
        None
    }
}

impl fmt::Debug for EmbedTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match &self.inner {
            Inner::None => "none",
            #[cfg(windows)]
            Inner::Hwnd(_) => "hwnd",
            #[cfg(target_os = "linux")]
            Inner::Gtk(_) => "gtk",
        };
        write!(f, "EmbedTarget({kind})")
    }
}
