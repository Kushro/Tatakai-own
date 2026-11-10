//! Loopback PUT receiver for FFmpeg's HLS muxer output: `PUT /sink/<session sink token>/<run generation>/<name>`.
//! Nothing here is reachable from the LAN (bound to 127.0.0.1); the token is a random capability.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use http_body_util::{BodyExt, Limited};
use hyper::body::Incoming;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;

use crate::sessions::{Manager, SinkError};

/// A single fMP4 segment (or init) larger than this is refused.
const MAX_OBJECT_BYTES: usize = 256 * 1024 * 1024;

pub async fn start(manager: Arc<Manager>) -> std::io::Result<(SocketAddr, tokio::task::JoinHandle<()>)> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let addr = listener.local_addr()?;
    *manager.sink_addr.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(addr);
    let task = tokio::spawn(async move {
        loop {
            let Ok((sock, _)) = listener.accept().await else { return };
            let manager = manager.clone();
            tokio::spawn(async move {
                let svc = hyper::service::service_fn(move |req| {
                    let manager = manager.clone();
                    async move { Ok::<_, std::convert::Infallible>(handle(manager, req).await.unwrap_or_else(status)) }
                });
                // FFmpeg's HLS muxer half-closes (shuts down its write side) right after the body and only then reads
                // the status; without `half_close` hyper takes that EOF as an abort and drops the request mid-handler,
                // so large segments (and every back-pressured one) were silently lost.
                let _ = hyper::server::conn::http1::Builder::new().half_close(true).serve_connection(TokioIo::new(sock), svc).await;
            });
        }
    });
    Ok((addr, task))
}

fn status(code: StatusCode) -> Response<Body> {
    let mut r = Response::new(Body::empty());
    *r.status_mut() = code;
    r
}

async fn handle(manager: Arc<Manager>, req: Request<Incoming>) -> Result<Response<Body>, StatusCode> {
    if req.method() != Method::PUT && req.method() != Method::POST {
        return Err(StatusCode::METHOD_NOT_ALLOWED);
    }
    let path = req.uri().path().to_owned();
    let mut seg = path.trim_start_matches('/').split('/');
    if seg.next() != Some("sink") {
        return Err(StatusCode::NOT_FOUND);
    }
    let token = seg.next().ok_or(StatusCode::NOT_FOUND)?;
    let run_gen: u32 = seg.next().and_then(|g| g.parse().ok()).ok_or(StatusCode::NOT_FOUND)?;
    let name = seg.next().ok_or(StatusCode::NOT_FOUND)?.to_owned();
    if seg.next().is_some() {
        return Err(StatusCode::NOT_FOUND);
    }
    let Some(session) = manager.by_sink_token(token) else {
        tracing::debug!(%name, "sink: unknown token");
        return Err(StatusCode::NOT_FOUND);
    };
    let body = Limited::new(req.into_body(), MAX_OBJECT_BYTES).collect().await.map_err(|e| { tracing::debug!(%name, error = %e, "sink: body failed"); StatusCode::PAYLOAD_TOO_LARGE })?.to_bytes();
    let len = body.len();
    let r = match manager.sink_put(&session, run_gen, &name, body).await {
        Ok(()) => Ok(status(StatusCode::OK)),
        Err(SinkError::BadName) => Err(StatusCode::BAD_REQUEST),
        Err(SinkError::Stale) => Err(StatusCode::CONFLICT),
    };
    tracing::debug!(%name, run_gen, len, status = r.as_ref().map_or_else(|c| c.as_u16(), |_| 200), "sink: put");
    r
}
