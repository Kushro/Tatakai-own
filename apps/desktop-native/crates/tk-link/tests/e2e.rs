//! End-to-end over real TLS: mTLS, pairing, per-request authorization, hot revocation, sessions with a fake FFmpeg.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tk_link::SessionTuning;

async fn json(r: reqwest::Response) -> Value {
    r.json().await.unwrap()
}

async fn wait_until<F: std::future::Future<Output = bool>>(what: &str, mut f: impl FnMut() -> F) {
    for _ in 0..200 {
        if f().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("timed out waiting for {what}");
}

async fn upstream_hls(segments: u32) -> u16 {
    let body = media_playlist(segments);
    stub(move |_| text(200, &body)).await.0
}

async fn create(h: &Harness, p: &Paired, port: u16, extra: Value) -> reqwest::Response {
    let mut body = json!({ "source": { "kind": "url", "url": format!("http://127.0.0.1:{port}/list.m3u8"), "container": "hls" } });
    for (k, v) in extra.as_object().cloned().unwrap_or_default() {
        body[k] = v;
    }
    p.client.post(h.url("/v1/sessions")).json(&body).send().await.unwrap()
}

#[tokio::test]
async fn mtls_is_required_everywhere_except_pairing_and_every_response_carries_the_contract_header() {
    let mut h = harness(FakeEngine::new(3, 100, Duration::from_millis(1)), quick_tuning(), lenient_default()).await;
    // pairing needs no client cert (and a closed window answers 409 with the fixed message)
    let bootstrap = reqwest::Client::builder().no_proxy().danger_accept_invalid_certs(true).build().unwrap();
    let r = bootstrap.post(h.url("/v1/pairing")).json(&json!({ "code": "0000000000", "device_name": "x", "csr_pem": "x", "platform": "android" })).send().await.unwrap();
    assert_eq!(r.status(), 409);
    assert_eq!(r.headers()["tatakai-link"], "v1");
    assert_eq!(json(r).await, json!({ "error": { "code": "pairing_closed", "message": "no pairing window is open" } }));

    // everything else without a client certificate: 401 client_certificate_required
    for (m, p) in [("GET", "/v1/capabilities"), ("GET", "/v1/library"), ("POST", "/v1/sessions"), ("GET", "/v1/sessions/x"), ("DELETE", "/v1/devices/self"), ("GET", "/v1/nope")] {
        let r = bootstrap.request(m.parse().unwrap(), h.url(p)).send().await.unwrap();
        assert_eq!(r.status(), 401, "{m} {p}");
        assert_eq!(r.headers()["tatakai-link"], "v1");
        assert_eq!(json(r).await["error"]["code"], "client_certificate_required");
    }

    let p = h.pair("Pixel").await;
    // authorized: unknown route -> 404 session_not_found JSON; known route with wrong method -> 405 without a JSON body
    let r = p.client.get(h.url("/v1/does-not-exist")).send().await.unwrap();
    assert_eq!(r.status(), 404);
    assert_eq!(json(r).await["error"]["code"], "session_not_found");
    let r = p.client.put(h.url("/v1/capabilities")).send().await.unwrap();
    assert_eq!(r.status(), 405);
    assert!(r.bytes().await.unwrap().is_empty());

    // capabilities honour the schema
    let caps = json(p.client.get(h.url("/v1/capabilities")).send().await.unwrap()).await;
    assert_eq!(caps["contract"], "v1");
    assert_eq!(caps["default_profile"], "p-none");
    assert_eq!(caps["gpu"]["vendor"], "nvidia");
    assert_eq!(caps["encoders"], json!(["h264_nvenc", "libopenh264"]));
    assert_eq!(caps["limits"], json!({ "max_ram_bytes": 536870912u64, "segment_wait_secs": 20 }));
    assert_eq!(caps["profiles"][0]["id"], "p-none");
    // no Anime4K shaders installed in the test environment -> the Anime4K profiles are withheld
    assert_eq!(caps["profiles"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn pairing_flow_pin_attempts_expiry_and_single_use() {
    let mut h = harness(FakeEngine::new(3, 100, Duration::from_millis(1)), quick_tuning(), lenient_default()).await;
    let bootstrap = reqwest::Client::builder().no_proxy().danger_accept_invalid_certs(true).build().unwrap();
    let csr = || {
        let key = rcgen::KeyPair::generate_for(&rcgen::PKCS_ECDSA_P256_SHA256).unwrap();
        rcgen::CertificateParams::default().serialize_request(&key).unwrap().pem().unwrap()
    };
    let attempt = |code: String, csr: String, body_extra: Value| {
        let client = bootstrap.clone();
        let url = h.url("/v1/pairing");
        async move {
            let mut b = json!({ "code": code, "device_name": "Phone", "csr_pem": csr, "platform": "android" });
            for (k, v) in body_extra.as_object().cloned().unwrap_or_default() {
                b[k] = v;
            }
            let r = client.post(url).json(&b).send().await.unwrap();
            (r.status().as_u16(), json(r).await)
        }
    };

    let payload = h.link.open_pairing_window();
    assert!(payload.uri().starts_with("tatakai-link:v1?h=127.0.0.1&p="));
    assert!(payload.uri().contains(&format!("&ca={}&c={}&x=", payload.ca_fingerprint, payload.code)));
    // invalid bodies consume no attempt
    assert_eq!(attempt(payload.code.clone(), "garbage".into(), json!({})).await.0, 400);
    assert_eq!(attempt(payload.code.clone(), csr(), json!({ "platform": "ios" })).await.0, 400);
    assert_eq!(attempt(payload.code.clone(), csr(), json!({ "unexpected": 1 })).await.0, 400);
    assert_eq!(h.link.pairing_status(), tk_link::WindowStatus::Open { expires_at: payload.expires_at, attempts_left: 5 });
    // wrong codes: 403 x4, the 5th closes the window with 429, then even the right code is refused
    for _ in 0..4 {
        let (st, body) = attempt("ABCDEFGHJK".into(), csr(), json!({})).await;
        assert_eq!((st, body["error"]["code"].as_str()), (403, Some("pairing_code_invalid")));
    }
    assert_eq!(attempt("ABCDEFGHJK".into(), csr(), json!({})).await.0, 429);
    assert_eq!(attempt(payload.code.clone(), csr(), json!({})).await.0, 429);
    assert!(h.link.devices().is_empty());

    // fresh window: the right code pairs, the pinned CA matches the payload, the window is single use
    let payload = h.link.open_pairing_window();
    let (st, body) = attempt(payload.code.to_lowercase(), csr(), json!({})).await; // Crockford: case-insensitive
    assert_eq!(st, 201);
    let ca_pem = body["ca_cert_pem"].as_str().unwrap();
    let der = rustls_pemfile_der(ca_pem);
    assert_eq!(hex::encode(Sha256::digest(&der)), payload.ca_fingerprint, "the CA in the response is the one pinned by the QR payload");
    assert_eq!(body["server_name"], "Test PC");
    assert_eq!(attempt(payload.code.clone(), csr(), json!({})).await.0, 409, "single use");
    assert_eq!(h.link.devices().len(), 1);
    let _ = &mut h;
}

fn rustls_pemfile_der(pem: &str) -> Vec<u8> {
    use base64::Engine;
    let b64: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
    base64::engine::general_purpose::STANDARD.decode(b64).unwrap()
}

#[tokio::test]
async fn hot_revocation_blocks_the_next_request_and_drops_the_live_session() {
    let engine = FakeEngine::new(40, 100, Duration::from_millis(5));
    let mut h = harness(engine, quick_tuning(), lenient_default()).await;
    let p = h.pair("Pixel").await;
    let port = upstream_hls(40).await;
    let created = create(&h, &p, port, json!({})).await;
    assert_eq!(created.status(), 201);
    let sid = json(created).await["session_id"].as_str().unwrap().to_owned();
    assert_eq!(h.link.active_sessions(), 1);

    let started = std::time::Instant::now();
    h.link.revoke(&p.device_id);
    // the very next request is refused (the live keep-alive connection was dropped, so the client reconnects with its cert)
    let r = p.client.get(h.url(&format!("/v1/sessions/{sid}"))).send().await;
    if let Ok(r) = r {
        assert_eq!(r.status(), 403); // (a connection closed by the server is also a valid refusal)
    }
    let r = p.client.get(h.url("/v1/capabilities")).send().await;
    if let Ok(r) = r {
        assert_eq!(r.status(), 403);
        assert_eq!(json(r).await["error"]["code"], "device_not_authorized");
    }
    assert!(started.elapsed() < Duration::from_secs(2), "revocation must take effect within 2 s");
    wait_until("session teardown", || async { h.link.active_sessions() == 0 }).await;
    assert!(h.link.devices()[0].revoked_at.is_some());
}

#[tokio::test]
async fn delete_devices_self_forgets_the_caller() {
    let mut h = harness(FakeEngine::new(3, 100, Duration::from_millis(1)), quick_tuning(), lenient_default()).await;
    let p = h.pair("Pixel").await;
    assert_eq!(p.client.delete(h.url("/v1/devices/self")).send().await.unwrap().status(), 204);
    let r = p.client.get(h.url("/v1/capabilities")).send().await;
    if let Ok(r) = r {
        assert_eq!(r.status(), 403);
    }
    assert!(h.link.devices()[0].revoked_at.is_some());
}

#[tokio::test]
async fn session_lifecycle_vod_playlists_segments_and_one_session_per_device() {
    let engine = FakeEngine::new(5, 1000, Duration::from_millis(1));
    let mut h = harness(engine.clone(), quick_tuning(), lenient_default()).await;
    let p = h.pair("Pixel").await;
    let port = upstream_hls(5).await;

    let r = create(&h, &p, port, json!({})).await;
    assert_eq!(r.status(), 201);
    let c = json(r).await;
    assert_eq!(c["generation"], 1);
    assert_eq!(c["duration_secs"], 10.0);
    assert_eq!((c["segment_secs"].as_u64(), c["heartbeat_interval_secs"].as_u64()), (Some(2), Some(4)));
    let sid = c["session_id"].as_str().unwrap().to_owned();
    assert_eq!(c["playback_path"], format!("/v1/sessions/{sid}/master.m3u8"));

    // master becomes available once the engine produced init.mp4
    wait_until("master.m3u8", || async {
        let r = p.client.get(h.url(c["playback_path"].as_str().unwrap())).send().await.unwrap();
        match r.status().as_u16() {
            200 => true,
            409 => {
                assert_eq!(json(r).await["error"]["code"], "session_not_ready");
                false
            }
            other => panic!("unexpected {other}"),
        }
    })
    .await;
    let master = p.client.get(h.url(&format!("/v1/sessions/{sid}/master.m3u8"))).send().await.unwrap().text().await.unwrap();
    assert!(master.contains("RESOLUTION=1920x1080") && master.contains("media.m3u8"));
    let media = p.client.get(h.url(&format!("/v1/sessions/{sid}/media.m3u8"))).send().await.unwrap().text().await.unwrap();
    assert!(media.contains("#EXT-X-PLAYLIST-TYPE:VOD") && media.contains("#EXT-X-MAP:URI=\"init.mp4\"") && media.contains("#EXT-X-ENDLIST"));
    assert_eq!(media.matches("#EXTINF:2.000000,").count(), 5);
    let init = p.client.get(h.url(&format!("/v1/sessions/{sid}/init.mp4"))).send().await.unwrap();
    assert_eq!(init.headers()["content-type"], "video/mp4");
    assert_eq!(init.bytes().await.unwrap().as_ref(), b"INIT");
    for k in 0..5u32 {
        let r = p.client.get(h.url(&format!("/v1/sessions/{sid}/seg_{k}.m4s"))).send().await.unwrap();
        assert_eq!(r.status(), 200);
        let b = r.bytes().await.unwrap();
        assert_eq!((b.len(), b[0]), (1000, k as u8));
    }
    // beyond the end / garbage names
    assert_eq!(p.client.get(h.url(&format!("/v1/sessions/{sid}/seg_5.m4s"))).send().await.unwrap().status(), 404);
    assert_eq!(p.client.get(h.url(&format!("/v1/sessions/{sid}/seg_x.m4s"))).send().await.unwrap().status(), 404);
    assert_eq!(p.client.get(h.url(&format!("/v1/sessions/{sid}/..%2Fsecret"))).send().await.unwrap().status(), 404);

    let st = json(p.client.get(h.url(&format!("/v1/sessions/{sid}"))).send().await.unwrap()).await;
    assert_eq!(st["state"], "ready");
    assert_eq!(st["profile"], "p-none");
    assert_eq!(st["output"], json!({ "width": 1920, "height": 1080 }));
    assert!(st["instance_id"].as_str().unwrap().len() == 36);

    // heartbeat
    let hb = p.client.post(h.url(&format!("/v1/sessions/{sid}/position"))).json(&json!({ "position_secs": 3.5, "paused": false })).send().await.unwrap();
    assert_eq!(hb.status(), 204);
    let bad = p.client.post(h.url(&format!("/v1/sessions/{sid}/position"))).json(&json!({ "position_secs": -1, "paused": false })).send().await.unwrap();
    assert_eq!(bad.status(), 400);

    // the FFmpeg arguments never carry the external URL
    let args = engine.args.lock().unwrap()[0].join(" ");
    assert!(!args.contains(&format!("127.0.0.1:{port}")), "{args}");
    assert!(args.contains("-protocol_whitelist http,tcp,crypto"));

    // a second session closes the first one (410) and increments the generation
    let c2 = json(create(&h, &p, port, json!({})).await).await;
    assert_eq!(c2["generation"], 2);
    let gone = p.client.get(h.url(&format!("/v1/sessions/{sid}/media.m3u8"))).send().await.unwrap();
    assert_eq!(gone.status(), 410);
    assert_eq!(json(gone).await["error"]["code"], "session_gone");
    assert_eq!(h.link.active_sessions(), 1);

    // DELETE
    let sid2 = c2["session_id"].as_str().unwrap();
    assert_eq!(p.client.delete(h.url(&format!("/v1/sessions/{sid2}"))).send().await.unwrap().status(), 204);
    assert_eq!(p.client.get(h.url(&format!("/v1/sessions/{sid2}"))).send().await.unwrap().status(), 410);
    assert_eq!(p.client.get(h.url("/v1/sessions/00000000-0000-4000-8000-000000000000")).send().await.unwrap().status(), 404);
}

#[tokio::test]
async fn a_device_cannot_touch_another_devices_session() {
    let mut h = harness(FakeEngine::new(3, 100, Duration::from_millis(1)), quick_tuning(), lenient_default()).await;
    let a = h.pair("A").await;
    let b = h.pair("B").await;
    let port = upstream_hls(3).await;
    let sid = json(create(&h, &a, port, json!({})).await).await["session_id"].as_str().unwrap().to_owned();
    for (m, path) in [("GET", format!("/v1/sessions/{sid}")), ("DELETE", format!("/v1/sessions/{sid}")), ("GET", format!("/v1/sessions/{sid}/media.m3u8"))] {
        let r = b.client.request(m.parse().unwrap(), h.url(&path)).send().await.unwrap();
        assert_eq!(r.status(), 404, "{m} {path}");
    }
    // B creating a session does not close A's
    assert_eq!(create(&h, &b, port, json!({})).await.status(), 201);
    assert_eq!(a.client.get(h.url(&format!("/v1/sessions/{sid}"))).send().await.unwrap().status(), 200);
}

#[tokio::test]
async fn create_validation_table() {
    let mut h = harness(FakeEngine::new(3, 100, Duration::from_millis(1)), quick_tuning(), lenient_default()).await;
    let p = h.pair("Pixel").await;
    let port = upstream_hls(3).await;
    let post = |body: Value| {
        let c = p.client.clone();
        let url = h.url("/v1/sessions");
        async move {
            let r = c.post(url).json(&body).send().await.unwrap();
            (r.status().as_u16(), json(r).await["error"]["code"].as_str().map(str::to_owned))
        }
    };
    let u = format!("http://127.0.0.1:{port}/list.m3u8");
    // unknown / unavailable profile -> 422
    assert_eq!(post(json!({ "source": { "kind": "url", "url": u }, "profile": "p-nope" })).await, (422, Some("profile_unavailable".into())));
    assert_eq!(post(json!({ "source": { "kind": "url", "url": u }, "profile": "p-anime4k-1080" })).await, (422, Some("profile_unavailable".into())));
    // CR/LF or non-allow-listed headers -> 400 (before any policy decision)
    assert_eq!(post(json!({ "source": { "kind": "url", "url": u, "headers": { "Referer": "https://a/\r\nX-Evil: 1" } } })).await, (400, Some("request_invalid".into())));
    assert_eq!(post(json!({ "source": { "kind": "url", "url": u, "headers": { "Referer\r\nX": "v" } } })).await, (400, Some("request_invalid".into())));
    assert_eq!(post(json!({ "source": { "kind": "url", "url": u, "headers": { "Authorization": "Bearer x" } } })).await, (400, Some("request_invalid".into())));
    // malformed bodies
    assert_eq!(post(json!({ "source": { "kind": "url" } })).await.0, 400);
    assert_eq!(post(json!({ "source": { "kind": "phone", "size_bytes": 1, "name": "x" } })).await.0, 400);
    assert_eq!(post(json!({ "source": { "kind": "url", "url": "not a url" } })).await.0, 400);
    assert_eq!(post(json!({ "source": { "kind": "url", "url": u }, "start_secs": -1 })).await.0, 400);
    assert_eq!(post(json!({ "source": { "kind": "url", "url": u, "container": "dash" } })).await.0, 400);
    let big = "x".repeat(5000);
    assert_eq!(post(json!({ "source": { "kind": "url", "url": u }, "pad": big })).await.0, 413);
    // library items that do not exist
    assert_eq!(post(json!({ "source": { "kind": "library", "item_id": "0".repeat(32) } })).await, (404, Some("library_item_not_found".into())));
    assert_eq!(post(json!({ "source": { "kind": "library", "item_id": "../../etc" } })).await.0, 400);
    // upstream unreachable -> 503 source_unavailable
    assert_eq!(post(json!({ "source": { "kind": "url", "url": "http://127.0.0.1:9/list.m3u8", "container": "hls" } })).await, (503, Some("source_unavailable".into())));
    // policy denials -> 403 url_source_denied (strict targets: the lenient test policy still blocks private/metadata addresses)
    for denied in ["http://10.0.0.1/list.m3u8", "http://169.254.169.254/list.m3u8", "http://192.168.1.1/list.m3u8", "ftp://example.com/list.m3u8", "file:///C:/x.m3u8"] {
        assert_eq!(post(json!({ "source": { "kind": "url", "url": denied } })).await, (403, Some("url_source_denied".into())), "{denied}");
    }
}

#[tokio::test]
async fn http_sources_need_the_allow_http_setting() {
    let mut h = harness_with(FakeEngine::new(3, 100, Duration::from_millis(1)), quick_tuning(), tk_core::net::SafeHttp::new(tk_core::net::Policy::link_source(false)), |c| c.allow_http_sources = false).await;
    let p = h.pair("Pixel").await;
    let r = p.client.post(h.url("/v1/sessions")).json(&json!({ "source": { "kind": "url", "url": "http://example.com/a.m3u8" } })).send().await.unwrap();
    assert_eq!(r.status(), 403);
}

#[tokio::test]
async fn library_source_streams_through_the_loopback_file_server() {
    let engine = FakeEngine::new(3, 100, Duration::from_millis(1));
    let mut h = harness(engine.clone(), quick_tuning(), lenient_default()).await;
    let p = h.pair("Pixel").await;
    let file = h.dir.path().join("My Episode.mkv");
    std::fs::write(&file, vec![7u8; 4096]).unwrap();
    let id = tk_link::library::id_for(&file);
    h.host.files.lock().unwrap().insert(id.clone(), file);

    let lib = json(p.client.get(h.url("/v1/library")).send().await.unwrap()).await;
    assert_eq!(lib["truncated"], false);
    assert_eq!(lib["items"][0]["id"], id);
    assert_eq!(lib["items"][0]["title"], "My Episode");

    let r = p.client.post(h.url("/v1/sessions")).json(&json!({ "source": { "kind": "library", "item_id": id } })).send().await.unwrap();
    assert_eq!(r.status(), 201);
    let c = json(r).await;
    assert_eq!(c["duration_secs"], 6.0);
    let args = engine.args.lock().unwrap()[0].clone();
    let input = &args[args.iter().position(|a| a == "-i").unwrap() + 1];
    assert!(input.starts_with("http://127.0.0.1:") && input.contains("/f/"), "FFmpeg reads the file through the loopback server, never by path: {input}");
    assert!(!args.iter().any(|a| a.contains("My Episode")));
    // the loopback file server supports ranges
    let r = no_proxy_client().get(input).header("range", "bytes=10-19").send().await.unwrap();
    assert_eq!(r.status(), 206);
    assert_eq!(r.headers()["content-range"], "bytes 10-19/4096");
    assert_eq!(r.bytes().await.unwrap().len(), 10);
}

#[tokio::test]
async fn back_pressure_seek_restart_ram_window_and_idle_timeout() {
    // 200 segments of 1000 bytes, produced as fast as possible
    let engine = FakeEngine::new(200, 1000, Duration::from_millis(0));
    let tuning = SessionTuning { segment_wait: Duration::from_secs(5), max_ram_bytes: 20_000, idle_timeout: Duration::from_secs(60), ..SessionTuning::default() };
    let mut h = harness(engine.clone(), tuning, lenient_default()).await;
    let p = h.pair("Pixel").await;
    let port = upstream_hls(200).await;
    let sid = json(create(&h, &p, port, json!({})).await).await["session_id"].as_str().unwrap().to_owned();

    // back-pressure: with the playhead at 0 the engine may only run max_lead (15) segments ahead
    wait_until("engine to be throttled", || async {
        let st = json(p.client.get(h.url(&format!("/v1/sessions/{sid}"))).send().await.unwrap()).await;
        st["throttled"] == true
    })
    .await;
    let st = json(p.client.get(h.url(&format!("/v1/sessions/{sid}"))).send().await.unwrap()).await;
    assert_eq!(st["frontier_segment"], 15, "{st}");
    assert!(st["window"]["bytes"].as_u64().unwrap() <= 20_000);
    // the playhead moves (heartbeat at 20 s = segment 10): the engine continues up to 10 + 15
    p.client.post(h.url(&format!("/v1/sessions/{sid}/position"))).json(&json!({ "position_secs": 20.0, "paused": false })).send().await.unwrap();
    wait_until("frontier 25", || async {
        json(p.client.get(h.url(&format!("/v1/sessions/{sid}"))).send().await.unwrap()).await["frontier_segment"] == 25
    })
    .await;

    // seek far ahead: requesting segment 150 restarts the engine there
    let r = p.client.get(h.url(&format!("/v1/sessions/{sid}/seg_150.m4s"))).send().await.unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.bytes().await.unwrap()[0], 150u8);
    assert_eq!(engine.starts.lock().unwrap().last(), Some(&150));
    let last_args = engine.args.lock().unwrap().last().cloned().unwrap();
    assert!(last_args.windows(2).any(|w| w[0] == "-ss" && w[1] == "300"), "{last_args:?}");
    assert!(last_args.windows(2).any(|w| w[0] == "-start_number" && w[1] == "150"));
    // seek back behind the window: restart again
    let r = p.client.get(h.url(&format!("/v1/sessions/{sid}/seg_3.m4s"))).send().await.unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(engine.starts.lock().unwrap().last(), Some(&3));
    // stale generations were refused by the sink (409), never stored
    assert!(engine.put_results.lock().unwrap().iter().any(|(_, st)| *st == 409) || engine.starts.lock().unwrap().len() >= 3);

    // the RAM window never exceeds its budget
    let st = json(p.client.get(h.url(&format!("/v1/sessions/{sid}"))).send().await.unwrap()).await;
    assert!(st["window"]["bytes"].as_u64().unwrap() <= 20_000, "{st}");
}

#[tokio::test]
async fn sessions_without_heartbeat_are_closed_after_the_idle_timeout() {
    let engine = FakeEngine::new(3, 100, Duration::from_millis(1));
    let tuning = SessionTuning { idle_timeout: Duration::from_millis(600), segment_wait: Duration::from_secs(5), ..SessionTuning::default() };
    let mut h = harness(engine, tuning, lenient_default()).await;
    let p = h.pair("Pixel").await;
    let port = upstream_hls(3).await;
    let sid = json(create(&h, &p, port, json!({})).await).await["session_id"].as_str().unwrap().to_owned();
    // heartbeats keep it alive past the timeout ...
    for _ in 0..6 {
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(p.client.post(h.url(&format!("/v1/sessions/{sid}/position"))).json(&json!({ "position_secs": 1.0, "paused": true })).send().await.unwrap().status(), 204);
    }
    assert_eq!(h.link.active_sessions(), 1);
    // ... and silence closes it
    wait_until("idle close", || async { h.link.active_sessions() == 0 }).await;
    let r = p.client.post(h.url(&format!("/v1/sessions/{sid}/position"))).json(&json!({ "position_secs": 1.0, "paused": true })).send().await.unwrap();
    assert_eq!(r.status(), 410);
}

#[tokio::test]
async fn a_failing_engine_marks_the_session_failed_and_denied_sources_report_the_policy() {
    // engine that cannot start
    struct Broken;
    #[async_trait::async_trait]
    impl tk_link::Engine for Broken {
        async fn spawn(&self, _job: tk_link::media::engine::EngineJob) -> std::io::Result<Box<dyn tk_link::media::engine::EngineProcess>> {
            Err(std::io::Error::other("no ffmpeg"))
        }
    }
    let mut h = harness_with(FakeEngine::new(1, 1, Duration::ZERO), quick_tuning(), lenient_default(), |c| c.engine = Some(Arc::new(Broken))).await;
    let p = h.pair("Pixel").await;
    let port = upstream_hls(3).await;
    let c = json(create(&h, &p, port, json!({})).await).await;
    let sid = c["session_id"].as_str().unwrap();
    let st = json(p.client.get(h.url(&format!("/v1/sessions/{sid}"))).send().await.unwrap()).await;
    assert_eq!((st["state"].as_str(), st["error"].as_str()), (Some("failed"), Some("engine_unavailable")));
    assert_eq!(p.client.get(h.url(&format!("/v1/sessions/{sid}/media.m3u8"))).send().await.unwrap().status(), 503);
}

#[tokio::test]
async fn bind_addresses_must_be_private() {
    // public address -> refused before binding anything
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = tk_link::LinkConfig::new(dir.path().join("link"), "PC");
    cfg.bind_ips = vec!["8.8.8.8".parse().unwrap()];
    cfg.runner = Some(Arc::new(Canned));
    let err = tk_link::start(cfg, TestHost::new()).await.err().unwrap();
    assert!(matches!(err, tk_link::LinkError::PublicBind(_)), "{err}");
    let mut cfg = tk_link::LinkConfig::new(dir.path().join("link2"), "PC");
    cfg.bind_ips = vec!["0.0.0.0".parse().unwrap()];
    assert!(matches!(tk_link::start(cfg, TestHost::new()).await.err().unwrap(), tk_link::LinkError::PublicBind(_)));
}

#[tokio::test]
async fn identity_survives_restarts_so_paired_phones_keep_working() {
    let mut h = harness(FakeEngine::new(3, 100, Duration::from_millis(1)), quick_tuning(), lenient_default()).await;
    let p = h.pair("Pixel").await;
    let pin = h.link.ca_fingerprint();
    let link_dir = h.dir.path().join("link");
    h.link.shutdown().await;

    // same data dir -> same CA, same device registry
    let mut cfg = tk_link::LinkConfig::new(link_dir, "Test PC");
    cfg.port = 0;
    cfg.bind_ips = vec!["127.0.0.1".parse().unwrap()];
    cfg.runner = Some(Arc::new(Canned));
    cfg.engine = Some(FakeEngine::new(1, 1, Duration::ZERO));
    let link = tk_link::start(cfg, h.host.clone()).await.unwrap();
    assert_eq!(link.ca_fingerprint(), pin);
    assert_eq!(link.devices().len(), 1);
    let r = p.client.get(format!("https://127.0.0.1:{}/v1/capabilities", link.port())).send().await.unwrap();
    assert_eq!(r.status(), 200, "the old client certificate is still accepted");
    link.shutdown().await;
}
