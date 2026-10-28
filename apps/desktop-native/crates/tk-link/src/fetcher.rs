//! Loopback SSRF fetcher for the `url` session source (03 section 4.1). FFmpeg NEVER receives the external URL: it reads
//! `http://127.0.0.1:<port>/u/<token>/...` and this server fetches upstream through `tk_core::net::SafeHttp`
//! (IP validation incl. DNS rebinding, manual redirects, cross-origin Cookie/Authorization strip), rewriting HLS playlists
//! so segments, keys and maps go through here too.
//!
//! Paths: `/u/<token>/p/<depth>/<b64url target>` (playlist, rewritten) and `/u/<token>/s/<b64url target>` (opaque bytes).

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use axum::body::Body;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use futures_util::StreamExt;
use hyper::body::Incoming;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tk_core::net::{NetError, SafeHttp, SafeRequest, host_matches_permissions, origin_of, read_capped};
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use url::Url;

use crate::playlist::{self, MAX_PLAYLIST_BYTES, RewriteError, UriKind};

pub const MAX_CONCURRENT_PER_SESSION: usize = 64;
pub const MAX_BYTES_PER_SESSION: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FetchError {
    /// The policy refuses this URL (scheme, port, address, host, rewrite rules): `403 url_source_denied`.
    #[error("denied by policy")]
    Denied,
    /// Upstream unreachable / bad answer: `503 source_unavailable`.
    #[error("source unavailable")]
    Unavailable,
}

impl From<NetError> for FetchError {
    fn from(e: NetError) -> Self {
        match e {
            NetError::Scheme | NetError::Port | NetError::Host | NetError::BlockedIp(_) | NetError::Header(_) | NetError::TooManyRedirects => Self::Denied,
            NetError::Dns | NetError::Timeout | NetError::TooLarge | NetError::Http(_) => Self::Unavailable,
        }
    }
}

impl From<RewriteError> for FetchError {
    fn from(e: RewriteError) -> Self {
        match e {
            RewriteError::NotPlaylist | RewriteError::NotUtf8 | RewriteError::BadUri => Self::Unavailable,
            _ => Self::Denied,
        }
    }
}

struct Token {
    /// Per-source client: global policy + optional extension `network:domain:*` intersection.
    http: SafeHttp,
    initial: Url,
    headers: Vec<(String, String)>,
    bytes: AtomicU64,
    slots: Semaphore,
    denied: AtomicBool,
}

impl Token {
    /// Cookie only travels to the initial origin; everything else on the allow-list is forwarded.
    fn headers_for(&self, target: &Url, range: Option<String>) -> Vec<(String, String)> {
        let same = origin_of(target) == origin_of(&self.initial);
        let mut h: Vec<_> =
            self.headers.iter().filter(|(k, _)| same || !k.eq_ignore_ascii_case("cookie")).cloned().collect();
        if let Some(r) = range {
            h.retain(|(k, _)| !k.eq_ignore_ascii_case("range"));
            h.push(("range".into(), r));
        }
        h
    }
}

pub struct SourceSpec {
    pub url: Url,
    /// Already sanitised with [`tk_core::net::sanitize_headers`].
    pub headers: Vec<(String, String)>,
    /// `network:domain:*` permissions of the extension, when the PC has it installed.
    pub extension_permissions: Option<Vec<String>>,
}

pub struct Fetcher {
    addr: SocketAddr,
    http: SafeHttp,
    tokens: Arc<Mutex<HashMap<String, Arc<Token>>>>,
    files: Arc<Mutex<HashMap<String, std::path::PathBuf>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Fetcher {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn enc(u: &Url) -> String {
    URL_SAFE_NO_PAD.encode(u.as_str())
}

fn dec(s: &str) -> Option<Url> {
    let bytes = URL_SAFE_NO_PAD.decode(s).ok()?;
    Url::parse(std::str::from_utf8(&bytes).ok()?).ok()
}

impl Fetcher {
    /// Bind 127.0.0.1 on an ephemeral port and serve. `http` carries the global policy (tests inject a lenient one).
    pub async fn start(http: SafeHttp) -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let addr = listener.local_addr()?;
        let tokens: Arc<Mutex<HashMap<String, Arc<Token>>>> = Arc::default();
        let files: Arc<Mutex<HashMap<String, std::path::PathBuf>>> = Arc::default();
        let shared = tokens.clone();
        let shared_files = files.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((sock, _)) = listener.accept().await else { return };
                let tokens = shared.clone();
                let files = shared_files.clone();
                tokio::spawn(async move {
                    let svc = hyper::service::service_fn(move |req| handle(tokens.clone(), files.clone(), req));
                    let _ = hyper::server::conn::http1::Builder::new().serve_connection(TokioIo::new(sock), svc).await;
                });
            }
        });
        Ok(Self { addr, http, tokens, files, task })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Register a source; returns the session token. The first request path is [`Fetcher::entry_url`].
    pub fn register(&self, spec: SourceSpec) -> String {
        let token = uuid::Uuid::new_v4().simple().to_string();
        let mut http = self.http.clone();
        if let Some(perms) = spec.extension_permissions {
            http = http.with_host_filter(Arc::new(move |h: &str| host_matches_permissions(h, &perms)));
        }
        let t = Arc::new(Token {
            http,
            initial: spec.url,
            headers: spec.headers,
            bytes: AtomicU64::new(0),
            slots: Semaphore::new(MAX_CONCURRENT_PER_SESSION),
            denied: AtomicBool::new(false),
        });
        self.tokens.lock().unwrap_or_else(PoisonError::into_inner).insert(token.clone(), t);
        token
    }

    pub fn unregister(&self, token: &str) {
        self.tokens.lock().unwrap_or_else(PoisonError::into_inner).remove(token);
        self.files.lock().unwrap_or_else(PoisonError::into_inner).remove(token);
    }

    /// Serve one local file (library source) at `/f/<token>` with Range support. The path never reaches FFmpeg.
    pub fn register_file(&self, path: std::path::PathBuf) -> String {
        let token = uuid::Uuid::new_v4().simple().to_string();
        self.files.lock().unwrap_or_else(PoisonError::into_inner).insert(token.clone(), path);
        token
    }

    pub fn file_url(&self, token: &str) -> String {
        format!("http://{}/f/{token}", self.addr)
    }

    /// True once any request of this source was refused by the policy.
    pub fn was_denied(&self, token: &str) -> bool {
        self.tokens.lock().unwrap_or_else(PoisonError::into_inner).get(token).is_some_and(|t| t.denied.load(Ordering::Relaxed))
    }

    pub fn bytes_served(&self, token: &str) -> u64 {
        self.tokens.lock().unwrap_or_else(PoisonError::into_inner).get(token).map_or(0, |t| t.bytes.load(Ordering::Relaxed))
    }

    /// The loopback URL FFmpeg opens: an HLS playlist (rewritten) or an opaque stream (mp4).
    pub fn entry_url(&self, token: &str, target: &Url, hls: bool) -> String {
        if hls {
            format!("http://{}/u/{token}/p/0/{}", self.addr, enc(target))
        } else {
            format!("http://{}/u/{token}/s/{}", self.addr, enc(target))
        }
    }

    /// Validate the URL against the policy (scheme, port, DNS answers) before a session is created.
    pub async fn validate(&self, token: &str, target: &Url) -> Result<(), FetchError> {
        let t = self.token(token).ok_or(FetchError::Unavailable)?;
        t.http.resolve_validated(target).await.map(|_| ()).map_err(FetchError::from)
    }

    /// Load the top playlist (and, for a master, its first variant) to learn the duration and fail early.
    pub async fn probe_hls(&self, token: &str, target: &Url) -> Result<Option<f64>, FetchError> {
        let t = self.token(token).ok_or(FetchError::Unavailable)?;
        let top = load_playlist(&t, token, target, 0).await?;
        if !top.is_master {
            return Ok((top.duration_secs > 0.0).then_some(top.duration_secs));
        }
        // first variant line of the rewritten master -> decode its target
        let first = top
            .text
            .lines()
            .find(|l| l.contains("/p/1/"))
            .and_then(|l| l.split("/p/1/").nth(1))
            .and_then(|b| dec(b.split('"').next().unwrap_or(b)));
        let Some(variant) = first else { return Ok(None) };
        let media = load_playlist(&t, token, &variant, 1).await?;
        Ok((media.duration_secs > 0.0).then_some(media.duration_secs))
    }

    fn token(&self, token: &str) -> Option<Arc<Token>> {
        self.tokens.lock().unwrap_or_else(PoisonError::into_inner).get(token).cloned()
    }
}

async fn load_playlist(t: &Token, token: &str, target: &Url, depth: u8) -> Result<playlist::Rewritten, FetchError> {
    let resp = t.http.send(SafeRequest::get(target.clone(), t.headers_for(target, None))).await.map_err(|e| {
        let e = FetchError::from(e);
        if e == FetchError::Denied {
            t.denied.store(true, Ordering::Relaxed);
        }
        e
    })?;
    if !resp.response.status().is_success() {
        return Err(FetchError::Unavailable);
    }
    let final_url = resp.final_url.clone();
    let body = read_capped(resp.response, MAX_PLAYLIST_BYTES).await.map_err(|e| match e {
        NetError::TooLarge => FetchError::Denied,
        other => other.into(),
    })?;
    let addr_prefix = format!("/u/{token}");
    let r = playlist::rewrite(&body, &final_url, depth, |u, kind| map_uri(t, &addr_prefix, depth, u, kind));
    match r {
        Ok(r) => Ok(r),
        Err(e) => {
            let e = FetchError::from(e);
            if e == FetchError::Denied {
                t.denied.store(true, Ordering::Relaxed);
            }
            Err(e)
        }
    }
}

/// Validate one URI of a playlist (cheap, synchronous checks) and produce its loopback path.
fn map_uri(t: &Token, prefix: &str, depth: u8, u: &Url, kind: UriKind) -> Result<String, RewriteError> {
    t.http.check_url(u).map_err(|_| RewriteError::Denied)?;
    // Keys and init maps are fetched first and decide whether decryption can work at all: refuse literal private
    // addresses here instead of at fetch time.
    if matches!(kind, UriKind::Key | UriKind::Map)
        && let Some(ip) = match u.host() {
            Some(url::Host::Ipv4(v)) => Some(std::net::IpAddr::V4(v)),
            Some(url::Host::Ipv6(v)) => Some(std::net::IpAddr::V6(v)),
            _ => None,
        }
        && !t.http.ip_allowed(ip)
    {
        return Err(RewriteError::Denied);
    }
    Ok(match kind {
        UriKind::Playlist => format!("{prefix}/p/{}/{}", depth + 1, enc(u)),
        UriKind::Segment | UriKind::Key | UriKind::Map => format!("{prefix}/s/{}", enc(u)),
    })
}

fn status(code: StatusCode) -> Response<Body> {
    let mut r = Response::new(Body::empty());
    *r.status_mut() = code;
    r
}

type Tokens = Arc<Mutex<HashMap<String, Arc<Token>>>>;
type Files = Arc<Mutex<HashMap<String, std::path::PathBuf>>>;

async fn handle(tokens: Tokens, files: Files, req: Request<Incoming>) -> Result<Response<Body>, std::convert::Infallible> {
    Ok(handle_inner(tokens, files, req).await.unwrap_or_else(status))
}

/// `GET /f/<token>` with `Range: bytes=a-b` (single range) support.
async fn serve_file(path: std::path::PathBuf, range: Option<String>) -> Result<Response<Body>, StatusCode> {
    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    let mut f = tokio::fs::File::open(&path).await.map_err(|_| StatusCode::NOT_FOUND)?;
    let len = f.metadata().await.map_err(|_| StatusCode::NOT_FOUND)?.len();
    let (start, end, partial) = match range.as_deref().and_then(|r| r.strip_prefix("bytes=")) {
        Some(r) => {
            let (a, b) = r.split_once('-').ok_or(StatusCode::RANGE_NOT_SATISFIABLE)?;
            let (a, b): (u64, u64) = if a.is_empty() {
                let n: u64 = b.parse().map_err(|_| StatusCode::RANGE_NOT_SATISFIABLE)?;
                (len.saturating_sub(n), len.saturating_sub(1))
            } else {
                (a.parse().map_err(|_| StatusCode::RANGE_NOT_SATISFIABLE)?, if b.is_empty() { len.saturating_sub(1) } else { b.parse().map_err(|_| StatusCode::RANGE_NOT_SATISFIABLE)? })
            };
            if a >= len || b < a {
                return Err(StatusCode::RANGE_NOT_SATISFIABLE);
            }
            (a, b.min(len - 1), true)
        }
        None => (0, len.saturating_sub(1), false),
    };
    f.seek(std::io::SeekFrom::Start(start)).await.map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let take = if len == 0 { 0 } else { end - start + 1 };
    let stream = tokio_util::io::ReaderStream::new(f.take(take));
    let mut resp = Response::new(Body::from_stream(stream));
    let h = resp.headers_mut();
    h.insert("accept-ranges", "bytes".parse().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?);
    h.insert("content-length", take.to_string().parse().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?);
    if partial {
        *resp.status_mut() = StatusCode::PARTIAL_CONTENT;
        resp.headers_mut().insert("content-range", format!("bytes {start}-{end}/{len}").parse().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?);
    }
    Ok(resp)
}

async fn handle_inner(tokens: Tokens, files: Files, req: Request<Incoming>) -> Result<Response<Body>, StatusCode> {
    if req.method() != Method::GET && req.method() != Method::HEAD {
        return Err(StatusCode::METHOD_NOT_ALLOWED);
    }
    // /u/<token>/p/<depth>/<b64>  |  /u/<token>/s/<b64>
    let path = req.uri().path().to_owned();
    let mut seg = path.trim_start_matches('/').split('/');
    let first = seg.next();
    if first == Some("f") {
        let token = seg.next().ok_or(StatusCode::NOT_FOUND)?;
        let path = files.lock().unwrap_or_else(PoisonError::into_inner).get(token).cloned().ok_or(StatusCode::NOT_FOUND)?;
        let range = req.headers().get(hyper::header::RANGE).and_then(|v| v.to_str().ok()).map(str::to_owned);
        return serve_file(path, range).await;
    }
    if first != Some("u") {
        return Err(StatusCode::NOT_FOUND);
    }
    let token = seg.next().ok_or(StatusCode::NOT_FOUND)?.to_owned();
    let kind = seg.next().ok_or(StatusCode::NOT_FOUND)?;
    let t = tokens.lock().unwrap_or_else(PoisonError::into_inner).get(&token).cloned().ok_or(StatusCode::NOT_FOUND)?;
    let (depth, target) = match kind {
        "p" => {
            let d: u8 = seg.next().and_then(|d| d.parse().ok()).ok_or(StatusCode::NOT_FOUND)?;
            (Some(d), dec(seg.next().ok_or(StatusCode::NOT_FOUND)?).ok_or(StatusCode::BAD_REQUEST)?)
        }
        "s" => (None, dec(seg.next().ok_or(StatusCode::NOT_FOUND)?).ok_or(StatusCode::BAD_REQUEST)?),
        _ => return Err(StatusCode::NOT_FOUND),
    };
    let _permit = tokio::time::timeout(Duration::from_secs(20), t.slots.acquire()).await.map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?.map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    if t.bytes.load(Ordering::Relaxed) >= MAX_BYTES_PER_SESSION {
        return Err(StatusCode::FORBIDDEN);
    }
    let map_err = |e: FetchError| match e {
        FetchError::Denied => StatusCode::FORBIDDEN,
        FetchError::Unavailable => StatusCode::BAD_GATEWAY,
    };

    if let Some(depth) = depth {
        let r = load_playlist(&t, &token, &target, depth).await.map_err(map_err)?;
        let body = r.text.into_bytes();
        t.bytes.fetch_add(body.len() as u64, Ordering::Relaxed);
        let mut resp = Response::new(Body::from(body));
        resp.headers_mut().insert("content-type", "application/vnd.apple.mpegurl".parse().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?);
        return Ok(resp);
    }

    let range = req.headers().get(hyper::header::RANGE).and_then(|v| v.to_str().ok()).map(str::to_owned);
    let up = t.http.send(SafeRequest::get(target.clone(), t.headers_for(&target, range))).await.map_err(|e| {
        let e = FetchError::from(e);
        if e == FetchError::Denied {
            t.denied.store(true, Ordering::Relaxed);
        }
        map_err(e)
    })?;
    let mut resp = Response::new(Body::empty());
    *resp.status_mut() = up.response.status();
    for name in ["content-type", "content-length", "content-range", "accept-ranges"] {
        if let Some(v) = up.response.headers().get(name) {
            resp.headers_mut().insert(hyper::header::HeaderName::from_static(name), v.clone());
        }
    }
    let t2 = t.clone();
    let stream = up.response.bytes_stream().map(move |chunk| match chunk {
        Ok(b) => {
            let total = t2.bytes.fetch_add(b.len() as u64, Ordering::Relaxed) + b.len() as u64;
            if total > MAX_BYTES_PER_SESSION { Err(std::io::Error::other("session byte quota exceeded")) } else { Ok(b) }
        }
        Err(e) => Err(std::io::Error::other(e.without_url().to_string())),
    });
    *resp.body_mut() = Body::from_stream(stream);
    Ok(resp)
}
