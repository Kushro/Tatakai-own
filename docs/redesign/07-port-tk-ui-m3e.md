# 07 — Plan: llevar el prototipo "Tatakai Next" (M3 Expressive) a código en `tk-ui`

Rama base: `redesign/m3e-native` (rama de trabajo propuesta: `m3e/tk-ui-expressive`, mergeada después sobre `redesign/m3e-native`).
Prototipo de referencia: artifact "Tatakai Next" (https://claude.ai/artifact/4Mhp3SDMgUXMGkmb5sGCiy). Sus archivos (`styles.css`, `ui.js`, `sources.js`, `screens.js`, `android.js`, `player.js`, `app.js`, `i18n.js`) son la especificación visual y de flujo; **no** se copian al repo, se traducen a Dioxus + CSS con los tokens generados.

Alcance: **solo desktop (`apps/desktop-native`)**. La paridad Android (Compose) se deriva del mismo prototipo pero es otro plan (`apps/android-native`, ver §8).

## 1. Qué cambia respecto a tk-ui actual

| Área | tk-ui hoy (M0) | Objetivo |
|---|---|---|
| Color | `tokens.css` generado (48 roles × paleta × modo × contraste) | **Se conserva tal cual.** El prototipo deriva colores con `oklch()` solo para no depender del generador; en código se usan `--md-sys-color-*`. |
| Shell | title bar + rail 80/256 | Igual + chips de estado en title bar (offline, `Link :puerto`), FAB de búsqueda en el rail, atajos `1..6`, `?` |
| Navegación | 5 destinos | 6: **Inicio · Buscar · Biblioteca · Extensiones · Enlace · Ajustes** (02-ux-flows §2; Descargas queda M1) |
| Inicio | hero + continuar + filas | hero multi-browse con autoplay/dots, filas con error inline por fila, fila `customHome` perezosa (IntersectionObserver → `use_effect` + `onvisible`) |
| Buscar | lista + detalle | + historial, chips tipo/origen (Manga/Novela deshabilitados "M1"), **side sheet de filtros**, contador de resultados |
| Detalles | header + episodios | + chips de metadatos, split button Ver/Continuar ▾, pestañas Episodios/Información/Relacionados, pills Visto/Relleno/Resumen, **panel de fuentes** (§3), enviar a dispositivo, floating toolbar sticky |
| Player | controles básicos + preset menu | + resolución con failover visible, marcadores intro/outro + "Saltar intro", preview al hacer hover en seek, menús subs/audio/calidad/velocidad/suavizado, PiP, captura, next-episode con cuenta atrás, chip de fuente clicable → selector |
| Biblioteca | — | **nueva**: Continuar viendo (grid) + Historial local, botón "Sincronizar ahora" |
| Extensiones | lista + instalar por ruta | + dropzone de archivo, pill de compatibilidad con banner explicativo, permisos como chips con icono, hash/dominios/fecha, **registro por extensión** plegable |
| Enlace | QR + código + dispositivos + firewall | + **tarjeta de estado con error legible** (os error 10048 → causa + "Reintentar" + "Reiniciar en otro puerto"), pasos 1-2-3, capacidades de la PC, renombrar dispositivo, pill "Conectado ahora"/"Sesión activa" |
| Ajustes | paneles apilados | **índice lateral sticky** + secciones: Apariencia (+reducir movimiento), Reproductor (+**selección de fuente** §3, decodificador, auto-next, skip, estilo de subtítulos), Metadatos (+caché), Cuenta, Link (+puertos), **Datos y almacenamiento**, Acerca de (+versiones, licencias, logs, actualizaciones) |

## 2. Componentes nuevos o ampliados en `tk-ui/src/components/`

| Archivo | Cambio |
|---|---|
| `button.rs` | tamaños `Xs..Xl`, variantes `Elevated`/`Danger`, **shape morph** al presionar (`border-radius` full → medium, ya en CSS), `SplitButton`, `IconButton { on: bool }` |
| `chips.rs` | `Chip { kind: Filter/Assist/Info, on, icon, dismiss }`, `Pill { tone: Ok/Warn/Err/Neutral/Primary, plain }` |
| `segmented.rs` (nuevo) | botón-grupo conectado (`.seg`), con `small`, `disabled_keys` |
| `form.rs` | `TextField { icon, hint, error, mono }`, `Switch` con thumb que crece al presionar |
| `nav_rail.rs` | FAB de búsqueda, badge en destino, pie con atajos, `data-expanded` ya existe |
| `title_bar.rs` | chips de estado (offline, Link) |
| `media.rs` | `MediaCover { badge, score, progress, ep }` 13:18 con monograma, `MediaCard { wide }`, `ContinueCard`, `EpisodeRow { thumb, pills, duration }`, `HeroCarousel { autoplay, dots }` |
| `loading.rs` | `LoadingIndicator` con morph de 7 formas via `<animate attributeName="d">` (48 puntos por forma, ver `ui.js::blob`) + variante `contained` |
| `progress.rs` | `WavyProgress { fraction, active, thin }` con fase animada solo si `!reduce_motion` |
| `state.rs` | `EmptyState`, `ErrorState { title, cause, detail(plegable, `<code>` seleccionable), actions }`, `SkeletonRow` |
| `sheet.rs` (nuevo) | side sheet (bottom sheet si contenedor < 600px) con scrim, Esc, foco |
| `dialog.rs` | `ConfirmDialog { icon, danger, wide, content }` |
| `snackbar.rs` (nuevo) | host global con acción y auto-cierre |
| `menu.rs` (nuevo) | menú anclado, `menuitemradio`, atajo a la derecha, cierre por click fuera/Esc |
| `floating_toolbar.rs` (nuevo) | toolbar sticky inferior de Detalles (M3E `HorizontalFloatingToolbar`) |
| `source_row.rs` (nuevo) | fila de fuente (§3) |

CSS: `app.css` se reescribe a partir de `styles.css` del prototipo **sustituyendo** cada `--primary/--sc-*/…` por su `--md-sys-color-*`, cada `--sh-*` por `--md-sys-shape-corner-*`, cada `--mo-*` por `--md-sys-motion-*` y las fuentes por `--tk-font-*`. Los `@container frame (...)` pasan a `@media` (desktop no necesita marco). Regla: **ningún literal de color** fuera de `tokens.css`; los únicos literales permitidos son layout (`--tk-titlebar-h`, `--tk-rail-w`, `--tk-drawer-w`).

## 3. Modelo de fuentes (web + torrent) y selección automática/manual

Este es el cambio funcional más grande y toca `tk-core`, `tk-ext`, `tk-app` y `tk-ui`.

### 3.1 Modelo (`tk-core/src/model.rs`)
```rust
pub enum SourceKind { Web, Torrent }
pub struct StreamSource {
    pub ext_id: String, pub ext_name: String, pub kind: SourceKind,
    pub label: String,            // servidor ("Alpha", "Ok.ru") o grupo ("SubsPlease")
    pub quality: Quality,         // P480 | P720 | P1080 | P1440 | P2160 | Unknown
    pub container: Option<String>,// "HLS" | "MP4" | "HEVC 10-bit" …
    pub subs: Vec<Lang>, pub dubs: Vec<Lang>,
    pub reliability: f32,         // 0..1 (ver 3.3)
    pub note: Option<String>,     // "dub latino", "multi-sub", "mini"
    pub web: Option<WebInfo>,     // { url, headers, latency_ms }
    pub torrent: Option<TorrentInfo>, // { info_hash, size_bytes, seeders, leechers, magnet }
}
pub struct SourcePrefs { pub mode: SourceMode /*Auto|Manual*/, pub prefer: Option<SourceKind>, pub min_quality: Quality, pub sub_lang: Lang, pub dub_lang: Lang }
```
Persistencia: `SourcePrefs` en `settings_store.rs` (config.json) con valores por defecto `Auto / any / 720p / es / ja`. Elección manual por episodio en `LocalStore` (`picked_source(media_id, ep)`), expira al terminar el episodio.

### 3.2 Host de extensiones (`tk-ext`)
- `sources/stream` ya devuelve una lista; se **amplía el contrato** (`contracts/extensions/`) para que cada entrada pueda traer `server`, `quality`, `subs`, `dubs`, `note`. Compat: campos ausentes → `Unknown`/vacío, nunca error.
- Nueva capacidad `torrent:session` en el manifest (`compat.rs`): en M0 desktop se acepta el manifest pero el motor libtorrent es **M2**; las fuentes torrent se listan (con seeders/leechers del index) y al reproducir se muestra el error `src.m2` con la alternativa web. Así el selector ya enseña el modelo completo sin fingir que reproduce.

### 3.3 Ranking automático (`tk-core/src/sources.rs`, nuevo; es la función `score` de `sources.js`)
`reliability*50 ± 20 por tipo preferido + 6·min(calidad,3) − 25 si < mínima + 10 subs pref + 6 dub pref + (torrent: min(seeders,500)/50, −20 si < 20 seeders | web: −latencia/100) − 1000 si la extensión está desactivada`. Confiabilidad = media móvil (α = 0.2) de éxitos/fallos por `(ext, label)` guardada en `LocalStore`; valor inicial 0.8. Test unitario con las 11 fuentes de ejemplo del prototipo (`data.js::sourcesFor`) como fixture: el orden esperado está en el artifact.

### 3.4 UI
- `SourceRow`: icono por tipo, título `Ext · Servidor/Grupo`, pills `Webstream/Torrent`, `Recomendada`, `Ext desactivada`; metadatos clave-valor (web: servidor, calidad+contenedor, latencia, subs, dubs; torrent: grupo, calidad+códec, tamaño, seeders, leechers, subs, dubs, hash); barra de confiabilidad con tono ok/warn/err.
- `SourcePicker` (sheet): banner con modo actual y enlace a Ajustes, segmented Todas/Web/Torrent, chips con preferencias activas, lista ordenada por score, nota M2.
- Detalles: panel lateral `SourceSummary` = fila recomendada + "Elegida por el sistema" + "Ver todas las fuentes (n)"; el ▾ del split button abre el picker y, al elegir, navega al player con esa fuente.
- Player: chip de fuente en la barra superior abre el picker; en modo **Manual** el player no resuelve hasta que el usuario elige (botón central "Elige una fuente"); failover sigue la lista ordenada y registra el fallo en `failed[]` (visible en el ErrorState final).
- Ajustes › Reproductor: Selección de fuente (Auto/Manual), Tipo preferido, Calidad mínima, Subtítulos preferidos, Audio preferido.

## 4. Pantallas (`tk-ui/src/screens/`)

| Archivo | Trabajo |
|---|---|
| `home.rs` | hero carousel (`HeroCarousel`), `ContinueCard`, `SectionRow` con `ErrorState` inline + reintento por fila, fila lazy de extensión |
| `search.rs` | historial (LocalStore, 6 entradas), chips tipo/origen, `FilterSheet`, `data-dual` ya existe |
| `details.rs` | header nuevo, pestañas, rangos de 100, pills, `SourceSummary`, `SendToDevice` dialog, `FloatingToolbar` |
| `library.rs` (nuevo) | Continuar viendo + Historial desde `LocalStore` (`watch_history`) |
| `player.rs` + `player_controls.rs` | estados `Resolving/Buffering/Playing/Error/Manual`, marcadores (ani.zip/AniSkip: M1, dejar el hook), menús, `NextEpisodeCard` con cuenta atrás, chip de fuente |
| `extensions.rs` | dropzone (`ondrop` de Dioxus + ruta), `ExtensionRow` con log plegable (`tk-ext` ya tiene logs por extensión) |
| `link.rs` | `StatusCard` con `UiError` mapeado: `AddrInUse` → `link.err.port` + acción "Reiniciar en otro puerto" (cambia `link.port` y reinicia `tk-link`), pasos, capacidades (`GET /v1/capabilities` local), renombrar dispositivo (`tk-link::devices::rename`, nuevo) |
| `settings.rs` | índice lateral (anclas + `IntersectionObserver` → `onscroll` en Dioxus), secciones nuevas, `SourcePrefs` |

i18n: añadir a `tk-ui/i18n/{en,es}.json` todas las claves nuevas del prototipo (`src.*`, `lib.*`, `settings.data.*`, `settings.about.*`, `link.err.*`, `link.steps.*`, `link.caps.*`, `ext.logs` …). `i18n.rs` ya hace fallback a `en`; el test `check-i18n` debe cubrir ambos JSON.

## 5. Orden de trabajo y paralelismo (≤ 4 agentes Sonnet, Fable diseña/revisa)

| Ola | Tareas (independientes dentro de la ola) | Entrega |
|---|---|---|
| **0 · Base (Fable, 1 sesión)** | `app.css` traducido a tokens; `components/{segmented,sheet,snackbar,menu,floating_toolbar}.rs`; i18n completo | `cargo check -p tk-ui` verde; página de muestra (`screens/gallery.rs`, solo `debug_assertions`) con todos los componentes |
| **1 · 3 agentes en paralelo** | A: `home.rs` + `library.rs` + `media.rs` · B: `search.rs` + `details.rs` (sin fuentes) + `extensions.rs` · C: `tk-core/sources.rs` + modelo + prefs + contrato de extensiones + tests | cada uno su rama `m3e/tk-ui-<a|b|c>`; sin tocar archivos de otra ola |
| **2 · 2 agentes** | D: `link.rs` + `settings.rs` (incluye prefs de fuentes de C) · E: `player.rs` + `player_controls.rs` + `SourcePicker`/`SourceRow`/`SourceSummary` (usa C) | ídem |
| **3 · Verificación (Fable + 1 revisor Opus 4.8)** | `cargo clippy -p tk-ui -p tk-core -p tk-ext --all-targets`, `cargo test` (fixtures de fuentes), `dx serve` manual con `NullPlayer`, revisión adversarial contra este documento y el artifact | PR `m3e/tk-ui-expressive → redesign/m3e-native` |

Reglas para los agentes: un solo `cargo build` por ola (compartir `target/`), nunca `rm -rf` sobre variables, tests con `tempfile`, nada de literales de color, cada pantalla con sus tres estados (carga/vacío/error).

### 5.1 Ola 0 · entregado (rama `m3e/tk-ui-expressive`)

Verificado: `cargo check -p tk-ui --all-targets`, `cargo test -p tk-ui` (49 tests), `cargo clippy -p tk-ui --all-targets` y `cargo check -p tk-app`, todos en verde. Sin verificar todavía: la app en ejecución (queda para la ola 3 con `dx serve`).

**CSS por dueño.** `assets/app.css` contiene base, shell y componentes. Cada área tiene su archivo en `assets/css/`, cargado en orden desde `app.rs::APP_CSS`. Un agente solo edita el suyo:

| Archivo | Dueño |
|---|---|
| `css/media.css`, `css/home.css`, `css/library.css` | Ola 1 · A |
| `css/search.css`, `css/details.css`, `css/extensions.css` | Ola 1 · B |
| `css/link.css`, `css/settings.css` | Ola 2 · D |
| `css/player.css`, `css/sources.css` (ya trae `.tk-srow`/`.tk-slist` del prototipo) | Ola 2 · E |

Los estilos de pantalla actuales se movieron tal cual a esos archivos; cada agente los reescribe desde `styles.css` del prototipo. Ayudantes compuestos en `:root` de `app.css`: `--tk-mo-spatial(-fast|-slow)`, `--tk-mo-fx(-fast|-slow)`, `--tk-shadow-1/3`, `--tk-hover`.

**Componentes disponibles** (`tk_ui::components`):

| Componente | Notas |
|---|---|
| `Button` | variantes `Filled/Tonal/Elevated/Outlined/Text/Danger`, tamaños `Xs..Xl`, `icon`, `trailing_icon`, `block` |
| `SplitButton` | acción + chevron (`expanded`, `on_menu`); se envuelve en `Menu` |
| `IconButton` | `variant` (`Standard/Filled/Tonal/Outlined`), `size` (`Sm/Md/Lg`), `toggle` + `toggled` |
| `Segmented` | `small`, `disabled_keys`, flechas/Inicio/Fin mueven la selección |
| `Chip`, `InfoChip`, `Pill`, `Banner`, `Kbd`, `Tone` | `Tone::{Neutral, Ok, Warn, Err, Primary}` |
| `TextField`, `Switch` | campo con `icon`, `hint`, `error`, `mono`, `disabled` |
| `Menu`, `MenuItem`, `MenuSeparator` | anclado al `trigger`, `align`, `placement` (`Above` para el player), Esc/Tab/clic fuera cierran, flechas mueven el foco, `checked: Some(_)` = radio |
| `Sheet` | lateral (inferior < 600 px), `wide`, `footer`, Esc y scrim cierran |
| `ConfirmDialog` | `icon`, `danger` (por defecto `true`), `wide`, `children` para contenido |
| `use_snackbar()` | `.show(texto)`, `.show_action(texto, etiqueta, Callback)`; host único en el shell |
| `FloatingToolbar`, `ToolbarSeparator` | sticky inferior |
| `LoadingIndicator` | morph SMIL de 7 formas, `contained`, `small` |
| `WavyProgress` | `active` (onda animada), `thin`; id de clip único por instancia |
| `EmptyState`, `ErrorState` | `ErrorState` acepta `title`, `cause`, `retry: false` y acciones extra como `children`; el detalle técnico va plegado y seleccionable |
| `SkeletonRow`, `SkeletonGrid`, `SkeletonLines` | shimmer tras 150 ms |
| `NavRail`, `RailFootItem` | FAB de búsqueda, `badges`, pie |

**Shell.** Destino nuevo `NavDest::Library` con ruta `/library` (placeholder para A). Atajos `1..6` navegan fuera del reproductor; el puente JS ahora envía si el player está visible. Ruta `/gallery` con todos los componentes, accesible desde el pie del rail solo en debug. i18n: 390 claves en en/es, incluidas las del prototipo salvo las exclusivas de Android.

**Decisiones que afectan a las olas 1-2.**
1. Los tokens no tienen roles de éxito/aviso. "Ok" usa primary(-container) y "warn" tertiary(-container), también en la barra de confiabilidad de fuentes.
2. "Reducir movimiento" ya funciona en CSS con `:root[data-reduce-motion="true"]`. Falta el campo en `Settings` y que `platform::apply_theme` ponga el atributo (ola 2 · D).
3. Los chips de estado de la title bar (sin conexión, `Link :puerto`) tienen CSS (`.tk-titlebar__status`) pero no datos. Los conecta D junto con `link.rs`.
4. `SourceRow`, `SourcePicker` y `SourceSummary` quedan para E. tk-ui no enlaza tk-core, así que necesitan un view-model de fuente en `backend.rs` que C define junto al modelo.
5. El fondo blanco del QR (`css/link.css`) es el único literal de color permitido: el código necesita contraste fijo para escanearse.
6. La base no está formateada con `rustfmt`; no ejecutar `cargo fmt` sobre todo el crate para no mezclar reformateos con cambios.

## 6. Riesgos y decisiones abiertas

1. **Torrent en desktop es M2**: el selector lo muestra pero no reproduce. Si no se quiere enseñar algo que no funciona, ocultar las fuentes torrent detrás de `settings.experimental.torrent_sources` (por defecto off) hasta M2.
2. **`<animate>` SMIL en WebView2**: funciona; si se prefiere evitar SMIL, el `LoadingIndicator` cae a rotación + `clip-path` animado.
3. **Container queries**: en desktop no se necesitan; si más adelante tk-ui corre en ventanas estrechas (tablet Windows), convertir `@media` a `@container` como hace el prototipo.
4. **Renombrar dispositivos** requiere persistir `name` en `tk-link::devices` (hoy solo `device_id` + cert). Cambio pequeño, sin contrato nuevo.
5. **`Reiniciar en otro puerto`**: `tk-link` debe poder reiniciarse en caliente; hoy arranca al abrir la pantalla. Hace falta `LinkHost::restart(port)` en `tk-app/src/live/link.rs`.

## 7. Criterios de aceptación

- Todas las pantallas del artifact existen en tk-ui con los mismos textos (es/en) y estados.
- Cero colores literales en `app.css` (test: `grep -E '#[0-9a-f]{3,8}|rgb\(' app.css` solo devuelve `scrim`/sombras).
- Ranking de fuentes reproducible por test con el fixture del prototipo.
- Modo Manual: el player no inicia red hasta elegir fuente (test de `PlayerState`).
- `prefers-reduced-motion` y el ajuste "Reducir movimiento" desactivan autoplay del hero, fase del wavy y rebotes.
- Error 10048 en Link muestra causa, detalle plegable y las dos acciones; "Reiniciar en otro puerto" deja el servidor activo.

## 8. Nota Android (fuera de este plan)

El mismo artifact en modo *Teléfono/Tablet Android* fija: barra inferior **Inicio · Buscar · Biblioteca · Tú** (Tú agrupa Perfil, Historial, Descargas, Extensiones, Dispositivo enlazado, Ajustes), pantalla **Dispositivo enlazado** (cliente Link: estado de la PC, escanear QR / pegar código, perfil de mejora, olvidar), menú Mejorar con "Mejorar en PC", ajustes con Color dinámico y aviso de batería. Se implementa en `apps/android-native/feature/*` con Compose M3E reutilizando `core/designsystem`; el modelo de fuentes de §3 va a `core/model` y `core/metadata` para que ambas apps compartan el ranking.
