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
- **Trackers**: librqbit 9.1.0 no expone announce ni scrape, así que el estado sale siempre `TrackerStatus::Unknown` y las cifras (seeders / leechers / descargado) vacías aunque el tracker funcione. La lista incluye los 5 trackers públicos que `live/torrent.rs` añade a cada torrent.
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

## Tokens

- Corregido: el esquema `Expressive` rota el matiz del primario (violeta -> verde azulado, sakura -> azul, aurora -> naranja). Las tres paletas pasaron a `Vibrant`; un test exige que el matiz del primario esté a <= 20 grados de la semilla en todas las paletas no monocromas.
