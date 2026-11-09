//! Media proxy `GET | HEAD /media/{ticket}/{*path}` + `POST /media/refresh` (worker **k**; classic
//! `server/src/proxy.mjs`, `cache.mjs`; checklist `server.api.resolve-proxy`, `server.cors-proxy-local`,
//! `server.proxy`): a resolve result whose stream needs headers (Referer, cookies, User-Agent) is rewritten to
//! `/media/<ticket>/<file name>`; the ticket (random id -> `{ url, headers, session, expires }`) lives in memory, the
//! bytes stream through [`crate::upstream::Upstream`] with `Range` passthrough, HLS playlists are rewritten so
//! segments and keys go through the proxy too, and small objects (images, subtitles, `.ts` segments up to 512 KiB) are
//! cached under `<dir>/media-cache` (oldest dropped first above [`MAX_CACHE_BYTES`], [`CACHE_TTL_SECS`] each).
//! The same proxy serves mpv on the loopback when a direct stream needs it (`server.cors-proxy-local`):
//! `tk-app/src/live/server.rs` mints a ticket for the session [`LOCAL_SESSION`] (only accepted from 127.0.0.1) and
//! plays `http://127.0.0.1:<port>/api/server/v1/media/...`. The optional outbound proxy (HTTP / SOCKS5) and
//! FlareSolverr are applied by the upstream client.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use axum::body::Body;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use url::Url;

use crate::api::Failure;
use crate::upstream::{Upstream, strip_credentials};

/// Sub-directory of the server dir holding cached media.
pub const CACHE_DIR: &str = "media-cache";
/// Tickets live this long (classic: 6 h).
pub const TICKET_TTL_SECS: i64 = 6 * 3600;
/// Tickets kept in memory at most (expired ones go first).
pub const MAX_TICKETS: usize = 20_000;
/// Objects cached at most this big, and the cache in total.
pub const MAX_CACHED_OBJECT: usize = 512 * 1024;
pub const MAX_CACHE_BYTES: u64 = 32 * 1024 * 1024;
pub const CACHE_TTL_SECS: i64 = 300;
/// Playlists read at most this big before rewriting.
const MAX_PLAYLIST: usize = 2 * 1024 * 1024;
/// Session of tickets minted for the local player (accepted only from a loopback peer).
pub const LOCAL_SESSION: &str = "local:player";

/// Remove every cached object (best effort; a missing directory is fine).
pub async fn clear_cache(dir: &Path) {
    let cache = dir.join(CACHE_DIR);
    if let Err(e) = tokio::fs::remove_dir_all(&cache).await
        && e.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(error = %e, "could not clear the media cache");
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ticket {
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// Device id that may use it, `public:<epoch>` for anonymous viewers or [`LOCAL_SESSION`].
    pub session: String,
    pub expires_unix: i64,
}

/// File name shown after the ticket (players detect the format from it): the last path segment of `url`, reduced
/// to `[A-Za-z0-9._-]`, at most 80 characters, `media` when nothing is left.
pub fn display_name(url: &str) -> String {
    let path = Url::parse(url).ok().map(|u| u.path().to_owned()).unwrap_or_default();
    let last = path.rsplit('/').find(|s| !s.is_empty()).unwrap_or("");
    let clean: String = last.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')).take(80).collect();
    if clean.is_empty() || clean.chars().all(|c| c == '.') { "media".to_owned() } else { clean }
}

/// Rewrite an HLS playlist so every URI (media lines and `URI="..."` attributes) goes through `mint` (pure).
pub fn rewrite_playlist(text: &str, playlist_url: &Url, mint: &mut dyn FnMut(&Url) -> String) -> String {
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            out.push(line.to_owned());
        } else if trimmed.starts_with('#') {
            let mut rewritten = String::with_capacity(line.len());
            let mut rest = line;
            while let Some(i) = rest.find("URI=\"") {
                let (head, tail) = rest.split_at(i + 5);
                rewritten.push_str(head);
                match tail.find('"') {
                    Some(j) => {
                        let value = &tail[..j];
                        match playlist_url.join(value) {
                            Ok(u) if matches!(u.scheme(), "http" | "https") => rewritten.push_str(&mint(&u)),
                            _ => rewritten.push_str(value),
                        }
                        rest = &tail[j..];
                    }
                    None => {
                        rest = tail;
                        break;
                    }
                }
            }
            rewritten.push_str(rest);
            out.push(rewritten);
        } else {
            match playlist_url.join(trimmed) {
                Ok(u) if matches!(u.scheme(), "http" | "https") => out.push(mint(&u)),
                _ => out.push(line.to_owned()),
            }
        }
    }
    let mut s = out.join("\n");
    if text.ends_with('\n') {
        s.push('\n');
    }
    s
}

fn is_playlist(content_type: &str, url: &str) -> bool {
    let ct = content_type.to_ascii_lowercase();
    ct.contains("mpegurl") || url.split(['?', '#']).next().is_some_and(|p| p.to_ascii_lowercase().ends_with(".m3u8"))
}

fn cacheable(content_type: &str) -> bool {
    let ct = content_type.to_ascii_lowercase();
    ct.starts_with("image/")
        || ct.starts_with("text/vtt")
        || ct.starts_with("application/octet-stream")
        || ct.starts_with("application/x-subrip")
        || ct.starts_with("video/mp2t")
}

struct CacheIndex {
    /// key -> (content type, size, expires)
    entries: HashMap<String, (String, u64, i64)>,
    order: VecDeque<String>,
    bytes: u64,
}

/// On-disk cache of small proxied objects.
pub struct MediaCache {
    dir: PathBuf,
    index: Mutex<CacheIndex>,
}

impl MediaCache {
    pub fn new(server_dir: &Path) -> Self {
        // the cache is per run: leftovers of a previous run are removed
        let dir = server_dir.join(CACHE_DIR);
        let _ = std::fs::remove_dir_all(&dir);
        Self { dir, index: Mutex::new(CacheIndex { entries: HashMap::new(), order: VecDeque::new(), bytes: 0 }) }
    }

    /// `(entries, bytes)` still valid.
    pub fn stats(&self) -> (u32, u64) {
        let now = crate::auth::now_unix();
        self.index
            .lock()
            .map(|g| {
                let live: Vec<_> = g.entries.values().filter(|(_, _, exp)| *exp > now).collect();
                (live.len() as u32, live.iter().map(|(_, n, _)| *n).sum())
            })
            .unwrap_or((0, 0))
    }

    pub fn clear(&self) {
        if let Ok(mut g) = self.index.lock() {
            g.entries.clear();
            g.order.clear();
            g.bytes = 0;
        }
    }

    fn key(url: &str, headers: &[(String, String)]) -> String {
        let mut h = headers.to_vec();
        h.sort();
        crate::auth::token_hash(&serde_json::json!([url, h]).to_string())
    }

    async fn get(&self, key: &str) -> Option<(String, bytes::Bytes)> {
        let now = crate::auth::now_unix();
        let ct = {
            let g = self.index.lock().ok()?;
            let (ct, _, exp) = g.entries.get(key)?;
            if *exp <= now {
                return None;
            }
            ct.clone()
        };
        let data = tokio::fs::read(self.dir.join(key)).await.ok()?;
        Some((ct, bytes::Bytes::from(data)))
    }

    async fn put(&self, key: String, content_type: String, data: &[u8]) {
        if data.len() > MAX_CACHED_OBJECT || tokio::fs::create_dir_all(&self.dir).await.is_err() {
            return;
        }
        if tokio::fs::write(self.dir.join(&key), data).await.is_err() {
            return;
        }
        let mut evict = Vec::new();
        if let Ok(mut g) = self.index.lock() {
            if let Some((_, old, _)) = g.entries.remove(&key) {
                g.bytes = g.bytes.saturating_sub(old);
                g.order.retain(|k| k != &key);
            }
            g.bytes += data.len() as u64;
            g.entries.insert(key.clone(), (content_type, data.len() as u64, crate::auth::now_unix() + CACHE_TTL_SECS));
            g.order.push_back(key);
            while g.bytes > MAX_CACHE_BYTES {
                let Some(k) = g.order.pop_front() else { break };
                if let Some((_, n, _)) = g.entries.remove(&k) {
                    g.bytes = g.bytes.saturating_sub(n);
                }
                evict.push(k);
            }
        }
        for k in evict {
            let _ = tokio::fs::remove_file(self.dir.join(k)).await;
        }
    }
}

/// Tickets + cache + upstream client of one running server.
pub struct MediaProxy {
    tickets: Mutex<HashMap<String, Ticket>>,
    pub cache: MediaCache,
    pub upstream: Upstream,
}

impl MediaProxy {
    pub fn new(server_dir: &Path, upstream: Upstream) -> Self {
        Self { tickets: Mutex::new(HashMap::new()), cache: MediaCache::new(server_dir), upstream }
    }

    /// Register `url` + `headers` for `session`; returns `<base>/api/server/v1/media/<ticket>/<name>`. `expires_unix`
    /// = `None` uses [`TICKET_TTL_SECS`].
    pub fn mint(&self, base: &str, url: &str, headers: &[(String, String)], session: &str, expires_unix: Option<i64>) -> String {
        let id = crate::auth::new_token();
        let now = crate::auth::now_unix();
        let ticket = Ticket {
            url: url.to_owned(),
            headers: crate::upstream::clean_headers(headers),
            session: session.to_owned(),
            expires_unix: expires_unix.unwrap_or(now + TICKET_TTL_SECS),
        };
        if let Ok(mut g) = self.tickets.lock() {
            if g.len() >= MAX_TICKETS {
                g.retain(|_, t| t.expires_unix > now);
            }
            if g.len() >= MAX_TICKETS
                && let Some(oldest) = g.iter().min_by_key(|(_, t)| t.expires_unix).map(|(k, _)| k.clone())
            {
                g.remove(&oldest);
            }
            g.insert(id.clone(), ticket);
        }
        format!("{}{}/media/{id}/{}", base.trim_end_matches('/'), crate::API_PREFIX, display_name(url))
    }

    pub fn ticket(&self, id: &str) -> Option<Ticket> {
        self.tickets.lock().ok().and_then(|g| g.get(id).cloned())
    }

    pub fn ticket_count(&self) -> usize {
        self.tickets.lock().map(|g| g.len()).unwrap_or(0)
    }

    /// Serve one proxied object. `session_ok` says whether the ticket's session may still play (device not revoked,
    /// loopback peer for [`LOCAL_SESSION`]); `cache_enabled` = the media cache may be used.
    pub async fn serve(
        &self,
        method: &axum::http::Method,
        ticket_id: &str,
        range: Option<&str>,
        base: &str,
        session_ok: &dyn Fn(&str) -> bool,
    ) -> Result<Response, Failure> {
        let t = self.ticket(ticket_id).ok_or_else(|| Failure::new(StatusCode::NOT_FOUND, "media_not_found"))?;
        if t.expires_unix < crate::auth::now_unix() {
            return Err(Failure::new(StatusCode::GONE, "media_expired").with_message("Resolve this source again to refresh playback"));
        }
        if !session_ok(&t.session) {
            return Err(Failure::new(StatusCode::UNAUTHORIZED, "device_revoked"));
        }
        let key = MediaCache::key(&t.url, &t.headers);
        let head = method == axum::http::Method::HEAD;
        if !head
            && range.is_none()
            && let Some((ct, data)) = self.cache.get(&key).await
        {
            let mut r = Response::new(Body::from(data));
            set(&mut r, header::CONTENT_TYPE, &ct);
            set(&mut r, "x-tatakai-cache", "HIT");
            secure(r.headers_mut());
            return Ok(r);
        }
        let mut headers = t.headers.clone();
        headers.retain(|(k, _)| k != "range" && k != "accept-encoding");
        headers.push(("accept-encoding".into(), "identity".into()));
        if let Some(r) = range {
            if !valid_range(r) {
                return Err(Failure::new(StatusCode::RANGE_NOT_SATISFIABLE, "invalid_range"));
            }
            headers.push(("range".into(), r.to_owned()));
        }
        let url = Url::parse(&t.url).map_err(|_| Failure::new(StatusCode::BAD_REQUEST, "invalid_url"))?;
        let m = if head { reqwest::Method::HEAD } else { reqwest::Method::GET };
        let (resp, final_url) = self.upstream.send(m, url, &headers).await.map_err(|e| match e {
            crate::upstream::UpstreamError::Blocked(_) => Failure::new(StatusCode::FORBIDDEN, "private_upstream"),
            crate::upstream::UpstreamError::Timeout => Failure::new(StatusCode::GATEWAY_TIMEOUT, "upstream_timeout"),
            other => Failure::new(StatusCode::BAD_GATEWAY, "upstream_unavailable").with_message(other.to_string()),
        })?;
        let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        let mut out = HeaderMap::new();
        for name in ["content-type", "content-range", "accept-ranges", "content-length", "etag", "last-modified"] {
            if let Some(v) = resp.headers().get(name).and_then(|v| HeaderValue::from_bytes(v.as_bytes()).ok()) {
                out.insert(axum::http::HeaderName::from_static(name), v);
            }
        }
        secure(&mut out);
        let ct = out.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_owned();
        if head {
            return Ok((status, out, Body::empty()).into_response());
        }
        if status.is_success() && is_playlist(&ct, final_url.as_str()) {
            let text = read_capped(resp, MAX_PLAYLIST).await?;
            let text = String::from_utf8_lossy(&text).into_owned();
            let parent_origin = tk_core::net::origin_of(&final_url);
            let session = t.session.clone();
            let expires = t.expires_unix;
            let parent_headers = t.headers.clone();
            let mut mint = |u: &Url| {
                let h = if tk_core::net::origin_of(u) == parent_origin { parent_headers.clone() } else { strip_credentials(&parent_headers) };
                self.mint(base, u.as_str(), &h, &session, Some(expires))
            };
            let body = rewrite_playlist(&text, &final_url, &mut mint);
            out.remove(header::CONTENT_LENGTH);
            out.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/vnd.apple.mpegurl"));
            return Ok((status, out, Body::from(body)).into_response());
        }
        let size = resp.content_length().unwrap_or(0);
        if status == StatusCode::OK && range.is_none() && size > 0 && size <= MAX_CACHED_OBJECT as u64 && cacheable(&ct) {
            let data = read_capped(resp, MAX_CACHED_OBJECT).await?;
            self.cache.put(key, ct, &data).await;
            out.insert(header::CONTENT_LENGTH, HeaderValue::from(data.len()));
            out.insert("x-tatakai-cache", HeaderValue::from_static("MISS"));
            return Ok((status, out, Body::from(data)).into_response());
        }
        Ok((status, out, Body::from_stream(resp.bytes_stream())).into_response())
    }
}

fn set(r: &mut Response, name: impl axum::http::header::IntoHeaderName, value: &str) {
    if let Ok(v) = HeaderValue::from_str(value) {
        r.headers_mut().insert(name, v);
    }
}

/// Headers every proxied answer carries (classic `proxy.mjs`).
fn secure(h: &mut HeaderMap) {
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("sandbox; default-src 'none'"));
}

/// `bytes=<a>-<b>` with optional ends (classic `^bytes=\d*-\d*$`).
pub fn valid_range(r: &str) -> bool {
    let Some(spec) = r.strip_prefix("bytes=") else { return false };
    let Some((a, b)) = spec.split_once('-') else { return false };
    (a.chars().all(|c| c.is_ascii_digit())) && b.chars().all(|c| c.is_ascii_digit()) && !(a.is_empty() && b.is_empty())
}

async fn read_capped(resp: reqwest::Response, cap: usize) -> Result<Vec<u8>, Failure> {
    let mut resp = resp;
    let mut buf = Vec::new();
    loop {
        match resp.chunk().await {
            Ok(Some(c)) => {
                if buf.len() + c.len() > cap {
                    return Err(Failure::new(StatusCode::PAYLOAD_TOO_LARGE, "response_too_large"));
                }
                buf.extend_from_slice(&c);
            }
            Ok(None) => return Ok(buf),
            Err(_) => return Err(Failure::new(StatusCode::BAD_GATEWAY, "upstream_unavailable")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playlists_are_rewritten_through_the_proxy() {
        let base = Url::parse("https://cdn.example/show/ep1/master.m3u8").unwrap();
        let text = "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\"\n#EXTINF:4,\nseg-1.ts\n\nhttps://other.example/seg-2.ts\n";
        let mut seen = Vec::new();
        let mut mint = |u: &Url| {
            seen.push(u.to_string());
            format!("P{}", seen.len())
        };
        let out = rewrite_playlist(text, &base, &mut mint);
        assert_eq!(out, "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"P1\"\n#EXTINF:4,\nP2\n\nP3\n");
        assert_eq!(
            seen,
            vec![
                "https://cdn.example/show/ep1/key.bin".to_owned(),
                "https://cdn.example/show/ep1/seg-1.ts".to_owned(),
                "https://other.example/seg-2.ts".to_owned()
            ]
        );
    }

    #[test]
    fn names_ranges_and_types() {
        assert_eq!(display_name("https://a.example/x/master.m3u8?t=1"), "master.m3u8");
        assert_eq!(display_name("https://a.example/"), "media");
        assert_eq!(display_name("https://a.example/v/ep_1(x).mp4"), "ep_1x.mp4");
        assert!(valid_range("bytes=0-") && valid_range("bytes=-500") && valid_range("bytes=10-20"));
        assert!(!valid_range("bytes=-") && !valid_range("items=0-1") && !valid_range("bytes=a-b"));
        assert!(is_playlist("application/vnd.apple.mpegurl", "https://a/x"));
        assert!(is_playlist("text/plain", "https://a/x/index.M3U8?x=1"));
        assert!(!is_playlist("video/mp4", "https://a/x.mp4"));
        assert!(cacheable("image/jpeg") && !cacheable("video/mp4"));
    }

    #[test]
    fn tickets_carry_the_session_and_a_file_name() {
        let dir = tempfile::tempdir().unwrap();
        let p = MediaProxy::new(dir.path(), Upstream::new(None, None, std::time::Duration::from_secs(5)));
        let u = p.mint("http://192.168.1.5:8099", "https://cdn.example/a/index.m3u8", &[("Referer".into(), "https://e/".into())], "dev1", None);
        assert!(u.starts_with("http://192.168.1.5:8099/api/server/v1/media/"));
        assert!(u.ends_with("/index.m3u8"));
        let id = u.trim_start_matches("http://192.168.1.5:8099/api/server/v1/media/").split('/').next().unwrap();
        let t = p.ticket(id).unwrap();
        assert_eq!(t.session, "dev1");
        assert_eq!(t.headers, vec![("referer".to_owned(), "https://e/".to_owned())]);
        assert_eq!(p.ticket_count(), 1);
    }
}
