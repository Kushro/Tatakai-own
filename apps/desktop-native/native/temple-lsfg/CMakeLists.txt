# SPDX-License-Identifier: MIT
#
# temple-lsfg: LSFG frame generation helper used by Tatakai Link (Windows with MSVC, Linux with GCC/Clang).
# Built by packaging/linux/build-temple-lsfg.sh (Linux, output temple-lsfg) and by the Windows job of
# .github/workflows/next-release.yml (output temple-lsfg.exe); both ship it next to tatakai-next.
cmake_minimum_required(VERSION 3.20)

project(temple-lsfg LANGUAGES C CXX)

if(NOT WIN32 AND NOT CMAKE_SYSTEM_NAME STREQUAL "Linux")
    message(FATAL_ERROR "temple-lsfg supports Windows (MSVC) and Linux only")
endif()

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)
if(MSVC)
    # Static CRT: the helper ships next to tatakai-next.exe and must not need the VC++ redistributable.
    set(CMAKE_MSVC_RUNTIME_LIBRARY "MultiThreaded$<$<CONFIG:Debug>:Debug>")
endif()

if(NOT CMAKE_BUILD_TYPE AND NOT CMAKE_CONFIGURATION_TYPES)
    set(CMAKE_BUILD_TYPE Release)
endif()

set(TP "${CMAKE_CURRENT_SOURCE_DIR}/third_party")

if(MSVC)
    add_compile_definitions(NOMINMAX _CRT_SECURE_NO_WARNINGS)
    add_compile_options(/utf-8 /EHsc /permissive-)
else()
    # Linux: Release gets -O2 from CMake; the vendored sources are not warning-clean, so no -Werror.
    set(CMAKE_POSITION_INDEPENDENT_CODE ON)
endif()

# std::thread needs pthreads on Linux; Windows uses its own threads (no extra library).
set(TEMPLE_THREADS "")
if(NOT WIN32)
    set(THREADS_PREFER_PTHREAD_FLAG ON)
    find_package(Threads REQUIRED)
    set(TEMPLE_THREADS Threads::Threads)
endif()

# volk: Vulkan meta-loader (MIT). Vulkan headers: vendored, Apache-2.0.
add_library(volk STATIC "${TP}/volk/volk.c")
target_include_directories(volk PUBLIC "${TP}/volk" "${TP}/vulkan-headers/include")
target_compile_definitions(volk PUBLIC VK_NO_PROTOTYPES)
if(NOT WIN32)
    # volk dlopen()s libvulkan.so.1 at run time; there is no link-time dependency on Vulkan.
    target_link_libraries(volk PUBLIC ${CMAKE_DL_LIBS})
endif()

# pe-parse (MIT) without ICU: unicode_win.cpp / unicode_posix.cpp provide peparse::from_utf16.
if(WIN32)
    set(TEMPLE_UNICODE_SRC "${CMAKE_CURRENT_SOURCE_DIR}/src/unicode_win.cpp")
else()
    set(TEMPLE_UNICODE_SRC "${CMAKE_CURRENT_SOURCE_DIR}/src/unicode_posix.cpp")
endif()
add_library(peparse STATIC
    "${TP}/pe-parse/src/buffer.cpp"
    "${TP}/pe-parse/src/parse.cpp"
    "${TEMPLE_UNICODE_SRC}")
target_include_directories(peparse PUBLIC "${TP}/pe-parse/include")

# lsfg-vk framegen core (MIT), both the 3.1 and the 3.1P (performance) variants.
set(LSFG "${TP}/lsfg-vk")
file(GLOB LSFG_SOURCES CONFIGURE_DEPENDS
    "${LSFG}/src/common/*.cpp"
    "${LSFG}/src/core/*.cpp"
    "${LSFG}/src/pool/*.cpp"
    "${LSFG}/v3.1_src/*.cpp"
    "${LSFG}/v3.1_src/shaders/*.cpp"
    "${LSFG}/v3.1p_src/*.cpp"
    "${LSFG}/v3.1p_src/shaders/*.cpp")
add_library(lsfg_framegen STATIC ${LSFG_SOURCES})
target_include_directories(lsfg_framegen
    PUBLIC "${LSFG}/include" "${LSFG}/public"
    PRIVATE "${LSFG}/v3.1_include" "${LSFG}/v3.1p_include")
target_compile_definitions(lsfg_framegen PUBLIC TEMPLE_LSFG_LOCAL)
target_link_libraries(lsfg_framegen PUBLIC volk ${TEMPLE_THREADS})

add_executable(temple-lsfg
    src/main.cpp
    src/shader_loader.cpp)
target_link_libraries(temple-lsfg PRIVATE lsfg_framegen peparse volk ${TEMPLE_THREADS})

