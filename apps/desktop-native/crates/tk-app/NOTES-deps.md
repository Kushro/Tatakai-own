# Dependencies requested by B6

None. Everything used is already in `[workspace.dependencies]`:

* tk-ui: dioxus (desktop, router), qrcode (svg), base64 (font data URIs), async-trait, serde, serde_json, tokio.
* tk-player: libmpv2 (optional `mpv`), windows-sys (`Win32_Foundation`, `Win32_UI_WindowsAndMessaging`), tokio (sync).
* tk-app: windows-sys (`Win32_System_Threading` for the named mutex), directories, tracing-subscriber (`env-filter`, `fmt`), tempfile (dev).

Integration note: `tk-app` has a `live` cargo feature (off by default) that enables `src/live/*`. Turn it on by
adding `"live"` to `default` in `crates/tk-app/Cargo.toml` once tk-ext and tk-link are merged, then fix any signature
drift in `src/live/ext.rs` and `src/live/link.rs` (every guess is marked `CONTRACT-GUESS`).
