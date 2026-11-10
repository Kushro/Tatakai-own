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
