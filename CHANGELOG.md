# Changelog — Tatakai Next

Cambios de Tatakai Next (rama `main`). Tatakai Classic tiene su propio changelog en la rama `legacy`.

## [0.4.2] - 2026-11-20

Ficha renovada en el escritorio y en el teléfono: el episodio que sigue a la vista, nuevo estilo "Cine editorial" y cuatro formas de ordenar la cabecera.

**Ficha**
- Nueva cabecera con cuatro composiciones a elegir en Ajustes › Apariencia › Cabecera de la ficha: Una línea, Ficha técnica, Siguiente episodio y Ficha y episodio (la de serie). Los presets no la cambian.
- "Ficha y episodio" junta los datos con etiqueta (estado, próximo episodio, "3 de 12 vistos", año, puntuación), las acciones con su nombre y una tarjeta con el episodio que sigue: título, progreso, tiempo restante, Continuar y la fuente.
- El episodio a continuar se ve sin bajar, también en ventanas de 1024×640 y con el menú lateral abierto. Los episodios ya vistos se pliegan en "Episodios anteriores".
- El botón principal ya no se corta nunca, y la línea de fuente dice de qué episodio es.
- Escritorio: la barra de pestañas queda fija arriba con sus acciones, las pestañas se recorren con las flechas, cada episodio es una sola parada de Tab y la tecla M lo marca como visto.
- Teléfono: Favorito, Seguir y Tráiler pasan a ser botones directos (Favorito funciona aunque el título no esté en ninguna lista), los géneros están en Información y la insignia +18 va junto al título.

**Estilo "Cine editorial"**
- Nuevo estilo de página (preset Cine): el fanart a sangre sin desenfoque, título grande y metadatos como texto. En Inicio el destacado ocupa como máximo el 45 % del alto.

**Teléfono**
- La Ficha respeta el tema de la app (oscuro o AMOLED) aunque el sistema diga otra cosa.
- El reproductor mantiene la orientación al pasar al siguiente episodio.
- Los subtítulos ya no salen gigantes con el teléfono vertical.

**Accesibilidad**
- Los botones se anuncian con su texto visible, la cuenta regresiva se lee completa ("en 2 días y 5 horas") y los selectores de Apariencia se recorren con las flechas.

## [0.4.1] - 2026-11-17

Arreglos de las pruebas de 0.4.0 en Tatakai Link y más opciones de subtítulos y de fin de episodio.

**Tatakai Link (procesar en la PC)**
- Pausar un buen rato ya no deja la alerta "Tu PC está preparando el video…" parpadeando para siempre: la PC cortaba la entrada del video a los 20 s de cada tramo y a veces se quedaba sin producir. Ahora la entrada solo se corta sin datos, la PC deja de procesar mientras está en pausa y retoma antes de que el reproductor llegue, y reinicia sola un procesamiento trabado.
- Ya no se pierden secciones del video (eran los cortes de esos tramos).
- Saltar ±10 s dentro de lo ya procesado es instantáneo: la PC guarda 60 s detrás de la posición.
- El final del episodio termina normal en vez de "tu PC no pudo seguir con esta sesión".
- Si una sesión se traba, el teléfono la renueva en el mismo punto (hasta 3 veces) y si no, ofrece "Reproducir en el teléfono".

**Reproductor**
- Pantalla de fin de episodio: siguiente episodio con cuenta regresiva, volver a ver, episodios, volver a la ficha, progreso en AniList y "Terminaste la serie".
- La barra de tiempo distingue lo visto, lo descargado o en buffer, las partes del torrent y lo que ya procesó la PC (con leyenda). Arreglado el buffer de libmpv, que se mostraba al doble de la posición.

**Subtítulos**
- Estilo completo como en Enigmata: tamaño, fuente (incluidas importadas), negrita y cursiva, color y opacidad, contorno, sombra, caja de fondo con transparencia, posición, márgenes, interlineado, suavizado y alineación; ASS: respetar el original, aplicar mi estilo o texto simple. Vista previa y estilos rápidos.
- En el reproductor: Subtítulos › Estilo cambia en vivo, con "Más opciones" para el editor completo. Tu configuración anterior se conserva.

## [0.4.0] - 2026-11-17

Mejoras de video en el teléfono a la par del escritorio, Tatakai Link elegible desde el reproductor y arreglos para tablets.

**Reproductor del teléfono**
- libmpv es el motor predeterminado (las instalaciones que estaban en Media3 pasan a libmpv una vez) con Anime4K A+A de entrada. Media3 queda como motor simple y respaldo, con "Cambiar a MPV" en el mismo punto.
- Nuevo panel "Mejoras" en el reproductor: Anime4K A, A+A, B, C, B+B y C+A en calidad Rápida, Equilibrada o HQ, FSR, Fidelity y CAS; nitidez, deband, suavizado, proporción y zoom, brillo/contraste/saturación/gamma/tono y decodificador. Todo cambia al instante, sin reiniciar el video, y se mantiene al cambiar de episodio o de modo.
- Un indicador arriba dice qué mejora corre y dónde ("Anime4K A+A · HQ", "PC · Anime4K 1080p + LSFG ×2", "PC escala · Teléfono FG ×2").
- Ajustes automáticos: baja la calidad si el teléfono se calienta, con poca batería o si se pierden cuadros, con "Deshacer".
- Generación de cuadros en el teléfono (LSFG, con el Lossless.dll propio importado en Ajustes › Reproductor): ×2 a ×5 o automático, perfiles Ahorro/Equilibrado/Calidad.

**Tatakai Link**
- "Procesar en: Teléfono | PC | Mixto" desde el reproductor, en cualquier dirección y en el mismo punto, con el mismo audio y subtítulos. Mixto: la PC escala y el teléfono genera los cuadros.
- El reproductor muestra qué hace la PC (escalado, generación de cuadros con el multiplicador real, códec, margen) y solo ofrece los perfiles que la PC sostiene.
- Los subtítulos de la fuente llegan desde la PC (ASS con estilos en MPV, VTT en Media3).
- Ajustes › Enlace: "Procesar por defecto en".

**Subtítulos**
- La traducción funciona con cualquier subtítulo de texto, también los internos de MKV/MP4 de torrents, y traduce por adelantado.
- Google sin clave ya no se corta con errores 429: pide en tandas espaciadas, reintenta con espera y alterna dos accesos; un solo aviso si limita.

**Tablets y Android**
- Torrents: ya no se cierra la app en tablets con Android 16 (el sistema de archivos tiraba el servicio de almacenamiento); las descargas van al almacenamiento interno de la app.
- En horizontal: Biblioteca y las demás listas usan todo el ancho, y el menú lateral se desplaza cuando no entra.
- Continuar viendo: tocar o mantener apretada la tarjeta abre los detalles; el botón de reproducir sigue el episodio.

## [0.3.4] - 2026-11-15

**Integrado mejorado (mpv-anime-build)**
- Arranca con Anime4K en vez de FidelityFX: modo A+A (High Fid.) en calidad HQ para fuentes de SD a 4K, con la detección de anime en ON (Tatakai solo reproduce anime). Vale también para el mpv de la generación de cuadros. Lo que ya hayas elegido en "Anime Build Options" se respeta.

## [0.3.3] - 2026-11-15

Generación de cuadros y mpv-anime-build, más claros y completos (escritorio).

**Generación de cuadros**
- Al reproducir en mpv externo por la generación de cuadros, la app lo dice ("por la generación de cuadros (×N)") y explica por qué: lsfg-vk solo funciona con Vulkan y el reproductor integrado dibuja con OpenGL.
- Botón "Ver sin generación de cuadros": reproduce ese episodio en el motor integrado elegido desde el mismo punto, sin cambiar los ajustes.
- Con el motor Integrado mejorado, el mpv de la generación de cuadros carga mpv-anime-build: sus perfiles (FidelityFX, Anime4K…), scripts y uosc con su menú de clic derecho. Antes arrancaba sin configuración.
- Ese mpv abre a pantalla completa en el monitor de la app (en X11; en Wayland lo ubica el compositor).
- Ajustes › Reproductor avisa junto al motor cuando la generación de cuadros está activa.

**Integrado mejorado**
- Con uosc: el puntero, los clics (incluido el derecho), la rueda y el arrastre llegan bien a uosc, con coordenadas correctas en pantallas HiDPI.
- Con los controles de Tatakai: nuevo menú "Perfiles" (también con clic derecho sobre el video) con los perfiles de mpv-anime-build.

## [0.3.2] - 2026-11-15

Tarjetas más legibles y arte de mejor calidad.

**Tarjetas**
- Tamaño de tarjeta configurable en Ajustes › Apariencia › Diseño y estilo: chica, mediana, grande o extra grande (también dentro de los presets; Cine usa grandes). Las grandes son horizontales, con título, estado, formato, año, episodios, puntaje, géneros y, en las filas de episodios, "Episodio N · hace X".
- "Solo póster" muestra siempre el título (ya no hace falta pasar el puntero).

**Arte**
- El banner destacado y el fondo del Detalle usan el fanart en alta resolución de TVDB (vía ani.zip) y el logo del título cuando existen, en vez del banner de AniList de 1900×400. Los banners de baja resolución se recortan en lugar de estirarse.

**Episodios**
- La lista de episodios se completa con ani.zip por número: capturas, títulos, sinopsis, duración y rating. Antes, si Kitsu respondía solo con números (series nuevas), todas las miniaturas eran la misma imagen.

## [0.3.1] - 2026-11-15

Uso de AniList más cuidadoso y rápido: menos pedidos, sin chocar con el límite y sin caer a Kitsu por un rato (lo que mostraba títulos raros).

**AniList (escritorio y Android)**
- Un solo cliente de AniList para toda la app: respeta los límites que informa AniList (`X-RateLimit-*`), espera lo que pide `Retry-After` y reintenta solo si la espera es corta; si es larga, usa otra fuente solo ese tiempo.
- Lo que pides tú (abrir un título, buscar, reproducir) pasa antes que lo de fondo (filas de Inicio, recomendaciones, calendario).
- Pedidos iguales al mismo tiempo comparten una sola llamada.
- Las filas de Inicio, "Porque viste…", el orden de visionado y el mes del calendario se piden juntos en menos pedidos.
- Inicio se muestra al instante con lo guardado y se refresca por detrás; la ficha de un título se pide una sola vez para Detalle, Episodios y Reproducir.
- Las tarjetas de las filas piden menos datos.
- Escritorio: "Nuevos episodios de tus series" ya no hace un pedido por cada 50 títulos de tu lista; el bloque de calendario de Inicio pide solo hoy y mañana; los géneros que usa "Para ti" se guardan una semana. Con la lista real de prueba, abrir Inicio pasó de unos 20 pedidos a 11 la primera vez y 2 las siguientes.
- Escritorio: Ajustes › Sistema muestra los pedidos a AniList por minuto, los límites recibidos y la espera actual.

**Tatakai Link**
- Link ya no se anuncia en la red al arrancar cuando el anuncio está desactivado.

## [0.3.0] - 2026-11-15

Rediseño de la interfaz inspirado en lo que más gustaba de Hayase, con variantes configurables, y sincronización del historial de Link por dispositivo.

**Diseño y estilo (Ajustes › Apariencia)**
- Cuatro presets (Tatakai, Cine, Denso, Tranquilo) que aplican de una vez el diseño de Inicio, el estilo de tarjetas, el menú lateral, la densidad, las animaciones, la vista de episodios, de relacionados y del calendario. Cada opción se puede cambiar suelta.
- Menú lateral con nombres o angosto (solo íconos), densidad cómoda o compacta, animaciones expresivas, sutiles o desactivadas, efectos de desenfoque opcionales.
- Calendario y Para ti pasan a ser entradas del menú principal (en Android, en la barra de navegación).

**Inicio**
- Destacado: banner a todo el ancho con chips (episodios, formato, estado, temporada, puntaje), sinopsis, géneros, Continuar / Ver ahora y una barra que avanza sola y se pausa con el puntero encima. También Carrusel y Solo filas.
- Tarjetas con detalle (punto de estado, año y formato), clásicas o solo póster; filas con flechas al pasar el puntero.
- Más fluido: el carrusel ya no recalcula el layout de toda la página en cada giro, el desenfoque por tarjeta se quitó y las filas fuera de pantalla no se pintan hasta acercarse.

**Detalle**
- Episodios en Tarjetas (miniatura con duración y rating, sinopsis, fecha relativa, progreso), Lista o Compacta.
- Relaciones como grafo de la franquicia con zoom, ajustar y pantalla completa, o como lista agrupada.
- Personajes y Relaciones se cargan la primera vez que se abren.

**Calendario**
- Vistas Semana, Agenda, Mes y Hoy (línea de tiempo con la hora actual); la elegida se recuerda.

**Tatakai Link: historial por dispositivo**
- Cada dispositivo emparejado tiene un modo de historial: sincronización completa, solo recibir (lo que reproduce no se guarda en la PC; ideal para dispositivos de prueba) o pausada.
- La PC registra de qué dispositivo vino cada episodio: "aportó N episodios" y "Borrar lo que aportó".
- "Usar como fuente": ese dispositivo envía todo su historial, gana en la PC y los demás reemplazan el suyo. Necesita la app Android 0.3.0 en los teléfonos.

## [0.2.0] - 2026-11-15

Versión de estabilidad: 14 rondas de pruebas de escritorio, Tatakai Link y Android (búsqueda, reproducción con y sin Anime4K/FSR y generación de cuadros, todos los reproductores, torrents y extensiones) y más de 140 problemas corregidos.

**Tatakai Link**
- El paquete de escritorio (AppImage, PKGBUILD, instalador y zip de Windows) trae el helper `temple-lsfg`: Link ofrece los perfiles LSFG y Anime4K sin configurar nada a mano. Los shaders se instalan en una carpeta con permisos de escritura.
- Las sesiones ya no se traban: duración real en torrents y MP4, saltos cortos sin reiniciar la PC, reconexión en el mismo punto si la sesión vence o se corta la red.
- Enviar al teléfono manda la fuente que elegiste (web o torrent) y puede transmitir un torrent desde el motor de la PC; un torrent ya bajado arranca en menos de un segundo. Un envío viejo se reabre al instante.
- Cambiar el perfil de mejora desde el reproductor del teléfono siempre termina con un aviso y sigue en el mismo punto, también en torrents. Los perfiles que el teléfono no puede decodificar (1440p/2160p) aparecen deshabilitados con el motivo; Anime4K baja de nivel si el teléfono no da abasto y se apaga con poca batería.
- La lista de dispositivos y su estado se actualizan solos; volver a vincular el mismo teléfono reemplaza su entrada.

**Fuentes y extensiones**
- Selección de fuente Manual pregunta siempre, con tu elección arriba; si tu elección no apareció, se dice. La lista se ordena igual en la PC y el teléfono, sin duplicados, y muestra idiomas, tamaño y grupo.
- Listas vacías o parciales ya no esconden fuentes durante minutos; la hoja de fuentes y el reproductor abren al instante con la lista conocida y la refrescan detrás.
- El teléfono hereda las extensiones de la PC por el servidor de extensiones, con "Actualizar desde la PC" e instalación del `.kai` de la PC.
- Si AniList limita o no responde, el respaldo (Kitsu y otros) conserva los títulos y la portada de AniList y se guarda poco tiempo, así las extensiones siguen encontrando las fuentes; la búsqueda por texto también tiene respaldo.
- Llamadas de extensiones superpuestas o abandonadas ya no se bloquean ni se interrumpen entre sí (manga, novelas y Descubrir).

**Reproductor**
- Ningún preset que libmpv rechace vuelve a cerrar la app; los streams HLS con muchas pistas se abren con el master filtrado.
- Fuentes sin decodificador por hardware se marcan y caen a libmpv; el reproductor externo dice cuándo no hay app para reproducir.
- El torrent deja de bajar al cerrar el reproductor; "Borrar después de ver" vuelve a funcionar; se guarda la posición final.

**Android**
- Controles del reproductor, Detalles, Inicio, selector de fuentes, extensiones y ajustes rediseñados para pantallas de teléfono en vertical; lectores de manga y novelas en vertical y horizontal (lectura de derecha a izquierda, barras compactas).
- Manga y novelas de Toko por sus rutas propias; géneros, roles del staff y rankings de AniList en español.

**Escritorio**
- Barra inferior en ventanas bajas, dos paneles solo en pantallas anchas; tamaños con el separador decimal del idioma; textos más claros en errores, permisos de extensiones y estados de carga.

## [0.1.2] - 2026-11-10

- Escritorio: con un idioma de coma decimal (español, alemán, francés…) las extensiones leían `0.4` como `0`, `parseFloat("1.5")` como 1 y los decimales de JSON como 0 (el motor JS usaba el formato numérico del sistema). Toko descartaba todos los torrents de nyaa por eso. Ahora cada extensión usa el formato numérico internacional.
- Fuentes: los torrents muestran grupo, resolución y tamaño aunque la extensión mande un título corto (SubsPlease); se piden los torrents antes que los streams, y la ficha resuelve solo el episodio que sigue (antes resolvía el 1 primero y bloqueaba la extensión ~40 s).
- Episodios sin miniatura muestran la imagen de la serie con el número.

## [0.1.1] - 2026-11-10

- Android: vincular AniList fallaba con `unsupported_grant_type`. La app mandaba `redirect_uri` en el grant implícito, que AniList rechaza (siempre usa la URL registrada del cliente). Ahora usa la misma URL que el escritorio.

## [0.1.0] - 2026-11-10

Primera versión pública de Tatakai Next (vista previa).

- Escritorio (Windows, Linux) y Android nativos, con extensiones (`.kai`, Aniyomi / Mihon / CloudStream vía servidor), manga, novelas, descargas y torrents.
- Cuenta = AniList (MyAnimeList opcional): listas, progreso automático, favoritos y social; sin cuenta, todo local.
- Reproductor con Anime4K, FSR y otros presets, deband, ajustes de imagen y generación de cuadros (lsfg-vk, Linux) con el preset de la app.
- Tatakai Link: emparejar el teléfono con la PC, continuar en la PC, control remoto y sesiones de mejora (Anime4K y LSFG ×2..×5) hacia el teléfono.
- Android firmado con la clave de lanzamiento de Tatakai: las próximas versiones se instalan encima.
