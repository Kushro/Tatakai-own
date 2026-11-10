# Problemas conocidos (integración M0)

Resultado de la integración Wave 2 (`redesign/m3e-native`). Nada de esto bloquea la compilación ni los tests listados en `PR-BODY.md`.

## Sin ejecutar nunca (solo compilado / probado en JVM o con mocks)

- **Android**: la APK debug compila (52 tests JVM verdes) pero no se instaló ni se ejecutó en un dispositivo. Sin probar en dispositivo: JNI de QuickJS, timeout de 15 s, handshake mTLS contra un Link real, PiP, `NavigationSuiteScaffold` + `ListDetailSceneStrategy`, scoping de ViewModels por entrada Navigation3.
- **Desktop**: `cargo check -p tk-app` (live + mpv) compila, pero no se enlazó ni se ejecutó (no se hizo `cargo build --release` ni `dx bundle`; libmpv/FFmpeg no están descargados). FFmpeg solo se probó con argumentos golden; el benchmark de sostenibilidad no existe; DASH se rechaza en M0. Los tests de `tk-app` no corren con `live` en el workspace (`--no-default-features`).
- Supabase: 14 migraciones nuevas de upstream sin ejecutar contra una base real (ver PR-BODY).

## Fuentes tipográficas no empaquetadas

Space Grotesk y Outfit (y JetBrains Mono en Android) no están incluidas; no se descargó nada. Android usa la familia de sistema documentada en `designsystem/theme/Fonts.kt` (TODO `res/font`); desktop tiene `fonts.lock.json` con hashes `null` y cae a la fuente del sistema. Hace falta OK del usuario para descargar (OFL).

## Android

- AGP 8.13.2 con **Material3 Adaptive 1.3.0** (necesario para `ListDetailSceneStrategy` de Navigation 3): sus AAR declaran `minAgpVersion=9.1.0` y `compileSdk 37`. Se subió `compileSdk` a 37 (`targetSdk` sigue en 36), `android.suppressUnsupportedCompileSdk=37.0` y se desactivaron las tareas `check*AarMetadata` en todos los módulos (`build.gradle.kts` raíz). Alternativa limpia: migrar a AGP 9.1 (cambia el plugin de Kotlin integrado).
  - Consecuencia: con esas tareas desactivadas, `bundleReleaseLocalLintAar` (BundleAar de cada librería, requerido por lintVital en `assembleRelease`) falla con `aarMetadataCheck specifies directory ... which doesn't exist`. Se resolvió con `lint { checkReleaseBuilds = false }` en `app/build.gradle.kts` (no corre lintVital en release; `./gradlew lint` sigue disponible a mano). Se prefirió a mantener las tareas habilitadas porque `AarMetadataCheck` no tiene modo no-fatal. Se elimina al migrar a AGP 9.1.
- Kotlin 2.4.0 falla en el backend (JVM IR) con `x?.let(::funcionLocal)` dentro de lambdas composables; se reescribió con lambda explícita. Evitar referencias a funciones locales en `TatakaiApp.kt`.
- `signOut` empuja el historial pendiente (mejor esfuerzo) y luego limpia el historial local: lo no empujado se pierde.
- Room v1 con migración destructiva, esquema no exportado. Sesión de Supabase en SharedPreferences por defecto (privada de la app, sin cifrar).
- Sin `MediaSessionService`: no hay reproducción en segundo plano en M0. "Iniciar sesión" desde la pantalla Link navega con un `MediaRef` vacío (sin contexto de título).
- Sin tests para los módulos data/database/supabase.
- Warnings de compilación: overload obsoleto de `ButtonGroup` (designsystem), expresiones sin uso en `LinkClient.kt`.

## Desktop

- `Dioxus.toml`: globs de recursos pendientes. FSR.glsl sin pin; los hashes de Anime4K son los de Temple (pasar `-Anime4kSource` si hay desajuste).
- `tk-link` no registra plataforma ni "visto por última vez" por dispositivo: la UI muestra plataforma vacía.
- El servidor Link arranca al abrir la pantalla Link por primera vez (evita el prompt del firewall al inicio); el desinstalador deja la regla de firewall.
- Sesión Supabase solo en memoria (no persiste entre arranques). ACL de Windows del directorio de claves no portado (se usa DPAPI).
- No implementados en desktop: `FloatingToolbar`, `AdaptiveSheet`.
- `LinkHost` no expone biblioteca local en M0.

## Linux (ola 2, empaquetado)

- `packaging/linux/` (PKGBUILD, `build-appimage.sh`, `AppRun`) y `.github/workflows/desktop-native-linux.yml` se escribieron sin ejecutar `makepkg`, linuxdeploy ni el workflow; solo se validaron con `bash -n`, `makepkg --printsrcinfo`, `desktop-file-validate` y `appstreamcli validate --no-net`, y `install-tree.sh` / `fetch-deps.sh` se probaron en un directorio temporal.
- AppImage: WebKitGTK busca sus procesos auxiliares (`WebKitWebProcess`, `WebKitNetworkProcess`, el *injected bundle*) en una ruta absoluta compilada. `build-appimage.sh` copia ese directorio al AppDir y parchea en `libwebkit2gtk-4.1.so.0` `/usr` → `././` (misma longitud), y `AppRun` arranca la app con `cwd = $APPDIR/usr`; el directorio del usuario queda en `$OWD`. Si una versión futura de WebKitGTK cambia cómo resuelve esa ruta, el AppImage abre sin contenido.
- AppImage construido en Ubuntu 24.04: necesita glibc >= 2.39 en el sistema destino. libGL/EGL, fontconfig y glibc vienen siempre del sistema (excludelist de linuxdeploy).
- El hook del plugin GTK de linuxdeploy fuerza `GDK_BACKEND=x11`; `AppRun` lo anula y respeta el valor del usuario (sin valor, GTK elige Wayland). Sin probar con la GTK empaquetada sobre Plasma Wayland.
- Video incrustado: necesita el renderizador DMA-BUF de WebKitGTK. Con `WEBKIT_DISABLE_DMABUF_RENDERER=1` o `WEBKIT_DISABLE_COMPOSITING_MODE=1` mpv pasa a su propia ventana (con aviso). Alternativas de arquitectura (GTK 4 / subsuperficies) en `10-linux-video-embed-debt.md`.
- En el AppImage, un mpv externo lanzado por la app hereda `GIO_MODULE_DIR`, `GTK_*`, `GDK_PIXBUF_MODULE_FILE` y `XDG_DATA_DIRS` del hook; no se esperan efectos en mpv, pero el lanzador podría limpiarlos cuando `APPIMAGE` está definido.
- Wayland: sin `app_id` explícito GTK usa el nombre del programa (`tatakai-next`), que no coincide con `app.tatakai.own.next.desktop`; el `.desktop` lleva `StartupWMClass=tatakai-next` para que Plasma asocie icono y ventana. Lo limpio es fijar el `app_id` (`EventLoopBuilderExtUnix::with_app_id("app.tatakai.own.next")`) en `tk-app/src/main.rs`.
- FSR.glsl y las fuentes Space Grotesk / Outfit siguen sin hash en sus locks: `fetch-deps.sh` las omite (nunca instala sin verificar) hasta que se fijen con `--pin`.
- El PKGBUILD compila desde el checkout local (no descarga fuentes) y `check()` repite los tests del workspace en perfil release (`makepkg --nocheck` para saltarlos).

## Torrents (ola 3)

Contrato: `10-wave3-torrents-contract.md`. Los seis workers escribieron sin compilar; lo que sigue es lo conocido al cerrar la ola, a revisar tras el build del integrador y la primera prueba real.

- **Sin probar contra la red real**: DHT, trackers, UPnP y el streaming con prioridad de piezas solo tienen (como mucho) tests en loopback con DHT apagado. Sin medir: tiempo hasta el primer frame, comportamiento del seek a zonas sin descargar, uso de CPU/disco de las descargas en segundo plano.
- **PEX no se puede apagar**: `librqbit::SessionOptions` 9.1.0 no tiene opción para PEX. El ajuste existe por contrato, pero `ProtocolStatus::pex` refleja lo que hace el motor, no el ajuste.
- **Trackers**: el estado y las cifras salen de la sonda propia de announce / scrape de la app (`tk-torrent/src/tracker_probe.rs`, se vuelve a preguntar cada 15 min o con "Reanunciar"), no de los announces internos de librqbit 9.1.0 (que no los expone); pueden diferir de lo que ve el motor. La lista incluye los 5 trackers públicos que `live/torrent.rs` añade a cada torrent, más los propios del usuario (ola A).
- **Listener**: `tk-torrent` usa `ListenerMode::TcpAndUtp` (uTP está marcado como no estable aguas arriba en librqbit 9.1.0): vigilar regresiones de uTP; si dan problemas, volver a solo TCP en `engine.rs`.
- **Cuenta de peers**: `seeders` sale de los peers vivos de librqbit (`live_seeders`) y `leechers` = conectados − seeders, no del *scrape* de los trackers: puede diferir de lo que muestra el sitio de la fuente.
- **Peers**: librqbit 9.1.0 no expone el bitfield de cada peer y no hay GeoIP, así que `progress` es siempre 0 y `country` vacío; solo salen los flags D, U, I y P (E, X, H y S nunca).
- **Windows**: el puerto de escucha entrante puede disparar el aviso del firewall la primera vez (el contrato no prevé una regla de firewall para torrents, a diferencia de Link). UPnP en redes con CGNAT no abre nada: `nat` / `forwarding` lo indican.
- **TLS de trackers / descargas `.torrent` por HTTPS**: `native-tls` (OpenSSL en Linux, SChannel en Windows), distinto del rustls/ring del resto de la app; un fallo TLS en un tracker concreto no se reintenta con otro backend.
- **Fallback de fuentes en Auto**: las torrent ya no se saltan; una fuente con 0 seeders puede consumir hasta 90 s (timeout de metadata) antes de pasar a la siguiente.
- **Política "después de ver"** (`Delete` por defecto): borrar al salir del player elimina también los archivos de un torrent no conservado. Si la app se cierra de golpe durante la reproducción, el torrent queda en la sesión y se limpia en el siguiente arranque: el motor borra (con sus archivos) los torrents restaurados cuyo registro en `labels.json` tiene `persist: false`. Al salir del player la política espera 20 s antes de borrar o pausar (siguiente episodio de un batch).
- **Archivo del episodio en batches**: `pick_episode_file` es heurístico (tipo anitomy). Numeración absoluta frente a por temporada, especiales y OVAs pueden elegir mal; si no encaja nada, el player lleva a la página Files del cliente, que por ahora es solo lectura (no hay método del backend para elegir o reproducir un archivo concreto desde ahí).
- **Cierre de la app**: no hay apagado explícito (el `launch` de Dioxus no vuelve); el motor se para en el `Drop` del servicio, que puede no correr al salir el proceso. El fast-resume puede quedar sin escribir y el siguiente arranque vuelve a verificar los datos (no se pierden).
- **URLs de streaming**: el token cambia en cada arranque del motor; si un cambio de ajustes reinicia el motor, las URLs abiertas mueren. Por eso `apply_settings` no reinicia mientras hay un stream abierto (el cambio se aplica en el siguiente arranque).
- **Timeout de red de mpv**: para URLs loopback (`http://127.0.0.1`) se usa `network-timeout` = 120 s en los tres modos (en mpv externo solo para esas URLs); un atasco más largo hace que el player pase a la siguiente fuente.
- **Descargar para después**: el botón de descarga de las filas torrent está en el selector de fuentes de la ficha, no en el del player. Downloads distingue las descargas en segundo plano solo por `persist && progress < 1 && !streaming`: un torrent conservado que se vio a medias también aparece ahí.
- **Respuesta tardía de `torrent_play`**: si el usuario cambia de fuente mientras se espera la metadata, ese torrent no recibe `torrent_playback_ended`; queda en la sesión hasta el siguiente arranque (si no está conservado) o en la Library.
- **Marcas intro/outro**: AniSkip solo con MAL id y solo para episodios con datos enviados por la comunidad; sin datos no hay marcas ni botón "Saltar intro".
- **Android**: solo diseño (`11-android-torrents-design.md`); nada implementado. Riesgos ya identificados ahí: subtítulos ASS simplificados en Media3, límite de 6 h/24 h de los servicios `dataSync` en Android 15, SAF sin rutas de archivo para el motor.

## Android (ola `android/revamp`, contrato `12-android-wave-contract.md`)

Lo que quedará sin probar al cerrar la ola aunque compile (los workers escriben sin compilar; un solo build al final):

- **Nada se ejecuta en un dispositivo real** dentro de la ola: ni la WebView oculta del runtime de navegador (bypass de Cloudflare de Toko / Latino), ni libmpv (`dev.jdtech.mpv:libmpv` 1.0.0: superficie, ASS, hwdec), ni la app externa (extras de cabeceras de mpv-android / VLC), ni CameraX + zxing del escaneo de QR.
- **Torrents (libtorrent4j 2.1.0-39)**: sin probar contra la red real (DHT, trackers, UPnP apagado por defecto), sin medir tiempo hasta el primer frame ni seek a zonas sin descargar; el servidor loopback solo tiene, como mucho, tests JVM con un proveedor de bytes falso. Los `.so` vienen dentro de jars (`lib/<abi>/libtorrent4j.so`): confirmar en el APK que AGP los empaqueta para arm64 / arm / x86_64. libtorrent4j depende de un mantenedor principal.
- **Servicio en primer plano**: tipos `mediaPlayback` / `dataSync`, `onTimeout` de Android 15 (límite ~6 h/24 h de `dataSync`), Doze, `WifiLock`/`WakeLock` y la notificación del canal `torrents` solo se comprueban en dispositivo; las *user-initiated data transfer jobs* no se evaluaron.
- **Red medida / batería / almacenamiento**: callbacks de `ConnectivityManager`, `ACTION_BATTERY_LOW/OKAY`, `getAllocatableBytes` y el movimiento a una carpeta SAF al completar sin probar; desinstalar la app borra las descargas del almacenamiento específico.
- **Media3 con torrents**: lectura progresiva de MKV por loopback con timeouts largos sin medir; los subtítulos ASS/SSA se ven simplificados (aviso en el panel de pistas); HEVC/AV1 10-bit depende del decodificador del dispositivo.
- **Presets de shaders** (Anime4K / FSR): el ajuste se guarda pero no se empaquetan shaders en esta ola; en Media3 no tienen efecto.
- **Fixtures de release**: `contracts/fixtures/release/{names,packs}.json` se generan desde los tests de `release.rs`; `cargo test -p tk-core` todavía no las lee (solo Kotlin), así que la equivalencia Rust ↔ Kotlin se garantiza por construcción, no por un test cruzado.
- **Room**: la tabla nueva de estadísticas de fuentes sube la versión con migración destructiva: se pierde el historial local no sincronizado al actualizar.
- **Idioma por app** en Android < 13 (contexto con locale en `attachBaseContext`) y `reduce motion` del sistema (ANIMATOR_DURATION_SCALE) sin probar en dispositivo.
- Plataforma `android-37` se descarga en el primer build del equipo Linux (solo había `android-36`).
- El SDK instala la API 37 como `platforms;android-37.0` (hash `android-37.0`): las convenciones fijan `compileSdkMinor = 0` además de `compileSdk = 37`; sin él AGP 8.13.2 busca `android-37` y falla.

Integración de la ola (rama `android/revamp`, merges a → e → f → d → c → b sin conflictos; aún sin compilar):

- **Room v2**: tablas nuevas `source_stats` y `picked_source`; se mantiene `fallbackToDestructiveMigration` sin migración v1 → v2, así que al actualizar se borra el historial local no sincronizado (y las estadísticas / fuentes elegidas).
- **Nada compilado ni ejecutado**: ningún test JVM nuevo de la ola se ha corrido. Puntos de riesgo que señalaron los workers: casos `(derived)` de `contracts/fixtures/release/names.json` (si falla `ReleaseFixturesTest`, revisarlos antes que el port), getters swig de libtorrent4j como propiedades Kotlin, interop de `MPVLib`, argumentos con nombre de `Font(resId, weight, style, variationSettings)` (`@ExperimentalTextApi`), la tarea `copyThirdPartyNotices` de `app/build.gradle.kts` con AGP 8.13.2 / Gradle 9.3.1 (si falla, quitar ese bloque solo vacía la parte de avisos de la hoja de Licencias), `FlowRow` y nombres de iconos.
- **Loopback en claro**: `app/src/main/res/xml/network_security_config.xml` permite HTTP en claro solo a `127.0.0.1` / `localhost` (servidor de streaming de torrents); sin probar en dispositivo con OkHttp + Media3.
- **Notificaciones de torrents**: tocar una notificación abre `Route.Torrents(section, torrentId)` (extras `app.tatakai.next.torrent.SECTION` / `TORRENT_ID`, leídos en `MainActivity` vía `addOnNewIntentListener`); la acción "Ver" no arranca el player sobre el archivo local, solo abre el cliente.
- **Extensiones**: la resolución de Cloudflare con la WebView oculta no se adjunta a una ventana (medida 1080×1920); algunos retos pueden necesitar una vista visible. Las cookies del `CookieManager` son de toda la app (sin `androidx.webkit` no hay perfiles por WebView): aislamiento por navegador de mejor esfuerzo. Los logs por extensión viven solo en memoria (200 líneas). Los tiempos de `quickjs-kt` (`evaluationTimeoutMillis` 90 s con navegador, watchdog de 15 s sin él) no se pudieron comprobar contra fuentes.
- **Reproductor**: motor externo con extras de cabeceras de mejor esfuerzo (mpv-android `headers`, VLC `http-headers`); libmpv sin bundle de CA (`tls-verify` queda en el valor por defecto de mpv), libass sin fuente de la app; sin "Precargar el siguiente", pantalla completa ni captura; selección de calidad solo en streams adaptativos de Media3; sin test JVM de `PlayerViewModel`.
- **Pantallas**: Relacionados de la ficha son títulos de Trending/Popular con un género en común y al tocarlos se abre Buscar por ese género (no hay endpoint de relaciones ni callback para abrir otra ficha); sin píldora Recap ni "Añadir a lista"; la búsqueda dentro de extensiones sigue M1 (chips deshabilitados); los filtros de Buscar se aplican en el cliente a las páginas cargadas; la fila de extensión en Inicio muestra el id de la extensión, no su nombre.
- **Torrents (cliente)**: sin GeoIP (país del peer siempre vacío); PEX solo se aplica a torrents añadidos después del cambio; exportar a Descargas requiere Android 10+; tras mover a SAF el torrent sale de la sesión (detalle con mensaje "movido"); "Compartir archivo" sustituido por compartir magnet + Guardar en Descargas (sin `FileProvider`); sin fusión de la notificación de Media3 con la del servicio; sin foco D-pad / TV.
- **Ajustes / Tú**: "Abrir logs" solo muestra el snackbar M1; sin contador de cambios pendientes en Cuenta; los botones de control remoto y "Continuar en el PC" de Dispositivo enlazado son marcadores M1; la tarjeta de capacidades no muestra GPU / codificador / upscaler (`LinkCapabilities` no los trae).
- **Release / ProGuard**: no hay build de release con `minifyEnabled`; cuando se active hace falta `-keep class org.libtorrent4j.swig.libtorrent_jni {*;}` (y revisar JNI de libmpv / QuickJS).
- **contracts**: `node --test contracts/test` falla solo en `synthetic-fs.kai is stale`, previo a la ola: el contenido del zip es idéntico a las fuentes, solo cambian los bytes de deflate entre versiones de zlib de Node (v26 en este equipo).

Verificación en dispositivo (moto g51 5G, Android 12 / API 31, arm64, 2026-11-04):

- **Funciona en el teléfono**: arranque sin crash, Inicio con carrusel y filas, Buscar, ficha con color de la portada, Biblioteca (Continuar / Historial / Torrents), Tú, Ajustes (cambio de idioma en caliente en API 31), Dispositivo enlazado, instalación de `.kai` por SAF, WebView oculta abierta por Toko, panel y hoja de fuentes con filtros, Media3 y libmpv reproduciendo HLS (reanudación incluida), conmutación automática a la siguiente fuente, torrent por loopback reproducido con libmpv, descarga en segundo plano con servicio en primer plano y notificación, detalle del torrent (trackers, archivos), borrado con archivos.
- **Toko 4.5.5 y Latino 1.0.1 no son compatibles tal como se publican** (también en desktop: mismo análisis): `cheerio` 1.x importa `undici` (de ahí `net`, `tls`, `fs`, `sqlite`, `worker_threads`) y `browser-runtime-bootstrap.ts` importa `child_process` / `fs` y llama a `os.homedir()` al cargar. Con `undici`, `child_process`, `fs` y `os` sustituidos por stubs en el build de la extensión pasan a "Parcial" y funcionan. Es un cambio a hacer en el repo de cada extensión (alias de esbuild), no en el host.
- **Límite de memoria de QuickJS**: con 64 MiB (el límite anterior del contrato `globals.v1.md`) la ruta `stream` de Toko termina en "Extension exceeded its memory limit"; con 256 MiB lista 13 fuentes. El contrato, desktop (`tk-ext`) y Android pasan a 256 MiB.
- **Resolución lenta**: el panel espera a la ruta más lenta (la ruta `torrent` de Toko agota su presupuesto contra dominios caídos: `feed.animetosho.org`, `toon.snvhost.com`, `anizone.pw` ya no resuelven), así que tarda 45–90 s. El reproductor reutiliza durante 3 min los candidatos que resolvió la ficha.
- **Torrent de streaming**: con la política por defecto "borrar después de ver" desaparece de Biblioteca › Torrents al salir del reproductor (esperado). El primer arranque de la sesión puede quedarse unos segundos con 0 peers hasta que el DHT arranca.
- Corregido tras la prueba: cierre de la app al volver de la ficha al reproductor (NPE de `HorizontalFloatingToolbar` de material3 1.5.0-alpha15 en el pase lookahead; ahora es un Surface + Row propio), libmpv sin vídeo cuando `loadfile` llega antes que la superficie, encabezados de Biblioteca y Buscar bajo la barra de estado, fecha de emisión ISO sin formato, contraste del botón Ajustes en el banner de la hoja de fuentes, badge de Torrents tapando la etiqueta, "Transcurrido" en milisegundos, primera descarga tras "Entiendo" rechazada por `background_off`, receptor de batería sin recibir `BATTERY_CHANGED` en API < 33.

## Tokens

- Corregido: el esquema `Expressive` rota el matiz del primario (violeta -> verde azulado, sakura -> azul, aurora -> naranja). Las tres paletas pasaron a `Vibrant`; un test exige que el matiz del primario esté a <= 20 grados de la semilla en todas las paletas no monocromas.

## Android nativo: cliente del servidor de extensiones integrado (`13-integrated-server-contract.md` §3.10, w6)

- **Cleartext global + guard de la app**: `app/src/main/res/xml/network_security_config.xml` pasa a `<base-config cleartextTrafficPermitted="true">` (solo CAs del sistema). Hace falta porque el servidor de la PC es HTTP plano en la LAN (`http://192.168.x.x:<puerto>`, Tailscale `100.64/10`, `*.local`) y el `network_security_config` no admite rangos de IP. La política real es `app.tatakai.next.extensionserver.HostGuard` (misma regla que `hostGuard.ts`, §3.8), aplicada al conectar, al emparejar y en cada petición, incluidas las URLs de medios devueltas (deben ser del mismo origen que el servidor guardado). Efecto colateral: el resto de la app (metadatos, extensiones `.kai`) ya no tiene el bloqueo de cleartext de la plataforma; las extensiones siguen limitadas por su `NetworkPolicy` (dominios del manifiesto) y las APIs de metadatos usan `https://` fijas.
- **Tokens**: las conexiones (url, serverId, nombre, token, activado) se guardan en `noBackupFilesDir/extension-servers.bin`, cifradas con AES-256-GCM bajo una clave no exportable de AndroidKeyStore (`tatakai_extension_server_token_seal`, distinta de la del Link). Si la clave se pierde (restauración, borrado de credenciales) el archivo se lee como "sin servidores" y hay que volver a emparejar. Los tokens no se registran en ningún log.
- **Deep link**: `tatakai://extensionserver/connect?server=…&code=…` (intent filter VIEW + BROWSABLE en `MainActivity`) abre la pantalla "Servidor de extensiones de la PC", que **pide confirmación** antes de emparejar; nunca se empareja solo por abrir un enlace. No se acepta ningún enlace web (guard de `contracts/test`).
- **Fusión de fuentes**: `ExtensionRepositoryImpl.resolveCandidates` consulta en paralelo las extensiones instaladas y `ExternalCandidateSource` (los servidores activos); `extId = server:<serverId>/<fila>` (no se repite el `serverId` que ya trae el `providerKey`). Presupuesto por servidor 30 s: al agotarse se usa la última instantánea NDJSON recibida; un servidor apagado falla por el timeout de conexión (5 s) y no aporta nada. Resultados completos y no vacíos se reutilizan 3 min (Details → reproductor). Solo anime/`watch`: `invoke` existe en el cliente pero no hay todavía catálogo / manga / novelas del servidor en la app nativa.
- **Medios**: las fuentes web solo se aceptan si su URL es del origen del servidor (proxy `/api/server/v1/media/*`, que no necesita token); `embed` / `custom` y fuentes DASH sin `.mpd` en la URL se descartan (el proxy no conserva la extensión). Los torrents (magnet / info-hash) del servidor se reproducen con el motor de torrents del teléfono.
- **Sin probar**: nada compilado ni ejecutado (ni `testDebugUnitTest` de `:core:extension-server` ni `assembleDebug`); sin prueba en dispositivo contra el servidor real de Electron. El test de presupuesto usa `throttleBody` de MockWebServer y mide tiempos reales (< 4 s con presupuesto de 1 s).

## App clásica / Electron: cliente y host del servidor de extensiones integrado (`13-integrated-server-contract.md`, w4 / w5 / integrador)

- **APK clásico, cleartext global**: `android/app/src/main/res/xml/network_security_config.xml` pasa a `<base-config cleartextTrafficPermitted="true">` (antes solo `127.0.0.1` / `localhost`) para que `CapacitorHttp` y el proxy de medios loopback lleguen a `http://192.168.x.x:<puerto>`. La política del servidor de extensiones es `src/core/extensions/server/hostGuard.ts`; el resto de peticiones nativas (bundles de extensiones, otras fuentes de medios) pierde el bloqueo de cleartext de la plataforma. El WebView sigue bloqueando contenido mixto.
- **APK y Electron sin resultados progresivos**: en Capacitor (`CapacitorHttp`) y en Electron contra un servidor `http://` (puente `extension:fetch-resource` del proceso main) `/resolve` se pide como JSON agregado; las fuentes del servidor aparecen todas juntas. El puente de Electron tiene un timeout propio de 15 s (axios), por lo que una resolución agregada lenta puede fallar por ese camino.
- **Imágenes de manga desde un servidor LAN en el APK**: son URLs `http://`; un `<img>` en el WebView `https://tatakai.me` las bloquea como contenido mixto si el lector no las pasa por el proxy móvil. Sin verificar en dispositivo.
- **Deep link en Electron**: `desktop/window/deep-link.cjs` conserva `%` solo para las rutas `extensionserver/…`, para que el `server=` codificado de `tatakai://extensionserver/connect?…` llegue intacto.
- **Rutas del Home Server sin chequeo de Host/Origin propio**: `/server/*` y `/api/server/*` saltan los chequeos de `server.cjs` (como antes); la seguridad depende de `allowedOrigins` de `core.mjs` y de `requireDeviceToken` (sin acceso anónimo en escritorio).
- **Fuentes `.kai` remotas sin `sourceType`**: Tatakai Next descarta las fuentes del servidor sin tipo (la URL proxificada no tiene extensión). Aniyomi, Mihon/CloudStream y las extensiones interpretadas lo envían; algunas `.kai` instaladas (`invokeExtension` → `single`) podrían no hacerlo.

## Ola A de paridad (`15-parity-wave-a-contract.md` y `15-parity-wave-a-contract-android.md`)

Diez workers (desktop a, c, d, e, f, g, i; Android o, p, q) escribieron sin compilar; el integrador tampoco compiló (solo `rustfmt --check`, `cargo metadata` y `node --test contracts/test`). El primer `cargo build` / `gradlew` de la ola es del fixer.

- **Desktop, sin compilar**: el código nuevo dentro de `rsx!` solo se revisó a mano (`rustfmt` no entra en las macros). Usos nuevos a vigilar en el primer build: `use_reactive((&a,), ...)` dentro de `use_resource` y `dioxus::core::Task` (g, `settings/accounts.rs`), `catch_unwind` alrededor del icono de bandeja en Linux sin AppIndicator (f, `tray.rs`).
- **Debrid (desktop)**: las comprobaciones de caché de Real-Debrid / AllDebrid añaden el magnet a la cuenta (las sondas que resultan no cacheadas se borran después). `mark_cached` no recibe el id / episodio: los toma de la entrada más reciente de `LiveBackend::candidates` cuyas fuentes coinciden (propuesta para la ola B: pasar `(id, episode)` desde `LiveBackend::sources`). El orden de los servicios es fijo (`DebridServiceVm::ALL`, "Prioridad N"): el `Backend` no tiene método para guardar otro orden.
- **Debrid (Android)**: Real-Debrid y AllDebrid no tienen endpoint de caché, así que su disponibilidad se aprende al reproducir (sonda de unos 6 sondeos; un torrent de RD no cacheado se borra y se usa libtorrent); la insignia de la fuente solo refleja TorBox / Premiumize.
- **Precarga del siguiente episodio**: en desktop un torrent de un solo archivo pide `torrent_preload_next` al 80 % de la reproducción (un batch elige su siguiente archivo solo); en Android se dispara cuando el archivo en streaming está descargado al 80 % (lado del motor), no al 80 % de la posición.
- **Horario de ancho de banda**: en Unix la hora local sale de `date +%H` (Windows: `GetLocalTime`).
- **Manejador `magnet:` (desktop)**: la segunda instancia entrega el enlace por el archivo `<data>/magnet.pending` también en Windows (no `WM_COPYDATA`: el procedimiento de ventana es de Dioxus / tao). En Linux / macOS solo un arranque con magnet cede a la instancia que ya corre; un segundo arranque normal sigue arrancando, como antes. `data-manual` en la biblioteca de torrents solo persiste para torrents sin título; los añadidos a mano con título se marcan solo durante esa ejecución.
- **"Enviar al dispositivo"** desde la ficha (desktop) sigue en `UiError::not_wired("send_to_device")`: no es de la ola A.
- **Ficha desktop**: los ids `ext:<extensión>:<fuente>:<item>` (resultados de buscar dentro de una extensión) muestran extras vacíos; falta pedir `custom/info` al host. La vista de episodios y el "mostrar siempre" de 18+ se guardan en `localStorage` del web view (`tk.details.episodeView`, `tk.details.adultOk`), no en `ui-settings.json`.
- **Idioma de títulos**: desktop lo aplica en todas las tarjetas y en la ficha (`Settings::title_language`, Ajustes › Apariencia). Android lo aplica en Buscar, la ficha y sus filas, pero no en `MediaCard` / `EpisodeRow` de `:core:ui` (Inicio, Biblioteca, apariciones de personajes): falta subir `Media.displayTitle(TitleLanguage)` a `:core:ui`.
- **Android, navegación**: `DetailsScreen` no tiene callback para abrir otro título, así que relaciones, temporadas y recomendaciones abren fichas anidadas en una pila interna (`rememberSaveable` + `BackHandler`); los personajes desde Buscar se abren en el sitio. Cambiarlo pide una enmienda de firmas (`onOpenMedia`, `onOpenCharacter`).
- **Android, buscar en extensión**: una extensión se considera buscable si `InstalledExtension.type == "custom"` (el modelo no expone `customSources`); una `onlinestream` con fuentes custom sale como chip desactivado aunque `searchExtension` funcionaría.
- **Android, reproductor**: el estilo avanzado de subtítulos (colores, posición, fuente importada) vive en `SharedPreferences` `tatakai_player` (privado de p), no en `AppSettings`; `AppReset.resetAll` lo borra con el resto de `shared_prefs`, `filesDir/fonts` y `filesDir/shaders`. El retraso negativo de subtítulos en Media3 solo funciona con pistas externas o cargadas. Con `backgroundPlayback` apagado no hay `MediaSession` ni notificación.
- **Room v3** (listas, Android): migración destructiva como antes: al actualizar se pierde el historial local no sincronizado y las listas locales.
- **MAL sin `REPEATING` nativo**: se escribe como `watching` + `is_rewatching=true` y se lee como `REPEATING`.
- **Fixture empaquetada**: `node --test contracts/test` sigue fallando en `synthetic-fs.kai is stale` (CRLF; `contracts/fixtures/extensions/build.mjs` la regenera). No es de esta ola.

## Ola B de paridad (`16-parity-wave-b-contract.md` y `16-parity-wave-b-contract-android.md`, integración)

Nada de esta ola se compiló ni se ejecutó al integrar (F2); el primer build es F7.

- **Desktop, descargas de capítulos**: manga (l) y novela (m) escriben sus filas de `downloads` (`url` = `manga:<ext>/<capítulo>` o `novel:<ext>/<capítulo>`) y las descargan en su propia tarea, sin `downloads::enqueue_jobs` de h. El motor de h las ignora (`tk_core::download::engine::is_reader_row`) y, si la app se cerró a mitad, las marca `failed` (`interrupted`) en vez de reencolarlas. Reintentar / reanudar esas filas desde la página Descargas no las vuelve a ejecutar.
- **Desktop, proveedores de metadata**: el interruptor por proveedor vive en `HomePrefs::disabled_providers` (b), no en un campo propio de `Settings`; el router recibe la prioridad sin los apagados y `MetadataRouter::ordered` ya no pregunta a los que faltan en ella. MAL solo responde con `TATAKAI_MAL_CLIENT_ID`.
- **Desktop, proxy saliente** (`server.proxy`): las peticiones de las extensiones (`tk_core::net::SafeHttp`, `no_proxy()`) no usan el proxy; solo el proxy de medios del servidor y los streams del reproductor local.
- **Desktop, extensiones mixtas**: una extensión con fuentes `watch` y `read` manda sus tarjetas de lectura a la ficha normal; solo las de un único tipo abren la ficha de manga / novela.
- **Desktop, manga de servidores emparejados**: los capítulos de los servidores de extensiones de la PC (`~read` del cliente de j) no se listan en el lector (no hay API de cliente para l).
- **Ids compartidos**: `DownloadState` de Android tiene `removing` (solo Android) y no tiene `cancelled` (solo desktop); `DownloadProfile` usa ya los ids de desktop (`best` / `balanced` / `data_saver`). El id de un add-on de Stremio sigue el formato `stremio:<id>` en ambos, pero el `<id>` es aleatorio en desktop (`extserver::new_id`) y el `id` del manifiesto en Android: son locales de cada dispositivo.
- **Android, contenido maduro**: el modo SHOW / BLUR / WARN se aplica en Inicio; la lista de resultados de Búsqueda (`feature/search`) todavía no (haría falta mover `MatureContent.kt` de `feature/home` a `core/ui`).
- **Android, descargas web**: HLS se exporta como un solo TS concatenado (o fMP4 con el segmento init delante); solo se baja la media playlist de la variante elegida (sin pistas de audio alternativas separadas); se usa `PlatformScheduler` (JobScheduler) porque `media3-exoplayer-workmanager` no está en la caché de Gradle; Android 15 limita los servicios `dataSync` a unas 6 h cada 24 h; las auto-descargas que encola el worker caen a `manager.addDownload` sin notificación en primer plano cuando Android no deja arrancar el servicio desde segundo plano.
- **Android, Link**: el long-poll de `GET /v1/events` solo corre con la app en primer plano (sin servicio en segundo plano). La PC no expone su plataforma al teléfono (`link.last-seen` muestra la última vez vista real, no la plataforma del PC).
- **Android, narración**: `TextToSpeech` no pausa a mitad de frase (Reanudar repite el párrafo actual); KittenTTS es solo escritorio. Lora y OpenDyslexic no van empaquetadas (caen a serif del sistema o sans con más espaciado; se puede importar un TTF/OTF). MangaDex solo es fuente de capítulos cuando no hay ninguna extensión `read`.
- **Android, social**: la cuota de AniList (90 req/min) se comparte con el sondeo de notificaciones (worker cada 15 min, solo con la opción activa y AniList enlazado). "Escribir reseña" abre `https://anilist.co/review/editor/<anilistId>` (sin comprobar contra AniList).

## Ola B de paridad

Re-revisión F8 (2026-11-09, rama `parity/wave-b`). Página: `docs/redesign/parity/audit-wave-b.html`; capturas en `docs/redesign/parity/wave-b/next/`. Humo de escritorio con `scripts/parity/wk.mjs` sobre el binario debug, con un directorio de datos vacío, sin cuentas, sin extensiones y sin dispositivos emparejados.

### Estado del build

- **Desktop**: verde (`cargo build -p tk-app`); la app arranca y las pantallas de abajo responden.
- **Android**: rojo. No se hizo humo de Android; todo lo de `16-parity-wave-b-contract-android.md` §5 queda sin ejecutar y sin verificar en dispositivo.
- **Clásico**: no se lanzó. `node_modules/electron` no trae el binario (`dist/electron` no existe) y instalarlo exige red, así que no hay capturas de `cdp.mjs` en esta pasada.

### Fallos vistos en el humo (desktop)

- **Extras de la ficha rotos** (`details.facts`, `details.related`, `details.characters-staff`, `details.recommendations`, `details.seasons-watch-order`, `details.next-episode`, `details.trailer`): todas las pestañas Información / Personajes / Relacionados y el orden de visionado muestran "`<id>` was not found on AniList" y no aparece ningún `[data-extra]`, `[data-next-airing]` ni `[data-trailer]`. Causa reproducida con curl: `id_vars` en `tk-core/src/metadata/anilist/details.rs` envía `{"id": 21, "idMal": null}` y AniList responde 404 "Not Found." (sin la clave `idMal` responde 200). La rama MAL, `{"id": null, "idMal": N}`, probablemente falla igual. Pendiente de arreglo: omitir la clave nula. El test `id_vars_prefer_the_anilist_id` fija el comportamiento erróneo.
- **El asistente no persiste `setup_done` al terminar**: tras `Empezar a ver` no se escribió `ui-settings.json`; cerrar y reabrir mostró el asistente otra vez. El archivo solo se creó al cambiar otro ajuste (entonces trae `setup_done: true` y el reinicio ya no lo muestra).
- **Buscar**: no existe `[data-filters]` (el contrato lo usa para comprobar el filtro activo); una búsqueda de personajes por "Luffy" devolvió primero resultados sin relación con el texto.
- **Arnés**: `wk.mjs FULL` dejaba estilos en línea en `#main`, `.tk-shell` y `.tk-shell__body`, y la pantalla siguiente salía vacía. Se corrigió para no sobrescribir los estilos guardados; Dioxus puede volver a pintarlos, así que tras un `FULL` hay que quitar los `style` con `overflow: visible`. `CLICK:Cerrar` coincide con el botón de cerrar la ventana de la app y la cierra.

### Ids sin ejercitar (solo "manual")

El checklist tras F4 no tiene ids en `stub` ni `missing`, pero estos pasos de aceptación no se pudieron ejecutar sin cuenta, extensión, dispositivo o red de instalación (y por la regla de no iniciar sesión, no emparejar y no instalar desde la red):

- **Reproductor y fuentes** (ola A d / e, ola B k): `player.*` (subtítulos preferidos, estilo, traducción, captura, atajos, auto-siguiente, temporizador, Discord), `sources.remembered-vs-prefs`, `sources.embed-sniffer*` en reproducción, `sources.debrid.*` con clave y `sources.debrid.episode-context`.
- **Descargas** (h): cola, pausa / reanudar tras reinicio, integridad, historial, límite de almacenamiento con uso real, auto-descarga, `downloads.local-library.*` (la hoja de descarga y las pestañas sí cargan; la hoja de la ficha dice "0 ya descargados").
- **Extensiones** (j): emparejar servidores, Stremio con manifest, actualizaciones, preferencias por extensión, proveedores personalizados (`#/provider/<id>`), `ext.deep-link.server`.
- **Servidor y Link** (k): `server.pairing`, `server.devices`, `server.viewer-password`, `/invoke`, `/resolve`, `/media`, `server.flaresolverr`, `server.proxy`, `link.send-to-device`, `link.remote-control`, `link.handoff`, `link.discovery`. Solo se comprobó `/health` (protocolVersion 1, pairingRequired true), `/sources` sin token (401) y que el cursor de logs crece.
- **Manga y novelas** (l / m): descarga de capítulos y lectura sin red, reanudar `data-page`; ficha, lector, descarga y TTS de novelas (necesitan una extensión de novelas); `novel.tts.*` con clave de OpenAI.
- **Inicio** (b): `home.because-you-watched`, `home.followed-updates`, `home.torrent-activity`, `home.recs.*` con historial (la página muestra el estado vacío) y `home.recs.atlas`, recordatorios de calendario (`data-reminder`).
- **Social** (n): `social.*` y `details.reviews` con cuenta de AniList (sin cuenta solo se ve "Conecta AniList").
- **Torrents** (i): añadir magnet, alineación de episodios, historial de sesiones, estado de trackers, `xdg-open magnet:`.
- **Setup y sistema** (f): `system.reset-app`, autostart, bandeja, Licencias, Buscar actualizaciones.

### Proveedores de metadata descartados

Ninguno se descartó en esta pasada. `data-provider-status` de los 8 proveedores: AniList, Kitsu, ani.zip, Shikimori, Tenrai, MangaDex y MangaUpdates en `ok`; MAL en `error` por no haber `TATAKAI_MAL_CLIENT_ID` (la ficha no puede mostrar `[data-facts-source~="mal"]` sin él). Shikimori y Tenrai se descartarían con nota si dejaran de responder (checklist `metadata.providers.*`); hoy responden. Además el humo no pudo comprobar su enriquecimiento de la ficha por el fallo de extras de arriba.

### Límites del sniffer, tk-server y KittenTTS

- **Sniffer**: el interruptor y el tiempo máximo (5..60 s, 20 por defecto) están en Ajustes › Servidor y red (`data-sniffer`, `data-sniffer-timeout`), no en "Fuentes"; una fuente embed solo llega al sniffer si ninguna HLS / MP4 / torrent válida la precede. No se probó con una extensión que devuelva `type: "embed"`.
- **tk-server**: solo sirve extensiones `.kai` (los ecosistemas Sora / Mangayomi / Seanime / Java se usan por el servidor Node emparejado). FlareSolverr y el proxy saliente solo cubren las peticiones del Upstream de tk-server y la reproducción por el proxy loopback; las peticiones de las extensiones (`SafeHttp`) se detallan en la nota de cierre de k en el checklist. En esta pasada solo se verificó arranque, `/health`, 401 sin token y parada.
- **KittenTTS** (solo escritorio): en esta máquina `data-kitten`, `data-kitten-runtime` y `data-kitten-phonemizer` salen `missing` (no hay `espeak-ng`); no se pulsó `Instalar` porque descarga runtime y modelo de la red. Sin extensión de novelas tampoco hay texto que narrar.

## Pruebas manuales después de la ola B (2026-11-09)

- **Video negro intermitente en el reproductor incrustado (Linux Wayland)**: en un perfil nuevo, la primera reproducción (Toko · anizone, Frieren EP1) avanzó con audio y tiempo pero el área del video quedó negra, sin controles visibles en la captura de escritorio. En un segundo arranque con el mismo perfil se vio bien dos veces seguidas. El log solo dice `embedded mpv GL surface ready` en ambos casos; no se pudo reproducir. Hipótesis a probar: la superficie GL se crea antes de que la ventana termine de moverse / redimensionarse, o la primera hoja / menú abierta sobre la ficha deja la composición de WebKit sin alfa. Revisar con `linux_embed_probe` y con un arranque en frío reproduciendo de inmediato.
- **Menú del reproductor mal anclado**: "Preajuste de escalado" se abre junto al botón de subtítulos, que queda resaltado.
- **Aviso de versión nueva falso**: `live::system` informa `a newer release is available latest=v7.0.0` en Next 0.1.0: compara contra los releases del Tatakai clásico. Debe mirar solo los tags de Next (o un canal propio).
- **Pruebas de reproducción**: el audio de la app se manda a un sink nulo propio (`PULSE_SINK` / `PIPEWIRE_NODE`) y `scripts/parity/mute.sh <pid>` silencia solo los streams de esa app; nunca por nombre "mpv".

