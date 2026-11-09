//! `tk_ext::BrowserRuntime` over hidden app web views (docs/redesign/09 section 7, owner D): no Chrome dependency,
//! the same engine as the app (WebKitGTK on Linux, WebView2 on Windows).
//!
//! Every web-view operation runs on the UI thread: this type only holds a [`UiThread`] sender and posts jobs; the
//! jobs keep the views in a UI-thread `thread_local!` keyed by [`BrowserId`] (module `ui`). The views are plain wry
//! views, not Dioxus `new_window`s: Dioxus' navigation handler sends every `https:` navigation to the system
//! browser, which would open the provider site there.
//!
//! * Linux: a `gtk::OffscreenWindow` (mapped, so the page sees itself visible and is not throttled, yet never on
//!   screen) holding a `gtk::Box` the view is built into with `build_gtk`; `headless: false` uses a normal toplevel.
//! * Windows: a child view of the main window (`build_as_child`), hidden unless `headless: false`, with its own
//!   WebView2 data folder (environments sharing the app's folder must use identical options).
//!
//! Limits and URL checks are listed in `tk_ext::browser` (tk-ext repeats the URL, size and count checks): views are
//! incognito (ephemeral storage), navigations outside the provider domains are cancelled (subframes may also load
//! `challenges.cloudflare.com`), popups, downloads and (Linux) permission requests are denied, the current page must
//! be on a provider domain before `evaluate`, each operation times out after `clamp(timeout, 1, 30)` s and a browser
//! closes itself 150 s after creation.
//!
//! Wave B (worker **k**, `sources.embed-sniffer`, `.adblock`): [`WebviewBrowsers::sniff`] opens a hidden view of
//! the same kind for an embed page with a capture script injected in every frame ([`ui::SNIFF_SCRIPT`]): `fetch`,
//! `XMLHttpRequest`, `<video src>` and the resource timing entries report every `.m3u8` / `.mp4` (or HLS / MP4
//! content type) the player requests, frames post theirs to the top window, and the app polls the list. While
//! sniffing the view opens no windows (`window.open` is a no-op and new-window requests are denied), refuses
//! navigations outside the embed / extension domains and to the ad hosts, drops ad `<script>` / `<iframe>` nodes,
//! mutes media and is never shown.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;
use tk_ext::{BrowserError, BrowserId, BrowserRuntime};
use tk_ui::ui_thread::UiThread;
use tokio::sync::oneshot;

const AUTO_CLOSE: Duration = Duration::from_secs(150);
const MAX_PER_EXTENSION: usize = 2;

pub struct WebviewBrowsers {
    ui: UiThread,
    seq: AtomicU64,
    /// Per-operation timeout of each open browser (also the set of ids this runtime considers open).
    timeouts: Arc<Mutex<HashMap<BrowserId, Duration>>>,
}

impl WebviewBrowsers {
    pub fn new(ui: UiThread) -> Self {
        Self { ui, seq: AtomicU64::new(1), timeouts: Arc::default() }
    }

    fn timeout_of(&self, id: &BrowserId) -> Result<Duration, BrowserError> {
        self.timeouts.lock().unwrap_or_else(|p| p.into_inner()).get(id).copied().ok_or(BrowserError::Closed)
    }

    fn forget(&self, id: &BrowserId) {
        self.timeouts.lock().unwrap_or_else(|p| p.into_inner()).remove(id);
    }
}

/// One media request the sniffer saw (`window.__tkSniff` entries; camelCase from the capture script).
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Captured {
    pub url: String,
    /// `location.href` of the frame that made the request (the `Referer` the CDN saw).
    #[serde(default)]
    pub referer: String,
    #[serde(default)]
    pub origin: String,
    #[serde(default)]
    pub user_agent: String,
    #[serde(default)]
    pub content_type: String,
}

impl Captured {
    /// HLS playlist (by extension or content type).
    pub fn is_hls(&self) -> bool {
        let path = self.url.split(['?', '#']).next().unwrap_or("").to_ascii_lowercase();
        path.ends_with(".m3u8") || self.content_type.to_ascii_lowercase().contains("mpegurl")
    }
}

/// What a sniff found and how long it took.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SniffOutcome {
    pub captured: Vec<Captured>,
    pub title: Option<String>,
}

/// How long the sniffer keeps waiting for an HLS playlist once only MP4 requests were seen.
const MP4_GRACE: Duration = Duration::from_millis(1500);
const SNIFF_POLL: Duration = Duration::from_millis(400);

impl WebviewBrowsers {
    /// Open `url` (with `referer`) in a hidden capture view limited to `domains` minus `blocked`, wait until it
    /// requests an HLS playlist (or an MP4 and no playlist follows within 1.5 s) or `timeout` runs out, close the
    /// view and return what it captured (HLS first). An empty result means the page played nothing in time.
    pub async fn sniff(&self, url: &str, referer: Option<&str>, domains: Vec<String>, blocked: Vec<String>, timeout: Duration) -> Result<SniffOutcome, BrowserError> {
        if domains.is_empty() {
            return Err(BrowserError::NoDomains);
        }
        let id: BrowserId = format!("tk-sniff-{}", self.seq.fetch_add(1, Ordering::Relaxed));
        {
            // reclaimed even if this future is dropped mid-way
            let ui = self.ui.clone();
            let id = id.clone();
            tokio::spawn(async move {
                tokio::time::sleep(timeout + Duration::from_secs(10)).await;
                ui.post(move || ui::close(&id));
            });
        }
        let result = self.sniff_inner(&id, url, referer, domains, blocked, timeout).await;
        let close_id = id.clone();
        let _ = self.ui.run(move || ui::close(&close_id)).await;
        result
    }

    async fn sniff_inner(&self, id: &BrowserId, url: &str, referer: Option<&str>, domains: Vec<String>, blocked: Vec<String>, timeout: Duration) -> Result<SniffOutcome, BrowserError> {
        let started = tokio::time::Instant::now();
        let deadline = started + timeout;
        let open = {
            let (id, url, referer) = (id.clone(), url.to_owned(), referer.map(str::to_owned));
            move || ui::open_sniffer(id, domains, blocked, &url, referer.as_deref())
        };
        within(Duration::from_secs(10), async { self.ui.run(open).await.unwrap_or(Err(BrowserError::Unavailable)) }).await?;
        let mut mp4_since: Option<tokio::time::Instant> = None;
        let mut last = SniffOutcome::default();
        while tokio::time::Instant::now() < deadline {
            tokio::time::sleep(SNIFF_POLL).await;
            let poll_id = id.clone();
            let raw = match within(Duration::from_secs(3), async { ui_then(&self.ui, move || ui::sniff_poll(&poll_id)).await }).await {
                Ok(raw) => raw,
                Err(BrowserError::Timeout) => continue, // a busy page; try again
                Err(e) => return Err(e),
            };
            last = ui::parse_sniff(&raw);
            if last.captured.iter().any(Captured::is_hls) {
                break;
            }
            if !last.captured.is_empty() {
                let since = *mp4_since.get_or_insert_with(tokio::time::Instant::now);
                if since.elapsed() >= MP4_GRACE {
                    break;
                }
            }
        }
        last.captured.sort_by_key(|c| !c.is_hls());
        Ok(last)
    }
}

/// Legacy: `Math.min(30, Math.max(1, Number(options.timeout) || 20))` seconds.
fn op_timeout(opts: &Value) -> Duration {
    let t = match opts.get("timeout") {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.trim().parse::<f64>().ok(),
        _ => None,
    };
    Duration::from_secs_f64(t.filter(|t| t.is_finite() && *t != 0.0).unwrap_or(20.0).clamp(1.0, 30.0))
}

async fn within<T>(t: Duration, fut: impl std::future::Future<Output = Result<T, BrowserError>>) -> Result<T, BrowserError> {
    tokio::time::timeout(t, fut).await.unwrap_or(Err(BrowserError::Timeout))
}

/// Run a UI job that hands back a receiver, then wait for what the receiver gets.
async fn ui_then<T: Send + 'static>(
    ui: &UiThread,
    job: impl FnOnce() -> Result<oneshot::Receiver<T>, BrowserError> + Send + 'static,
) -> Result<T, BrowserError> {
    let rx = ui.run(job).await.ok_or(BrowserError::Unavailable)??;
    rx.await.map_err(|_| BrowserError::Closed)
}

#[async_trait]
impl BrowserRuntime for WebviewBrowsers {
    async fn new_browser(&self, ext_id: &str, allowed_domains: &[String], opts: Value) -> Result<BrowserId, BrowserError> {
        if allowed_domains.is_empty() {
            return Err(BrowserError::NoDomains);
        }
        let timeout = op_timeout(&opts);
        let headless = opts.get("headless") != Some(&Value::Bool(false));
        let id: BrowserId = format!("tk-browser-{}", self.seq.fetch_add(1, Ordering::Relaxed));
        self.timeouts.lock().unwrap_or_else(|p| p.into_inner()).insert(id.clone(), timeout);

        // Auto-close, armed before creation so a view created after a timed-out wait is still reclaimed.
        // Unconditional: closing an id that is already gone is a no-op on the UI side.
        {
            let ui = self.ui.clone();
            let timeouts = self.timeouts.clone();
            let id = id.clone();
            tokio::spawn(async move {
                tokio::time::sleep(AUTO_CLOSE).await;
                timeouts.lock().unwrap_or_else(|p| p.into_inner()).remove(&id);
                ui.post(move || ui::close(&id));
            });
        }

        let job = {
            let id = id.clone();
            let ext_id = ext_id.to_owned();
            let domains = allowed_domains.to_vec();
            move || ui::open(id, ext_id, domains, headless)
        };
        let created = within(timeout, async { self.ui.run(job).await.unwrap_or(Err(BrowserError::Unavailable)) }).await;
        match created {
            Ok(()) => Ok(id),
            Err(e) => {
                self.forget(&id);
                let close_id = id.clone();
                self.ui.post(move || ui::close(&close_id)); // in case the job still runs after the timeout
                Err(e)
            }
        }
    }

    async fn navigate(&self, id: &BrowserId, url: &str) -> Result<(), BrowserError> {
        let t = self.timeout_of(id)?;
        let (id, url) = (id.clone(), url.to_owned());
        within(t, async { ui_then(&self.ui, move || ui::navigate(&id, &url)).await? }).await
    }

    async fn evaluate(&self, id: &BrowserId, js: &str) -> Result<Value, BrowserError> {
        let t = self.timeout_of(id)?;
        let (id, js) = (id.clone(), js.to_owned());
        let raw = within(t, async { ui_then(&self.ui, move || ui::evaluate(&id, &js)).await }).await?;
        ui::parse_evaluation(&raw)
    }

    async fn cookies(&self, id: &BrowserId, urls: &[String]) -> Result<Vec<Value>, BrowserError> {
        let t = self.timeout_of(id)?;
        let (id, urls) = (id.clone(), urls.to_vec());
        within(t, async { ui_then(&self.ui, move || ui::cookies(&id, urls)).await }).await
    }

    async fn close(&self, id: &BrowserId) -> Result<(), BrowserError> {
        self.forget(id);
        let id = id.clone();
        let _ = self.ui.run(move || ui::close(&id)).await;
        Ok(())
    }
}

/// UI-thread side: the views and everything that touches them.
mod ui {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;
    use std::sync::{Arc, Mutex};

    use dioxus::desktop::wry;
    use serde_json::{Value, json};
    use tk_ext::browser::browser_subresource_allowed;
    use tk_ext::{BrowserError, BrowserId, browser_url_allowed};
    use tokio::sync::oneshot;

    use super::MAX_PER_EXTENSION;

    type Loads = Rc<RefCell<Vec<oneshot::Sender<Result<(), BrowserError>>>>>;

    struct Session {
        ext_id: String,
        domains: Arc<Vec<String>>,
        /// Waiters of the current navigation, resolved by the page-load handler.
        loads: Loads,
        view: wry::WebView,
        #[cfg(target_os = "linux")]
        window: gtk::Window,
        /// Declared after `view`: the web context outlives the view.
        #[cfg(windows)]
        _context: wry::WebContext,
    }

    thread_local! {
        static SESSIONS: RefCell<HashMap<BrowserId, Session>> = RefCell::new(HashMap::new());
    }

    fn failed(e: impl std::fmt::Display) -> BrowserError {
        BrowserError::Failed(e.to_string())
    }

    /// Top-level navigations stay on the provider site; subframes may also load the Cloudflare challenge. wry's
    /// navigation handler does not say which frame navigates, so both use the subresource rule and `evaluate`
    /// re-checks the top-level URL strictly.
    fn navigation_allowed(url: &str, domains: &[String]) -> bool {
        url == "about:blank" || url == "about:srcdoc" || browser_subresource_allowed(url, domains)
    }

    /// Capture view settings (wave B, worker k): the ad hosts refused on top of the domain rule.
    #[derive(Clone, Default)]
    pub(super) struct SniffSetup {
        pub blocked: Arc<Vec<String>>,
    }

    /// The sniffer's navigation rule: the extension browser rule, minus the ad hosts. Pure.
    pub(super) fn sniff_navigation_allowed(url: &str, domains: &[String], blocked: &[String]) -> bool {
        if url == "about:blank" || url == "about:srcdoc" {
            return true;
        }
        let host = reqwest::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_ascii_lowercase)).unwrap_or_default();
        let ad = blocked.iter().any(|b| host == *b || host.ends_with(&format!(".{b}")));
        !ad && browser_subresource_allowed(url, domains)
    }

    /// Capture script injected at document start in every frame (see the module docs). `__BLOCKED__` is replaced
    /// by the JSON array of ad hosts.
    pub(super) const SNIFF_SCRIPT: &str = r#"(function(){
if (window.__tkSniffInit) return; window.__tkSniffInit = true;
var isTop = false; try { isTop = window.top === window; } catch (e) {}
var found = []; if (isTop) window.__tkSniff = found;
var blocked = __BLOCKED__;
var mediaRe = /\.(m3u8|mp4)(\?|#|$)/i;
function add(entry){ if (!found.some(function(x){ return x.url === entry.url; }) && found.length < 50) found.push(entry); }
function report(u, ct){
  try { u = new URL(String(u), location.href).href; } catch (e) { return; }
  if (!/^https?:/i.test(u)) return;
  if (!(mediaRe.test(u) || /mpegurl|video\/mp4/i.test(ct || ''))) return;
  var entry = { url: u, referer: location.href, origin: location.origin, userAgent: navigator.userAgent, contentType: ct || '' };
  if (isTop) add(entry); else { try { window.top.postMessage({ __tkSniff: entry }, '*'); } catch (e) {} }
}
if (isTop) window.addEventListener('message', function(ev){ var d = ev && ev.data && ev.data.__tkSniff; if (d && typeof d.url === 'string') add(d); });
var of = window.fetch;
if (of) window.fetch = function(input){ try { report(typeof input === 'string' ? input : (input && input.url)); } catch (e) {}
  return of.apply(this, arguments).then(function(r){ try { report(r.url, r.headers.get('content-type')); } catch (e) {} return r; }); };
var oo = XMLHttpRequest.prototype.open;
XMLHttpRequest.prototype.open = function(m, u){ try { report(u); this.addEventListener('load', function(){ try { report(this.responseURL, this.getResponseHeader('content-type')); } catch (e) {} }); } catch (e) {} return oo.apply(this, arguments); };
try { var d = Object.getOwnPropertyDescriptor(HTMLMediaElement.prototype, 'src');
  if (d && d.set) Object.defineProperty(HTMLMediaElement.prototype, 'src', { configurable: true, get: d.get, set: function(v){ report(v); return d.set.call(this, v); } }); } catch (e) {}
try { new PerformanceObserver(function(l){ l.getEntries().forEach(function(e){ report(e.name); }); }).observe({ type: 'resource', buffered: true }); } catch (e) {}
window.open = function(){ return null; };
function isBlocked(src){ try { var h = new URL(src, location.href).hostname.toLowerCase(); return blocked.some(function(b){ return h === b || h.slice(-(b.length + 1)) === '.' + b; }); } catch (e) { return false; } }
try { new MutationObserver(function(ms){ ms.forEach(function(m){ m.addedNodes.forEach(function(n){
  var tag = n && n.tagName;
  if ((tag === 'SCRIPT' || tag === 'IFRAME') && n.src && isBlocked(n.src)) { try { n.type = 'javascript/blocked'; n.remove(); } catch (e) {} }
  if (tag === 'VIDEO' || tag === 'AUDIO') { try { n.muted = true; } catch (e) {} }
}); }); }).observe(document, { childList: true, subtree: true }); } catch (e) {}
setInterval(function(){ try {
  document.querySelectorAll('video, audio').forEach(function(v){ v.muted = true; if (v.paused) { var p = v.play(); if (p && p.catch) p.catch(function(){}); } });
  var b = document.querySelector('.vjs-big-play-button, .jw-display-icon-container, .plyr__control--overlaid, .play-button, [aria-label="Play"]');
  if (b && !b.__tkClicked) { b.__tkClicked = true; b.click(); }
} catch (e) {} }, 1000);
})();"#;

    /// `SNIFF_SCRIPT` with the ad hosts filled in. Pure.
    pub(super) fn sniff_script(blocked: &[String]) -> String {
        SNIFF_SCRIPT.replace("__BLOCKED__", &serde_json::to_string(blocked).unwrap_or_else(|_| "[]".into()))
    }

    fn configure<'a>(
        b: wry::WebViewBuilder<'a>,
        domains: Arc<Vec<String>>,
        loads: Loads,
        visible: bool,
        sniff: Option<SniffSetup>,
    ) -> wry::WebViewBuilder<'a> {
        let b = match &sniff {
            // muted autoplay lets the embed player request its playlist without a click
            Some(setup) => b.with_autoplay(true).with_initialization_script_for_main_only(sniff_script(&setup.blocked), false),
            None => b.with_autoplay(false),
        };
        let blocked = sniff.map(|s| s.blocked);
        b.with_incognito(true)
            .with_visible(visible)
            .with_focused(false)
            .with_devtools(false)
            .with_clipboard(false)
            .with_navigation_handler(move |url: String| {
                let ok = match &blocked {
                    Some(ads) => sniff_navigation_allowed(&url, &domains, ads),
                    None => navigation_allowed(&url, &domains),
                };
                if !ok {
                    tracing::debug!("extension browser: blocked navigation to {url}");
                }
                ok
            })
            .with_new_window_req_handler(|_url, _features| wry::NewWindowResponse::Deny)
            .with_download_started_handler(|_url, _path| false)
            .with_on_page_load_handler(move |event, _url| {
                if matches!(event, wry::PageLoadEvent::Finished) {
                    let waiters = std::mem::take(&mut *loads.borrow_mut());
                    for w in waiters {
                        let _ = w.send(Ok(()));
                    }
                }
            })
    }

    pub(super) fn open(id: BrowserId, ext_id: String, domains: Vec<String>, headless: bool) -> Result<(), BrowserError> {
        let open_for_ext = SESSIONS.with(|s| s.borrow().values().filter(|x| x.ext_id == ext_id).count());
        if open_for_ext >= MAX_PER_EXTENSION {
            return Err(BrowserError::Limit);
        }
        let session = build(ext_id, Arc::new(domains), Rc::default(), headless, None)?;
        SESSIONS.with(|s| {
            s.borrow_mut().insert(id, session);
        });
        Ok(())
    }

    /// Hidden capture view for the embed sniffer (wave B, worker k), already navigating to `url` with `referer`.
    pub(super) fn open_sniffer(id: BrowserId, domains: Vec<String>, blocked: Vec<String>, url: &str, referer: Option<&str>) -> Result<(), BrowserError> {
        const SNIFF_OWNER: &str = "tk-sniffer";
        let open = SESSIONS.with(|s| s.borrow().values().filter(|x| x.ext_id == SNIFF_OWNER).count());
        if open >= MAX_PER_EXTENSION {
            return Err(BrowserError::Limit);
        }
        if !browser_url_allowed(url, &domains) {
            return Err(BrowserError::UrlNotAllowed(url.to_owned()));
        }
        let setup = SniffSetup { blocked: Arc::new(blocked) };
        let session = build(SNIFF_OWNER.to_owned(), Arc::new(domains), Rc::default(), true, Some(setup))?;
        let mut headers = wry::http::HeaderMap::new();
        if let Some(r) = referer.filter(|r| r.starts_with("https://") || r.starts_with("http://"))
            && let Ok(v) = wry::http::HeaderValue::from_str(r)
        {
            headers.insert(wry::http::header::REFERER, v);
        }
        let loaded = if headers.is_empty() { session.view.load_url(url) } else { session.view.load_url_with_headers(url, headers) };
        if let Err(e) = loaded {
            destroy(session);
            return Err(failed(e));
        }
        SESSIONS.with(|s| {
            s.borrow_mut().insert(id, session);
        });
        Ok(())
    }

    /// Read the capture list of a sniffer view (top frame) as the JSON the callback gets.
    pub(super) fn sniff_poll(id: &str) -> Result<oneshot::Receiver<String>, BrowserError> {
        with_session(id, |s| {
            let (tx, rx) = oneshot::channel();
            let tx = Mutex::new(Some(tx));
            s.view
                .evaluate_script_with_callback("JSON.stringify({ c: window.__tkSniff || [], t: document.title || '' })", move |raw| {
                    if let Some(tx) = tx.lock().ok().and_then(|mut g| g.take()) {
                        let _ = tx.send(raw);
                    }
                })
                .map_err(failed)?;
            Ok(rx)
        })
    }

    /// Parse a `sniff_poll` answer (the returned string, JSON-encoded once more by the engine); garbage = nothing
    /// captured. Pure.
    pub(super) fn parse_sniff(raw: &str) -> super::SniffOutcome {
        #[derive(serde::Deserialize, Default)]
        struct Poll {
            #[serde(default)]
            c: Vec<super::Captured>,
            #[serde(default)]
            t: String,
        }
        let inner: String = serde_json::from_str(raw).unwrap_or_default();
        let poll: Poll = serde_json::from_str(&inner).unwrap_or_default();
        let title = Some(poll.t.trim().to_owned()).filter(|t| !t.is_empty());
        super::SniffOutcome { captured: poll.c.into_iter().filter(|c| c.url.starts_with("https://") || c.url.starts_with("http://")).collect(), title }
    }

    #[cfg(target_os = "linux")]
    fn build(ext_id: String, domains: Arc<Vec<String>>, loads: Loads, headless: bool, sniff: Option<SniffSetup>) -> Result<Session, BrowserError> {
        use gtk::prelude::*;
        use webkit2gtk::{PermissionRequestExt, WebViewExt};
        use wry::{WebViewBuilderExtUnix, WebViewExtUnix};

        let window: gtk::Window = if headless {
            gtk::OffscreenWindow::new().upcast()
        } else {
            let w = gtk::Window::new(gtk::WindowType::Toplevel);
            // Closing the debugging window only hides it; the session ends with close() or the auto-close.
            w.connect_delete_event(|w, _| {
                w.hide();
                gtk::glib::Propagation::Stop
            });
            w
        };
        window.set_default_size(1000, 760);
        let container = gtk::Box::new(gtk::Orientation::Vertical, 0);
        window.add(&container);
        let view = match configure(wry::WebViewBuilder::new(), domains.clone(), loads.clone(), true, sniff).build_gtk(&container) {
            Ok(v) => v,
            Err(e) => {
                // SAFETY: the window was created above and is not used after this.
                unsafe { window.destroy() };
                return Err(failed(e));
            }
        };
        view.webview().connect_permission_request(|_, request| {
            request.deny();
            true
        });
        window.show_all();
        Ok(Session { ext_id, domains, loads, view, window })
    }

    #[cfg(windows)]
    fn build(ext_id: String, domains: Arc<Vec<String>>, loads: Loads, headless: bool, sniff: Option<SniffSetup>) -> Result<Session, BrowserError> {
        let dir = std::env::var_os("LOCALAPPDATA")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join("Tatakai-Next")
            .join("ExtensionBrowser");
        let mut context = wry::WebContext::new(Some(dir));
        let parent = dioxus::desktop::window();
        let bounds = wry::Rect {
            position: wry::dpi::Position::Logical(wry::dpi::LogicalPosition::new(0.0, 0.0)),
            size: wry::dpi::Size::Logical(wry::dpi::LogicalSize::new(1000.0, 760.0)),
        };
        let view = configure(wry::WebViewBuilder::new_with_web_context(&mut context), domains.clone(), loads.clone(), !headless, sniff)
            .with_bounds(bounds)
            .build_as_child(&*parent.window)
            .map_err(failed)?;
        Ok(Session { ext_id, domains, loads, view, _context: context })
    }

    #[cfg(not(any(target_os = "linux", windows)))]
    fn build(_ext_id: String, _domains: Arc<Vec<String>>, _loads: Loads, _headless: bool, _sniff: Option<SniffSetup>) -> Result<Session, BrowserError> {
        Err(BrowserError::Unavailable)
    }

    fn with_session<R>(id: &str, f: impl FnOnce(&Session) -> Result<R, BrowserError>) -> Result<R, BrowserError> {
        SESSIONS.with(|s| {
            let map = s.borrow();
            let session = map.get(id).ok_or(BrowserError::Closed)?;
            f(session)
        })
    }

    pub(super) fn navigate(id: &str, url: &str) -> Result<oneshot::Receiver<Result<(), BrowserError>>, BrowserError> {
        with_session(id, |s| {
            if !browser_url_allowed(url, &s.domains) {
                return Err(BrowserError::UrlNotAllowed(url.to_owned()));
            }
            let (tx, rx) = oneshot::channel();
            s.loads.borrow_mut().push(tx);
            s.view.load_url(url).map_err(failed)?;
            Ok(rx)
        })
    }

    /// Wraps an expression (a trailing `;` is tolerated) so it returns `{ ok, v | e }` as a JSON string: an
    /// exception surfaces as an error (wry swallows them) and `undefined` becomes `null`. Promises are not awaited.
    fn wrap_expression(js: &str) -> String {
        let expr = js.trim().trim_end_matches(';');
        format!(
            "(function(){{try{{var v=(\n{expr}\n);return JSON.stringify({{ok:true,v:v===undefined?null:v}});}}catch(e){{return JSON.stringify({{ok:false,e:String(e&&e.message||e)}});}}}})()"
        )
    }

    pub(super) fn parse_evaluation(raw: &str) -> Result<Value, BrowserError> {
        // The callback gets the JSON of the returned string: the envelope encoded twice.
        let bad = || BrowserError::Failed("ChromeDP evaluation failed".into());
        let inner: String = serde_json::from_str(raw).map_err(|_| bad())?;
        let env: Value = serde_json::from_str(&inner).map_err(|_| bad())?;
        if env.get("ok") == Some(&Value::Bool(true)) {
            Ok(env.get("v").cloned().unwrap_or(Value::Null))
        } else {
            Err(BrowserError::Failed(env.get("e").and_then(Value::as_str).unwrap_or("ChromeDP evaluation failed").to_owned()))
        }
    }

    pub(super) fn evaluate(id: &str, js: &str) -> Result<oneshot::Receiver<String>, BrowserError> {
        with_session(id, |s| {
            let current = s.view.url().unwrap_or_default();
            if !browser_url_allowed(&current, &s.domains) {
                return Err(BrowserError::UrlNotAllowed(current));
            }
            let (tx, rx) = oneshot::channel();
            let tx = Mutex::new(Some(tx));
            s.view
                .evaluate_script_with_callback(&wrap_expression(js), move |raw| {
                    if let Some(tx) = tx.lock().ok().and_then(|mut g| g.take()) {
                        let _ = tx.send(raw);
                    }
                })
                .map_err(failed)?;
            Ok(rx)
        })
    }

    /// CDP `Network.Cookie` shape.
    #[allow(clippy::too_many_arguments)]
    fn cookie_json(name: String, value: String, domain: String, path: String, expires: Option<i64>, http_only: bool, secure: bool, same_site: String) -> Value {
        json!({
            "name": name, "value": value, "domain": domain, "path": path,
            "expires": expires.map_or(-1.0, |e| e as f64), "size": name.len() + value.len(),
            "httpOnly": http_only, "secure": secure, "session": expires.is_none(), "sameSite": same_site,
        })
    }

    pub(super) fn cookies(id: &str, urls: Vec<String>) -> Result<oneshot::Receiver<Vec<Value>>, BrowserError> {
        let domains = with_session(id, |s| Ok(s.domains.clone()))?;
        if let Some(bad) = urls.iter().find(|u| !browser_url_allowed(u, &domains)) {
            return Err(BrowserError::UrlNotAllowed(bad.clone()));
        }
        if urls.is_empty() {
            return Err(BrowserError::Failed("ChromeDP cookie URLs are required".into()));
        }
        let (tx, rx) = oneshot::channel();
        cookies_into(id, urls, tx)?;
        Ok(rx)
    }

    /// WebKitGTK: the cookie manager's async API (wry's `cookies_for_url` spins a nested GTK main loop, which would
    /// re-enter the Dioxus event loop from inside this job).
    #[cfg(target_os = "linux")]
    fn cookies_into(id: &str, urls: Vec<String>, tx: oneshot::Sender<Vec<Value>>) -> Result<(), BrowserError> {
        use webkit2gtk::{CookieManagerExt, WebViewExt, WebsiteDataManagerExt};
        use wry::WebViewExtUnix;

        let manager = with_session(id, |s| {
            s.view
                .webview()
                .website_data_manager()
                .and_then(|m| m.cookie_manager())
                .ok_or_else(|| BrowserError::Failed("no cookie manager".into()))
        })?;
        // (callbacks still to come, cookies so far, reply)
        let state = Rc::new(RefCell::new((urls.len(), Vec::<Value>::new(), Some(tx))));
        for url in &urls {
            let state = state.clone();
            manager.cookies(url, None::<&webkit2gtk::gio::Cancellable>, move |result| {
                let mut st = state.borrow_mut();
                if let Ok(list) = result {
                    for mut c in list {
                        let v = cookie_json(
                            c.name().map(|s| s.to_string()).unwrap_or_default(),
                            c.value().map(|s| s.to_string()).unwrap_or_default(),
                            c.domain().map(|s| s.to_string()).unwrap_or_default(),
                            c.path().map(|s| s.to_string()).unwrap_or_default(),
                            c.expires().map(|d| d.to_unix()),
                            c.is_http_only(),
                            c.is_secure(),
                            format!("{:?}", c.same_site_policy()),
                        );
                        if !st.1.contains(&v) {
                            st.1.push(v);
                        }
                    }
                }
                st.0 = st.0.saturating_sub(1);
                if st.0 == 0 {
                    let cookies = std::mem::take(&mut st.1);
                    if let Some(tx) = st.2.take() {
                        let _ = tx.send(cookies);
                    }
                }
            });
        }
        Ok(())
    }

    /// WebView2: wry's `cookies_for_url` (it pumps messages while waiting). The session is taken out of the map
    /// meanwhile, so a job running inside that pump sees it closed instead of hitting a `RefCell` borrow.
    #[cfg(windows)]
    fn cookies_into(id: &str, urls: Vec<String>, tx: oneshot::Sender<Vec<Value>>) -> Result<(), BrowserError> {
        let session = SESSIONS.with(|s| s.borrow_mut().remove(id)).ok_or(BrowserError::Closed)?;
        let mut out: Vec<Value> = Vec::new();
        let mut error = None;
        for url in &urls {
            match session.view.cookies_for_url(url) {
                Ok(list) => {
                    for c in list {
                        let v = cookie_json(
                            c.name().to_owned(),
                            c.value().to_owned(),
                            c.domain().unwrap_or_default().to_owned(),
                            c.path().unwrap_or("/").to_owned(),
                            c.expires_datetime().map(|d| d.unix_timestamp()),
                            c.http_only().unwrap_or(false),
                            c.secure().unwrap_or(false),
                            c.same_site().map(|s| s.to_string()).unwrap_or_else(|| "None".into()),
                        );
                        if !out.contains(&v) {
                            out.push(v);
                        }
                    }
                }
                Err(e) => error = Some(failed(e)),
            }
        }
        // A close() during the pump found nothing to close; the auto-close still reclaims the view.
        SESSIONS.with(|s| {
            s.borrow_mut().insert(id.to_owned(), session);
        });
        match error {
            Some(e) if out.is_empty() => Err(e),
            _ => {
                let _ = tx.send(out);
                Ok(())
            }
        }
    }

    #[cfg(not(any(target_os = "linux", windows)))]
    fn cookies_into(_id: &str, _urls: Vec<String>, _tx: oneshot::Sender<Vec<Value>>) -> Result<(), BrowserError> {
        Err(BrowserError::Unavailable)
    }

    fn destroy(session: Session) {
        // Pending navigations resolve as closed.
        session.loads.borrow_mut().clear();
        #[cfg(windows)]
        {
            let _ = session.view.clear_all_browsing_data();
        }
        #[cfg(target_os = "linux")]
        {
            use gtk::prelude::*;
            let Session { view, window, .. } = session;
            drop(view);
            // SAFETY: the window is ours alone (never handed out) and is not used after this.
            unsafe { window.destroy() };
        }
        #[cfg(not(target_os = "linux"))]
        {
            drop(session);
        }
    }

    pub(super) fn close(id: &str) {
        if let Some(session) = SESSIONS.with(|s| s.borrow_mut().remove(id)) {
            destroy(session);
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn evaluation_envelopes_are_unwrapped() {
            let ok = serde_json::to_string(r#"{"ok":true,"v":{"url":"https://a.b/"}}"#).unwrap();
            assert_eq!(parse_evaluation(&ok).unwrap(), json!({ "url": "https://a.b/" }));
            let err = serde_json::to_string(r#"{"ok":false,"e":"boom"}"#).unwrap();
            assert_eq!(parse_evaluation(&err), Err(BrowserError::Failed("boom".into())));
            assert!(parse_evaluation("").is_err());
            assert!(wrap_expression("document.title;").contains("(\ndocument.title\n)"));
        }

        #[test]
        fn sniffer_rules_and_script() {
            let d = vec!["embed.example".to_owned()];
            let ads = vec!["popads.net".to_owned()];
            assert!(sniff_navigation_allowed("https://embed.example/e/1", &d, &ads));
            assert!(sniff_navigation_allowed("https://player.embed.example/x", &d, &ads));
            assert!(!sniff_navigation_allowed("https://c.popads.net/x", &d, &ads));
            assert!(!sniff_navigation_allowed("https://other.example/", &d, &ads));
            let js = sniff_script(&ads);
            assert!(js.contains(r#"var blocked = ["popads.net"];"#));
            assert!(js.contains("window.open = function(){ return null; };"));
            let raw = serde_json::to_string(r#"{"c":[{"url":"https://cdn.x/a.m3u8","referer":"https://embed.example/e/1","origin":"https://embed.example","userAgent":"UA","contentType":""},{"url":"blob:x"}],"t":" Ep 1 "}"#).unwrap();
            let out = parse_sniff(&raw);
            assert_eq!(out.captured.len(), 1);
            assert!(out.captured[0].is_hls());
            assert_eq!(out.title.as_deref(), Some("Ep 1"));
            assert!(parse_sniff("garbage").captured.is_empty());
        }

        #[test]
        fn navigation_rule_allows_the_challenge_frame_only() {
            let d = vec!["sololatino.net".to_owned()];
            assert!(navigation_allowed("https://sololatino.net/x", &d));
            assert!(navigation_allowed("https://challenges.cloudflare.com/cdn-cgi/x", &d));
            assert!(navigation_allowed("about:blank", &d));
            assert!(!navigation_allowed("https://ads.example/", &d));
            assert!(!navigation_allowed("http://sololatino.net/", &d));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeouts_follow_the_legacy_clamp() {
        assert_eq!(op_timeout(&serde_json::json!({})), Duration::from_secs(20));
        assert_eq!(op_timeout(&serde_json::json!({ "timeout": 5 })), Duration::from_secs(5));
        assert_eq!(op_timeout(&serde_json::json!({ "timeout": 90 })), Duration::from_secs(30));
        assert_eq!(op_timeout(&serde_json::json!({ "timeout": 0.2 })), Duration::from_secs(1));
        assert_eq!(op_timeout(&serde_json::json!({ "timeout": "7" })), Duration::from_secs(7));
    }
}
