//! Outbound requests of the server (worker **k**): the media proxy and the extension installer fetch through
//! [`Upstream::send`], which re-validates every hop with the SSRF rules of `tk_core::net` (`Policy::extension_fetch`:
//! public addresses only, credentials stripped across origins, at most 5 redirects), optionally goes through the
//! user's outbound HTTP / SOCKS5 proxy (`server.proxy`; SOCKS needs reqwest's `socks` feature, see NOTES-deps.md), and
//! retries a Cloudflare challenge once through FlareSolverr when one is configured (`server.flaresolverr`; the
//! `cf_clearance` cookie and User-Agent it returns are reused per host for 10 minutes).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use tk_core::net::{Policy, SafeHttp, origin_of};
use url::Url;

/// How long a FlareSolverr clearance is reused for a host.
const CLEARANCE_TTL: Duration = Duration::from_secs(10 * 60);
const MAX_REDIRECTS: usize = 5;

#[derive(Debug, thiserror::Error)]
pub enum UpstreamError {
    #[error("blocked: {0}")]
    Blocked(String),
    #[error("proxy: {0}")]
    Proxy(String),
    #[error("http: {0}")]
    Http(String),
    #[error("timeout")]
    Timeout,
    #[error("too many redirects")]
    Redirects,
}

/// Headers never forwarded upstream (hop-by-hop or set by the client itself).
pub fn clean_headers(input: &[(String, String)]) -> Vec<(String, String)> {
    input
        .iter()
        .filter(|(k, v)| {
            let k = k.to_ascii_lowercase();
            !k.is_empty()
                && k.len() <= 200
                && v.len() <= 8192
                && !k.contains(['\r', '\n', ':'])
                && !v.contains(['\r', '\n'])
                && !matches!(k.as_str(), "host" | "connection" | "content-length" | "transfer-encoding" | "proxy-authorization")
                && !k.starts_with("x-forwarded-")
        })
        .map(|(k, v)| (k.to_ascii_lowercase(), v.clone()))
        .collect()
}

/// `headers` without cookies / authorization (cross-origin children of a playlist, redirects).
pub fn strip_credentials(headers: &[(String, String)]) -> Vec<(String, String)> {
    clean_headers(headers).into_iter().filter(|(k, _)| k != "cookie" && k != "authorization").collect()
}

/// Whether an answer is a Cloudflare challenge page (what FlareSolverr can solve).
pub fn is_cloudflare_challenge(status: u16, headers: &HeaderMap) -> bool {
    if !matches!(status, 403 | 429 | 503) {
        return false;
    }
    let get = |n: &str| headers.get(n).and_then(|v| v.to_str().ok()).unwrap_or("").to_ascii_lowercase();
    get("cf-mitigated") == "challenge" || (get("server").contains("cloudflare") && get("content-type").contains("text/html"))
}

pub struct Upstream {
    http: SafeHttp,
    proxy: Option<String>,
    flaresolverr: Option<String>,
    timeout: Duration,
    clearances: Mutex<HashMap<String, (Instant, String, String)>>,
}

impl Upstream {
    pub fn new(proxy: Option<String>, flaresolverr: Option<String>, timeout: Duration) -> Self {
        Self { http: SafeHttp::new(Policy::extension_fetch()), proxy, flaresolverr, timeout, clearances: Mutex::new(HashMap::new()) }
    }

    fn client(&self, url: &Url, addrs: &[std::net::SocketAddr]) -> Result<reqwest::Client, UpstreamError> {
        let mut b = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(self.timeout)
            .read_timeout(self.timeout);
        match &self.proxy {
            Some(p) => b = b.proxy(reqwest::Proxy::all(p.as_str()).map_err(|e| UpstreamError::Proxy(e.to_string()))?),
            None => {
                b = b.no_proxy();
                if let Some(url::Host::Domain(d)) = url.host() {
                    b = b.resolve_to_addrs(d, addrs);
                }
            }
        }
        b.build().map_err(|e| UpstreamError::Http(e.to_string()))
    }

    fn clearance(&self, host: &str) -> Option<(String, String)> {
        let g = self.clearances.lock().ok()?;
        g.get(host).filter(|(at, _, _)| at.elapsed() < CLEARANCE_TTL).map(|(_, c, ua)| (c.clone(), ua.clone()))
    }

    /// One request with SSRF re-validation on every hop. Returns the response (headers received; the body is
    /// streamed by the caller) and the final URL.
    pub async fn send(&self, method: reqwest::Method, url: Url, headers: &[(String, String)]) -> Result<(reqwest::Response, Url), UpstreamError> {
        let origin0 = origin_of(&url);
        let mut url = url;
        let mut method = method;
        let base = clean_headers(headers);
        let mut solved = false;
        let mut hop = 0;
        loop {
            let addrs = self.http.resolve_validated(&url).await.map_err(|e| UpstreamError::Blocked(e.to_string()))?;
            let client = self.client(&url, &addrs)?;
            let cross = origin_of(&url) != origin0;
            let mut hm = HeaderMap::new();
            let mut list: Vec<(String, String)> = if cross { strip_credentials(&base) } else { base.clone() };
            if let Some((cookie, ua)) = url.host_str().and_then(|h| self.clearance(h)) {
                list.retain(|(k, _)| k != "user-agent");
                list.push(("user-agent".into(), ua));
                match list.iter_mut().find(|(k, _)| k == "cookie") {
                    Some((_, v)) => *v = format!("{v}; {cookie}"),
                    None => list.push(("cookie".into(), cookie)),
                }
            }
            for (k, v) in &list {
                let (Ok(n), Ok(val)) = (HeaderName::from_bytes(k.as_bytes()), HeaderValue::from_str(v)) else { continue };
                hm.append(n, val);
            }
            let fut = client.request(method.clone(), url.clone()).headers(hm).send();
            let resp = tokio::time::timeout(self.timeout, fut)
                .await
                .map_err(|_| UpstreamError::Timeout)?
                .map_err(|e| UpstreamError::Http(e.to_string()))?;
            let status = resp.status();
            if status.is_redirection() && status.as_u16() != 304 {
                let Some(loc) = resp.headers().get(reqwest::header::LOCATION).and_then(|v| v.to_str().ok()) else {
                    return Ok((resp, url));
                };
                hop += 1;
                if hop > MAX_REDIRECTS {
                    return Err(UpstreamError::Redirects);
                }
                let next = url.join(loc).map_err(|e| UpstreamError::Http(e.to_string()))?;
                if status.as_u16() == 303 {
                    method = reqwest::Method::GET;
                }
                url = next;
                continue;
            }
            if !solved && self.flaresolverr.is_some() && is_cloudflare_challenge(status.as_u16(), resp.headers()) {
                solved = true;
                if let Some(fs) = &self.flaresolverr
                    && let Some(host) = url.host_str().map(str::to_owned)
                {
                    match solve(fs, url.as_str(), self.timeout.max(Duration::from_secs(60))).await {
                        Ok(s) => {
                            if let Ok(mut g) = self.clearances.lock() {
                                g.insert(host, (Instant::now(), s.cookie_header, s.user_agent));
                            }
                            continue;
                        }
                        Err(e) => tracing::debug!(error = %e, "FlareSolverr could not solve the challenge"),
                    }
                }
            }
            return Ok((resp, url));
        }
    }
}

/// What FlareSolverr returned for a challenge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Solution {
    pub cookie_header: String,
    pub user_agent: String,
}

/// Parse a FlareSolverr `request.get` answer (`{ status: "ok", solution: { cookies: [{name, value}], userAgent } }`).
pub fn parse_solution(v: &serde_json::Value) -> Option<Solution> {
    if v["status"].as_str() != Some("ok") {
        return None;
    }
    let sol = &v["solution"];
    let cookies: Vec<String> = sol["cookies"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|c| Some(format!("{}={}", c["name"].as_str()?, c["value"].as_str()?)))
                .filter(|c| !c.contains(['\r', '\n', ';']))
                .collect()
        })
        .unwrap_or_default();
    let ua = sol["userAgent"].as_str().unwrap_or("").to_owned();
    if cookies.is_empty() || ua.is_empty() || ua.contains(['\r', '\n']) {
        return None;
    }
    Some(Solution { cookie_header: cookies.join("; "), user_agent: ua })
}

/// `POST <endpoint>/v1 { cmd: "request.get", url, maxTimeout }` (the endpoint is the user's own FlareSolverr,
/// usually on this PC, so it is reached with a plain client).
pub async fn solve(endpoint: &str, url: &str, timeout: Duration) -> Result<Solution, String> {
    let client = reqwest::Client::builder().timeout(timeout + Duration::from_secs(5)).build().map_err(|e| e.to_string())?;
    let body = serde_json::json!({ "cmd": "request.get", "url": url, "maxTimeout": timeout.as_millis() as u64 });
    let resp = client.post(format!("{}/v1", endpoint.trim_end_matches('/'))).json(&body).send().await.map_err(|e| e.to_string())?;
    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    parse_solution(&v).ok_or_else(|| v["message"].as_str().unwrap_or("FlareSolverr returned no clearance").to_owned())
}

/// `GET <endpoint>/` -> `{ msg: "FlareSolverr is ready!", version }`: `(version, latency ms)` (the app's "Test").
pub async fn probe_flaresolverr(endpoint: &str, timeout: Duration) -> Result<(Option<String>, u32), String> {
    let u = Url::parse(endpoint).map_err(|e| e.to_string())?;
    if !matches!(u.scheme(), "http" | "https") {
        return Err("use an http:// or https:// address".into());
    }
    let client = reqwest::Client::builder().timeout(timeout).build().map_err(|e| e.to_string())?;
    let started = Instant::now();
    let resp = client.get(u).send().await.map_err(|e| e.to_string())?;
    let latency = started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
    let v: serde_json::Value = resp.json().await.map_err(|_| "not a FlareSolverr endpoint (no JSON answer)".to_owned())?;
    let ready = v["msg"].as_str().is_some_and(|m| m.to_ascii_lowercase().contains("ready")) || v["version"].is_string();
    if !ready {
        return Err("not a FlareSolverr endpoint".into());
    }
    Ok((v["version"].as_str().map(str::to_owned), latency))
}

/// Check that an outbound proxy URL works: one HTTPS request through it (`server.proxy` "Test"). Returns the
/// latency in milliseconds.
pub async fn probe_proxy(proxy: &str, timeout: Duration) -> Result<u32, String> {
    let p = Url::parse(proxy).map_err(|e| e.to_string())?;
    if !matches!(p.scheme(), "http" | "https" | "socks5" | "socks5h") {
        return Err("use http://, https://, socks5:// or socks5h://".into());
    }
    let client = reqwest::Client::builder()
        .proxy(reqwest::Proxy::all(proxy).map_err(|e| e.to_string())?)
        .timeout(timeout)
        .build()
        .map_err(|e| e.to_string())?;
    let started = Instant::now();
    // Any HTTP answer proves the tunnel works (AniList is a host the app already talks to).
    client.get("https://graphql.anilist.co/").send().await.map_err(|e| e.to_string())?;
    Ok(started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_are_cleaned_and_credentials_stripped() {
        let h = vec![
            ("Referer".to_owned(), "https://a/".to_owned()),
            ("Host".to_owned(), "x".to_owned()),
            ("Cookie".to_owned(), "k=v".to_owned()),
            ("X-Forwarded-For".to_owned(), "1.2.3.4".to_owned()),
            ("bad\r\nname".to_owned(), "v".to_owned()),
        ];
        assert_eq!(clean_headers(&h), vec![("referer".to_owned(), "https://a/".to_owned()), ("cookie".to_owned(), "k=v".to_owned())]);
        assert_eq!(strip_credentials(&h), vec![("referer".to_owned(), "https://a/".to_owned())]);
    }

    #[test]
    fn cloudflare_challenges_are_recognised() {
        let mut h = HeaderMap::new();
        h.insert("cf-mitigated", HeaderValue::from_static("challenge"));
        assert!(is_cloudflare_challenge(403, &h));
        assert!(!is_cloudflare_challenge(200, &h));
        let mut g = HeaderMap::new();
        g.insert("server", HeaderValue::from_static("cloudflare"));
        g.insert("content-type", HeaderValue::from_static("text/html; charset=UTF-8"));
        assert!(is_cloudflare_challenge(503, &g));
        assert!(!is_cloudflare_challenge(503, &HeaderMap::new()));
    }

    #[test]
    fn flaresolverr_solutions_parse() {
        let v = serde_json::json!({ "status": "ok", "solution": {
            "cookies": [{ "name": "cf_clearance", "value": "abc" }, { "name": "x", "value": "1" }], "userAgent": "Mozilla/5.0" } });
        assert_eq!(parse_solution(&v), Some(Solution { cookie_header: "cf_clearance=abc; x=1".into(), user_agent: "Mozilla/5.0".into() }));
        assert_eq!(parse_solution(&serde_json::json!({ "status": "error", "message": "timeout" })), None);
    }
}
