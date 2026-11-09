# Third-party notices - Tatakai native Android

This app is licensed **AGPL-3.0-or-later** (see `LICENSE`). Decision 1 of `docs/redesign/06-user-decisions.md`: code may be copied from the
author's own repositories (Miko GPL-3.0, Enigmata AGPL-3.0, Enigmata-Temple, Miko-Kagura, limusic-forge GPL-3.0, larper-forge, mpvExKEV,
design_system_android). The repository root stays MPL-2.0 (compatible with GPL/AGPL, MPL-2.0 section 3.3).

## 1. Code copied or adapted from other repositories

Every branch that copies a file MUST add a row here (origin repo + path, license, what was changed) and keep the file's original copyright header.

| Target file(s) in this app | Origin (repo / path / commit) | Origin license | Notes |
|---|---|---|---|
| `build-logic/**` (convention plugins) | Miko `buildSrc/` (patterns: Android/Compose/JVM config, JVM 17 target, compileSdk 36 / minSdk 26) | GPL-3.0 | Adapted: precompiled script plugins renamed to `tatakai.*`, no spotless/lint plugins |
| `gradlew`, `gradlew.bat`, `gradle/wrapper/gradle-wrapper.jar` | Miko (stock Gradle 9.3.1 wrapper) | Apache-2.0 (Gradle) | Unmodified except `distributionUrl` already pointing at 9.3.1 |
| `LICENSE` | Enigmata `LICENSE` (verbatim AGPL-3.0 text) | AGPL-3.0 | - |

## 2. Libraries (resolved versions are in `gradle/libs.versions.toml`)

| Library | License |
|---|---|
| AndroidX (Core, Activity, Lifecycle, Navigation 3, Room, WorkManager, DataStore, CameraX, Compose, Material 3 / Material 3 Adaptive) | Apache-2.0 |
| AndroidX Media3 (ExoPlayer, HLS, DASH, Session, ui-compose, datasource-okhttp) | Apache-2.0 |
| Kotlin, kotlinx.coroutines, kotlinx.serialization, kotlinx.collections.immutable | Apache-2.0 |
| OkHttp 5 / Okio | Apache-2.0 |
| Coil 3 | Apache-2.0 |
| Koin | Apache-2.0 |
| quickjs-kt (dokar3) with bundled QuickJS | Apache-2.0 / MIT (QuickJS, Fabrice Bellard) |
| jsoup | MIT |
| MaterialKolor | MIT |
| ZXing core | Apache-2.0 |
| material-color-utilities (design token generation, vendored in `packages/tokens/vendor`) | Apache-2.0 |
| JUnit 4, Turbine, kotlin-test, MockWebServer | EPL-1.0 / Apache-2.0 |
| libtorrent4j 2.1.0-39 (`org.libtorrent4j:libtorrent4j` + `-android-arm64/-arm/-x86_64`, native libtorrent-rasterbar, Boost, OpenSSL) - android wave | MIT (libtorrent4j), BSD-3-Clause (libtorrent), BSL-1.0 (Boost), Apache-2.0 (OpenSSL 3) |
| libmpv for Android 1.0.0 (`dev.jdtech.mpv:libmpv`, bundles mpv + FFmpeg + libass) - android wave | MIT (wrapper); mpv LGPL-2.1-or-later, FFmpeg LGPL-2.1-or-later, libass ISC (as built upstream; verify on upgrade) |

## 3. Fonts (bundled in `res/font`, never loaded from a CDN) - added by `m3e/android-shell`

Space Grotesk, Outfit and JetBrains Mono are licensed under the **SIL Open Font License 1.1**. Ship each family's `OFL.txt` next to the font files and list them in Settings > About.

| Font file (`core/designsystem/src/main/res/font/`) | Family / upstream | Source | License text in the APK |
|---|---|---|---|
| `space_grotesk.ttf` | Space Grotesk, variable `[wght]` 300-700 (Florian Karsten) | `github.com/google/fonts` `ofl/spacegrotesk/SpaceGrotesk[wght].ttf` (main, fetched 2026-11-04) | `assets/licenses/OFL-SpaceGrotesk.txt` |
| `outfit.ttf` | Outfit, variable `[wght]` 100-900 (Outfitio) | `github.com/google/fonts` `ofl/outfit/Outfit[wght].ttf` (main, fetched 2026-11-04) | `assets/licenses/OFL-Outfit.txt` |
| `jetbrains_mono.ttf` | JetBrains Mono, variable `[wght]` 100-800 (JetBrains) | `github.com/google/fonts` `ofl/jetbrainsmono/JetBrainsMono[wght].ttf` (main, fetched 2026-11-04) | `assets/licenses/OFL-JetBrainsMono.txt` |

Files are unmodified (renamed only, Android resource names). `Fonts.kt` pins the `wght` axis per weight with `FontVariation.Settings`.
Settings > About > Licenses shows this file (copied into the APK assets as `licenses/THIRD_PARTY_NOTICES.md` by `:app`) and the three OFL texts.

## 4. Services

No usage-tracking or crash-reporting SDKs (see apps/README.md, rule 3). There is no own backend and no own login (2026-11-09). The app talks only to: AniList (GraphQL + OAuth), MyAnimeList (REST + OAuth), ani.zip, Kitsu, trace.moe, api.github.com (release notice), the debrid services the user links, the translation endpoints the user configures, hosts allowed by installed
extensions, and a paired PC on the local network (Tatakai Link, incl. the watch-history sync).

## 5. Code adapted from the author's own repositories

### UI / theme (m3e/android-shell)

| Target file(s) | Origin | License | Notes |
|---|---|---|---|
| `core/designsystem/.../theme/TatakaiTheme.kt` | Miko `app/src/main/java/eu/kanade/presentation/theme/TachiyomiTheme.kt` (pattern: `MaterialExpressiveTheme` + `DynamicMaterialExpressiveTheme`) | GPL-3.0 | Re-written around the generated token palettes; only the structure/call shapes are reused |
| `core/designsystem/.../components/SectionCard.kt`, `core/ui/.../media/MediaCover.kt` (13:18 ratio), `AdaptiveSheet.kt` (concept) | Tatami `E:/Gitbutler Repos/design_system_android` (author's own) | author's own | Adapted to M3E and the token API |

### Link client and key vault (m3e/android-media, AGPL-3.0-or-later)

Code adapted from the author's own repositories (AGPL-3.0-or-later), per decision 1 of docs/redesign/06-user-decisions.md:

- `core/link/.../LinkCrypto.kt` (LinkCrypto, LinkCsr, Der), `PairingPayload.kt`, `LinkHttp.kt`, `LinkError.kt` (structure), `PairingClient.kt`:
  adapted from Enigmata `app/src/main/java/app/enigmata/temple/{crypto,pairing,client}` (TempleCrypto, TempleCsr, TemplePairingPayload, TempleHttp, TempleError, TemplePairingClient).
- `core/link/.../LinkKeyVault.kt`: key-sealing model (software P-256 key sealed with an AndroidKeyStore AES-GCM key under `noBackupFilesDir`) from Enigmata `temple/vault/{TempleKeyVault,KeystoreTempleKeyCipher}`.

### Background playback with libmpv (parity wave A, worker p, AGPL-3.0-or-later)

| Target file(s) | Origin | License | Notes |
|---|---|---|---|
| `core/player-mpv/.../MpvPlayerAdapter.kt`, `core/player/.../PlaybackService.kt` (observe-the-core pattern, per-core observer re-registration, "no notification update after shutdown" gate, `onTaskRemoved` handling) | Enigmata `app/src/main/java/app/enigmata/ui/player/MediaPlaybackService.kt` (author's own) | AGPL-3.0-or-later | Re-expressed over Media3 `SimpleBasePlayer` + `MediaSessionService` (no `MediaSessionCompat`); patterns adapted (no file copied verbatim) from Enigmata commit `ab5d96e0cc31b3dbe26a2eb695c5b009e58f191a` |

### Bundled mpv shaders (parity wave A, worker p)

Shipped unmodified in `core/player-mpv/src/main/assets/shaders/` (each file keeps its own licence header) and copied to `filesDir/shaders/<preset>/` at runtime.

| File(s) | Origin | License | Notes |
|---|---|---|---|
| `Anime4K_Clamp_Highlights.glsl`, `Anime4K_Restore_CNN_{M,S,Soft_M}.glsl`, `Anime4K_Upscale_CNN_x2_{M,S}.glsl`, `Anime4K_Upscale_Denoise_CNN_x2_M.glsl`, `Anime4K_AutoDownscalePre_x2.glsl` | Anime4K v4 GLSL by bloc97 (github.com/bloc97/Anime4K) | MIT | Presets A / A+A / B / C (chains as desktop `tk-player/src/preset.rs`) |
| `FSR.glsl` | AMD FidelityFX FSR 1.0.2, mpv port by agyild | MIT (Copyright (c) 2021 Advanced Micro Devices, Inc.) | Preset "fsr" |
