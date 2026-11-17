#include <volk.h>
#include <vulkan/vulkan_core.h>

#include "core/instance.hpp"
#include "common/exception.hpp"

#include <cstdint>
#include <memory>
#include <vector>

using namespace LSFG::Core;

const std::vector<const char*> requiredExtensions = {

};

namespace LSFG::Core {
// Enigmata modification (FG-F, D4): the instance declares
// min(loader version, 1.3), with the previous 1.1 as the floor for 1.0
// loaders, so that the 1.2/1.3 feature structs device.cpp enables are within
// the instance's apiVersion. device.cpp decides its feature chains with
// min(this, physical device version); it is deterministic (the loader version
// does not change), so device.cpp calls it again instead of storing it.
// Requires volkInitialize().
extern uint32_t instanceApiVersion();
uint32_t instanceApiVersion() {
    const uint32_t loader = volkGetInstanceVersion();
    if (loader < VK_API_VERSION_1_1) return VK_API_VERSION_1_1;
    return loader < VK_API_VERSION_1_3 ? loader : VK_API_VERSION_1_3;
}
}

Instance::Instance() {
    volkInitialize();

    // create Vulkan instance
    const VkApplicationInfo appInfo{
        .sType = VK_STRUCTURE_TYPE_APPLICATION_INFO,
        .pApplicationName = "lsfg-vk-base",
        .applicationVersion = VK_MAKE_VERSION(0, 0, 1),
        .pEngineName = "lsfg-vk-base",
        .engineVersion = VK_MAKE_VERSION(0, 0, 1),
        // All mandatory Android paths have Vulkan 1.1 implementations. Device
        // creation probes 1.2/1.3 features and opts into them when available
        // and allowed by this version (Enigmata: was a fixed VK_API_VERSION_1_1).
        .apiVersion = instanceApiVersion()
    };
    const VkInstanceCreateInfo createInfo{
        .sType = VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO,
        .pApplicationInfo = &appInfo,
        .enabledExtensionCount = static_cast<uint32_t>(requiredExtensions.size()),
        .ppEnabledExtensionNames = requiredExtensions.data()
    };
    VkInstance instanceHandle{};
    auto res = vkCreateInstance(&createInfo, nullptr, &instanceHandle);
    if (res != VK_SUCCESS)
        throw LSFG::vulkan_error(res, "Failed to create Vulkan instance");

    volkLoadInstance(instanceHandle);

    // store in shared ptr
    this->instance = std::shared_ptr<VkInstance>(
        new VkInstance(instanceHandle),
        [](VkInstance* instance) {
            vkDestroyInstance(*instance, nullptr);
        }
    );
}
