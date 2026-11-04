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

## Torrents

The app has a built-in BitTorrent engine (`crates/tk-torrent`, on [librqbit](https://github.com/ikatson/rqbit)
9.1.0, Apache-2.0) and a torrent client in the rail (**Torrents**). Design and contract:
[`docs/redesign/10-wave3-torrents-contract.md`](../../docs/redesign/10-wave3-torrents-contract.md).

### Playing a torrent

1. Install an extension that returns torrent sources: a `torrent` extension, or a custom one such as Toko that
   lists the `torrent` route (its Nyaa, SubsPlease, AnimeTosho, SeaDex... providers then show up in the picker).
2. Open an episode and pick a torrent source (chips show group, BD/WEB/REMUX, resolution, codec, 10-bit, dual
   audio, multi-sub, batch and SeaDex best; the filter row narrows the list without changing its order), or let
   Auto choose.
3. The app adds the torrent, waits for its metadata (up to 90 s), picks the episode's file (inside a season pack
   too) and streams it while it downloads. The seek bar shows the downloaded ranges and, when known, intro/outro.
   For a batch, Next episode reuses the same torrent. If no file matches, the player offers the client's Files page to look at
   what the torrent contains (choosing a file there is not available yet).

Streaming works in all three player modes: the player loads a local URL
(`http://127.0.0.1:<port>/<token>/...`, a random token per run, never reachable from other machines), so
external mpv plays it the same way.

### Where things are stored

| What | Where |
|---|---|
| Downloaded files | Torrents > Settings > Download folder; default `<Downloads>/Tatakai` (`~/Downloads/Tatakai`, `%USERPROFILE%\Downloads\Tatakai`), else `<data>/torrents/downloads` |
| Session, fast-resume, DHT state, labels | `<data>/torrents/` |
| Torrent settings | `<data>/torrent-settings.json` |

`<data>` is `~/.config/Tatakai-Next/` on Linux and `%APPDATA%\Tatakai-Next\` on Windows. **After watching**
decides what happens to a torrent you only streamed: **Delete** (default) removes it and its files about 20 s after you leave
the player (a torrent left over by a crash is removed on the next start); **Keep** keeps it in the client's Library (you can also toggle Keep per torrent there).

### Background downloads

Off by default. The Downloads page explains the cost first (CPU, disk and network use, and uploading to other
peers while the app is open) and only enables the switch after you press **I understand**. With it on, kept
torrents keep downloading and seeding while the app runs; with it off they pause when playback ends. To fetch an
episode for later, use the download button of a torrent row in the source picker of the details page; it shows up
in Torrents > Downloads. Nothing runs after the app is closed.

### Ports and network

| Setting | Default | Notes |
|---|---|---|
| Listen port (TCP/uTP, incoming peers) | `0` = random each run | Set a fixed port (1024-65535) to forward it on your router. On Windows the first incoming connection may show a firewall prompt. |
| UPnP | on | Asks the router to forward the listen port; the Overview page shows NAT / forwarding status. |
| Streaming server | random loopback port | Bound to `127.0.0.1` only; not configurable. |
| DHT / PEX / LSD, max connections (50), speed limits (0 = unlimited) | Torrents > Settings | Applied live where the engine allows it. |

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
| `WEBKIT_DISABLE_DMABUF_RENDERER=1` | Disables WebKitGTK's DMA-BUF renderer. Use it only if the window is blank, black or flickers (some NVIDIA setups). The embedded player needs that renderer: with this set, mpv plays in its own window. Not set by default (the app also stops Dioxus from setting it). |
| `GDK_BACKEND=x11` / `wayland` | Forces the GTK backend. Default: Wayland when available, X11 otherwise; both must work. |
| `WEBKIT_DISABLE_COMPOSITING_MODE=1` | Last resort for rendering glitches on old GPUs (slower). Like the variable above, it moves mpv to its own window. |
| `TK_MPV_LOG=<file>` | Diagnostics: libmpv writes its own verbose log to `<file>`. |
| `TATAKAI_NEXT_DATA_DIR=<dir>` | Uses another data directory (development, tests). |
| `RUST_LOG=<filter>` | Log filter (default `info,tk_ui=debug,tk_player=debug,tk_app=debug`). |

### Packaging (`packaging/linux/`)

| File | What |
|---|---|
| `PKGBUILD` | Arch / CachyOS package built from this checkout: `cd packaging/linux && makepkg -si` (`--nocheck` skips tests). Depends on the system `mpv`, `webkit2gtk-4.1`, `gtk3`. |
| `build-appimage.sh` | `Tatakai_Next-<version>-x86_64.AppImage` with linuxdeploy + the GTK plugin; bundles libmpv and its dependencies, the WebKitGTK helper processes and the TLS module. Build on the oldest distribution you want to support. |
| `AppRun` | AppImage entry: GTK hook, keeps the user's `GDK_BACKEND` and WebKitGTK's DMA-BUF renderer (needed by the embedded player). |
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
