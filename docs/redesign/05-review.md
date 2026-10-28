# 05 — Revisión independiente del plan (M0 "M3E nativo")

Revisor: Fable (gate único antes de codificar) · Fecha: 2026-10-27
Entradas: `00..04-*.md`, `research/01..04`, y verificación directa en Tatakai-own, miko, design_system_android, Enigmata, Enigmata-Temple, limusic-forge, caché Gradle/Cargo del host, Maven Central / Google Maven / crates.io.

## Veredicto: **APROBADO CON ENMIENDAS OBLIGATORIAS**

El diseño es sólido: la ADR elige bien (Compose M3E + Media3 en Android, Dioxus + libmpv en desktop, contratos compartidos en vez de UniFFI prematuro), el sistema de diseño usa M3 Expressive de verdad (`MaterialExpressiveTheme`, `MotionScheme.expressive()`, `MaterialShapes`/`Morph`, `LoadingIndicator`, `FloatingToolbar`, `ButtonGroup`, `WavyProgressIndicator`, `ShortNavigationBar`/`WideNavigationRail` — todos verificados en `material3-android 1.5.0-alpha15`, que es lo que resuelve el BOM 2026.06.01 en la caché del host), la IA/navegación por clase de ventana es correcta, Link v1 extiende Temple con criterio (fuente `url` + fetcher SSRF es la pieza que Temple no tenía) y la propiedad de archivos por rama es realmente disjunta.

Lo que el usuario percibirá en M0: una app Android nueva (`app.tatakai.own.next`) y un binario desktop nuevo con el core loop de anime (Inicio → Buscar → Detalles → Player con `.kai`), tema M3E con 10 paletas/AMOLED/contraste, y enlace PC↔móvil con Anime4K en la PC y en el player desktop. Es rediseño real y medible, no scaffolding — pero es una app **paralela** (manga/novela/descargas/biblioteca completa siguen en el legacy hasta M1–M4). Debe decirse así en la descripción del PR.

Riesgo principal: el alcance de la Ola 1 es grande para 5 agentes Sonnet sin compilador (≈15 módulos Kotlin + 6 crates), y hay cuatro supuestos del plan que **no se cumplen en este host** (JDK, Android Studio, modelo de clave mTLS, overlay del player). Las enmiendas los corrigen y recortan lo prescindible. La estimación de 50–60 agente-h es optimista: cuenten 70–90 y presupuesten una segunda mini-ronda de fixes del integrador.

---

## Enmiendas obligatorias

### E1 — Toolchain Android: usar exactamente el de Miko (04 §3.1, R5; 00 tabla)
Verificado: en el host **solo existe JDK 25** (`JAVA_HOME` = Temurin 25.0.3; los JBR de Rider/DataGrip también son 25). **Android Studio no está instalado**, así que "JBR 21 de Android Studio" no existe. Gradle 8.14.3 no arranca en JDK 25.
- Cambiar a **Gradle 9.3.1 + AGP 8.13.2 + Kotlin 2.4.0** (combo que Miko compila en este host con JDK 25). JVM target 17 se mantiene. KSP pinneado a la línea `2.4.0-*`.
- B1 agrega `apps/android-native/local.properties` al `.gitignore` y documenta `sdk.dir=D:/Android-SDK` (`ANDROID_HOME` no está seteado; el SDK está en `D:/Android-SDK`, confirmado en `android/local.properties`).
- Media3: pinnear **1.11.1** (en caché; `media3-ui-compose` existe desde 1.8 pero 1.8.x no está en el host). `quickjs-kt`: `io.github.dokar3:quickjs-kt:1.0.15` (última estable en Maven Central; no está en caché → se valida en E2).

### E2 — B1 valida la resolución completa de dependencias antes de abrir la Ola 1 (04 §4 B1)
`cargo metadata` ya está. Falta el equivalente Gradle. B1 debe ejecutar **una vez** `./gradlew help` + `./gradlew :app:dependencies --configuration debugRuntimeClasspath` (o `buildEnvironment` + `dependencies` por módulo) hasta que todo el catálogo resuelva (quickjs-kt, supabase-kt BOM, koin, materialKolor 5.0.0-alpha07, navigation3 1.0.1, adaptive-navigation3 1.3.0, play-services-code-scanner, room/ksp). Esto además calienta la caché para que las corridas de la Ola 1 sean cortas.
Añadir: B1 descarga los **sources jars** de `material3-android 1.5.0-alpha15`, `navigation3-runtime 1.0.1`, `adaptive-navigation3 1.3.0`, `media3-ui-compose 1.11.1` (Google Maven: `…/<artefacto>-<ver>-sources.jar`) a `apps/android-native/.deps-sources/` (gitignored) y anota la ruta en `apps/README.md`. **Regla para todo brief Kotlin**: ninguna API de M3E/Nav3/Media3-Compose se usa sin haberla encontrado con grep en esos sources (los alpha cambian firmas; Sonnet alucina parámetros).

### E3 — Presupuesto de compilación (no de build) en la Ola 1 (04 §4, R1)
El plan permite un solo test-run JVM por rama Kotlin; los módulos Android (`core/ui`, `core/player`, `core/link` Android, `feature/*`, `app`) llegarían a integración sin haberse compilado nunca. Con la caché caliente, `compileDebugKotlin` de un módulo cuesta 1–3 min y no produce APK.
- Cada rama Kotlin (B2, B3, B4) puede correr **una** vez `./gradlew :<sus módulos>:compileDebugKotlin` al cierre (además del test-run JVM ya previsto). Scope estricto a sus módulos: como GitButler comparte un único working dir, los WIP de otras ramas pueden romper la configuración; si falla por un módulo ajeno, se reporta y no se arregla.
- Rust: `cargo check -p <crate>` libre (es barato); `cargo test` sigue siendo uno por rama.
- Integración: el integrador itera con `compileDebugKotlin` por módulo antes del único `assembleDebug`.

### E4 — Modelo de clave del cliente Link: copiar el de Enigmata, no Keystore no-exportable (03 §1; 04 B4)
Verificado en `Enigmata/app/.../temple/crypto/TempleCrypto.kt` y `vault/TempleKeyVault.kt`: la clave del cliente es **software P-256**, sellada en reposo con AES-GCM de AndroidKeyStore en `noBackupFilesDir`, y se exporta como PKCS#8 porque mpv (`tls-key-file`) la necesita en archivo. El plan propone clave Keystore `PURPOSE_SIGN` no exportable + `X509KeyManager`: (a) bloquea el motor mpv Android de M1, (b) el soporte de claves Keystore en handshakes TLS varía por OEM. Reescribir con el modelo de Enigmata (clave en memoria vía `X509KeyManager` para OkHttp; PKCS#8 temporal solo cuando llegue mpv). Mantener pin SHA-256 del DER de la CA.

### E5 — Capas del player desktop: patrón limusic-forge, sin ventana overlay aparte (04 B6, R2)
Verificado en `limusic-forge/src-tauri/src/nativevideo_windows.rs`: ventana principal **transparente** (tao/wry, misma base que Dioxus desktop), hijo `WS_CHILD` (clase `STATIC`, `WS_EX_NOACTIVATE`, **sin** `WS_CLIPSIBLINGS`) puesto en `HWND_BOTTOM` bajo WebView2 como `wid` de mpv, y la página deja un "hueco" (`set_rect` en px CSS × scale factor; el hueco solo se abre cuando `vo-configured` es true). Los controles los dibuja la propia UI web encima. Es más simple y ya probado; el overlay como ventana propia transparente tiene problemas de z-order/hit-testing/DPI. Dioxus 0.7.10 lo soporta: `Config::with_window(WindowBuilder::new().with_transparent(true))` + `Config::with_background_color((0,0,0,0))` (verificado en `dioxus-desktop-0.7.10/src/config.rs`). El fallback a OSC de mpv queda solo si la transparencia falla. Se lee limusic como referencia, se **reescribe**.

### E6 — Fetcher SSRF / FFmpeg: completar la lista blanca y los límites (03 §4.1, §7)
- `-protocol_whitelist http,tcp,crypto` (sin `crypto`, HLS con `#EXT-X-KEY` AES-128 falla aunque la clave pase por el fetcher).
- El reescritor de playlists rechaza URIs `data:`, `file:`, esquemas distintos de http(s), limita playlist a ≤ 2 MiB y profundidad master→media→(sin más niveles).
- Tabla de tests de B5 debe incluir: rebinding (DNS devuelve pública y luego privada), redirección a IP privada, `#EXT-X-KEY` con URI absoluta a host no permitido, cabecera con CR/LF.
- Regla Windows Firewall: la regla (perfil Privado) requiere elevación; se crea **solo** desde Ajustes > Enlace > "Permitir en red privada" con prompt UAC, nunca al primer arranque.

### E7 — Recorte de alcance M0 (04 §1; 03 §2–3; 02 §3)
Para que B4 y B5 (las ramas más riesgosas) sean viables en una ola ciega:
- **mDNS + TOFU → M1**. M0 = QR/código pegado (igual que Temple; elimina el flujo TOFU de comparación visual, que era el punto más débil del diseño de seguridad).
- **`GET/PUT /v1/settings` → M1.**
- **`GET /v1/library` y fuente `library`**: en M0 se define como *una carpeta elegida por el usuario, no recursiva, listada con ffprobe*; si B5 no llega, se difiere a M1 y la única fuente de M0 es `url` (que es la que importa para extensiones).
- **Biblioteca Android M0 = local**: "Continuar viendo" + historial (Room). Lectura de listas AniList/MAL requiere tokens OAuth que el login email/contraseña de M0 no vincula → M1 junto con escritura.
- Supabase en desktop: se mantiene, pero toda pantalla funciona sin sesión (local-first ya es regla).
Actualizar 04 §1 "Incluye/No incluye", §3.5 y 02 §3 (Biblioteca, Dispositivo enlazado) en consecuencia.

### E8 — Inyección de configuración y guard de fork (00 Supabase; 04 §3; §4 B1)
No está definido cómo llegan `SUPABASE_URL`/`SUPABASE_ANON_KEY` a los binarios.
- Android: B1 define `TATAKAI_SUPABASE_URL` y `TATAKAI_SUPABASE_ANON_KEY` leídas de `local.properties`/env → `BuildConfig` en `:app` (fallan en build si faltan, con mensaje).
- Desktop: `%APPDATA%\Tatakai-Next\config.json` creado al primer arranque desde `option_env!` o vacío (la app arranca sin cuenta).
- Nada commiteado; documentado en `apps/README.md`.
- Test de guardia en ambos lados: regex `tatakai\.me|tatakaiapi|sentry|datadog|analytics` sobre `apps/ packages/ contracts/` (el plan solo cubre `tatakai.me`). También en `packaging/*.ps1`.

### E9 — Continuidad del historial con el legacy (00 Supabase; 04 B1/B3/B5)
Verificado el esquema real de `public.watch_history` (`supabase/main.sql:1710`): `anime_id text, anime_name text, anime_poster, episode_id text, episode_number int, progress_seconds, duration_seconds, completed, watched_at, mal_id, anilist_id`, con `UNIQUE (user_id, episode_id)` (`20251231031018`). `contracts/supabase/m0-tables.md` (B1) debe copiar ese esquema **literal** y, sobre todo, documentar **el formato exacto de `anime_id` y `episode_id` que escribe el legacy** (grep en `src/` del upsert) para que el nativo escriba las mismas claves y el historial sea el mismo en ambas apps. `HistorySyncWorker` y `Supabase::upsert_history` usan `Prefer: resolution=merge-duplicates` + `on_conflict=user_id,episode_id`. El `HistoryEntry` de 04 §3.2 ya mapea 1:1; renombrar campos a los de la tabla en el doc de contrato.

### E10 — i18n: es/en/**ja** en ambas plataformas (01 §8; 04 B2/B6)
El legacy tiene `src/locales/{en,es,ja}.json`; el plan solo prevé strings en/es en B2 y nada en desktop.
- B2: `values/`, `values-es/`, `values-ja/` (ja puede ir marcado "draft" para revisión), lint `HardcodedText` como error.
- B6: `tk-ui/i18n/{en,es,ja}.json` + helper `t("key")`, idioma del sistema con override en Ajustes. Nada de texto literal en componentes.

### E11 — Componentes que no existen tal cual (01 §6)
`ModalSideSheet` **no existe** en material3 1.5.0-alpha15 (verificado en el AAR). `AdaptiveSheet` = `ModalBottomSheet` en compact y side sheet **custom** (Surface anclado + scrim) en medium/expanded. `HorizontalMultiBrowseCarousel` está en `androidx.compose.material3.carousel` (paquete aparte). Corregir la tabla.

### E12 — Generador de tokens: salida y test deterministas (01 §1, §8; 04 §3.4)
- El generador emite el constructor **completo** de `ColorScheme` 1.5 (incl. `surfaceBright/Dim`, `surfaceContainer{Lowest..Highest}`, `primaryFixed*`, `secondaryFixed*`, `tertiaryFixed*`); AMOLED se deriva del dark por post-proceso documentado.
- El test de contraste enumera los pares exactos: `(onPrimary, primary)`, `(onSecondary, secondary)`, `(onTertiary, tertiary)`, `(onError, error)`, `(onSurface, surface)`, `(onSurfaceVariant, surface)`, `(onPrimaryContainer, primaryContainer)`, `(onSecondaryContainer, secondaryContainer)`, `(onTertiaryContainer, tertiaryContainer)` ≥ 4.5:1 en light/dark/amoled; `mono` y las paletas `Fidelity` pueden declarar excepciones explícitas en `tokens.json` (`"allowContrastFailures": [...]`). Sin esto el criterio ✔ de B1 no es verificable.

---

## Sugerencias (no bloqueantes)
- Fuentes Space Grotesk / Outfit / JetBrains Mono son OFL: empaquetar sus `OFL.txt` junto a los archivos y listarlos en "Acerca de".
- Integrador: registrar del único `assembleDebug` el tamaño del APK y, en el humo manual, el arranque en frío (`adb shell am start -W`) como baseline para M1.
- `rusqlite` 0.32 está en caché; cualquier versión más nueva compila C (bundled) igual. Pinnear la cacheada.
- B2: no usar `haze` en M0 (presupuesto de movimiento §5 ya lo limita; evita una dependencia alpha más).
- En `NOTES-deps.md`, exigir coordenadas completas con versión y motivo; el integrador solo copia.
- Orden de merge propuesto (B1→B3→B4→B2→B5→B6) es correcto; añadir que B2 se mergea después de B3/B4 para que el integrador compile `:app` una sola vez con todos los módulos Koin presentes.
- `cargo test --workspace --no-default-features` deja a `tk-player` en `NullPlayer`: añadir un `cargo check -p tk-player --features mpv` en integración (no enlaza, valida que el código mpv compila).

## Puntos que requieren decisión del usuario
1. **Licencia**: `apps/*` bajo MPL-2.0 (default del plan) y corregir `package.json` `"license": "MIT"` vs `LICENSE` MPL-2.0 en un commit aparte (fuera de este PR, que no toca `package.json`).
2. **Recortes E7** (mDNS, settings compartidos, biblioteca remota, listas AniList en Biblioteca → M1). Afectan a "enlace como Temple" solo en lo que Temple tampoco tiene.
3. **Presupuesto E3**: aceptar `compileDebugKotlin` por módulo en la Ola 1 (son compilaciones, no builds de APK; reducen de forma drástica el riesgo de la única ronda de fixes).
4. **ja** como traducción borrador generada por los agentes (marcada para revisión) vs. dejar `ja` vacío con fallback a `en`.
5. Nombre/ids de convivencia: `app.tatakai.own.next` y `%APPDATA%\Tatakai-Next\`.
