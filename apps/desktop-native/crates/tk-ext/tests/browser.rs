//! Browser runtime bridge (`configureBrowserRuntime`, docs/redesign/09 section 7) over a mock [`BrowserRuntime`].

use std::io::Write;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{Value, json};
use tk_ext::{BrowserError, BrowserId, BrowserRuntime, ExtensionHost};

#[derive(Default)]
struct Mock {
    log: Mutex<Vec<String>>,
    seq: Mutex<u32>,
}

impl Mock {
    fn log(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }
    fn push(&self, s: String) {
        self.log.lock().unwrap().push(s);
    }
}

#[async_trait]
impl BrowserRuntime for Mock {
    async fn new_browser(&self, ext_id: &str, allowed_domains: &[String], opts: Value) -> Result<BrowserId, BrowserError> {
        let mut n = self.seq.lock().unwrap();
        *n += 1;
        self.push(format!("new {ext_id} {allowed_domains:?} {}", opts["timeout"]));
        Ok(format!("b{n}"))
    }

    async fn navigate(&self, id: &BrowserId, url: &str) -> Result<(), BrowserError> {
        self.push(format!("navigate {id} {url}"));
        Ok(())
    }

    async fn evaluate(&self, id: &BrowserId, js: &str) -> Result<Value, BrowserError> {
        self.push(format!("evaluate {id} {js}"));
        if js == "big" {
            return Ok(Value::String("x".repeat(2_100_000)));
        }
        Ok(json!({ "url": "https://www.example.com/a", "ua": "Mock UA" }))
    }

    async fn cookies(&self, id: &BrowserId, urls: &[String]) -> Result<Vec<Value>, BrowserError> {
        self.push(format!("cookies {id} {urls:?}"));
        Ok(vec![json!({ "name": "cf_clearance", "value": "ok", "domain": ".example.com", "path": "/", "expires": -1 })])
    }

    async fn close(&self, id: &BrowserId) -> Result<(), BrowserError> {
        self.push(format!("close {id}"));
        Ok(())
    }
}

fn kai(id: &str, browser_domains: &[&str], bundle: &str) -> Vec<u8> {
    let manifest = json!({
        "id": id, "name": id, "version": "1.0.0", "type": "onlinestream", "description": "t",
        "permissions": ["network:domain:example.com"], "browserDomains": browser_domains,
    });
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, data) in [("manifest.json", manifest.to_string()), ("bundle.js", bundle.to_owned())] {
        w.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(data.as_bytes()).unwrap();
    }
    w.finish().unwrap().into_inner()
}

const BUNDLE: &str = r#"
module.exports = class {
  configureBrowserRuntime(rt) { this.rt = rt; }
  async solve() {
    if (!this.rt) return { configured: false };
    const b = await this.rt.newBrowser({ headless: true, timeout: 5 });
    await b.navigate('https://www.example.com/a');
    const page = await b.evaluate('({url: location.href, ua: navigator.userAgent})');
    const { cookies } = await b.executeCDP('Network.getCookies', { urls: ['https://www.example.com/a'] });
    await b.sleep(10);
    const errs = [];
    const bad = [
      () => b.navigate('https://evil.test/'),
      () => b.navigate('http://example.com/'),
      () => b.executeCDP('Runtime.evaluate', {}),
      () => b.executeCDP('Network.getCookies', { urls: ['https://evil.test/'] }),
      () => b.executeCDP('Network.getCookies', { urls: [] }),
      () => b.sleep(20000),
      () => b.evaluate('x'.repeat(40000)),
      () => b.evaluate('big'),
    ];
    for (const f of bad) { try { await f(); errs.push('no error'); } catch (e) { errs.push(e.message); } }
    const b2 = await this.rt.newBrowser({});
    let limit = 'no error';
    try { await this.rt.newBrowser({}); } catch (e) { limit = e.message; }
    await b.close();
    await b2.close();
    let closed = 'no error';
    try { await b.navigate('https://example.com/'); } catch (e) { closed = e.message; }
    return { configured: true, page, cookies, errs, limit, closed };
  }
};
"#;

fn host_with(id: &str, browser_domains: &[&str], runtime: Option<Arc<Mock>>) -> (tempfile::TempDir, ExtensionHost) {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join(format!("{id}.kai"));
    std::fs::write(&file, kai(id, browser_domains, BUNDLE)).unwrap();
    let mut host = ExtensionHost::open(dir.path().join("ext")).unwrap();
    if let Some(rt) = runtime {
        host = host.with_browser_runtime(rt);
    }
    host.install_kai(&file).unwrap();
    (dir, host)
}

#[tokio::test]
async fn bundle_drives_the_browser_runtime_with_legacy_checks() {
    let mock = Arc::new(Mock::default());
    let (_d, host) = host_with("cfx", &["example.com", "evil.test"], Some(mock.clone()));
    let out = host.invoke("cfx", "solve", json!([])).await.unwrap();
    assert_eq!(out["configured"], true, "{out}");
    assert_eq!(out["page"]["ua"], "Mock UA");
    assert_eq!(out["cookies"][0]["name"], "cf_clearance");
    assert_eq!(
        out["errs"],
        json!([
            "ChromeDP URL is outside the provider website",
            "ChromeDP URL is outside the provider website",
            "Unsupported ChromeDP CDP command: Runtime.evaluate",
            "ChromeDP URL is outside the provider website",
            "ChromeDP cookie URLs are required",
            "Invalid ChromeDP sleep duration",
            "Invalid ChromeDP expression",
            "ChromeDP response is too large",
        ])
    );
    assert_eq!(out["limit"], "ChromeDP browser limit reached");
    assert_eq!(out["closed"], "ChromeDP browser is closed");

    let log = mock.log();
    // evil.test has no network permission, so it is never offered to the runtime; blocked URLs never reach it
    assert_eq!(log[0], r#"new cfx ["example.com"] 5"#);
    assert!(log.iter().all(|l| !l.contains("evil.test")), "{log:?}");
    assert!(log.contains(&"close b1".to_owned()) && log.contains(&"close b2".to_owned()), "{log:?}");
}

#[tokio::test]
async fn no_runtime_without_permitted_browser_domains() {
    let mock = Arc::new(Mock::default());
    let (_d, host) = host_with("plain", &[], Some(mock.clone()));
    assert_eq!(host.invoke("plain", "solve", json!([])).await.unwrap()["configured"], false);
    let (_d2, host2) = host_with("noruntime", &["example.com"], None);
    assert_eq!(host2.invoke("noruntime", "solve", json!([])).await.unwrap()["configured"], false);
    assert!(mock.log().is_empty());
}
