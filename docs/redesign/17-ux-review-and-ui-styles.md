# 17 — Revisión UX/UI (clásico · rediseño · Next) y propuesta de estilos de UI

Fecha: 2026-11-09. Estado: propuesta, nada implementado.

**Material usado**

- **Clásico**: capturas `classic-*.jpg` de la auditoría del 2026-11-08 (`parity/audit-2026-11-08.html`) y código en `src/` (`tailwind.config.ts`, `src/index.css`, `src/components/settings/MediaCardStyleSelector.tsx`, `src/components/settings/panels/MobileAppSettingsPanel.tsx`).
- **Rediseño** (prototipo "Tatakai Next", vanilla JS): `styles.css`, `screens.js`, `player.js`, `sources.js`, `app.js`.
- **Next hoy** (después de la ola B): `parity/wave-b/next/s01..s46*.png`, `parity/audit-wave-b.html`, el código de `tk-ui` (`app.rs`, `components/*`, `assets/app.css`, `assets/css/*`, `model/mod.rs`, `platform.rs`) y `packages/tokens`.

**Lo que se deja afuera**

- Los extras de la Ficha que fallan (Información, Relacionados, Personajes, `idMal: null`) son un bug de datos que se arregla aparte. No se cuentan como problema de diseño (`s08`, `s09`).
- El reproductor de Next no tiene capturas: el perfil de prueba no tenía extensiones. Lo que digo del reproductor sale del código (`player.css`, `screens/player/*`) y del prototipo.
- Las capturas `s*` son a 1280×800, con el perfil vacío.

---

## 1. Revisión por área

Leyenda: **C+** = lo que el clásico hace mejor · **R+** = lo que el rediseño hace mejor · **N−** = dónde Next hoy queda por debajo de los dos.

### 1.1 Shell y navegación

- **C+** Rail fino, solo íconos, con el logo arriba. Los destinos van en grupos separados por divisores (contenido / comunidad / descargas) y cuenta y ajustes van abajo (`classic-03`, `classic-11`). Entra entero en 720 px de alto y nunca trunca texto.
- **R+** Rail M3 con FAB de búsqueda (Ctrl+K), etiquetas, drawer con Ctrl+B y badges. La title bar propia lleva pills de estado (offline, `Link :puerto`). Son **6 destinos** (`app.js` `DESTS_DESKTOP`) y en teléfono hay un "Tú" que agrupa Extensiones, Enlace y Ajustes.
- **N−**
  - Hay **10 destinos**, más el FAB, más el toggle, más el pie (`NavDest::ALL`). A 800 px de alto el rail **desborda**: en `s03` "Ajustes" queda cortado abajo y en `s40` el FAB queda cortado arriba, porque el rail scrollea.
  - Las etiquetas se truncan ("Bibliot…", "Descar…", "Extensi…" en `s03`). Un rail con etiquetas truncadas es peor que uno solo de íconos.
  - Los íconos se pisan con otras acciones. Novelas usa `bookmark`, el mismo ícono de "Añadir a lista" (`s07`). Manga usa la grilla, el mismo ícono del pie del rail (galería, `s40`).
  - Hay **dos hamburguesas**: la de la title bar y la del rail (`s03`).
  - No hay entrada de cuenta en el shell. El clásico tenía "entrar" en el pie y el prototipo tenía "Tú". Hoy el perfil de AniList queda escondido como chip dentro de Biblioteca (`s19`, `s40`).

### 1.2 Inicio (y Calendario, Recomendaciones)

- **C+**
  - Mucha densidad editorial: banners de "Descubrí tu próxima serie" y bloques "Último manga/novela leído" con CTA (`classic-01`).
  - El Top 10 va en grilla con rango Día/Semana/Mes, y hay una "Explorar géneros" con tiles.
  - El calendario tiene columnas altas con portada grande, título en 2 líneas, chip "HOY" y badge "SIGUIENTE" (`classic-16`).
- **R+**
  - El saludo y un hero multi-browse (el slide activo es ancho y los siguientes son pósters) con autoplay que respeta reduce-motion.
  - "Continuar viendo" va como tarjeta horizontal con barra.
  - Cada fila degrada sola: error con causa, detalle y reintento (`sectionRow` en `screens.js`). Skeletons.
- **N−**
  - Fila de 8 chips de proveedores de metadatos en el encabezado (`s03`). Es ruido técnico en la pantalla más vista. En el prototipo esto tenía sentido con 2 o 3 proveedores.
  - El Top 10 usa números gigantes que se montan sobre las portadas y desalinean la fila (`s04`).
  - "Se emite hoy y mañana" son dos columnas de alturas muy distintas, y la de "Hoy" deja un bloque vacío enorme (`s04`).
  - Cuatro filas enteras (Populares, Mejor valorados, Próximos, Más favoritos) se ven con portadas monograma en vez de imagen (`s04`). Si es el lazy-load de la captura, falta un skeleton. Si es real, es un fallo de carga sin estado.
  - Calendario: 7 columnas a 1280 px dejan los títulos en 1 o 2 palabras ("Maomao no…", "Nia Listo…", `s06`), peor que el clásico. "Mis series" y los recordatorios están bien.
  - Recomendaciones y Atlas no tienen entrada visible: solo se llega desde Ajustes › Inicio (lo dice `audit-wave-b`).

### 1.3 Buscar

- **C+** Explorar ("Para ti / Mejores / Recientes / A-Z"), tipo (Anime/Manga/Novelas/Personajes), año, género y filtros en una sola barra (`classic-02`).
- **R+** Barra M3 con Ctrl+K, chips de tipo y de fuente (metadatos o extensión), historial con borrar, y **lista | detalle** en ventanas anchas, con la ficha embebida.
- **Next**: es el área más cerca del rediseño y está bien. Tiene estado vacío con explicación, Explorar y Géneros como chips (`s30`), hoja de filtros (`s31`) y "Cargar más" (`s32`).
- **N−**
  - Los resultados ya no usan lista | detalle: van en grilla a pantalla completa (`s32`), así que se pierde el selling point del rediseño.
  - Falta el chip del filtro activo: el contrato espera `[data-filters]` y no existe.
  - Los géneros van en español en Buscar (`s30`) pero en inglés en la Ficha (`s07`) y en Novelas (`s11`).

### 1.4 Ficha (anime)

- **C+**
  - Banner a sangre con la imagen desenfocada detrás. Metadatos densos en grilla (emisión, estado, estudio, temporada, popularidad, favoritos, duración, fuente) arriba del pliegue.
  - Episodios como tarjetas con miniatura. Después: árbol de serie, reseñas, comentarios y relacionados (`classic-51`).
- **R+**
  - Jerarquía clara: portada + título + chips. Botón dividido "Continuar ep. N ▾", que despliega el selector de fuente. Dos o tres acciones secundarias.
  - Pestañas Episodios/Info/Relacionados, rangos de 100 episodios y panel lateral de fuentes y "Enviar a dispositivo".
  - Toolbar flotante con las acciones principales al scrollear.
- **N−**
  - **No hay banner ni fondo**: la Ficha se ve plana y genérica (`s07`) frente al clásico (`classic-51`).
  - La fila de acciones tiene **8 controles con el mismo peso** (Ver, ▾, Añadir a lista, ♥, Seguir, Enviar, Descargar, Copiar enlace) y se parte en dos líneas (`s07`).
  - Strings crudos: "RELEASING" y "FINISHED" en mayúsculas y en inglés. "Puntuación 84" (escala 0-100) convive con "★ 8.4" en las tarjetas y "84 %" en manga (`s07`, `s12`).
  - Las tiles de datos "ESTADO RELEASING / GÉNEROS 7" repiten los chips y no aportan (`s07`). El clásico ahí ponía 8 datos útiles.
  - El panel de fuentes vacío usa un título más grande que los encabezados de la página y empuja todo (`s07`).
  - En `s38` quedan dos overlays abiertos a la vez: el menú "Añadir a lista" sigue abierto detrás de la hoja de descarga. Además la toolbar flotante tapa la lista.

### 1.5 Fuentes

- **C+** Lista operativa y completa: contador ("42 Fuentes"), filtro de texto, orden por seeders/tamaño/calidad/proveedor/nombre, "Ocultar 0 seeders", "Ocultar lotes", chips de idioma, punto de salud por fila (`classic-04`). Está pensada para quien elige a mano.
- **R+** Ranking automático explicado (`score` en `sources.js`). Hoja con la recomendada arriba, pills de confiabilidad, latencia o seeders y subs/dubs. Modo auto o manual.
- **N−** No se pudo ejercitar sin extensiones. Lo que se ve en la Ficha es un resumen ("Selección de fuente: Automática · Ajustes", `next-12-details`). Hay que verificar que la hoja de fuentes de Next tenga **los filtros del clásico**: ordenar, ocultar 0 seeders y lotes, idioma. Sin eso, el usuario avanzado pierde capacidad.

### 1.6 Reproductor

- **C+**
  - Página "teatro": video, título, Anterior/Siguiente, **lista de episodios siempre visible** al costado y fuentes debajo (`classic-03`).
  - La configuración va en una side sheet con pestañas Reproducción/Video/Subtítulos/Audio/Atajos, todo con chips grandes (`classic-07`, `classic-08`).
- **R+** Inmersivo a pantalla completa con controles M3E: marcadores de intro/outro en la barra, preview al buscar, chip de fuente en la barra superior, presets de escalado (Ctrl+1..6), tarjeta de siguiente episodio con anillo y estados de error con failover.
- **Next**: implementa el inmersivo del prototipo (`tk-pc*`, `tk-pc__next`, `tk-pc__marker` en `player.css`). Los episodios van en una hoja (`EpisodeSheet`).
- **N−** No verificable visualmente todavía. A nivel de diseño, lo único que el usuario del clásico extraña es **ver episodios y fuentes sin abrir nada**. Eso justifica una variante Legacy "teatro" (sección 2.4).

### 1.7 Biblioteca y Descargas

- **C+** Descargas como "Biblioteca fuera de línea": barra de acciones (carpeta de sync, importar videos, descarga de torrents, abrir carpeta), pestañas Biblioteca / Auto-descarga / Historial y un **estado vacío con CTA** ("Encuentra una serie", `classic-13`).
- **R+** Biblioteca simple: Continuar en grilla, historial como lista de episodios y "Sincronizar" con estado de la cuenta.
- **N−**
  - Los estados vacíos **no proponen el siguiente paso**: "Aún no hay nada" (`s19`) y "No hay nada descargándose" (`s18`) no tienen botón.
  - En Biblioteca aparece el widget de calendario "Hoy / Mañana" debajo del vacío (`s19`). No pertenece ahí y duplica lo de Inicio.
  - Biblioteca suma 11 pestañas subrayadas más 3 chips sociales (Perfil, Actividad, Notificaciones) más "Sincronizar listas" activo sin cuenta (`s19`).
  - En Descargas, las pestañas son chips y el campo "Buscar" va con etiqueta, desalineado con las pestañas (`s18`). La franja de ruta y espacio libre sí es mejor que la del clásico.

### 1.8 Manga

- **C+** Descubrimiento muy rico: spotlight, "Último manga leído", "Descubrimiento inteligente" con géneros, tiles de géneros con arte y filas de manhwa/manhua (`classic-22`).
- **R+** No lo tenía: el prototipo es solo anime.
- **Next**: Inicio limpio con hero y filas (`s10`). La ficha está bien resuelta, con mejor fuente, filtro y agrupar por volumen (`s12`). El lector es inmersivo con tira de miniaturas y modos paginado/webtoon (`s13`–`s15`). Es de lo mejor de Next.
- **N−**
  - El CTA del hero dice **"Ver"** en manga (`s10`).
  - El encabezado mezcla título, campo con etiqueta y segmented: no se parece a ninguna otra página.
  - El estado de lectura son 6 chips outline sueltos que parecen filtros (`s12`). En anime el mismo concepto es un menú "Añadir a lista ▾" (`s07`). Es el mismo concepto con dos controles distintos.

### 1.9 Novelas

- **C+** Igual que manga: "Últimas novelas leídas", descubrimiento y géneros con arte (`classic-23`).
- **N−**
  - **Título y descripción alineados a la derecha** con un hueco a la izquierda (`s11`). Es un bug de layout.
  - Géneros en inglés.
  - Filtros como `<select>` nativos, que no siguen el sistema.
  - El hero queda debajo de los filtros y su CTA dice "Ver" en vez de "Leer".

### 1.10 Extensiones

- **C+** Catálogo navegable: pestañas Explorar / Mi biblioteca / Fuentes, categorías (Streaming, Manga, Torrent, Servicios públicos), búsqueda, tarjetas con autor, versión, instalaciones y "Instalada" (`classic-11`).
- **R+** Instalación por drag&drop o ruta, filas con compatibilidad, permisos como chips, banners de compatibilidad parcial, hash, dominios y logs.
- **N−**
  - Con 0 instaladas, el panel de instalación (drop + ruta + URL) **ocupa todo el pliegue** y el estado vacío queda debajo (`s21`). El caso común (agregar un repositorio y elegir) está escondido en la pestaña "Repositorios".
  - "Actualizaciones" es un botón aparte de las pestañas y "Desactivar todas" se ve deshabilitado sin explicación (`s21`).

### 1.11 Ajustes

- **C+**
  - **Modal sobre la app** con **buscador de ajustes**, navegación agrupada (Usuario / Aplicación / Información) y paneles cortos.
  - Selectores visuales con preview: estilo de tarjeta "Cartel limpio vs Superposición cinematográfica" y temas como tarjetas con degradado (`classic-40`). Reproductor con pestañas (`classic-43`).
  - Ya existía "Apariencia móvil: Refreshed / Classic" (`classic-40`), un antecedente directo de esta propuesta.
- **R+** Filas de ajuste homogéneas (label + hint + control a la derecha), paneles y switches M3, segmented.
- **N−**
  - **Una sola página de unos 9000 px** (`s17`), sin buscador. Las tablas de atajos (reproductor, lector) quedan expandidas en línea.
  - La paleta son 20 chips con un punto de color, sin preview del resultado (`s16`).
  - El asistente inicial queda alineado a la izquierda en una ventana ancha y su stepper se parte en 2 líneas (`s01`).

### 1.12 Link y servidor

- **R+** y **Next ≈ R**: tarjeta de estado con puerto, IP y huella, checklist de 3 pasos, emparejar y lista de dispositivos (`s22`). Está bien.
- **N−**
  - El primer paso del checklist ("Revisa tu firewall") sale marcado sin que el usuario haya hecho nada, y los otros dos son círculos sin número (`s22`).
  - "Servidor de extensiones" en el encabezado de Link mezcla dos conceptos. El servidor tiene su propia página (`s29`) colgando de Link.

### 1.13 Social

- **C+** Perfil con temas y comunidad (fuera de alcance por decisión: en Next la cuenta es AniList/MAL y no hay social propio).
- **Next**: el estado vacío "Conecta AniList" es claro y lleva CTA (`s40`).
- **N−** Se llega solo desde chips dentro de Biblioteca. No hay avatar ni cuenta en el shell, y las notificaciones no tienen badge global (ver 1.1).

### 1.14 Transversales (afectan a todo)

| Tema | Evidencia | Problema |
|---|---|---|
| Pestañas | `s07`/`s19` subrayadas, `s18`/`s40` chips, `s21`/`s10`/`s06` segmented | Tres componentes para lo mismo. |
| Encabezados de página | `s10`, `s11`, `s18`, `s21` | Cada área arma el suyo (`tk-page__head` se usa en 23 de unas 80 vistas). |
| Strings de datos | `s07`, `s11`, `s12` | Enums crudos en inglés, géneros sin traducir, tres escalas de puntaje. |
| Estados vacíos | `s18`, `s19`, `s21` | Explican qué pasa, pero no qué hacer después. |
| Overlays | `s38` | Abrir una hoja no cierra el menú anterior. La toolbar flotante tapa contenido. |
| Densidad | `s03`, `s07` | Mucho aire vertical arriba y luego todo el contenido abajo del pliegue. El clásico mostraba más arriba del pliegue. |
| Accesibilidad | `nav_rail.rs`, `s03` | Bien: `aria-current`, `aria-label`, foco visible, reduce-motion. Mal: etiquetas truncadas sin tooltip en el rail compacto (solo `title`) y el contraste de los chips outline del lector o estado de manga es bajo. |

### 1.15 Prueba con extensiones (2026-11-09, después de los arreglos)

Perfil limpio en el monitor secundario, con Toko 4.5.5, Latino 1.0.1 y HentaiDB 1.1.0 instaladas desde la propia UI (Extensiones › Ruta de un archivo .kai) y el audio de la app silenciado. Capturas en `parity/wave-b/after-fixes/`.

- **Instalar** (`e3`): las tres entraron a la primera, con compatibilidad "Completa", permisos y SHA-256. El bloque de instalación sigue arriba de la lista aunque ya haya extensiones (#7).
- **Buscar → ficha** (`f1s`, `f3`): Frieren aparece primero. La ficha ahora carga Información, Personajes y Relacionados. Sigue plana (#2) y repite "Enviar a dispositivo" en la cabecera, en la barra flotante y en una tarjeta lateral.
- **Fuentes** (`g1`, `g2s`): 16 fuentes en ~40 s. La recomendada es Toko · anizone (720p HLS, subs ES, audio JA), que coincide con las preferencias. El ranking y las etiquetas de release se leen bien; los problemas están en #17 y #18.
- **Reproducción** (`h1`): "Ver episodio 1" arrancó anizone y reprodujo (1:22 de 25:59, con "Saltar intro" a tiempo). El video de mpv no sale en las capturas del inspector ni en las de escritorio (mpv dibuja en una subsuperficie), así que el reproductor se evalúa por sus controles.
- **No probado**: lector de manga y de novela con una extensión `read` / `novel` (estas tres son de anime), descargas reales y el sniffer de embeds.

### 1.16 Top 19 por impacto en el usuario

| # | Problema | Captura | Arreglo en una línea |
|---|---|---|---|
| 1 | El rail desborda y trunca (10 destinos) | `s03`, `s40` | Agrupar: Inicio · Buscar · Biblioteca · Manga · Novelas · Descargas (con Torrents adentro). Extensiones, Enlace y Ajustes al pie. Tooltip real en modo compacto. |
| 2 | Ficha plana, sin banner, con 8 acciones del mismo peso | `s07` vs `classic-51` | Banner desenfocado de fondo. Split "Ver ▾" + "Añadir a lista ▾" + menú ⋯ con el resto. |
| 3 | Ajustes de 9000 px sin buscador | `s17` vs `classic-40` | Buscador que filtra filas (las claves i18n ya existen) y una sección por vez. Atajos plegados. |
| 4 | Strings crudos y escalas mezcladas | `s07`, `s11`, `s12` | Mapear estado y género a i18n. Una sola escala de puntaje (★ 8.4) en todas partes. |
| 5 | Tres componentes de pestañas | `s07`, `s18`, `s21` | Un `Tabs` con variante `primary` (subrayado) y `segmented`. Prohibir chips como pestañas. |
| 6 | Estados vacíos sin acción | `s18`, `s19` | Siempre un CTA: "Buscar algo para ver", "Conectar AniList", "Agregar repositorio". |
| 7 | Extensiones: el instalador tapa lo importante | `s21` | Estado vacío arriba con "Agregar repositorio" e "Instalar .kai…" (abre hoja). Lista de instaladas primero. |
| 8 | Overlays apilados y FTB que tapa | `s38` | Un overlay a la vez (`Menu` se cierra al abrir `Sheet`). La FTB se oculta mientras hay una hoja abierta. |
| 9 | Novelas alineada a la derecha y encabezados dispares | `s11`, `s10` | Todas las áreas usan `PageHead { title, intro, actions }`. |
| 10 | Calendario ilegible a 1280 | `s06` vs `classic-16` | Por debajo de unos 1500 px: modo agenda (lista por día) o 5 días con scroll horizontal. Título en 2 líneas. |
| 11 | Selector de fuentes sin filtros del clásico | `classic-04` | Orden, ocultar 0 seeders y lotes, chips de idioma y contador en la hoja de fuentes. |
| 12 | Inicio: chips de proveedores, Top 10 desalineado, widget de emisión desparejo | `s03`, `s04` | Proveedores a Ajustes. Top 10 con número a la izquierda de la tarjeta, no encima. Emisión en una sola fila. |
| 13 | Biblioteca mezcla calendario y social | `s19` | Sacar el widget de emisión. Lo social va a la entrada de cuenta (avatar) del shell. |
| 14 | CTA "Ver" en manga y novela; estado de lectura como chips sueltos | `s10`, `s11`, `s12` | "Leer" y el mismo "Añadir a lista ▾" que en anime. |
| 15 | Íconos ambiguos y doble hamburguesa | `s03` | Novelas → `book`, Manga → `menu_book`/`auto_stories`. Quitar la hamburguesa de la title bar. |
| 16 | Asistente inicial plano: tarjeta alineada a la izquierda sobre fondo vacío, stepper partido en 2 líneas, 21 chips de paleta sin vista previa | `after-fixes/wizard-1`, `s01` | Layout centrado con panel de vista previa a la derecha (la app con la paleta y el modo elegidos), stepper de una línea con números y conector, paletas como swatches grandes. |
| 17 | La ficha dice "no hay fuentes" o muestra un esqueleto durante ~40 s mientras las extensiones resuelven, sin decir qué pasa | `after-fixes/f3` | Estado "Buscando en Toko, Latino… (2 de 3)" con progreso por extensión y lo primero que llega ya listado. |
| 18 | Hoja de fuentes: todas dicen "Confiabilidad: Media · 80 %", el hash del torrent se ve crudo y un pack de 44 GB con doblaje RU queda 4.º | `after-fixes/g2s` | Ocultar la confiabilidad hasta tener historial propio, hash en un "copiar", y bajar packs y audio que no coincide con las preferencias (o agruparlos bajo "Otras"). |
| 19 | Reproductor con 18 botones y la barra inferior que se esconde enseguida | `after-fixes/h1` | Agrupar Imagen en imagen, Captura, Temporizador y Atajos en "Más"; la barra no se oculta mientras el puntero está sobre ella. |

Del 1 al 19 todos afectan a los dos estilos (16 a 19 salieron de la prueba con extensiones de §1.15). Conviene arreglarlos **antes** o en paralelo a la Fase 1 de abajo, porque si no se mantienen dos veces.

---

## 2. Propuesta: estilos de UI

### 2.1 Objetivo y principio

- `UiStyle::Nuevo` (por defecto) es el rediseño M3 Expressive tal como se viene construyendo.
- `UiStyle::Legacy` se ve y se siente como el clásico: glass oscuro, radios grandes, rail de íconos, Ficha con banner, tarjetas cartel o superposición, reproductor teatro opcional.
- `Mezcla` (más adelante) es un mapa por área que elige Nuevo o Legacy.

**Regla central: una pantalla, un estado, varias presentaciones.** Cada pantalla (`screens/*.rs`) sigue siendo única y es dueña de los datos, las señales, la navegación y los `data-*` del contrato. El estilo solo cambia:

1. tokens (CSS variables),
2. reglas CSS de componentes, y
3. en unos pocos **puntos de variación** con nombre, el componente hoja que se renderiza.

No se duplica ninguna pantalla.

Antecedente: el clásico ya tenía `mobileLayout: refreshed | classic` y `mediaCardStyle: poster | overlay` (`MobileAppSettingsPanel.tsx`, `MediaCardStyleSelector.tsx`). Es la misma idea, ahora formalizada.

### 2.2 Cómo está hoy tk-ui (lo que se aprovecha)

- `tokens.css` lo genera `packages/tokens/generate.mjs` con selectores `:root[data-palette][data-mode][data-contrast]`. Los componentes ya consumen `var(--md-sys-*)`, `--tk-space-*` y `--md-sys-shape-*`.
- `platform::apply_theme_js` pone atributos en `<html>`, y `App` lo llama en un `use_effect` sobre `settings`. El estilo es un atributo más.
- `Settings` es `#[serde(default)]`, así que agregar un campo no rompe archivos viejos.
- El CSS es BEM `tk-*` y va partido por área (`assets/css/*.css`, lista `APP_CSS` en `app.rs`). Se agrega un `legacy.css` (o uno por área) al final y gana por orden.
- `Sheet`, `Dialog` y `Menu` se renderizan dentro del árbol del componente que los abre (`position: fixed`, sin portal). Por eso heredan el atributo del ancestro de la página, algo clave para Mezcla.
- El reproductor ya ubica mpv sobre un rectángulo (`PlayerHost::set_rect`, `screens/player/mod.rs:382`). Un layout "teatro" con el video en una caja es viable sin tocar `tk-player`.

### 2.3 Capas

**Capa 0: tokens por estilo (sin Rust)**

- Las paletas (20) y los modos se comparten. Un usuario Legacy elige paleta igual que hoy. El clásico se veía rosa por la paleta Cherry Blossom, no por ser "clásico".
- `tokens.json` suma `styles.legacy` con los tokens **no cromáticos**:
  - forma: `--tk-radius-card: 16px`, `--tk-radius-panel: 20px` (el clásico usa `--radius: 1.25rem`) y botones píldora sin morph;
  - tipografía: en el clásico el cuerpo es Space Grotesk y el display es Outfit 700-800, al revés que en Next;
  - superficies "glass": `--tk-surface-panel: color-mix(... 3%)`, `--tk-panel-blur: 24px`, `--tk-panel-hairline`;
  - densidad: `--tk-page-gap`, `--tk-row-gap` y `--tk-card-w`;
  - movimiento: Legacy usa fades y transforms simples, sin `--tk-mo-bounce`.
- `generate.mjs` emite `[data-ui-style="legacy"] { … }` (y lo mismo para `nuevo`, con los valores actuales) en `tokens.css`, y `TatakaiStyleTokens.kt` para Android.
- **Importante**: van como selector de atributo **en cualquier elemento**, no solo en `:root`. Las custom properties heredan del ancestro más cercano, así que el anidado de Mezcla funciona solo.

**Capa 1: variantes de componentes por CSS (sin Rust o casi)**

`assets/css/legacy.css` con reglas `[data-ui-style="legacy"] .tk-…` para:

- `.tk-card`: cartel (título debajo, como hoy) o **superposición** (título sobre degradado, como en el clásico). Es un sub-ajuste "Tarjetas" que vale en los dos estilos (ver 2.6).
- `.tk-btn`, `.tk-iconbtn`, `.tk-chip`, `.tk-seg`: píldoras planas, filled con acento y sin el morph de forma de M3E.
- Panels: glass con borde hairline arriba (`.glass-panel::before` del clásico).
- Pestañas: en Legacy, el `Tabs` unificado del punto 5 del top 15 se dibuja como barra píldora (`classic-11`: "Explorar / Mi biblioteca / Fuentes").
- `.tk-page__title`: Outfit 800 y más grande, como "Descubre" o "Extensiones" del clásico.

Esto solo ya da un 60-70 % de la sensación "clásica" con una sola hoja de estilos.

**Capa 2: puntos de variación de layout (Rust, acotados)**

Solo donde la estructura es realmente distinta. Cada punto es un componente despachador con la **misma VM y los mismos handlers**:

```rust
#[component]
pub fn DetailsHero(vm: DetailsHeroVm, on: DetailsActions) -> Element {
    match use_ui_style(UiArea::Details) {
        UiStyle::Nuevo => rsx! { DetailsHeroNuevo { vm, on } },
        UiStyle::Legacy => rsx! { DetailsHeroLegacy { vm, on } },
    }
}
```

| Punto de variación | Nuevo | Legacy | Dónde |
|---|---|---|---|
| `Shell/NavRail` | Rail M3 con etiquetas, FAB y drawer | Rail fino de íconos con logo, grupos con divisores, cuenta y ajustes al pie, tooltips | `components/nav_rail.rs` (prop `variant`) |
| `Home/Hero` + orden de bloques | Carrusel multi-browse | Spotlight grande + Top 10 en grilla + tiles de géneros con arte | `screens/home.rs`, `components/media.rs::HeroCarousel` |
| `Details/Hero` | Portada + chips + split | Banner desenfocado a sangre + grilla de metadatos + episodios como tarjetas | `screens/details.rs` |
| `Player/Layout` | Inmersivo | Teatro: video en caja + Anterior/Siguiente + episodios al costado + fuentes debajo | `screens/player/mod.rs` (la caja `.tk-hole` cambia de lugar, `set_rect` ya existe) |
| `Settings/Container` | Página con índice | Modal sobre la ruta actual, con buscador y grupos | `screens/settings/mod.rs` |
| `Calendar/Grid` | Columnas compactas o agenda | Columnas altas con portada grande y badges HOY/SIGUIENTE | `screens/calendar.rs` |
| `Manga/Novel Home` | Hero + filas | Spotlight + "Último leído" + descubrimiento con géneros | `screens/manga/home.rs`, `screens/novel/home.rs` |

**Ese es el techo: unos 7 puntos.** Cualquier otra diferencia tiene que entrar por la Capa 0 o la Capa 1. Si en un PR aparece un `match style` fuera de estos puntos, se rechaza en la revisión.

### 2.4 Dónde vive el ajuste y cómo se aplica

**Modelo** (`model/ui_style.rs`, nuevo; el patrón "un struct de prefs por dueño" de la ola A/B):

```rust
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum UiStyle { #[default] Nuevo, Legacy }

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum UiStyleChoice { #[default] Nuevo, Legacy, Mezcla }

/// Mezcla: one field per area (a struct, not a map: no typos, serde(default) per field).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(default)]
pub struct UiMix { pub shell: UiStyle, pub home: UiStyle, pub search: UiStyle, pub details: UiStyle,
    pub player: UiStyle, pub library: UiStyle, pub manga: UiStyle, pub novels: UiStyle,
    pub extensions: UiStyle, pub settings: UiStyle, pub link: UiStyle }

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(default)]
pub struct UiPrefs { pub style: UiStyleChoice, pub mix: UiMix, pub cards: CardStyle /* poster | overlay */ }

impl UiPrefs { pub fn for_area(&self, a: UiArea) -> UiStyle { /* Nuevo|Legacy directo; Mezcla → mix.<a> */ } }
```

`Settings` suma `pub ui: UiPrefs`. `sanitized()` no necesita nada, porque los enums con default ya cubren valores desconocidos si se usa `#[serde(other)]` en una variante de respaldo.

**Aplicación**

- `apply_theme_js` suma `data-ui-style` en `<html>` con el estilo del **shell** (`for_area(Shell)`), más `data-card-style`.
- Un hook `use_ui_style(area) -> UiStyle` lee `Ui.settings`.
- Cada página pone en su raíz `"data-ui-style": style.id()` y `"data-ui-area": area.id()`. Se hace con un wrapper `PageRoot { area, children }` que reemplaza el `div { class: "tk-page" }` actual (23 usos de `tk-page__head` más las raíces). Como `Sheet`, `Menu` y `Dialog` se renderizan dentro de la página, heredan el estilo.
- `Route::ui_area()` (al lado de `nav_dest()`) da el área de cada ruta. El player, los lectores y el setup tienen la suya.
- Las reglas de Capa 1 usan `[data-ui-style="legacy"] .tk-card`. Con Mezcla puede pasar que `<html>` sea legacy (shell) y la página nuevo. Como **el rail y `.tk-main` son hermanos** y la página nunca vive dentro del rail, alcanza con escribir las reglas del shell como `[data-ui-style=legacy] > .tk-shell …` (o poner el atributo en `.tk-rail` y `.tk-titlebar`, no en `<html>`), y las de la página siempre relativas a la raíz de la página. **Recomendación**: no poner `data-ui-style` en `<html>` cuando el estilo es Mezcla. Se pone en `.tk-titlebar`, en `.tk-rail` y en cada `PageRoot`, y así nunca hay dos ancestros en conflicto.
- No usar `@container style()`: WebKitGTK en Linux no lo garantiza.

**Interfaz** (Ajustes › Apariencia, primer bloque, "Estilo de la interfaz")

- Tres tarjetas con miniatura (como `MediaCardStyleSelector` del clásico): **Nuevo** · **Clásico** (etiqueta visible; el enum sigue siendo `Legacy`) · **Mezcla**.
- Al elegir Mezcla se despliega una tabla "Área → [Nuevo | Clásico]" (segmented por fila) con atajos: "Empezar desde Nuevo" y "Empezar desde Clásico".
- Sub-ajuste "Tarjetas: Cartel / Superposición", válido en los dos estilos.
- Se aplica al instante: la señal ya re-renderiza, no hace falta reiniciar.
- El asistente inicial (`s01`, paso 1) suma "Estilo: Nuevo / Clásico".
- `data-ui-style-choice` en el bloque, para el arnés.

### 2.5 Mezcla después

- Granularidad recomendada: **por área**, que es la tabla de arriba con 11 áreas. Coincide con cómo piensa el usuario ("Inicio como antes, la Ficha nueva").
- Lo que **no** se mezcla: dentro de un área todo es de un estilo. Tokens globales como la tipografía del shell siguen al shell.
- Para que no se vea Frankenstein: Mezcla comparte paleta y modo. Solo cambian forma, densidad y layout, y la Capa 0 garantiza que los dos estilos usen los mismos roles de color.
- Presets sugeridos: "Clásico con reproductor nuevo" (`player: Nuevo`, el resto Legacy) y "Nuevo con Ficha clásica" (`details: Legacy`).

### 2.6 Android

- **Fase inicial: solo tema.**
  - `TatakaiTheme(style = UiStyle.Nuevo | Legacy)` cambia `Shapes` (radios), `Typography` (cuerpo y display invertidos), un `LocalUiStyle` y el `CardStyle` de `HeroCarousel` y las tarjetas.
  - Los tokens salen de `TatakaiStyleTokens.kt`, generado por el mismo `generate.mjs`.
  - Las pantallas Compose no se tocan.
- El clásico móvil ya distinguía "Refreshed / Classic" en el hero y el header. Si se quiere más, el único punto de variación razonable en Android es el hero de Inicio y de la Ficha.
- ¿Sincronizar el estilo por Link? No por defecto: el estilo es de cada dispositivo. Queda como pregunta abierta.

### 2.7 Riesgos

| Riesgo | Mitigación |
|---|---|
| **Hooks `data-*` del contrato** (`checklist.json`, `scripts/parity/wk.mjs`) | Los `data-*` viven en la pantalla o la VM, no en la hoja de variante: las dos variantes reciben los mismos atributos por props. `wk.mjs --ui-style legacy` corre el humo dos veces, y un test de Rust renderiza cada punto de variación en ambos estilos y compara el conjunto de `data-*`. |
| i18n | Legacy no trae textos del clásico: reutiliza las claves de Next. Claves nuevas: unas 20 (`settings.ui_style.*`, `ui_area.*`), en `en` y `es`. |
| Costo de mantener dos estilos | Techo de unos 7 puntos de variación. Las features nuevas se hacen en Nuevo y la variante Legacy aparece sola por Capa 0 y 1. Solo los 7 puntos piden trabajo doble. Capturas de humo en ambos estilos en CI cuando exista. |
| Reproductor teatro en Legacy | mpv es una ventana nativa (HWND o widget GL): **nada HTML puede dibujarse encima del video** (menús, toasts) y la caja tiene que actualizar `set_rect` al scrollear o redimensionar. En Linux se suma la deuda de `10-linux-video-embed-debt.md`. Fuera de la caja de video el layout es libre, y el inmersivo sigue a un clic ("Pantalla completa"). |
| Regla CSS que se escapa entre áreas en Mezcla | Atributo en hermanos (titlebar, rail, página) y nunca en `<html>` en modo Mezcla. Lint simple: toda regla de `legacy.css` empieza con `[data-ui-style="legacy"]`. |
| Peso del CSS | Unos 600-900 líneas más. Irrelevante: todo es `include_str!`. |
| Tipografía Legacy | Es la misma familia (Space Grotesk y Outfit) invertida. No hace falta descargar fuentes nuevas. |

### 2.8 Esfuerzo estimado

| Fase | Qué | Archivos o áreas | Tamaño aprox. |
|---|---|---|---|
| F0 · Arreglos comunes | Top 15 (1-15). Ver 1.15. | `nav_rail.rs`, `app.rs`, `details.rs`, `settings/mod.rs`, `extensions.rs`, `library.rs`, `downloads.rs`, `novel/home.rs`, `manga/*`, `calendar.rs`, `home.rs`, componente `Tabs` y `PageHead` nuevos, i18n | 2000-3000 líneas, una ola de unos 6 workers |
| F1 · Plumbing + piel Legacy | `UiPrefs`, `use_ui_style`, `PageRoot`, `Route::ui_area`, `apply_theme_js`, `tokens.json` + `generate.mjs` (bloque `styles`), `legacy.css`, selector en Apariencia y en el setup, tests de serde y de `data-*` | unos 15 archivos | 1200-1800 líneas |
| F2 · Puntos de variación de layout | NavRail, Home, DetailsHero, Settings modal, Calendar, Manga y Novel home | unos 10 archivos | 2500-3500 líneas |
| F3 · Reproductor teatro | Layout Legacy del player, sincronizar `set_rect` con scroll y resize, fuentes debajo | `screens/player/*`, `player.css` | 800-1200 líneas |
| F4 · Mezcla | `UiMix`, tabla por área, presets, reglas CSS por hermanos, humo por área | unos 6 archivos | 400-600 líneas |
| Android-1 | `TatakaiTheme(style)`, `TatakaiStyleTokens.kt`, ajuste en Apariencia | unos 5 archivos | 300-500 líneas |

### 2.9 Plan por fases recomendado

1. **F0 primero**, en una ola con contrato y workers en paralelo (el flujo preferido: contrato, workers sin build, integrador, un build, fixer). Mejora Nuevo y evita mantener los mismos defectos dos veces.
2. **F1** sale con "Clásico (beta)" en Apariencia. A esa altura ya se ve como el clásico en forma, tipo y superficies, sin cambiar layouts.
3. **F2** convierte Clásico en "de verdad": rail, banner en la Ficha, Inicio, Ajustes modal. Va en orden de impacto: Ficha, rail, Ajustes, Inicio, Calendario, Manga y Novelas.
4. **F3** reproductor teatro, solo si el usuario lo confirma (por el riesgo de airspace).
5. **F4** Mezcla, cuando Nuevo y Legacy estén estables. **Android-1** puede ir en paralelo a F2.

---

## 3. Preguntas abiertas

1. Etiqueta visible: ¿"Clásico" (recomendado) o "Legacy"?
2. Al elegir Clásico, ¿se cambia también la paleta a Flor de cerezo (rosa, como se veía el clásico) o se respeta la paleta actual del usuario?
3. Reproductor Legacy: ¿hace falta el modo teatro, sabiendo que no se puede dibujar nada encima del video y que en Linux suma riesgo? ¿O alcanza con mostrar episodios y fuentes en el inmersivo?
4. Ajustes en Clásico: ¿modal como antes, o alcanza con el buscador en la página (que entra en F0 para los dos estilos)?
5. Rail: ¿está bien bajar a 6 destinos principales (Torrents adentro de Descargas; Extensiones, Enlace y Ajustes al pie)? Cambia los atajos 1..0.
6. Mezcla: ¿por área (11 áreas, recomendado) o por "pieza" (rail, tarjetas, Ficha, reproductor)?
7. Android: ¿solo tema por ahora? ¿El estilo se sincroniza por Link o es de cada dispositivo?
8. Tarjetas "Superposición cinematográfica": ¿se ofrecen también en Nuevo, como sub-ajuste independiente?
