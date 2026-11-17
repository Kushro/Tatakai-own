# lsfg-vk framegen (vendored)

This directory is vendored from two upstreams. Files are copied byte for byte, except the
two files listed under «Local modifications (Enigmata)». See design.md R13 and R13
«Enmienda 1» (E1).

## Primary upstream (64 of 68 files)

- Upstream: https://github.com/FrankBarretta/lsfg-vk-android (branch `release`)
- Commit: b55b1820c81294b62c787384462eaef9bb6d6e7d
- Subdirectory: `framegen/` (plus the repository `LICENSE.md`)
- License: MIT (see `LICENSE.md`, Copyright (c) 2025 lsfg-vk)

## Secondary upstream (4 files, replacing the b55b1820 versions)

- Upstream: https://github.com/amyroseee/LSFG-Android-A6xx
- Commit: f1c0e9ae5552d8c69cbfb504ff00461c989eb85b
- Subdirectory: `lsfg-vk-android/framegen/` (same relative paths as here)
- License: MIT

| File | Git blob | Change vs b55b1820 |
|------|----------|--------------------|
| `src/core/device.cpp` | 8f49e98dcdcb00f5a2870a1b81ff32848ef09f97 | Vulkan 1.1 minimum; timeline semaphore optional (enabled only if supported); `nullDescriptor` probed through `VK_EXT_robustness2` instead of assumed; the 1x1 descriptor fallback image is cleared and moved to `GENERAL` before use; Android required extensions reduced to the AHB extension (its KHR dependencies are core in 1.1); explicit check of the storage-image format features; capability log line |
| `include/core/device.hpp` | 54920e88591e195c4630f5e6ca5fcaddbd5c4722 | Records whether timeline semaphores are supported |
| `src/core/instance.cpp` | 507363ea223c84feb4a660e5135db53196f95c0a | Instance created with `VK_API_VERSION_1_1` instead of 1.3 |
| `src/common/utils.cpp` | e538f51ea4c8c2a346321abe4bfec01f936cc361 | `clearImage` adds a barrier (transfer write -> compute read) and leaves the image in `GENERAL` |

Why: these changes let the framegen core run on Vulkan 1.1 drivers such as Adreno 6xx.
That covers the experimental mode for untested GPUs added in design.md R13 «Enmienda 1».

## Local modifications (Enigmata)

Two of the A6xx files above carry a small Enigmata change (3.2.0-beta.3, unit FG-F,
item D4). Each edit is marked in the source with an «Enigmata modification» comment.
The license of these files stays MIT.

| File | Change vs A6xx f1c0e9a |
|------|------------------------|
| `src/core/instance.cpp` | Instance `apiVersion` = min(loader version from `vkEnumerateInstanceVersion` via volk, 1.3), with 1.1 as the floor (the previous fixed value) for 1.0 loaders. Adds `LSFG::Core::instanceApiVersion()` |
| `src/core/device.cpp` | The 1.2/1.3 feature chains are chosen with min(instance `apiVersion`, physical device `apiVersion`) instead of the device version alone; the «Device path» log line also prints both versions |

Why: the A6xx instance was created with 1.1 while device creation enabled the Vulkan 1.3
feature structs on 1.3 devices, which the Vulkan specification does not allow (the
usable version is the minimum of both). A Vulkan 1.1 device such as Adreno 6xx takes the
1.1 path exactly as before.

Not included: the root Vulkan layer (`src/`, `include/` at repo root), DXVK, dxbc, toml11,
and any `Lossless.dll` or extracted SPIR-V shaders. `CMakeLists.txt` is kept as a reference only.

## Files copied

All files come from b55b1820 except the four marked (A6xx f1c0e9a).

- `CMakeLists.txt`
- `LICENSE.md`
- `include/common/exception.hpp`
- `include/common/utils.hpp`
- `include/core/buffer.hpp`
- `include/core/commandbuffer.hpp`
- `include/core/commandpool.hpp`
- `include/core/descriptorpool.hpp`
- `include/core/descriptorset.hpp`
- `include/core/device.hpp` (A6xx f1c0e9a)
- `include/core/fence.hpp`
- `include/core/image.hpp`
- `include/core/instance.hpp`
- `include/core/pipeline.hpp`
- `include/core/sampler.hpp`
- `include/core/semaphore.hpp`
- `include/core/shadermodule.hpp`
- `include/pool/resourcepool.hpp`
- `include/pool/shaderpool.hpp`
- `public/lsfg_3_1.hpp`
- `public/lsfg_3_1p.hpp`
- `src/common/exception.cpp`
- `src/common/utils.cpp` (A6xx f1c0e9a)
- `src/core/buffer.cpp`
- `src/core/commandbuffer.cpp`
- `src/core/commandpool.cpp`
- `src/core/descriptorpool.cpp`
- `src/core/descriptorset.cpp`
- `src/core/device.cpp` (A6xx f1c0e9a, modified by Enigmata)
- `src/core/fence.cpp`
- `src/core/image.cpp`
- `src/core/instance.cpp` (A6xx f1c0e9a, modified by Enigmata)
- `src/core/pipeline.cpp`
- `src/core/sampler.cpp`
- `src/core/semaphore.cpp`
- `src/core/shadermodule.cpp`
- `src/pool/resourcepool.cpp`
- `src/pool/shaderpool.cpp`
- `v3.1_include/v3_1/context.hpp`
- `v3.1_include/v3_1/shaders/alpha.hpp`
- `v3.1_include/v3_1/shaders/beta.hpp`
- `v3.1_include/v3_1/shaders/delta.hpp`
- `v3.1_include/v3_1/shaders/gamma.hpp`
- `v3.1_include/v3_1/shaders/generate.hpp`
- `v3.1_include/v3_1/shaders/mipmaps.hpp`
- `v3.1_src/context.cpp`
- `v3.1_src/lsfg.cpp`
- `v3.1_src/shaders/alpha.cpp`
- `v3.1_src/shaders/beta.cpp`
- `v3.1_src/shaders/delta.cpp`
- `v3.1_src/shaders/gamma.cpp`
- `v3.1_src/shaders/generate.cpp`
- `v3.1_src/shaders/mipmaps.cpp`
- `v3.1p_include/v3_1p/context.hpp`
- `v3.1p_include/v3_1p/shaders/alpha.hpp`
- `v3.1p_include/v3_1p/shaders/beta.hpp`
- `v3.1p_include/v3_1p/shaders/delta.hpp`
- `v3.1p_include/v3_1p/shaders/gamma.hpp`
- `v3.1p_include/v3_1p/shaders/generate.hpp`
- `v3.1p_include/v3_1p/shaders/mipmaps.hpp`
- `v3.1p_src/context.cpp`
- `v3.1p_src/lsfg.cpp`
- `v3.1p_src/shaders/alpha.cpp`
- `v3.1p_src/shaders/beta.cpp`
- `v3.1p_src/shaders/delta.cpp`
- `v3.1p_src/shaders/gamma.cpp`
- `v3.1p_src/shaders/generate.cpp`
- `v3.1p_src/shaders/mipmaps.cpp`
