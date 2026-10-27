//! Conformance of the Rust host against the shared fixtures (`contracts/fixtures/extensions`).

use std::path::PathBuf;

use serde_json::{Value, json};
use tk_core::{Compat, Container, Media, MediaKind, MediaRef};
use tk_ext::{ExtError, ExtensionHost};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../contracts/fixtures/extensions")
}

fn expected(name: &str, file: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(fixtures().join(name).join(file)).unwrap()).unwrap()
}

fn host() -> (tempfile::TempDir, ExtensionHost) {
    let dir = tempfile::tempdir().unwrap();
    let host = ExtensionHost::open(dir.path().join("extensions")).unwrap();
    (dir, host)
}

fn media() -> Media {
    Media {
        media_ref: MediaRef::anilist(1),
        kind: MediaKind::Anime,
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

#[tokio::test]
async fn synthetic_full_matches_expected_stream_json() {
    let (_d, host) = host();
    let info = host.install_kai(&fixtures().join("packed/synthetic-full.kai")).unwrap();
    assert_eq!(info.id, "synthetic-full");
    assert_eq!(info.compat, Compat::Full);
    assert!(info.enabled);
    assert_eq!(host.get("synthetic-full").unwrap().permissions.len(), 2);

    let exp = expected("synthetic-full", "expected.stream.json");

    // providers
    let providers = host.call("synthetic-full", "providers", json!({})).await.unwrap();
    assert_eq!(providers["count"], exp["providers"]["count"]);
    let names: Vec<_> = providers["providers"].as_array().unwrap().iter().map(|p| p["name"].clone()).collect();
    assert_eq!(Value::Array(names), exp["providers"]["names"]);

    // stream
    let body = host
        .call("synthetic-full", "stream", json!({ "anilistId": 1, "episode": 3, "titles": ["Synthetic"], "stream": "0" }))
        .await
        .unwrap();
    assert_eq!(body["count"], exp["count"]);
    let status: Vec<_> = body["providerStatus"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| json!({ "provider": d["provider"], "status": d["status"], "resultCount": d["resultCount"] }))
        .collect();
    assert_eq!(Value::Array(status), exp["providerStatus"]);
    for (got, want) in body["sources"].as_array().unwrap().iter().zip(exp["sources"].as_array().unwrap()) {
        assert_eq!(got["providerName"], want["providerName"]);
        assert_eq!(got["type"], want["type"]);
        assert_eq!(got["quality"], want["quality"]);
        assert_eq!(got["url"], want["url"]);
        assert_eq!(got["providerPriority"], want["providerPriority"]);
        assert_eq!(got["audioLanguage"], want["audioLanguage"]);
        assert_eq!(got["headers"], want["headers"]);
        assert_eq!(got["subtitles"].as_array().unwrap().len() as u64, want["subtitleCount"].as_u64().unwrap());
    }
    assert_eq!(body["cached"], false);

    // second identical call is served from the result cache
    let again = host
        .call("synthetic-full", "stream", json!({ "anilistId": 1, "episode": 3, "titles": ["Synthetic"], "stream": "0" }))
        .await
        .unwrap();
    assert_eq!(again["cached"], true);

    // custom/home
    let home = host.call("synthetic-full", "custom/home", json!({ "sourceId": "demo" })).await.unwrap();
    assert_eq!(home["sections"].as_array().unwrap().len() as u64, exp["customHome"]["sections"].as_u64().unwrap());
    assert_eq!(home["sections"][0]["items"][0]["id"], exp["customHome"]["firstItemId"]);

    // permission-gated fetch
    let probe = host.invoke("synthetic-full", exp["networkGate"]["bundleMethod"].as_str().unwrap(), json!([])).await.unwrap();
    assert_eq!(probe, exp["networkGate"]["expectedMessage"]);
}

#[tokio::test]
async fn synthetic_full_resolves_playable_stream_sources() {
    let (_d, host) = host();
    host.install_kai(&fixtures().join("packed/synthetic-full.kai")).unwrap();
    let streams = host.resolve_streams(&media(), 3).await;
    assert_eq!(streams.len(), 3);
    assert_eq!(streams[0].container, Container::Hls);
    assert_eq!(streams[0].provider, "alpha");
    assert_eq!(streams[0].extension_id, "synthetic-full");
    assert_eq!(streams[0].quality.as_deref(), Some("1080p"));
    assert_eq!(streams[0].headers["Referer"], "https://alpha.example.invalid/");
    assert_eq!(streams[0].subtitles.len(), 1);
    assert_eq!(streams[0].subtitles[0].format, "vtt");
    assert_eq!(streams[0].subtitles[0].lang, "en");
    assert_eq!((streams[1].container, streams[2].container), (Container::Mp4, Container::Mp4));
    assert_eq!(streams[1].quality.as_deref(), Some("720p"));
    assert_eq!(streams[2].quality.as_deref(), Some("480p"));
}

#[tokio::test]
async fn gated_routes_and_unknown_routes() {
    let (_d, host) = host();
    host.install_kai(&fixtures().join("packed/synthetic-full.kai")).unwrap();
    // manifest.apiServer.routes = ["sources","stream"]: torrent is not enabled
    assert_eq!(
        host.call("synthetic-full", "torrent", json!({ "anilistId": 1 })).await.unwrap_err(),
        ExtError::RouteNotAllowed("torrent".into())
    );
    assert!(matches!(host.call("synthetic-full", "nope", json!({})).await, Err(ExtError::RouteNotFound(_))));
    assert!(matches!(host.call("missing", "health", json!({})).await, Err(ExtError::NotInstalled(_))));
    assert!(matches!(host.call("synthetic-full", "stream", json!({})).await, Err(ExtError::BadRequest(_))));
    assert!(matches!(host.call("synthetic-full", "providers/ghost", json!({})).await, Err(ExtError::NotFound(_))));
    assert!(matches!(host.call("synthetic-full", "manga/chapters", json!({})).await, Err(ExtError::NotImplemented(_))));
    let health = host.call("synthetic-full", "health", json!({})).await.unwrap();
    assert_eq!(health["ok"], true);
    assert_eq!(health["namespace"], "synthetic");
}

#[tokio::test]
async fn synthetic_fs_is_unsupported_and_stays_disabled() {
    let (_d, host) = host();
    let exp = expected("synthetic-fs", "expected.json");
    let info = host.install_kai(&fixtures().join("packed/synthetic-fs.kai")).unwrap();
    assert_eq!(info.compat, Compat::Unsupported);
    assert_eq!(exp["compat"], "unsupported");
    assert!(!info.enabled);
    assert_eq!(info.compat_reason.as_deref(), exp["reason"].as_str());
    assert_eq!(host.call("synthetic-fs", "stream", json!({ "anilistId": 1 })).await.unwrap_err(), ExtError::Unsupported);
    assert_eq!(host.set_enabled("synthetic-fs", true).unwrap_err(), ExtError::Unsupported);
    assert!(host.resolve_streams(&media(), 1).await.is_empty());
}

#[tokio::test]
async fn enable_disable_remove_and_reload_from_disk() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("extensions");
    {
        let host = ExtensionHost::open(&root).unwrap();
        host.install_kai(&fixtures().join("packed/synthetic-full.kai")).unwrap();
        host.set_enabled("synthetic-full", false).unwrap();
        assert!(matches!(host.call("synthetic-full", "health", json!({})).await, Err(ExtError::Disabled(_))));
    }
    // a new host sees the persisted state
    let host = ExtensionHost::open(&root).unwrap();
    let list = host.list();
    assert_eq!(list.len(), 1);
    assert!(!list[0].enabled);
    host.set_enabled("synthetic-full", true).unwrap();
    assert_eq!(host.call("synthetic-full", "health", json!({})).await.unwrap()["ok"], true);
    host.remove("synthetic-full").unwrap();
    assert!(host.list().is_empty());
    assert!(!root.join("synthetic-full").exists());
    assert!(matches!(host.remove("synthetic-full"), Err(ExtError::NotInstalled(_))));
    assert!(matches!(host.remove("../escape"), Err(ExtError::NotInstalled(_))));
}
