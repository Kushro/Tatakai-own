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
| supabase-kt (auth-kt, postgrest-kt) and Ktor client | Apache-2.0 / MIT (supabase-kt) |
| quickjs-kt (dokar3) with bundled QuickJS | Apache-2.0 / MIT (QuickJS, Fabrice Bellard) |
| jsoup | MIT |
| MaterialKolor | MIT |
| ZXing core | Apache-2.0 |
| material-color-utilities (design token generation, vendored in `packages/tokens/vendor`) | Apache-2.0 |
| JUnit 4, Turbine, kotlin-test, MockWebServer | EPL-1.0 / Apache-2.0 |

## 3. Fonts (bundled in `res/font`, never loaded from a CDN) - added by `m3e/android-shell`

Space Grotesk, Outfit and JetBrains Mono are licensed under the **SIL Open Font License 1.1**. Ship each family's `OFL.txt` next to the font files and list them in Settings > About.

## 4. Services

No usage-tracking or crash-reporting SDKs (see apps/README.md, rule 3). The app talks only to: the user's own Supabase project, AniList (GraphQL), ani.zip, Kitsu, hosts allowed by installed
extensions, and a paired PC on the local network.

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
