# 14 — Plan de paridad: del Tatakai clásico a Tatakai Next (2 olas)

Objetivo: que Tatakai Next (desktop `apps/desktop-native` y Android `apps/android-native`) haga lo que hoy hace el Tatakai clásico (`src/`, `desktop/`, `server/`), para poder retirar el clásico (M4 de `04`). La dirección es una sola: se porta del clásico hacia Next, nunca al revés.

Punto de partida: la auditoría del 2026-11-08 (página "Paridad Tatakai Next", capturas de Tatakai 7.0.0 y de `tk-app` en `main` @ `1a12f42`, más un inventario de código de ambas apps). Allí se clasificó cada feature como **Falta**, **Stub** (el control existe pero no hace nada), **Parcial**, **Paridad**, **Next mejor** o **Fuera de plan**.

El trabajo se hace en **2 olas grandes de 10 programadores cada una**. 10 es el máximo de agentes simultáneos de un workflow en esta PC (`min(16, CPUs − 2)` con 12 núcleos), así que cada ola usa todo el paralelismo disponible. La ola B arranca sobre el código ya integrado y compilado de la ola A. Al final van una fase de testing y una re-revisión de paridad.

Precedencia: este documento no cambia `00`-`13`. Cada ola escribe su contrato (`15-parity-wave-a-contract.md`, `16-parity-wave-b-contract.md`) y ese contrato manda sobre este plan donde difieran.

## 0. Decisiones del usuario (2026-11-08/09; prevalecen)

1. **2 olas de 10 programadores** (7 desktop + 3 Android cada una), 20 en total. 10 es el máximo de agentes simultáneos en esta PC.
2. Alcance: todo el núcleo (stubs, reproductor, listas, descargas, extensiones, descubrir) **más** manga y novelas, social, servidor nativo de extensiones en Rust y traducción de subtítulos.
3. Desktop y Android en la misma ola.
4. Un integrador y un **auditor de completitud** separado en cada ola.
5. **Una sola compilación para todo el plan**, al final de la ola B, y una segunda solo si la primera falla. Ningún agente compila antes: ni el contrato, ni los programadores, ni el integrador, ni la ronda de cierre. El fixer sí puede usar `cargo check -p` y `compileDebugKotlin` incrementales mientras corrige; eso no cuenta como compilación extra.
6. Al final, testing y re-revisión de paridad sobre el binario compilado.
7. Modelos: contrato **Fable 5.1 high**; programadores **Opus 5.5 medium**; integrador, auditor y fixer **Opus 5.5 high**; build y re-revisión **Sonnet 5.5**.
8. **2 workflows por ola**, cortando después del contrato: el contrato se revisa (yo y el usuario) antes de lanzar a los programadores.

### 0.1 Enmiendas del 2026-11-09 (prevalecen sobre el resto de este documento)

9. **Sin login propio.** Se elimina el login de Supabase de Next (desktop y Android): la cuenta es **AniList y/o MAL** (OAuth). Listas, progreso por episodio, favoritos y puntuación viven en AniList/MAL (**g**/**q**). Antes de quitar el código de Supabase se integra lo que daba (ver `analyze-before-removing`): el historial con la **posición exacta para retomar** queda local y se **sincroniza entre tus dispositivos por Tatakai Link** (la PC sirve una ruta de historial por mTLS; el teléfono la consume). En la ola A esto lo hacen **f** (desktop: quitar Supabase, ruta de historial en `tk-link`) y **q** (Android: quitar Supabase y `checkTatakaiConfig`, cliente de historial por Link). La configuración `TATAKAI_SUPABASE_*` desaparece.
10. Por lo anterior quedan **fuera**: registro, verificación por código, recuperar contraseña, cambio de email/contraseña, sesiones, sincronizar ajustes en la nube, y lo social que necesitaba backend propio (comentarios por episodio, salas para ver juntos, perfiles propios). **Lo social se reduce a lo que ofrece AniList** (perfil, seguidores/seguidos, actividad, notificaciones de AniList): sigue siendo **n** (desktop) y **t** (Android) en la ola B. Los ids OAuth (`TATAKAI_ANILIST_CLIENT_ID`, `TATAKAI_MAL_CLIENT_ID`; uno por plataforma porque AniList admite una sola redirección por cliente) los registra el usuario; sin ellos, "Conectar" muestra un error claro.
11. Entran en la ola A: asistente de primer arranque y "restablecer la app" (**f**); abrir enlaces `magnet:` desde el sistema, historial de sesiones de torrent y estado de trackers (**i**). **Torrents y debrid de Android pasan a q en la ola A** (antes r, ola B).
12. Android: reproducción en segundo plano con **Media3 y libmpv** (**p**). Para libmpv, referencia: `Enigmata/app/src/main/java/app/enigmata/ui/player/MediaPlaybackService.kt` (servicio con `MediaSessionCompat` que observa el core de mpv); copiar está permitido conservando el aviso y anotándolo en `THIRD_PARTY_NOTICES.md`.
13. Ola B: **sniffer de fuentes embed** en la WebView oculta (abre el embed, captura la URL m3u8/mp4 y la reproduce con mpv) en **k** (desktop) y **r** (Android); **MAL, Shikimori y Tenrai** en el router de metadata (**b**; si un proveedor ya no responde, se descarta con nota); **KittenTTS** local opcional (**m**).
14. Quedan **fuera**: cloudflared, WARP, y los extras sociales del clásico (encuestas, GIFs, salas).

## 1. Fases

"‖" = en paralelo.

| Fase | Ola | Modelo | Agentes | Qué hace |
|---|---|---|---|---|
| **F0 Contrato** | A y B | Fable high | 3: desktop ‖ Android ‖ checklist (en B, la checklist ya existe y solo se amplía) | Escriben el contrato de la ola y dejan en el árbol todas las firmas compartidas como stubs (§3), sin compilar (`cargo metadata` sí, para resolver dependencias nuevas). No commitean: el commit lo hago yo al revisarlo. |
| *Revisión del contrato* | A y B | — | yo + usuario | Fin del workflow 1 de la ola. |
| **F1 Programadores** | A y B | Opus medium | 10 en paralelo | Cada uno implementa su área (§2) en su worktree, archivos exclusivos; marca en la checklist los IDs que cerró. Commit en su rama, sin push. |
| **F2 Integración** | A y B | Opus high | 1 | Mergea las 10 ramas, reconcilia, agrega dependencias de los `NOTES-deps.md`, regenera tokens si cambiaron. |
| **F4 Auditoría** | A y B | Opus high | 1 | Recorre los IDs de la ola en la checklist contra el código integrado; descarta stubs (`NotWired`, `TODO`, botones deshabilitados, "Próximamente"/"M1") y lista los pendientes por área. No corrige. |
| **F5 Ronda de cierre** | A y B | Opus medium + integrador Opus high | hasta 6 ‖ + 1 | Cada pendiente vuelve al área que lo dejó; se integra igual que F2. Si F4 no encontró pendientes, se salta. |
| **F6 Tests** | solo B | Opus medium | 2: desktop ‖ Android | Escriben los tests unitarios que falten para la lógica nueva de ambas olas. No los corren. |
| **F7 Build final** | solo B | Sonnet | 2: desktop ‖ Android | `cargo build -p tk-app` + `cargo test --workspace` ‖ `./gradlew assembleDebug testDebugUnitTest`. Errores literales. |
| **F7b Fixer + build 2**, si falla | solo B | Opus high; build Sonnet | 0-2 + 0-2 | Uno por plataforma rota: corrige con checks incrementales y luego va el segundo y último build. Si sigue rojo, se detiene y reporta. |
| **F8 Re-revisión de paridad** | solo B | Sonnet | 1 | Humo e2e y capturas con el arnés (§5) del clásico y de Next, regenera la página de paridad y deja en `KNOWN-ISSUES.md` lo que siga faltando. |

Ola A: 3 + 10 + 1 + 1 + hasta 7 = **15 a 22 agentes**. Ola B: lo mismo más 2 + 2 + hasta 4 + 1 = **20 a 31**. Total **~35 a 53**.

### Workflows

- 2 por ola: **(1)** contrato; **(2)** programadores → integración → auditoría → cierre (y en la ola B además tests → build final → fixer → re-revisión). 4 en total.
- Superan la guía de tamaño de la sesión (menos de 10 agentes por workflow) porque el usuario lo pidió así; no hace falta cambiar la configuración. El tope duro de 10 agentes simultáneos se respeta solo.
- Entre la ola A y la ola B **no hay binario** (no se compila): la revisión intermedia es de código y del contrato de B, no de la app.
- Si un workflow se corta (red, límite de tasa), se reanuda con `resumeFromRunId`: las fases ya terminadas vuelven del caché.

### Requisitos para correrlo

- 10 agentes Opus a la vez es un pico grande de tokens y puede chocar con límites de tasa. El script debe tolerar que un agente falle por red: antes de relanzarlo se mira el disco, porque el trabajo puede estar ya commiteado.
- Disco: solo se compila una vez, en la rama de la ola B (el debug actual de `tk-app` ocupa 4,3 GB, más el `build/` de Gradle). Hay 192 GB libres en DATOS.
- La parte social (ola B) necesita tablas nuevas en Supabase. El contrato escribe las migraciones en `contracts/supabase/`; **aplicarlas al proyecto de Supabase lo hace el usuario** (o lo autoriza explícitamente).

### Ramas

- Ola A: `parity/wave-a` desde `main`. Se integra y audita ahí, sin compilar.
- Ola B: `parity/wave-b` desde `parity/wave-a` ya cerrada. El build final corre aquí.
- F6 a F8 trabajan sobre `parity/wave-b`. Nada se pushea ni se mergea a `main` sin que el usuario lo pida.

## 2. Reparto

Rutas orientativas; el contrato de cada ola fija las exactas. Desktop: `tk-ui` = `apps/desktop-native/crates/tk-ui/src`, `live` = `tk-app/src/live`. Android: rutas bajo `apps/android-native/`. Cada programador es dueño también de su namespace de i18n y de su sección de Ajustes.

### Ola A — núcleo de anime y cuentas

Lo que la ola B necesita ya resuelto: sesión, listas, ficha y reproductor.

| W | Área | Alcance | Dueño de |
|---|---|---|---|
| a | Buscar | Filtros reales en la query de AniList (género, temporada, estado, formato, tags, rangos, orden, 18+), paginación, búsqueda dentro de una extensión, "Ver todo" con el feed de la fila, búsqueda por imagen (trace.moe), por estudio, páginas de personaje | `screens/search.rs`, `screens/character.rs`, `live/search.rs`, `tk-core/metadata/anilist/search.rs`, `tk-core/tracemoe.rs` |
| c | Ficha | Relacionados, personajes y staff, recomendaciones, temporadas y orden de visionado, próximo episodio, ficha técnica completa, tráiler; monta el botón de lista de **g** y deja puntos de montaje vacíos para descargar temporada y comentarios (ola B) | `screens/details.rs`, `tk-core/metadata/anilist/details.rs`, `live/details.rs` |
| d | Subtítulos | Respetar el idioma preferido (bug de la auditoría), estilo vía `sub-*` de mpv, desfase, cargar SRT/ASS, subtítulo secundario, traducción en vivo y precargada (Google, DeepL/DeepLX, Azure, OpenAI-compatible), captura de pantalla | `tk-player/src/subs*.rs`, `screens/player/subs.rs`, `tk-core/translate/**`, `live/subs.rs` |
| e | Flujo del reproductor | Episodio anterior, auto-saltar intro y auto-siguiente con cuenta configurable, temporizador de sueño, bucle, volumen estable, atajos reasignables, Discord Rich Presence; la fuente recordada no debe ganar a las preferencias (bug de la auditoría) | `screens/player/{mod,flow}.rs`, `components/player_controls.rs`, `shortcuts.rs`, `tk-app/src/discord.rs` |
| f | Cuenta y sistema | Sesión de Supabase persistente, registro, verificación por código, recuperar contraseña, Limpiar caché, Abrir carpeta, Exportar historial, Abrir logs, Licencias, iniciar con el sistema, bandeja, aviso de versión nueva | `tk-core/supabase/**`, `screens/settings/{account,data,about}.rs`, `tk-app/src/{tray,autostart,update_check}.rs` |
| g | Listas | OAuth de AniList y MAL por loopback, lectura y escritura, cola offline, conflictos, importar/exportar, progreso automático al terminar un episodio, Biblioteca por estados, Favoritos, sección Cuentas | `tk-core/lists/**`, `screens/library.rs`, `components/list_button.rs`, `screens/settings/accounts.rs`, `live/lists.rs` |
| i | Torrents y debrid | Magnet o `.torrent` a mano con alineación de episodios, trackers propios, horario de ancho de banda, límite de caché, precarga del siguiente episodio; Real-Debrid, TorBox, AllDebrid y Premiumize como fuentes | `tk-torrent/**`, `screens/torrents/**`, `tk-core/debrid/**`, `live/torrent*.rs` |
| o | Android: descubrir y ficha | Lo de **a** y **c** en Android | `feature/{search,details}/**`, `core/metadata/**` |
| p | Android: reproductor | Lo de **d** y **e**; además pantalla completa y reproducción en segundo plano con `MediaSessionService` | `core/player/**`, `core/player-mpv/**`, `feature/player/**` |
| q | Android: cuenta, listas y sistema | Lo de **f** y **g** (sin bandeja ni autostart), Abrir logs | `core/supabase/**`, `core/data/**`, `feature/{library,settings,you}/**` |

### Ola B — lo que se apoya en la ola A

| W | Área | Alcance | Dueño de |
|---|---|---|---|
| b | Inicio y descubrir | Últimos episodios, Top 10, Próximos estrenos, Más favoritos, Porque viste X, nube de géneros, filas infinitas, calendario de emisión con recordatorios de las series que sigues (dentro de Inicio/Biblioteca), recomendaciones con perfil de gustos, más paletas | `screens/home.rs`, `components/airing.rs`, `tk-core/recs.rs`, `live/home.rs`, `packages/tokens/**` |
| h | Descargas | Motor HLS/MP4 con reanudación y cola, página Descargas (une torrents y web), modal descargar temporada (en el punto de montaje que dejó **c**), auto-descarga de nuevos episodios de la lista "Viendo", biblioteca local (importar, escanear carpeta, emparejar, reproducir sin conexión) | `tk-core/download/**`, `screens/downloads.rs`, `components/download_season.rs`, `live/downloads.rs` |
| j | Extensiones | Cliente de servidor de extensiones, addons de Stremio, actualizaciones, preferencias por extensión, repositorios por URL, proveedores personalizados (filas en Inicio y páginas por proveedor) | `tk-ext/**`, `screens/extensions.rs`, `screens/provider.rs`, `live/ext.rs`, `tk-core/extserver_client.rs` |
| k | Servidor y red | Modo servidor nativo (crate nueva `tk-server`, axum) compatible con `/api/server/v1` de `server/` para que los teléfonos usen las extensiones de la PC, emparejamiento, FlareSolverr, proxy | `crates/tk-server/**`, `screens/link.rs`, `live/{browser,server}.rs` |
| l | Manga | Metadata (AniList, MangaDex, MangaUpdates), inicio, búsqueda y ficha de manga, capítulos por extensión, lector paginado/vertical/webtoon con sus ajustes, descarga de capítulos | `tk-core/manga/**`, `screens/manga/**`, `components/reader/**`, `live/manga.rs` |
| m | Novelas | Inicio, ficha y capítulos de novelas, lector con tipografía y ajustes, narración TTS (voz del sistema, OpenAI, ElevenLabs) | `tk-core/novel/**`, `screens/novel/**`, `tk-app/src/tts.rs`, `live/novel.rs` |
| n | Social | Perfil, seguir, notificaciones, comentarios por episodio y por anime (en el punto de montaje que dejó **c**), ver juntos (salas, chat, cola, sincronía de reproducción) sobre Supabase Realtime, migraciones | `tk-core/social/**`, `screens/{profile,room,notifications}.rs`, `components/comments.rs`, `live/social.rs`, `contracts/supabase/migrations/**` |
| r | Android: descargas, extensiones e Inicio | Lo de **b**, **h** y **j** (descargas HLS con el `DownloadManager` de Media3, auto-descarga con WorkManager) | `feature/home/**`, `core/torrent/**`, `core/download/**` (nuevo), `core/extensions*/**`, `core/extension-server/**`, `feature/{torrents,extensions}/**` |
| s | Android: manga y novelas | Lo de **l** y **m** | `core/manga/**`, `core/novel/**`, `feature/{manga,novel}/**` (nuevos) |
| t | Android: social | Lo de **n** | `core/social/**`, `feature/social/**` (nuevos) |

`app/**` de Android (shell, rutas, manifest) queda en manos del contrato de cada ola; si un programador necesita algo ahí, lo pide en su commit y lo hace el integrador.

## 3. Puntos calientes que el contrato debe resolver

Archivos que varias áreas tocarían a la vez. El contrato de cada ola los deja preparados para que cada programador solo edite lo suyo. Los cambios estructurales (partir archivos, el i18n por namespace) los hace el contrato de la **ola A** y la ola B ya los encuentra hechos.

**Desktop**
- `tk-ui/src/backend.rs` (trait `Backend`): todos los métodos nuevos de la ola **con implementación por defecto `NotWired`**, y los VM nuevos en `tk-ui/src/model.rs` (o en `model/<área>.rs` si el contrato lo parte).
- `tk-app/src/live/mod.rs` (`impl Backend for LiveBackend`, un solo bloque): una línea por método que delega a `live/<área>.rs`, con stub. Nadie edita `mod.rs` después del contrato.
- `tk-ui/src/app.rs` (`Route`) y `components/nav_rail.rs`: todas las rutas nuevas de la ola con pantalla placeholder.
- `tk-ui/i18n/{en,es}.json` (ola A): se parten en `i18n/{en,es}/<namespace>.json` y `i18n.rs` los fusiona en compilación. Un namespace por programador.
- Ola A: `screens/player.rs` (1.938 líneas) → `screens/player/{mod,subs,flow}.rs`; `screens/settings.rs` → `screens/settings/<sección>.rs`; `tk-core/metadata/anilist.rs` → `anilist/{search,details,lists}.rs`.
- Componentes que un programador crea y otro monta (botón de lista, descargar temporada, comentarios): el contrato define la firma y deja un stub o un punto de montaje vacío; el dueño lo implementa.
- `tk-core/src/store.rs`: todas las tablas nuevas de la ola como `CREATE TABLE IF NOT EXISTS`.
- `Cargo.toml` / `Cargo.lock`: el contrato agrega por adelantado las crates previstas para su ola (ola A: bandeja, autostart, Discord RPC, OAuth; ola B: TTS, axum para `tk-server`) y verifica que resuelven. Lo que aparezca después va a `NOTES-deps.md`.

**Android**
- `core/model`: solo el contrato agrega tipos; lo demás se crea privado en el módulo.
- `app/.../nav/Routes.kt`, `TatakaiApp.kt`, manifest: rutas y destinos nuevos en el contrato.
- `settings.gradle.kts` y catálogo: los módulos nuevos de la ola B (`core/download`, `core/manga`, `core/novel`, `core/social`, `feature/manga`, `feature/novel`, `feature/social`) los crea el contrato de la ola B; dependencias nuevas por `NOTES-deps.md`.

**Compartido**
- `packages/tokens`: dueño **b** (ola B); el integrador corre `node packages/tokens/generate.mjs` y verifica que `Tokens.kt` y `tokens.css` queden regenerados.
- `contracts/supabase/migrations`: dueño **n** (Android **t** solo lee).

## 4. Checklist de paridad

El contrato de la ola A la escribe completa en `contracts/parity/checklist.json`, a partir de la tabla de la auditoría y del inventario del clásico. Cada entrada:

```json
{ "id": "player.subs.preferred-lang", "wave": "A", "area": "d", "android": "p",
  "classic": "src/components/video/SubtitleSelector.tsx",
  "accept": "Con preferencia ES, un stream con pista ES arranca con esa pista activa",
  "status": "todo" }
```

- Los programadores pasan `status` a `done` en los IDs que cierran (solo los suyos).
- El auditor (F4) de cada ola no confía en ese campo: verifica cada ID de su ola en el código y lo marca `verified`, `stub` o `missing` con la ruta que encontró.
- El contrato de la ola B puede agregar IDs que se descubran al trabajar la ola A, sin borrar ninguno.
- La re-revisión (F8) usa el campo `accept` para elegir qué capturar y qué comprobar.

## 5. Arnés de capturas

La auditoría usó dos scripts que el contrato de la ola A versiona en `scripts/parity/`:
- `cdp.mjs`: maneja el clásico relanzado con `--remote-debugging-port`; navega por hash y captura páginas completas.
- `wk.mjs`: maneja Next lanzado con `WEBKIT_INSPECTOR_HTTP_SERVER=127.0.0.1:<port>` por el protocolo del inspector de WebKit (`Target.sendMessageToTarget`, `Runtime.evaluate`, `Page.snapshotRect`); hace clic por texto y expande los contenedores con scroll para capturar la página entera.

El video de mpv no sale en `Page.snapshotRect` (va debajo del web view); el reproductor se captura desde el escritorio. El audio se silencia a nivel PulseAudio antes de reproducir. En Android, capturas por `adb exec-out screencap`.

## 6. Riesgos

| Riesgo | Mitigación |
|---|---|
| El contrato no anticipa una firma y dos programadores inventan la misma cosa distinta | Contrato en Fable high, revisado antes de lanzar a los programadores; el integrador unifica y lo anota |
| Errores de compilación acumulados de 20 ramas que nunca compilaron | Propiedad exclusiva de archivos, `mod.rs` y rutas cerrados en el contrato; el fixer compila incremental; un segundo build. Si aun así queda rojo, se detiene y reporta |
| Trabajo a medias que parece terminado | Auditor separado en cada ola que no confía en lo que dicen los programadores; ronda de cierre F5 |
| La ola B se escribe sobre código de la ola A que nunca compiló | Riesgo aceptado (decisión 5); el contrato de B lee el código de A y no solo su contrato |
| Límites de tasa o caída de agentes por red | El script reintenta revisando primero el disco |
| Social sin tablas en Supabase | Las migraciones quedan escritas; sin aplicarlas, la UI social muestra un error claro en vez de fallar en silencio |
| Costo | ~35-53 agentes, la mayoría Opus |

## 7. Decisiones que quedan con valor por defecto

Si el usuario no dice otra cosa, los contratos usan estas:

| # | Pregunta | Valor por defecto |
|---|---|---|
| D4 | Ecosistemas que no son `.kai` | Stremio nativo. Aniyomi/Mihon/CloudStream/Sora/Mangayomi a través del servidor de extensiones (Next como cliente) |
| D5 | Japonés como tercer idioma | No; `06` fijó en + es |
| D6 | Pantallas que `04` fusionó | Se mantiene la fusión: calendario en Inicio/Biblioteca, géneros/tendencias/colecciones como feeds de Buscar, favoritos en listas, biblioteca offline en Descargas. Playlists y tier lists fuera |
| D7 | Yomi (traductor de manga), Wrapped, comunidad/foros, panel de admin, modo TV/IPTV | Fuera de este plan |
