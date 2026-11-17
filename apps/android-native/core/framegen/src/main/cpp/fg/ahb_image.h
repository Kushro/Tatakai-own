// SPDX-License-Identifier: GPL-3.0-only
//
// Ported from Enigmata (app/src/main/cpp/fg) to Tatakai Next 0.4.0 (core/framegen): JNI class, library name,
// log tags and debug properties renamed; no other change.
//
// Adapted from FrankBarretta/LSFG-Android-Application
// (commit b84754199823615d32d65fe33ea59481dca88dcf),
// app/src/main/cpp/ahb_image_bridge.hpp, GPL-3.0.
// Modifications: Enigmata, AGPL-3.0-or-later (combined under GPL-3.0 section 13).
//
// Enigmata frame generation (R13): AHardwareBuffer <-> VkImage bridge on our
// device (VK_ANDROID_external_memory_android_hardware_buffer), without the
// OPAQUE_FD export: images are shared with framegen as AHardwareBuffers
// (createContextFromAHB).
#pragma once

#include "fg_device.h"

#include <android/hardware_buffer.h>
#include <volk.h>

#include <cstdint>

namespace efg {

// VkImage + dedicated VkDeviceMemory bound to an AHardwareBuffer. RAII (I2):
// the destructor (or reset()) destroys the image and memory with dev->fn and
// releases the AHB when ownsAhb. Move-only; must not outlive `dev`.
struct AhbImage {
    FgDevice* dev = nullptr;
    AHardwareBuffer* ahb = nullptr;  // released on reset() iff ownsAhb
    bool ownsAhb = false;
    VkImage image = VK_NULL_HANDLE;
    VkDeviceMemory memory = VK_NULL_HANDLE;
    VkExtent2D extent{0, 0};
    VkFormat format = VK_FORMAT_UNDEFINED;
    // formatFeatures the driver reported for the AHB (0 = not reported). The
    // engine checks BLIT_SRC / BLIT_DST here before a format-converting blit.
    VkFormatFeatureFlags features = 0;

    AhbImage() = default;
    ~AhbImage();
    AhbImage(const AhbImage&) = delete;
    AhbImage& operator=(const AhbImage&) = delete;
    AhbImage(AhbImage&& other) noexcept;
    AhbImage& operator=(AhbImage&& other) noexcept;

    explicit operator bool() const { return image != VK_NULL_HANDLE; }

    // Safe on a partially initialized or empty image; idempotent.
    void reset();
};

// Creates the VkImage and imports the memory of out.ahb (already set by the
// caller together with ownsAhb). Common path of the two functions below.
// Returns FgResult::OK or VK_INIT; on failure `out` is reset()-safe.
int32_t wrapAhb(FgDevice& dev, AhbImage& out);

// Allocates a new AHardwareBuffer (w x h, fmt; GPU_SAMPLED_IMAGE |
// GPU_COLOR_OUTPUT) owned by `out` and wraps it.
int32_t createAhbImage(FgDevice& dev, uint32_t w, uint32_t h, VkFormat fmt, AhbImage& out);

// Wraps an external AHardwareBuffer (e.g. from the AImageReader) without
// taking ownership; the caller keeps it alive while `out` is in use.
int32_t importAhbImage(FgDevice& dev, AHardwareBuffer* ahb, AhbImage& out);

}  // namespace efg
