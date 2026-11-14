// SPDX-License-Identifier: MIT
//
// Temple modification: new file (see public/temple_local.hpp).
#include <volk.h>
#include <vulkan/vulkan_core.h>

#include "common/temple_local_impl.hpp"
#include "common/exception.hpp"
#include "core/commandbuffer.hpp"
#include "core/fence.hpp"

#include <cstdint>
#include <vector>

using namespace TempleLsfg;

LocalImages TempleLsfg::createLocalImages(LSFG::Vulkan& vk, VkExtent2D extent, VkFormat format) {
    constexpr VkImageUsageFlags usage = VK_IMAGE_USAGE_STORAGE_BIT | VK_IMAGE_USAGE_SAMPLED_BIT |
        VK_IMAGE_USAGE_TRANSFER_SRC_BIT | VK_IMAGE_USAGE_TRANSFER_DST_BIT;

    LocalImages images;
    images.in0 = LSFG::Core::Image(vk.device, extent, format, usage, VK_IMAGE_ASPECT_COLOR_BIT);
    images.in1 = LSFG::Core::Image(vk.device, extent, format, usage, VK_IMAGE_ASPECT_COLOR_BIT);
    images.out.reserve(vk.generationCount);
    for (uint64_t i = 0; i < vk.generationCount; i++)
        images.out.emplace_back(vk.device, extent, format, usage, VK_IMAGE_ASPECT_COLOR_BIT);

    // UNDEFINED -> GENERAL once, so the layout the shaders' barriers track is the real one.
    std::vector<LSFG::Core::Image*> all{ &images.in0, &images.in1 };
    for (auto& img : images.out) all.push_back(&img);

    std::vector<VkImageMemoryBarrier> barriers;
    barriers.reserve(all.size());
    for (auto* img : all) {
        barriers.push_back(VkImageMemoryBarrier{
            .sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER,
            .srcAccessMask = 0,
            .dstAccessMask = VK_ACCESS_SHADER_READ_BIT | VK_ACCESS_SHADER_WRITE_BIT |
                VK_ACCESS_TRANSFER_READ_BIT | VK_ACCESS_TRANSFER_WRITE_BIT,
            .oldLayout = VK_IMAGE_LAYOUT_UNDEFINED,
            .newLayout = VK_IMAGE_LAYOUT_GENERAL,
            .srcQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED,
            .dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED,
            .image = img->handle(),
            .subresourceRange = {
                .aspectMask = img->getAspectFlags(),
                .levelCount = 1,
                .layerCount = 1,
            },
        });
    }

    LSFG::Core::CommandBuffer cmd(vk.device, vk.commandPool);
    cmd.begin();
    vkCmdPipelineBarrier(cmd.handle(),
        VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT,
        0, 0, nullptr, 0, nullptr,
        static_cast<uint32_t>(barriers.size()), barriers.data());
    cmd.end();

    LSFG::Core::Fence fence(vk.device);
    cmd.submit(vk.device.getComputeQueue(), fence);
    if (!fence.wait(vk.device))
        throw LSFG::vulkan_error(VK_TIMEOUT, "Image layout transition timed out");

    for (auto* img : all) img->setLayout(VK_IMAGE_LAYOUT_GENERAL);
    return images;
}

LocalContext TempleLsfg::describeLocal(LSFG::Vulkan& vk, const LocalImages& images, int32_t id) {
    LocalContext ctx;
    ctx.id = id;
    ctx.device = vk.device.handle();
    ctx.physicalDevice = vk.device.getPhysicalDevice();
    ctx.queue = vk.device.getComputeQueue();
    ctx.queueFamily = vk.device.getComputeFamilyIdx();
    ctx.in0 = images.in0.handle();
    ctx.in1 = images.in1.handle();
    for (const auto& img : images.out) ctx.out.push_back(img.handle());
    return ctx;
}
