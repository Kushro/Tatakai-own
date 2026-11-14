// SPDX-License-Identifier: MIT
//
// Temple modification: new file. Local (non-shared) image API used by temple-lsfg on Windows,
// where the Android AHardwareBuffer / opaque fd sharing does not apply. The caller gets the
// Vulkan handles of the framegen device and of the images the context was built on, and moves
// pixels with its own staging buffers on the same device and queue.
#pragma once

#include <vulkan/vulkan_core.h>

#include <cstdint>
#include <vector>

#ifndef TEMPLE_LSFG_API
#define TEMPLE_LSFG_API
#endif

namespace TempleLsfg {

    /// Handles of a context created with createContextLocal. All images are
    /// VK_IMAGE_USAGE_STORAGE | SAMPLED | TRANSFER_SRC | TRANSFER_DST, left in
    /// VK_IMAGE_LAYOUT_GENERAL and owned by the context (valid until deleteContext/finalize).
    struct LocalContext {
        int32_t id{};
        VkDevice device{};
        VkPhysicalDevice physicalDevice{};
        VkQueue queue{};
        uint32_t queueFamily{};
        VkImage in0{};
        VkImage in1{};
        std::vector<VkImage> out; // one per generated frame, in temporal order
    };

}
