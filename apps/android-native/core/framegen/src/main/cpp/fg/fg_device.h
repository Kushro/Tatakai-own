// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Ported from Enigmata (app/src/main/cpp/fg) to Tatakai Next 0.4.0 (core/framegen): JNI class, library name,
// log tags and debug properties renamed; no other change.
//
// Enigmata frame generation (R13): our Vulkan instance and device (copies and
// presenter). Minimum Vulkan 1.1; whether a device is eligible (1.3, or 1.1/1.2
// with the experimental untested-GPU switch) is decided in Kotlin (FgPolicy).
// Framegen owns a second VkDevice (compute only) and calls
// volkLoadInstance/volkLoadDevice, which overwrite volk's global function
// pointers. Therefore everything here goes through `fn` (VolkDeviceTable) and
// `ifn` (instance pointers cached before LSFG initialize); the volk globals are
// only used inside create() before framegen starts.
//
// Required device extensions: VK_ANDROID_external_memory_android_hardware_buffer,
// VK_KHR_external_memory, VK_KHR_sampler_ycbcr_conversion,
// VK_KHR_dedicated_allocation, VK_KHR_get_memory_requirements2,
// VK_KHR_bind_memory2, VK_KHR_maintenance1, VK_KHR_swapchain.
// Optional: VK_GOOGLE_display_timing.
#pragma once

#include <volk.h>

#include <cstdint>
#include <mutex>
#include <string>

namespace efg {

// Instance-level entry points resolved against OUR instance.
struct InstanceFns {
    PFN_vkGetInstanceProcAddr getInstanceProcAddr = nullptr;
    PFN_vkDestroyInstance destroyInstance = nullptr;
    PFN_vkEnumeratePhysicalDevices enumeratePhysicalDevices = nullptr;
    PFN_vkGetPhysicalDeviceProperties getPhysicalDeviceProperties = nullptr;
    PFN_vkGetPhysicalDeviceProperties2 getPhysicalDeviceProperties2 = nullptr;
    PFN_vkGetPhysicalDeviceFeatures2 getPhysicalDeviceFeatures2 = nullptr;
    PFN_vkGetPhysicalDeviceMemoryProperties getPhysicalDeviceMemoryProperties = nullptr;
    PFN_vkGetPhysicalDeviceQueueFamilyProperties getPhysicalDeviceQueueFamilyProperties = nullptr;
    PFN_vkGetPhysicalDeviceImageFormatProperties2 getPhysicalDeviceImageFormatProperties2 = nullptr;
    PFN_vkEnumerateDeviceExtensionProperties enumerateDeviceExtensionProperties = nullptr;
    PFN_vkCreateDevice createDevice = nullptr;
    PFN_vkGetDeviceProcAddr getDeviceProcAddr = nullptr;
    PFN_vkCreateAndroidSurfaceKHR createAndroidSurfaceKHR = nullptr;
    PFN_vkDestroySurfaceKHR destroySurfaceKHR = nullptr;
    PFN_vkGetPhysicalDeviceSurfaceSupportKHR getPhysicalDeviceSurfaceSupportKHR = nullptr;
    PFN_vkGetPhysicalDeviceSurfaceCapabilitiesKHR getPhysicalDeviceSurfaceCapabilitiesKHR = nullptr;
    PFN_vkGetPhysicalDeviceSurfaceFormatsKHR getPhysicalDeviceSurfaceFormatsKHR = nullptr;
    PFN_vkGetPhysicalDeviceSurfacePresentModesKHR getPhysicalDeviceSurfacePresentModesKHR = nullptr;
};

// What the selected GPU offers (also reported by probeDeviceInfo).
struct Caps {
    uint32_t apiVersion = 0;
    std::string gpuName;
    uint32_t vendorId = 0;
    uint32_t deviceId = 0;
    bool ahb = false;            // VK_ANDROID_external_memory_android_hardware_buffer
    bool swapchain = false;      // VK_KHR_swapchain (+ surface extensions)
    bool displayTiming = false;  // VK_GOOGLE_display_timing
    bool fp16 = false;           // shaderFloat16
};

struct FgDevice {
    VkInstance instance = VK_NULL_HANDLE;
    VkPhysicalDevice phys = VK_NULL_HANDLE;
    VkDevice device = VK_NULL_HANDLE;
    VkQueue queue = VK_NULL_HANDLE;
    uint32_t family = UINT32_MAX;  // graphics + compute + present
    VolkDeviceTable fn{};
    InstanceFns ifn{};
    uint64_t lsfgUuid = 0;  // vendorID << 32 | deviceID, for LSFG initialize
    Caps caps{};
    std::mutex queueMutex;  // serializes every use of `queue` (engine and presenter)

    FgDevice() = default;
    FgDevice(const FgDevice&) = delete;
    FgDevice& operator=(const FgDevice&) = delete;

    // Creates instance and device. Returns FgResult::OK, VK_INIT or
    // MISSING_EXTENSION; on failure the object is destroy()-safe.
    int32_t create();

    // Last step of stop() (I3). Idempotent.
    void destroy();

    // vkQueueSubmit under queueMutex.
    VkResult submit(const VkSubmitInfo& info, VkFence fence);

    // vkQueuePresentKHR under queueMutex.
    VkResult present(const VkPresentInfoKHR& info);
};

// Probes the GPU with a temporary instance and writes
// "apiVersion=..;gpuName=..;vendor=..;device=..;ahb=0|1;swapchain=0|1;displayTiming=0|1;fp16=0|1".
// Returns FgResult::OK or VK_INIT.
int32_t probeDeviceInfo(std::string& out);

}  // namespace efg
