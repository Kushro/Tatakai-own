# Changelog — Tatakai Next

Cambios de Tatakai Next (rama `main`). Tatakai Classic tiene su propio changelog en la rama `legacy`.

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
