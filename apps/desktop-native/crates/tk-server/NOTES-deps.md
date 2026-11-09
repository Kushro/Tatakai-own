# tk-server: dependencies requested by worker k (wave B)

Nothing new to add as a crate. One feature of a crate already in the workspace:

| Crate | Change | Why |
|---|---|---|
| `reqwest` 0.12 (workspace) | add the feature `socks` to `[workspace.dependencies] reqwest` (`features = ["rustls-tls", "json", "stream", "socks"]`) | `server.proxy`: the outbound proxy of the media proxy accepts `socks5://` / `socks5h://` (`upstream::Upstream`, `upstream::probe_proxy`). Without the feature `reqwest::Proxy::all("socks5://...")` builds but every request fails with "socks feature not enabled"; `http://` / `https://` proxies work either way. The feature only pulls `tokio-socks` (pure Rust, no TLS, no CMake / NASM / OpenSSL). Contract 16 section 6 already prefers `reqwest/socks`. |

Everything else tk-server uses (`axum`, `tower-http`, `argon2`, `tokio`, `tokio-util`, `futures-util`, `bytes`, `url`, `uuid`, `time`, `sha2`, `hex`, `serde_json`, `thiserror`, `tracing`, `tempfile` for tests) is already in its `Cargo.toml`.

## Integrator patch outside tk-server (no new dependency)

`patches/tk-core-net-outbound.patch` (apply from the repo root with `git apply`) adds to `tk-core/src/net.rs`, which k
does not own: `Policy::route_outbound` (true only in `Policy::extension_fetch()`), the `Outbound` trait, `Clearance`,
`OutboundFuture`, `OutboundSlot`, `set_extension_outbound`, `is_cloudflare_challenge`, `SafeHttp::with_outbound`, and
the proxy / clearance / single FlareSolverr retry inside `SafeHttp::send`, with tests. `tk-ext` needs no change: its
`SafeHttp` is built from `Policy::extension_fetch()`, so every extension `fetch` (host.rs `SafeHttp`, runtime.rs
`do_fetch`) follows the route the app installs. `tk_server::upstream::ExtOutbound` and `tk-app/src/live/server.rs`
(`apply_extension_route`) use the new items and only compile once the patch is applied.
