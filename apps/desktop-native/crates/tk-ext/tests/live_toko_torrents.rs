//! Live diagnostic (network, the user's Toko build): how long each Toko torrent provider takes inside the native
//! runtime and how many results it returns. Ignored by default; run with
//! `TOKO_KAI=/path/to/toko.kai cargo test -p tk-ext --test live_toko_torrents -- --ignored --nocapture`.

use std::time::Instant;

use std::sync::Arc;

use serde_json::{Value, json};
use tk_ext::{BrowserError, BrowserId, BrowserRuntime, ExtensionHost};

/// Inert browser runtime: the app always attaches one (`with_browser_runtime`), the plain test did not.
struct NoBrowser;

#[async_trait::async_trait]
impl BrowserRuntime for NoBrowser {
    async fn new_browser(&self, _ext_id: &str, _allowed: &[String], _opts: Value) -> Result<BrowserId, BrowserError> {
        Err(BrowserError::Unavailable)
    }
    async fn navigate(&self, _id: &BrowserId, _url: &str) -> Result<(), BrowserError> {
        Err(BrowserError::Unavailable)
    }
    async fn evaluate(&self, _id: &BrowserId, _js: &str) -> Result<Value, BrowserError> {
        Err(BrowserError::Unavailable)
    }
    async fn cookies(&self, _id: &BrowserId, _urls: &[String]) -> Result<Vec<Value>, BrowserError> {
        Ok(Vec::new())
    }
    async fn close(&self, _id: &BrowserId) -> Result<(), BrowserError> {
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "network + a local toko.kai"]
async fn toko_torrent_providers_in_the_native_runtime() {
    let kai = std::env::var("TOKO_KAI").unwrap_or_default();
    let dir = tempfile::tempdir().unwrap();
    // TOKO_STORE=<a copy of an app's extensions folder>: use that installation as is (no install).
    let store = std::env::var("TOKO_STORE").ok();
    let mut host = ExtensionHost::open(store.clone().map(std::path::PathBuf::from).unwrap_or_else(|| dir.path().to_path_buf())).unwrap();
    if std::env::var("TOKO_BROWSER").is_ok() {
        host = host.with_browser_runtime(Arc::new(NoBrowser));
    }
    let ext = if store.is_some() { host.get("tatakai.extension.toko").expect("toko in the store") } else { host.install_kai(std::path::Path::new(&kai)).unwrap() };
    let opts = json!({
        "anilistId": 169581,
        "titles": serde_json::from_str::<serde_json::Value>(&std::env::var("TOKO_TITLES").unwrap_or_else(|_| r#"["Magical Explorer", "Magical★Explorer: Eroge no Yuujin Chara ni Tensei Shitakedo, Game Chishiki Tsukatte Jiyuu ni Ikiru"]"#.to_owned())).unwrap(),
        "episode": std::env::var("TOKO_EP").ok().and_then(|e| e.parse::<u32>().ok()).unwrap_or(6),
        "stream": "0",
        "resolution": "1080p",
        "providerOptions": { "timeoutMs": 20_000 },
    });
    for name in ["nyaa", "nekobt", "subplease", "animetosho", "aniliberty", "seadex"] {
        let t0 = Instant::now();
        let r = host.invoke(&ext.id, "runProvider", json!([name, opts])).await;
        let ms = t0.elapsed().as_millis();
        match r {
            Ok(v) => {
                let n = v["results"].as_array().map_or(0, Vec::len);
                println!("{name:>10}: {n:>3} results in {ms:>6} ms  error={}", v["error"]);
            }
            Err(e) => println!("{name:>10}: ERROR after {ms} ms: {e}"),
        }
    }
    // The exact route call `resolve_candidates` makes, then the same mapping into picker candidates.
    let t0 = Instant::now();
    let body = host.call(&ext.id, "torrent", opts.clone()).await.expect("torrent route");
    let raw = body["sources"].as_array().map_or(0, Vec::len);
    let mut by: std::collections::BTreeMap<String, usize> = Default::default();
    for s in body["sources"].as_array().into_iter().flatten() {
        *by.entry(s["source"].as_str().or_else(|| s["provider"].as_str()).unwrap_or("?").to_owned()).or_default() += 1;
    }
    let mapped = tk_ext::candidates::map_candidates(&ext.id, "Toko", &body);
    println!("torrent route: {raw} raw {by:?} -> {} candidates in {} ms", mapped.len(), t0.elapsed().as_millis());
    if let Some(first) = body["sources"].as_array().and_then(|l| l.iter().find(|s| s["source"] == "nyaa")) {
        println!("first nyaa raw: {}", serde_json::to_string(first).unwrap().chars().take(600).collect::<String>());
    }
    // Like the app: `stream` and `torrent` at the same time on the same (single-threaded) worker.
    let t0 = Instant::now();
    let (st, tr) = tokio::join!(
        async { let r = host.call(&ext.id, "stream", opts.clone()).await; (r, t0.elapsed().as_millis()) },
        async { let r = host.call(&ext.id, "torrent", opts.clone()).await; (r, t0.elapsed().as_millis()) },
    );
    let count = |r: &Result<serde_json::Value, tk_ext::ExtError>| -> String {
        match r {
            Ok(v) => {
                let mut by: std::collections::BTreeMap<String, usize> = Default::default();
                for s in v["sources"].as_array().into_iter().flatten() {
                    *by.entry(s["source"].as_str().or_else(|| s["provider"].as_str()).unwrap_or("?").to_owned()).or_default() += 1;
                }
                format!("{by:?}")
            }
            Err(e) => format!("ERR {e}"),
        }
    };
    println!("concurrent: stream {} in {} ms | torrent {} in {} ms", count(&st.0), st.1, count(&tr.0), tr.1);
    // Two `torrent` calls at once on the same worker (the app can ask twice for one episode).
    let (a, b) = tokio::join!(host.call(&ext.id, "torrent", opts.clone()), host.call(&ext.id, "torrent", opts.clone()));
    println!("double torrent: a {} | b {}", count(&a), count(&b));
    let mut ep5 = opts.clone();
    ep5["episode"] = json!(5);
    let (a, b) = tokio::join!(host.call(&ext.id, "torrent", opts.clone()), host.call(&ext.id, "torrent", ep5));
    println!("torrent ep6 + ep5 at once: ep6 {} | ep5 {}", count(&a), count(&b));
    for line in host.logs(&ext.id).iter().rev().take(15) {
        println!("log: {line}");
    }
}
