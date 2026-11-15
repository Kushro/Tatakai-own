# Changelog — Tatakai Next

Cambios de Tatakai Next (rama `main`). Tatakai Classic tiene su propio changelog en la rama `legacy`.

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
