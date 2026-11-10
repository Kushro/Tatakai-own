# Mejoras de video: inventario Enigmata / clásico / Next (2026-11-09)

Pedido del usuario: que el reproductor de escritorio de Next tenga toda la batería de mejoras de Enigmata (incluida la generación de cuadros) y el "mpv mejorado". Pausado a pedido del usuario hasta cerrar los arreglos de UI (`17-ux-review-and-ui-styles.md`, fase F0).

Fuentes: `Enigmata` (Android, 3.6.0 del 2026-10-28) y `Enigmata-Temple-linux` (servidor de PC en Rust, 0.7.0, LSFG/libplacebo). En la máquina del usuario: `lsfg-vk 2.0.0` como capa implícita (`/usr/lib/liblsfg-vk-layer.so`, Lossless Scaling en Steam); el `mpv 0.41` de Arch no trae `vf=vapoursynth`.

**El clásico no tiene "mpv mejorado"**: lanza un mpv externo pelado (`desktop/services/external-player.cjs`) y aplica Anime4K en WebGL2 (`src/lib/video/anime4k/Anime4KRenderer.ts`). El mpv mejorado existe en **Next**: modo "Integrado mejorado" (mpv-anime-build v5.6 + uosc).

| Mejora | Enigmata | Next hoy | Port (esfuerzo, riesgo) |
|---|---|---|---|
| Generación de cuadros LSFG ×2..×5 | lsfg-vk vendorizado + motor Vulkan propio (`app/src/main/cpp/fg/*`), perfiles SAVER/BALANCED/QUALITY/CUSTOM, Auto por fps objetivo, `Lossless.dll` del usuario → SPIR-V | falta | **L, alto** embebido (GLArea es OpenGL; hace falta interop GL↔Vulkan + núcleo de `Enigmata-Temple-linux/native/temple-lsfg` + pacing). **S atajo**: modo mpv externo con `vo=gpu-next gpu-api=vulkan` + lsfg-vk 2.0 (Linux). Windows: sin capa |
| Interpolación ("Suavizado") | `display-resample` + `tscale=oversample` + `display-fps-override` con los Hz reales | parcial (`mpv_player.rs`: solo `interpolation` + `video-sync`) | S; en Linux el render API no reporta swaps |
| Anime4K completo (A/B/C/A+A/B+B/C+A × S/M/L + Deblur/Darken/Thin) | `Anime4KManager.kt`, FP16 opcional | parcial (A, A+A, B, C en S/M) | S/M: ampliar `ShaderPreset` (modo × calidad) y el lock |
| ArtCNN (compute) | manual, gpu-next + Vulkan, pesos CC BY-NC | falta (viene en el zip de mpv-anime-build) | S; verificar `//!COMPUTE` en el `vo=libmpv` GL de Linux; licencia NC en NOTICES |
| FSRCNNX, KrigBilateral, SSimSuperRes/Downscaler, adaptive-sharpen (motor Fidelity) | `ShaderChainManager.kt` por tier | falta (solo FSR) | S: presets + lock (o tomar del zip de mpv-anime-build) |
| Motor de calidad automático (anime/live, tiers, bajada por cuadros perdidos) | `domain/quality/*` | falta | M: lógica pura portable a Rust, leer `frame-drop-count` |
| Deband (GPU / gradfun) | `QualityRuntime.kt` | falta | S |
| Filtros brillo/contraste/saturación/gamma/tono/nitidez | `VideoSettingsFilterPresetsCard.kt` | falta | S |
| HDR hdr-toys (PQ/HLG/BT.2020) | `domain/hdr/*`, exige gpu-next + Vulkan | falta | M, riesgoso en Linux embebido (GL, no gpu-next) |
| Posprocesado (Natural, Vívido, Anime, Cel, Cine…) | shader generado (`PostProcessingShaderBuilder.kt`) | falta | M |
| Ambient (glow) | shader con `video-scale-x/y` | falta | M |
| Autocrop | `cropdetect` → `video-crop` | falta | M, opcional |
| Modos de mpv-anime-build sin uosc | — | con nuestros controles no se pueden cambiar (`script-message anime4k-mode …` solo en su `input.conf`) | S: menú que mande esos `script-message` |
| Shaders en perfil nuevo | assets del APK | **hecho** (`tk-app/src/shaders.rs`, commit `ab7bad06`): descarga verificada en el primer uso; sin probar en la app | generalizar a los locks nuevos |

## Orden recomendado

1. Probar en la app la descarga de shaders y extenderla a varios locks (o alimentarla desde el zip de mpv-anime-build).
2. Presets: Anime4K completo, Fidelity, CAS/NVScaler, ArtCNN (S/M).
3. Deband, filtros de imagen y Suavizado completo (S).
4. Menú de modos de mpv-anime-build sin uosc (S).
5. Generación de cuadros, atajo: "mpv externo + LSFG" con Vulkan y lsfg-vk (S, Linux).
6. Motor de calidad automático (M).
7. Posprocesado y ambient (M).
8. HDR (M, riesgoso).
9. Generación de cuadros embebida (L): spike en la RX 9070 XT primero.

A verificar antes de planificar: el embed de Linux fuerza `vo=libmpv` (renderer `gpu` sobre GL, no gpu-next), lo que afecta a hdr-toys PQ/HLG y quizá a los shaders compute; en una PC híbrida, confirmar que GL y Vulkan usen la GPU discreta.

## Enigmata Temple como proxy de procesamiento (revisión 2026-11-10)

Temple (`Enigmata-Temple-linux` v0.7.0; contrato `docs/contract/v1.md`) es un servidor HTTPS/mTLS con emparejamiento por QR (`POST /v1/pairing`), `GET /v1/capabilities` (perfiles, LSFG), `POST /v1/sessions` y salida HLS VOD fMP4 (H.264 + AAC, SDR, segmentos de 2 s, seek nativo, una pista de audio por sesión, subtítulos WebVTT). Sus fuentes son solo `phone` (el cliente le sirve rangos de **un archivo único con Range**), `library` (carpetas de la PC) y `default`: **no acepta URL ni HLS**, y la mayoría de las fuentes de extensiones son HLS. Perfiles en Linux: VAAPI, libplacebo, Anime4K, `minterpolate` y LSFG ×2..×5 / auto120 (`temple-lsfg`, núcleo propio, no la capa lsfg-vk).

Tatakai Link (`tk-link`) copia el mismo modelo y ya tiene lo que a Temple le falta (fuente `url` con headers y reescritura de HLS), pero hoy fija `-hwaccel d3d11va` y no tiene VAAPI ni LSFG.

Opciones, de mejor a peor:
1. **Llevar a tk-link el pipeline de Temple** (VAAPI, LSFG con `temple-lsfg`, tuning): un solo servidor, HLS y headers se quedan en Next, el teléfono lo recibe por Link. Mucho trabajo; corrige de paso el d3d11va fijo.
2. **Android de Next como cliente de Temple** (como Enigmata): sirve hoy para MP4 directos y torrents; HLS no entra.
3. **Escritorio "Procesar con Temple"**: en la misma PC aporta poco (el escritorio ya hace Anime4K y lsfg-vk en tiempo real, Temple recodifica a H.264).
4. **Link delega en Temple**: descartado salvo que Temple sume la fuente `url`.

Lo que destrabaría 2-4: una fuente `{"kind":"url", url, headers}` en Temple (portando `tk-link/src/fetcher.rs`), aceptando que Temple vea las URLs y headers de las extensiones.
