//! F6 (parity wave B): owner routes, handle API and error envelope of the native extension server, on an ephemeral
//! loopback port with an empty extension host (same harness as `server.rs`).

use std::sync::Arc;

use serde_json::{Value, json};
use tk_server::{ServerConfig, ServerError, start};

async fn server_in(dir: &std::path::Path, port: u16) -> Result<tk_server::ServerHandle, ServerError> {
    let ext = Arc::new(tk_ext::ExtensionHost::open(dir.join("ext")).unwrap());
    let mut cfg = ServerConfig::new(dir.join("server"), port);
    cfg.loopback_only = true;
    cfg.name = "Test PC".into();
    start(cfg, ext).await
}

fn base(h: &tk_server::ServerHandle) -> String {
    format!("http://127.0.0.1:{}/api/server/v1", h.addr.port())
}

fn bootstrap(dir: &std::path::Path) -> String {
    let raw = std::fs::read_to_string(dir.join("server").join("state.json")).unwrap();
    let v: Value = serde_json::from_str(&raw).unwrap();
    v["bootstrap"].as_str().unwrap().to_owned()
}

#[tokio::test]
async fn handle_reports_status_and_local_urls() {
    let dir = tempfile::tempdir().unwrap();
    let h = server_in(dir.path(), 0).await.unwrap();
    let port = h.addr.port();
    assert_ne!(port, 0);
    assert!(h.addr.ip().is_loopback());
    let st = h.status();
    assert_eq!(st.devices, 0);
    assert!(st.server_id.is_some_and(|id| !id.is_empty()));
    assert!(h.pairing_required());
    assert!(h.lan_urls().is_empty(), "loopback only: nothing for phones");
    assert_eq!(h.local_media_base(), format!("http://127.0.0.1:{port}/api/server/v1/media/"));
    let url = h.local_media_url("https://cdn.example/v/index.m3u8", &[("Referer".into(), "https://x/".into())]);
    assert!(url.starts_with(&h.local_media_base()), "{url}");
    assert_eq!(h.config().port, port, "config carries the bound port");
    // a viewer password lifts the pairing requirement
    h.set_viewer_password(Some("a long viewer password")).unwrap();
    assert!(h.viewer_password_set());
    assert!(!h.pairing_required());
    h.stop().await;
}

#[tokio::test]
async fn a_busy_port_is_a_bind_error_and_stop_frees_it() {
    let dir = tempfile::tempdir().unwrap();
    let h = server_in(dir.path(), 0).await.unwrap();
    let port = h.addr.port();
    let other = tempfile::tempdir().unwrap();
    assert!(matches!(server_in(other.path(), port).await, Err(ServerError::Bind { .. })));
    h.stop().await;
    let again = server_in(other.path(), port).await.expect("port free after stop");
    again.stop().await;
}

#[tokio::test]
async fn setup_needs_the_bootstrap_secret_and_owner_routes_need_an_admin() {
    let dir = tempfile::tempdir().unwrap();
    let h = server_in(dir.path(), 0).await.unwrap();
    let base = base(&h);
    let http = reqwest::Client::new();

    let denied = http.post(format!("{base}/setup")).send().await.unwrap();
    assert_eq!(denied.status(), 403);
    let body: Value = denied.json().await.unwrap();
    assert_eq!(body["success"], false);
    assert_eq!(body["error"]["code"], "bootstrap_required");

    let owner: Value =
        http.post(format!("{base}/setup")).header("X-Tatakai-Bootstrap", bootstrap(dir.path())).send().await.unwrap().json().await.unwrap();
    assert_eq!(owner["data"]["role"], "admin");
    let admin = owner["data"]["token"].as_str().unwrap().to_owned();
    let admin_id = owner["data"]["deviceId"].as_str().unwrap().to_owned();

    // the owner mints a pairing code over HTTP; a phone pairs with it
    let pairing: Value = http.post(format!("{base}/auth/pairing")).bearer_auth(&admin).send().await.unwrap().json().await.unwrap();
    assert_eq!(pairing["data"]["expiresIn"], 300);
    let code = pairing["data"]["code"].as_str().unwrap().to_owned();
    assert!(pairing["data"]["deepLink"].as_str().unwrap().starts_with("tatakai://extensionserver/connect?server="));
    let paired: Value = http.post(format!("{base}/auth/pair")).json(&json!({ "code": code, "label": "Pixel" })).send().await.unwrap().json().await.unwrap();
    let viewer = paired["data"]["token"].as_str().unwrap().to_owned();
    assert_eq!(paired["data"]["role"], "viewer");

    // a viewer token cannot reach owner routes
    let forbidden = http.get(format!("{base}/devices")).bearer_auth(&viewer).send().await.unwrap();
    assert_eq!(forbidden.status(), 403);
    let body: Value = forbidden.json().await.unwrap();
    assert_eq!(body["error"]["code"], "forbidden");

    // the owner sees both, itself marked current
    let devices: Value = http.get(format!("{base}/devices")).bearer_auth(&admin).send().await.unwrap().json().await.unwrap();
    let rows = devices["data"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    let me = rows.iter().find(|r| r["id"] == admin_id.as_str()).unwrap();
    assert_eq!(me["current"], true);
    assert!(rows.iter().any(|r| r["label"] == "Pixel" && r["current"] == false));
    // only viewer devices count for the status
    assert_eq!(h.status().devices, 1);
    h.stop().await;
}

#[tokio::test]
async fn logout_revokes_the_calling_token() {
    let dir = tempfile::tempdir().unwrap();
    let h = server_in(dir.path(), 0).await.unwrap();
    let base = base(&h);
    let http = reqwest::Client::new();
    let p = h.create_pairing("http://127.0.0.1").unwrap();
    let paired: Value = http.post(format!("{base}/auth/pair")).json(&json!({ "code": p.code })).send().await.unwrap().json().await.unwrap();
    let token = paired["data"]["token"].as_str().unwrap().to_owned();
    assert_eq!(h.devices().len(), 1);
    assert_eq!(h.devices()[0].label, "Device", "missing label");
    let out = http.post(format!("{base}/auth/logout")).bearer_auth(&token).send().await.unwrap();
    assert_eq!(out.status(), 200);
    assert!(h.devices().is_empty());
    let after = http.get(format!("{base}/sources")).bearer_auth(&token).send().await.unwrap();
    assert_eq!(after.status(), 401);
    h.stop().await;
}

#[tokio::test]
async fn numeric_codes_pair_and_bad_codes_are_throttled() {
    let dir = tempfile::tempdir().unwrap();
    let h = server_in(dir.path(), 0).await.unwrap();
    let base = base(&h);
    let http = reqwest::Client::new();
    // a code sent as a JSON number still pairs (leading zeros aside)
    let mut p = h.create_pairing("http://127.0.0.1").unwrap();
    while p.code.starts_with('0') {
        p = h.create_pairing("http://127.0.0.1").unwrap();
    }
    let n: u32 = p.code.parse().unwrap();
    let ok = http.post(format!("{base}/auth/pair")).json(&json!({ "code": n })).send().await.unwrap();
    assert_eq!(ok.status(), 200);

    for _ in 0..tk_server::state::MAX_ATTEMPTS {
        let r = http.post(format!("{base}/auth/pair")).json(&json!({ "code": "nope" })).send().await.unwrap();
        assert_eq!(r.status(), 401);
        let body: Value = r.json().await.unwrap();
        assert_eq!(body["error"]["code"], "pairing_expired");
    }
    let throttled = http.post(format!("{base}/auth/pair")).json(&json!({ "code": "nope" })).send().await.unwrap();
    assert_eq!(throttled.status(), 429);
    let body: Value = throttled.json().await.unwrap();
    assert_eq!(body["error"]["code"], "login_throttled");
    h.stop().await;
}

#[tokio::test]
async fn malformed_bodies_unknown_routes_and_native_use_the_envelope() {
    let dir = tempfile::tempdir().unwrap();
    let h = server_in(dir.path(), 0).await.unwrap();
    let base = base(&h);
    let http = reqwest::Client::new();
    let bad = http.post(format!("{base}/auth/pair")).header("content-type", "application/json").body("{ nope").send().await.unwrap();
    assert_eq!(bad.status(), 400);
    let body: Value = bad.json().await.unwrap();
    assert_eq!(body["error"]["code"], "invalid_json");

    let missing = http.get(format!("{base}/does-not-exist")).send().await.unwrap();
    assert_eq!(missing.status(), 404);
    let body: Value = missing.json().await.unwrap();
    assert_eq!(body["success"], false);
    assert_eq!(body["error"]["code"], "route_not_found");

    let native = http.post(format!("{base}/native")).send().await.unwrap();
    assert_eq!(native.status(), 404);

    // a viewer login without a viewer password asks for pairing
    let login = http.post(format!("{base}/auth/login")).json(&json!({ "role": "viewer", "password": "whatever long" })).send().await.unwrap();
    assert_eq!(login.status(), 401);
    let body: Value = login.json().await.unwrap();
    assert_eq!(body["error"]["code"], "pairing_required");
    h.stop().await;
}
