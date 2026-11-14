// SPDX-License-Identifier: MIT
//
// Temple modification: new file (see public/temple_local.hpp).
#pragma once

#include "common/utils.hpp"
#include "core/image.hpp"
#include "temple_local.hpp"

#include <vulkan/vulkan_core.h>

#include <cstdint>
#include <vector>

namespace TempleLsfg {

    struct LocalImages {
        LSFG::Core::Image in0;
        LSFG::Core::Image in1;
        std::vector<LSFG::Core::Image> out;
    };

    /// Creates the input pair and the vk.generationCount output images (RGBA, device local,
    /// transfer capable) and moves them to GENERAL, updating the layout tracking.
    ///
    /// @throws LSFG::vulkan_error
    LocalImages createLocalImages(LSFG::Vulkan& vk, VkExtent2D extent, VkFormat format);

    /// Fills the public description of a context built on `images`.
    LocalContext describeLocal(LSFG::Vulkan& vk, const LocalImages& images, int32_t id);

}
