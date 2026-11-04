//! `tk_ext::BrowserRuntime` over hidden app web views (docs/redesign/09 section 5, owner D): no Chrome dependency,
//! the same engine as the app (WebKitGTK on Linux, WebView2 on Windows).
//!
//! Every web-view operation must run on the UI thread: this type only holds a [`UiThread`] sender and posts jobs;
//! the jobs keep the hidden windows (`DesktopContext` from `dioxus::desktop::window().new_window(..)` with an
//! invisible `WindowBuilder`, or wry views) in a UI-thread `thread_local!` keyed by [`BrowserId`]. Limits and URL
//! checks are listed in `tk_ext::browser`; the domain checks use `tk_ext::browser_url_allowed`.

use async_trait::async_trait;
use serde_json::Value;
use tk_ext::{BrowserError, BrowserId, BrowserRuntime};
use tk_ui::ui_thread::UiThread;

pub struct WebviewBrowsers {
    ui: UiThread,
}

impl WebviewBrowsers {
    pub fn new(ui: UiThread) -> Self {
        Self { ui }
    }
}

#[async_trait]
impl BrowserRuntime for WebviewBrowsers {
    async fn new_browser(&self, ext_id: &str, allowed_domains: &[String], opts: Value) -> Result<BrowserId, BrowserError> {
        // Contract stub (owner D).
        let _ = (&self.ui, ext_id, allowed_domains, opts);
        Err(BrowserError::Unavailable)
    }

    async fn navigate(&self, id: &BrowserId, url: &str) -> Result<(), BrowserError> {
        let _ = (id, url);
        Err(BrowserError::Closed)
    }

    async fn evaluate(&self, id: &BrowserId, js: &str) -> Result<Value, BrowserError> {
        let _ = (id, js);
        Err(BrowserError::Closed)
    }

    async fn cookies(&self, id: &BrowserId, urls: &[String]) -> Result<Vec<Value>, BrowserError> {
        let _ = (id, urls);
        Err(BrowserError::Closed)
    }

    async fn close(&self, id: &BrowserId) -> Result<(), BrowserError> {
        let _ = id;
        Ok(())
    }
}
