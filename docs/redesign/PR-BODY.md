# Sync upstream 6.4.2 + apps nativas Tatakai (M3 Expressive, M0)

## Resumen

- **Sync upstream 6.3.0 -> 6.4.2** sobre el fork endurecido (detalle en `docs/upstream-sync-6.4.2.md`). La app legacy (Capacitor/Electron/web) queda sin tocar por el rediseño.
- **Apps nativas en paralelo, M0**, en `apps/`:
  - `apps/android-native`: Kotlin 2.4 + Jetpack Compose + Material 3 Expressive (`app.tatakai.own.next`).
  - `apps/desktop-native`: Rust + Dioxus 0.7 + libmpv (`tatakai-next`).
  - Fuente única de diseño: `packages/tokens` (genera `Tokens.kt` y `tokens.css`); contratos compartidos en `contracts/` (Link v1, host de extensiones, router de metadatos).
- Licencia de las apps nativas AGPL-3.0-or-later (decisión 1 de `docs/redesign/06-user-decisions.md`); avisos en cada `THIRD_PARTY_NOTICES.md`.
- Documentación del rediseño: `docs/redesign/` (ADR, sistema de diseño, UX, Link/upscale, plan, review, decisiones, `KNOWN-ISSUES.md`).

## Ramas integradas (merge `--no-ff`)

| Rama | Commits |
|---|---|
| `m3e/foundation` (ya en la base) | tokens, contratos, docs, esqueletos |
| `m3e/android-data` | 7b451ec, 1eb88d7 (model, metadata, network, database, supabase, data) |
| `m3e/android-media` | 503ee42, 724126a, 8524698 (extensiones QuickJS, cliente Link, player Media3) |
| `m3e/android-shell` | a077817, 62b5c65, 738d276 (tema, features, shell Navigation3) |
| `m3e/desktop-core` | cc9228e, 736a160, f7e7f62, 6c3dfc7 (tk-core, tk-ext, tk-link) |
| `m3e/desktop-app` | 342cf5f, 94aa8c8, ba5e60d (tk-player, tk-ui, tk-app, packaging) |

Commits de integración: fix de tokens (matiz del primario), backend `live` de desktop cableado contra la API real, cableado Android (`HistoryFlush`, decorator de ViewModel de Navigation3, compileSdk 37), consolidación de `NOTICES`.

## Qué hace cada app (M0)

- **Android**: shell adaptable (barra/rail por ancho), Inicio, Búsqueda, Detalles, Biblioteca, Ajustes, Extensiones (instalar `.kai` por SAF, QuickJS), player Media3 (cabeceras por fuente, fallback de fuentes, gestos, PiP) y cliente Tatakai Link (QR/pegado, mTLS con CA pineada). Metadatos AniList/ani.zip/Kitsu; historial en Room con sync a Supabase propio.
- **Desktop**: shell Dioxus (en/es) con las mismas pantallas, player libmpv en ventana hija con presets Anime4K/FSR, host de extensiones `.kai` (rquickjs), servidor Tatakai Link (CA/mTLS, emparejamiento, fetcher anti-SSRF, perfiles FFmpeg) e instalador NSIS.

## Cómo compilar y ejecutar

Android (`apps/android-native`, JDK 25 Temurin):

```
# local.properties (gitignored)
sdk.dir=D:/Android-SDK
TATAKAI_SUPABASE_URL=https://TU-PROYECTO.supabase.co
TATAKAI_SUPABASE_ANON_KEY=TU_ANON_KEY
./gradlew.bat testDebugUnitTest assembleDebug
```

Desktop (`apps/desktop-native`):

```
pwsh packaging/fetch-deps.ps1      # libmpv, FFmpeg, shaders, fuentes (lo ejecuta el usuario)
. packaging/deps/env.ps1           # RUSTFLAGS para enlazar libmpv (x64 Native Tools)
cargo run -p tk-app                # features por defecto: mpv + live
```

Configuración: `config.json` en `%APPDATA%\Tatakai-Next` (Supabase propio, prioridad de proveedores) o variables de build; sin Supabase la app corre sin cuenta.

Tokens: `node packages/tokens/generate.mjs` (regenera `Tokens.kt` + `tokens.css`).

## Verificación (resultados exactos)

- `node --test packages/tokens/test contracts/test`: 26 pass / 0 fail (incluye el guard de `tatakai.me`/telemetría y el nuevo test de matiz del primario).
- Legacy tras `npm ci`: `npm run type-check` **132 errores** (línea base 132-135; todos en `src/`, 0 en apps/packages/contracts). `npm test` **1154 pass / 79 fail / 23 errors** (idéntico a la línea base post-sync, delta 0).
- Android: `testDebugUnitTest assembleDebug` BUILD SUCCESSFUL; **52 tests, 0 fallos**; `app-debug.apk` = 38 029 878 bytes (~36,3 MiB).
- Desktop: `cargo test --workspace --no-default-features` **192 tests pass**, 0 fallos; `cargo clippy --workspace --no-default-features --all-targets` sin avisos; `cargo check -p tk-player --features mpv` OK; `cargo check -p tk-app` (live + mpv) OK. El binario no se enlazó ni se ejecutó.
- Guard `tatakai.me|tatakaiapi|sentry|datadog|analytics` sobre `apps/ packages/ contracts/`: solo ocurrencias permitidas (README de apps y el test del propio guard).

## Problemas conocidos

Ver `docs/redesign/KNOWN-ISSUES.md`. Resumen: nada ejecutado en dispositivo ni en Windows real; fuentes Space Grotesk/Outfit no empaquetadas (fallback de sistema); AGP 8.13.2 con Adaptive 1.3.0 requiere compileSdk 37 y desactivar la verificación de metadatos AAR; sesión Supabase de desktop solo en memoria; `signOut` en Android descarta el historial no empujado.

## Elementos de upstream descartados

(Detalle en `docs/upstream-sync-6.4.2.md`.) Submódulo `tatakai-server` y scripts `server:*`; proxy `hoko.tatakai.me`; defaults `db/api.tatakai.me` en `write-client-env`; `useCustomTracker` (ahora no-op); `ContentCatalogManager`; `getNovelHome` (bundle TatakaiAPI); estado BetterStack; página Turnstile hospedada; reescritura de imágenes `wsrv.nl`; `BRAND_ASSET_BASE` remoto; flujo OAuth AniList de upstream; tienda/marketplace de extensiones y `extensions.tatakai.me`; AltStore y enlaces a `Snozxyx`; `StatusPage` de upstream; tests que importan `../TatakaiAPI/**`.

## Acciones del usuario

1. Aplicar las 14 migraciones nuevas (`20261019000000`..`20261026000007`) primero a un Supabase de pruebas; si el proyecto ya aplicó migraciones posteriores del fork, usar `supabase db push --include-all`.
2. **Puente Aniyomi (resuelto)**: los artefactos ya no salen de `Snozxyx/TatakaiServer`. Se compilan desde el código fuente auditado (`aayush2622/DartotsuExtensionBridge@7778da7`) en [Kushro/TatakaiServer-own](https://github.com/Kushro/TatakaiServer-own/releases/tag/compat-runtime-bridge-2.0.17-own), con atestación de procedencia (`gh attestation verify <asset> --repo Kushro/TatakaiServer-own`). El APK de Android salió idéntico byte a byte al del upstream; el JAR de desktop difiere solo en metadatos de empaquetado y queda fijado a nuestro build. Hallazgos medios de la auditoría (sin verificación de checksums de dependencias Gradle, JitPack sin filtro, descarga de JBR sin hash en la ruta de Cloudflare) quedan documentados en el fork.
3. Fuentes Space Grotesk / Outfit (OFL): autorizar su descarga o aportarlas.
4. Ejecutar `fetch-deps.ps1` (libmpv, FFmpeg, Anime4K, FSR, fuentes); revisar hashes (`-Anime4kSource` si difieren).
5. Humo en dispositivo: instalar la APK debug, Inicio -> Detalles -> reproducir con un `.kai` propio; emparejar con la PC por QR; perfil `p-anime4k-1080`; registrar tamaño de APK y arranque en frío como línea base de M1.
6. Crear `local.properties` / `config.json` con el Supabase propio.

## Hoja de ruta

- **M1**: mDNS + TOFU en Link, `/v1/settings`, biblioteca remota de la PC, listas AniList/MAL (lectura/escritura, OAuth), manga/novela, descargas Android, mpv Android + Anime4K, baseline profile.
- **M2**: watch rooms, torrents/debrid, sync por Link, fuente `phone`, TV, Stremio compat, proceso `:extensions` aislado, HEVC.
- **M3**: traducción on-device, comunidad, stats/wrapped, IPTV, RIFE/FG local, Mangayomi/Seanime compat, evaluar UniFFI.
- **M4**: paridad; el nativo toma el applicationId/instalador definitivo, migración de datos, retiro de Capacitor/Electron, instalador NSIS firmado + auto-update.
