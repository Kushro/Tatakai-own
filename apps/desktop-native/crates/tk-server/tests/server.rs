//! End-to-end checks of the native extension server (worker k): start on an ephemeral port with an empty
//! extension host, then `/health`, pairing, `/sources`, revocation, the viewer password and the log cursor.

use std::sync::Arc;

use serde_json::{Value, json};
use tk_server::{ServerConfig, start};

async fn server() -> (tempfile::TempDir, tk_server::ServerHandle, String) {
    let dir = tempfile::tempdir().unwrap();
    let ext = Arc::new(tk_ext::ExtensionHost::open(dir.path().join("ext")).unwrap());
    let mut cfg = ServerConfig::new(dir.path().join("server"), 0);
    cfg.loopback_only = true;
    cfg.name = "Test PC".into();
    let h = start(cfg, ext).await.unwrap();
    let base = format!("http://127.0.0.1:{}/api/server/v1", h.addr.port());
    (dir, h, base)
}

#[tokio::test]
async fn health_pairing_sources_and_revocation() {
    let (_dir, h, base) = server().await;
    let http = reqwest::Client::new();
    let health: Value = http.get(format!("{base}/health")).send().await.unwrap().json().await.unwrap();
    assert_eq!(health["success"], true);
    assert_eq!(health["data"]["protocolVersion"], 1);
    assert_eq!(health["data"]["pairingRequired"], true);
    assert_eq!(health["data"]["platform"], "desktop");
    assert_eq!(health["data"]["name"], "Test PC");

    // no token: 401 authentication_required
    let r = http.get(format!("{base}/sources")).send().await.unwrap();
    assert_eq!(r.status(), 401);

    let pairing = h.create_pairing(&format!("http://127.0.0.1:{}", h.addr.port())).unwrap();
    let paired: Value =
        http.post(format!("{base}/auth/pair")).json(&json!({ "code": pairing.code, "label": "Pixel" })).send().await.unwrap().json().await.unwrap();
    let token = paired["data"]["token"].as_str().unwrap().to_owned();
    let device = paired["data"]["deviceId"].as_str().unwrap().to_owned();
    // single use
    let again = http.post(format!("{base}/auth/pair")).json(&json!({ "code": pairing.code })).send().await.unwrap();
    assert_eq!(again.status(), 401);

    let sources: Value = http.get(format!("{base}/sources")).bearer_auth(&token).send().await.unwrap().json().await.unwrap();
    assert_eq!(sources["data"], json!([]));
    let exts: Value = http.get(format!("{base}/extensions")).bearer_auth(&token).send().await.unwrap().json().await.unwrap();
    assert_eq!(exts["data"], json!([]));
    assert_eq!(h.devices().len(), 1);

    let (_, cursor_before) = h.logs(0);
    let resolve = http
        .post(format!("{base}/resolve"))
        .bearer_auth(&token)
        .header("accept", "application/x-ndjson")
        .json(&json!({ "anilistId": 21, "title": "One Piece", "episode": 1 }))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let last: Value = serde_json::from_str(resolve.lines().last().unwrap()).unwrap();
    assert_eq!(last["done"], true);
    let (_, cursor_after) = h.logs(0);
    assert!(cursor_after > cursor_before, "every request is logged");

    h.revoke(&device).unwrap();
    let denied: Value = http.get(format!("{base}/sources")).bearer_auth(&token).send().await.unwrap().json().await.unwrap();
    assert_eq!(denied["error"]["code"], "device_not_authorized");
    h.stop().await;
}

#[tokio::test]
async fn viewer_password_gates_manual_login() {
    let (_dir, h, base) = server().await;
    let http = reqwest::Client::new();
    h.set_viewer_password(Some("a long viewer password")).unwrap();
    let health: Value = http.get(format!("{base}/health")).send().await.unwrap().json().await.unwrap();
    assert_eq!(health["data"]["accessPasswordRequired"], true);
    let bad = http.post(format!("{base}/auth/login")).json(&json!({ "role": "viewer", "password": "wrong password!" })).send().await.unwrap();
    assert_eq!(bad.status(), 401);
    let good: Value = http
        .post(format!("{base}/auth/login"))
        .json(&json!({ "role": "viewer", "password": "a long viewer password", "label": "Tablet" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(good["data"]["token"].as_str().is_some_and(|t| t.len() == 64));
    h.stop().await;
}

/// BUG-055: a paired phone downloads an installed extension as a `.kai` (manifest.json + bundle.js, byte for byte)
/// with the SHA-256 of its bundle, and `GET /extensions` carries the same hash.
#[tokio::test]
async fn installed_extensions_download_as_kai_packages() {
    use sha2::Digest;
    use std::io::Read;
    let dir = tempfile::tempdir().unwrap();
    let ext = Arc::new(tk_ext::ExtensionHost::open(dir.path().join("ext")).unwrap());
    let manifest = r#"{"id":"t.ext","name":"T","version":"1.0.2","type":"custom","description":"d","permissions":[]}"#;
    let bundle = "module.exports = { hello: 1 };";
    ext.install_unpacked(manifest, bundle).unwrap();
    let mut cfg = ServerConfig::new(dir.path().join("server"), 0);
    cfg.loopback_only = true;
    let h = start(cfg, ext.clone()).await.unwrap();
    let base = format!("http://127.0.0.1:{}/api/server/v1", h.addr.port());
    let http = reqwest::Client::new();

    // never without auth
    assert_eq!(http.get(format!("{base}/extensions/t.ext/package")).send().await.unwrap().status(), 401);
    let pairing = h.create_pairing(&format!("http://127.0.0.1:{}", h.addr.port())).unwrap();
    let paired: Value =
        http.post(format!("{base}/auth/pair")).json(&json!({ "code": pairing.code, "label": "Pixel" })).send().await.unwrap().json().await.unwrap();
    let token = paired["data"]["token"].as_str().unwrap().to_owned();

    let r = http.get(format!("{base}/extensions/t.ext/package")).bearer_auth(&token).send().await.unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["content-type"], "application/zip");
    assert_eq!(r.headers()["content-disposition"], "attachment; filename=\"t.ext.kai\"");
    let sha = r.headers()["x-tatakai-bundle-sha256"].to_str().unwrap().to_owned();
    let installed_bundle = std::fs::read(ext.root().join("t.ext").join("bundle.js")).unwrap();
    let installed_manifest = std::fs::read(ext.root().join("t.ext").join("manifest.json")).unwrap();
    let expected: String = sha2::Sha256::digest(&installed_bundle).iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(sha, expected);
    let body = r.bytes().await.unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(body.to_vec())).unwrap();
    assert_eq!(zip.len(), 2);
    let mut read = |name: &str| {
        let mut v = Vec::new();
        zip.by_name(name).unwrap().read_to_end(&mut v).unwrap();
        v
    };
    assert_eq!(read("manifest.json"), installed_manifest);
    assert_eq!(read("bundle.js"), installed_bundle);
    // it installs again as the same extension
    let other = tk_ext::ExtensionHost::open(dir.path().join("ext2")).unwrap();
    assert_eq!(other.install_kai_bytes(&body).unwrap().version, "1.0.2");

    let exts: Value = http.get(format!("{base}/extensions")).bearer_auth(&token).send().await.unwrap().json().await.unwrap();
    assert_eq!(exts["data"][0]["bundleSha256"], expected);

    // unknown ids and traversal attempts: 404 extension_not_found in the usual envelope
    // ".." (bare or %2E%2E) is normalised away by the URL: another route, 404 route_not_found
    for dots in ["..", "%2E%2E"] {
        assert_eq!(http.get(format!("{base}/extensions/{dots}/package")).bearer_auth(&token).send().await.unwrap().status(), 404);
    }
    for id in ["nope", "..%2F..%2Fserver", "t.ext%2F..%2F.."] {
        let r = http.get(format!("{base}/extensions/{id}/package")).bearer_auth(&token).send().await.unwrap();
        assert_eq!(r.status(), 404, "{id}");
        let v: Value = r.json().await.unwrap_or(json!({ "success": false, "error": { "code": "extension_not_found" } }));
        assert_eq!(v["success"], false, "{id}");
        assert_eq!(v["error"]["code"], "extension_not_found", "{id}");
    }
    h.stop().await;
}
