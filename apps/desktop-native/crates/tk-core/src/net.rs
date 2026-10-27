//! SSRF-safe outbound HTTP shared by the extension host (`fetch`) and the Link `url` source fetcher.
//!
//! Guarantees, per hop: scheme/port allow-list, DNS resolved by us, every resolved address validated (loopback, private,
//! CGNAT, link-local, ULA, multicast, unspecified, documentation, IPv4-mapped/NAT64 embeds), and the connection is pinned
//! to the validated addresses (`resolve_to_addrs`) so a second DNS answer (rebinding) can never be used. Redirects are
//! followed manually (max 5), each hop is re-validated, and `Cookie`/`Authorization` are dropped on a change of origin.

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use thiserror::Error;
use url::Url;

#[derive(Debug, Error)]
pub enum NetError {
    #[error("scheme not allowed")]
    Scheme,
    #[error("port not allowed")]
    Port,
    #[error("host not allowed")]
    Host,
    #[error("address not allowed: {0}")]
    BlockedIp(IpAddr),
    #[error("dns resolution failed")]
    Dns,
    #[error("too many redirects")]
    TooManyRedirects,
    #[error("invalid header: {0}")]
    Header(&'static str),
    #[error("request timed out")]
    Timeout,
    #[error("response too large")]
    TooLarge,
    #[error("http error: {0}")]
    Http(String),
}

impl From<reqwest::Error> for NetError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_timeout() { Self::Timeout } else { Self::Http(e.without_url().to_string()) }
    }
}

/// IPv4 addresses that must never be reachable from an untrusted URL.
pub fn is_blocked_v4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_multicast()
        || ip.is_broadcast()
        || ip.is_unspecified()
        || ip.is_documentation()
        || o[0] == 0                                  // 0.0.0.0/8
        || (o[0] == 100 && (o[1] & 0xC0) == 64)       // 100.64.0.0/10 CGNAT (also Alibaba metadata 100.100.100.200)
        || (o[0] == 192 && o[1] == 0 && o[2] == 0)    // 192.0.0.0/24 IETF protocol assignments
        || (o[0] == 198 && (o[1] & 0xFE) == 18)       // 198.18.0.0/15 benchmarking
        || o[0] >= 240                                // reserved / future use
}

pub fn is_blocked_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_blocked_v4(v4);
    }
    let s = ip.segments();
    // 64:ff9b::/96 (NAT64) embeds an IPv4 address in the last 32 bits
    if s[0] == 0x0064 && s[1] == 0xff9b && s[2..6] == [0, 0, 0, 0] {
        let v4 = Ipv4Addr::new((s[6] >> 8) as u8, s[6] as u8, (s[7] >> 8) as u8, s[7] as u8);
        return is_blocked_v4(v4);
    }
    // 6to4 (2002::/16) embeds an IPv4 address in bits 16..48
    if s[0] == 0x2002 {
        let v4 = Ipv4Addr::new((s[1] >> 8) as u8, s[1] as u8, (s[2] >> 8) as u8, s[2] as u8);
        return is_blocked_v4(v4);
    }
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || (s[0] & 0xFE00) == 0xFC00      // fc00::/7 ULA
        || (s[0] & 0xFFC0) == 0xFE80      // fe80::/10 link-local
        || (s[0] & 0xFFC0) == 0xFEC0      // fec0::/10 site-local (deprecated)
        || (s[0] == 0x2001 && s[1] == 0x0db8) // documentation
}

pub fn is_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => is_blocked_v4(v),
        IpAddr::V6(v) => is_blocked_v6(v),
    }
}

/// `network:domain:<host>` permission match: exact host or a subdomain of it; `*` matches any host.
/// Accepts either the full permission strings or bare domains.
pub fn host_matches_permissions(host: &str, permissions: &[String]) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    permissions.iter().any(|p| {
        let Some(d) = p.strip_prefix("network:domain:") else { return false };
        let d = d.trim().trim_end_matches('.').to_ascii_lowercase();
        d == "*" || host == d || host.ends_with(&format!(".{d}"))
    })
}

#[async_trait]
pub trait Resolver: Send + Sync {
    async fn resolve(&self, host: &str, port: u16) -> std::io::Result<Vec<IpAddr>>;
}

pub struct SystemResolver;

#[async_trait]
impl Resolver for SystemResolver {
    async fn resolve(&self, host: &str, port: u16) -> std::io::Result<Vec<IpAddr>> {
        Ok(tokio::net::lookup_host((host, port)).await?.map(|a| a.ip()).collect())
    }
}

#[derive(Clone, Debug)]
pub struct Policy {
    pub allow_http: bool,
    /// `None` = any port except 0.
    pub allowed_ports: Option<Vec<u16>>,
    pub max_redirects: usize,
    pub timeout: Duration,
    /// Lower-case header names that may be forwarded; `None` = any header (CR/LF is always rejected).
    pub header_allowlist: Option<Vec<&'static str>>,
    pub max_header_bytes: usize,
    /// Test-only escape hatch: permit loopback addresses (never set in production code).
    pub allow_loopback: bool,
}

impl Policy {
    /// Policy of the Link `url` source (03 section 4.1).
    pub fn link_source(allow_http: bool) -> Self {
        Self {
            allow_http,
            allowed_ports: Some(vec![80, 443, 8080, 8443]),
            max_redirects: 5,
            timeout: Duration::from_secs(20),
            header_allowlist: Some(vec!["referer", "origin", "user-agent", "accept", "accept-language", "cookie", "range"]),
            max_header_bytes: 8 * 1024,
            allow_loopback: false,
        }
    }

    /// Policy of the extension host `fetch` (globals.v1.md section 2): any port, http+https, 30 s.
    pub fn extension_fetch() -> Self {
        Self {
            allow_http: true,
            allowed_ports: None,
            max_redirects: 5,
            timeout: Duration::from_secs(30),
            header_allowlist: None,
            max_header_bytes: 32 * 1024,
            allow_loopback: false,
        }
    }
}

/// Origin triple used for the cross-origin credential strip.
pub fn origin_of(u: &Url) -> (String, String, u16) {
    (u.scheme().to_owned(), u.host_str().unwrap_or("").to_ascii_lowercase(), u.port_or_known_default().unwrap_or(0))
}

/// Validate `headers` against `policy`; returns name/value pairs ready to send.
pub fn sanitize_headers(policy: &Policy, headers: &BTreeMap<String, String>) -> Result<Vec<(String, String)>, NetError> {
    let mut total = 0usize;
    let mut out = Vec::with_capacity(headers.len());
    for (k, v) in headers {
        if k.is_empty() || k.bytes().any(|b| b == b'\r' || b == b'\n' || b == 0 || b == b':') {
            return Err(NetError::Header("illegal header name"));
        }
        if v.bytes().any(|b| b == b'\r' || b == b'\n' || b == 0) {
            return Err(NetError::Header("CR/LF in header value"));
        }
        total += k.len() + v.len() + 4;
        if total > policy.max_header_bytes {
            return Err(NetError::Header("headers too large"));
        }
        let lower = k.to_ascii_lowercase();
        if let Some(allow) = &policy.header_allowlist {
            if !allow.contains(&lower.as_str()) {
                return Err(NetError::Header("header not allowed"));
            }
        } else if matches!(lower.as_str(), "host" | "content-length" | "transfer-encoding" | "connection") {
            continue;
        }
        out.push((lower, v.clone()));
    }
    Ok(out)
}

pub struct SafeRequest {
    pub method: reqwest::Method,
    pub url: Url,
    pub headers: Vec<(String, String)>,
    pub body: Option<Bytes>,
}

impl SafeRequest {
    pub fn get(url: Url, headers: Vec<(String, String)>) -> Self {
        Self { method: reqwest::Method::GET, url, headers, body: None }
    }
}

pub struct SafeResponse {
    pub response: reqwest::Response,
    pub final_url: Url,
}

/// Extra per-host gate (e.g. an extension's `network:domain:*` permissions), checked on every hop.
pub type HostFilter = Arc<dyn Fn(&str) -> bool + Send + Sync>;

#[derive(Clone)]
pub struct SafeHttp {
    resolver: Arc<dyn Resolver>,
    policy: Policy,
    host_filter: Option<HostFilter>,
}

impl SafeHttp {
    pub fn new(policy: Policy) -> Self {
        Self { resolver: Arc::new(SystemResolver), policy, host_filter: None }
    }

    pub fn with_resolver(policy: Policy, resolver: Arc<dyn Resolver>) -> Self {
        Self { resolver, policy, host_filter: None }
    }

    /// Require every hop's host to satisfy `filter` (in addition to the IP/port/scheme checks).
    pub fn with_host_filter(mut self, filter: HostFilter) -> Self {
        self.host_filter = Some(filter);
        self
    }

    pub fn policy(&self) -> &Policy {
        &self.policy
    }

    /// Scheme / port / host-form checks that need no DNS.
    pub fn check_url(&self, u: &Url) -> Result<(), NetError> {
        match u.scheme() {
            "https" => {}
            "http" if self.policy.allow_http => {}
            _ => return Err(NetError::Scheme),
        }
        if !u.username().is_empty() || u.password().is_some() {
            return Err(NetError::Host);
        }
        let port = u.port_or_known_default().ok_or(NetError::Port)?;
        if port == 0 || self.policy.allowed_ports.as_ref().is_some_and(|p| !p.contains(&port)) {
            return Err(NetError::Port);
        }
        let host = u.host_str().filter(|h| !h.is_empty()).ok_or(NetError::Host)?;
        if self.host_filter.as_ref().is_some_and(|f| !f(host)) {
            return Err(NetError::Host);
        }
        Ok(())
    }

    fn ip_ok(&self, ip: IpAddr) -> bool {
        (self.policy.allow_loopback && ip.is_loopback()) || !is_blocked_ip(ip)
    }

    /// Resolve and validate. Every returned address is safe to connect to.
    pub async fn resolve_validated(&self, u: &Url) -> Result<Vec<SocketAddr>, NetError> {
        self.check_url(u)?;
        let port = u.port_or_known_default().ok_or(NetError::Port)?;
        let ips: Vec<IpAddr> = match u.host() {
            Some(url::Host::Ipv4(v)) => vec![IpAddr::V4(v)],
            Some(url::Host::Ipv6(v)) => vec![IpAddr::V6(v)],
            Some(url::Host::Domain(d)) => self.resolver.resolve(d, port).await.map_err(|_| NetError::Dns)?,
            None => return Err(NetError::Host),
        };
        if ips.is_empty() {
            return Err(NetError::Dns);
        }
        // Reject if ANY answer is forbidden: a mixed answer is how rebinding/rotation attacks hide.
        if let Some(bad) = ips.iter().find(|ip| !self.ip_ok(**ip)) {
            return Err(NetError::BlockedIp(*bad));
        }
        Ok(ips.into_iter().map(|ip| SocketAddr::new(ip, port)).collect())
    }

    /// Perform a request, following up to `max_redirects` redirects with full re-validation.
    pub async fn send(&self, req: SafeRequest) -> Result<SafeResponse, NetError> {
        let origin0 = origin_of(&req.url);
        let mut url = req.url;
        let mut method = req.method;
        let mut body = req.body;
        for hop in 0..=self.policy.max_redirects {
            let addrs = self.resolve_validated(&url).await?;
            let mut builder = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .no_proxy()
                .timeout(self.policy.timeout)
                .connect_timeout(self.policy.timeout);
            if let Some(url::Host::Domain(d)) = url.host() {
                builder = builder.resolve_to_addrs(d, &addrs);
            }
            let client = builder.build()?;

            let cross_origin = origin_of(&url) != origin0;
            let mut hm = HeaderMap::new();
            for (k, v) in &req.headers {
                if cross_origin && (k.eq_ignore_ascii_case("cookie") || k.eq_ignore_ascii_case("authorization")) {
                    continue;
                }
                let name = HeaderName::from_bytes(k.as_bytes()).map_err(|_| NetError::Header("illegal header name"))?;
                let val = HeaderValue::from_str(v).map_err(|_| NetError::Header("illegal header value"))?;
                hm.append(name, val);
            }
            let mut rb = client.request(method.clone(), url.clone()).headers(hm);
            if let Some(b) = &body {
                rb = rb.body(b.clone());
            }
            let resp = rb.send().await?;
            let status = resp.status();
            if status.is_redirection() && status.as_u16() != 304 {
                let Some(loc) = resp.headers().get(reqwest::header::LOCATION).and_then(|v| v.to_str().ok()) else {
                    return Ok(SafeResponse { response: resp, final_url: url });
                };
                if hop == self.policy.max_redirects {
                    return Err(NetError::TooManyRedirects);
                }
                let next = url.join(loc).map_err(|_| NetError::Host)?;
                if matches!(status.as_u16(), 301..=303) && method != reqwest::Method::GET && method != reqwest::Method::HEAD {
                    method = reqwest::Method::GET;
                    body = None;
                }
                url = next;
                continue;
            }
            return Ok(SafeResponse { response: resp, final_url: url });
        }
        Err(NetError::TooManyRedirects)
    }
}

/// Read a response body with a hard cap.
pub async fn read_capped(mut resp: reqwest::Response, cap: usize) -> Result<Bytes, NetError> {
    if resp.content_length().is_some_and(|l| l > cap as u64) {
        return Err(NetError::TooLarge);
    }
    let mut buf = Vec::new();
    while let Some(chunk) = resp.chunk().await? {
        if buf.len() + chunk.len() > cap {
            return Err(NetError::TooLarge);
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn blocked_address_table() {
        for blocked in [
            "127.0.0.1", "127.255.255.254", "10.0.0.1", "172.16.0.1", "172.31.255.255", "192.168.1.1", "100.64.0.1", "100.100.100.200",
            "169.254.169.254", "169.254.0.1", "0.0.0.0", "0.1.2.3", "224.0.0.1", "255.255.255.255", "192.0.2.1", "198.18.0.1", "240.0.0.1",
            "::1", "::", "fe80::1", "fc00::1", "fd00:ec2::254", "ff02::1", "::ffff:127.0.0.1", "::ffff:10.0.0.1", "::ffff:169.254.169.254",
            "64:ff9b::7f00:1", "2002:7f00:1::1", "2001:db8::1",
        ] {
            assert!(is_blocked_ip(ip(blocked)), "{blocked} must be blocked");
        }
        for ok in ["1.1.1.1", "8.8.8.8", "93.184.216.34", "172.32.0.1", "100.128.0.1", "2606:4700:4700::1111", "::ffff:8.8.8.8"] {
            assert!(!is_blocked_ip(ip(ok)), "{ok} must be allowed");
        }
    }

    #[test]
    fn permission_matching() {
        let perms = vec!["network:domain:cdn.example.com".to_owned(), "parse:html".to_owned()];
        assert!(host_matches_permissions("cdn.example.com", &perms));
        assert!(host_matches_permissions("a.cdn.example.com", &perms));
        assert!(host_matches_permissions("CDN.Example.com.", &perms));
        assert!(!host_matches_permissions("evilcdn.example.com", &perms));
        assert!(!host_matches_permissions("example.com", &perms));
        assert!(host_matches_permissions("anything.test", &["network:domain:*".to_owned()]));
        assert!(!host_matches_permissions("anything.test", &["parse:html".to_owned()]));
    }

    #[test]
    fn header_sanitizing() {
        let p = Policy::link_source(false);
        let mut h = BTreeMap::new();
        h.insert("Referer".to_owned(), "https://a/".to_owned());
        h.insert("User-Agent".to_owned(), "x".to_owned());
        assert_eq!(sanitize_headers(&p, &h).unwrap().len(), 2);
        let mut bad = h.clone();
        bad.insert("Referer".to_owned(), "https://a/\r\nX-Evil: 1".to_owned());
        assert!(matches!(sanitize_headers(&p, &bad), Err(NetError::Header(_))));
        let mut bad = BTreeMap::new();
        bad.insert("Host".to_owned(), "evil".to_owned());
        assert!(sanitize_headers(&p, &bad).is_err(), "allow-list rejects Host");
        let mut bad = BTreeMap::new();
        bad.insert("X-A\r\nB".to_owned(), "v".to_owned());
        assert!(sanitize_headers(&p, &bad).is_err());
        let mut big = BTreeMap::new();
        big.insert("Referer".to_owned(), "a".repeat(9 * 1024));
        assert!(sanitize_headers(&p, &big).is_err());
    }

    struct MapResolver(HashMap<&'static str, Vec<IpAddr>>);

    #[async_trait]
    impl Resolver for MapResolver {
        async fn resolve(&self, host: &str, _port: u16) -> std::io::Result<Vec<IpAddr>> {
            self.0.get(host).cloned().ok_or_else(|| std::io::Error::other("nxdomain"))
        }
    }

    /// First answer public, every later answer private (classic rebinding).
    struct RebindingResolver(AtomicUsize);

    #[async_trait]
    impl Resolver for RebindingResolver {
        async fn resolve(&self, _host: &str, _port: u16) -> std::io::Result<Vec<IpAddr>> {
            Ok(if self.0.fetch_add(1, Ordering::SeqCst) == 0 { vec![ip("93.184.216.34")] } else { vec![ip("10.0.0.5")] })
        }
    }

    fn strict(resolver: Arc<dyn Resolver>) -> SafeHttp {
        SafeHttp::with_resolver(Policy::link_source(true), resolver)
    }

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[tokio::test]
    async fn rejects_private_literals_and_hostnames_resolving_to_private() {
        let mut m = HashMap::new();
        m.insert("intranet.test", vec![ip("192.168.1.10")]);
        m.insert("mixed.test", vec![ip("93.184.216.34"), ip("127.0.0.1")]);
        m.insert("good.test", vec![ip("93.184.216.34")]);
        let http = strict(Arc::new(MapResolver(m)));
        for target in [
            "http://127.0.0.1/x", "http://10.1.2.3/", "http://169.254.169.254/latest/meta-data", "http://[::1]/", "http://[fd00::1]/",
            "http://0.0.0.0/", "http://100.64.1.1/", "https://intranet.test/", "https://mixed.test/",
            "http://[::ffff:127.0.0.1]/",
        ] {
            let r = http.resolve_validated(&u(target)).await;
            assert!(matches!(r, Err(NetError::BlockedIp(_))), "{target}: {r:?}");
        }
        assert!(http.resolve_validated(&u("https://good.test/")).await.is_ok());
        assert!(matches!(http.resolve_validated(&u("https://missing.test/")).await, Err(NetError::Dns)));
    }

    #[tokio::test]
    async fn scheme_port_and_userinfo_checks() {
        let http = SafeHttp::with_resolver(Policy::link_source(false), Arc::new(MapResolver(HashMap::new())));
        assert!(matches!(http.check_url(&u("http://a.test/")), Err(NetError::Scheme)));
        assert!(matches!(http.check_url(&u("ftp://a.test/")), Err(NetError::Scheme)));
        assert!(matches!(http.check_url(&u("file:///etc/passwd")), Err(NetError::Scheme)));
        assert!(matches!(http.check_url(&u("https://a.test:22/")), Err(NetError::Port)));
        assert!(matches!(http.check_url(&u("https://a.test:6379/")), Err(NetError::Port)));
        assert!(matches!(http.check_url(&u("https://user:pw@a.test/")), Err(NetError::Host)));
        assert!(http.check_url(&u("https://a.test:8443/")).is_ok());
    }

    #[tokio::test]
    async fn rebinding_second_resolution_is_never_used() {
        let http = strict(Arc::new(RebindingResolver(AtomicUsize::new(0))));
        // first validation sees the public address ...
        assert!(http.resolve_validated(&u("https://rebind.test/")).await.is_ok());
        // ... the second resolution (a later hop / re-check) now answers private and is rejected
        assert!(matches!(http.resolve_validated(&u("https://rebind.test/")).await, Err(NetError::BlockedIp(_))));
    }

    /// Minimal HTTP/1.1 stub: replies per request with `responder(request_text)`.
    async fn stub(responder: impl Fn(&str) -> String + Send + Sync + 'static) -> (u16, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen: Arc<Mutex<Vec<String>>> = Arc::default();
        let seen2 = seen.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = listener.accept().await else { return };
                let mut buf = vec![0u8; 8192];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let resp = responder(&req);
                seen2.lock().unwrap().push(req);
                let _ = s.write_all(resp.as_bytes()).await;
            }
        });
        (port, seen)
    }

    fn lo_policy() -> Policy {
        Policy { allow_loopback: true, allowed_ports: None, ..Policy::link_source(true) }
    }

    fn ok(body: &str) -> String {
        format!("HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len())
    }

    fn redirect(to: &str) -> String {
        format!("HTTP/1.1 302 Found\r\nlocation: {to}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
    }

    #[tokio::test]
    async fn redirect_to_private_ip_is_rejected_after_the_first_hop() {
        for target in ["http://10.0.0.1/secret", "http://169.254.169.254/latest", "http://192.168.0.1:8080/", "http://[::1]:8080/"] {
            let t = target.to_owned();
            let (port, _) = stub(move |_| redirect(&t)).await;
            let http = SafeHttp::with_resolver(lo_policy(), Arc::new(MapResolver(HashMap::new())));
            let r = http.send(SafeRequest::get(u(&format!("http://127.0.0.1:{port}/start")), vec![])).await;
            // 10.x etc. are blocked IPs; the loopback IPv6 case is allowed by the test policy but nothing listens -> http error.
            if target.contains("[::1]") {
                assert!(r.is_err(), "{target}");
            } else {
                assert!(matches!(r, Err(NetError::BlockedIp(_))), "{target}: {:?}", r.err());
            }
        }
    }

    #[tokio::test]
    async fn redirect_loop_stops_at_five() {
        let (port, seen) = stub(move |_| redirect("/again")).await;
        let http = SafeHttp::with_resolver(lo_policy(), Arc::new(MapResolver(HashMap::new())));
        let r = http.send(SafeRequest::get(u(&format!("http://127.0.0.1:{port}/start")), vec![])).await;
        assert!(matches!(r, Err(NetError::TooManyRedirects)));
        assert_eq!(seen.lock().unwrap().len(), 6);
    }

    #[tokio::test]
    async fn cookie_and_authorization_are_dropped_on_cross_origin_redirect_but_kept_same_origin() {
        // origin B (the redirect target, different port => different origin) records what it receives
        let (port_b, seen_b) = stub(|_| ok("done")).await;
        let target = format!("http://127.0.0.1:{port_b}/landing");
        let (port_a, seen_a) = stub(move |req| if req.starts_with("GET /start") { redirect(&target) } else { ok("a") }).await;
        let http = SafeHttp::with_resolver(lo_policy(), Arc::new(MapResolver(HashMap::new())));
        let headers = vec![
            ("cookie".to_owned(), "sid=secret".to_owned()),
            ("authorization".to_owned(), "Bearer tok".to_owned()),
            ("referer".to_owned(), "https://ref.example/".to_owned()),
        ];
        let resp = http.send(SafeRequest::get(u(&format!("http://127.0.0.1:{port_a}/start")), headers)).await.unwrap();
        assert_eq!(resp.response.status().as_u16(), 200);
        let first = seen_a.lock().unwrap()[0].to_ascii_lowercase();
        assert!(first.contains("cookie: sid=secret") && first.contains("authorization: bearer tok"));
        let second = seen_b.lock().unwrap()[0].to_ascii_lowercase();
        assert!(!second.contains("cookie:"), "cookie leaked cross-origin: {second}");
        assert!(!second.contains("authorization:"), "authorization leaked cross-origin: {second}");
        assert!(second.contains("referer: https://ref.example/"), "non-credential headers survive");

        // same-origin redirect keeps credentials
        let (port_c, seen_c) = stub(|req| if req.starts_with("GET /start") { redirect("/next") } else { ok("c") }).await;
        http.send(SafeRequest::get(u(&format!("http://127.0.0.1:{port_c}/start")), vec![("cookie".into(), "k=v".into())])).await.unwrap();
        let reqs = seen_c.lock().unwrap().clone();
        assert!(reqs[1].starts_with("GET /next") && reqs[1].to_ascii_lowercase().contains("cookie: k=v"));
    }

    #[tokio::test]
    async fn read_capped_enforces_the_limit() {
        let (port, _) = stub(|_| ok(&"x".repeat(1000))).await;
        let http = SafeHttp::with_resolver(lo_policy(), Arc::new(MapResolver(HashMap::new())));
        let resp = http.send(SafeRequest::get(u(&format!("http://127.0.0.1:{port}/")), vec![])).await.unwrap();
        assert!(matches!(read_capped(resp.response, 999).await, Err(NetError::TooLarge)));
        let resp = http.send(SafeRequest::get(u(&format!("http://127.0.0.1:{port}/")), vec![])).await.unwrap();
        assert_eq!(read_capped(resp.response, 1000).await.unwrap().len(), 1000);
    }
}
