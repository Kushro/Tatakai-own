# apps/ - native Tatakai (M3 Expressive) apps

New code in new directories; the legacy React/Capacitor/Electron app (`src/`, `android/`, `desktop/`, `supabase/`) keeps shipping untouched until parity (M4).
Design docs: [`docs/redesign/`](../docs/redesign/README.md). Contracts shared by both platforms: [`contracts/`](../contracts). Design tokens: [`packages/tokens/`](../packages/tokens).

| Path | What | License |
|---|---|---|
| `apps/android-native/` | Kotlin 2.4 + Jetpack Compose + Material 3 Expressive, `applicationId app.tatakai.own.next` | AGPL-3.0-or-later |
| `apps/desktop-native/` | Rust 1.94.1 + Dioxus 0.7.10 (WebView2) + libmpv, binary `tatakai-next.exe`, data in `%APPDATA%\Tatakai-Next\` | AGPL-3.0-or-later |
| `packages/tokens/` | `tokens.json` -> `Tokens.kt` + `tokens.css` (generated files are committed) | (repo) MPL-2.0 |
| `contracts/` | extension-host routes/globals, Link v1 OpenAPI, Supabase M0 tables, fixtures | (repo) MPL-2.0 |

The repository root stays MPL-2.0. The apps are AGPL-3.0-or-later so GPL/AGPL code from the author's own repos can be copied (decision 1 of `06-user-decisions.md`);
every copied file is recorded in the app's `THIRD_PARTY_NOTICES.md`.

## 1. Toolchain (this host)

| Piece | Version / location |
|---|---|
| JDK | **Temurin 25.0.3** (`JAVA_HOME`). Android Studio is NOT installed; there is no JBR 21. |
| Gradle | **9.3.1** via `apps/android-native/gradlew.bat` (wrapper jar copied from Miko) |
| AGP / Kotlin / KSP | **8.13.2 / 2.4.0 / 2.3.12** (KSP no longer has a `<kotlin>-x.y.z` line; `2.3.x` works with Kotlin 2.4.0) - same combo Miko builds with on JDK 25 |
| Android SDK | `sdk.dir=D:/Android-SDK` in `apps/android-native/local.properties` (gitignored; `ANDROID_HOME` is not set). compileSdk/targetSdk 36, minSdk 26, JVM target 17 |
| Compose | BOM `2026.06.01`; Material 3 pinned to `1.5.0-alpha15` (what the BOM resolves); Navigation 3 `1.0.1`; Material 3 Adaptive `1.3.0` |
| Media3 | **1.11.1** (ExoPlayer, HLS, DASH, Session, ui-compose, datasource-okhttp) |
| Extension host | quickjs-kt **1.0.15** (QuickJS, precompiled JNI, no NDK) |
| QR scan | CameraX **1.6.2** + ZXing core **3.4.1**; **no Google Play services** (always keep a "paste code" alternative) |
| Supabase | supabase-kt **3.8.0** (auth-kt, postgrest-kt) + Ktor **3.6.0** (okhttp engine) |
| Rust | **1.94.1** pinned by `apps/desktop-native/rust-toolchain.toml` (same as Miko-Kagura / Enigmata-Temple); MSVC build tools required (rquickjs and rusqlite compile C) |
| TLS | rustls 0.23 with the **`ring`** provider (no aws-lc: it needs CMake + NASM on Windows). rusqlite is pinned to the cached **0.32.1** (bundled) |
| Node | >= 22 (tests/generators use only `node:*` built-ins; `zlib.crc32` needs >= 22.2). No `npm install` needed for `packages/tokens` or `contracts` |

Everyday commands (from the repo root):

```
node --test packages/tokens/test contracts/test          # tokens (schema, no-diff regeneration, AA contrast) + contracts + tatakai.me/telemetry guard
node packages/tokens/generate.mjs                        # regenerate Tokens.kt + tokens.css after editing tokens.json
node contracts/fixtures/extensions/build.mjs             # re-pack fixture .kai (packed/*.kai are committed)
node contracts/fixtures/metadata/gen-expected.mjs        # refresh the metadata mapping oracle
cd apps/android-native && ./gradlew.bat help             # Gradle (use ./gradlew.bat in Git Bash; JAVA_HOME must be the Temurin 25)
cd apps/desktop-native && cargo metadata --format-version 1 > /dev/null   # resolves (no compile)
cd apps/desktop-native && cargo check --workspace --no-default-features  # compiles without libmpv
```

Windows note: in Git Bash never redirect to `NUL` (it creates a file called `NUL`); use `/dev/null`, or run from PowerShell/cmd with `> NUL`.

## 2. Configuration injection (review E8) - nothing is committed

| App | How `TATAKAI_SUPABASE_URL` / `TATAKAI_SUPABASE_ANON_KEY` arrive |
|---|---|
| Android | `apps/android-native/local.properties` (see `local.properties.example`) or environment variables with the same names -> `BuildConfig.TATAKAI_SUPABASE_URL` / `BuildConfig.TATAKAI_SUPABASE_ANON_KEY` in `:app`. The task `checkTatakaiConfig` (wired to `preBuild` of `:app` only) FAILS the app build with a clear message when either is missing; `help`, `dependencies` and library-module builds are unaffected. |
| Desktop | `%APPDATA%\Tatakai-Next\config.json`, created at first launch from compile-time `option_env!("TATAKAI_SUPABASE_URL")` / `option_env!("TATAKAI_SUPABASE_ANON_KEY")` (or empty). Without it the app still starts without an account (local-first). |

The anon key is public by design but still not committed. The Supabase session is never sent to any host other than the Supabase URL.

## 3. API sources for Compose alphas (review E2) - grep before you call

M3E / Navigation 3 / Media3-Compose are alpha APIs and signatures change. **Do not use a Material 3 Expressive, Navigation 3, Adaptive or Media3-Compose API unless you found it with grep in the sources jars.**
They are extracted (gitignored) under:

```
apps/android-native/.deps-sources/
  material3-android-1.5.0-alpha15/                      (commonMain/androidx/compose/material3/...)
  material3-adaptive-navigation-suite-android-1.5.0-alpha15/
  navigation3-runtime-android-1.0.1/   navigation3-ui-android-1.0.1/
  adaptive-navigation3-android-1.3.0/
  media3-ui-compose-1.11.1/
```

Fetch/refresh with `node apps/android-native/scripts/fetch-deps-sources.mjs` (needs the JDK's `jar` on PATH). Known corrections from the review: `ModalSideSheet` does NOT exist in material3 1.5.0-alpha15
(AdaptiveSheet = `ModalBottomSheet` in compact + a custom side sheet elsewhere); `HorizontalMultiBrowseCarousel` lives in `androidx.compose.material3.carousel`; `Typography`/`Shapes`/`MotionScheme`
need `@OptIn(ExperimentalMaterial3ExpressiveApi::class)` (the generated `Tokens.kt` already compiles against these).

## 4. Wave-1 branches and ownership

All branches are git worktrees under `E:/Gitbutler Repos/Tatakai-own-wt/<name>` (this repo is not GitButler-managed): commit with plain `git commit` in your worktree, never push, never edit another branch's paths.
After B1 merges, ownership of each placeholder passes to the owner of that directory (it may overwrite it freely).

| # | Branch | Exclusive paths | Wave |
|---|---|---|---|
| B1 | `m3e/foundation` | `packages/tokens/**`, `contracts/**`, `docs/redesign/**`, `apps/README.md`, build skeletons (Gradle settings/root/catalog/wrapper/`build-logic`, Cargo workspace/toolchain/config), generated `Tokens.kt` and `tokens.css`, `eslint.config.js` (ignores only) | 0 |
| B2 | `m3e/android-shell` | `apps/android-native/app/**`, `core/designsystem/**` (except `generated/`), `core/ui/**`, `feature/{home,search,details,library,settings,extensions,link}/**` | 1 |
| B3 | `m3e/android-data` | `core/{model,network,metadata,database,supabase,data}/**` | 1 |
| B4 | `m3e/android-media` | `core/{extensions,extensions-quickjs,player,link}/**`, `feature/player/**` | 1 |
| B5 | `m3e/desktop-core` | `apps/desktop-native/crates/{tk-core,tk-ext,tk-link}/**` | 1 |
| B6 | `m3e/desktop-app` | `apps/desktop-native/crates/{tk-player,tk-ui,tk-app}/**` (except `tk-ui/assets/tokens.css`), `apps/desktop-native/packaging/**`, `apps/desktop-native/Dioxus.toml` | 1 |

**Frozen after B1:** `gradle/libs.versions.toml`, `settings.gradle.kts`, `build.gradle.kts` (root), `gradle.properties`, `build-logic/**`, `apps/desktop-native/Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`.
Need a new dependency? Add a `NOTES-deps.md` in YOUR module/crate directory with the full coordinates, version and reason; the integrator copies it into the frozen files. (`:app` keeps the E8 BuildConfig injection.)

Module types (convention plugins): `tatakai.jvm.library` -> `:core:model`, `:core:metadata`, `:core:extensions`; `tatakai.android.library` -> the other `core:*` (note: `:core:link` is an Android library, it needs Keystore/Android TLS types);
`tatakai.android.library` + `tatakai.android.compose` -> `:core:designsystem`, `:core:ui`; `tatakai.android.feature` (library + compose + core deps + Koin + lifecycle) -> `feature:*`; `tatakai.android.application` + compose -> `:app`.
Type-safe project accessors are on (`projects.core.model`, `projects.core.extensionsQuickjs`).

## 5. Rules for every agent

1. Shell safety (verbatim from the user's global rules): verify variables before ANY destructive command (print the resolved path with `echo` and `ls` it in a separate step; abort if unexpected); never point a destructive command at `%APPDATA%`, `$HOME`, `%USERPROFILE%`, `C:\`, drive roots or the whole `%TEMP%` - always the most specific subdirectory; prefer literal absolute paths, else fail-safe forms (`rm -rf "${VAR:?}/subdir"`); shell state does not persist between tool calls.
2. Tests never write to real user directories (`tempfile` / `TemporaryFolder` / injected paths). Desktop data lives in `%APPDATA%\Tatakai-Next\` only at runtime, never in tests.
3. **Forbidden anywhere under `apps/ packages/ contracts/`:** `*.tatakai.me` hosts, the TatakaiAPI backend, crash-reporting SDKs and telemetry/usage-statistics SDKs or endpoints. `node --test contracts/test` fails on any match (case-insensitive scan; only this README and the guard itself are allow-listed because they explain the rule).
4. Copying from the author's own repos is allowed (AGPL app) but conserve the copyright header and add a row to the app's `THIRD_PARTY_NOTICES.md`.
5. Only `en` and `es` strings in M0 (Japanese falls back to English; do not create `values-ja` or `ja.json`). No hardcoded UI text.
6. Do not touch `src/`, `android/`, `desktop/`, `supabase/`, `package.json`, `package-lock.json`. M0 adds no Supabase migrations.
7. Builds are scarce: at most ONE `compileDebugKotlin` scoped to your own modules (and one JVM test run) per Kotlin branch; `cargo check -p <crate>` is free, ONE `cargo test` per Rust branch.
8. Extensions: renderer (React) extensions are not hosted natively; only `.kai` bundles run, inside QuickJS, per `contracts/extension-host/`.
