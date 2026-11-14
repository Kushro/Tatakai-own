# Código de terceros de temple-lsfg (procedencia)

Todo lo de esta carpeta llegó con el fork de temple-lsfg (Enigmata Temple, commit `34ff76c`), que a su vez
lo tomó de Enigmata (otra app del mismo autor), copiado de los orígenes que siguen. Nada se descargó de
internet para esta copia. No hay `Lossless.dll` ni shaders
extraídos en el repositorio: el auxiliar los lee en memoria de la DLL que tiene la persona.

## `lsfg-vk/` (MIT, Copyright (c) 2025 lsfg-vk; ver `lsfg-vk/LICENSE.md`)

- Origen principal: <https://github.com/FrankBarretta/lsfg-vk-android>, rama `release`, commit
  `b55b1820c81294b62c787384462eaef9bb6d6e7d`, subcarpeta `framegen/` (64 de 68 archivos).
- Origen secundario (4 archivos que reemplazan a los de b55b1820): <https://github.com/amyroseee/LSFG-Android-A6xx>,
  commit `f1c0e9ae5552d8c69cbfb504ff00461c989eb85b` (`src/core/device.cpp`, `include/core/device.hpp`,
  `src/core/instance.cpp`, `src/common/utils.cpp`; MIT).
- Modificaciones de Enigmata (sobre `src/core/instance.cpp` y `src/core/device.cpp`, marcadas «Enigmata
  modification»): versión de API de la instancia = min(cargador, 1.3) y cadenas de features con
  min(instancia, dispositivo).
- No incluidos: la capa Vulkan raíz, DXVK, dxbc, toml11, `CMakeLists.txt` (se reemplaza por el de
  `native/temple-lsfg/`) y las notas internas de Enigmata.

### Modificaciones de Temple (MIT, marcadas «Temple modification» en el código)

| Archivo | Cambio |
| --- | --- |
| `public/lsfg_3_1.hpp`, `public/lsfg_3_1p.hpp` | Macro de exportación portable `TEMPLE_LSFG_API` en lugar de `__attribute__((visibility("default")))` (MSVC no lo acepta); `waitIdle` y la API local también con `TEMPLE_LSFG_LOCAL`; declaración de `createContextLocal` |
| `public/temple_local.hpp` (nuevo) | `TempleLsfg::LocalContext`: handles de Vulkan del dispositivo, la cola y las imágenes del contexto |
| `include/common/temple_local_impl.hpp`, `src/common/temple_local.cpp` (nuevos) | Crean las imágenes locales (RGBA8, `STORAGE`, `SAMPLED`, `TRANSFER_SRC`, `TRANSFER_DST`) y las pasan a `GENERAL` una vez, actualizando el seguimiento de layout |
| `v3.1_include/v3_1/context.hpp`, `v3.1p_include/v3_1p/context.hpp`, `v3.1_src/context.cpp`, `v3.1p_src/context.cpp` | Constructor de `Context` sobre imágenes ya creadas (equivalente al de AHardwareBuffer de Android, sin compartir memoria) |
| `v3.1_src/lsfg.cpp`, `v3.1p_src/lsfg.cpp` | `createContextLocal`; `waitIdle` disponible con `TEMPLE_LSFG_LOCAL` |
| `src/core/device.cpp` | En Windows y con `TEMPLE_LSFG_LOCAL` (también Linux) no se exigen `VK_KHR_external_memory_fd` ni `VK_KHR_external_semaphore_fd` |

Se compila con `TEMPLE_LSFG_LOCAL`. Las rutas de fd (`createContext`) y de semáforos importados quedan en el
código pero no se usan.

## `volk/` (MIT, Copyright (c) 2018-2025 Arseny Kapoulkine; ver `volk/LICENSE.md`)

- <https://github.com/zeux/volk>, commit `be3dbd49bf77052665e96b6c7484af855e7e5f67` (el que fija
  lsfg-vk-android). Copia sin modificaciones: `volk.c`, `volk.h`, `LICENSE.md`.

## `pe-parse/` (MIT, Copyright (c) 2013 Andrew Ruef; ver `pe-parse/LICENSE`)

- <https://github.com/trailofbits/pe-parse>, commit `31ac5966503689d5693cd9fb520bd525a8710e17`. Copia sin
  modificaciones de `include/pe-parse/*`, `src/buffer.cpp` y `src/parse.cpp`. Sin ICU: el símbolo
  `peparse::from_utf16` lo aporta `../src/unicode_win.cpp` en Windows y `../src/unicode_posix.cpp` en Linux (código propio, MIT).

## `vulkan-headers/` (Apache-2.0, Copyright 2015-2024 The Khronos Group Inc.)

- Cabeceras `vulkan/vulkan_core.h`, `vulkan/vk_platform.h`, `vulkan/vulkan_win32.h` y `vk_video/*.h`
  (`VK_HEADER_VERSION` 275), copiadas del NDK de Android r28 (`28.2.13676358`, sysroot). Son las cabeceras
  de Khronos sin cambios; el texto de la licencia está en `vulkan-headers/LICENSE-Apache-2.0.txt`
  (extraído del archivo `NOTICE` del NDK). Origen oficial: <https://github.com/KhronosGroup/Vulkan-Headers>.

## Código propio y licencias del auxiliar

- `src/main.cpp`, `src/unicode_win.cpp`, `src/unicode_posix.cpp`, `CMakeLists.txt`: código de Temple, MIT.
- `src/shader_loader.{h,cpp}`: adaptación del cargador de shaders de Enigmata, que a su vez adapta
  `android_shader_loader.cpp` de <https://github.com/FrankBarretta/LSFG-Android-Application>
  (commit `b84754199823615d32d65fe33ea59481dca88dcf`, **GPL-3.0**). Por eso esos dos archivos están bajo
  GPL-3.0-only y el `temple-lsfg` (`.exe` en Windows) resultante es una obra combinada bajo GPL-3.0 (las partes MIT son
  compatibles). Quien lo redistribuya debe ofrecer el código fuente correspondiente (este directorio).
