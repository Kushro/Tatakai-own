# 04 — Plan de implementación

> **Estado: aprobado con enmiendas.** Esta versión ya incorpora `05-review.md` (E1-E12) y `06-user-decisions.md` (prevalece sobre todo lo anterior): Gradle 9.3.1 / AGP 8.13.2 / Kotlin 2.4.0 / JDK 25, Media3 1.11.1, quickjs-kt 1.0.15,
> CameraX + zxing (sin GMS), mDNS/TOFU fuera de M0, apps AGPL-3.0-or-later (se permite copiar de los repos del usuario), solo `en` + `es`, un git worktree por rama.
> Las desviaciones respecto del borrador original están marcadas **(desviación)**.

## 1. Alcance de ESTE PR: "M0 — Fundaciones + rebanada vertical"
Una reescritura con paridad de ~227k LOC no cabe en un PR. Este PR entrega **código nuevo en directorios nuevos**, sin tocar el legacy
(que sigue publicándose), y deja funcionando el core loop en ambas plataformas nativas:

**Incluye**
- `packages/tokens` (JSON + schema + generador → Kotlin/CSS) y `contracts/` (rutas del host de extensiones, Link v1 OpenAPI JSON, fixtures de metadata, tablas Supabase de M0).
- **Android nativo** (`apps/android-native`, Kotlin/Compose M3E, `app.tatakai.own.next`): shell adaptable (bar/rail), Inicio (anime) → Buscar → Detalles → Player Media3 con cabeceras;
  router de metadata (AniList + ani.zip + Kitsu); login Supabase email/contraseña + `watch_history` local-first con sync; host QuickJS de `.kai` (instalar desde archivo, rutas `sources/stream/providers`);
  **biblioteca local** ("Continuar viendo" + historial, Room); Ajustes (apariencia, metadata, cuenta); **cliente Link** (emparejar por QR con CameraX+zxing o pegando el código, sesión de transcodificación remota con fuente `url`, reproducción mTLS).
- **Desktop nativo** (`apps/desktop-native`, Rust + Dioxus 0.7): shell con rail, Inicio/Buscar/Detalles/Ajustes/Extensiones/Enlace; player libmpv embebido con presets Anime4K/FSR;
  router de metadata + Supabase REST + historial local (rusqlite); host rquickjs de `.kai`; **servidor Link** (CA, pairing por QR/código, capabilities, sesión `url` con fetcher SSRF y, si B5 llega, `library` de una carpeta no recursiva; FFmpeg+libplacebo Anime4K, HLS fMP4).

**No incluye (diferido)** — ver roadmap §8: **mDNS + TOFU, `/v1/settings`, biblioteca remota de la PC más allá de una carpeta, listas AniList/MAL en Biblioteca (E7)**, japonés (`ja`), manga/novela, reader, descargas, torrents/debrid, watch rooms, comunidad, TV, traducción, motor mpv Android, handoff/control remoto, sync de biblioteca vía Link, capas Mangayomi/Stremio/Seanime, instalador/firmado/auto-update desktop.

**Quick wins de perf en el legacy React**: **fuera de este PR**. `redesign/upstream-sync` reescribe justo los archivos implicados (Index, Search, Trending, ExtensionHub, MangaReader); tocarlos en paralelo garantiza conflictos.
Se proponen como PR chico `perf/legacy-quick-wins` *después* de mergear upstream-sync (lista en §9).

## 2. Ramas
Padre: **`redesign/m3e-native`** (desde `main`). PR único `redesign/m3e-native → main`. Todas las sub-ramas se crean desde el padre (después de B1) y se mergean al padre.
**VCS (06 #7)**: el repo NO está gestionado por GitButler. Cada rama tiene su propio **git worktree** en `E:/Gitbutler Repos/Tatakai-own-wt/<nombre>` (`foundation`, `android-shell`, ...); se commitea con `git commit` plano dentro del worktree, sin push. Esto elimina el problema de working-dir compartido de E3.
Ninguna rama toca `src/`, `android/`, `desktop/`, `supabase/`, `package.json`, `package-lock.json` (excepción única: B1 agrega 2 globs a `eslint.config.js`).

| # | Rama | Dueño exclusivo de (paths) | Ola | Depende |
|---|---|---|---|---|
| B1 | `m3e/foundation` | `packages/tokens/**`, `contracts/**`, `docs/redesign/**`, `apps/README.md`, LICENSE/THIRD_PARTY_NOTICES de cada app, **esqueletos** de `apps/android-native/` (settings.gradle.kts, build.gradle.kts raíz, gradle.properties, gradle/libs.versions.toml, gradle wrapper, `build-logic/**`, un `build.gradle.kts` placeholder + `.gitkeep` por módulo) y de `apps/desktop-native/` (Cargo.toml workspace, rust-toolchain.toml, `.cargo/config.toml`, `Cargo.toml` + `src/lib.rs` placeholder por crate), los 2 archivos generados (`…/designsystem/generated/Tokens.kt`, `crates/tk-ui/assets/tokens.css`), `eslint.config.js` (solo `ignores`) | 0 | — |
| B2 | `m3e/android-shell` | `apps/android-native/app/**`, `core/designsystem/**` (salvo `generated/`), `core/ui/**`, `feature/{home,search,details,library,settings,extensions,link}/**` | 1 | B1 |
| B3 | `m3e/android-data` | `core/{model,network,metadata,database,supabase,data}/**` | 1 | B1 |
| B4 | `m3e/android-media` | `core/{extensions,extensions-quickjs,player,link}/**`, `feature/player/**` | 1 | B1 |
| B5 | `m3e/desktop-core` | `apps/desktop-native/crates/{tk-core,tk-ext,tk-link}/**` | 1 | B1 |
| B6 | `m3e/desktop-app` | `apps/desktop-native/crates/{tk-player,tk-ui,tk-app}/**` (salvo `tk-ui/assets/tokens.css`), `apps/desktop-native/packaging/**`, `apps/desktop-native/Dioxus.toml` | 1 | B1 |

Al mergear B1, la propiedad de cada placeholder pasa al dueño de ese directorio (puede sobrescribirlo). `libs.versions.toml`, `settings.gradle.kts`, `build.gradle.kts` raíz, `gradle.properties`, `build-logic/**` y el `Cargo.toml` raíz (+ `Cargo.lock`, `rust-toolchain.toml`) quedan **congelados** tras B1:
si una rama necesita una dependencia nueva lo anota en `NOTES-deps.md` de su módulo y el integrador la agrega (evita colisiones).

**Olas**: Ola 0 = B1 (≈2–3 h). Ola 1 = B2…B6 en paralelo (5 agentes). Ola 2 = integración (1 agente: merges, build final, una ronda de fixes).

## 3. Contratos (los agentes de la Ola 1 programan contra esto; B1 los materializa)

### 3.1 Gradle (B1) — entregado en `apps/android-native/`
- **Toolchain (E1, desviación)**: **Gradle 9.3.1** (wrapper copiado de Miko), **AGP 8.13.2**, **Kotlin 2.4.0**, **KSP 2.3.12** (KSP ya no sigue la línea `<kotlin>-x.y.z`; `2.3.x` funciona con Kotlin 2.4.0), Compose BOM **2026.06.01** (resuelve material3 `1.5.0-alpha15`, que el catálogo fija), minSdk 26, target/compile 36, JVM target 17. **Se compila con el JDK 25 Temurin del host** (`JAVA_HOME`); Android Studio/JBR 21 no existen aquí. SDK en `sdk.dir=D:/Android-SDK` (`local.properties`, gitignored).
- **Convention plugins** (`build-logic`, precompiled script plugins): `tatakai.android.application`, `tatakai.android.library`, `tatakai.android.compose`, `tatakai.jvm.library`, `tatakai.android.feature` (= library + compose + core:model/designsystem/ui + Koin + lifecycle). Accessors tipados de proyecto activados (`projects.core.model`).
- Módulos JVM puros (tests baratos sin Android): `:core:model`, `:core:metadata`, `:core:extensions`. **(desviación)** `:core:link` es **librería Android** (Keystore/`X509KeyManager`/TLS de Android); su parser de payload y cliente REST siguen siendo testeables en JVM. Android: el resto.
- **Catálogo** (`gradle/libs.versions.toml`, claves exactas; versiones verificadas con `./gradlew` — todo el catálogo resuelve): `androidx-core-ktx`, `androidx-activity-compose`, `androidx-lifecycle-*`, `navigation3-runtime/-ui` (1.0.1), `material3-adaptive*` (1.3.0, incl. `-navigation3`), `compose-material3` (1.5.0-alpha15) y `compose-material3-adaptive-navigation-suite`, `materialkolor` (5.0.0-alpha07), `coil-compose`/`coil-network-okhttp` (3.5.0), `okhttp` (5.3.2), `kotlinx-serialization-json` (1.11.0), `kotlinx-coroutines-*` (1.11.0), `koin-*` (4.2.0, BOM), `room-*` (2.8.4) + `ksp`, `work-runtime` (2.11.2), `androidx-datastore-preferences`, `supabase-bom` (3.8.0)/`auth-kt`/`postgrest-kt` + `ktor-client-okhttp` (3.6.0), `media3-*` (**1.11.1**: common/exoplayer/-hls/-dash/ui-compose/session/datasource-okhttp), `quickjs-kt` (**1.0.15**), `jsoup` (1.22.2), **CameraX 1.6.2 (`androidx-camera-*`) + `zxing-core` 3.4.1 — sin `play-services-code-scanner`** (06 #3: QR sin GMS, permiso de cámara pedido en contexto, siempre con alternativa "pegar código"), `junit`, `kotlin-test`, `kotlinx-coroutines-test`, `mockwebserver3`, `turbine`.
- **Configuración (E8)**: `:app` inyecta `TATAKAI_SUPABASE_URL` / `TATAKAI_SUPABASE_ANON_KEY` (de `local.properties` o variables de entorno) como `BuildConfig`; la tarea `checkTatakaiConfig` (ligada a `preBuild` de `:app`) falla con mensaje claro si faltan, sin afectar `help`/`dependencies`/otros módulos.
- **Fuentes de las APIs alpha (E2)**: `apps/android-native/.deps-sources/` (gitignored; `node apps/android-native/scripts/fetch-deps-sources.mjs`). Ninguna API de M3E/Nav3/Adaptive/Media3-Compose se usa sin haberla encontrado con grep ahí.

### 3.2 Kotlin `:core:model` (B3 implementa; B2/B4 consumen) — paquete `app.tatakai.next.model`
```kotlin
enum class MediaKind { ANIME, MANGA }
enum class FeedKind { TRENDING, POPULAR, TOP_RATED, AIRING, UPCOMING, SEASONAL, RECENTLY_COMPLETED }
@Serializable data class MediaRef(val anilistId: Int? = null, val malId: Int? = null, val kitsuId: Int? = null)
@Serializable data class Media(val ref: MediaRef, val kind: MediaKind, val title: String, val titles: Map<String,String>,
  val cover: String?, val banner: String?, val color: String?, val synopsis: String?, val genres: List<String>,
  val episodes: Int?, val status: String?, val season: String?, val year: Int?, val score: Int?, val sourceProvider: String)
@Serializable data class Episode(val number: Int, val title: String?, val image: String?, val airDate: String?, val isFiller: Boolean = false)
data class FeedPage(val items: List<Media>, val page: Int, val hasNext: Boolean)
@Serializable data class SubtitleTrack(val url: String, val lang: String, val label: String, val format: String)
@Serializable data class StreamSource(val url: String, val headers: Map<String,String>, val container: Container /* HLS, MP4, DASH */,
  val quality: String?, val provider: String, val extensionId: String, val subtitles: List<SubtitleTrack> = emptyList())
@Serializable data class PlayerArgs(val media: MediaRef, val title: String, val episode: Int, val startMs: Long = 0, val remoteSessionId: String? = null)
data class HistoryEntry(val animeId: String, val title: String, val poster: String?, val episodeId: String, val episode: Int,
  val progressSec: Int, val durationSec: Int?, val completed: Boolean, val watchedAt: Instant, val anilistId: Int?, val malId: Int?)
sealed interface LoadState<out T> { data object Loading; data class Ok<T>(val value: T); data class Err(val cause: AppError) }
sealed interface AppError { data object Offline; data class RateLimited(val provider: String); data class Extension(val id: String, val msg: String); data class Http(val code: Int); data class Unknown(val msg: String) }

interface MetadataRepository { suspend fun feed(kind: MediaKind, feed: FeedKind, page: Int = 1): FeedPage
  suspend fun search(kind: MediaKind, query: String, page: Int = 1): FeedPage
  suspend fun media(kind: MediaKind, ref: MediaRef): Media?
  suspend fun episodes(ref: MediaRef): List<Episode> }
interface HistoryRepository { fun continueWatching(limit: Int = 20): Flow<List<HistoryEntry>>
  suspend fun record(entry: HistoryEntry); fun progressFor(anilistId: Int): Flow<Map<Int, HistoryEntry>> }
interface AuthRepository { val session: StateFlow<UserSession?>; suspend fun signIn(email: String, password: String): Result<Unit>; suspend fun signOut() }
interface SettingsRepository { val settings: StateFlow<AppSettings>; suspend fun update(transform: (AppSettings) -> AppSettings) }
// AppSettings: paletteId, amoled, contrast, dynamicColor, providerPriority: List<String>, playerHeadersDebug=false
interface ExtensionRepository { val installed: StateFlow<List<InstalledExtension>>
  suspend fun installFromUri(uri: String): Result<InstalledExtension>; suspend fun setEnabled(id: String, enabled: Boolean); suspend fun remove(id: String)
  suspend fun resolveStreams(media: Media, episode: Int): List<StreamSource> }   // InstalledExtension: id,name,version,type,permissions,compat(FULL|PARTIAL|UNSUPPORTED),enabled
interface LinkRepository { val state: StateFlow<LinkState>; fun discover(): Flow<List<DiscoveredPc>> /* M0: siempre vacío (mDNS es M1); el emparejamiento es por payload QR/pegado */
  suspend fun pair(payload: String, deviceName: String): Result<PairedPc>; suspend fun capabilities(): Result<LinkCapabilities>
  suspend fun startSession(source: LinkSource, profileId: String?): Result<RemoteSession>; suspend fun stop(sessionId: String); suspend fun forget() }
```
**Claves de historial (E9)**: `HistoryEntry.animeId` = `String(anilistId)` (o `"mal:<malId>"`), `episodeId` = `"<animeId>?ep=<episode>"`, `completed` si progreso/duración ≥ 0,9, `malId`/`anilistId` solo si no son nulos; nombres de columna en `contracts/supabase/m0-tables.md` (`anime_id`, `anime_name`, `anime_poster`, `episode_id`, `episode_number`, `progress_seconds`, `duration_seconds`, `completed`, `watched_at`, `mal_id`, `anilist_id`).
Koin: cada módulo expone `val <nombre>Module: Module` — `networkModule, metadataModule, databaseModule, supabaseModule, dataModule` (B3), `extensionsModule, playerModule, linkModule` (B4).
`:app` (B2) los lista en `startKoin`. B4 expone `@Composable fun PlayerScreen(args: PlayerArgs, onBack: () -> Unit)` en `feature/player` y `fun LinkHttpClientFactory` interno.
Rutas Navigation3 (B2, `:app`): `Home, Search, Details(kind, anilistId), Player(args), Library, Settings, Extensions, Link`.

### 3.3 Rust (B1 crea workspace; B5/B6 implementan) — `apps/desktop-native`
Toolchain **1.94.1** (`rust-toolchain.toml`, igual que Miko-Kagura/Temple), edition 2024, resolver 3. `cargo metadata` resuelve y `cargo check --workspace --no-default-features` compila los placeholders (sin aws-lc/cmake en el árbol). El `Cargo.toml` real manda sobre esta lista. `[workspace.dependencies]` (congelado): `dioxus = { version = "0.7.10", features = ["desktop","router"] }`, `tokio` (full), `reqwest 0.12` (rustls-tls, json, stream; sin default), `serde`/`serde_json`, `thiserror`, `anyhow`, `tracing`, `async-trait`, `rusqlite 0.32` (bundled, la versión en caché), `rquickjs 0.9` (features `futures`, `loader`, `parallel`), `scraper`, `axum 0.8`, `hyper`, `rustls 0.23` (**`ring`**, sin aws-lc), `tokio-rustls`, `rcgen` (ring), `x509-parser`, ~~`mdns-sd`~~ (**fuera de M0**, se agrega en M1), `libmpv2 = "4"`, `windows-sys`/`windows` (Win32 HWND, Job Objects, DPAPI), `raw-window-handle`, `directories`, `url`, `ipnet`, `hickory-resolver`, `m3u8-rs`, `qrcode`, `zip`, `sha2`, `base32` (+ `hmac`, `hex`, `base64`, `bytes`, `futures-util`, `tokio-util`, `http-body-util`, `hyper-util`, `uuid`, `time`, `tracing-subscriber`, `tempfile` añadidas por B1 porque el árbol queda congelado).
```rust
// tk-core (B5)
pub mod model;   // Media, MediaRef, Episode, FeedKind, MediaKind, StreamSource{url,headers,container,quality,provider,extension_id,subtitles}, HistoryEntry
#[async_trait] pub trait MetadataProvider: Send + Sync { fn id(&self) -> &'static str; fn kinds(&self) -> &[MediaKind];
  async fn feed(&self, k: MediaKind, f: FeedKind, page: u32) -> Result<FeedPage, ProviderError>;
  async fn search(&self, k: MediaKind, q: &str, page: u32) -> Result<FeedPage, ProviderError>;
  async fn media(&self, k: MediaKind, r: &MediaRef) -> Result<Option<Media>, ProviderError>;
  async fn episodes(&self, r: &MediaRef) -> Result<Vec<Episode>, ProviderError>; }
pub struct MetadataRouter; // new(Vec<Arc<dyn MetadataProvider>>, priority: Vec<String>) ; mismas firmas que el trait; cooldown 60 s por proveedor
pub struct Supabase;       // new(url, anon_key); sign_in_password; refresh; upsert_history(&[HistoryEntry]); select_history(limit)
pub struct LocalStore;     // rusqlite en %APPDATA%\Tatakai-Next\data.db: history, settings, cache(ttl)
// tk-ext (B5)
pub struct ExtensionHost;  // install_kai(path) -> InstalledExtension ; list() ; set_enabled ; remove
impl ExtensionHost { pub async fn call(&self, ext_id: &str, route: &str, params: serde_json::Value) -> Result<serde_json::Value, ExtError>;
  pub async fn resolve_streams(&self, media: &Media, episode: u32) -> Vec<StreamSource>; }
// tk-link (B5)
pub trait LinkHost: Send + Sync + 'static { fn library(&self) -> Vec<LibraryItem>; fn library_path(&self, id: &str) -> Option<PathBuf>; fn ffmpeg_path(&self) -> Option<PathBuf>; }
pub async fn start(cfg: LinkConfig, host: Arc<dyn LinkHost>) -> Result<LinkHandle, LinkError>;
impl LinkHandle { pub fn open_pairing_window(&self) -> PairingPayload /* uri + code + expires */; pub fn devices(&self) -> Vec<Device>;
  pub fn revoke(&self, id: &str); pub fn events(&self) -> tokio::sync::broadcast::Receiver<LinkEvent>; pub async fn shutdown(self); }
// tk-player (B6) — capas según E5: ventana principal transparente + hijo WS_CHILD en HWND_BOTTOM (wid de mpv) + "hueco" en la web UI; SIN ventana overlay aparte
pub struct Player; // new(parent_hwnd: isize) -> Result<Self>; load(&StreamSource, start_secs); play/pause/seek/set_volume
  // set_shader_preset(ShaderPreset::{Off, Anime4kA, Anime4kAA, Anime4kB, Anime4kC, Fsr}); set_rect(x,y,w,h); events() -> Receiver<PlayerEvent{Position,Duration,Paused,Eof,Error,Tracks}>
```
`tk-app` (B6) = binario `tatakai-next`; compone tk-ui + tk-core + tk-ext + tk-link + tk-player. Feature cargo `mpv` (default) en tk-player: sin ella compila con un `NullPlayer` (para `cargo test` sin libmpv).

### 3.4 Tokens (B1) — entregado en `packages/tokens/`
Esquema, API Kotlin y selectores CSS exactos: `01-design-system.md §1`. Resumen: `object TatakaiTokens { palettes: Map<String, PaletteSchemes>, palette(id, ContrastLevel), shapes, motion, spacing, elevation, density, typography(fonts, scale) }` con
`data class PaletteSchemes(light, dark, amoled)` (constructor completo de `ColorScheme` 1.5, 48 roles). CSS: `--md-sys-color-<role>`, `--md-sys-typescale-<role>-{font,size,line-height,weight,weight-emphasized,tracking}`, `--md-sys-shape-corner-<size>`,
`--md-sys-motion-<spatial|effects>-<fast|default|slow>-{duration,easing}`, `--tk-space-<n>`; paleta por `:root[data-palette=…][data-mode=light|dark|amoled][data-contrast=medium|high]`. **(desviación)** `typography` es una función que recibe las `FontFamily` de la app.

### 3.5 Link v1 — `contracts/link/openapi.v1.json` (OpenAPI 3.1 con los trims E6/E7; 26 códigos de error; `03-pairing-upscale.md` explica el diseño). Puerto 47321; esquema `tatakai-link:v1`; cabecera `Tatakai-Link: v1`.
### 3.6 Extension host — `contracts/extension-host/routes.v1.json` (rutas derivadas de `host-server.cjs` + shapes de `src/core/extensions/sdk/types.ts` y `abstract-source.ts`), `globals.v1.md` (globals, `require`, niveles de compatibilidad) y `compat.mjs` (análisis de referencia). Fixtures: `contracts/fixtures/extensions/{synthetic-full,synthetic-fs}` (fuente) y `packed/*.kai` (zips deterministas; `node contracts/fixtures/extensions/build.mjs`): uno `full` y otro con `require('fs')` → `unsupported`, que ambos hosts deben pasar (`expected.stream.json`, `expected.json`).
### 3.7 Metadata y Supabase — `contracts/fixtures/metadata/` (AniList trending/search/rate-limited, ani.zip, Kitsu) con `expected.json` generado por `reference-mapper.mjs` (el mapeo a `Media`/`Episode`/`FeedPage` es código ejecutable) y `contracts/supabase/m0-tables.md` (E9).

## 4. Tareas y criterios de aceptación por rama (sin builds completos)
**B1 foundation** (hecha) — tokens.json (10 paletas + AMOLED + 3 niveles de contraste), schema, `generate.mjs`, generados; contratos; esqueletos Gradle/Cargo; `apps/README.md`; docs.
✔ `node --test packages/tokens/test contracts/test` (schema válido, regenerar = sin diff, contraste AA con los 9 pares de E12, contratos, guard). ✔ `cargo metadata --manifest-path apps/desktop-native/Cargo.toml` y `cargo check --workspace --no-default-features`. ✔ Gradle: `./gradlew help` + resolución de dependencias de **todos** los módulos (E2) + `:core:designsystem:compileDebugKotlin` (valida `Tokens.kt` contra material3 1.5.0-alpha15). ✔ `npm run type-check` sigue verde. ✔ guard `tatakai\.me|tatakaiapi|sentry|datadog|analytics` = 0 en `apps/ packages/ contracts/`.

**B2 android-shell** — Activity única, `TatakaiTheme` (tokens generados + materialKolor dinámico + AMOLED + MotionScheme expressive), `NavigationSuiteScaffold`, Navigation3 con list-detail en expanded, pantallas Home/Search/Details/Library(local)/Settings/Extensions/Link con ViewModels contra §3.2, componentes (MediaCover, MediaCard, EpisodeRow, SectionCard, AdaptiveSheet = `ModalBottomSheet` en compact + side sheet custom en wide [E11], Empty/Error/Skeleton), SplashScreen API, **strings `values/` + `values-es/` solamente** (lint `HardcodedText` como error), sin `haze` en M0, escáner QR con CameraX+zxing y "pegar código". Fuentes empaquetadas con su `OFL.txt`.
✔ ViewModel tests con fakes de §3.2 (JVM, `kotlinx-coroutines-test` + turbine) escritos; ✔ revisión estática: toda lista es Lazy con `key`; ningún `collectAsState` sin `WithLifecycle`; sin `tatakai.me`. (Compila en integración.)

**B3 android-data** — (usar `contracts/supabase/m0-tables.md` y `contracts/fixtures/metadata/expected.json` como oráculos) OkHttp compartido (sin enviar sesión Supabase a terceros), router de metadata (AniList GraphQL, ani.zip, Kitsu; prioridad + cooldown + fallback, mapeo a `Media`), Room (history, cache TTL), supabase-kt auth+postgrest, `HistoryRepositoryImpl` local-first + `HistorySyncWorker` (upsert `onConflict user_id,episode_id`, coalescido 15 s), `SettingsRepository` (DataStore).
✔ `./gradlew :core:model:test :core:metadata:test` (JVM, único run permitido): router fallback/cooldown, mapeo contra `contracts/fixtures/metadata/*.json` con MockWebServer.

**B4 android-media** — `:core:extensions` (JVM: parse/validación manifest, análisis de compat, dispatcher de rutas, política de red `network:domain:*`, interfaz `JsEngine`), `:core:extensions-quickjs` (impl quickjs-kt: 1 runtime/extensión, 64 MiB, timeout 15 s, shims), instalación por SAF a almacenamiento interno, `:core:player` (ExoPlayer + `OkHttpDataSource` con cabeceras por fuente, MediaSession, PiP, fallback automático a la siguiente fuente ×3, reporte de progreso a `HistoryRepository`), `feature/player` (controles M3E, gestos, chips), `:core:link` (payload parser que rechaza `v0`/expirados, CSR + clave P-256 de software sellada con AES-GCM de AndroidKeyStore [E4, modelo de Enigmata; copiable], `X509KeyManager`, pin SHA-256 de la CA, cliente REST Link v1 según `contracts/link/openapi.v1.json`, **sin NsdManager/mDNS**, latido 4 s; reproduce HLS remoto con OkHttp mTLS en Media3).
✔ `./gradlew :core:extensions:test :core:link:test` (JVM): manifest válido/ inválido, fixtures `contracts/fixtures/extensions/packed/*.kai` → compat correcto (`expected.*`), parser de payload (v1 ok, v0 rechazado, expirado), errores Link mapeados.

**B5 desktop-core** — tk-core (modelos, router + 3 proveedores, Supabase REST, LocalStore), tk-ext (rquickjs host con shims y política de red; instalación `.kai`), tk-link (CA/certs rcgen, pairing window, mTLS axum, revocación, capabilities con detección de encoders vía `ffmpeg -encoders`, sesiones `url` (+ `library` de una carpeta si llega), fetcher SSRF con reescritura HLS [`-protocol_whitelist http,tcp,crypto`, E6], **sin mDNS**, builder de args FFmpeg + perfiles Anime4K, HLS on-demand + ventana RAM, latido/idle, Job Object).
✔ `cargo test -p tk-core -p tk-ext -p tk-link` (único run): fixtures de metadata, fixtures `.kai`, tabla SSRF (IPs privadas/loopback/rebinding [pública y luego privada]/redirect a IP privada/redirect cross-origin quita Cookie/`#EXT-X-KEY` absoluta a host no permitido/cabecera con CR-LF/`data:`-`file:` en playlists/playlist > 2 MiB; E6), pairing (código, intentos, expiración), args FFmpeg exactos por perfil (golden strings). ✔ `cargo clippy -p …` sin warnings nuevos.

**B6 desktop-app** — tk-player (libmpv2, child HWND con `wid`, `vo=gpu-next`, cabeceras via `http-header-fields`/`referrer`/`user-agent`, presets de shaders, `NullPlayer`), tk-ui (Router Dioxus, rail/drawer, title bar sin marco, pantallas, componentes de `01 §6`, CSS a partir de `tokens.css`, fuentes empaquetadas), player con las capas de E5 (ventana principal transparente + hijo `WS_CHILD` en `HWND_BOTTOM` + hueco en la UI web; los controles son la propia UI; **sin** ventana overlay; fallback solo si falla la transparencia: OSC de mpv), i18n `tk-ui/i18n/{en,es}.json` + `t("key")`, tk-app (wiring, single-instance, logs a `%APPDATA%\Tatakai-Next\logs`), `packaging/fetch-deps.ps1` (libmpv + ffmpeg + Anime4K por lock JSON con SHA-256; **lo ejecuta el usuario**, nunca el build), `Dioxus.toml`.
✔ `cargo check --workspace --no-default-features` (sin libmpv; `cargo check` no enlaza, pero así tampoco hace falta la import lib), ✔ tests de estado puro de UI (`model.rs` sin Dioxus) y de mapeo preset→`glsl-shaders`.

**Reglas comunes para todos los agentes** (copiar en cada brief; versión completa en `apps/README.md`): reglas de seguridad de comandos destructivos del usuario (CLAUDE.md global, textual); los tests nunca escriben en directorios reales del usuario (usar `tempfile`/`TemporaryFolder`/paths inyectados); no `*.tatakai.me`, no TatakaiAPI, no Sentry/Datadog/analytics (guard en `contracts/test`); **se permite copiar de los repos propios del usuario (06 #1) conservando los avisos de copyright y anotando el origen en `apps/<app>/THIRD_PARTY_NOTICES.md`**; commits con `git commit` plano en el worktree de la rama (el repo no es GitButler), sin push; no editar archivos fuera de su columna de propiedad; **ninguna API de M3E/Nav3/Adaptive/Media3-Compose sin grep en `.deps-sources/` (E2)**; presupuesto E3: cada rama Kotlin puede correr **una** vez `./gradlew :<sus módulos>:compileDebugKotlin` (y un test-run JVM) al cierre, Rust `cargo check -p <crate>` libre y un `cargo test`; en Windows/Git Bash nunca `> NUL`.

## 5. Integración (Ola 2) y builds finales
Orden de merge al padre: B1 → B3 → B4 → B2 → B5 → B6 (sin conflictos esperados por propiedad disjunta; B2 después de B3/B4 para compilar `:app` una sola vez con todos los módulos Koin presentes). Luego el integrador agrega deps de los `NOTES-deps.md` (con coordenadas completas, versión y motivo) y corre **una vez**:
1. `node --test packages/tokens/test contracts/test`
2. `npm run type-check` (legacy intacto) y `npm test`
3. `cd apps/android-native && ./gradlew.bat testDebugUnitTest assembleDebug` (JDK 25 Temurin; requiere `TATAKAI_SUPABASE_*` en `local.properties`; sin NDK). Antes, iterar con `compileDebugKotlin` por módulo (E3).
4. `cd apps/desktop-native && cargo test --workspace --no-default-features && cargo clippy --workspace --no-default-features && cargo check -p tk-player --features mpv` (esto último no enlaza pero valida que el código mpv compila)
5. *(Opcional, si el usuario ejecutó `fetch-deps.ps1`)* `cargo build --release -p tk-app` y/o `dx bundle --release`
6. Humo manual (usuario): instalar APK debug, abrir Inicio → Detalles → reproducir con un `.kai` propio; emparejar con la PC por QR; reproducir con perfil `p-anime4k-1080`. Registrar tamaño del APK y arranque en frío (`adb shell am start -W`) como baseline para M1.
7. Commit aparte del integrador: `package.json` `"license": "MIT"` → `MPL-2.0` (06 #1).
Una ronda de fixes del integrador (presupuestar una segunda mini-ronda); si quedan fallos se documentan como known-issues en la descripción del PR.

## 6. Estimación
| Rama | Agente-horas | Notas |
|---|---|---|
| B1 | 2–3 | generador + contratos + esqueletos |
| B2 | 8–10 | UI amplia |
| B3 | 7–9 | |
| B4 | 10–12 | la más riesgosa (QuickJS + mTLS Keystore) |
| B5 | 10–12 | tk-link es lo más grande |
| B6 | 8–10 | HWND/overlay |
| Integración | 3–5 | 1 build Android + 1 cargo test; 1 ronda de fixes |
| **Total** | **≈ 70–90 agente-h** (corregido por la revisión: la estimación original de 50–60 era optimista) | **Wall-clock ≈ 16–24 h** (3 h ola 0 + 10–14 h ola 1 + 4–6 h integración) |

## 7. Riesgos
| # | Riesgo | Prob./Impacto | Mitigación |
|---|---|---|---|
| R1 | Kotlin sin compilar hasta la integración → muchos errores de una vez | Alta/Medio | Contratos exactos §3.2, módulos JVM con 1 test-run por rama, ronda de fixes presupuestada; integrador puede correr `compileDebugKotlin` por módulo para iterar barato |
| R2 | Overlay de controles sobre el HWND de mpv en WebView2 (z-order, DPI, resize) | Media/Alto | Ventana owned transparente; fallback documentado a OSC de mpv; se acepta para M0 |
| R3 | Compat de bundles `.kai` reales en QuickJS (dependen de builtins Node) | Alta/Medio | Nivel de compat visible; shims mínimos; fixtures; los legacy siguen en Electron |
| R4 | Versiones pre-release (M3E alpha, materialKolor alpha, Navigation3, quickjs-kt) | Media/Medio | Versiones exactas ya pinneadas y resueltas (todo el catálogo resuelve, E2); sources jars para grep; fallback `quickjs-android` y Navigation Compose |
| R5 | JDK 25 en el host (no hay Android Studio ni JBR 21) | Resuelto | Gradle 9.3.1 + AGP 8.13.2 + Kotlin 2.4.0, el combo que Miko compila aquí (E1); `./gradlew help` y la resolución completa verificados en B1 |
| R6 | FFmpeg/libplacebo/FG solo medidos en AMD; NVIDIA/Intel sin probar | Media/Medio | Perfiles con `measured_sustain`; `p-none` siempre disponible; FG servidor solo AMD |
| R7 | Seguridad del fetcher (SSRF) y de mTLS | Baja/Alto | Tabla de tests obligatoria; revisión adversarial antes del PR |
| R8 | Licencias | Baja/Medio | Apps AGPL-3.0-or-later (06 #1) → se puede copiar de los repos del usuario con avisos y `THIRD_PARTY_NOTICES.md`; avisos MIT/LGPL/OFL en Acerca de; `package.json` MIT→MPL-2.0 en commit aparte |
| R9 | `rquickjs`/`rusqlite bundled` compilan C → MSVC requerido | Baja/Bajo | Host ya compila Rust nativo (Temple/Kagura) |

## 8. Roadmap
- **M1** — **mDNS + TOFU en Link, `/v1/settings`, biblioteca remota de la PC, listas AniList/MAL en Biblioteca (lectura y escritura) con vinculación OAuth (E7)**, Manga/novela (detalles, Reader con pinch-zoom y prefetch), descargas Android (reusar lógica del servicio Java actual reescrita en Kotlin), motor mpv Android + Anime4K local, escritura de listas AniList/MAL, Link: handoff + control remoto + eventos, baseline profile + macrobenchmark, MAL/Shikimori/Tenrai en el router.
- **M2** — Watch rooms (realtime), torrents (libtorrent4j / librqbit desktop), debrid, sync de biblioteca/descargas vía Link, fuente `phone` (canal de rangos), TV (Compose for TV), Stremio compat, proceso `:extensions` aislado, HEVC.
- **M3** — Traducción on-device (desktop con `ort`), comunidad, stats/wrapped, IPTV, RIFE/FG local, LSFG en teléfono, upscale de imágenes en Reader, Mangayomi/Seanime compat, UniFFI core compartido (evaluar).
- **M4** — Paridad: el nativo toma el applicationId/instalador definitivo, migración de datos, retiro de Capacitor/Electron. Instalador NSIS firmado + auto-update propio.

## 9. Quick wins legacy (PR separado tras upstream-sync)
Splash por `ready` en vez de 2500 ms fijos; `GlobalListeners` con throttle (≥60 s) y solo queries visibles; lazy-import de `typescript`, `phonemizer`, `onnxruntime`, HF transformers solo en `/translate`;
Index: secciones bajo el pliegue con `IntersectionObserver`; desactivar blur/glass en móvil por defecto; quitar Sentry/Datadog si quedan; `manualChunks` para framer-motion/recharts/tiptap. Verificación: `type-check` + un `vite build` comparando tamaños.
