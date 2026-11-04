//! Engine end to end on loopback, without the network: a torrent made in a temp dir over data that is already
//! there (DHT, LSD, UPnP and persistence off), streamed back through the HTTP server.

use std::time::Duration;

use librqbit::spawn_utils::BlockingSpawner;
use librqbit::{CreateTorrentOptions, create_torrent};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::{AddOptions, AddSource, EngineConfig, TorrentEngine, TorrentState};

const NAME: &str = "[Group] Show - 01.mkv";

fn content() -> Vec<u8> {
    (0..100_000u32).map(|i| (i.wrapping_mul(7) % 251) as u8).collect()
}

/// Minimal HTTP/1.1 GET (`Connection: close`): status, lowercase headers, body.
async fn http_get(url: &str, range: Option<&str>) -> (u16, Vec<(String, String)>, Vec<u8>) {
    let u = url::Url::parse(url).unwrap();
    let addr = format!("{}:{}", u.host_str().unwrap(), u.port().unwrap());
    let mut s = tokio::net::TcpStream::connect(addr).await.unwrap();
    let mut req = format!("GET {} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n", u.path());
    if let Some(r) = range {
        req.push_str(&format!("Range: {r}\r\n"));
    }
    req.push_str("\r\n");
    s.write_all(req.as_bytes()).await.unwrap();
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).await.unwrap();
    let split = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let head = String::from_utf8_lossy(&buf[..split]).into_owned();
    let body = buf[split + 4..].to_vec();
    let mut lines = head.lines();
    let status = lines.next().unwrap().split_whitespace().nth(1).unwrap().parse().unwrap();
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    (status, headers, body)
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn streams_a_local_torrent() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    let file = data.join(NAME);
    std::fs::write(&file, content()).unwrap();
    let torrent = create_torrent(&file, CreateTorrentOptions { piece_length: Some(16384), ..Default::default() }, &BlockingSpawner::new(1))
        .await
        .unwrap();
    let bytes = torrent.as_bytes().unwrap().to_vec();

    let mut cfg = EngineConfig::new(data.clone(), dir.path().join("state"));
    cfg.dht = false;
    cfg.lsd = false;
    cfg.upnp = false;
    cfg.persist_session = false;
    let engine = TorrentEngine::start(cfg).await.unwrap();

    let id = engine.add(AddSource::TorrentBytes(bytes.clone()), AddOptions::default()).await.unwrap();
    assert_eq!(id, torrent.info_hash().as_string());

    // The data is already on disk: the initial check finds every piece.
    let mut complete = false;
    for _ in 0..100 {
        let st = engine.stats(&id).unwrap();
        if st.state != TorrentState::Initializing && st.progress >= 1.0 {
            assert_eq!(st.pieces, st.pieces_have);
            complete = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(complete, "torrent never completed its initial check");

    let files = engine.files(&id).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].name, NAME);
    assert_eq!(files[0].size_bytes, content().len() as u64);
    assert_eq!(engine.piece_bitfield(&id, 0).unwrap(), vec![(0.0, 1.0)]);

    let url = engine.stream_url(&id, 0).unwrap();
    assert!(url.starts_with("http://127.0.0.1:"), "{url}");

    let (status, headers, body) = http_get(&url, None).await;
    assert_eq!(status, 200);
    assert_eq!(header(&headers, "content-type"), Some("video/x-matroska"));
    assert_eq!(header(&headers, "accept-ranges"), Some("bytes"));
    assert_eq!(body, content());

    let (status, headers, body) = http_get(&url, Some("bytes=10-19")).await;
    assert_eq!(status, 206);
    assert_eq!(header(&headers, "content-range"), Some("bytes 10-19/100000"));
    assert_eq!(body, content()[10..20].to_vec());

    let (status, _, _) = http_get(&url, Some("bytes=200000-")).await;
    assert_eq!(status, 416);

    let mut wrong = url::Url::parse(&url).unwrap();
    wrong.set_path(&format!("/not-the-token/{id}/0/x.mkv"));
    let (status, _, _) = http_get(wrong.as_str(), None).await;
    assert_eq!(status, 404);

    // Adding it again is the same torrent.
    assert_eq!(engine.add(AddSource::TorrentBytes(bytes), AddOptions::default()).await.unwrap(), id);
    assert_eq!(engine.list().len(), 1);

    engine.remove(&id, false).await.unwrap();
    assert!(engine.list().is_empty());
    assert!(file.exists(), "remove without delete_files keeps the data");
    engine.shutdown().await;
    engine.shutdown().await;
}
