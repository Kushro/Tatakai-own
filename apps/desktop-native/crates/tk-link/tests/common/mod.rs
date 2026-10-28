//! Shared test harness: loopback stubs, a fake FFmpeg engine, a test host and a paired mTLS client.
#![allow(dead_code)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use rcgen::{CertificateParams, KeyPair, PKCS_ECDSA_P256_SHA256};
use tk_core::net::{Policy, Resolver, SafeHttp};
use tk_link::media::capabilities::CommandRunner;
use tk_link::media::engine::{Engine, EngineExit, EngineJob, EngineProcess};
use tk_link::{LibraryItem, LinkConfig, LinkHandle, LinkHost, SessionTuning};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub type Seen = Arc<Mutex<Vec<String>>>;

/// Loopback HTTP stub. `responder(raw_request) -> (status, headers, body)`. One request per connection.
pub async fn stub(responder: impl Fn(&str) -> (u16, Vec<(String, String)>, Vec<u8>) + Send + Sync + 'static) -> (u16, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen: Seen = Arc::default();
    let seen2 = seen.clone();
    let responder = Arc::new(responder);
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = listener.accept().await else { return };
            let responder = responder.clone();
            let seen = seen2.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 16 * 1024];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                seen.lock().unwrap().push(req.clone());
                let (status, headers, body) = responder(&req);
                let mut head = format!("HTTP/1.1 {status} X\r\ncontent-length: {}\r\nconnection: close\r\n", body.len());
                for (k, v) in headers {
                    head.push_str(&format!("{k}: {v}\r\n"));
                }
                head.push_str("\r\n");
                let _ = s.write_all(head.as_bytes()).await;
                let _ = s.write_all(&body).await;
            });
        }
    });
    (port, seen)
}

pub fn text(status: u16, body: &str) -> (u16, Vec<(String, String)>, Vec<u8>) {
    (status, vec![("content-type".into(), "application/vnd.apple.mpegurl".into())], body.as_bytes().to_vec())
}

pub fn redirect(to: &str) -> (u16, Vec<(String, String)>, Vec<u8>) {
    (302, vec![("location".into(), to.into())], Vec::new())
}

pub fn request_path(req: &str) -> String {
    req.lines().next().and_then(|l| l.split_whitespace().nth(1)).unwrap_or("").to_owned()
}

/// Maps host names to fixed addresses (no real DNS in tests).
pub struct MapResolver(pub HashMap<&'static str, Vec<std::net::IpAddr>>);

#[async_trait]
impl Resolver for MapResolver {
    async fn resolve(&self, host: &str, _port: u16) -> std::io::Result<Vec<std::net::IpAddr>> {
        self.0.get(host).cloned().ok_or_else(|| std::io::Error::other("nxdomain"))
    }
}

/// A SafeHttp that behaves like the production one except that loopback is reachable on any port.
pub fn lenient_http(resolver: Arc<dyn Resolver>) -> SafeHttp {
    let policy = Policy { allow_loopback: true, allowed_ports: None, ..Policy::link_source(true) };
    SafeHttp::with_resolver(policy, resolver)
}

pub fn lenient_default() -> SafeHttp {
    lenient_http(Arc::new(MapResolver(HashMap::new())))
}

pub fn no_proxy_client() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().build().unwrap()
}

// ---- fake FFmpeg ------------------------------------------------------------------------------------------------------

pub struct FakeEngine {
    pub total_segments: u32,
    pub seg_bytes: usize,
    pub delay: Duration,
    pub starts: Arc<Mutex<Vec<u32>>>,
    pub args: Arc<Mutex<Vec<Vec<String>>>>,
    pub put_results: Arc<Mutex<Vec<(u32, u16)>>>,
}

impl FakeEngine {
    pub fn new(total_segments: u32, seg_bytes: usize, delay: Duration) -> Arc<Self> {
        Arc::new(Self { total_segments, seg_bytes, delay, starts: Arc::default(), args: Arc::default(), put_results: Arc::default() })
    }
}

struct FakeProc {
    task: Option<tokio::task::JoinHandle<()>>,
}

#[async_trait]
impl EngineProcess for FakeProc {
    async fn wait(&mut self) -> EngineExit {
        match self.task.take() {
            Some(t) => match t.await {
                Ok(()) => EngineExit::Success,
                Err(_) => EngineExit::Failed,
            },
            None => EngineExit::Failed,
        }
    }

    fn kill(&mut self) {
        if let Some(t) = &self.task {
            t.abort();
        }
    }
}

#[async_trait]
impl Engine for FakeEngine {
    async fn spawn(&self, job: EngineJob) -> std::io::Result<Box<dyn EngineProcess>> {
        self.starts.lock().unwrap().push(job.start_segment);
        self.args.lock().unwrap().push(job.args.iter().map(|a| a.to_string_lossy().into_owned()).collect());
        let (total, bytes, delay, results) = (self.total_segments, self.seg_bytes, self.delay, self.put_results.clone());
        let task = tokio::spawn(async move {
            let client = no_proxy_client();
            let put = |name: String, body: Vec<u8>| {
                let client = client.clone();
                let url = format!("{}/{name}", job.sink_base);
                async move { client.put(url).body(body).send().await.map(|r| r.status().as_u16()).unwrap_or(0) }
            };
            if put("init.mp4".into(), b"INIT".to_vec()).await != 200 {
                return;
            }
            for k in job.start_segment..total {
                let st = put(format!("seg_{k}.m4s"), vec![(k % 251) as u8; bytes]).await;
                results.lock().unwrap().push((k, st));
                if st != 200 {
                    return; // stale generation or session closed
                }
                tokio::time::sleep(delay).await;
            }
        });
        Ok(Box::new(FakeProc { task: Some(task) }))
    }
}

// ---- host + canned ffmpeg detection ----------------------------------------------------------------------------------

pub struct TestHost {
    pub files: Mutex<HashMap<String, PathBuf>>,
    pub extension_perms: Mutex<HashMap<String, Vec<String>>>,
}

impl TestHost {
    pub fn new() -> Arc<Self> {
        Arc::new(Self { files: Mutex::default(), extension_perms: Mutex::default() })
    }
}

impl LinkHost for TestHost {
    fn library(&self) -> Vec<LibraryItem> {
        self.files
            .lock()
            .unwrap()
            .iter()
            .map(|(id, p)| LibraryItem { id: id.clone(), title: p.file_stem().unwrap().to_string_lossy().into_owned(), duration_secs: 6.0, width: 1280, height: 720, size_bytes: 10 })
            .collect()
    }

    fn library_path(&self, id: &str) -> Option<PathBuf> {
        self.files.lock().unwrap().get(id).cloned()
    }

    fn ffmpeg_path(&self) -> Option<PathBuf> {
        Some(PathBuf::from("ffmpeg-for-tests"))
    }

    fn extension_permissions(&self, id: &str) -> Option<Vec<String>> {
        self.extension_perms.lock().unwrap().get(id).cloned()
    }
}

pub struct Canned;

#[async_trait]
impl CommandRunner for Canned {
    async fn run(&self, _p: &std::path::Path, args: &[&str]) -> std::io::Result<String> {
        Ok(if args.contains(&"-encoders") {
            " V....D h264_nvenc           NVIDIA NVENC H.264 encoder\n V....D libopenh264          OpenH264\n"
        } else {
            " ... libplacebo        VV->V      libplacebo\n"
        }
        .to_owned())
    }
}

// ---- harness ---------------------------------------------------------------------------------------------------------

pub struct Harness {
    pub link: LinkHandle,
    pub dir: tempfile::TempDir,
    pub host: Arc<TestHost>,
    pub engine: Arc<FakeEngine>,
    pub base: String,
    pub ca_pem: String,
}

pub fn quick_tuning() -> SessionTuning {
    SessionTuning { segment_wait: Duration::from_secs(5), ..SessionTuning::default() }
}

pub async fn harness(engine: Arc<FakeEngine>, tuning: SessionTuning, http: SafeHttp) -> Harness {
    harness_with(engine, tuning, http, |_| {}).await
}

pub async fn harness_with(engine: Arc<FakeEngine>, tuning: SessionTuning, http: SafeHttp, tweak: impl FnOnce(&mut LinkConfig)) -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let host = TestHost::new();
    let mut cfg = LinkConfig::new(dir.path().join("link"), "Test PC");
    cfg.port = 0;
    cfg.bind_ips = vec!["127.0.0.1".parse().unwrap()];
    cfg.tuning = tuning;
    cfg.engine = Some(engine.clone());
    cfg.runner = Some(Arc::new(Canned));
    cfg.fetch_http = Some(http);
    cfg.allow_http_sources = true;
    tweak(&mut cfg);
    let link = tk_link::start(cfg, host.clone()).await.unwrap();
    let base = format!("https://127.0.0.1:{}", link.port());
    // the CA is whatever the server hands out; tests re-read it from the pairing response
    Harness { link, dir, host, engine, base, ca_pem: String::new() }
}

pub struct Paired {
    pub client: reqwest::Client,
    pub device_id: String,
}

impl Harness {
    pub fn anonymous_client(&self) -> reqwest::Client {
        // trusts nothing: only used for requests that must fail at TLS level
        reqwest::Client::builder().no_proxy().build().unwrap()
    }

    /// Pair a new device and return an mTLS client trusting the server CA.
    pub async fn pair(&mut self, name: &str) -> Paired {
        let payload = self.link.open_pairing_window();
        let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256).unwrap();
        let csr = CertificateParams::default().serialize_request(&key).unwrap().pem().unwrap();
        // First contact: no client cert; trust the CA that the payload pins (compare its fingerprint after the fact).
        let bootstrap = reqwest::Client::builder().no_proxy().danger_accept_invalid_certs(true).build().unwrap();
        let resp = bootstrap
            .post(format!("{}/v1/pairing", self.base))
            .json(&serde_json::json!({ "code": payload.code, "device_name": name, "csr_pem": csr, "platform": "android" }))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 201, "pairing failed");
        assert_eq!(resp.headers().get("tatakai-link").unwrap(), "v1");
        let body: serde_json::Value = resp.json().await.unwrap();
        let ca_pem = body["ca_cert_pem"].as_str().unwrap().to_owned();
        self.ca_pem = ca_pem.clone();
        let identity = reqwest::Identity::from_pem(format!("{}\n{}", body["client_cert_pem"].as_str().unwrap(), key.serialize_pem()).as_bytes()).unwrap();
        let client = reqwest::Client::builder()
            .no_proxy()
            .tls_built_in_root_certs(false)
            .add_root_certificate(reqwest::Certificate::from_pem(ca_pem.as_bytes()).unwrap())
            .identity(identity)
            .build()
            .unwrap();
        Paired { client, device_id: body["device_id"].as_str().unwrap().to_owned() }
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }
}

/// HLS media playlist with `n` 2-second segments.
pub fn media_playlist(n: u32) -> String {
    let mut p = String::from("#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:2\n");
    for k in 0..n {
        p.push_str(&format!("#EXTINF:2.0,\nseg{k}.ts\n"));
    }
    p.push_str("#EXT-X-ENDLIST\n");
    p
}
