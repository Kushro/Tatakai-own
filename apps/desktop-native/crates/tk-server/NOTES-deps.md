# tk-server: dependencies requested by worker k (wave B)

Nothing new to add as a crate. One feature of a crate already in the workspace:

| Crate | Change | Why |
|---|---|---|
| `reqwest` 0.12 (workspace) | add the feature `socks` to `[workspace.dependencies] reqwest` (`features = ["rustls-tls", "json", "stream", "socks"]`) | `server.proxy`: the outbound proxy of the media proxy accepts `socks5://` / `socks5h://` (`upstream::Upstream`, `upstream::probe_proxy`). Without the feature `reqwest::Proxy::all("socks5://...")` builds but every request fails with "socks feature not enabled"; `http://` / `https://` proxies work either way. The feature only pulls `tokio-socks` (pure Rust, no TLS, no CMake / NASM / OpenSSL). Contract 16 section 6 already prefers `reqwest/socks`. |

Everything else tk-server uses (`axum`, `tower-http`, `argon2`, `tokio`, `tokio-util`, `futures-util`, `bytes`, `url`, `uuid`, `time`, `sha2`, `hex`, `serde_json`, `thiserror`, `tracing`, `tempfile` for tests) is already in its `Cargo.toml`.
