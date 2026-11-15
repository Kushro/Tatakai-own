//! Runtime behaviour: shims, limits, network gate. Extensions are built in-memory (zip) into tempdirs.

use std::io::Write;
use std::time::Duration;

use serde_json::{Value, json};
use tk_ext::{ExtError, ExtensionHost};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn kai(id: &str, perms: &[&str], bundle: &str) -> Vec<u8> {
    let manifest = json!({ "id": id, "name": id, "version": "1.0.0", "type": "onlinestream", "description": "t", "permissions": perms });
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data) in [("manifest.json", manifest.to_string()), ("bundle.js", bundle.to_owned())] {
        w.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(data.as_bytes()).unwrap();
    }
    w.finish().unwrap().into_inner()
}

struct Ctx {
    _dir: tempfile::TempDir,
    host: ExtensionHost,
}

fn install(id: &str, perms: &[&str], bundle: &str, host_cfg: impl FnOnce(ExtensionHost) -> ExtensionHost) -> Ctx {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(format!("{id}.kai"));
    std::fs::write(&file, kai(id, perms, bundle)).unwrap();
    let host = host_cfg(ExtensionHost::open(dir.path().join("ext")).unwrap());
    host.install_kai(&file).unwrap();
    Ctx { _dir: dir, host }
}

async fn run(c: &Ctx, id: &str, method: &str, args: Value) -> Result<Value, ExtError> {
    c.host.invoke(id, method, args).await
}

const SHIMS: &str = r#"
const crypto = require('crypto');
const EventEmitter = require('events');
const path = require('path');
const util = require('util');
const qs = require('querystring');
module.exports = class {
  async run() { try { return await this.run2(); } catch (e) { return { error: e.message, stack: e.stack }; } }
  async run2() {
    const out = {};
    out.sha256 = crypto.createHash('sha256').update('abc').digest('hex');
    out.md5 = crypto.createHash('md5').update(Buffer.from('abc')).digest('hex');
    out.hmac = crypto.createHmac('sha256', 'key').update('The quick brown fox jumps over the lazy dog').digest('hex');
    out.b64 = Buffer.from('héllo wörld', 'utf8').toString('base64');
    out.fromB64 = Buffer.from(out.b64, 'base64').toString('utf8');
    out.hex = Buffer.from([1, 2, 255]).toString('hex');
    out.btoa = btoa('hello'); out.atob = atob('aGVsbG8=');
    out.enc = Array.from(new TextEncoder().encode('é'));
    out.dec = new TextDecoder().decode(new Uint8Array([0xc3, 0xa9]));
    const u = new URL('../x?y=1&z=a%20b#frag', 'https://User@Example.com:443/c/d/e');
    out.url = [u.href, u.origin, u.host, u.pathname, u.search, u.hash, u.searchParams.get('z'), u.username];
    const sp = new URLSearchParams({ a: '1' }); sp.append('b', 'x y'); out.sp = sp.toString();
    const e = new EventEmitter(); let n = 0; e.on('x', (v) => { n += v; }); e.once('x', (v) => { n += v * 10; }); e.emit('x', 1); e.emit('x', 1); out.events = n;
    out.path = [path.join('/a', '../b', 'c.txt'), path.basename('/x/y.js', '.js'), path.extname('f.tar.gz'), path.dirname('/a/b/c')];
    out.util = util.format('%s=%d %j', 'k', 42, { a: 1 });
    out.qs = qs.parse('a=1&a=2&b=%20');
    out.uuid = crypto.randomUUID().length;
    out.rand = crypto.getRandomValues(new Uint8Array(8)).length;
    out.clone = structuredClone({ d: [1, { e: 2 }] });
    out.proc = [process.platform, typeof process.nextTick, process.versions.node];
    let order = [];
    await new Promise((res) => { setTimeout(() => { order.push('t10'); res(); }, 10); setTimeout(() => order.push('t0'), 0); order.push('sync'); });
    out.order = order;
    let ticks = 0; const iv = setInterval(() => { ticks++; }, 5); await new Promise((r) => setTimeout(r, 40)); clearInterval(iv); out.interval = ticks >= 3;
    return out;
  }
  async node() {
    const out = {};
    for (const m of ['fs', 'child_process', 'net', 'undici', './local']) {
      try { require(m); out[m] = 'loaded'; } catch (e) { out[m] = e.message; }
    }
    const http = require('http');
    out.httpLoads = typeof http.get;
    try { http.get('x'); } catch (e) { out.httpCall = e.message; }
    return out;
  }
  async html() {
    const $ = __tatakai_parse_html__('<html><body><div class="a" id="d"><p>hi <b>there</b></p><p data-x="7">yo</p></div><span>s</span></body></html>');
    const ps = $.find('div.a p');
    let seen = [];
    ps.each((i, el) => seen.push(i + ':' + el.text()));
    return {
      len: ps.length, text: ps.text(), attr: $.find('p').eq(1).attr('data-x'), missing: $.find('p').first().attr('nope'),
      html: $.find('div').html(), parentTag: $.find('b').parent().text(), next: $.find('div').next().text(), prev: $.find('span').prev().attr('id'),
      children: $.find('div').children('p').length, last: $.find('p').last().text(), seen,
      bad: (() => { try { __tatakai_parse_html__(''); return 'no'; } catch (e) { return e.name; } })(),
    };
  }
}
"#;

#[tokio::test]
async fn shims_behave_like_node() {
    let c = install("shims", &[], SHIMS, |h| h);
    let r = run(&c, "shims", "run", json!([])).await.unwrap();
    assert!(r.get("error").is_none(), "shim bundle failed: {r}");
    assert_eq!(r["sha256"], "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    assert_eq!(r["md5"], "900150983cd24fb0d6963f7d28e17f72");
    assert_eq!(r["hmac"], "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8");
    assert_eq!(r["b64"], "aMOpbGxvIHfDtnJsZA==");
    assert_eq!(r["fromB64"], "héllo wörld");
    assert_eq!(r["hex"], "0102ff");
    assert_eq!((r["btoa"].as_str(), r["atob"].as_str()), (Some("aGVsbG8="), Some("hello")));
    assert_eq!(r["enc"], json!([195, 169]));
    assert_eq!(r["dec"], "é");
    assert_eq!(r["url"], json!(["https://User@example.com/c/x?y=1&z=a%20b#frag", "https://example.com", "example.com", "/c/x", "?y=1&z=a%20b", "#frag", "a b", "User"]));
    assert_eq!(r["sp"], "a=1&b=x+y");
    assert_eq!(r["events"], 12);
    assert_eq!(r["path"], json!(["/b/c.txt", "y", ".gz", "/a/b"]));
    assert_eq!(r["util"], "k=42 {\"a\":1}");
    assert_eq!(r["qs"], json!({ "a": ["1", "2"], "b": " " }));
    assert_eq!(r["uuid"], 36);
    assert_eq!(r["rand"], 8);
    assert_eq!(r["clone"], json!({ "d": [1, { "e": 2 }] }));
    assert_eq!(r["proc"], json!(["win32", "function", "20.11.0"]));
    assert_eq!(r["order"], json!(["sync", "t0", "t10"]));
    assert_eq!(r["interval"], true);
}

#[tokio::test]
async fn require_tiers() {
    let c = install("tiers", &[], SHIMS, |h| h);
    let r = run(&c, "tiers", "node", json!([])).await.unwrap();
    for m in ["fs", "child_process", "net", "undici", "./local"] {
        assert_eq!(r[m], format!("Module '{m}' is not supported by the native extension runtime"), "{m}");
    }
    assert_eq!(r["httpLoads"], "function");
    assert_eq!(r["httpCall"], "http.get is not available in the native extension runtime");
    // 'http' is a stub module and require(m) is dynamic -> partial (installed and enabled)
    let info = c.host.get("tiers").unwrap();
    assert_eq!(info.compat, tk_core::Compat::Partial);
    assert!(info.enabled);
}

#[tokio::test]
async fn cheerio_like_html_adapter() {
    let c = install("html", &[], SHIMS, |h| h);
    let r = run(&c, "html", "html", json!([])).await.unwrap();
    assert_eq!(r["len"], 2);
    assert_eq!(r["text"], "hi thereyo");
    assert_eq!(r["attr"], "7");
    assert_eq!(r["missing"], Value::Null);
    assert_eq!(r["html"], "<p>hi <b>there</b></p><p data-x=\"7\">yo</p>");
    assert_eq!(r["parentTag"], "hi there");
    assert_eq!(r["next"], "s");
    assert_eq!(r["prev"], "d");
    assert_eq!(r["children"], 2);
    assert_eq!(r["last"], "yo");
    assert_eq!(r["seen"], json!(["0:hi there", "1:yo"]));
    assert_eq!(r["bad"], "InvalidInputError");
}

#[tokio::test]
async fn infinite_loop_times_out_and_the_runtime_recovers() {
    let bundle = "module.exports = class { spin() { while (true) {} } ok() { return 'fine'; } never() { return new Promise(() => {}); } };";
    let c = install("spin", &[], bundle, |h| h.with_call_timeout(Duration::from_millis(400)));
    assert_eq!(run(&c, "spin", "spin", json!([])).await.unwrap_err(), ExtError::Timeout);
    // the runtime was discarded and is rebuilt
    assert_eq!(run(&c, "spin", "ok", json!([])).await.unwrap(), "fine");
    // a promise that can never settle is reported instead of hanging
    assert!(matches!(run(&c, "spin", "never", json!([])).await, Err(ExtError::Js(_))));
}

#[tokio::test]
async fn memory_limit_is_enforced() {
    let bundle = "module.exports = class { hog() { const a = []; for (;;) a.push(new Array(100000).fill(1)); } ok() { return 1; } };";
    let c = install("hog", &[], bundle, |h| h.with_call_timeout(Duration::from_secs(20)));
    assert_eq!(run(&c, "hog", "hog", json!([])).await.unwrap_err(), ExtError::OutOfMemory);
    assert_eq!(run(&c, "hog", "ok", json!([])).await.unwrap(), 1);
}

#[tokio::test]
async fn bundle_errors_are_reported_and_console_is_captured() {
    let bundle = "module.exports = class { boom() { console.log('before', {a:1}); throw new Error('kaboom'); } };";
    let c = install("boom", &[], bundle, |h| h);
    assert_eq!(run(&c, "boom", "boom", json!([])).await.unwrap_err(), ExtError::Js("kaboom".into()));
    assert_eq!(c.host.logs("boom"), vec!["[log] before {\"a\":1}".to_owned()]);
    assert!(matches!(run(&c, "boom", "missing", json!([])).await, Err(ExtError::NotImplemented(_))));
}

#[tokio::test]
async fn bundle_without_usable_export_fails_on_load() {
    let c = install("empty", &[], "module.exports = 42;", |h| h);
    assert!(matches!(run(&c, "empty", "x", json!([])).await, Err(ExtError::Js(m)) if m.contains("no usable export")));
}

/// Loopback stub: GET /echo?... -> JSON, answers in the order of arrival after a short delay.
async fn stub() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = listener.accept().await else { return };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let first = req.lines().next().unwrap_or("").to_owned();
                tokio::time::sleep(Duration::from_millis(150)).await;
                let body = json!({ "line": first, "ua": req.to_ascii_lowercase().contains("x-test: yes") }).to_string();
                let resp = format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nx-answer: 42\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
                let _ = s.write_all(resp.as_bytes()).await;
            });
        }
    });
    port
}

#[tokio::test]
async fn fetch_reaches_permitted_hosts_in_parallel() {
    let port = stub().await;
    let bundle = r#"module.exports = class {
      async go(port) {
        const t0 = Date.now();
        const rs = await Promise.all([1, 2, 3, 4].map((i) => fetch('http://127.0.0.1:' + port + '/echo?i=' + i, { headers: { 'X-Test': 'yes' } })));
        const bodies = await Promise.all(rs.map((r) => r.json()));
        return { ms: Date.now() - t0, ok: rs.map((r) => r.ok), status: rs[0].status, hdr: rs[0].headers.get('x-answer'), lines: bodies.map((b) => b.line), ua: bodies[0].ua, url: rs[0].url };
      }
      async denied(port) {
        try { await fetch('http://localhost:' + port + '/x'); return 'allowed'; } catch (e) { return e.message; }
      }
    };"#;
    let c = install("net", &["network:domain:127.0.0.1"], bundle, |h| h.with_loopback_for_tests());
    let r = run(&c, "net", "go", json!([port])).await.unwrap();
    assert_eq!(r["ok"], json!([true, true, true, true]));
    assert_eq!(r["status"], 200);
    assert_eq!(r["hdr"], "42");
    assert_eq!(r["ua"], true);
    assert_eq!(r["lines"][2], format!("GET /echo?i=3 HTTP/1.1"));
    // four 150 ms requests overlap instead of running back to back (generous bound for slow CI machines)
    assert!(r["ms"].as_u64().unwrap() < 550, "fetches ran serially: {}ms", r["ms"]);
    // 'localhost' is not covered by network:domain:127.0.0.1
    assert_eq!(run(&c, "net", "denied", json!([port])).await.unwrap(), "Network permission denied");
}

#[tokio::test]
async fn fetch_to_private_addresses_is_rejected_even_when_permitted() {
    let bundle = r#"module.exports = class { async go(u) { try { await fetch(u); return 'allowed'; } catch (e) { return e.message; } } };"#;
    let c = install("ssrf", &["network:domain:*"], bundle, |h| h);
    for u in ["http://127.0.0.1:9/", "http://10.0.0.1/", "http://169.254.169.254/latest/meta-data/", "http://[::1]/", "http://192.168.1.1/"] {
        let r = run(&c, "ssrf", "go", json!([u])).await.unwrap();
        assert!(r.as_str().unwrap().contains("address not allowed"), "{u}: {r}");
    }
}

/// BUG-121: work a call leaves running (a provider the bundle gave up on) is dropped once its answer is known; it
/// no longer runs inside the next calls of the single JS thread.
#[tokio::test]
async fn work_left_running_by_a_call_is_dropped_when_it_answers() {
    let bundle = r#"module.exports = class {
      constructor() { this.late = 0; }
      quick() { setTimeout(() => { this.late++; }, 30); return 'ok'; }
      async wait() { await new Promise((r) => setTimeout(r, 150)); return this.late; }
    };"#;
    let c = install("left", &[], bundle, |h| h);
    assert_eq!(run(&c, "left", "quick", json!([])).await.unwrap(), "ok");
    // Before: the 30 ms timer of `quick` fired during `wait`, which saw 1.
    assert_eq!(run(&c, "left", "wait", json!([])).await.unwrap(), 0);
}

/// BUG-121: a queued call whose caller already gave up (timeout) is not run; the calls behind it start sooner.
#[tokio::test]
async fn queued_calls_nobody_waits_for_are_skipped() {
    let bundle = r#"module.exports = class {
      constructor() { this.started = 0; }
      async slow() { this.started++; await new Promise((r) => setTimeout(r, 300)); return 'done'; }
      count() { return this.started; }
    };"#;
    let c = install("queue", &[], bundle, |h| h);
    let calls = (0..3).map(|_| tokio::time::timeout(Duration::from_millis(50), run(&c, "queue", "slow", json!([]))));
    for r in futures_util::future::join_all(calls).await {
        assert!(r.is_err(), "every caller timed out");
    }
    let t0 = std::time::Instant::now();
    assert_eq!(run(&c, "queue", "count", json!([])).await.unwrap(), 1, "only the call already running was run");
    assert!(t0.elapsed() < Duration::from_millis(600), "count waited only for the running call: {:?}", t0.elapsed());
}

fn anime() -> tk_core::Media {
    tk_core::Media { format: None,
        media_ref: tk_core::MediaRef::anilist(1),
        kind: tk_core::MediaKind::Anime,
        title: "Synthetic".into(),
        titles: Default::default(),
        cover: None,
        banner: None,
        color: None,
        synopsis: None,
        genres: vec![],
        episodes: None,
        status: None,
        season: None,
        year: None,
        score: None,
        source_provider: "test".into(),
    }
}

/// BUG-102: an empty source listing is never cached by the host (the next ask, "Retry", reaches the bundle again),
/// `refresh=1` (fresh listings) skips the cache, and a listing where a provider timed out is reported as partial.
#[tokio::test]
async fn empty_listings_are_not_cached_and_fresh_ones_skip_the_cache() {
    let bundle = r#"module.exports = class {
      constructor() { this.n = 0; this.mode = 'empty'; }
      setMode(m) { this.mode = m; return m; }
      calls() { return this.n; }
      async streamAll(opts, emit) {
        this.n++;
        if (this.mode === 'empty') { emit({ provider: 'p', results: [], diagnostic: { provider: 'p', status: 'empty' } }); return; }
        emit({ provider: 'p', results: [{ url: 'https://a.test/v.m3u8', quality: '1080p' }], diagnostic: { provider: 'p', status: 'ok' } });
        if (this.mode === 'partial') emit({ provider: 'q', results: [], diagnostic: { provider: 'q', status: 'timeout' } });
      }
    };"#;
    let c = install("cache", &[], bundle, |h| h);
    let params = json!({ "anilistId": 1, "titles": ["Synthetic"], "episode": 3 });
    let c = &c;
    let calls = || async move { run(c, "cache", "calls", json!([])).await.unwrap() };
    let stream = |extra: Value| {
        let mut p = params.clone();
        if let (Some(o), Some(e)) = (p.as_object_mut(), extra.as_object()) {
            o.extend(e.clone());
        }
        async move { c.host.call("cache", "stream", p).await.unwrap() }
    };

    // Empty answers (network or proxy failure) are asked again every time.
    assert_eq!(stream(json!({})).await["count"], 0);
    assert_eq!(stream(json!({})).await["count"], 0);
    assert_eq!(calls().await, 2, "the empty listing was not cached");

    // A complete listing is cached; refresh=1 skips that cache.
    run(&c, "cache", "setMode", json!(["ok"])).await.unwrap();
    assert_eq!(stream(json!({})).await["count"], 1);
    let again = stream(json!({})).await;
    assert_eq!((again["count"].clone(), again["cached"].clone()), (json!(1), json!(true)));
    assert_eq!(calls().await, 3);
    assert_eq!(stream(json!({ "refresh": "1" })).await["cached"], false);
    assert_eq!(calls().await, 4);

    // Through the host API: fresh listings skip the bundle cache, and a provider timeout marks the listing partial.
    let l = c.host.list_candidates(&anime(), 3, true).await;
    assert_eq!((l.candidates.len(), l.partial), (1, false));
    assert_eq!(calls().await, 5);
    run(&c, "cache", "setMode", json!(["partial"])).await.unwrap();
    let l = c.host.list_candidates(&anime(), 3, true).await;
    assert_eq!((l.candidates.len(), l.partial), (1, true));
    // After the outbound route changed, the next call runs on a fresh runtime (no cached listings, no old work).
    c.host.restart_workers();
    assert_eq!(calls().await, 0);
}
