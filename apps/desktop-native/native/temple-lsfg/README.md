# temple-lsfg

Helper de **Tatakai Link** (Windows y Linux) que hace **generación de cuadros LSFG** (Lossless Scaling Frame
Generation) con Vulkan sobre la GPU de la PC (en Linux, con la `libvulkan.so.1` del sistema). Lee cuadros RGBA
crudos por stdin y escribe por stdout los cuadros intermedios. Link lo lanza entre dos FFmpeg (decodificación y
escalado, `temple-lsfg`, codificación) en los perfiles LSFG (`crates/tk-link/src/media/lsfg.rs`); no se usa a
mano salvo para pruebas.

Procedencia: fork del helper `native/temple-lsfg` de Enigmata Temple (commit `34ff76c`), del mismo autor. Desde
acá evoluciona por su cuenta, sin sincronización con el original.

Usa el motor [lsfg-vk](https://github.com/FrankBarretta/lsfg-vk-android) (core de framegen, MIT), que ejecuta
sobre Vulkan los shaders SPIR-V precompilados que Lossless Scaling trae en su `Lossless.dll`.

## La DLL la pone la persona

`Lossless.dll` es de Lossless Scaling (Steam) y **no se copia al repositorio ni a los paquetes**. La indica la
persona (ajuste «Lossless.dll» del reproductor) o Link la busca en las carpetas de Steam. El helper la abre sólo
para lectura, extrae en memoria los shaders (RT_RCDATA 353..400 FP32 y 304..351 FP16), reescribe sus bindings y
no deja nada en disco.

## Dónde lo busca Link

Junto al ejecutable (`tatakai-next`) o una carpeta más arriba, como `temple-lsfg` (Linux) o `temple-lsfg.exe`
(Windows), o en la ruta absoluta de la variable `TEMPLE_LSFG` (desarrollo). Los paquetes lo dejan junto al
ejecutable: `/usr/lib/tatakai-next/temple-lsfg` (PKGBUILD y AppImage) y `<instalación>\temple-lsfg.exe`
(instalador y zip de Windows).

## Línea de comandos

```
temple-lsfg --probe --dll <Lossless.dll>
temple-lsfg --dll <Lossless.dll> --width W --height H --multiplier N [--flow-scale F] [--performance]
```

- `--probe`: imprime **una línea JSON** y sale. `{"ok":true,"device":"<GPU>","fp16":true|false}` (exit 0) o
  `{"ok":false,"error":"<código>"}` (exit 2). Códigos: `dll_unreadable`, `missing_spirv`, `no_vulkan`,
  `device_unsupported`. Crea de verdad el dispositivo Vulkan de LSFG con la GPU elegida.
- Ejecución: stdin recibe cuadros RGBA de 8 bits (`W*H*4` bytes cada uno, cadencia constante) hasta EOF.
  stdout recibe el cuadro 0 tal cual y, por cada cuadro k >= 1, los `N-1` intermedios entre k-1 y k y luego
  el cuadro k: `(n-1)*N + 1` cuadros del mismo tamaño. `--multiplier N` va de 2 a 5; `--flow-scale F` en
  (0, 1] (por defecto 1.0; LSFG lo limita a 0.25 por abajo); W y H pares y >= 64. `--performance` usa la
  variante 3.1P (más liviana).
- stderr: una línea por evento (`device`, `ready`, `done`, `error`), sin rutas del usuario.
- Códigos de salida: 0 bien; 1 uso incorrecto; 2 error de arranque (los cuatro códigos de arriba, también
  al arrancar la ejecución); 3 error de GPU en ejecución (`device_lost`, `lsfg_failed`); 4 entrada truncada
  (`truncated_frame`); 5 stdout cerrado (`output_closed`).

GPU elegida: dispositivo físico de hardware (nunca CPU), con prioridad discreta, integrada, virtual y, a
igualdad, mayor heap local. Precisión: FP16 si la GPU soporta `shaderFloat16` y la DLL trae el juego FP16
completo; si no, FP32. Para depurar, `TEMPLE_LSFG_PRECISION=fp32` fuerza FP32 y `TEMPLE_LSFG_PROFILE=1` imprime
en stderr cuánto esperó cada etapa (lectura de stdin, GPU con hasta 3 cuadros en vuelo, escritura de stdout).

## Compilar

Requiere `cmake` (3.20+), un compilador de C++20 y, opcionalmente, `ninja`. No hacen falta las cabeceras de
Vulkan: volk y las cabeceras van incluidas y el cargador de Vulkan sólo se necesita al ejecutar.

- **Linux** (GCC 11+ o Clang 14+): `packaging/linux/build-temple-lsfg.sh [--out <carpeta>]` deja
  `target/temple-lsfg/temple-lsfg` e imprime su ruta. `build-appimage.sh` y el PKGBUILD lo llaman solos.
- **Windows** (Visual Studio 2022 con C++ x64, desde una consola de desarrollador):
  `cmake -S native/temple-lsfg -B target/temple-lsfg -G Ninja -DCMAKE_BUILD_TYPE=Release` y
  `cmake --build target/temple-lsfg`. CRT estático: sólo necesita `vulkan-1.dll` del controlador. En CI lo
  compila el paso «temple-lsfg» del job de Windows de `.github/workflows/next-release.yml`.

## Licencias

- Código propio (`src/main.cpp`, `src/unicode_win.cpp`, `src/unicode_posix.cpp`, `CMakeLists.txt`): MIT
  ([`LICENSE`](LICENSE)). lsfg-vk, volk y pe-parse: MIT, cada uno con su archivo en `third_party/`.
- Cabeceras de Vulkan (Khronos): Apache-2.0.
- `src/shader_loader.*`: GPL-3.0-only ([`LICENSE-GPL-3.0.txt`](LICENSE-GPL-3.0.txt)), adaptado del cargador de
  LSFG-Android-Application. El binario `temple-lsfg` es por eso una obra combinada bajo GPL-3.0; este directorio
  es su código fuente correspondiente.
- Procedencia exacta de cada parte y lista de modificaciones: [`third_party/README.md`](third_party/README.md).
- Lossless Scaling y su `Lossless.dll` son de sus autores; el helper no los redistribuye.
