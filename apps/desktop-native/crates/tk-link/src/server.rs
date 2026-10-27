//! HTTPS server: rustls (ring provider) with optional client certificates, axum routes, per-request device
//! authorization and hot revocation (live connections of a revoked device are dropped).

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use http_body_util::{BodyExt, Limited};
use hyper::Request;
use hyper::body::Incoming;
use hyper_util::rt::TokioIo;
use hyper_util::service::TowerToHyperService;
use rustls::RootCertStore;
use rustls::server::WebPkiClientVerifier;
use serde::Deserialize;
use serde_json::json;
use time::OffsetDateTime;
use tokio::net::TcpListener;
use tokio::sync::{Semaphore, broadcast};
use tokio_util::sync::CancellationToken;
use tokio_rustls::TlsAcceptor;

use crate::LinkEvent;
use crate::api::{ApiError, ApiResult, CONTRACT_HEADER, CONTRACT_VERSION, ErrorCode};
use crate::devices::DeviceRegistry;
use crate::identity::{Identity, sha256_hex};
use crate::media::capabilities::{BenchStore, Capabilities, Environment, build as build_caps};
use crate::pairing::{PairingError, PairingWindow};
use crate::sessions::{CreateRequest, Manager};

pub const MAX_CONNECTIONS: usize = 64;
pub const BODY_TIMEOUT: Duration = Duration::from_secs(10);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub struct AppState {
    pub identity: Arc<Identity>,
    pub devices: Arc<DeviceRegistry>,
    pub pairing: Arc<PairingWindow>,
    pub manager: Arc<Manager>,
    pub env: Arc<Environment>,
    pub bench: Arc<Mutex<BenchStore>>,
    pub host: Arc<dyn crate::LinkHost>,
    pub instance_id: String,
    pub server_name: String,
    pub client_fg_allowed: bool,
    pub events: broadcast::Sender<LinkEvent>,
}

impl AppState {
    pub fn capabilities(&self) -> Capabilities {
        let bench = self.bench.lock().unwrap_or_else(PoisonError::into_inner);
        build_caps(&self.env, &bench, &self.instance_id, &self.server_name, self.client_fg_allowed)
    }
}

/// Resolved at TLS handshake from the presented client certificate.
#[derive(Clone, Debug)]
struct ConnInfo {
    has_cert: bool,
    device_id: Option<String>,
}

#[derive(Clone)]
struct Authed(String);

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/v1/pairing", post(pairing))
        .route("/v1/capabilities", get(capabilities))
        .route("/v1/library", get(library))
        .route("/v1/devices/self", delete(forget_self))
        .route("/v1/sessions", post(create_session))
        .route("/v1/sessions/{id}", get(session_state).delete(delete_session))
        .route("/v1/sessions/{id}/position", post(position))
        .route("/v1/sessions/{id}/{file}", get(session_file))
        .fallback(not_found)
        .layer(middleware::from_fn_with_state(state.clone(), auth))
        .layer(middleware::from_fn(contract_header))
        .with_state(state)
}

async fn contract_header(req: Request<Body>, next: Next) -> Response {
    let mut resp = next.run(req).await;
    resp.headers_mut().insert(CONTRACT_HEADER, HeaderValue::from_static(CONTRACT_VERSION));
    resp
}

/// mTLS on every route except `POST /v1/pairing`; the active device is checked PER REQUEST.
async fn auth(State(st): State<AppState>, mut req: Request<Body>, next: Next) -> Response {
    if req.uri().path() == "/v1/pairing" {
        return next.run(req).await;
    }
    let Some(info) = req.extensions().get::<ConnInfo>().cloned() else { return ApiError(ErrorCode::Internal).into_response() };
    if !info.has_cert {
        return ApiError(ErrorCode::ClientCertificateRequired).into_response();
    }
    let Some(device) = info.device_id else { return ApiError(ErrorCode::DeviceNotAuthorized).into_response() };
    if !st.devices.is_active(&device, OffsetDateTime::now_utc().unix_timestamp()) {
        return ApiError(ErrorCode::DeviceNotAuthorized).into_response();
    }
    req.extensions_mut().insert(Authed(device));
    next.run(req).await
}

async fn not_found() -> ApiError {
    ErrorCode::SessionNotFound.into()
}

async fn read_json<T: serde::de::DeserializeOwned>(body: Body, limit: usize) -> ApiResult<T> {
    let collected = tokio::time::timeout(BODY_TIMEOUT, Limited::new(body, limit).collect())
        .await
        .map_err(|_| ApiError(ErrorCode::BodyTimeout))?
        .map_err(|e| if e.downcast_ref::<http_body_util::LengthLimitError>().is_some() { ApiError(ErrorCode::BodyTooLarge) } else { ApiError(ErrorCode::RequestInvalid) })?;
    serde_json::from_slice(&collected.to_bytes()).map_err(|_| ErrorCode::RequestInvalid.into())
}

// ---- handlers ------------------------------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PairingBody {
    code: String,
    device_name: String,
    csr_pem: String,
    platform: String,
}

async fn pairing(State(st): State<AppState>, body: Body) -> ApiResult<Response> {
    let b: PairingBody = read_json(body, 16 * 1024).await?;
    let name_len = b.device_name.chars().count();
    if b.platform != "android" || name_len == 0 || name_len > 64 || b.csr_pem.len() > 8192 || b.code.len() > 32 {
        return Err(ErrorCode::RequestInvalid.into());
    }
    let issued = st
        .pairing
        .redeem(&b.code, &b.csr_pem, &b.device_name, OffsetDateTime::now_utc())
        .map_err(|e: PairingError| ApiError(e.code()))?;
    let _ = st.events.send(LinkEvent::DevicePaired { device_id: issued.device_id.clone(), name: crate::pairing::sanitize_device_name(&b.device_name) });
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "device_id": issued.device_id,
            "client_cert_pem": issued.client_cert_pem,
            "ca_cert_pem": issued.ca_cert_pem,
            "server_name": st.server_name,
        })),
    )
        .into_response())
}

async fn capabilities(State(st): State<AppState>) -> Json<Capabilities> {
    Json(st.capabilities())
}

async fn library(State(st): State<AppState>) -> Json<serde_json::Value> {
    let host = st.host.clone();
    let items = tokio::task::spawn_blocking(move || host.library()).await.unwrap_or_default();
    let truncated = items.len() > crate::library::MAX_ITEMS;
    let items: Vec<_> = items.into_iter().take(crate::library::MAX_ITEMS).collect();
    Json(json!({ "items": items, "truncated": truncated }))
}

async fn forget_self(State(st): State<AppState>, axum::Extension(Authed(device)): axum::Extension<Authed>) -> StatusCode {
    st.devices.revoke(&device, OffsetDateTime::now_utc().unix_timestamp());
    st.manager.close_for_device(&device);
    let _ = st.events.send(LinkEvent::DeviceRevoked { device_id: device });
    StatusCode::NO_CONTENT
}

async fn create_session(State(st): State<AppState>, axum::Extension(Authed(device)): axum::Extension<Authed>, body: Body) -> ApiResult<Response> {
    let req: CreateRequest = read_json(body, 4 * 1024).await?;
    let created = st.manager.create(&device, req).await?;
    Ok((StatusCode::CREATED, Json(created)).into_response())
}

async fn session_state(State(st): State<AppState>, axum::Extension(Authed(device)): axum::Extension<Authed>, Path(id): Path<String>) -> ApiResult<Json<serde_json::Value>> {
    let s = st.manager.owned(&id, &device)?;
    Ok(Json(st.manager.state_json(&s, &st.instance_id)))
}

async fn delete_session(State(st): State<AppState>, axum::Extension(Authed(device)): axum::Extension<Authed>, Path(id): Path<String>) -> ApiResult<StatusCode> {
    let s = st.manager.owned(&id, &device)?;
    st.manager.close(&s.id);
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HeartbeatBody {
    position_secs: f64,
    paused: bool,
}

async fn position(State(st): State<AppState>, axum::Extension(Authed(device)): axum::Extension<Authed>, Path(id): Path<String>, body: Body) -> ApiResult<StatusCode> {
    let hb: HeartbeatBody = read_json(body, 1024).await?;
    if !hb.position_secs.is_finite() || hb.position_secs < 0.0 {
        return Err(ErrorCode::RequestInvalid.into());
    }
    let s = st.manager.owned(&id, &device)?;
    st.manager.heartbeat(&s, hb.position_secs, hb.paused);
    Ok(StatusCode::NO_CONTENT)
}

fn with_type(body: impl Into<Body>, ct: &'static str, no_store: bool) -> Response {
    let mut r = Response::new(body.into());
    r.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static(ct));
    if no_store {
        r.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    r
}

async fn session_file(State(st): State<AppState>, axum::Extension(Authed(device)): axum::Extension<Authed>, Path((id, file)): Path<(String, String)>) -> ApiResult<Response> {
    let s = st.manager.owned(&id, &device)?;
    // heartbeat-less clients still keep the session alive while they fetch media
    match file.as_str() {
        "master.m3u8" => Ok(with_type(st.manager.master_playlist(&s)?, "application/vnd.apple.mpegurl", true)),
        "media.m3u8" => Ok(with_type(st.manager.media_playlist(&s)?, "application/vnd.apple.mpegurl", true)),
        "init.mp4" => Ok(with_type(st.manager.init_segment(&s).await?, "video/mp4", false)),
        f => {
            let k = f.strip_prefix("seg_").and_then(|r| r.strip_suffix(".m4s")).and_then(|k| k.parse::<u32>().ok()).ok_or(ApiError(ErrorCode::SessionNotFound))?;
            Ok(with_type(st.manager.segment(&s, k).await?, "video/iso.segment", false))
        }
    }
}

// ---- TLS + connection loop ------------------------------------------------------------------------------------------

pub fn tls_config(identity: &Identity) -> Result<Arc<rustls::ServerConfig>, rustls::Error> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut roots = RootCertStore::empty();
    roots.add(identity.ca_der().clone())?;
    let verifier = WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider.clone())
        .allow_unauthenticated()
        .build()
        .map_err(|e| rustls::Error::General(e.to_string()))?;
    let mut cfg = rustls::ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])?
        .with_client_cert_verifier(verifier)
        .with_single_cert(identity.server_chain(), identity.server_key())?;
    cfg.alpn_protocols = vec![b"http/1.1".to_vec()];
    cfg.session_storage = Arc::new(rustls::server::NoServerSessionStorage {});
    cfg.send_tls13_tickets = 0;
    Ok(Arc::new(cfg))
}

struct ConnService {
    inner: TowerToHyperService<Router>,
    info: ConnInfo,
}

impl hyper::service::Service<Request<Incoming>> for ConnService {
    type Response = Response;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Response, Infallible>> + Send>>;

    fn call(&self, mut req: Request<Incoming>) -> Self::Future {
        req.extensions_mut().insert(self.info.clone());
        Box::pin(hyper::service::Service::call(&self.inner, req))
    }
}

/// Accept loop for one listener. Runs until `shutdown` is notified.
pub async fn serve(listener: TcpListener, tls: Arc<rustls::ServerConfig>, state: AppState, limit: Arc<Semaphore>, shutdown: CancellationToken) {
    let acceptor = TlsAcceptor::from(tls);
    let app = router(state.clone());
    loop {
        let (sock, _peer) = tokio::select! {
            r = listener.accept() => match r { Ok(v) => v, Err(_) => continue },
            () = shutdown.cancelled() => return,
        };
        let Ok(permit) = limit.clone().try_acquire_owned() else {
            drop(sock); // 64 connections max
            continue;
        };
        let acceptor = acceptor.clone();
        let app = app.clone();
        let devices = state.devices.clone();
        let shutdown = shutdown.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let Ok(Ok(tls)) = tokio::time::timeout(HANDSHAKE_TIMEOUT, acceptor.accept(sock)).await else { return };
            let info = {
                let (_, conn) = tls.get_ref();
                match conn.peer_certificates().and_then(|c| c.first()) {
                    Some(leaf) => ConnInfo { has_cert: true, device_id: devices.by_fingerprint(&sha256_hex(leaf)).map(|d| d.id) },
                    None => ConnInfo { has_cert: false, device_id: None },
                }
            };
            let mut revoked = devices.subscribe_revocations();
            let svc = ConnService { inner: TowerToHyperService::new(app), info: info.clone() };
            let conn = hyper::server::conn::http1::Builder::new().keep_alive(true).serve_connection(TokioIo::new(tls), svc);
            tokio::pin!(conn);
            loop {
                tokio::select! {
                    _ = &mut conn => return,
                    msg = revoked.recv() => match msg {
                        Ok(id) if info.device_id.as_deref() == Some(id.as_str()) => return, // hot revocation: drop the connection
                        Err(broadcast::error::RecvError::Closed) => std::future::pending::<()>().await,
                        _ => {}
                    },
                    () = shutdown.cancelled() => { conn.as_mut().graceful_shutdown(); }
                }
            }
        });
    }
}
