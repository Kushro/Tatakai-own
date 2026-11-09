# tk-link - dependency notes (B5)

No new workspace dependency is required. Notes for the integrator:

* `axum` (default-features off) provides routing/extractors only; connections are served with `hyper::server::conn::http1`
  over `tokio-rustls`, adapted with `hyper_util::service::TowerToHyperService` (no direct `tower` dependency).
* `tokio_util::sync::CancellationToken` and `tokio_util::io::ReaderStream` (feature `io`, already enabled) are used.
* mDNS (`mdns-sd`, already in the workspace) is used since wave B by `src/discovery.rs` (worker k).
* Unused-but-wired crates kept in Cargo.toml for later milestones: `ipnet`, `hickory-resolver`, `m3u8-rs`, `hmac`, `base32`.
