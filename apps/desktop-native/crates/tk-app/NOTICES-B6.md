# Third-party code copied or adapted by branch B6 (m3e/desktop-app)

The integrator consolidates these lines into `apps/desktop-native/THIRD_PARTY_NOTICES.md`.

| Origin (author's own repos) | License | Used in |
|---|---|---|
| limusic-forge `src-tauri/src/nativevideo_windows.rs` (child `STATIC` window at `HWND_BOTTOM`, no `WS_CLIPSIBLINGS`, hole in the page) - rewritten for windows-sys | GPL-3.0-or-later | `crates/tk-player/src/surface.rs` |
| limusic-forge `crates/player` (libmpv2 setup, option set, event-thread structure) - rewritten | GPL-3.0-or-later | `crates/tk-player/src/mpv_player.rs` |
| Miko-Kagura `src/ui/title_bar.rs` (frameless title bar) | see Miko-Kagura LICENSE | `crates/tk-ui/src/components/title_bar.rs` |
| Miko-Kagura `packaging/windows.nsi` (NSIS template) | see Miko-Kagura LICENSE | `packaging/windows.nsi` |
| Miko-Kagura `scripts/fetch-upscalers.ps1` (verified-download pattern) | see Miko-Kagura LICENSE | `packaging/fetch-deps.ps1` |
| Enigmata-Temple `packaging/anime4k.lock.json`, `ffmpeg.lock.json` (pins, hashes) | AGPL-3.0 | `packaging/*.lock.json` |
| Enigmata-Temple `temple-ui` (QR display approach) | AGPL-3.0 | `crates/tk-ui/src/qr.rs` |
| limusic-forge `.github/workflows/windows-release.yml` (libmpv URL and SHA-256) | GPL-3.0-or-later | `packaging/libmpv.lock.json` |
| larper-forge `app/crates/lf-ui/assets/fuentes/JetBrainsMono.woff2` + `OFL-JetBrainsMono.txt` | SIL OFL 1.1 | `crates/tk-ui/assets/fonts/` |

Runtime pieces fetched by the USER with `packaging/fetch-deps.ps1` (not in the repo): libmpv (shinchiro build), FFmpeg (BtbN LGPL build),
Anime4K shaders (MIT / Unlicense, bloc97), FSR.glsl (MIT, agyild), Space Grotesk and Outfit (SIL OFL 1.1).
