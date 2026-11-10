# Tatakai Next

Tatakai para Windows, Linux y Android, reescrito en nativo: escritorio en **Rust + Dioxus + libmpv** y Android en **Kotlin + Jetpack Compose (Material 3 Expressive)**. Mirá anime con tus extensiones, llevá tus listas en AniList (o MyAnimeList) y seguí en el teléfono o en la PC con **Tatakai Link**, que además puede mejorar el video desde la PC (Anime4K y generación de cuadros LSFG).

> **¿Buscás la app clásica (Electron + Capacitor)?** Está en la rama [`legacy`](https://github.com/Kushro/Tatakai-own/tree/legacy) y sus versiones se publican con etiquetas `legacy-v*`.

## Descargas

En [Releases](https://github.com/Kushro/Tatakai-own/releases/latest):

| Plataforma | Archivo |
|---|---|
| Windows x64 | `Tatakai-Next-v<versión>-windows-x64-setup.exe` (instalador por usuario) o `…-portable.zip` |
| Linux x86_64 | `Tatakai-Next-v<versión>-linux-x86_64.AppImage` |
| Android 8+ | `Tatakai-Next-v<versión>-android-arm64-v8a.apk` (casi todos los teléfonos; hay `armeabi-v7a`, `x86_64` y `universal`), firmado con la clave de Tatakai (las actualizaciones se instalan encima) |

Sin cuenta propia: tu identidad es tu cuenta de AniList / MyAnimeList; sin cuenta, todo queda en el dispositivo.

## Repositorio

| Ruta | Qué es |
|---|---|
| [`apps/desktop-native/`](apps/desktop-native/README.md) | App de escritorio (`tatakai-next`): crates `tk-ui`, `tk-app`, `tk-player`, `tk-core`, `tk-link`, `tk-server` |
| [`apps/android-native/`](apps/README.md) | App de Android (`app.tatakai.own.next`) |
| [`contracts/`](contracts) | Contratos compartidos: Tatakai Link v1 (OpenAPI), host de extensiones, checklist de paridad |
| [`packages/tokens/`](packages/tokens) | Tokens de diseño → `Tokens.kt` + `tokens.css` |
| [`docs/redesign/`](docs/redesign/README.md) | Diseño, decisiones y problemas conocidos (`KNOWN-ISSUES.md`) |

Compilar y probar: [`apps/README.md`](apps/README.md).

## Versiones y CI

- `main` = Tatakai Next. `.github/workflows/next-ci.yml` prueba cada push y PR (solo lo que cambió: escritorio, Android, contratos).
- Una etiqueta `v<semver>` (igual a `version` de `apps/desktop-native/Cargo.toml`) dispara `next-release.yml`: AppImage, instalador y zip de Windows, y APK firmado; se publica como **Tatakai Next &lt;versión&gt;** (latest). `1.2.3-beta.1` sale como pre-release.
- `legacy` = Tatakai Classic, con su propio CI y etiquetas `legacy-v*` (nunca "latest").

## Licencia

Las apps son AGPL-3.0-or-later; la raíz del repositorio, MPL-2.0. Ver `LICENSE`, `NOTICE` y los `THIRD_PARTY_NOTICES.md` de cada app.
