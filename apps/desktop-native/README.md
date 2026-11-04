# apps/desktop-native - Tatakai Next (native desktop)

Rust + Dioxus 0.7 desktop app with an embedded libmpv player and a sandboxed QuickJS host for `.kai`
extensions. Binary `tatakai-next`. **Linux and Windows are both primary targets; Linux comes first** (decision 0.1
of [`docs/redesign/09-wave2-contract.md`](../../docs/redesign/09-wave2-contract.md)). License: AGPL-3.0-or-later
(`LICENSE`, third-party code in `THIRD_PARTY_NOTICES.md`).

| Crate | What |
|---|---|
| `tk-core` | metadata router, storage (SQLite), history, Supabase REST |
| `tk-ext` | extension host (rquickjs), JS prelude, permission-gated `fetch`, browser runtime for Cloudflare bypass |
| `tk-link` | Tatakai Link server (phone pairing, FFmpeg upscale sessions) |
| `tk-player` | libmpv player, engine modes, external mpv over JSON IPC; `NullPlayer` without the `mpv` feature |
| `tk-ui` | Dioxus UI; talks to the app only through the `Backend` trait (`src/backend.rs`) |
| `tk-app` | the `tatakai-next` binary: wiring and the live `Backend` (`src/live/*`) |

Data lives in `~/.config/Tatakai-Next/` on Linux and `%APPDATA%\Tatakai-Next\` on Windows (logs in `logs/`);
it never touches the legacy Electron app's data.

## Player modes (Settings > Player > Engine)

| Mode | What you get |
|---|---|
| **Embedded** (default) | libmpv drawn inside the app window, with the app's controls. Linux: libmpv render API (OpenGL) into a GTK `GLArea` placed under the WebKitGTK web view, whose background is transparent at widget level (the toplevel window stays opaque). Windows: child HWND under WebView2. |
| **Embedded enhanced** | The same embedded player loading the vendored [mpv-anime-build](https://github.com/Chinna95P/mpv-anime-build) v5.6 (`vendor/mpv-anime-build/`, MIT): its profiles, shaders and scripts. UI: the app's controls (default, uosc disabled) or the build's own uosc UI. The zip is verified against `SHA256SUMS` and extracted into `<data>/mpv-anime-build/5.6/` on first use. |
| **External mpv** | Your own mpv, with your config and key bindings, in its own window. The app shows a "Playing in external mpv" screen with Stop and Back, follows position/duration over mpv's JSON IPC (unix socket on Linux, named pipe on Windows) and records progress as usual. Closing mpv returns to where you were. |

## Linux

### Dependencies

Runtime: libmpv with client API 2.x (mpv >= 0.35), WebKitGTK 4.1, GTK 3, glib-networking (TLS for the web
view), libxdo, ffmpeg (Tatakai Link). A CJK font (e.g. Noto Sans CJK) is needed for kanji/kana titles.
Optional: libayatana-appindicator (tray), xdg-desktop-portal (file dialogs), mpv itself for the external mode.

Arch / CachyOS:

```
sudo pacman -S --needed base-devel rustup pkgconf mpv webkit2gtk-4.1 gtk3 glib-networking libsoup3 xdotool ffmpeg noto-fonts-cjk
```

Ubuntu 24.04 / Debian 13 (older releases ship libmpv 1.x, which libmpv2 refuses at startup):

```
sudo apt install build-essential pkg-config curl libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev \
  libjavascriptcoregtk-4.1-dev libmpv-dev libxdo-dev libayatana-appindicator3-dev glib-networking ffmpeg fonts-noto-cjk
```

The toolchain is pinned by `rust-toolchain.toml` (rustup installs it on first use; the extra Windows target in
that file is only downloaded, never used on Linux).

### Build and run

```
cd apps/desktop-native
packaging/linux/fetch-deps.sh                  # Anime4K shaders (+ fonts with a pinned hash), SHA-256 verified
cargo run -p tk-app                            # features by default: mpv + live
cargo test --workspace --no-default-features   # unit/fixture tests, no libmpv needed
```

In development the app finds fonts, shaders and the mpv-anime-build zip in the source tree; installed builds
look next to the executable (`/usr/lib/tatakai-next/`, see Packaging).

### Environment variables

| Variable | Effect |
|---|---|
| `WEBKIT_DISABLE_DMABUF_RENDERER=1` | Disables WebKitGTK's DMA-BUF renderer. Use it if the window is blank, black or flickers (NVIDIA, some Wayland compositors). The AppImage sets it by default (`=0` to opt out). |
| `GDK_BACKEND=x11` / `wayland` | Forces the GTK backend. Default: Wayland when available, X11 otherwise; both must work. |
| `WEBKIT_DISABLE_COMPOSITING_MODE=1` | Last resort for rendering glitches on old GPUs (slower). |
| `TATAKAI_NEXT_DATA_DIR=<dir>` | Uses another data directory (development, tests). |
| `RUST_LOG=<filter>` | Log filter (default `info,tk_ui=debug,tk_player=debug,tk_app=debug`). |

### Packaging (`packaging/linux/`)

| File | What |
|---|---|
| `PKGBUILD` | Arch / CachyOS package built from this checkout: `cd packaging/linux && makepkg -si` (`--nocheck` skips tests). Depends on the system `mpv`, `webkit2gtk-4.1`, `gtk3`. |
| `build-appimage.sh` | `Tatakai_Next-<version>-x86_64.AppImage` with linuxdeploy + the GTK plugin; bundles libmpv and its dependencies, the WebKitGTK helper processes and the TLS module. Build on the oldest distribution you want to support. |
| `AppRun` | AppImage entry: GTK hook, `WEBKIT_DISABLE_DMABUF_RENDERER=1` by default, keeps the user's `GDK_BACKEND`. |
| `install-tree.sh` | Shared install step (PKGBUILD and AppImage). |
| `fetch-deps.sh` | Linux counterpart of `fetch-deps.ps1` (shaders, fonts; libmpv/ffmpeg come from the system). |
| `app.tatakai.own.next.desktop`, `.metainfo.xml` | Desktop entry and AppStream metadata. |

Installed layout (both formats):

```
/usr/bin/tatakai-next -> ../lib/tatakai-next/tatakai-next
/usr/lib/tatakai-next/tatakai-next
/usr/lib/tatakai-next/assets/fonts/            bundled .woff2 + OFL texts
/usr/lib/tatakai-next/shaders/                 Anime4K (+ FSR once pinned), anime4k.lock.json
/usr/lib/tatakai-next/mpv-anime-build/         mpv-anime-build-v5.6.zip, SHA256SUMS, LICENSE, README.md
/usr/lib/tatakai-next/ffmpeg/bin/{ffmpeg,ffprobe} -> /usr/bin/...
/usr/share/applications/app.tatakai.own.next.desktop
/usr/share/metainfo/app.tatakai.own.next.metainfo.xml
/usr/share/icons/hicolor/<size>/apps/app.tatakai.own.next.png
/usr/share/licenses/tatakai-next/
```

CI: `.github/workflows/desktop-native-linux.yml` (Ubuntu 24.04: tests, release build, AppImage artifact; on a
manual run, optionally the PKGBUILD in an Arch container).

## Windows

```
pwsh packaging/fetch-deps.ps1      # libmpv, FFmpeg, shaders, fonts (run by the user)
. packaging/deps/env.ps1           # RUSTFLAGS to link libmpv (x64 Native Tools prompt)
cargo run -p tk-app
dx bundle --release -p tk-app --package-types nsis   # installer (packaging/windows.nsi)
```
