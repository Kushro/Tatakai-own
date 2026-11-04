//! Browser runtime for extensions that need a real browser (Cloudflare bypass of Toko / Latino providers),
//! docs/redesign/09 section 5. Mirrors the legacy Electron host (`desktop/runtime/extension/seanime-browser-host.cjs`
//! + `extension-api-host/host-server.cjs`), with a hidden app web view instead of a `BrowserWindow`.
//!
//! **When**: only for a bundle whose instance has `configureBrowserRuntime` AND whose manifest lists
//! `browserDomains`; those domains are filtered by the `network:domain:*` permissions ([`browser_domains`]). An
//! extension without permitted browser domains never sees the runtime.
//!
//! **JS side** (`js/host.js` after instantiation, `js/prelude.js` for the plumbing, owner D):
//! `instance.configureBrowserRuntime({ newBrowser })`, where `newBrowser(options)` resolves to
//! `{ navigate(url), evaluate(expression), executeCDP(method, params), sleep(ms), close() }`:
//! * `navigate` / `evaluate` / `close` -> [`BrowserRuntime`] calls through a native `__host_browser_start(seq, op,
//!   id, arg)` (`op`: `new` | `navigate` | `evaluate` | `cookies` | `close`; `arg`: options JSON, URL, expression or
//!   JSON URL array) whose completion comes back through the worker's event loop like `fetch`
//!   (`__tk_browser_done(seq, json)`); pending browser operations count as pending work in `__tk_status` so a call
//!   waiting on them never "settles". The worker only accepts ids it opened, closes what is left when its runtime is
//!   discarded or it stops, and lets a call that opened a browser run up to 90 s (a Cloudflare solve takes 15-45 s).
//! * `executeCDP` supports ONLY `Network.getCookies` with `{ urls: [1..=10 urls] }` (and `Network.getAllCookies`, read
//!   for the permitted domains) and resolves to `{ cookies: [...] }` ([`BrowserRuntime::cookies`]); any other method
//!   rejects (`Unsupported ChromeDP CDP command: <m>`).
//! * `sleep(ms)` is a plain timer in JS (0..=10000 ms, else rejects).
//!
//! **Limits** (same as legacy, enforced by the implementation): at most 2 live browsers per extension; every URL
//! (navigate, cookie urls, the current page before `evaluate`) must pass [`browser_url_allowed`]; subresource requests
//! may additionally go to `https://challenges.cloudflare.com`; popups and permission requests are denied; storage is
//! ephemeral per browser (cleared on close); an operation times out after `clamp(options.timeout, 1, 30)` s (default
//! 20); `evaluate` expressions are <= 32 KiB; results are <= 2 MB of JSON; a browser auto-closes 150 s after creation;
//! `options.headless === false` shows the window (debugging), anything else keeps it hidden.

use async_trait::async_trait;
use serde_json::Value;

use crate::manifest::Manifest;

/// Opaque id of one browser session (the legacy host used a UUID).
pub type BrowserId = String;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum BrowserError {
    #[error("ChromeDP requires a permitted public provider website")]
    NoDomains,
    #[error("ChromeDP browser limit reached")]
    Limit,
    #[error("ChromeDP URL is outside the provider website")]
    UrlNotAllowed(String),
    #[error("ChromeDP browser is closed")]
    Closed,
    #[error("ChromeDP operation timed out")]
    Timeout,
    #[error("ChromeDP response is too large")]
    TooLarge,
    #[error("the browser runtime is unavailable")]
    Unavailable,
    #[error("{0}")]
    Failed(String),
}

/// Implemented by tk-app with hidden app web views (`live/browser.rs`); injected with
/// [`crate::ExtensionHost::with_browser_runtime`]. All methods are called from tokio tasks.
#[async_trait]
pub trait BrowserRuntime: Send + Sync + 'static {
    /// Open a hidden browser for `ext_id` restricted to `allowed_domains` (already filtered by
    /// [`browser_domains`]; never empty). `opts` is the bundle's `newBrowser` argument (`{ headless, timeout }`).
    async fn new_browser(&self, ext_id: &str, allowed_domains: &[String], opts: Value) -> Result<BrowserId, BrowserError>;
    /// Load `url` and resolve once the page finished loading (or the operation timeout hits).
    async fn navigate(&self, id: &BrowserId, url: &str) -> Result<(), BrowserError>;
    /// Evaluate `js` (an expression) in the current page and return its JSON value (`null` for undefined).
    async fn evaluate(&self, id: &BrowserId, js: &str) -> Result<Value, BrowserError>;
    /// Cookies visible to `urls`, CDP `Network.Cookie` shape:
    /// `{ name, value, domain, path, expires (unix s, -1 = session), httpOnly, secure, session, sameSite }`.
    async fn cookies(&self, id: &BrowserId, urls: &[String]) -> Result<Vec<Value>, BrowserError>;
    /// Close and forget the browser (idempotent).
    async fn close(&self, id: &BrowserId) -> Result<(), BrowserError>;
}

/// The manifest's `browserDomains` that a `network:domain:<d>` permission covers (equal or a subdomain of `d`),
/// syntactically valid (`[a-z0-9.-]+`, not an IP, not localhost / `.local`), lower-cased and deduplicated.
pub fn browser_domains(manifest: &Manifest) -> Vec<String> {
    let network: Vec<String> = manifest
        .permissions
        .iter()
        .filter_map(|p| p.strip_prefix("network:domain:"))
        .map(|d| d.trim().trim_start_matches("*.").to_ascii_lowercase())
        .filter(|d| !d.is_empty())
        .collect();
    let mut out: Vec<String> = Vec::new();
    for d in &manifest.browser_domains {
        let d = d.trim().to_ascii_lowercase();
        if !valid_domain(&d) || out.contains(&d) {
            continue;
        }
        if network.iter().any(|n| d == *n || d.ends_with(&format!(".{n}"))) {
            out.push(d);
        }
    }
    out
}

fn valid_domain(d: &str) -> bool {
    !d.is_empty()
        && d.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        && d.parse::<std::net::IpAddr>().is_err()
        && d != "localhost"
        && !d.ends_with(".localhost")
        && !d.ends_with(".local")
}

/// Legacy `checkUrl`: `https:`, no user info, no explicit port, host equal to or a subdomain of one of `domains`.
pub fn browser_url_allowed(url: &str, domains: &[String]) -> bool {
    let Ok(u) = url::Url::parse(url) else { return false };
    if u.scheme() != "https" || !u.username().is_empty() || u.password().is_some() || u.port().is_some() {
        return false;
    }
    let Some(host) = u.host_str() else { return false };
    let host = host.to_ascii_lowercase();
    domains.iter().any(|d| host == *d || host.ends_with(&format!(".{d}")))
}

/// Subresources a challenge page may load besides the provider site (legacy `onBeforeRequest`).
pub fn browser_subresource_allowed(url: &str, domains: &[String]) -> bool {
    browser_url_allowed(url, domains) || browser_url_allowed(url, &["challenges.cloudflare.com".to_owned()])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(browser: &[&str], perms: &[&str]) -> Manifest {
        let json = serde_json::json!({
            "id": "latino", "name": "Latino", "version": "1.0.0", "type": "onlinestream", "description": "",
            "permissions": perms, "browserDomains": browser,
        });
        Manifest::parse(&json.to_string()).unwrap()
    }

    #[test]
    fn browser_domains_need_a_network_permission() {
        let m = manifest(
            &["sololatino.net", "www2.pelisforte.se", "evil.example", "127.0.0.1", "SoloLatino.net", "x.local"],
            &["network:domain:sololatino.net", "network:domain:pelisforte.se", "network:domain:127.0.0.1"],
        );
        assert_eq!(browser_domains(&m), ["sololatino.net", "www2.pelisforte.se"]);
        assert!(browser_domains(&manifest(&[], &["network:domain:a.com"])).is_empty());
    }

    #[test]
    fn urls_are_checked_like_the_legacy_host() {
        let d = vec!["sololatino.net".to_owned()];
        assert!(browser_url_allowed("https://sololatino.net/anime/x", &d));
        assert!(browser_url_allowed("https://www.sololatino.net/", &d));
        assert!(!browser_url_allowed("http://sololatino.net/", &d));
        assert!(!browser_url_allowed("https://sololatino.net:8443/", &d));
        assert!(!browser_url_allowed("https://user@sololatino.net/", &d));
        assert!(!browser_url_allowed("https://notsololatino.net/", &d));
        assert!(!browser_url_allowed("https://challenges.cloudflare.com/x", &d));
        assert!(browser_subresource_allowed("https://challenges.cloudflare.com/x", &d));
    }
}
