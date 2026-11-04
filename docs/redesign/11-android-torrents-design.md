# 11 — Torrents en Android: diseño (sin código todavía)

Estado: **diseño aprobado para implementar después de la ola 3 de desktop** · Fecha: 2026-11-04 · Origen: decisión del usuario 4 de `10-wave3-torrents-contract.md` §0 ("Android: solo diseño en esta ola").

Este documento describe pantallas, flujos, modelo compartido y decisiones técnicas para llevar a `apps/android-native` lo que la ola 3 entrega en desktop (motor librqbit, cliente estilo Hayase, etiquetas de release, batches, marcas en la barra de progreso). No hay Kotlin en esta ola: los nombres de módulos, clases y claves son **propuestas** que la rama de implementación confirma con grep en `.deps-sources/` (regla E2 de `04-implementation-plan.md`).

Referencias: `10-wave3-torrents-contract.md` (§2 motor, §3 etiquetas, §5.2 view-models, §5.4 pantallas), `01-design-system.md` (tokens, componentes), `02-ux-flows.md` (navegación Android), `00-decision-stack.md` (stack y enmiendas).

## 1. Objetivos y no-objetivos

**Objetivos**
- Reproducir una fuente torrent desde el selector de fuentes igual que en desktop: añadir → metadata → elegir el archivo del episodio → streaming por HTTP loopback → Media3 (o mpv cuando exista en Android, M1).
- Cliente de torrents con las mismas secciones que desktop (Overview, Files, Peers, Trackers, Library, Downloads, Settings), adaptadas a pantalla táctil y tamaños compact / medium / expanded.
- Mismo significado de datos que desktop: los VMs de `10` §5.2 se reflejan uno a uno en Kotlin; `parse_release` y `pick_episode_file` dan el mismo resultado con las mismas entradas.
- Descargas en segundo plano solo detrás del aviso "Entiendo" y respetando red medida, batería y los límites de servicios en primer plano de Android 14/15.

**No-objetivos (por ahora)**
- Seeding prolongado o "cliente de torrents general" sin relación con anime. El cliente existe para ver y guardar episodios.
- Android TV: se diseña la navegación con D-pad (§3.6) pero se implementa con la ola de TV (M2).
- Sincronizar la biblioteca de torrents entre teléfono y PC por Link (M2, `sync de biblioteca/descargas vía Link`).

## 2. Modelo compartido con desktop

### 2.1 Regla

Igual que el resto del rediseño ("contratos compartidos, implementaciones nativas", `00`): **la semántica es una sola**, definida por `10-wave3-torrents-contract.md` §2 (tipos de `tk-torrent`) y §5.2 (VMs de `tk-ui`). Android no inventa campos nuevos; si necesita uno, se añade primero al contrato de desktop como ítem aditivo.

### 2.2 Tipos Kotlin propuestos (`:core:model`, paquete `app.tatakai.next.model.torrent`)

Todos `@Serializable`, camelCase, enums de estado en minúscula (`@SerialName("downloading")`), igual que el serde de desktop, para que un JSON de fixture valga para ambos.

| Kotlin | Equivale a (desktop) | Notas |
|---|---|---|
| `typealias TorrentId = String` | `tk_torrent::TorrentId` | info-hash v1 hex minúscula (40) |
| `enum class TorrentState { INITIALIZING, DOWNLOADING, SEEDING, PAUSED, ERROR }` | `TorrentState` / `TorrentStateVm` | clave de texto `torrent_state_<id>` |
| `data class ProtocolStatus(dht, lsd, pex, nat, forwarding, persisting, streaming: Boolean)` | `ProtocolStatus` / `TorrentProtocolVm` | |
| `data class TorrentStats(...)` | `TorrentStatsVm` | mismos campos: `totalBytes` = bytes seleccionados, `peers` = conectados |
| `data class TorrentFile(index, path, name, sizeBytes, downloadedBytes, progress, selected, streams, episode: Int?)` | `TorrentFileVm` | `episode` sale de `parseRelease(name)` |
| `data class TorrentPeer(addr, client?, progress, downBps, upBps, downloadedBytes, uploadedBytes, country?, flags)` | `TorrentPeerVm` | `flags` con las letras `D U I P E X H S` |
| `enum class TrackerStatus { WORKING, UPDATING, FAILED, UNKNOWN }` + `data class TorrentTracker(...)` | `TrackerStatusVm` / `TorrentTrackerVm` | |
| `data class TorrentLibraryItem(...)` | `TorrentLibraryItemVm` | `path` es una ruta de archivo o un `content://` (§6) |
| `data class TorrentLabel(mediaId, title, episode?, poster?)` | `TorrentLabel` | persistido por el motor |
| `enum class AfterWatch { KEEP, DELETE }` (por defecto `DELETE`) | `AfterWatchVm` | |
| `data class TorrentSettings(...)` | `TorrentSettingsVm` | + campos solo Android, §2.4 |
| `data class TorrentPlayback(playback: PlayerArgs + StreamSource, torrentId, fileIndex, fileName, batch)` | `TorrentPlaybackVm` | `StreamSource.container = MP4` o nuevo `PROGRESSIVE` (§5.3) |
| `enum class SkipKind { INTRO, OUTRO, RECAP }` + `data class SkipMark(kind, startSecs, endSecs)` | `SkipMarkVm` | AniSkip, igual que desktop |
| `data class ReleaseTags(...)`, `enum class ReleaseSource { BD, REMUX, WEB, TV, DVD }`, `enum class VideoCodec { H264, HEVC, AV1, VP9 }` | `tk_core::release` | etiquetas `label()` idénticas ("BD", "H.264"...) |

Repositorio (`:core:model/Repositories.kt`, mismo estilo que `ExtensionRepository`):

```kotlin
interface TorrentRepository {
    val events: SharedFlow<TorrentEvent>                       // Added, MetadataReady, StateChanged, FileCompleted, Completed, StreamOpened/Closed, Removed, Error
    suspend fun play(sourceId: String, media: MediaRef, episode: Int): Result<TorrentPlayback>
    suspend fun playbackEnded(torrentId: TorrentId, completed: Boolean)
    suspend fun addDownload(sourceId: String, media: MediaRef, episode: Int?): Result<TorrentId>
    fun stats(id: TorrentId): Flow<TorrentStats>              // tick de 1 s mientras haya colector
    suspend fun files(id: TorrentId): List<TorrentFile>
    fun peers(id: TorrentId): Flow<List<TorrentPeer>>
    suspend fun trackers(id: TorrentId): List<TorrentTracker>
    val library: StateFlow<List<TorrentLibraryItem>>           // más nuevo primero
    suspend fun remove(id: TorrentId, deleteFiles: Boolean); suspend fun pause(id: TorrentId); suspend fun resume(id: TorrentId)
    suspend fun setPersist(id: TorrentId, persist: Boolean)
    suspend fun seekMarks(id: TorrentId, fileIndex: Int): List<ClosedFloatingPointRange<Double>>
    suspend fun skipMarks(media: MediaRef, episode: Int): List<SkipMark>
}
```

Diferencias deliberadas con el trait `Backend` de desktop: Android expone `Flow` en lugar de que cada pantalla haga polling (el `Flow` hace el polling de 1 s por dentro y se detiene sin colectores, `stateIn(WhileSubscribed(5_000))`), y `openFolder` no existe (en Android se abre el archivo con un `Intent` de vista o se comparte; §6).

### 2.3 Algoritmos portables (mismos resultados en Kotlin y Rust)

- **`parseRelease(name): ReleaseTags`** — tipo anitomy, sin dependencias, nunca falla. Tokeniza por corchetes / paréntesis / separadores (`._ -`), reconoce grupo (primer `[...]`), resolución (`480p`..`2160p`, `4K`, `1920x1080`), fuente (`BD`, `BDRip`, `Blu-ray` → BD; `REMUX`; `WEB`, `WEB-DL`, `WEBRip` → WEB; `TV`, `HDTV`; `DVD`), códec (`x264`/`H.264`/`AVC`, `x265`/`HEVC`/`H.265`, `AV1`, `VP9`), profundidad (`10bit`, `10-bit`, `Hi10P`, `Hi10`), `Dual Audio`, `Multi-Subs`/`Multi Sub`, temporada (`S02`, `Season 2`, `2nd Season`), episodios (`- 05`, `E05`, `S01E05`, rangos `01-12`, `01~12`), batch (`[Batch]`, `Complete`, rango de episodios o temporada sin episodio), versión `v2` ignorada para el número. `seadexBest` **nunca** sale del nombre (lo pone el mapeo de la extensión).
- **`pickEpisodeFile(files, episode, titles): Int?`** — devuelve `CandidateFile.index` (no la posición); salta no-video (`isVideoPath`: `mkv mp4 webm avi m4v mov ts`), `sample`, y `NCOP`/`NCED`/`PV`/`CM`/`Extras`/`SP`/`OVA` salvo que se pidan; entiende `S02E03` y numeración absoluta; desempate por el archivo más grande; `null` si nada encaja (la UI abre la sección Files del torrent para elegir a mano).
- **Fixtures compartidas (propuesta)**: `contracts/fixtures/release/names.json` (`[{ "name": "...", "expected": ReleaseTags }]`) y `contracts/fixtures/release/packs.json` (`[{ "files": [...], "episode": 3, "titles": [...], "expected": 7 }]`), generadas a partir de los tests de `tk-core/src/release.rs` de la ola 3. Rust y Kotlin (`:core:metadata` o un `:core:release` JVM puro) las consumen como oráculo, igual que `contracts/fixtures/metadata/expected.json`. Mientras no existan, la tabla de tests de `release.rs` es la referencia.
- **Ranking de fuentes** (`tk-core/src/sources.rs::score`): Android porta la misma fórmula (incluidos los bonus por `seadexBest` y por `batch` que contiene el episodio) en `:core:extensions` junto a `SourceNormalizer`. La fórmula vive en el doc-comment de `score`; cualquier cambio allí es un cambio de contrato para ambos.

### 2.4 Ajustes (`TorrentSettings`)

Mismos campos y límites que `TorrentSettingsVm` (`10` §5.2: límites ↓/↑ en KiB/s con 0 = sin límite y tope `1 000 000`, puerto 0 = aleatorio, UPnP, `maxPeers` 1..=500, DHT/PEX/LSD, `afterWatch`, `backgroundDownloads` + `backgroundAck`; sin ack → background apagado). Añadidos solo Android, con valores por defecto conservadores:

| Campo | Defecto | Efecto |
|---|---|---|
| `wifiOnly: Boolean` | `true` | Sin red no medida no se descarga en segundo plano; la reproducción pide confirmación en datos móviles (§4.1). |
| `pauseOnLowBattery: Boolean` | `true` | Pausa descargas en segundo plano con batería < 20 % sin cargador o con ahorro de batería. |
| `uploadOnMobileData: Boolean` | `false` | Con red medida el motor no sube datos (en librqbit, límite de subida mínimo; `disable_upload` es una feature de compilación, no un ajuste en vivo). |
| `storageLocation: StorageLocation` | `APP_SPECIFIC` | `APP_SPECIFIC` o `USER_FOLDER(treeUri)`; §6. |

El defecto de `maxPeers` en Android es **30** (no 50) y el de `upnp` es `false` (en redes móviles no existe y en Wi-Fi ajenas no conviene); el saneado es el mismo.

### 2.5 Textos

Mismas claves que desktop traducidas a recursos Android sustituyendo `.` por `_`: `torrent.state.downloading` → `R.string.torrent_state_downloading`, `src.tag.dual_audio` → `src_tag_dual_audio`. Solo `values/` (en) y `values-es/` (es), como el resto de la app. Las cadenas nuevas solo Android usan el prefijo `torrent_android_*` (permisos, notificaciones, red medida, batería).

## 3. Pantallas

Todas con estados de carga (`LoadingIndicator` M3E o skeleton), vacío (`EmptyState`) y error (`ErrorState` con reintento), como exige `01-design-system.md`. Colores solo por roles de `MaterialTheme.colorScheme` generados desde `packages/tokens`.

### 3.1 Destino de navegación

- **Compact (teléfono)**: la barra inferior ya tiene 5 destinos; Torrents **no** es un destino propio. Entra por **Biblioteca → pestaña "Torrents"** (junto a Continuar viendo / Historial) y por el chip "Abrir en el cliente" del reproductor. Badge en la pestaña con el número de descargas activas.
- **Medium / expanded (tablet, plegable abierto)**: `NavigationSuiteScaffold` muestra el rail; Torrents aparece como destino con el icono `magnet`, en el mismo orden que el rail de desktop (Inicio, Buscar, Biblioteca, **Torrents**, Extensiones, Link, Ajustes).
- Ruta Navigation3 nueva: `Torrents(section: TorrentSection = LIBRARY, torrentId: String? = null)`. Sin selección, Overview/Files/Peers/Trackers muestran un selector con la biblioteca o un `EmptyState` que lleva a Library (igual que desktop).

### 3.2 Biblioteca de torrents (entrada)

- Lista `LazyColumn` con `key = id` de tarjetas: póster (de `TorrentLabel.poster`), serie, "Ep. N" o "Batch · N archivos", tamaño, pill de estado, `LinearWavyProgressIndicator` con el progreso, fecha relativa. Menú de 3 puntos: Abrir, Reanudar/Pausar, Conservar / No conservar, Compartir archivo, Borrar.
- Selección múltiple con pulsación larga → barra contextual (borrar con / sin archivos, pausar, conservar).
- Expanded: `ListDetailPaneScaffold` (lista a la izquierda, detalle del torrent a la derecha con las pestañas de §3.3).
- Vacío: "Aún no hay torrents. Elige una fuente torrent en un episodio para empezar."

### 3.3 Detalle de un torrent (Overview / Files / Peers / Trackers)

`PrimaryScrollableTabRow` con las cuatro secciones por torrent + `HorizontalPager`. Cabecera fija: nombre (2 líneas), pill de estado, acciones Pausar/Reanudar y menú (abrir archivo, compartir magnet, quitar).

- **Overview**: progreso grande (`CircularWavyProgressIndicator` con %), rejilla 2×N de cifras (descargado, subido, tamaño seleccionado / total, piezas `piecesHave/pieces × pieceBytes`, ↓/↑, restante o "Transcurrido" o "Streaming", seeders / leechers / wires), info-hash con botón copiar (portapapeles, toast de confirmación), fila de pills de protocolo `DHT LSD PEX NAT Forwarding Persisting Streaming` (activo = rol `tertiaryContainer`, inactivo = `surfaceContainerHighest`). Banner de error con el mensaje si `state = ERROR`.
- **Files**: `SearchBar` arriba + lista de filas (nombre, episodio detectado, tamaño, barra de progreso fina, icono de streams activos). En compact no hay tabla: cada archivo es una fila de 2 líneas. Toque → "Reproducir este archivo" (usa el mismo flujo de §4.1 con el archivo elegido a mano) o "Descargar solo este".
- **Peers**: lista densa (ip:puerto, cliente, país como código ISO de 2 letras, progreso, ↓/↑) ordenable por un `SingleChoiceSegmentedButtonRow` (↓, ↑, progreso). Las flags se muestran como texto monoespaciado; un botón "?" abre un bottom sheet con la leyenda (`torrent_peers_flag_<letra>`). Expanded: tabla real con cabeceras ordenables, como desktop.
- **Trackers**: filas con url (elipsis central), pill de estado (`WORKING` ok, `FAILED` error, `UPDATING`/`UNKNOWN` neutral), seeders / leechers / descargado y el mensaje en línea secundaria.

Las cifras usan la fuente monoespaciada de los tokens y el mismo formateador que desktop (`1.38 GB`, `4.2 MB/s`, unidades decimales).

### 3.4 Descargas (segundo plano)

- Primera visita (o `backgroundAck = false`): pantalla de aviso, no un diálogo descartable: ilustración, texto (uso de CPU, batería, almacenamiento y datos; la app sube datos a otros peers mientras descarga; Android mostrará una notificación permanente) y botón lleno "Entiendo" + botón de texto "Ahora no". "Entiendo" guarda el ack y pide `POST_NOTIFICATIONS` si falta (Android 13+), explicando por qué.
- Después: interruptor "Descargas en segundo plano", chips de estado de red / batería ("Esperando Wi-Fi", "Pausado: batería baja") y la lista de descargas no completadas que no se están reproduciendo, con pausar / reanudar / quitar.
- Añadir una descarga: desde el selector de fuentes (acción secundaria "Descargar" en una fuente torrent) o desde la lista de episodios ("Descargar episodio" / "Descargar temporada" si la mejor fuente es un batch).

### 3.5 Ajustes de torrents

Subpágina de **Ajustes → Torrents** (no una sección del cliente en compact; en expanded también accesible desde el rail de Torrents). Grupos: Velocidad (límites ↓/↑ con `Slider` + campo numérico; 0 = sin límite), Red (Solo Wi-Fi, subir con datos móviles, puerto de escucha con validación 1024..=65535 o 0, UPnP, máx. conexiones, DHT/PEX/LSD), Almacenamiento (ubicación §6, espacio libre, "Después de ver: conservar / borrar"), Segundo plano (interruptor deshabilitado hasta aceptar el aviso, pausar con batería baja). Guardado inmediato por campo (DataStore), con validación inline y errores en `supportingText`.

### 3.6 Reproductor y selector de fuentes

- **Selector de fuentes**: `ModalBottomSheet` (compact) / side sheet (expanded, `AdaptiveSheet` de E11). Fila superior de `FilterChip` con scroll horizontal: Tipo (Web / Torrent), BD / WEB / REMUX, resolución, códec, 10-bit, Dual audio, Multi-sub, Batch, SeaDex best, fiabilidad mínima. Cada fuente muestra sus etiquetas como `AssistChip` pequeños (SeaDex best con rol `primaryContainer`), seeders y tamaño. **Filtrar nunca reordena**: el orden es el del ranking. Estado de filtros local del sheet (`rememberSaveable`).
- **Overlay de arranque / buffering** (encima de `PlayerSurface`): "Esperando metadata…", luego velocidad ↓, peers conectados y % del archivo; desaparece al primer frame y vuelve si `Player.STATE_BUFFERING` dura > 1 s.
- **Barra de progreso**: los rangos descargados (`seekMarks`, cada 2-3 s) como pista secundaria del `Slider` personalizado, el rango "listo para reproducir" desde la posición actual con más contraste y marcas intro/outro/recap con botón "Saltar intro". Saltar a una zona no descargada es posible (el servidor HTTP mueve la prioridad), con el overlay de buffering.
- **Siguiente episodio**: si `batch`, el botón "Siguiente" reutiliza el torrent (`play(torrentId, media, ep + 1)`) sin abrir el selector.
- Chip "Abrir en el cliente" en el panel de información → `Torrents(OVERVIEW, torrentId)`.
- TV (futuro): las pestañas de §3.3 son `TabRow` enfocable; tablas como listas de filas enfocables; el selector de fuentes como pantalla completa con los filtros en una columna lateral.

## 4. Flujos

### 4.1 Reproducir una fuente torrent

1. Detalles → episodio → selector de fuentes (o Auto: la primera del ranking que no falle, igual que desktop; las torrent ya no se saltan).
2. **Comprobación de red** (solo Android): si la red es medida y `wifiOnly`, diálogo "Estás en datos móviles. Ver por torrent puede usar ~X GB." con "Ver igual" / "Elegir fuente web". X = tamaño del archivo si se conoce (tras metadata) o el `size` de la fuente.
3. `TorrentRepository.play(...)` → el servicio de torrents arranca si no lo estaba (§5.2) → `add` (espera metadata ≤ 90 s, overlay "Esperando metadata…") → `files` → `pickEpisodeFile` con el título principal + sinónimos de `Media` → `selectFile` → `setLabel` → `streamUrl`.
4. Media3 abre `http://127.0.0.1:<puerto>/<token>/<hash>/<índice>/<nombre>` (progresivo con `Range`). El servicio pasa a tipo `mediaPlayback` mientras haya stream (§5.2).
5. Errores: sin metadata a los 90 s → `Offline` ("No se encontraron peers"), sin archivo que encaje → abre Files del torrent con el mensaje "Elige el archivo del episodio", fallo de Media3 → `reportSourceResult` y siguiente fuente (Auto) o error con reintento (Manual).
6. Al salir o cambiar de episodio → `playbackEnded(torrentId, completed)`: política de `10` §4.3 (no persistido → borrar con archivos salvo que sea descarga en segundo plano; persistido → pausar si el segundo plano está apagado, si no, seguir según red / batería).

### 4.2 Siguiente episodio en un batch

`TorrentPlayback.batch = true` (más de un video en el torrent). "Siguiente" o el autoplay al terminar llaman `play(torrentId, media, ep + 1)`: el motor ya tiene el torrent (el id es el info-hash), `pickEpisodeFile` elige el archivo, `selectFile` lo añade a `onlyFiles` y lo prioriza, `setLabel` actualiza el episodio. Sin archivo para `ep + 1` (fin del pack) → flujo normal de fuentes. Opción en el menú del episodio: "Precargar el siguiente" (selecciona `ep + 1` con prioridad baja mientras se ve `ep`; solo en Wi-Fi).

### 4.3 Descargas en segundo plano con aviso

1. Usuario pulsa "Descargar" en una fuente torrent o en un episodio.
2. Si `!backgroundAck` → pantalla de aviso de §3.4. "Ahora no" cancela; "Entiendo" → ack → permiso de notificaciones → continúa.
3. `addDownload(source, media, episode)` (rechaza si el segundo plano no está habilitado; ofrece activarlo en el mismo snackbar).
4. El servicio en primer plano (`dataSync`, §5.2) mantiene la sesión; la notificación muestra el progreso agregado y acciones Pausar todo / Abrir.
5. Restricciones: sin Wi-Fi (si `wifiOnly`) → pausa y chip "Esperando Wi-Fi"; batería baja → pausa; al completar todo → el servicio se detiene (o pasa a seeding solo si el usuario lo pidió, nunca por defecto en Android).
6. Archivo completo → notificación "Episodio N listo" con "Ver" (abre el reproductor con el archivo local, sin red).

### 4.4 Abrir en el cliente desde el reproductor

Chip "Abrir en el cliente" → `Torrents(OVERVIEW, torrentId)` en un back stack que vuelve al reproductor (en compact el reproductor se pausa; con PiP activo sigue en la ventana flotante).

## 5. Motor y servicio

### 5.1 Opciones de motor

| Criterio | **librqbit vía UniFFI** (recomendado) | **libtorrent4j** (alternativa) |
|---|---|---|
| Código compartido | Reutiliza `crates/tk-torrent` de desktop: misma sesión, mismo servidor HTTP de streaming con prioridad de piezas, mismos `labels.json`, mismos tipos (UniFFI genera los `data class` desde los tipos de §2). Un solo lugar para arreglar bugs de streaming. | Implementación independiente en Kotlin: el servidor HTTP loopback, la prioridad de piezas (`setPieceDeadline` / prioridades de libtorrent) y la persistencia se reescriben. |
| Build | Requiere NDK + `cargo-ndk` (o `cargo ndk`) y targets `aarch64-linux-android`, `armv7-linux-androideabi`, `x86_64-linux-android`. El equipo de desarrollo ya es Linux, así que el obstáculo de `00` ("NDK vía WSL") desaparece. TLS para trackers HTTPS: `native-tls` en Android es OpenSSL (habría que compilarlo con `openssl-sys` `vendored`); alternativa `rustls` con proveedor `ring` (en Android no hay restricción de CMake/NASM como en Windows). A decidir con `cargo tree` en la rama Android. | AAR precompilados por ABI desde Maven Central; sin NDK propio. |
| Tamaño (estimado, a medir) | librqbit + tokio + axum compilados en release con `opt-level="z"`, LTO y `strip`: del orden de pocos MB por ABI. | libtorrent-rasterbar + Boost + OpenSSL por ABI: del orden de varios MB por ABI. |
| ABI / dispositivos | `arm64-v8a` obligatorio, `armeabi-v7a` y `x86_64` opcionales (App Bundle separa por ABI). | Mismo conjunto de ABIs que publique el proyecto. |
| Mantenimiento | librqbit es activo pero joven: PEX no se puede apagar por opción (`KNOWN-ISSUES.md`, sección Torrents), el estado de trackers expuesto es limitado. Cada versión de librqbit se fija igual que en desktop (`=9.1.0`). | libtorrent es muy maduro (DHT, PEX, LSD, uTP, trackers completos); los bindings dependen de un mantenedor principal: comprobar la actividad del proyecto y la última versión al implementar. |
| Streaming | Ya resuelto en desktop (`FileStream` de librqbit, el seek mueve la prioridad). | Hay que escribirlo: servidor HTTP en Kotlin (p. ej. NanoHTTPD o un `ServerSocket` propio) que bloquea leyendo piezas con deadline. |

**Decisión propuesta**: librqbit vía UniFFI, en un módulo `:core:torrent` con la librería nativa `libtk_torrent_ffi.so` generada por un crate fino nuevo `crates/tk-torrent-ffi` (solo exporta la API de `TorrentEngine` de `10` §2 + un runtime tokio propio). **Fallback documentado**: libtorrent4j detrás de la misma interfaz `TorrentRepository`, si el build NDK o el tamaño no son aceptables. Esta decisión es la primera tarea de la rama Android (prueba de concepto: añadir un magnet, servir un archivo por loopback, reproducir en Media3 en un dispositivo real).

### 5.2 Servicio en primer plano y ciclo de vida

- `TorrentService : LifecycleService` en `:core:torrent`, dueño único de la sesión (un proceso; el motor no corre en el proceso `:extensions` futuro).
- Tipos de servicio (Android 14+ obliga a declararlos): `mediaPlayback` mientras hay un stream abierto (se integra con el `MediaSessionService` del reproductor cuando exista; mientras tanto, el propio servicio de torrents con tipo `mediaPlayback`), `dataSync` para descargas en segundo plano. Permisos en el manifest: `FOREGROUND_SERVICE`, `FOREGROUND_SERVICE_MEDIA_PLAYBACK`, `FOREGROUND_SERVICE_DATA_SYNC`, `POST_NOTIFICATIONS`, `WAKE_LOCK`, `ACCESS_NETWORK_STATE` (ya está), `ACCESS_WIFI_STATE`.
- **Límite de `dataSync` en Android 15**: el sistema limita este tipo a unas 6 h por cada 24 h y llama `Service.onTimeout`; ahí se pausa todo, se guarda el fastresume y se muestra la notificación "Descargas pausadas por el sistema; toca para reanudar". Alternativa a evaluar en la implementación: *user-initiated data transfer jobs* (`JobInfo.Builder.setUserInitiated`, permiso `RUN_USER_INITIATED_JOBS`, Android 14+), que encaja con descargas iniciadas por el usuario y respetan red/batería por diseño.
- Sin descargas activas ni streams → `stopForeground` + `stopSelf` tras 30 s; el motor hace `shutdown` (guarda la sesión). La siguiente reproducción lo vuelve a arrancar (arranque bajo demanda, como desktop).
- `WifiLock` (`WIFI_MODE_FULL_LOW_LATENCY` mientras se reproduce, `WIFI_MODE_FULL_HIGH_PERF` solo durante descargas en primer plano) y `PARTIAL_WAKE_LOCK` solo mientras haya transferencia real; se sueltan al pausar.
- Notificación (canal `torrents`, importancia baja): título del episodio o "N descargas", barra de progreso, ↓ velocidad, acciones Pausar / Reanudar / Detener. Durante la reproducción la notificación de Media3 manda; la de torrents se fusiona o se oculta (no dos notificaciones para lo mismo).

### 5.3 Streaming hacia Media3

- Mismo esquema que desktop: servidor HTTP en `127.0.0.1` con token aleatorio por arranque, `GET`/`HEAD` con `Range`, `Content-Type` por extensión. Media3 lo abre con `DefaultHttpDataSource` / `OkHttpDataSource` sin cabeceras y `ProgressiveMediaSource` (MKV por `MatroskaExtractor`, MP4 por `Mp4Extractor`). `StreamSource.container` gana un valor `PROGRESSIVE` (o se usa `MP4` con el tipo por extensión; a decidir en `:core:model`).
- `DefaultLoadControl` con buffer mayor para torrents (p. ej. 30-60 s) y `setPrioritizeTimeOverSizeThresholds(true)`; el servidor responde con lecturas que bloquean hasta tener la pieza, así que los timeouts de lectura de la data source deben ser largos (≥ 60 s) para no cortar el arranque.
- **Subtítulos ASS/SSA**: los fansubs de anime usan ASS con estilos y tipografías incrustadas; Media3 extrae los SSA de MKV pero los muestra sin estilos avanzados. Para torrents se recomienda priorizar el motor mpv de Android (M1, `00`: libmpv de mpv-android) cuando exista; mientras tanto Media3 con aviso "Subtítulos con formato simplificado" en el panel de pistas.
- HEVC 10-bit: depende del decodificador del dispositivo. El selector marca con un icono las fuentes `HEVC` + `10-bit` si `MediaCodecList` no anuncia el perfil `HEVCProfileMain10`, y el filtro "Compatible con este dispositivo" (solo Android) las oculta. AV1: igual con `AV1ProfileMain10`.

### 5.4 Configuración del motor en Android

`EngineConfig` de `10` §2 con: `persistence_dir = filesDir/torrents/`, `download_dir` según §6, `listen_port = 0` por defecto, `upnp = false`, `lsd` solo en Wi-Fi, `max_peers = 30`, DHT con persistencia en `filesDir/torrents/dht.json`. Al pasar a red medida: límite de subida a 1 KiB/s (o `uploadOnMobileData`) y `set_limits` en vivo; al perder la red: nada (librqbit reintenta), con el chip "Sin conexión" en el overlay.

## 6. Almacenamiento

- **Por defecto: almacenamiento específico de la app** (`Context.getExternalFilesDir("torrents")`, sin permisos). Ventajas: rutas de archivo reales (el motor necesita `File`/descriptores con acceso aleatorio), se borra con la app. Inconveniente: otras apps no lo ven, y desinstalar borra las descargas (se avisa en Ajustes).
- **Carpeta del usuario (SAF)**: `ACTION_OPEN_DOCUMENT_TREE` + `takePersistableUriPermission`. El motor no escribe en un `content://` directamente; opciones, de más simple a más compleja: (a) descargar en el almacenamiento de la app y **mover** el archivo completo a la carpeta elegida al terminar (`DocumentsContract`, copia en streaming, luego borra el original), (b) una `StorageFactory` propia de librqbit (`SessionOptions::default_storage_factory`) que escriba por `ParcelFileDescriptor` obtenidos del árbol SAF. Se implementa (a) primero; `TorrentLibraryItem.path` pasa a ser el `content://` tras moverlo, y la biblioteca sigue reproduciendo desde ahí (Media3 lee `content://`).
- **Exportar a Descargas públicas**: acción "Guardar en Descargas" (MediaStore `Downloads`, sin permisos en Android 10+).
- Espacio: antes de `add` sin `only_files`, y antes de seleccionar un archivo, se comprueba `StorageManager.getAllocatableBytes`; si no cabe, error claro con el tamaño necesario. Política "después de ver" igual que desktop.

## 7. Red medida, batería y privacidad

- `ConnectivityManager.registerDefaultNetworkCallback`: `NET_CAPABILITY_NOT_METERED` decide `wifiOnly`; VPN no cambia la política. Los cambios aplican límites en vivo (`set_limits`) y pausan / reanudan descargas en segundo plano, nunca la reproducción en curso (que ya fue confirmada en §4.1).
- Batería: `ACTION_BATTERY_LOW/OKAY`, `PowerManager.isPowerSaveMode` → pausa de descargas en segundo plano. Doze: un servicio en primer plano sigue con red; sin él (servicio detenido) no hay nada corriendo.
- Seeding: en Android no se sigue compartiendo después de completar salvo que el usuario lo active por torrent (el aviso de §3.4 lo explica). La ratio de subida se muestra en Overview.
- Privacidad: el aviso menciona que la IP es visible para los peers. Nada de telemetría (guard de `contracts/test`). Ninguna URL del servidor loopback sale del dispositivo; Link no expone torrents en esta fase.

## 8. Módulos y orden de implementación (propuesta para la rama Android)

| Paso | Módulo / archivo | Entregable |
|---|---|---|
| 1 | `crates/tk-torrent-ffi` (nuevo, desktop workspace) + `:core:torrent` | PoC: magnet → archivo → URL loopback → Media3 en dispositivo real. Decide librqbit vs libtorrent4j (§5.1). |
| 2 | `:core:model` | Tipos de §2.2 + `TorrentRepository`; fixtures de `contracts/fixtures/release/` (§2.3). |
| 3 | `:core:release` (JVM puro, o dentro de `:core:metadata`) | `parseRelease`, `isVideoPath`, `pickEpisodeFile` con los tests de las fixtures compartidas. |
| 4 | `:core:extensions` | Ruta `torrent` también para extensiones `custom`/`onlinestream` que la declaren o expongan `torrentAll` (mismo criterio que `10` §6); quitar el filtro `type != TORRENT` de `ExtensionRepositoryImpl`; etiquetas y ranking. |
| 5 | `:core:torrent` | `TorrentService`, notificación, red/batería, almacenamiento §6, política "después de ver". |
| 6 | `feature/player` | Rama torrent del reproductor, overlay, marcas en la barra, siguiente episodio en batch, selector con filtros. |
| 7 | `feature/torrents` (nuevo) + `feature/settings` | Pantallas de §3 (biblioteca, detalle con pestañas, descargas con aviso, ajustes). |

Criterios de aceptación: tests JVM de `:core:release` contra las fixtures compartidas (mismos resultados que `cargo test -p tk-core`), test instrumentado del servidor loopback (Range correcto, 404 con token incorrecto), y prueba manual en dispositivo: reproducir un episodio de un batch, pasar al siguiente sin volver a resolver fuentes, descargar en segundo plano con la pantalla apagada 10 min en Wi-Fi y comprobar que pausa al pasar a datos móviles.

## 9. Preguntas abiertas

1. ¿Torrents como destino propio en la barra inferior del teléfono (6 destinos no caben bien en M3) o dentro de Biblioteca como propone §3.1?
2. ¿Seeding en Android completamente apagado tras completar, o una ratio objetivo (p. ej. 1.0) solo en Wi-Fi + cargando?
3. ¿Mover a SAF al terminar (§6 a) es suficiente, o el usuario necesita ver los archivos parciales en su carpeta durante la descarga?
4. ¿Se espera al motor mpv de Android (M1) para habilitar torrents en Android, dado el problema de ASS en Media3 (§5.3)?
