//! Wave B (worker k): `POST /v1/control`, `GET /v1/player` and the `GET /v1/events` long-poll over real mTLS, plus
//! `last_seen_at` / platform tracking (`link.remote-control`, `link.handoff`, `link.send-to-device`, `link.last-seen`).

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{Canned, FakeEngine, lenient_default, quick_tuning};
use rcgen::{CertificateParams, KeyPair, PKCS_ECDSA_P256_SHA256};
use serde_json::{Value, json};
use tk_link::control::{ControlCommand, PhoneEvent, PlayerState};
use tk_link::{LibraryItem, LinkConfig, LinkHandle, LinkHost};

#[derive(Default)]
struct PlayerHost {
    playing: Mutex<Option<PlayerState>>,
    got: Mutex<Vec<ControlCommand>>,
}

impl LinkHost for PlayerHost {
    fn library(&self) -> Vec<LibraryItem> {
        Vec::new()
    }
    fn library_path(&self, _id: &str) -> Option<std::path::PathBuf> {
        None
    }
    fn ffmpeg_path(&self) -> Option<std::path::PathBuf> {
        Some("ffmpeg-for-tests".into())
    }
    fn control(&self, cmd: ControlCommand) -> bool {
        self.got.lock().unwrap().push(cmd);
        true
    }
    fn player_state(&self) -> Option<PlayerState> {
        self.playing.lock().unwrap().clone()
    }
}

struct Server {
    link: LinkHandle,
    _dir: tempfile::TempDir,
    host: Arc<PlayerHost>,
    base: String,
}

async fn server() -> Server {
    let dir = tempfile::tempdir().unwrap();
    let host = Arc::new(PlayerHost::default());
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
        .user_agent("okhttp/4.12.0")
        .tls_built_in_root_certs(false)
        .add_root_certificate(reqwest::Certificate::from_pem(ca_pem.as_bytes()).unwrap())
        .identity(identity)
        .build()
        .unwrap();
    (client, body["device_id"].as_str().unwrap().to_owned())
}

#[tokio::test]
async fn control_needs_a_player_except_open() {
    let s = server().await;
    let (phone, _) = pair(&s, "Pixel").await;

    // nothing playing: 404 / 409 no_player, but `open` (hand-off) is always taken
    let r = phone.get(format!("{}/v1/player", s.base)).send().await.unwrap();
    assert_eq!(r.status(), 404);
    assert_eq!(r.json::<Value>().await.unwrap()["error"]["code"], "no_player");
    let r = phone.post(format!("{}/v1/control", s.base)).json(&json!({ "command": "toggle" })).send().await.unwrap();
    assert_eq!(r.status(), 409);
    let r = phone
        .post(format!("{}/v1/control", s.base))
        .json(&json!({ "command": "open", "mediaId": "21", "episode": 3, "positionSecs": 120 }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 204);

    // playing: state and transport commands
    *s.host.playing.lock().unwrap() =
        Some(PlayerState { playing: true, title: Some("One Piece".into()), episode: Some(3), position_secs: 125, duration_secs: Some(1440), media_id: Some("21".into()) });
    let st: Value = phone.get(format!("{}/v1/player", s.base)).send().await.unwrap().json().await.unwrap();
    assert_eq!(st["positionSecs"], 125);
    assert_eq!(st["mediaId"], "21");
    for c in ["prev", "toggle", "next"] {
        let r = phone.post(format!("{}/v1/control", s.base)).json(&json!({ "command": c })).send().await.unwrap();
        assert_eq!(r.status(), 204, "{c}");
    }
    let bad = phone.post(format!("{}/v1/control", s.base)).json(&json!({ "command": "explode" })).send().await.unwrap();
    assert_eq!(bad.status(), 400);
    let got = s.host.got.lock().unwrap().clone();
    assert_eq!(
        got,
        vec![
            ControlCommand::Open { media_id: "21".into(), episode: 3, position_secs: 120 },
            ControlCommand::Prev,
            ControlCommand::Toggle,
            ControlCommand::Next
        ]
    );
}

#[tokio::test]
async fn events_long_poll_delivers_handoffs_once() {
    let s = server().await;
    let (phone, device) = pair(&s, "Pixel").await;

    // nothing queued: 204 after `wait`
    let r = phone.get(format!("{}/v1/events?wait=1", s.base)).send().await.unwrap();
    assert_eq!(r.status(), 204);

    // queued while the phone waits: delivered right away
    let poll = {
        let phone = phone.clone();
        let url = format!("{}/v1/events?wait=10", s.base);
        tokio::spawn(async move { phone.get(url).send().await.unwrap() })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    let ev = PhoneEvent::Handoff { session_id: "s1".into(), media_id: Some("21".into()), episode: Some(3), position_secs: 75, title: Some("One Piece".into()) };
    assert!(s.link.push_event(&device, ev));
    let r = tokio::time::timeout(Duration::from_secs(5), poll).await.unwrap().unwrap();
    assert_eq!(r.status(), 200);
    let page: Value = r.json().await.unwrap();
    assert_eq!(page["events"][0]["type"], "handoff");
    assert_eq!(page["events"][0]["sessionId"], "s1");
    assert_eq!(page["events"][0]["positionSecs"], 75);
    // drained on delivery
    let r = phone.get(format!("{}/v1/events?wait=1", s.base)).send().await.unwrap();
    assert_eq!(r.status(), 204);
    assert!(!s.link.push_event("unknown-device", PhoneEvent::Handoff { session_id: "x".into(), media_id: None, episode: None, position_secs: 0, title: None }));

    // last-seen and platform come from the authenticated requests
    let d = s.link.devices().into_iter().find(|d| d.id == device).unwrap();
    assert!(d.last_seen_at.is_some());
    assert_eq!(d.platform, "android");
}
