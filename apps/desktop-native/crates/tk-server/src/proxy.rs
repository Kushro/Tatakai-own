//! Media proxy `GET | HEAD /media/{ticket}/{*path}` + `POST /media/refresh` (worker **k**; classic
//! `server/src/proxy.mjs`, `cache.mjs`; checklist `server.api.resolve-proxy`, `server.cors-proxy-local`,
//! `server.proxy`): a resolve result whose stream needs headers (Referer, cookies, User-Agent) is rewritten to
//! `/media/<ticket>/<encoded url>`; the ticket (random id -> `{ url, headers, expires }`) lives in memory, the bytes
//! stream through `reqwest` with `Range` passthrough, HLS playlists are rewritten so segments go through the proxy
//! too, and small objects are cached under `<dir>/media-cache` (LRU by size, `ProxyStatusVm::max_cache_bytes`).
//! The same proxy serves mpv on the loopback when a direct stream needs headers mpv cannot send
//! (`server.cors-proxy-local`): `tk-app/src/live/server.rs` asks for a ticket and plays `http://127.0.0.1:<port>/...`.
//! Optional outbound proxy (HTTP / SOCKS5, `server.proxy`) and FlareSolverr are applied to the upstream client.

use std::path::Path;

/// Sub-directory of the server dir holding cached media.
pub const CACHE_DIR: &str = "media-cache";

/// Remove every cached object (best effort; a missing directory is fine).
pub async fn clear_cache(dir: &Path) {
    let cache = dir.join(CACHE_DIR);
    if let Err(e) = tokio::fs::remove_dir_all(&cache).await
        && e.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(error = %e, "could not clear the media cache");
    }
}
