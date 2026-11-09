//! `/v1/history` over real mTLS (wave A, worker f; docs/redesign/15 section 3.8): a paired phone pulls the PC history
//! page by page from its cursor, pushes its own rows (newest `watchedAt` wins, the exact position travels), clears
//! the PC history, and nothing works without a client certificate or after the device is revoked.

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{Canned, FakeEngine, lenient_default, quick_tuning};
use rcgen::{CertificateParams, KeyPair, PKCS_ECDSA_P256_SHA256};
use serde_json::{Value, json};
use time::OffsetDateTime;
use tk_core::{HistoryEntry, MediaRef};
use tk_link::{LibraryItem, LinkConfig, LinkHandle, LinkHost};

/// In-memory history with the PC's merge rule (`HistoryEntry::wins_over`), like `tk-app`'s `history_sync`.
#[derive(Default)]
struct MemHistory {
    rows: Mutex<Vec<HistoryEntry>>,
}

impl LinkHost for MemHistory {
    fn library(&self) -> Vec<LibraryItem> {
        Vec::new()
    }
    fn library_path(&self, _id: &str) -> Option<std::path::PathBuf> {
        None
    }
    fn ffmpeg_path(&self) -> Option<std::path::PathBuf> {
        Some("ffmpeg-for-tests".into())
    }
    fn history_since(&self, since_unix: i64, limit: usize) -> Vec<HistoryEntry> {
        let mut v: Vec<HistoryEntry> =
            self.rows.lock().unwrap().iter().filter(|e| e.watched_at.unix_timestamp() > since_unix).cloned().collect();
        v.sort_by_key(|e| e.watched_at);
        v.truncate(limit);
        v
    }
    fn history_merge(&self, rows: Vec<HistoryEntry>) -> usize {
        let mut store = self.rows.lock().unwrap();
        let mut merged = 0;
        for r in rows {
            match store.iter_mut().find(|e| e.episode_id == r.episode_id) {
                Some(cur) if r.wins_over(cur) => {
                    *cur = r;
                    merged += 1;
                }
                Some(_) => {}
                None => {
                    store.push(r);
                    merged += 1;
                }
            }
        }
        merged
    }
    fn history_clear(&self) -> bool {
        self.rows.lock().unwrap().clear();
        true
    }
}

fn entry(ep: u32, progress: u32, at: i64) -> HistoryEntry {
    HistoryEntry::for_playback(&MediaRef::anilist(21), "One Piece", None, ep, progress, Some(1440), OffsetDateTime::from_unix_timestamp(at).unwrap()).unwrap()
}

struct Server {
    link: LinkHandle,
    _dir: tempfile::TempDir,
    host: Arc<MemHistory>,
    base: String,
}

async fn server() -> Server {
    let dir = tempfile::tempdir().unwrap();
    let host = Arc::new(MemHistory::default());
    let mut cfg = LinkConfig::new(dir.path().join("link"), "Test PC");
    cfg.port = 0;
    cfg.bind_ips = vec!["127.0.0.1".parse().unwrap()];
    cfg.tuning = quick_tuning();
    cfg.engine = Some(FakeEngine::new(1, 10, Duration::from_millis(1)));
    cfg.runner = Some(Arc::new(Canned));
    cfg.fetch_http = Some(lenient_default());
    let link = tk_link::start(cfg, host.clone()).await.unwrap();
    let base = format!("https://127.0.0.1:{}", link.port());
    Server { link, _dir: dir, host, base }
}

/// Pair a phone (same flow as `common::Harness::pair`) and return its mTLS client and device id.
async fn pair(s: &Server, name: &str) -> (reqwest::Client, String) {
    let payload = s.link.open_pairing_window();
    let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
    let csr = CertificateParams::default().serialize_request(&key).unwrap().pem().unwrap();
    let bootstrap = reqwest::Client::builder().no_proxy().danger_accept_invalid_certs(true).build().unwrap();
    let resp = bootstrap
        .post(format!("{}/v1/pairing", s.base))
        .json(&json!({ "code": payload.code, "device_name": name, "csr_pem": csr, "platform": "android" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201, "pairing failed");
    let body: Value = resp.json().await.unwrap();
    let ca_pem = body["ca_cert_pem"].as_str().unwrap().to_owned();
    let identity = reqwest::Identity::from_pem(format!("{}\n{}", body["client_cert_pem"].as_str().unwrap(), key.serialize_pem()).as_bytes()).unwrap();
    let client = reqwest::Client::builder()
        .no_proxy()
        .tls_built_in_root_certs(false)
        .add_root_certificate(reqwest::Certificate::from_pem(ca_pem.as_bytes()).unwrap())
        .identity(identity)
        .build()
        .unwrap();
    (client, body["device_id"].as_str().unwrap().to_owned())
}

async fn body(r: reqwest::Response) -> Value {
    r.json().await.unwrap()
}

#[tokio::test]
async fn pull_pages_follow_the_cursor_oldest_first() {
    let s = server().await;
    s.host.history_merge(vec![entry(1, 100, 1_000), entry(2, 383, 2_000), entry(3, 900, 3_000)]);
    let (phone, _) = pair(&s, "Pixel").await;

    // first full pull, 2 rows per page
    let r = phone.get(format!("{}/v1/history?limit=2", s.base)).send().await.unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["tatakai-link"], "v1");
    let page = body(r).await;
    assert_eq!(page["items"].as_array().unwrap().iter().map(|i| i["episode"].as_u64().unwrap()).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(page["truncated"], true);
    assert_eq!(page["next_since_unix"], 2_000);
    // the exact resume position and the camelCase row shape
    let item = &page["items"][1];
    assert_eq!(item["progressSec"], 383);
    assert_eq!(item["animeId"], "21");
    assert_eq!(item["episodeId"], "21?ep=2");
    assert_eq!(item["anilistId"], 21);
    assert!(item.get("malId").is_none(), "unknown values are omitted, never null");

    // next page from the cursor
    let page = body(phone.get(format!("{}/v1/history?since=2000&limit=2", s.base)).send().await.unwrap()).await;
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    assert_eq!(page["items"][0]["episode"], 3);
    assert_eq!(page["truncated"], false);
    assert_eq!(page["next_since_unix"], 3_000);

    // nothing newer: empty page keeps the cursor
    let page = body(phone.get(format!("{}/v1/history?since=3000", s.base)).send().await.unwrap()).await;
    assert_eq!(page, json!({ "items": [], "next_since_unix": 3000, "truncated": false }));

    // bad queries are `request_invalid`, not axum's plain-text rejection
    for q in ["since=-1", "since=abc", "limit=x"] {
        let r = phone.get(format!("{}/v1/history?{q}", s.base)).send().await.unwrap();
        assert_eq!(r.status(), 400, "{q}");
        assert_eq!(body(r).await["error"]["code"], "request_invalid", "{q}");
    }
}

#[tokio::test]
async fn push_merges_by_newest_and_moves_the_exact_position() {
    let s = server().await;
    s.host.history_merge(vec![entry(1, 100, 1_000), entry(2, 700, 5_000)]);
    let (phone, _) = pair(&s, "Pixel").await;

    // ep 1 newer on the phone (left at 6:23), ep 2 older (loses), ep 3 new, one row without ids and one with a bad
    // date are dropped without failing the rest
    let push = json!({ "items": [
        { "animeId": "21", "title": "One Piece", "episodeId": "21?ep=1", "episode": 1, "progressSec": 383, "durationSec": 1440,
          "completed": false, "watchedAt": "2026-10-27T21:04:11.000Z", "anilistId": 21 },
        { "animeId": "21", "title": "One Piece", "episodeId": "21?ep=2", "episode": 2, "progressSec": 10,
          "completed": false, "watchedAt": "1970-01-01T00:16:40Z", "anilistId": 21 },
        { "animeId": "21", "title": "One Piece", "episodeId": "21?ep=3", "episode": 3, "progressSec": 1400, "durationSec": 1440,
          "completed": true, "watchedAt": "2026-10-27T21:05:00Z", "anilistId": 21 },
        { "animeId": "x", "title": "No ids", "episodeId": "x?ep=1", "episode": 1, "watchedAt": "2026-10-27T21:05:00Z" },
        { "animeId": "21", "title": "Bad date", "episodeId": "21?ep=9", "episode": 9, "watchedAt": "yesterday", "anilistId": 21 }
    ]});
    let r = phone.post(format!("{}/v1/history", s.base)).json(&push).send().await.unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(body(r).await, json!({ "merged": 2 }));

    let rows = s.host.rows.lock().unwrap().clone();
    let ep = |n: u32| rows.iter().find(|e| e.episode == n).cloned().unwrap();
    assert_eq!(ep(1).progress_sec, 383, "the phone's newer position wins");
    assert_eq!(ep(1).watched_at.unix_timestamp(), 1_793_135_051);
    assert_eq!(ep(2).progress_sec, 700, "an older phone row never overwrites the PC");
    assert!(ep(3).completed);
    assert!(rows.iter().all(|e| e.episode != 9 && e.anime_id != "x"));

    // the merged rows come back to a pull (here: a second phone doing its first full sync)
    let (tablet, _) = pair(&s, "Tablet").await;
    let page = body(tablet.get(format!("{}/v1/history", s.base)).send().await.unwrap()).await;
    assert_eq!(page["items"].as_array().unwrap().len(), 3);

    // invalid pushes
    for bad in [json!({ "items": [] }), json!({ "items": [], "user_id": "x" }), json!({ "nope": 1 })] {
        let r = phone.post(format!("{}/v1/history", s.base)).json(&bad).send().await.unwrap();
        assert_eq!(r.status(), 400, "{bad}");
        assert_eq!(body(r).await["error"]["code"], "request_invalid");
    }
    let too_many = json!({ "items": (0..501).map(|i| json!({ "animeId": "21", "title": "t", "episodeId": format!("21?ep={i}"),
        "episode": i, "watchedAt": "2026-10-27T21:05:00Z", "anilistId": 21 })).collect::<Vec<_>>() });
    let r = phone.post(format!("{}/v1/history", s.base)).json(&too_many).send().await.unwrap();
    assert_eq!(r.status(), 400);
    let huge = format!(r#"{{"items":[{{"animeId":"21","title":"{}","episodeId":"21?ep=1","episode":1,"watchedAt":"2026-10-27T21:05:00Z","anilistId":21}}]}}"#, "x".repeat(300 * 1024));
    let r = phone.post(format!("{}/v1/history", s.base)).header("content-type", "application/json").body(huge).send().await.unwrap();
    assert_eq!(r.status(), 413);
    assert_eq!(body(r).await["error"]["code"], "body_too_large");
}

#[tokio::test]
async fn delete_clears_the_pc_history() {
    let s = server().await;
    s.host.history_merge(vec![entry(1, 100, 1_000), entry(2, 383, 2_000)]);
    let (phone, _) = pair(&s, "Pixel").await;
    let r = phone.delete(format!("{}/v1/history", s.base)).send().await.unwrap();
    assert_eq!(r.status(), 204);
    assert!(s.host.rows.lock().unwrap().is_empty());
    let page = body(phone.get(format!("{}/v1/history", s.base)).send().await.unwrap()).await;
    assert_eq!(page["items"], json!([]));
}

#[tokio::test]
async fn history_needs_a_client_certificate_and_an_active_device() {
    let s = server().await;
    s.host.history_merge(vec![entry(1, 100, 1_000)]);
    let bootstrap = reqwest::Client::builder().no_proxy().danger_accept_invalid_certs(true).build().unwrap();
    for (m, p) in [("GET", "/v1/history"), ("POST", "/v1/history"), ("DELETE", "/v1/history")] {
        let r = bootstrap.request(m.parse().unwrap(), format!("{}{p}", s.base)).send().await.unwrap();
        assert_eq!(r.status(), 401, "{m} {p}");
        assert_eq!(body(r).await["error"]["code"], "client_certificate_required");
    }
    assert_eq!(s.host.rows.lock().unwrap().len(), 1, "nothing was cleared without a certificate");

    let (phone, device) = pair(&s, "Pixel").await;
    assert_eq!(phone.get(format!("{}/v1/history", s.base)).send().await.unwrap().status(), 200);
    s.link.revoke(&device);
    // Revoking drops the device's live keep-alive connection (as in e2e.rs), so a request may fail at the transport
    // level; that is a valid refusal too. Any answer that does arrive must be the 403.
    for _ in 0..2 {
        if let Ok(r) = phone.get(format!("{}/v1/history", s.base)).send().await {
            assert_eq!(r.status(), 403);
            assert_eq!(body(r).await["error"]["code"], "device_not_authorized");
        }
    }
    assert_eq!(s.host.rows.lock().unwrap().len(), 1, "a revoked device reads and clears nothing");
}
