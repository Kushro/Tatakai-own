//! The child window mpv draws into (E5). Pattern from limusic-forge `nativevideo_windows.rs`
//! (GPL-3.0-or-later, same author; see THIRD_PARTY_NOTICES.md), rewritten for windows-sys.
//!
//! The main window is transparent and WebView2 is above this child in z-order, so the picture shows
//! through the "hole" the page leaves for it. The child is a plain `STATIC` control created WITHOUT
//! `WS_CLIPSIBLINGS` (WebView2 sits above and covers it entirely; clipping to siblings would leave it
//! no visible region), parked at `HWND_BOTTOM`, and shown only while the page has a hole for it.

use crate::rect::{PixelRect, child_rect};
use crate::types::{PlayerError, Rect};

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, HWND_BOTTOM, SWP_HIDEWINDOW, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_NOZORDER, SWP_SHOWWINDOW, SetWindowPos, WS_CHILD, WS_EX_NOACTIVATE,
};

pub struct VideoSurface {
    // HWND is a raw pointer (not Send); keep it as an integer. Only touched on the UI thread.
    child: isize,
}

// SAFETY: the integer is only turned back into an HWND on the thread that owns the window.
unsafe impl Send for VideoSurface {}
unsafe impl Sync for VideoSurface {}

impl VideoSurface {
    /// Create the hidden child under `parent_hwnd`. Call on the thread that owns the parent window.
    pub fn create(parent_hwnd: isize) -> Result<Self, PlayerError> {
        if parent_hwnd == 0 {
            return Err(PlayerError::Surface("no parent window handle".into()));
        }
        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        // SAFETY: `class` is NUL-terminated and outlives the call; the parent is a live top-level
        // window owned by this thread.
        let child = unsafe {
            CreateWindowExW(
                WS_EX_NOACTIVATE,
                class.as_ptr(),
                std::ptr::null(),
                WS_CHILD,
                0,
                0,
                0,
                0,
                parent_hwnd as HWND,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        if child.is_null() {
            return Err(PlayerError::Surface(std::io::Error::last_os_error().to_string()));
        }
        Ok(Self { child: child as isize })
    }

    /// The value for mpv's `wid`.
    pub fn wid(&self) -> i64 {
        self.child as i64
    }

    /// Show the child at `rect` (CSS px x `dpr`) or hide it. Returns whether it is shown.
    pub fn place(&self, rect: Option<Rect>, dpr: f64) -> bool {
        let hwnd = self.child as HWND;
        match rect.and_then(|r| child_rect(r, dpr)) {
            Some(px) => self.show_at(px),
            None => {
                let flags = SWP_HIDEWINDOW | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE;
                // SAFETY: our own child window, on the thread that made it.
                unsafe { SetWindowPos(hwnd, std::ptr::null_mut(), 0, 0, 0, 0, flags) };
                false
            }
        }
    }

    fn show_at(&self, (x, y, w, h): PixelRect) -> bool {
        // Back to the bottom every time: under WebView2's window, which was created first.
        // SAFETY: as above.
        unsafe { SetWindowPos(self.child as HWND, HWND_BOTTOM, x, y, w, h, SWP_SHOWWINDOW | SWP_NOACTIVATE) != 0 }
    }
}

impl Drop for VideoSurface {
    fn drop(&mut self) {
        // SAFETY: ours; destroying an already-destroyed window just fails.
        unsafe { DestroyWindow(self.child as HWND) };
    }
}
