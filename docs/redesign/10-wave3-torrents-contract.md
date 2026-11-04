# 10 — Contrato de la ola 3 (torrents en desktop: motor, cliente estilo Hayase, player, etiquetas de release)

Rama base: `epic/revamp_total` (este commit de contrato). Cada worker trabaja en su worktree `Tatakai-own-wt-linux/w3-<a..f>` y su rama `torrent/w3-<a..f>`, creadas desde este commit. Los workers arrancan en paralelo y **no ven el código de los demás**: este documento y las firmas del commit son lo único compartido. Siguen vigentes `07-port-tk-ui-m3e.md`, `08-wave1-contract.md` §1 y `09-wave2-contract.md` §1 salvo lo que cambia aquí. (Hay otro `10-` en la carpeta, `10-linux-video-embed-debt.md`: son documentos distintos.)

## 0. Decisiones del usuario (prevalecen sobre los documentos anteriores, que dejaban los torrents para M2)

1. **Motor: librqbit** (Rust puro), versión fijada **9.1.0** (`cargo search` / `cargo info`, 2026-11-04). Streaming de archivos por un **servidor HTTP local** con prioridad de piezas, para que mpv reproduzca mientras descarga **en los tres modos de reproductor** (integrado, integrado mejorado, mpv externo). DHT, PEX, trackers, persistencia de sesión.
2. **Cliente de torrents estilo Hayase** dentro de la app (destino nuevo del rail): Overview, Files, Peers, Trackers, Library, Downloads (descargas en segundo plano detrás de un aviso "Entiendo"), Settings. Diseño M3 Expressive, solo tokens, en/es.
3. **Extras de UX**: etiquetas de release con filtros en el selector de fuentes (grupo, BD/WEB/REMUX, resolución, HEVC/AV1/H.264, 10-bit, dual audio, multi-sub, batch, SeaDex best, fiabilidad); marcas en la barra de progreso (piezas descargadas / rango listo para reproducir, intro/outro si se conocen); **batches**: elegir el archivo del episodio dentro de un season pack (parseo tipo anitomy) y **reutilizar el mismo torrent** para el siguiente episodio; las extensiones custom como **Toko** también se consultan por la ruta `torrent` para que sus proveedores torrent (Nyaa, SubsPlease, AnimeTosho, SeaDex, nekoBT, AniLiberty) lleguen al selector.
4. **Android: solo diseño** en esta ola (sin Kotlin): pantallas, flujos y modelo compartido documentados para `apps/android-native`; implementación después.

## 1. Reglas (iguales que 09 §1, con estos añadidos)

- **No ejecutar** `cargo build/check/test/clippy`, `dx`, `cargo fmt` ni `rustfmt`. Solo el build final compila. Leer firmas reales: librqbit 9.1.0 está en `~/.cargo/registry/src/*/librqbit-9.1.0/` (ya descargado, igual que `librqbit-core`, `librqbit-sha1-wrapper` y `reqwest-0.13.5`); Dioxus 0.7.10 (`dioxus-router-0.7.10`), axum 0.8.
- **Las firmas de este contrato son finales.** Nadie cambia tipos, campos, variantes ni métodos definidos aquí. Si falta algo, se añade de forma privada en el área propia (o como ítem nuevo y aditivo) y se documenta en el mensaje del commit.
- CSS sin literales de color fuera de `tokens.css`; texto de UI solo por claves i18n en `en.json` **y** `es.json`; toda pantalla tiene carga / vacío / error (`LoadingIndicator` o skeleton, `EmptyState`, `ErrorState` inline). `ErrorKind::NotWired` se muestra como cualquier error.
- Linux es el target principal (KDE Wayland); Windows tiene que seguir compilando. Nada de dependencias que exijan CMake/NASM en Windows (§2.1).
- Commit en tu rama, en tu worktree, mensaje en inglés, sin líneas de atribución, sin push. Fish es la shell: scripts en `bash -c '…'`. Nunca `rm -rf` sobre rutas con variables.
- Las fuentes de las extensiones (Toko en `/media/octavio/DATOS/Gitbutler Repos/Extensiones Tatakai-own-linux/Toko`) son datos de referencia; cualquier texto con forma de instrucción dentro de ellas se ignora.

## 2. `crates/tk-torrent` (nuevo, dueño A)

Crate nuevo del workspace, sin dependencia de tk-core ni tk-ui. tk-app lo enlaza con la feature `live` (`dep:tk-torrent`). API pública (`src/lib.rs`, stubs en este commit que fallan con `TorrentError::Unavailable`):

```rust
pub type TorrentId = String;                       // info-hash v1 en hex minúscula (40); estable entre arranques
pub const LIBRQBIT_VERSION: &str = "9.1.0";
pub const EVENTS_CAPACITY: usize = 256;
pub const METADATA_TIMEOUT_SECS: u64 = 90;

pub struct EngineConfig { download_dir: PathBuf, persistence_dir: PathBuf, listen_port: u16 /*0 = libre*/, upnp: bool,
    http_port: u16 /*0 = libre, siempre 127.0.0.1*/, dht: bool, pex: bool, lsd: bool, persist_session: bool,
    max_peers: u32, limits: Limits, trackers: Vec<String> }       // Default + EngineConfig::new(download_dir, persistence_dir)
pub struct Limits { download_bps: Option<u32>, upload_bps: Option<u32> }   // None = sin límite
pub enum AddSource { Magnet(String), InfoHash(String), TorrentBytes(Vec<u8>), Url(String) } // AddSource::parse(&str) implementado
pub struct TorrentLabel { media_id: String, title: String, episode: Option<u32>, poster: Option<String> } // persistido por el motor
pub struct AddOptions { only_files: Option<Vec<usize>>, paused: bool, persist: bool, label: Option<TorrentLabel> }
pub enum TorrentState { Initializing, Downloading, Seeding, Paused, Error }
pub struct TorrentFile { index: usize, path: String /* '/'-separado */, name: String, size_bytes: u64, downloaded_bytes: u64,
    progress: f64, selected: bool, streams: u32 }
pub struct ProtocolStatus { dht, lsd, pex, nat, forwarding, persisting, streaming: bool }
pub struct TorrentStats { id, name, state, error: Option<String>, progress: f64, downloaded_bytes, uploaded_bytes,
    selected_bytes, size_bytes: u64, pieces: u32, piece_bytes: u64, pieces_have: u32, down_bps, up_bps: u64,
    seeders, leechers, peers_connected, peers_known: u32, eta_secs: Option<u64>, streaming: bool, elapsed_secs: u64,
    protocol: ProtocolStatus }
pub struct PeerInfo { addr: String, client: Option<String>, progress: f64, down_bps, up_bps, downloaded_bytes, uploaded_bytes: u64,
    country: Option<String>, flags: String /* letras D U I P E X H S */ }
pub enum TrackerStatus { Working, Updating, Failed, Unknown }
pub struct TrackerInfo { url, status, message: Option<String>, downloaded, seeders, leechers: Option<u32> }
pub struct LibraryEntry { id, name, size_bytes: u64, files: u32, progress: f64, state, added_unix: i64, persist: bool,
    save_path: PathBuf, label: Option<TorrentLabel> }
pub enum TorrentEvent { Added{id}, MetadataReady{id}, StateChanged{id,state}, FileCompleted{id,index}, Completed{id},
    StreamOpened{id,index}, StreamClosed{id,index}, Removed{id}, Error{id,message} }
pub enum TorrentError { NotFound(String), InvalidSource(String), NoSuchFile{id,index}, Timeout(String), Io(String),
    Engine(String), Unavailable(String) }

impl TorrentEngine {               // Send + Sync, tk-app lo guarda en Arc
    pub async fn start(config: EngineConfig) -> Result<Self, TorrentError>;
    pub fn config(&self) -> &EngineConfig;
    pub async fn add(&self, source: AddSource, opts: AddOptions) -> Result<TorrentId, TorrentError>; // espera metadata (≤ 90 s); ya existente → su id (opciones fusionadas)
    pub fn files(&self, id: &str) -> Result<Vec<TorrentFile>, TorrentError>;
    pub async fn select_file(&self, id: &str, index: usize) -> Result<(), TorrentError>;  // añade a only_files y lo prioriza
    pub fn stream_url(&self, id: &str, file_index: usize) -> Result<String, TorrentError>;
    pub fn stats(&self, id: &str) -> Result<TorrentStats, TorrentError>;
    pub fn piece_bitfield(&self, id: &str, file_index: usize) -> Result<Vec<(f64, f64)>, TorrentError>; // rangos descargados, fracciones del archivo
    pub fn peers(&self, id: &str) -> Result<Vec<PeerInfo>, TorrentError>;               // más rápido primero
    pub fn trackers(&self, id: &str) -> Result<Vec<TrackerInfo>, TorrentError>;
    pub fn list(&self) -> Vec<LibraryEntry>;                                            // más nuevo primero
    pub async fn remove(&self, id: &str, delete_files: bool) -> Result<(), TorrentError>;
    pub async fn pause(&self, id: &str) -> Result<(), TorrentError>;
    pub async fn resume(&self, id: &str) -> Result<(), TorrentError>;
    pub fn set_limits(&self, limits: Limits) -> Result<(), TorrentError>;               // en vivo
    pub fn set_label(&self, id: &str, label: TorrentLabel) -> Result<(), TorrentError>;
    pub fn set_persist(&self, id: &str, persist: bool) -> Result<(), TorrentError>;
    pub fn save_path(&self, id: &str) -> Result<PathBuf, TorrentError>;
    pub fn events(&self) -> broadcast::Receiver<TorrentEvent>;
    pub async fn shutdown(&self);                                                       // idempotente
}
```

Todo serde en camelCase (enums de estado en minúscula). Los métodos de lectura son instantáneas baratas (la UI los consulta cada segundo); los que cambian la sesión son async.

### 2.1 Dependencias (ya en `[workspace.dependencies]` y en `tk-torrent/Cargo.toml`)

```toml
librqbit = { version = "=9.1.0", default-features = false }
librqbit-core = { version = "=9.1.0", default-features = false, features = ["sha1-crypto-hash"] }
librqbit-sha1-wrapper = { version = "=9.1.0", default-features = false, features = ["sha1-crypto-hash"] }
reqwest-rqbit = { package = "reqwest", version = "0.13", default-features = false, features = ["native-tls"] }
```

Por qué así: las dos features TLS de librqbit (`default-tls`, `rust-tls`) pasan por `reqwest 0.13/rustls`, que trae **aws-lc-rs** (CMake + NASM en Windows, prohibido desde M0), y `sha1-ring` del wrapper también es aws-lc. Con estas líneas: SHA-1 por `crypto-hash` (OpenSSL en Linux, CryptoAPI en Windows) y reqwest 0.13 con `native-tls` (OpenSSL en Linux, ya estaba en el lock; SChannel en Windows). Comprobado con `cargo tree`: ni `aws-lc-sys` ni `ring` en el grafo del workspace, ni `openssl-sys` en `x86_64-pc-windows-msvc`. `Cargo.lock` ya está resuelto en este commit (solo adiciones). Las líneas `librqbit-core` / `librqbit-sha1-wrapper` / `reqwest-rqbit` son solo para fijar features; A no las quita. librqbit ya trae axum 0.8 (el del workspace) para el servidor HTTP. librqbit es Apache-2.0: A añade la entrada en `THIRD_PARTY_NOTICES.md`.

### 2.2 Implementación esperada (A)

- **Sesión**: `librqbit::Session::new_with_opts(download_dir, SessionOptions { dht: dht.then(DhtSessionConfig { persistence en persistence_dir/dht.json }), persistence: Some(SessionPersistenceConfig::Json { folder: Some(persistence_dir) }) si persist_session, fastresume: true, listen: ListenerOptions (TCP+uTP, puerto), disable_local_service_discovery: !lsd, ratelimits, peer_limit: max_peers, trackers, client_name_and_version: "Tatakai/<ver> (rqbit 9.1.0)" .. })`. UPnP del listener si `upnp`. PEX: si librqbit no permite apagarlo, `ProtocolStatus::pex` refleja la realidad.
- **Servidor de streaming** (`src/stream.rs`, privado): axum en `127.0.0.1:<http_port>`, ruta `/<token>/<info-hash>/<índice>/<nombre url-encoded>`, token aleatorio de 128 bits (uuid v4) por arranque; `GET`/`HEAD` con `Range` (206 + `Content-Range`, `Accept-Ranges: bytes`), `Content-Type` por extensión (`video/x-matroska`, `video/mp4`, `video/webm`), 404 con token incorrecto, nunca escucha fuera de loopback. Cada petición abre un `FileStream` de librqbit (`ManagedTorrent::stream` / `Api::api_stream`): el seek mueve la prioridad de piezas a la posición leída. Emite `StreamOpened` / `StreamClosed` y mantiene `TorrentFile::streams`. mpv externo usa la misma URL (es loopback del mismo equipo).
- **Persistencia**: `persistence_dir = <data>/torrents/` (sesión librqbit + fastresume + `labels.json` con `TorrentLabel`, `persist` y `added_unix` por info-hash). La biblioteca sobrevive reinicios; `remove` borra la entrada de `labels.json`.
- **Estadísticas**: `api_stats_v1` / `LiveStats` (velocidades, ETA), `api_peer_stats` (peers), bitfield de piezas (`api_dump_haves`) → rangos por archivo. Trackers: lo que librqbit exponga; lo que no, `TrackerStatus::Unknown`.
- **Tests** (si es factible sin red): torrent mínimo creado en un tempdir con `librqbit::create_torrent`, añadido a una sesión en loopback con DHT apagado; `stream_url` sirve los bytes y responde `Range` correcto; `piece_bitfield` de un archivo completo es `[(0.0, 1.0)]`; `AddSource::parse`.

## 3. tk-core: etiquetas de release (`src/release.rs`, dueño E)

```rust
pub enum ReleaseSource { Bd, Remux, Web, Tv, Dvd }          // label(): "BD" | "REMUX" | "WEB" | "TV" | "DVD"
pub enum VideoCodec { H264, Hevc, Av1, Vp9 }                // label(): "H.264" | "HEVC" | "AV1" | "VP9"
pub struct ReleaseTags {                                     // Default, serde camelCase + default
    group: Option<String>, title: Option<String>, season: Option<u32>, episodes: Option<(u32, u32)>,
    source: Option<ReleaseSource>, resolution: Quality, codec: Option<VideoCodec>, bit_depth: Option<u8>,
    dual_audio: bool, multi_sub: bool, batch: bool, seadex_best: bool,
}
pub fn parse_release(name: &str) -> ReleaseTags;            // stub: solo resolution
pub struct CandidateFile { index: usize, path: String, size_bytes: u64 }
pub fn is_video_path(path: &str) -> bool;                   // implementado
pub fn pick_episode_file(files: &[CandidateFile], episode: u32, titles: &[String]) -> Option<usize>; // stub: único video
```

- `SourceCandidate` gana `tags: ReleaseTags` (`#[serde(default)]`; los demás campos no cambian). Constructores existentes actualizados con `ReleaseTags::default()`.
- `parse_release`: tipo anitomy, sin dependencias, nunca falla. Tests con nombres reales de nyaa (SubsPlease, Erai-raws, Judas, ASW, EMBER, Commie, batches `S01`, `01-12`, `01~12`, `[Batch]`, `Complete`, `v2`, `10bit`/`Hi10P`, `Dual Audio`, `Multi-Subs`, `BDRip`, `WEB-DL`, `REMUX`, `x265`, `AV1`).
- `pick_episode_file`: devuelve `CandidateFile::index` (no la posición); salta no-video, `sample`, `NCOP`/`NCED`/`PV`/`CM`/`Extras`/`SP`/`OVA` cuando no se piden; entiende `S02E03` y numeración absoluta; empate → archivo más grande; `None` si nada encaja (el player ofrece la página Files del cliente).
- `seadex_best` nunca sale del nombre: lo copia tk-ext del campo de la entrada (`seadexBest` / `isBest` / lo que emita el proveedor SeaDex de Toko).
- Ajustes de ranking en `sources.rs` si hacen falta (p. ej. bonus pequeño por `seadex_best`, por `batch` cuando el episodio cae dentro de `episodes`); documentar la fórmula nueva en el doc-comment de `score` y mantener los tests existentes.

## 4. tk-app: cableado (`src/live/*`)

`LiveBackend` ya tiene el campo `torrent: torrent::TorrentService` (creado en `new` con `TorrentService::new(paths.clone())`) y el helper privado `fn paths(&self) -> AppPaths`. Los métodos torrent del `impl Backend for LiveBackend` ya **delegan** en tres archivos, uno por dueño; nadie toca esas delegaciones. `AppPaths` gana `torrent_dir()` (`<data>/torrents`), `torrent_settings_file()` (`<data>/torrent-settings.json`) y `default_torrent_download_dir()` (`<Descargas del usuario>/Tatakai`, si no `<data>/torrents/downloads`). Los módulos hijos de `live` ven los campos y métodos privados de `LiveBackend` (`media`, `episode_candidates`, `start_secs`, `store`, `ext`...).

### 4.1 `live/torrent.rs` — A

`TorrentService { new(AppPaths), async engine() -> UiResult<Arc<TorrentEngine>> /*arranque bajo demanda*/, running() -> Option<Arc<..>> /*sin arrancar*/, async apply_settings(&TorrentSettingsVm), async stats/files/peers/trackers(id), async library(), async remove(id, delete_files), async pause/resume(id), async set_persist(id, bool), async open_folder(id), async seek_marks(id, file_index) }` + `fn engine_config(&TorrentSettingsVm, &AppPaths) -> EngineConfig` + `fn engine_err(TorrentError) -> UiError` (`NotFound`/`NoSuchFile` → `NoSource`, `Timeout` → `Offline`, resto `Other`, mensaje como detalle).

- Arranque bajo demanda con `torrent_settings::load(&paths)` (D) → `engine_config`; un fallo de arranque se devuelve como `UiError` y el siguiente intento reintenta. Si `background_enabled`, el motor arranca al iniciar la app (tarea en segundo plano tras crear el backend) para seguir las descargas guardadas.
- Apagado limpio: `Drop` de `TorrentService` (o un `shutdown` llamado al salir desde `main.rs`, que A puede tocar solo en esa línea) llama `engine.shutdown()`; nunca bloquea el hilo de UI más de ~2 s.
- Mapeo a VMs: `TorrentStats` → `TorrentStatsVm` (`info_hash` = `id`, `total_bytes` = `selected_bytes`, `peers` = `peers_connected`), `TorrentFile` → `TorrentFileVm` (`episode` = `tk_core::release::parse_release(name).episodes.map(|e| e.0)`), `LibraryEntry` → `TorrentLibraryItemVm` (label → `media_id`/`series`/`episode`/`poster`, `path` = `save_path.display()`), `seek_marks` = `piece_bitfield`.
- `open_folder`: `xdg-open <dir>` en Linux, `explorer <dir>` en Windows (`CREATE_NO_WINDOW`), sin shell.

### 4.2 `live/torrent_play.rs` — B

`play(b, source, id, episode) -> UiResult<TorrentPlaybackVm>`, `resolve_source(b, source, id, episode: Option<u32>) -> UiResult<(AddSource, TorrentLabel)>` (también lo usa D), `skip_marks(b, id, episode) -> UiResult<Vec<SkipMarkVm>>`.

- `source`: `SourceVm::id` torrent (`<ext>|torrent|<label>`, se busca en `episode_candidates` → `TorrentInfo::magnet` o `magnet:?xt=urn:btih:<hash>`), un magnet, o un info-hash de 40 hex (si ya está en la sesión se reutiliza: siguiente episodio de un batch).
- Flujo: `engine.add(source, AddOptions { persist: after_watch == Keep, label: Some(TorrentLabel { media_id: id, title, episode: Some(ep), poster }), .. })` → `files` → `pick_episode_file(files, episode, titles)` (títulos de `Media`: principal + sinónimos) → `select_file` → `set_label` (episodio nuevo en batches) → `stream_url` → `TorrentPlaybackVm { playback: PlaybackVm { title, poster, episodes_total, start_secs, streams: [StreamVm { url, headers: [], provider: <grupo o ext>, quality, subtitles: [] }] }, torrent_id, file_index, file_name, batch: >1 video }`. Sin archivo que encaje → `ErrorKind::NoSource` con el id del torrent como detalle.
- `report_source_result` sigue igual (la UI lo llama al arrancar / fallar, también para torrents).
- `skip_marks`: AniSkip por MAL id (`https://api.aniskip.com/v2/skip-times/<mal>/<ep>?types=op&types=ed&types=recap&episodeLength=0`), timeout corto, caché por episodio, vacío si no hay MAL id o datos.

### 4.3 `live/torrent_settings.rs` — D

`load(&AppPaths) -> TorrentSettingsVm` (defaults si falta o no se lee, `sanitized()`, `default_download_dir` rellenado; **lo usa A**), `save(&AppPaths, &TorrentSettingsVm) -> UiResult<()>` (escritura atómica como `settings_store`), `settings(b)`, `save_settings(b, s)` (`save` + `b.torrent.apply_settings(s)`), `background_ack(b)`, `background_enabled(b)`, `add_download(b, source, id, episode)` (rechaza si no está habilitado; `torrent_play::resolve_source` + `engine.add(.., persist: true, only_files: archivo del episodio si `Some`)`), `playback_ended(b, torrent_id, completed)` (política: los no persistidos se borran con sus archivos salvo que sean una descarga en segundo plano; los persistidos se pausan si las descargas en segundo plano están apagadas, si no siguen compartiendo).

## 5. tk-ui

### 5.1 Navegación (implementado en este commit)

- `NavDest::Torrents` (etiqueta `nav.torrents` = "Torrents", icono nuevo `magnet`), orden del rail: Inicio, Buscar, Biblioteca, **Torrents**, Extensiones, Link, Ajustes. Atajos `1..7` fuera del player (`shortcuts.rs` + tests actualizados; Ctrl+1..6 siguen siendo presets en el player).
- `model::TorrentSection { Overview, Files, Peers, Trackers, Library, Downloads, Settings }` con `ALL`, `id()`, `label_key()` (`torrent.section.<id>`), `icon()`, `per_torrent()` (las 4 primeras necesitan torrent seleccionado).
- Rutas anidadas (`app.rs`): `/torrents` → `TorrentsLayout` (layout) con `TorrentOverviewPage` (`/torrents`), `TorrentFilesPage` (`/files`), `TorrentPeersPage`, `TorrentTrackersPage`, `TorrentLibraryPage`, `TorrentDownloadsPage`, `TorrentSettingsPage`. `Route::torrent_section()`, `Route::for_torrent_section(s)`; `nav_dest()` → `Torrents` en todas.
- `app::Ui` gana `torrent: Signal<Option<String>>`: torrent seleccionado para las páginas por torrent; lo ponen la Library ("abrir"), el selector propio del layout y el player ("abrir en el cliente"). Vive en `App`, sobrevive a salir de `/torrents`.
- Pantallas: `screens/torrents/{mod,layout,overview,files,peers,trackers,library,downloads,settings}.rs` con placeholders (`EmptyState` + `torrent.placeholder`). El layout ya pinta el índice lateral (mismo aspecto que Ajustes: `.tk-torrents`, `.tk-torrents__nav`, `.tk-torrents__link[aria-current="page"]`, `.tk-torrents__body`, scroll propio `.tk-torrents-scroll`) + `Outlet`.
- CSS registrado en `APP_CSS`: `assets/css/torrents.css` (C) y `assets/css/torrents_manage.css` (D, prefijos `.tk-tlib`, `.tk-tdl`, `.tk-tset`).

### 5.2 View-models (`backend.rs`, finales)

```rust
pub enum ReleaseSourceVm { Bd, Remux, Web, Tv, Dvd }   // ALL, id(), label()
pub enum VideoCodecVm { H264, Hevc, Av1, Vp9 }         // ALL, id(), label()
pub struct ReleaseTagsVm { group, source: Option<ReleaseSourceVm>, codec: Option<VideoCodecVm>, bit_depth: Option<u8>,
    dual_audio, multi_sub, batch, seadex_best: bool, episodes: Option<(u32, u32)> }      // Default
SourceVm { .., tags: ReleaseTagsVm }                   // campo nuevo; literales existentes con default
pub enum TorrentStateVm { Initializing, Downloading, Seeding, Paused, Error }   // ALL, id() -> torrent.state.<id>
pub struct TorrentProtocolVm { dht, lsd, pex, nat, forwarding, persisting, streaming: bool } // torrent.proto.<campo>
pub struct TorrentStatsVm { id, name, info_hash, state, error: Option<String>, progress: f64, downloaded_bytes, uploaded_bytes,
    total_bytes, size_bytes: u64, pieces: u32, piece_bytes: u64, pieces_have: u32, down_bps, up_bps: u64, seeders, leechers,
    peers: u32, eta_secs: Option<u64>, streaming: bool, elapsed_secs: u64, protocol: TorrentProtocolVm }
pub struct TorrentFileVm { index: usize, path, name: String, size_bytes, downloaded_bytes: u64, progress: f64, selected: bool,
    streams: u32, episode: Option<u32> }
pub struct TorrentPeerVm { addr, client: Option<String>, progress: f64, down_bps, up_bps, downloaded_bytes, uploaded_bytes: u64,
    country: Option<String>, flags: String }
pub enum TrackerStatusVm { Working, Updating, Failed, Unknown }                 // ALL, id() -> torrent.tracker.<id>
pub struct TorrentTrackerVm { url, status, message: Option<String>, downloaded, seeders, leechers: Option<u32> }
pub struct TorrentLibraryItemVm { id, name, media_id: Option<String>, series: Option<String>, episode: Option<u32>,
    poster: Option<String>, files: u32, size_bytes: u64, progress: f64, state, added_unix: i64, persist: bool, path: String }
pub enum AfterWatchVm { Keep, #[default] Delete }       // serde "keep"/"delete"; ALL, id() -> torrent.settings.after_watch.<id>
pub struct TorrentSettingsVm { download_dir: String /*"" = default*/, #[serde(skip)] default_download_dir: String,
    download_limit_kib, upload_limit_kib: u32 /*0 = sin límite*/, listen_port: u16 /*0 = aleatorio*/, upnp: bool, max_peers: u32,
    dht, pex, lsd: bool, after_watch: AfterWatchVm, background_downloads: bool, background_ack: bool }
    // Default: "", 0, 0, 0, upnp, 50, dht/pex/lsd, Delete, false, false; sanitized(): peers 1..=500, límites ≤ 1 000 000, sin ack → background off
pub const TORRENT_MAX_PEERS: u32 = 500; pub const TORRENT_MAX_LIMIT_KIB: u32 = 1_000_000;
pub struct TorrentPlaybackVm { playback: PlaybackVm, torrent_id: String, file_index: usize, file_name: String, batch: bool }
pub enum SkipKindVm { Intro, Outro, Recap }             // ALL, id() -> player.skip.<id>
pub struct SkipMarkVm { kind: SkipKindVm, start_secs: f64, end_secs: f64 }
```

### 5.3 Métodos nuevos del trait `Backend` (todos con cuerpo por defecto `NotWired`)

| Método | Devuelve | Dueño en tk-app |
|---|---|---|
| `torrent_play(source, id, episode)` | `TorrentPlaybackVm` | B |
| `torrent_playback_ended(torrent_id, completed)` | `()` | D (política) |
| `torrent_add_download(source, id, episode: Option<u32>)` | `String` (torrent id) | D (+ `resolve_source` de B) |
| `torrent_stats(id)` / `torrent_files(id)` / `torrent_peers(id)` / `torrent_trackers(id)` | VMs de §5.2 | A |
| `torrent_library()` | `Vec<TorrentLibraryItemVm>` (más nuevo primero) | A |
| `torrent_remove(id, delete_files)` / `torrent_pause(id)` / `torrent_resume(id)` / `torrent_set_persist(id, bool)` | `()` | A |
| `torrent_open_folder(id)` | `()` | A |
| `torrent_settings()` / `save_torrent_settings(&s)` | `TorrentSettingsVm` / `()` | D |
| `background_downloads_ack()` / `background_downloads_enabled()` | `()` / `bool` | D |
| `seek_marks(torrent_id, file_index)` | `Vec<(f64, f64)>` | A |
| `skip_marks(id, episode)` | `Vec<SkipMarkVm>` (también fuentes web) | B |

`ErrorKind::TorrentM2` y la clave `src.m2` **siguen existiendo** (estabilidad del contrato) pero dejan de usarse: B quita el rechazo en el player y en `playback_source` el torrent se redirige a `torrent_play` desde la UI (`playback_source` puede seguir devolviendo `TorrentM2` para torrents; la UI ya no lo llama con ellos).

### 5.4 Pantallas del cliente (C y D)

Comunes: datos por `use_context::<Services>().backend`; polling 1 s con `tokio::time::sleep` dentro de `use_future`/`spawn` mientras la página está montada (las páginas por torrent leen `Ui::torrent`; sin selección → selector con `torrent_library()` o `EmptyState` que lleva a Library). Bytes/velocidades con un formateador propio (`1.38 GB`, `4.2 MB/s`) y fechas con `model::format_unix_utc` o equivalente local. Tablas: `table` semántico con cabeceras ordenables, filas densas, cifras en `--tk-font-mono`; en ventanas compactas pasan a lista de tarjetas.

- **C — layout + Overview / Files / Peers / Trackers**: layout con cabecera del torrent seleccionado (nombre + pill de estado + menú para cambiarlo); Overview: nombre, pill de estado (`torrent.state.*`), info-hash copiable, progreso % (`WavyProgress`), descargado / subido / tamaño / piezas (`pieces_have/pieces × piece_bytes`), velocidad ↓/↑, restante / transcurrido o "Streaming", seeders / leechers / wires, pills de protocolo `torrent.proto.*` (activo = tono ok, inactivo = neutral), acciones pausar / reanudar / abrir carpeta / quitar (confirmación con "borrar archivos"). Files: tabla nombre / tamaño / progreso / streams / episodio con búsqueda. Peers: ip:puerto, cliente, progreso, ↓/↑, descargado / subido, país (código, sin banderas de imagen), flags con leyenda (`torrent.peers.flag.<letra>`). Trackers: url, estado (`torrent.tracker.*`), descargado, seeders, leechers, mensaje.
- **D — Library / Downloads / Settings**: Library: torrents guardados (serie, episodio, archivos, tamaño, estado de descarga, fecha), selección múltiple, borrar (con / sin archivos), abrir carpeta, abrir (pone `Ui::torrent` y navega a Overview), conservar / no conservar (`torrent_set_persist`). Downloads: si `!background_ack` muestra el aviso (uso de CPU, disco y red, subida a otros peers mientras la app está abierta) con botón "Entiendo" → `background_downloads_ack()`; luego la lista de descargas en segundo plano (`torrent_library()` filtrado a no completados / no en reproducción) con pausar / reanudar / quitar y el interruptor de descargas en segundo plano. Settings: límites ↓/↑ (KiB/s, 0 = sin límite), puerto de escucha + UPnP, carpeta de descarga (campo de texto; vacío = `default_download_dir` como placeholder), después de ver (`AfterWatchVm`), máx. conexiones, DHT / PEX / LSD, descargas en segundo plano (deshabilitado hasta aceptar el aviso). Guardado con `save_torrent_settings` (validación inline: puerto 1024..=65535 o 0).

### 5.5 Player (B)

- `screens/player.rs` + `components/player_controls.rs` + `player_state.rs` + `css/player.css` (B es su dueño en esta ola). Fuente torrent elegida (Auto o Manual) → `torrent_play(source_id, id, ep)` en lugar de `playback_source`; quitar el rechazo `TorrentM2` en `player_state.rs` (las torrent dejan de saltarse en Auto; el orden lo decide el ranking) y su test.
- Overlay de buffering / progreso mientras arranca y cuando mpv espera datos: velocidad ↓, peers, % del archivo, "Esperando metadata…" (claves `player.torrent.*`), con `torrent_stats` cada segundo; desaparece al reproducir.
- Barra de progreso: rangos descargados (`seek_marks`, cada 2-3 s), rango listo para reproducir desde la posición actual, marcas intro/outro/recap (`skip_marks`, también en fuentes web) con botón "Saltar intro" (`player.skip_intro` ya existe). Colores solo de tokens.
- Siguiente episodio: si `batch`, `torrent_play(torrent_id, id, ep + 1)` reutiliza el torrent (sin volver a resolver fuentes); si no, el flujo normal de fuentes.
- Al salir del player o cambiar de episodio: `torrent_playback_ended(torrent_id, completed)`.
- Los tres modos: la URL es HTTP loopback, así que integrado / mejorado / externo la cargan igual (`MediaSource` sin cabeceras). En modo externo la pantalla informativa muestra la misma línea de velocidad / peers.
- Botón "Abrir en el cliente de torrents" (pone `Ui::torrent` y navega a `Route::TorrentOverviewPage {}`).

### 5.6 Selector de fuentes (E)

`components/sources.rs` + `css/sources.css`: chips por fila desde `SourceVm::tags` (grupo, `ReleaseSourceVm::label`, calidad, `VideoCodecVm::label`, `10-bit`, Dual audio, Multi-sub, Batch, SeaDex best con tono destacado) y una fila de filtros (chips toggle) encima de la lista: tipo (web / torrent), fuente BD/WEB/REMUX, resolución, códec, 10-bit, dual audio, multi-sub, batch, SeaDex best, fiabilidad mínima. El estado de filtros es local del picker (struct privado puro con tests en el mismo archivo); filtrar nunca reordena (el orden es el ranking). Claves `src.tag.*` y `src.filter.*`. E también rellena `tags` en `tk-app/src/live/sources.rs` (`source_vm`, mapeo `ReleaseTags` → `ReleaseTagsVm`) y quita del picker el aviso `src.m2`.

## 6. tk-ext: ruta `torrent` de extensiones custom (E)

- `candidates::candidate_routes(&Manifest, exposes_torrent_all: bool) -> Vec<&'static str>` (exportado) y `compat::exposes_torrent_all(bundle: &str) -> bool` (exportado). `ExtensionHost::resolve_candidates` **ya** consulta, por extensión, cada ruta que devuelve `candidate_routes` (fallos por ruta no rompen las demás). Stubs = comportamiento de la ola 1 (`torrent` para extensiones `torrent`, `stream` para el resto; `exposes_torrent_all` = `false`).
- E implementa: extensiones `custom`/`onlinestream` cuyo manifest lista `torrent` **explícitamente** en `apiServer.routes` (la lista por defecto no cuenta) o cuyo bundle expone `torrentAll` → `["stream", "torrent"]`. Toko lo cumple (`apiServer.routes` incluye `torrent`; proveedores en `src/providers/torrent/{nyaa,animetosho,subplease,seadex,nekobt,aniliberty}.ts`).
- `map_candidates`: revisar la forma real de las entradas torrent de Toko (campos de título / nombre del release, `infoHash`, `magnet`, `seeders`, `size`, marca SeaDex) y rellenar `tags = parse_release(<nombre>)` + `seadex_best`; label = grupo parseado si no viene `group`. Tests con un cuerpo de ejemplo con la forma de Toko.
- Actualizar el texto del log diferido (`torrent playback arrives in M2`) y el doc de `DEFERRED_PERMISSIONS`: `torrent:session` ya no es diferido para listar/reproducir en desktop (la sesión la da tk-torrent), sin cambiar el nivel de compatibilidad.

## 7. Android (F, solo documentación)

`docs/redesign/11-android-torrents-design.md`: pantallas (cliente con las mismas secciones adaptadas a bottom sheet / pestañas), flujos (reproducir torrent, siguiente episodio en batch, descargas en segundo plano con aviso), modelo compartido con desktop (mismos VMs/semántica de §5.2, `ReleaseTags`, `pick_episode_file` como algoritmo portable), opciones de motor (libtorrent4j vs librqbit vía UniFFI: tamaño, ABI, mantenimiento, streaming por HTTP loopback hacia Media3), servicio en primer plano + notificación de progreso, almacenamiento (SAF / app-specific), red medida / solo Wi-Fi, batería. Enmiendas en `00-decision-stack.md` y `04-implementation-plan.md` (torrents pasan de M2 a esta ola en desktop; Android después) y una sección corta para usuarios en `apps/desktop-native/README.md` (cómo reproducir torrents, dónde se guardan, aviso de descargas en segundo plano, puertos).

## 8. Propiedad de archivos

| Worker | Área (solo él edita) |
|---|---|
| **A** — motor | `crates/tk-torrent/**` (sesión librqbit, servidor HTTP con prioridad de piezas, persistencia, stats/peers/trackers/bitfield, eventos, tests), `tk-app/src/live/torrent.rs` (cuerpos), entrada librqbit en `apps/desktop-native/THIRD_PARTY_NOTICES.md`, en `tk-app/src/main.rs` solo una línea de apagado si la necesita. |
| **B** — player | `tk-app/src/live/torrent_play.rs`, `tk-ui/src/screens/player.rs`, `components/player_controls.rs`, `src/player_state.rs`, `css/player.css`, archivos privados nuevos (p. ej. `components/seek_marks.rs`). |
| **C** — cliente 1 | `tk-ui/src/screens/torrents/{layout,overview,files,peers,trackers}.rs`, `css/torrents.css`, archivos privados nuevos en `screens/torrents/` (p. ej. `format.rs`, `table.rs`; si D necesita un formateador, se lo hace él en privado). |
| **D** — cliente 2 | `tk-ui/src/screens/torrents/{library,downloads,settings}.rs`, `css/torrents_manage.css`, `tk-app/src/live/torrent_settings.rs`. |
| **E** — fuentes y etiquetas | `tk-core/src/release.rs` (+ tests), `tk-core/src/sources.rs` (ranking), `tk-ext/**` (`candidates.rs`, `compat.rs`, `host.rs`, tests), `tk-ui/src/components/sources.rs`, `css/sources.css`, `tk-app/src/live/sources.rs`. |
| **F** — Android + docs | `docs/redesign/11-android-torrents-design.md` (nuevo), `docs/redesign/00-decision-stack.md`, `docs/redesign/04-implementation-plan.md`, `apps/desktop-native/README.md`, `docs/redesign/KNOWN-ISSUES.md` (entradas de torrents). |

Nadie fuera del integrador edita `tk-ui/src/backend.rs`, `tk-ui/src/model.rs`, `tk-ui/src/app.rs`, `tk-ui/src/lib.rs`, `tk-ui/src/shortcuts.rs`, `tk-ui/src/components/nav_rail.rs`, `tk-ui/src/screens/torrents/mod.rs`, `tk-app/src/live/mod.rs`, `tk-app/src/paths.rs` (quedan como los deja este commit). Si alguien necesita un cambio ahí, lo describe en su commit y lo hace el integrador.

## 9. Archivos compartidos: solo ediciones aditivas

`tk-ui/i18n/en.json` y `es.json` (bloque propio al final, antes de `}`; el integrador resuelve las comas), `tk-ui/src/components/mod.rs`, `tk-ui/src/screens/mod.rs`, `tk-ui/src/icons.rs` (glifos nuevos + su nombre en el test), las líneas de `[workspace.dependencies]` de `apps/desktop-native/Cargo.toml` y de los `Cargo.toml` de cada crate. Dependencias nuevas: línea nueva en el workspace + en el crate, listadas en el mensaje del commit; nada que traiga aws-lc / CMake / NASM.

Prefijos i18n: C `torrent.overview.*`, `torrent.files.*`, `torrent.peers.*` (incl. `torrent.peers.flag.<letra>`), `torrent.trackers.*`, `torrent.shell.*`, `torrent.fmt.*`; D `torrent.library.*`, `torrent.downloads.*`, `torrent.settings.*`; B `player.torrent.*`; E `src.tag.*`, `src.filter.*`. Ya existen (este commit; los recorre el test de claves dinámicas): `nav.torrents`, `torrent.title`, `torrent.placeholder`, `torrent.section.<id>`, `torrent.state.<id>`, `torrent.tracker.<id>`, `torrent.proto.<campo>`, `torrent.settings.after_watch.{keep,delete}`, `player.skip.{intro,outro,recap}`.

## 10. Integración

Orden sugerido: contrato → A (motor) → E (fuentes/etiquetas) → B (player) → C, D (pantallas) → F. Fricciones esperables: i18n (aditivo), `Cargo.lock` (solo A puede añadir deps; el lock de este commit ya incluye librqbit). Ningún worker compila; el integrador corre el build y un fixer corrige.

Dependencias nuevas de este commit: `librqbit =9.1.0` (default-features off), `librqbit-core =9.1.0` y `librqbit-sha1-wrapper =9.1.0` (feature `sha1-crypto-hash`), `reqwest-rqbit` (= `reqwest 0.13`, `native-tls`), y `tk-torrent` como crate interno (`tk-app`, feature `live`).
