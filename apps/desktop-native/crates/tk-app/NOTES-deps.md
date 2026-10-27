# Dependencies requested by B6

None. Everything used is already in `[workspace.dependencies]`:

* tk-ui: dioxus (desktop, router), qrcode (svg), base64 (font data URIs), async-trait, serde, serde_json, tokio.
* tk-player: libmpv2 (optional `mpv`), windows-sys (`Win32_Foundation`, `Win32_UI_WindowsAndMessaging`), tokio (sync).
* tk-app: windows-sys (`Win32_System_Threading` for the named mutex), directories, tracing-subscriber (`env-filter`, `fmt`), tempfile (dev).

Integration: the `live` feature is now in `default` and `src/live/*` is wired to the real tk-ext / tk-link API.
