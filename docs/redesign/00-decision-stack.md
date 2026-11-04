# ADR-000 — Stack del rediseño Tatakai-own (M3 Expressive nativo)

Estado: **aprobado con enmiendas** (`05-review.md` E1-E12) y decisiones del usuario (`06-user-decisions.md`, prevalecen). Este documento ya incorpora ambas · Fecha: 2026-10-27 · Decide: Octavio (revisión: Fable)

## Contexto
- La app actual es React/Vite (~227k LOC TS/TSX) empaquetada en Capacitor (Android/TV) y Electron (desktop).
- En móvil el rendimiento es malo por causas estructurales, no por bugs puntuales: chunk principal 2.7 MB + vendor 1.9 MB,
  compilador de TypeScript en el cliente (3.5 MB), onnxruntime wasm (~53 MB), framer-motion en 95 archivos,
  Index monta ~30 secciones con queries, casi sin virtualización, blur/glass pesado, hls.js dentro del WebView,
  splash fijo de 2500 ms, y `GlobalListeners` invalida 7 queries en cada focus.
- Seguridad: en móvil las extensiones `.kai` se evalúan con `new Function` en el realm de la app
  (`src/core/extensions/mobile/mobileExtensionHost.ts`), es decir sin sandbox.
- El usuario ya tiene experiencia y código nativo en Miko (Compose M3E), Enigmata (Kotlin + mpv + Temple client),
  Enigmata-Temple (Rust + Dioxus 0.7 + FFmpeg/libplacebo), limusic-forge (libmpv embebido) y larper-forge (lf-ui).

## Decisión

| Plataforma | Elegido | Alternativas descartadas (por ahora) |
|---|---|---|
| Android phone/tablet/TV | **Kotlin 2.4.0 + Jetpack Compose (BOM 2026.06.01, Material 3 `1.5.0-alpha15`) + `MaterialExpressiveTheme`**; nuevo proyecto Gradle **9.3.1 + AGP 8.13.2** (JDK 25 Temurin del host, el mismo combo que compila Miko; E1) en `apps/android-native/` | (a) Seguir con Capacitor y arreglar perf: mejora parcial, no arregla hls.js-en-WebView ni la ausencia de sandbox. (b) Flutter / RN: sin reutilización de Miko/Enigmata. |
| Reproductor Android | **Media3 ExoPlayer 1.11.1 en M0**; **libmpv (AAR precompilado de mpv-android) en M1** como segundo motor para Anime4K en el dispositivo | Solo mpv desde M0: obliga NDK/AAR binario y suma riesgo antes de tener el core loop. Media3 GlEffect con Anime4K: posible, pero portar los .hook de mpv a efectos GL de Media3 es trabajo propio (M3, opcional). |
| Host de extensiones Android | **QuickJS vía `quickjs-kt` 1.0.15 (dokar3, Apache-2, JNI precompilado, bindings `asyncFunction` con coroutines)**, una instancia por extensión, límite de memoria + interrupt handler + timeout, sin APIs ambientales; `fetch` lo provee el host y aplica `network:domain:*` del manifest | `androidx.javascriptengine` (aislamiento de proceso real, pero sin callbacks host→JS síncronos: el puente de `fetch` sería un loop de mensajes frágil). Fallback documentado: `quickjs-android` (ya en el classpath de Miko). Aislamiento en proceso `:extensions` separado → M2. |
| Desktop (Windows primero; **enmendado en la ola 2: Linux primario**, ver abajo) | **Rust 1.94.1 + Dioxus 0.7.10 desktop (WebView2) + libmpv vía `libmpv2 = "4"` embebido en un HWND hijo (`wid`, `vo=gpu-next`) + `rquickjs`**. Capas del player (E5, patrón limusic-forge, **sin ventana overlay aparte**): ventana principal transparente (`WindowBuilder::with_transparent(true)` + `Config::with_background_color((0,0,0,0))`), hijo `WS_CHILD` (clase `STATIC`, `WS_EX_NOACTIVATE`, sin `WS_CLIPSIBLINGS`) en `HWND_BOTTOM` bajo WebView2 como `wid` de mpv; la página deja un "hueco" (`set_rect` en px CSS × scale factor, abierto solo cuando `vo-configured` es true) y los controles son la propia UI web encima. Fallback solo si la transparencia falla: OSC de mpv | (a) Mantener Electron + fixes de perf: válido como puente (sigue publicándose), pero 172 IPC handlers + Node en el renderer. (b) Compose Multiplatform Desktop: una sola UI, pero JVM pesada y embeber mpv en Swing/Skia es más difícil que el HWND hijo ya probado en limusic-forge. (c) Tauri 2: equivalente a Dioxus pero obliga a mantener UI en JS; el ecosistema del usuario (Temple, Kagura, lf-ui) es Dioxus. |
| Servidor de enlace/upscale | Crate `tk-link` dentro del binario desktop (axum + rustls con **proveedor `ring`**, rcgen; **mDNS/TOFU diferidos a M1**, E7), FFmpeg externo pinneado (BtbN win64 LGPL + libplacebo) con `-protocol_whitelist http,tcp,crypto` (E6). Clave de cliente del teléfono = modelo de Enigmata (clave P-256 de software sellada con AES-GCM de AndroidKeyStore; **no** clave Keystore no exportable, E4) | `aws-lc-rs` (lo usa Temple) exige CMake + NASM en Windows: evitarlo en M0. |
| Core compartido | **Contratos compartidos, implementaciones nativas**: JSON Schema + OpenAPI + fixtures en `contracts/`; Kotlin y Rust implementan cada uno | Core Rust único consumido por Android vía UniFFI: elimina duplicación, pero requiere compilar con NDK (WSL en este host) desde el día 1. Se reevalúa en M3 para `metadata` y `ext-host` cuando exista pipeline WSL. |
| Tokens de diseño | Un único `packages/tokens/tokens.json` → generador Node **sin dependencias npm** (`@material/material-color-utilities` 0.4.0 está *vendorizado* en `packages/tokens/vendor/`, Apache-2.0) → `Tokens.kt` (constructor completo de `ColorScheme` 1.5) + `tokens.css` commiteados; test AA con los 9 pares de E12 | Style Dictionary: dependencia extra; nuestro esquema es chico. |

## Enmienda de la ola 2 (2026-11-03): Linux primario y tres modos de reproductor

Decisión del usuario (`09-wave2-contract.md` §0, prevalece sobre la fila "Desktop" de arriba):

- **Linux es un target principal, por delante de Windows.** Equipo de desarrollo: CachyOS, KDE Plasma en Wayland; X11 sigue soportado. Nada nuevo es "solo Windows" salvo lo intrínseco (regla de firewall, HWND). En Linux la web view es **WebKitGTK 4.1** (wry/tao sobre gtk-rs 0.18) y libmpv, WebKitGTK y GTK son **librerías del sistema** (no se empaquetan en el PKGBUILD; sí en el AppImage).
- **Ruta de render integrada en Linux**: libmpv con la *render API* (OpenGL, `vo=libmpv`) dibujando en un `gtk::GLArea` colocado **debajo** de la web view dentro de un `gtk::Overlay` (la `WebView` se re-parenta desde la `default_vbox` de tao). La web view tiene fondo transparente **a nivel de widget**; la ventana toplevel es **opaca** fuera de Windows (la transparencia de toplevel parpadea con WebKitGTK/Wayland, commit `4813b3d`). La página deja el mismo "hueco" que en Windows (`set_rect`). EGL o GLX según el backend de GDK. Windows conserva el HWND hijo bajo WebView2 descrito arriba.
- **Tres modos de reproductor** (Ajustes › Reproductor › Motor):
  1. **Integrado** (por defecto): libmpv dentro de la ventana en Linux y Windows, con nuestros controles.
  2. **Integrado mejorado**: el mismo libmpv cargando [mpv-anime-build](https://github.com/Chinna95P/mpv-anime-build) v5.6 (MIT, vendorizado sin modificar en `apps/desktop-native/vendor/mpv-anime-build/` con `SHA256SUMS`; la app lo verifica y lo extrae en `<datos>/mpv-anime-build/5.6/` al primer uso). UI elegible: nuestros controles (por defecto; uosc desactivado, perfiles/shaders/scripts activos) o la UI uosc del build (nuestros controles se ocultan y la entrada se reenvía a mpv).
  3. **mpv externo**: el mpv de la máquina con la configuración y los controles del usuario, en su ventana, controlado por JSON IPC (socket unix en Linux, named pipe en Windows). La app muestra "Reproduciendo en mpv externo" con Detener y Atrás; al cerrar mpv vuelve a donde estaba; el progreso se registra igual.
- **Navegador para extensiones**: el bypass de Cloudflare de Toko/Latino usa una web view oculta de la propia app (sin Chrome), con la misma API que el `BrowserWindow` oculto del host Electron, limitada a `browserDomains` ∩ permisos `network:domain:*`.
- **Empaquetado Linux** (`apps/desktop-native/packaging/linux/`): PKGBUILD para Arch/CachyOS y AppImage (linuxdeploy + plugin GTK, libmpv incluida; no fuerza `WEBKIT_DISABLE_DMABUF_RENDERER`, que el reproductor incrustado necesita activo), con el layout "carpeta de la app" `/usr/lib/tatakai-next/` + symlink `/usr/bin/tatakai-next`; `.desktop` y metainfo AppStream con id `app.tatakai.own.next`. CI: `.github/workflows/desktop-native-linux.yml` (Ubuntu 24.04: libmpv con API 2.x).
- Datos en Linux: `~/.config/Tatakai-Next/` (equivalente a `%APPDATA%\Tatakai-Next\`).
- Licencias: en Linux libmpv/FFmpeg son los del sistema (enlace dinámico / proceso externo); el AppImage los incluye con sus licencias de distribución. mpv-anime-build (MIT) va en `THIRD_PARTY_NOTICES.md` y en `/usr/share/licenses/tatakai-next/`.

## Enmienda de la ola 3 (2026-11-04): torrents en desktop ahora, Android después

Decisión del usuario (`10-wave3-torrents-contract.md` §0, prevalece sobre el roadmap de `04` §8, que dejaba los torrents en M2):

- **Los torrents se adelantan de M2 a la ola 3 en desktop.** Motor **librqbit 9.1.0** (Rust puro, Apache-2.0) en el crate nuevo `crates/tk-torrent`, con DHT, PEX, trackers, LSD, UPnP opcional y persistencia de sesión (`<datos>/torrents/`). Se descarta libtorrent-rasterbar en desktop (C++ con Boost/OpenSSL: build propio en Windows) y se evita el TLS de librqbit por rustls (trae aws-lc, CMake + NASM en Windows): SHA-1 por `crypto-hash` y `reqwest 0.13` con `native-tls` (`10` §2.1).
- **Streaming por HTTP loopback** (`127.0.0.1`, token aleatorio por arranque, `Range`) con prioridad de piezas según la posición leída; los tres modos de reproductor (integrado, integrado mejorado, mpv externo) cargan la misma URL, sin cabeceras.
- **Cliente estilo Hayase** como destino nuevo del rail (Torrents: Overview, Files, Peers, Trackers, Library, Downloads, Settings), descargas en segundo plano solo tras un aviso explícito ("Entiendo").
- **Etiquetas de release** (`tk-core::release`, parseo tipo anitomy sin dependencias) para filtros en el selector de fuentes, elección del archivo del episodio dentro de un batch y reutilización del mismo torrent para el siguiente episodio. Las extensiones custom (Toko) también se consultan por la ruta `torrent`.
- **Android: solo diseño** en esta ola (`11-android-torrents-design.md`). Propuesta: reutilizar `tk-torrent` vía UniFFI (crate fino `tk-torrent-ffi`, NDK desde el equipo Linux) con libtorrent4j como alternativa; streaming loopback hacia Media3, servicio en primer plano (`mediaPlayback` / `dataSync`), almacenamiento específico de la app por defecto, solo Wi-Fi por defecto. Esto adelanta para el motor de torrents la evaluación de UniFFI que la fila "Core compartido" dejaba para M3; el resto del core sigue con contratos compartidos e implementaciones nativas.
- Extensiones: `torrent:session` deja de ser un permiso diferido en desktop (la sesión la da `tk-torrent`), sin cambiar el nivel de compatibilidad.

## Compatibilidad del contrato de extensiones `.kai`
- Se conserva el formato: ZIP con `manifest.json` (`id, name, version, type: torrent|onlinestream|custom`, `permissions: ["network:domain:x"]`, `apiServer.routes`) + `bundle.js` CJS (≤10 MB).
- Las rutas del host (hoy en `desktop/runtime/extension-api-host/host-server.cjs`) se congelan en `contracts/extension-host/routes.v1.json`:
  `health`, `sources`, `stream`, `torrent`, `providers`, `providers/{id}`, `manga/chapters`, `manga/pages`, `novel/search`, `novel/chapters`, `novel/content`,
  y los métodos custom de `AbstractSource` (`customHome/Search/Info/Watch/Read`).
- Globals que el host nativo debe proveer (shim): `fetch` (gated), `__tatakai_fetch__`, `__tatakai_parse_html__` (host: jsoup en Kotlin / scraper en Rust), `require` con lista blanca (`crypto` subset: `createHash`, `createHmac`, `randomUUID`, `randomBytes`; `buffer`, `events`, `url`, `path`, `querystring`, `util` mínimos), `console`, `setTimeout`, `URL`, `TextEncoder/Decoder`, `atob/btoa`, `Buffer`, `process`. **Especificación exacta: `contracts/extension-host/globals.v1.md`** (+ `compat.mjs`, implementación de referencia del análisis de compatibilidad).
- Cada extensión declara un **nivel de compatibilidad** calculado al instalar (`full | partial | unsupported`) por análisis estático de `require(...)`. Reglas: todo `require` en la lista blanca → `full`; `require` de builtins "stub" (`os`, `http`, `https`, `stream`, `zlib`…) o `require` dinámico → `partial` (carga, pero esas funciones lanzan al llamarlas); `fs`, `child_process`, `net`, otros builtins no listados, paquetes o rutas relativas → `unsupported` (se instala deshabilitada, con mensaje claro). Fixtures: `contracts/fixtures/extensions/`.
- **Idiomas de la UI: solo `en` + `es`** (06 #4); `ja` cae a inglés, no se crean `values-ja` ni `ja.json`.
- **Fuera del host nativo**: extensiones *renderer* (ESM `activate(ctx)` con React y privilegios totales). Se cortan en nativo; los temas de extensiones se podrán importar como tokens en M3.
- Capas de compatibilidad del upstream 6.4.2 (Mangayomi VM, Stremio, Seanime): M2 (Stremio es HTTP puro → barato; Mangayomi VM → caro).
- Solo sideload local (SAF en Android, diálogo de archivo en desktop). Sin store, sin `marketplace-client`.

## Supabase
- Kotlin: `supabase-kt` (auth + postgrest; realtime en M2 para watch rooms). Rust: cliente REST delgado propio sobre `reqwest` (GoTrue `/auth/v1/token`, PostgREST `/rest/v1/*`); no hay SDK oficial.
- Mismo proyecto Supabase propio; **M0 no agrega migraciones**. Las tablas que usa M0 (`profiles` solo lectura con columnas explícitas, `watch_history` con `upsert onConflict user_id,episode_id`) se documentan en `contracts/supabase/m0-tables.md`, que copia el esquema literal y **las claves exactas que escribe el legacy** (E9): `anime_id = String(anilistId)`, `episode_id = "<anime_id>?ep=<n>"`, `completed` si ≥ 90 %, `mal_id`/`anilist_id` solo si no son nulos.
- Configuración (E8): `TATAKAI_SUPABASE_URL` / `TATAKAI_SUPABASE_ANON_KEY` desde `local.properties`/env → `BuildConfig` (Android) y `%APPDATA%\Tatakai-Next\config.json` (desktop, desde `option_env!`); nada se commitea. Toda migración futura debe ser idempotente y re-ejecutable (`create … if not exists`, `drop policy if exists`).
- Tokens de AniList/MAL: viven en la tabla owner-only introducida en `b80d649`; los clientes nativos solo los leen con RLS de dueño.

## Router de metadata
- Se porta el contrato de `src/lib/metadata/types.ts` (`MetadataProvider`: feed/search/media/episodes/genres/ids/idsBatch, prioridad + cooldown + fallback).
- M0 nativo: AniList (GraphQL) + ani.zip (ids/episodios) + Kitsu (fallback). MAL, Shikimori, Tenrai → M1.
- Fixtures JSON grabadas (`contracts/fixtures/metadata/*.json`) para que los tests de Kotlin y Rust validen el mismo mapeo a `TatakaiMedia`.
- Nunca `*.tatakai.me`, nunca TatakaiAPI, nunca telemetría (sin Sentry/Datadog en las apps nativas). Test de guardia (`contracts/test/guard.test.mjs`, `node --test`) que falla si aparece `tatakai.me|tatakaiapi|sentry|datadog|analytics` (sin distinguir mayúsculas) en `apps/ packages/ contracts/` (incluye `packaging/*.ps1`).

## Migración y convivencia
- Monorepo: `apps/android-native/` (Gradle independiente), `apps/desktop-native/` (workspace Cargo), `packages/tokens/`, `contracts/`, `docs/redesign/`. Cada rama de trabajo vive en su propio **git worktree** (`E:/Gitbutler Repos/Tatakai-own-wt/<nombre>`; el repo no está gestionado por GitButler).
  El legacy (`src/`, `android/`, `desktop/`) **sigue publicándose sin cambios** hasta paridad.
- Android nativo usa `applicationId app.tatakai.own.next` (instalable junto al legacy). Cuando alcance paridad (M4) toma el id definitivo con migración de datos.
- Desktop nativo: binario `tatakai-next.exe` / `tatakai-next` (Linux), datos en `%APPDATA%\Tatakai-Next\` o `~/.config/Tatakai-Next/` (nunca toca los del Electron).

## Licencias (decisión del usuario 06 #1)
- **`apps/android-native` y `apps/desktop-native` son AGPL-3.0-or-later** (cada app lleva su `LICENSE` con el texto AGPL-3.0 y su `THIRD_PARTY_NOTICES.md`). El repo raíz sigue **MPL-2.0** (compatible con GPL/AGPL, MPL-2.0 sección 3.3); `packages/tokens` y `contracts/` quedan bajo la licencia del repo.
- **Se PERMITE copiar código de los repos propios del usuario**: Miko (GPL-3.0), Enigmata (AGPL-3.0), Enigmata-Temple (privado), Miko-Kagura, limusic-forge (GPL-3.0), larper-forge, mpvExKEV, design_system_android. Al copiar: conservar los avisos de copyright del archivo de origen y anotar el origen en `apps/<app>/THIRD_PARTY_NOTICES.md`. Esto reemplaza la regla anterior "reescribir, no copiar".
- Anime4K: MIT (se empaquetan los `.glsl`/`.hook` con su LICENSE). libmpv / FFmpeg: builds LGPL, enlace dinámico / proceso externo, DLL junto al exe, aviso en "Acerca de". Fuentes (Space Grotesk, Outfit, JetBrains Mono): OFL-1.1, con su `OFL.txt`.
- Inconsistencia previa del repo: `package.json` dice `"license": "MIT"` pero `LICENSE` es MPL-2.0. Se corrige a MPL-2.0 en este PR como **commit aparte del integrador** (B1 no toca `package.json`).

## Consecuencias
- (+) Rendimiento nativo, sandbox real de extensiones, reproductor nativo con headers, base para enlace PC↔móvil.
- (−) Dos implementaciones (Kotlin y Rust) de router/host: mitigado con contratos + fixtures compartidas.
- (−) Durante M0–M3 conviven tres frontends; el legacy solo recibe fixes.
