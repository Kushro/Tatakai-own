//! Loopback HTTP streaming server (wave 3 A): axum on `127.0.0.1:<EngineConfig::http_port>`, path
//! `/<token>/<info-hash>/<file index>/<name>`, `GET`/`HEAD` with `Range` (206 + `Content-Range`, `Accept-Ranges:
//! bytes`), `Content-Type` from the extension (`video/x-matroska`, `video/mp4`), 404 for a wrong token. Each request
//! opens a librqbit `FileStream` (seek = piece priority) and reports `TorrentEvent::StreamOpened` / `StreamClosed`.
//! Private to the crate: the public surface is `TorrentEngine::stream_url`.
