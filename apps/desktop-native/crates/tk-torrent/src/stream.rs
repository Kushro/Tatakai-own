//! Loopback HTTP streaming server: axum on `127.0.0.1:<EngineConfig::http_port>`, path
//! `/<token>/<info-hash>/<file index>/<name>`, `GET`/`HEAD` with `Range` (206 + `Content-Range`, `Accept-Ranges:
//! bytes`), `Content-Type` from the extension (`video/x-matroska`, `video/mp4`, `video/webm`), 404 for a wrong
//! token. Each `GET` opens a librqbit `FileStream` (seek = piece priority) and reports `TorrentEvent::StreamOpened` /
//! `StreamClosed` (through `StreamGuard`). Private to the crate: the public surface is `TorrentEngine::stream_url`.
//!
//! Never binds outside loopback; the random token keeps other local users / web pages from guessing URLs. External
//! mpv uses the same URL (same machine).

use std::io::SeekFrom;
use std::net::Ipv4Addr;
use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, Method, Response, StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::get;
use futures_util::StreamExt;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::net::TcpListener;
use tokio_util::io::ReaderStream;

use crate::engine::{Inner, file_name};

/// Read size of the response body (one librqbit read never crosses a piece anyway).
const CHUNK: usize = 64 * 1024;

/// Bind `127.0.0.1:<port>`; a taken port falls back to a free one.
pub(crate) async fn bind(port: u16) -> std::io::Result<TcpListener> {
    match TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await {
        Ok(l) => Ok(l),
        Err(e) if port != 0 => {
            tracing::warn!(port, error = %e, "torrent streaming port taken; using a free one");
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await
        }
        Err(e) => Err(e),
    }
}

/// Serve until the engine's cancellation token fires.
pub(crate) fn spawn_server(inner: Arc<Inner>, listener: TcpListener) {
    let cancel = inner.cancel.clone();
    let rt = inner.handle.clone();
    let app: Router = Router::new().route("/{token}/{hash}/{index}/{name}", get(serve_file)).with_state(inner);
    rt.spawn(async move {
        if let Err(e) = axum::serve(listener, app).with_graceful_shutdown(cancel.cancelled_owned()).await {
            tracing::warn!(error = %e, "torrent streaming server stopped");
        }
    });
}

fn finish(builder: axum::http::response::Builder, body: Body) -> Response<Body> {
    builder.body(body).unwrap_or_else(|e| {
        tracing::warn!(error = %e, "cannot build a stream response");
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    })
}

async fn serve_file(
    State(inner): State<Arc<Inner>>,
    Path((token, hash, index, _name)): Path<(String, String, usize, String)>,
    method: Method,
    headers: HeaderMap,
) -> Response<Body> {
    if token != inner.token {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Ok(handle) = inner.get(&hash) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let id = handle.info_hash().as_string();
    let Some(meta) = handle.metadata.load_full() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Some((len, mime)) =
        meta.file_infos.get(index).filter(|fi| !fi.attrs.padding).map(|fi| (fi.len, content_type(&file_name(fi))))
    else {
        return StatusCode::NOT_FOUND.into_response();
    };
    drop(meta);

    let range = parse_range(headers.get(header::RANGE).and_then(|v| v.to_str().ok()), len);
    let (status, start, end) = match range {
        Ok(Some((s, e))) => (StatusCode::PARTIAL_CONTENT, s, e),
        Ok(None) => (StatusCode::OK, 0, len),
        Err(()) => {
            let builder = Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{len}"))
                .header(header::ACCEPT_RANGES, "bytes");
            return finish(builder, Body::empty());
        }
    };
    let mut builder = Response::builder()
        .status(status)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::CONTENT_TYPE, mime)
        .header(header::CONTENT_LENGTH, end - start);
    if status == StatusCode::PARTIAL_CONTENT {
        builder = builder.header(header::CONTENT_RANGE, format!("bytes {start}-{}/{len}", end - 1));
    }
    if method == Method::HEAD || start == end {
        return finish(builder, Body::empty());
    }

    // A read selects the file so it keeps downloading past the stream's read-ahead window.
    if handle.only_files().is_some_and(|o| !o.contains(&index)) {
        let (bg, torrent) = (inner.clone(), handle.clone());
        inner.handle.spawn(async move {
            if let Err(e) = bg.select_files(&torrent, &[index]).await {
                tracing::debug!(error = %e, "cannot select the streamed file");
            }
        });
    }

    let mut reader = match handle.clone().stream(index).await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(%id, index, error = %format!("{e:#}"), "cannot open a torrent stream");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };
    if start > 0
        && let Err(e) = reader.seek(SeekFrom::Start(start)).await
    {
        tracing::warn!(%id, index, start, error = %e, "cannot seek a torrent stream");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    tracing::debug!(%id, index, start, end, "torrent stream opened");
    let open = inner.open_stream(&id, index);
    let body = ReaderStream::with_capacity(reader.take(end - start), CHUNK)
        .map(move |chunk| {
            // Dropped with the body: the stream counts as closed.
            let _open = &open;
            chunk
        })
        .take_until(inner.cancel.clone().cancelled_owned());
    finish(builder, Body::from_stream(body))
}

/// `Content-Type` from the file extension.
fn content_type(name: &str) -> &'static str {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "mkv" | "mka" => "video/x-matroska",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "avi" => "video/x-msvideo",
        "ts" | "m2ts" => "video/mp2t",
        "mov" => "video/quicktime",
        "ass" | "ssa" => "text/x-ssa",
        "srt" => "application/x-subrip",
        _ => "application/octet-stream",
    }
}

/// First range of a `Range: bytes=...` header as a half-open `[start, end)` byte range of a `len`-byte file.
/// `Ok(None)` = serve everything (no header, another unit, or a malformed header, which RFC 9110 says to ignore);
/// `Err(())` = 416.
fn parse_range(header: Option<&str>, len: u64) -> Result<Option<(u64, u64)>, ()> {
    let Some(value) = header else {
        return Ok(None);
    };
    let value = value.trim();
    let Some(spec) = value.get(..6).filter(|p| p.eq_ignore_ascii_case("bytes=")).and_then(|_| value.get(6..)) else {
        return Ok(None);
    };
    let first = spec.split(',').next().unwrap_or("").trim();
    let Some((a, b)) = first.split_once('-') else {
        return Ok(None);
    };
    let (a, b) = (a.trim(), b.trim());
    if a.is_empty() {
        // Suffix: the last `b` bytes.
        let Ok(n) = b.parse::<u64>() else {
            return Ok(None);
        };
        if n == 0 || len == 0 {
            return Err(());
        }
        return Ok(Some((len.saturating_sub(n), len)));
    }
    let Ok(start) = a.parse::<u64>() else {
        return Ok(None);
    };
    if start >= len {
        return Err(());
    }
    let end = if b.is_empty() {
        len
    } else {
        let Ok(last) = b.parse::<u64>() else {
            return Ok(None);
        };
        if last < start {
            return Ok(None);
        }
        last.saturating_add(1).min(len)
    };
    Ok(Some((start, end)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        assert_eq!(parse_range(None, 100), Ok(None));
        assert_eq!(parse_range(Some("bytes=0-"), 100), Ok(Some((0, 100))));
        assert_eq!(parse_range(Some("bytes=10-19"), 100), Ok(Some((10, 20))));
        assert_eq!(parse_range(Some("bytes=90-500"), 100), Ok(Some((90, 100))));
        assert_eq!(parse_range(Some("bytes=-10"), 100), Ok(Some((90, 100))));
        assert_eq!(parse_range(Some("bytes=-500"), 100), Ok(Some((0, 100))));
        assert_eq!(parse_range(Some("Bytes=5-6, 10-20"), 100), Ok(Some((5, 7))));
        assert_eq!(parse_range(Some("bytes=100-"), 100), Err(()));
        assert_eq!(parse_range(Some("bytes=-0"), 100), Err(()));
        assert_eq!(parse_range(Some("items=0-1"), 100), Ok(None));
        assert_eq!(parse_range(Some("bytes=abc"), 100), Ok(None));
        assert_eq!(parse_range(Some("bytes=20-10"), 100), Ok(None));
    }

    #[test]
    fn content_types() {
        assert_eq!(content_type("[SubsPlease] Show - 01 (1080p).MKV"), "video/x-matroska");
        assert_eq!(content_type("a.mp4"), "video/mp4");
        assert_eq!(content_type("a.webm"), "video/webm");
        assert_eq!(content_type("README"), "application/octet-stream");
    }
}
