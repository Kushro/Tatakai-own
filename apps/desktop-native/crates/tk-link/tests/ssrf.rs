//! The SSRF table of review E6 against the loopback fetcher (the only thing FFmpeg ever talks to).

mod common;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use base64::Engine;
use common::*;
use tk_core::net::{Policy, Resolver, SafeHttp};
use tk_link::fetcher::{FetchError, Fetcher, SourceSpec};
use url::Url;

fn b64(u: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(u)
}

fn spec(url: &str) -> SourceSpec {
    SourceSpec { url: Url::parse(url).unwrap(), headers: vec![], extension_permissions: None }
}

async fn strict() -> Fetcher {
    Fetcher::start(SafeHttp::with_resolver(Policy::link_source(true), Arc::new(MapResolver(HashMap::new())))).await.unwrap()
}

#[tokio::test]
async fn private_loopback_linklocal_and_odd_targets_are_denied_before_any_connection() {
    let f = strict().await;
    for target in [
        "http://127.0.0.1/a.m3u8", "http://127.1.2.3:8080/a.m3u8", "http://10.0.0.5/a.m3u8", "http://172.16.0.1/a.m3u8", "http://172.31.255.254/a.m3u8",
        "http://192.168.1.1/a.m3u8", "http://169.254.169.254/latest/meta-data/", "http://100.64.0.1/a.m3u8", "http://100.100.100.200/a.m3u8", "http://0.0.0.0/a.m3u8",
        "http://224.0.0.1/a.m3u8", "http://[::1]/a.m3u8", "http://[fd00::1]/a.m3u8", "http://[fe80::1]/a.m3u8", "http://[::ffff:10.0.0.1]/a.m3u8",
        // scheme / port / userinfo
        "ftp://example.com/a.m3u8", "file:///etc/passwd", "gopher://example.com/", "https://example.com:22/a.m3u8", "https://example.com:6379/", "https://user:pw@example.com/a.m3u8",
    ] {
        let token = f.register(spec(target));
        let r = f.validate(&token, &Url::parse(target).unwrap()).await;
        assert_eq!(r, Err(FetchError::Denied), "{target}");
    }
}

#[tokio::test]
async fn names_resolving_to_private_addresses_are_denied_including_mixed_answers() {
    let mut m: HashMap<&'static str, Vec<std::net::IpAddr>> = HashMap::new();
    m.insert("intranet.test", vec!["192.168.0.10".parse().unwrap()]);
    m.insert("mixed.test", vec!["93.184.216.34".parse().unwrap(), "10.1.1.1".parse().unwrap()]);
    m.insert("public.test", vec!["93.184.216.34".parse().unwrap()]);
    let f = Fetcher::start(SafeHttp::with_resolver(Policy::link_source(true), Arc::new(MapResolver(m)))).await.unwrap();
    for (host, want) in [("intranet.test", Err(FetchError::Denied)), ("mixed.test", Err(FetchError::Denied)), ("public.test", Ok(())), ("nxdomain.test", Err(FetchError::Unavailable))] {
        let url = format!("https://{host}/a.m3u8");
        let token = f.register(spec(&url));
        assert_eq!(f.validate(&token, &Url::parse(&url).unwrap()).await, want, "{host}");
    }
}

/// DNS rebinding: the name resolves to a permitted address once and to a private one afterwards.
struct Rebind(AtomicUsize);

#[async_trait]
impl Resolver for Rebind {
    async fn resolve(&self, _host: &str, _port: u16) -> std::io::Result<Vec<std::net::IpAddr>> {
        Ok(if self.0.fetch_add(1, Ordering::SeqCst) == 0 { vec!["127.0.0.1".parse().unwrap()] } else { vec!["10.9.9.9".parse().unwrap()] })
    }
}

#[tokio::test]
async fn dns_rebinding_public_then_private_is_rejected() {
    let (port, seen) = stub(|_| text(200, &media_playlist(2))).await;
    let policy = Policy { allow_loopback: true, allowed_ports: None, ..Policy::link_source(true) };
    let f = Fetcher::start(SafeHttp::with_resolver(policy, Arc::new(Rebind(AtomicUsize::new(0))))).await.unwrap();
    let url = Url::parse(&format!("http://rebind.test:{port}/list.m3u8")).unwrap();
    let token = f.register(SourceSpec { url: url.clone(), headers: vec![], extension_permissions: None });
    // first resolution (validation): permitted
    f.validate(&token, &url).await.unwrap();
    // the connection is not trusted to a later resolution: the next lookup answers 10.9.9.9 and is refused, nothing is fetched
    assert_eq!(f.probe_hls(&token, &url).await, Err(FetchError::Denied));
    assert!(seen.lock().unwrap().is_empty(), "no request may reach any host after the rebinding answer");
    assert!(f.was_denied(&token));
}

async fn fetch_via(_f: &Fetcher, entry: &str) -> (u16, Vec<u8>) {
    let r = no_proxy_client().get(entry).send().await.unwrap();
    (r.status().as_u16(), r.bytes().await.unwrap().to_vec())
}

#[tokio::test]
async fn redirect_to_a_private_ip_is_refused_and_flagged() {
    let (port, _) = stub(|_| redirect("http://10.0.0.1/secret")).await;
    let f = Fetcher::start(lenient_default()).await.unwrap();
    let url = format!("http://127.0.0.1:{port}/seg.ts");
    let token = f.register(spec(&url));
    let (status, _) = fetch_via(&f, &f.entry_url(&token, &Url::parse(&url).unwrap(), false)).await;
    assert_eq!(status, 403);
    assert!(f.was_denied(&token));
    // ... and a redirect to the cloud metadata address
    let (port2, _) = stub(|_| redirect("http://169.254.169.254/latest/meta-data/iam")).await;
    let url2 = format!("http://127.0.0.1:{port2}/seg.ts");
    let token2 = f.register(spec(&url2));
    assert_eq!(fetch_via(&f, &f.entry_url(&token2, &Url::parse(&url2).unwrap(), false)).await.0, 403);
}

#[tokio::test]
async fn cookie_goes_only_to_the_initial_origin_and_is_stripped_on_cross_origin_redirects() {
    // origin B: where a segment lives and where a redirect lands
    let (port_b, seen_b) = stub(|_| (200, vec![], b"BBBB".to_vec())).await;
    let target_b = format!("http://127.0.0.1:{port_b}/seg0.ts");
    // origin A: serves the playlist, a segment, and a redirecting segment
    let tb = target_b.clone();
    let (port_a, seen_a) = stub(move |req| {
        let p = request_path(req);
        if p.starts_with("/list.m3u8") {
            text(200, &format!("#EXTM3U\n#EXTINF:2,\nseg-a.ts\n#EXTINF:2,\n{tb}\n#EXTINF:2,\nredir.ts\n#EXT-X-ENDLIST\n"))
        } else if p.starts_with("/redir.ts") {
            redirect(&tb)
        } else {
            (200, vec![], b"AAAA".to_vec())
        }
    })
    .await;
    let f = Fetcher::start(lenient_default()).await.unwrap();
    let top = format!("http://127.0.0.1:{port_a}/list.m3u8");
    let token = f.register(SourceSpec {
        url: Url::parse(&top).unwrap(),
        headers: vec![("cookie".into(), "sid=SECRET".into()), ("referer".into(), "https://ref.example/".into()), ("user-agent".into(), "UA/1".into())],
        extension_permissions: None,
    });
    let (status, body) = fetch_via(&f, &f.entry_url(&token, &Url::parse(&top).unwrap(), true)).await;
    assert_eq!(status, 200);
    let rewritten = String::from_utf8(body).unwrap();
    assert!(!rewritten.contains("127.0.0.1:"), "segment URIs must be loopback paths, got {rewritten}");
    let segs: Vec<String> = rewritten.lines().filter(|l| l.contains("/s/")).map(str::to_owned).collect();
    assert_eq!(segs.len(), 3);
    for s in &segs {
        let (st, _) = fetch_via(&f, &format!("http://{}{s}", f.addr())).await;
        assert_eq!(st, 200, "{s}");
    }
    let a: Vec<String> = seen_a.lock().unwrap().iter().map(|r| r.to_ascii_lowercase()).collect();
    for req in a.iter().filter(|r| r.starts_with("get /seg-a.ts") || r.starts_with("get /redir.ts") || r.starts_with("get /list.m3u8")) {
        assert!(req.contains("cookie: sid=secret"), "same-origin requests keep the cookie: {req}");
        assert!(req.contains("referer: https://ref.example/") && req.contains("user-agent: ua/1"));
    }
    let b: Vec<String> = seen_b.lock().unwrap().iter().map(|r| r.to_ascii_lowercase()).collect();
    assert_eq!(b.len(), 2, "segment fetched directly + via the redirect");
    for req in &b {
        assert!(!req.contains("cookie"), "cookie leaked to another origin: {req}");
        assert!(req.contains("referer: https://ref.example/"), "non-credential headers still travel");
    }
}

#[tokio::test]
async fn ext_x_key_with_absolute_uri_to_a_disallowed_host_is_rejected() {
    let key_uris = ["http://10.0.0.9/key.bin", "http://169.254.169.254/key", "http://[fd00::5]/key", "file:///C:/secret.key", "data:text/plain;base64,AAAA"];
    for uri in key_uris {
        let body = format!("#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"{uri}\"\n#EXTINF:2,\nseg0.ts\n#EXT-X-ENDLIST\n");
        let (port, _) = stub(move |_| text(200, &body)).await;
        let f = Fetcher::start(lenient_default()).await.unwrap();
        let url = format!("http://127.0.0.1:{port}/list.m3u8");
        let token = f.register(spec(&url));
        let (status, _) = fetch_via(&f, &f.entry_url(&token, &Url::parse(&url).unwrap(), true)).await;
        assert_eq!(status, 403, "{uri}");
        assert_eq!(f.probe_hls(&token, &Url::parse(&url).unwrap()).await, Err(FetchError::Denied), "{uri}");
    }
    // a key on a permitted host passes and is rewritten to go through the fetcher
    let (port, _) = stub(|_| text(200, "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"keys/k.bin\"\n#EXTINF:2,\nseg0.ts\n#EXT-X-ENDLIST\n")).await;
    let f = Fetcher::start(lenient_default()).await.unwrap();
    let url = format!("http://127.0.0.1:{port}/list.m3u8");
    let token = f.register(spec(&url));
    let (status, body) = fetch_via(&f, &f.entry_url(&token, &Url::parse(&url).unwrap(), true)).await;
    assert_eq!(status, 200);
    assert!(String::from_utf8(body).unwrap().contains(&format!("URI=\"/u/{token}/s/")));
}

#[tokio::test]
async fn extension_permissions_intersect_with_the_generic_policy() {
    let (port, _) = stub(|_| text(200, "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"http://other.test/k\"\n#EXTINF:2,\nseg0.ts\n#EXT-X-ENDLIST\n")).await;
    let mut m: HashMap<&'static str, Vec<std::net::IpAddr>> = HashMap::new();
    m.insert("other.test", vec!["127.0.0.1".parse().unwrap()]);
    let f = Fetcher::start(lenient_http(Arc::new(MapResolver(m)))).await.unwrap();
    let url = Url::parse(&format!("http://127.0.0.1:{port}/list.m3u8")).unwrap();
    // generic policy only: other.test is fine
    let t1 = f.register(SourceSpec { url: url.clone(), headers: vec![], extension_permissions: None });
    assert!(f.probe_hls(&t1, &url).await.is_ok());
    // extension installed with a narrower network:domain list: other.test is outside it
    let t2 = f.register(SourceSpec { url: url.clone(), headers: vec![], extension_permissions: Some(vec!["network:domain:127.0.0.1".into()]) });
    assert_eq!(f.probe_hls(&t2, &url).await, Err(FetchError::Denied));
    let t3 = f.register(SourceSpec { url: url.clone(), headers: vec![], extension_permissions: Some(vec!["network:domain:127.0.0.1".into(), "network:domain:other.test".into()]) });
    assert!(f.probe_hls(&t3, &url).await.is_ok());
}

#[tokio::test]
async fn playlists_with_forbidden_schemes_nesting_or_excess_size_are_denied() {
    let cases: Vec<(&str, String)> = vec![
        ("data: segment", "#EXTM3U\n#EXTINF:2,\ndata:video/mp2t;base64,AAAA\n".into()),
        ("file: segment", "#EXTM3U\n#EXTINF:2,\nfile:///C:/Windows/win.ini\n".into()),
        ("file: map", "#EXTM3U\n#EXT-X-MAP:URI=\"file:///C:/x.mp4\"\n#EXTINF:2,\na.ts\n".into()),
        ("master inside master", "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nsub.m3u8\n".into()),
        ("playlist > 2 MiB", format!("#EXTM3U\n{}", "#EXTINF:1,\nsegment-name-long-enough.ts\n".repeat(80_000))),
    ];
    for (name, body) in cases {
        let (port, _) = stub(move |req| {
            if request_path(req).starts_with("/sub.m3u8") {
                text(200, "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nnested.m3u8\n") // a master reached as a variant: nesting
            } else {
                text(200, &body)
            }
        })
        .await;
        let f = Fetcher::start(lenient_default()).await.unwrap();
        let url = format!("http://127.0.0.1:{port}/list.m3u8");
        let token = f.register(spec(&url));
        let r = f.probe_hls(&token, &Url::parse(&url).unwrap()).await;
        assert_eq!(r, Err(FetchError::Denied), "{name}");
    }
}

#[tokio::test]
async fn entry_urls_are_loopback_only_and_unknown_tokens_are_404() {
    let f = strict().await;
    let token = f.register(spec("https://cdn.example.com/a.m3u8"));
    let e = f.entry_url(&token, &Url::parse("https://cdn.example.com/a.m3u8").unwrap(), true);
    assert!(e.starts_with("http://127.0.0.1:"));
    assert!(!e.contains("cdn.example.com"), "FFmpeg never sees the external URL");
    let (st, _) = fetch_via(&f, &format!("http://{}/u/not-a-token/s/{}", f.addr(), b64("https://x.example/"))).await;
    assert_eq!(st, 404);
    f.unregister(&token);
    let (st, _) = fetch_via(&f, &e).await;
    assert_eq!(st, 404);
    // POST is not served
    let r = no_proxy_client().post(&e).send().await.unwrap();
    assert_eq!(r.status(), 405);
}
