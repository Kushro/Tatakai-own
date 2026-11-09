# Dependencies requested by B6

None. Everything used is already in `[workspace.dependencies]`:

* tk-ui: dioxus (desktop, router), qrcode (svg), base64 (font data URIs), async-trait, serde, serde_json, tokio.
* tk-player: libmpv2 (optional `mpv`), windows-sys (`Win32_Foundation`, `Win32_UI_WindowsAndMessaging`), tokio (sync).
* tk-app: windows-sys (`Win32_System_Threading` for the named mutex), directories, tracing-subscriber (`env-filter`, `fmt`), tempfile (dev).

Integration: the `live` feature is now in `default` and `src/live/*` is wired to the real tk-ext / tk-link API.

# Wave A, worker i (torrents / magnet handler)

No new crate. Two more features of the `windows-sys` 0.61 already in `[workspace.dependencies]` (Windows only, no
new code pulled in; both are plain Win32 bindings):

* `Win32_System_Registry`: `src/magnet.rs` registers the per-user `magnet:` handler under
  `HKCU\Software\Classes\magnet` (`RegCreateKeyExW`, `RegSetValueExW`, `RegGetValueW`, `RegDeleteTreeW`).
* `Win32_System_SystemInformation`: `src/live/torrent.rs::local_hour` reads the local time (`GetLocalTime`) for the
  torrent bandwidth schedule.

Exact change in the workspace `Cargo.toml`, `windows-sys = { version = "0.61", features = [...] }`: add
`"Win32_System_Registry"` and `"Win32_System_SystemInformation"` to the list.
