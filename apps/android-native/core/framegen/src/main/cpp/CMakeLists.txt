# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Tatakai Next 0.4.0: phone frame generation (LSFG) native library, libtatakai_fg.so.
# Ported from Enigmata app/src/main/cpp/CMakeLists.txt (only the FG part, `libenigmata_fg.so` renamed); the targets,
# source lists, definitions and link options are the same.
#
# - ABIs: only arm64-v8a and x86_64. On the 32-bit ABIs nothing below is built, System.loadLibrary("tatakai_fg") fails
#   and FgNative.isAvailable() is false: the controller reports FgUnavailableReason.ABI.
# - VK_NO_PROTOTYPES is a PUBLIC definition of tatakai_volk, so it reaches every target that links it: framegen's public
#   lsfg_3_1.hpp includes vulkan/vulkan_core.h directly and, without the define, those prototypes would clash with volk's
#   function pointers. Framegen calls volkLoadDevice and overwrites volk's globals, so our code only uses VolkDeviceTable
#   and cached instance pointers (efg::FgDevice).
# - STL: AGP's default c++_static. The libmpv AAR ships its own libc++_shared.so, so -Wl,--exclude-libs,ALL keeps the
#   static libc++ and the framegen/volk/pe-parse symbols out of our dynamic symbol table; only JNI_OnLoad is reached
#   (RegisterNatives).
cmake_minimum_required(VERSION 3.22.1)

project("tatakai_fg" C CXX)

set(TFG_RELEASE_COMPILE_OPTIONS
    "$<$<CONFIG:Release>:-ffunction-sections>"
    "$<$<CONFIG:Release>:-fdata-sections>"
    "$<$<CONFIG:Release>:-fvisibility=hidden>"
)

set(TFG_LINK_OPTIONS
    "-Wl,-z,max-page-size=16384"
    "$<$<CONFIG:Release>:-Wl,-O1>"
    "$<$<CONFIG:Release>:-Wl,--gc-sections>"
    "$<$<CONFIG:Release>:-Wl,--icf=safe>"
)

if(ANDROID_ABI STREQUAL "arm64-v8a" OR ANDROID_ABI STREQUAL "x86_64")
    set(TFG_THIRD_PARTY "${CMAKE_CURRENT_SOURCE_DIR}/third_party")
    set(TFG_LSFG_DIR "${TFG_THIRD_PARTY}/lsfg-vk")

    # volk: Vulkan meta-loader (MIT), compiled directly from volk.c.
    add_library(tatakai_volk STATIC "third_party/volk/volk.c")
    target_include_directories(tatakai_volk PUBLIC "${TFG_THIRD_PARTY}/volk")
    target_compile_definitions(tatakai_volk PUBLIC VK_NO_PROTOTYPES VK_USE_PLATFORM_ANDROID_KHR=1)
    set_target_properties(
        tatakai_volk
        PROPERTIES
        C_VISIBILITY_PRESET hidden
        POSITION_INDEPENDENT_CODE ON
    )

    # lsfg-vk framegen (MIT), explicit source list (no GLOB) matching the vendored tree at b55b1820.
    add_library(
        lsfg_framegen
        STATIC
        "third_party/lsfg-vk/src/common/exception.cpp"
        "third_party/lsfg-vk/src/common/utils.cpp"
        "third_party/lsfg-vk/src/core/buffer.cpp"
        "third_party/lsfg-vk/src/core/commandbuffer.cpp"
        "third_party/lsfg-vk/src/core/commandpool.cpp"
        "third_party/lsfg-vk/src/core/descriptorpool.cpp"
        "third_party/lsfg-vk/src/core/descriptorset.cpp"
        "third_party/lsfg-vk/src/core/device.cpp"
        "third_party/lsfg-vk/src/core/fence.cpp"
        "third_party/lsfg-vk/src/core/image.cpp"
        "third_party/lsfg-vk/src/core/instance.cpp"
        "third_party/lsfg-vk/src/core/pipeline.cpp"
        "third_party/lsfg-vk/src/core/sampler.cpp"
        "third_party/lsfg-vk/src/core/semaphore.cpp"
        "third_party/lsfg-vk/src/core/shadermodule.cpp"
        "third_party/lsfg-vk/src/pool/resourcepool.cpp"
        "third_party/lsfg-vk/src/pool/shaderpool.cpp"
        "third_party/lsfg-vk/v3.1_src/context.cpp"
        "third_party/lsfg-vk/v3.1_src/lsfg.cpp"
        "third_party/lsfg-vk/v3.1_src/shaders/alpha.cpp"
        "third_party/lsfg-vk/v3.1_src/shaders/beta.cpp"
        "third_party/lsfg-vk/v3.1_src/shaders/delta.cpp"
        "third_party/lsfg-vk/v3.1_src/shaders/gamma.cpp"
        "third_party/lsfg-vk/v3.1_src/shaders/generate.cpp"
        "third_party/lsfg-vk/v3.1_src/shaders/mipmaps.cpp"
        "third_party/lsfg-vk/v3.1p_src/context.cpp"
        "third_party/lsfg-vk/v3.1p_src/lsfg.cpp"
        "third_party/lsfg-vk/v3.1p_src/shaders/alpha.cpp"
        "third_party/lsfg-vk/v3.1p_src/shaders/beta.cpp"
        "third_party/lsfg-vk/v3.1p_src/shaders/delta.cpp"
        "third_party/lsfg-vk/v3.1p_src/shaders/gamma.cpp"
        "third_party/lsfg-vk/v3.1p_src/shaders/generate.cpp"
        "third_party/lsfg-vk/v3.1p_src/shaders/mipmaps.cpp"
    )
    set_target_properties(
        lsfg_framegen
        PROPERTIES
        CXX_STANDARD 20
        CXX_STANDARD_REQUIRED ON
        CXX_VISIBILITY_PRESET hidden
        POSITION_INDEPENDENT_CODE ON
    )
    target_include_directories(
        lsfg_framegen
        PUBLIC "${TFG_LSFG_DIR}/include" "${TFG_LSFG_DIR}/public"
        PRIVATE "${TFG_LSFG_DIR}/v3.1_include" "${TFG_LSFG_DIR}/v3.1p_include"
    )
    # framegen uses AHardwareBuffer (android) and __android_log (log).
    target_link_libraries(lsfg_framegen PUBLIC tatakai_volk android log)

    # pe-parse (MIT) without ICU: fg/fg_unicode.cpp provides peparse::from_utf16.
    add_library(
        tatakai_peparse
        STATIC
        "third_party/pe-parse/src/buffer.cpp"
        "third_party/pe-parse/src/parse.cpp"
        "fg/fg_unicode.cpp"
    )
    target_include_directories(tatakai_peparse PUBLIC "${TFG_THIRD_PARTY}/pe-parse/include")
    set_target_properties(
        tatakai_peparse
        PROPERTIES
        CXX_STANDARD 17
        CXX_STANDARD_REQUIRED ON
        POSITION_INDEPENDENT_CODE ON
    )

    # JNI library loaded by app.tatakai.next.framegen.FgNative.
    add_library(
        tatakai_fg
        SHARED
        "fg/fg_jni.cpp"
        "fg/fg_device.cpp"
        "fg/ahb_image.cpp"
        "fg/image_reader_source.cpp"
        "fg/fg_engine.cpp"
        "fg/fg_presenter.cpp"
        "fg/fg_pacing.cpp"
        "fg/fg_shader_loader.cpp"
    )
    set_target_properties(
        tatakai_fg
        PROPERTIES
        CXX_STANDARD 20
        CXX_STANDARD_REQUIRED ON
    )
    target_link_libraries(
        tatakai_fg
        PRIVATE
        lsfg_framegen
        tatakai_peparse
        tatakai_volk
        android
        log
        mediandk
        nativewindow
    )
    target_link_options(tatakai_fg PRIVATE "-Wl,--exclude-libs,ALL")
    target_compile_options(tatakai_fg PRIVATE ${TFG_RELEASE_COMPILE_OPTIONS})
    target_link_options(tatakai_fg PRIVATE ${TFG_LINK_OPTIONS})
endif()
