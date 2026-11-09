//! Manga reader widgets (wave B, worker **l**; checklist `manga.reader.*`; classic `components/reader/*`:
//! `ReaderChrome`, `ReaderPageRail`, `ReaderImage`). Owned by l: private files here are registered with one `mod`
//! line each. Worker m (novels) does not use these: its reader is text (`screens/novel/reader.rs`).
//!
//! - **Page loading** ([`use_page_handler`], [`page_src`]): the web view cannot send the `Referer` / custom headers
//!   many image hosts require, so every page goes through the `tkpage` asset handler: [`page_src`] registers
//!   `(url, headers)` under a token and returns `/tkpage/<token>`; the handler asks the [`PageIo`] tk-app installs
//!   ([`install_page_io`], `live/manga.rs`) for the bytes. Offline pages (`file://`) take the same path, so the reader
//!   never needs network for a downloaded chapter. Pages are decoded by the web view (no image crate).
//! - [`prefs_form::ReaderPrefsForm`]: every `MangaReaderPrefs` option (mode, fit, direction, background, width,
//!   max width, gap, brightness, panel crop, loading, prefetch, auto-scroll, zoom, chrome...), shared by the reader's
//!   settings sheet and Settings > Manga reader.
//! - [`rail::PageRail`]: thumbnail strip of the chapter's pages; [`chapters::ChapterNavigator`]: chapter list sheet.

pub mod chapters;
pub mod prefs_form;
pub mod rail;

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, RwLock};

use dioxus::prelude::*;

use crate::backend::PageVm;

/// What [`PageIo`] calls return.
pub type IoFuture<T> = Pin<Box<dyn Future<Output = Result<T, String>> + Send + 'static>>;

/// Page bytes for the reader, implemented by tk-app (`live/manga.rs::LivePageIo`).
pub trait PageIo: Send + Sync {
    /// Bytes and MIME type of a page (`PageVm::url` with its headers; `file://` = offline copy).
    fn fetch(&self, url: String, headers: Vec<(String, String)>) -> IoFuture<(Vec<u8>, String)>;
    /// Save the page image to the user's pictures folder; returns the written path (`manga.reader.behavior`
    /// "save page").
    fn save(&self, url: String, headers: Vec<(String, String)>, file_stem: String) -> IoFuture<String>;
}

static PAGE_IO: RwLock<Option<Arc<dyn PageIo>>> = RwLock::new(None);

/// Install the page IO (tk-app, once; later calls replace it).
pub fn install_page_io(io: Arc<dyn PageIo>) {
    if let Ok(mut slot) = PAGE_IO.write() {
        *slot = Some(io);
    }
}

/// The installed page IO, if any.
pub fn page_io() -> Option<Arc<dyn PageIo>> {
    PAGE_IO.read().ok().and_then(|s| s.clone())
}

/// Name of the asset handler (first path segment of the page URLs).
pub const PAGE_HANDLER: &str = "tkpage";
/// Registered pages kept before the oldest are forgotten.
const MAX_REGISTERED: usize = 4000;

type PageKey = (String, Vec<(String, String)>);

static PAGES: Mutex<BTreeMap<String, (u64, PageKey)>> = Mutex::new(BTreeMap::new());
static SEQ: Mutex<u64> = Mutex::new(0);

/// FNV-1a of the url + headers: a stable token, so re-rendering a page never refetches it.
fn token_of(url: &str, headers: &[(String, String)]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for b in bytes {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    };
    feed(url.as_bytes());
    for (k, v) in headers {
        feed(b"\n");
        feed(k.as_bytes());
        feed(b":");
        feed(v.as_bytes());
    }
    format!("{h:016x}")
}

/// `src` of a page image (`/tkpage/<token>`; `retry` > 0 busts the web view's cache after a failure).
pub fn page_src(page: &PageVm, retry: u32) -> String {
    let token = token_of(&page.url, &page.headers);
    if let (Ok(mut map), Ok(mut seq)) = (PAGES.lock(), SEQ.lock()) {
        *seq += 1;
        map.insert(token.clone(), (*seq, (page.url.clone(), page.headers.clone())));
        if map.len() > MAX_REGISTERED {
            let mut by_age: Vec<(u64, String)> = map.iter().map(|(k, (s, _))| (*s, k.clone())).collect();
            by_age.sort();
            for (_, k) in by_age.into_iter().take(MAX_REGISTERED / 4) {
                map.remove(&k);
            }
        }
    }
    if retry > 0 {
        format!("/{PAGE_HANDLER}/{token}?r={retry}")
    } else {
        format!("/{PAGE_HANDLER}/{token}")
    }
}

/// The page registered under the token of a request path (`/tkpage/<token>[?...]`).
pub fn lookup(path: &str) -> Option<PageKey> {
    let token = path
        .trim_start_matches('/')
        .strip_prefix(PAGE_HANDLER)?
        .trim_start_matches('/');
    let token = token.split(['?', '#', '/']).next()?;
    PAGES.lock().ok()?.get(token).map(|(_, k)| k.clone())
}

/// Register the `tkpage` asset handler for the calling component's lifetime (the reader page).
pub fn use_page_handler() {
    dioxus::desktop::use_asset_handler(PAGE_HANDLER, move |req, responder| {
        let found = lookup(req.uri().path());
        spawn(async move {
            use dioxus::desktop::wry::http::Response;
            let reply = match (found, page_io()) {
                (Some((url, headers)), Some(io)) => match io.fetch(url, headers).await {
                    Ok((bytes, mime)) => Response::builder()
                        .status(200)
                        .header("Content-Type", mime)
                        .header("Cache-Control", "max-age=3600")
                        .body(bytes),
                    Err(e) => {
                        tracing::debug!(error = %e, "manga page fetch failed");
                        Response::builder().status(502).body(Vec::new())
                    }
                },
                (None, _) => Response::builder().status(404).body(Vec::new()),
                (_, None) => Response::builder().status(503).body(Vec::new()),
            };
            match reply {
                Ok(r) => responder.respond(r),
                Err(_) => responder.respond(Response::new(Vec::<u8>::new())),
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(url: &str, referer: Option<&str>) -> PageVm {
        PageVm {
            index: 0,
            url: url.into(),
            headers: referer
                .map(|r| vec![("Referer".to_owned(), r.to_owned())])
                .unwrap_or_default(),
            width: None,
            height: None,
        }
    }

    #[test]
    fn tokens_are_stable_and_header_sensitive() {
        let a = page("https://img/1.jpg", None);
        let b = page("https://img/1.jpg", Some("https://site/"));
        assert_eq!(token_of(&a.url, &a.headers), token_of(&a.url, &a.headers));
        assert_ne!(token_of(&a.url, &a.headers), token_of(&b.url, &b.headers));
    }

    #[test]
    fn src_round_trips_through_lookup() {
        let p = page("https://img/2.jpg", Some("https://site/"));
        let src = page_src(&p, 0);
        assert!(src.starts_with("/tkpage/"));
        let (url, headers) = lookup(&src).unwrap();
        assert_eq!(url, "https://img/2.jpg");
        assert_eq!(headers.len(), 1);
        let retry = page_src(&p, 2);
        assert!(retry.ends_with("?r=2"));
        assert_eq!(lookup(&retry).map(|k| k.0), Some(url));
        assert!(lookup("/other/abc").is_none());
    }
}
