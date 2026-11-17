// SPDX-License-Identifier: GPL-3.0-only
//
// Ported from Enigmata (app/src/main/cpp/fg) to Tatakai Next 0.4.0 (core/framegen): JNI class, library name,
// log tags and debug properties renamed; no other change.
//
// Adapted from FrankBarretta/LSFG-Android-Application
// (commit b84754199823615d32d65fe33ea59481dca88dcf), app/src/main/cpp/android_vk_session.cpp
// and app/src/main/cpp/android_vk_probe.cpp, GPL-3.0.
// Modifications: Enigmata, AGPL-3.0-or-later (combined under GPL-3.0 section 13).
//
// Enigmata frame generation (R13): our Vulkan instance and device (minimum 1.1).
//
// Function pointers (design decision 2). Framegen creates its own instance and
// device and calls volkInitialize/volkLoadInstance/volkLoadDevice, which
// overwrite volk's global pointers. The volk globals are therefore used only
// here, inside create()/probeDeviceInfo() and before framegen initializes:
//   - loader level (vkEnumerateInstanceExtensionProperties, vkCreateInstance);
//   - vkGetInstanceProcAddr, copied into ifn.getInstanceProcAddr and used to
//     resolve every InstanceFns entry against OUR instance;
//   - volkLoadInstanceOnly + volkLoadDeviceTable(&fn, device), which need the
//     global vkGetDeviceProcAddr of our instance at that moment.
// After create() returns, every call goes through `fn` or `ifn`.
//
// GPU choice: the first physical device (enumeration order) whose
// apiVersion >= 1.1, the same minimum as the vendored framegen
// (third_party/lsfg-vk/src/core/device.cpp). Eligibility (1.3, or 1.1/1.2 in
// the experimental "untested Vulkan" mode) is decided in Kotlin from
// caps.apiVersion, not here. Framegen selects its GPU by
// vendorID << 32 | deviceID, so lsfgUuid names the same GPU. probeDeviceInfo
// uses the same rule and, if no GPU reaches 1.1, reports the first GPU (its
// apiVersion tells Kotlin why it is not eligible); otherwise it reports the
// real version of the selected GPU (e.g. 1.1.x on Adreno 619).
//
// Instance: API 1.1 (like framegen's instance.cpp) with VK_KHR_surface +
// VK_KHR_android_surface (required, because VK_KHR_swapchain is required).
// VK_KHR_get_physical_device_properties2 is not enabled: it is core since 1.1
// and the instance is created with 1.1.
//
// Device extensions (required list of fg_device.h):
//   VK_ANDROID_external_memory_android_hardware_buffer  never core: must be advertised
//   VK_KHR_swapchain                                    never core: must be advertised
//   VK_KHR_external_memory, VK_KHR_sampler_ycbcr_conversion,
//   VK_KHR_dedicated_allocation, VK_KHR_get_memory_requirements2,
//   VK_KHR_bind_memory2, VK_KHR_maintenance1            core since 1.1: enabled when
//       advertised; when not advertised, accepted as provided by core (the
//       selected GPU is >= 1.1).
//   Optional, enabled when advertised:
//     VK_GOOGLE_display_timing (caps.displayTiming);
//     VK_EXT_queue_family_foreign: a dependency of
//       VK_ANDROID_external_memory_android_hardware_buffer (REV 14); not
//       required, because some drivers expose AHB without advertising it.
//
// FP16 (caps.fp16): VkPhysicalDeviceShaderFloat16Int8Features is chained into
// vkGetPhysicalDeviceFeatures2 only when the GPU is >= 1.2 (core) or
// advertises VK_KHR_shader_float16_int8; otherwise the structure would be
// invalid in the pNext chain and fp16 is reported as 0. This is a probe only:
// this device enables no features, so the extension is not enabled here
// (framegen enables it on its own device when it uses shaderFloat16).
// Not used: external_memory_fd, external_semaphore_fd and the opaque-fd handle
// type (design decision 1: images are shared as AHardwareBuffers).
//
// No device features are enabled: this device only records copies and
// presents. Queue: the first family with GRAPHICS and COMPUTE. Android has no
// platform present-support query; VK_KHR_android_surface requires every queue
// family to be able to present to any ANativeWindow, so no surface is needed
// here (the presenter may still check with ifn.getPhysicalDeviceSurfaceSupportKHR).
//
// Errors: missing required extension -> MISSING_EXTENSION; any other Vulkan or
// loader failure (including no GPU with Vulkan 1.1) -> VK_INIT. On failure
// create() leaves the object destroy()-safe (it already calls destroy()).

#include "fg_device.h"

#include "fg_types.h"

#include <android/log.h>

#include <cstdio>
#include <cstring>
#include <vector>

namespace efg {

namespace {

constexpr const char* kTag = "efg-device";
#define EFG_DEV_LOGE(...) __android_log_print(ANDROID_LOG_ERROR, kTag, __VA_ARGS__)
#define EFG_DEV_LOGI(...) __android_log_print(ANDROID_LOG_INFO, kTag, __VA_ARGS__)

constexpr const char* kRequiredInstanceExt[] = {
    VK_KHR_SURFACE_EXTENSION_NAME,
    VK_KHR_ANDROID_SURFACE_EXTENSION_NAME,
};

struct DeviceExt {
    const char* name;
    bool coreSince11;  // promoted to core in Vulkan 1.1
};

constexpr DeviceExt kRequiredDeviceExt[] = {
    {VK_ANDROID_EXTERNAL_MEMORY_ANDROID_HARDWARE_BUFFER_EXTENSION_NAME, false},
    {VK_KHR_EXTERNAL_MEMORY_EXTENSION_NAME, true},
    {VK_KHR_SAMPLER_YCBCR_CONVERSION_EXTENSION_NAME, true},
    {VK_KHR_DEDICATED_ALLOCATION_EXTENSION_NAME, true},
    {VK_KHR_GET_MEMORY_REQUIREMENTS_2_EXTENSION_NAME, true},
    {VK_KHR_BIND_MEMORY_2_EXTENSION_NAME, true},
    {VK_KHR_MAINTENANCE1_EXTENSION_NAME, true},
    {VK_KHR_SWAPCHAIN_EXTENSION_NAME, false},
};

constexpr const char* kOptionalDeviceExt[] = {
    VK_GOOGLE_DISPLAY_TIMING_EXTENSION_NAME,
    VK_EXT_QUEUE_FAMILY_FOREIGN_EXTENSION_NAME,  // dependency of the AHB extension (REV 14)
};

bool hasExt(const std::vector<VkExtensionProperties>& avail, const char* name) {
    for (const auto& e : avail) {
        if (std::strcmp(e.extensionName, name) == 0) return true;
    }
    return false;
}

// Loader-level enumeration (volk global, valid right after volkInitialize).
bool instanceExtensions(std::vector<VkExtensionProperties>& out) {
    out.clear();
    if (vkEnumerateInstanceExtensionProperties == nullptr) return false;
    uint32_t count = 0;
    if (vkEnumerateInstanceExtensionProperties(nullptr, &count, nullptr) != VK_SUCCESS) return false;
    out.resize(count);
    if (count == 0) return true;
    const VkResult r = vkEnumerateInstanceExtensionProperties(nullptr, &count, out.data());
    if (r != VK_SUCCESS && r != VK_INCOMPLETE) return false;
    out.resize(count);
    return true;
}

// Creates a Vulkan 1.1 instance with `exts` enabled (loader-level call).
VkResult makeInstance(const std::vector<const char*>& exts, VkInstance& out) {
    out = VK_NULL_HANDLE;
    if (vkCreateInstance == nullptr) return VK_ERROR_INITIALIZATION_FAILED;
    const VkApplicationInfo appInfo{
        .sType = VK_STRUCTURE_TYPE_APPLICATION_INFO,
        .pApplicationName = "Tatakai",
        .applicationVersion = VK_MAKE_API_VERSION(0, 1, 0, 0),
        .pEngineName = "tatakai-fg",
        .engineVersion = VK_MAKE_API_VERSION(0, 1, 0, 0),
        .apiVersion = VK_API_VERSION_1_1,
    };
    const VkInstanceCreateInfo info{
        .sType = VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO,
        .pApplicationInfo = &appInfo,
        .enabledExtensionCount = static_cast<uint32_t>(exts.size()),
        .ppEnabledExtensionNames = exts.empty() ? nullptr : exts.data(),
    };
    return vkCreateInstance(&info, nullptr, &out);
}

template <typename T>
void resolve(const InstanceFns& ifn, VkInstance instance, const char* name, T& out) {
    out = reinterpret_cast<T>(ifn.getInstanceProcAddr(instance, name));
}

// Resolves every InstanceFns entry against `instance` through the loader's
// vkGetInstanceProcAddr (copied first into ifn.getInstanceProcAddr). Returns
// false if a non-surface entry is missing; surface entries are checked by the
// caller (probeDeviceInfo does not enable the surface extensions).
bool loadInstanceFns(VkInstance instance, InstanceFns& ifn) {
    ifn = InstanceFns{};
    ifn.getInstanceProcAddr = vkGetInstanceProcAddr;
    if (ifn.getInstanceProcAddr == nullptr) return false;
    resolve(ifn, instance, "vkDestroyInstance", ifn.destroyInstance);
    resolve(ifn, instance, "vkEnumeratePhysicalDevices", ifn.enumeratePhysicalDevices);
    resolve(ifn, instance, "vkGetPhysicalDeviceProperties", ifn.getPhysicalDeviceProperties);
    resolve(ifn, instance, "vkGetPhysicalDeviceProperties2", ifn.getPhysicalDeviceProperties2);
    resolve(ifn, instance, "vkGetPhysicalDeviceFeatures2", ifn.getPhysicalDeviceFeatures2);
    resolve(ifn, instance, "vkGetPhysicalDeviceMemoryProperties", ifn.getPhysicalDeviceMemoryProperties);
    resolve(ifn, instance, "vkGetPhysicalDeviceQueueFamilyProperties",
            ifn.getPhysicalDeviceQueueFamilyProperties);
    resolve(ifn, instance, "vkGetPhysicalDeviceImageFormatProperties2",
            ifn.getPhysicalDeviceImageFormatProperties2);
    resolve(ifn, instance, "vkEnumerateDeviceExtensionProperties", ifn.enumerateDeviceExtensionProperties);
    resolve(ifn, instance, "vkCreateDevice", ifn.createDevice);
    resolve(ifn, instance, "vkGetDeviceProcAddr", ifn.getDeviceProcAddr);
    resolve(ifn, instance, "vkCreateAndroidSurfaceKHR", ifn.createAndroidSurfaceKHR);
    resolve(ifn, instance, "vkDestroySurfaceKHR", ifn.destroySurfaceKHR);
    resolve(ifn, instance, "vkGetPhysicalDeviceSurfaceSupportKHR", ifn.getPhysicalDeviceSurfaceSupportKHR);
    resolve(ifn, instance, "vkGetPhysicalDeviceSurfaceCapabilitiesKHR",
            ifn.getPhysicalDeviceSurfaceCapabilitiesKHR);
    resolve(ifn, instance, "vkGetPhysicalDeviceSurfaceFormatsKHR", ifn.getPhysicalDeviceSurfaceFormatsKHR);
    resolve(ifn, instance, "vkGetPhysicalDeviceSurfacePresentModesKHR",
            ifn.getPhysicalDeviceSurfacePresentModesKHR);
    return ifn.destroyInstance != nullptr && ifn.enumeratePhysicalDevices != nullptr &&
           ifn.getPhysicalDeviceProperties != nullptr && ifn.getPhysicalDeviceProperties2 != nullptr &&
           ifn.getPhysicalDeviceFeatures2 != nullptr && ifn.getPhysicalDeviceMemoryProperties != nullptr &&
           ifn.getPhysicalDeviceQueueFamilyProperties != nullptr &&
           ifn.getPhysicalDeviceImageFormatProperties2 != nullptr &&
           ifn.enumerateDeviceExtensionProperties != nullptr && ifn.createDevice != nullptr &&
           ifn.getDeviceProcAddr != nullptr;
}

bool surfaceFnsLoaded(const InstanceFns& ifn) {
    return ifn.createAndroidSurfaceKHR != nullptr && ifn.destroySurfaceKHR != nullptr &&
           ifn.getPhysicalDeviceSurfaceSupportKHR != nullptr &&
           ifn.getPhysicalDeviceSurfaceCapabilitiesKHR != nullptr &&
           ifn.getPhysicalDeviceSurfaceFormatsKHR != nullptr &&
           ifn.getPhysicalDeviceSurfacePresentModesKHR != nullptr;
}

// First GPU with apiVersion >= 1.1 (framegen's minimum; eligibility is decided
// in Kotlin); with allowFallback, the first GPU if none qualifies.
// VK_NULL_HANDLE if there is no GPU (or none qualifies).
VkPhysicalDevice pickPhysicalDevice(const InstanceFns& ifn, VkInstance instance, bool allowFallback) {
    uint32_t count = 0;
    if (ifn.enumeratePhysicalDevices(instance, &count, nullptr) != VK_SUCCESS || count == 0) {
        return VK_NULL_HANDLE;
    }
    std::vector<VkPhysicalDevice> devs(count);
    const VkResult r = ifn.enumeratePhysicalDevices(instance, &count, devs.data());
    if ((r != VK_SUCCESS && r != VK_INCOMPLETE) || count == 0) return VK_NULL_HANDLE;
    devs.resize(count);
    for (VkPhysicalDevice d : devs) {
        VkPhysicalDeviceProperties props{};
        ifn.getPhysicalDeviceProperties(d, &props);
        if (props.apiVersion >= VK_API_VERSION_1_1) return d;
    }
    return allowFallback ? devs[0] : VK_NULL_HANDLE;
}

bool deviceExtensions(const InstanceFns& ifn, VkPhysicalDevice phys, std::vector<VkExtensionProperties>& out) {
    out.clear();
    uint32_t count = 0;
    if (ifn.enumerateDeviceExtensionProperties(phys, nullptr, &count, nullptr) != VK_SUCCESS) return false;
    out.resize(count);
    if (count == 0) return true;
    const VkResult r = ifn.enumerateDeviceExtensionProperties(phys, nullptr, &count, out.data());
    if (r != VK_SUCCESS && r != VK_INCOMPLETE) return false;
    out.resize(count);
    return true;
}

// True if every required device extension is advertised or, for the ones
// promoted to core in 1.1, provided by core on this GPU.
bool requiredDeviceExtsPresent(const std::vector<VkExtensionProperties>& avail, uint32_t apiVersion,
                               const char** firstMissing) {
    for (const DeviceExt& e : kRequiredDeviceExt) {
        if (hasExt(avail, e.name)) continue;
        if (e.coreSince11 && apiVersion >= VK_API_VERSION_1_1) continue;
        if (firstMissing != nullptr) *firstMissing = e.name;
        return false;
    }
    return true;
}

// shaderFloat16 probe (see header): the feature structure is only valid in the
// chain on >= 1.2 or with VK_KHR_shader_float16_int8 advertised; below 1.1
// (probeDeviceInfo fallback GPU) vkGetPhysicalDeviceFeatures2 itself is not
// available for the device. Probe only: the extension is not enabled here.
bool queryFp16(const InstanceFns& ifn, VkPhysicalDevice phys, uint32_t apiVersion,
               const std::vector<VkExtensionProperties>& avail) {
    if (apiVersion < VK_API_VERSION_1_1) return false;
    if (apiVersion < VK_API_VERSION_1_2 && !hasExt(avail, VK_KHR_SHADER_FLOAT16_INT8_EXTENSION_NAME)) {
        return false;
    }
    VkPhysicalDeviceShaderFloat16Int8Features fp16{
        .sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_SHADER_FLOAT16_INT8_FEATURES,
    };
    VkPhysicalDeviceFeatures2 feats2{
        .sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2,
        .pNext = &fp16,
    };
    ifn.getPhysicalDeviceFeatures2(phys, &feats2);
    return fp16.shaderFloat16 == VK_TRUE;
}

// Fills caps for `phys`. surfaceExts: the instance offers (create: enabled)
// VK_KHR_surface + VK_KHR_android_surface.
void fillCaps(const InstanceFns& ifn, VkPhysicalDevice phys, const std::vector<VkExtensionProperties>& avail,
              bool surfaceExts, Caps& caps) {
    VkPhysicalDeviceProperties props{};
    ifn.getPhysicalDeviceProperties(phys, &props);
    caps.apiVersion = props.apiVersion;
    caps.gpuName.assign(props.deviceName, strnlen(props.deviceName, VK_MAX_PHYSICAL_DEVICE_NAME_SIZE));
    caps.vendorId = props.vendorID;
    caps.deviceId = props.deviceID;
    caps.ahb = hasExt(avail, VK_ANDROID_EXTERNAL_MEMORY_ANDROID_HARDWARE_BUFFER_EXTENSION_NAME);
    caps.swapchain = surfaceExts && hasExt(avail, VK_KHR_SWAPCHAIN_EXTENSION_NAME);
    caps.displayTiming = hasExt(avail, VK_GOOGLE_DISPLAY_TIMING_EXTENSION_NAME);
    caps.fp16 = queryFp16(ifn, phys, caps.apiVersion, avail);
}

uint32_t pickQueueFamily(const InstanceFns& ifn, VkPhysicalDevice phys) {
    uint32_t count = 0;
    ifn.getPhysicalDeviceQueueFamilyProperties(phys, &count, nullptr);
    if (count == 0) return UINT32_MAX;
    std::vector<VkQueueFamilyProperties> fams(count);
    ifn.getPhysicalDeviceQueueFamilyProperties(phys, &count, fams.data());
    constexpr VkQueueFlags kNeeded = VK_QUEUE_GRAPHICS_BIT | VK_QUEUE_COMPUTE_BIT;
    for (uint32_t i = 0; i < count; ++i) {
        if ((fams[i].queueFlags & kNeeded) == kNeeded && fams[i].queueCount > 0) return i;
    }
    return UINT32_MAX;
}

}  // namespace

int32_t FgDevice::create() {
    if (device != VK_NULL_HANDLE || instance != VK_NULL_HANDLE) destroy();

    if (volkInitialize() != VK_SUCCESS) {
        EFG_DEV_LOGE("volkInitialize failed (no Vulkan loader)");
        return FgResult::VK_INIT;
    }

    // Instance: surface extensions are required (VK_KHR_swapchain is required).
    std::vector<VkExtensionProperties> instAvail;
    if (!instanceExtensions(instAvail)) {
        EFG_DEV_LOGE("instance extension enumeration failed");
        return FgResult::VK_INIT;
    }
    std::vector<const char*> instExts;
    for (const char* name : kRequiredInstanceExt) {
        if (!hasExt(instAvail, name)) {
            EFG_DEV_LOGE("missing instance extension %s", name);
            return FgResult::MISSING_EXTENSION;
        }
        instExts.push_back(name);
    }
    VkResult r = makeInstance(instExts, instance);
    if (r != VK_SUCCESS || instance == VK_NULL_HANDLE) {
        EFG_DEV_LOGE("vkCreateInstance failed (%d)", static_cast<int>(r));
        instance = VK_NULL_HANDLE;
        return r == VK_ERROR_EXTENSION_NOT_PRESENT ? FgResult::MISSING_EXTENSION : FgResult::VK_INIT;
    }
    // Globals of OUR instance; volkLoadDeviceTable below needs its vkGetDeviceProcAddr.
    volkLoadInstanceOnly(instance);
    if (!loadInstanceFns(instance, ifn) || !surfaceFnsLoaded(ifn)) {
        EFG_DEV_LOGE("instance entry points unresolvable");
        // destroyInstance may be the only pointer resolved; destroy() handles both cases.
        destroy();
        return FgResult::VK_INIT;
    }

    // Physical device.
    phys = pickPhysicalDevice(ifn, instance, false);
    if (phys == VK_NULL_HANDLE) {
        EFG_DEV_LOGE("no GPU with Vulkan 1.1");
        destroy();
        return FgResult::VK_INIT;
    }
    std::vector<VkExtensionProperties> devAvail;
    if (!deviceExtensions(ifn, phys, devAvail)) {
        EFG_DEV_LOGE("device extension enumeration failed");
        destroy();
        return FgResult::VK_INIT;
    }
    fillCaps(ifn, phys, devAvail, true, caps);
    lsfgUuid = (static_cast<uint64_t>(caps.vendorId) << 32) | caps.deviceId;
    EFG_DEV_LOGI("GPU %s (API %u.%u.%u, vendor=0x%x device=0x%x) ahb=%d swapchain=%d displayTiming=%d fp16=%d",
                 caps.gpuName.c_str(), VK_API_VERSION_MAJOR(caps.apiVersion),
                 VK_API_VERSION_MINOR(caps.apiVersion), VK_API_VERSION_PATCH(caps.apiVersion), caps.vendorId,
                 caps.deviceId, caps.ahb ? 1 : 0, caps.swapchain ? 1 : 0, caps.displayTiming ? 1 : 0,
                 caps.fp16 ? 1 : 0);

    const char* missing = nullptr;
    if (!requiredDeviceExtsPresent(devAvail, caps.apiVersion, &missing)) {
        EFG_DEV_LOGE("missing device extension %s", missing != nullptr ? missing : "?");
        destroy();
        return FgResult::MISSING_EXTENSION;
    }
    std::vector<const char*> devExts;
    for (const DeviceExt& e : kRequiredDeviceExt) {
        if (hasExt(devAvail, e.name)) {
            devExts.push_back(e.name);
        } else {
            EFG_DEV_LOGI("%s not advertised; provided by Vulkan 1.1 core", e.name);
        }
    }
    for (const char* name : kOptionalDeviceExt) {
        if (hasExt(devAvail, name)) devExts.push_back(name);
    }

    // Device with one graphics + compute (+ present, see header) queue.
    family = pickQueueFamily(ifn, phys);
    if (family == UINT32_MAX) {
        EFG_DEV_LOGE("no graphics+compute queue family");
        destroy();
        return FgResult::VK_INIT;
    }
    const float priority = 1.0f;
    const VkDeviceQueueCreateInfo queueInfo{
        .sType = VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO,
        .queueFamilyIndex = family,
        .queueCount = 1,
        .pQueuePriorities = &priority,
    };
    const VkDeviceCreateInfo devInfo{
        .sType = VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO,
        .queueCreateInfoCount = 1,
        .pQueueCreateInfos = &queueInfo,
        .enabledExtensionCount = static_cast<uint32_t>(devExts.size()),
        .ppEnabledExtensionNames = devExts.data(),
    };
    r = ifn.createDevice(phys, &devInfo, nullptr, &device);
    if (r != VK_SUCCESS || device == VK_NULL_HANDLE) {
        EFG_DEV_LOGE("vkCreateDevice failed (%d)", static_cast<int>(r));
        device = VK_NULL_HANDLE;
        destroy();
        return r == VK_ERROR_EXTENSION_NOT_PRESENT ? FgResult::MISSING_EXTENSION : FgResult::VK_INIT;
    }

    // Per-device table: stays valid after framegen's volkLoadDevice.
    volkLoadDeviceTable(&fn, device);
    if (fn.vkDestroyDevice == nullptr || fn.vkGetDeviceQueue == nullptr || fn.vkDeviceWaitIdle == nullptr ||
        fn.vkQueueSubmit == nullptr || fn.vkQueuePresentKHR == nullptr ||
        fn.vkGetAndroidHardwareBufferPropertiesANDROID == nullptr) {
        EFG_DEV_LOGE("device entry points unresolvable");
        destroy();
        return FgResult::VK_INIT;
    }
    fn.vkGetDeviceQueue(device, family, 0, &queue);
    if (queue == VK_NULL_HANDLE) {
        EFG_DEV_LOGE("vkGetDeviceQueue returned null");
        destroy();
        return FgResult::VK_INIT;
    }
    EFG_DEV_LOGI("device ready (family=%u, %zu extensions, lsfgUuid=0x%llx)", family, devExts.size(),
                 static_cast<unsigned long long>(lsfgUuid));
    return FgResult::OK;
}

void FgDevice::destroy() {
    if (device != VK_NULL_HANDLE) {
        std::lock_guard<std::mutex> lock(queueMutex);
        if (fn.vkDeviceWaitIdle != nullptr) fn.vkDeviceWaitIdle(device);
        if (fn.vkDestroyDevice != nullptr) {
            fn.vkDestroyDevice(device, nullptr);
        } else if (ifn.getDeviceProcAddr != nullptr) {
            // Table never loaded: resolve vkDestroyDevice from our instance.
            auto destroyDevice =
                reinterpret_cast<PFN_vkDestroyDevice>(ifn.getDeviceProcAddr(device, "vkDestroyDevice"));
            if (destroyDevice != nullptr) destroyDevice(device, nullptr);
        }
        device = VK_NULL_HANDLE;
    }
    if (instance != VK_NULL_HANDLE) {
        PFN_vkDestroyInstance destroyInstance = ifn.destroyInstance;
        if (destroyInstance == nullptr && ifn.getInstanceProcAddr != nullptr) {
            destroyInstance =
                reinterpret_cast<PFN_vkDestroyInstance>(ifn.getInstanceProcAddr(instance, "vkDestroyInstance"));
        }
        if (destroyInstance != nullptr) destroyInstance(instance, nullptr);
        instance = VK_NULL_HANDLE;
    }
    phys = VK_NULL_HANDLE;
    queue = VK_NULL_HANDLE;
    family = UINT32_MAX;
    fn = VolkDeviceTable{};
    ifn = InstanceFns{};
    lsfgUuid = 0;
    caps = Caps{};
}

VkResult FgDevice::submit(const VkSubmitInfo& info, VkFence fence) {
    std::lock_guard<std::mutex> lock(queueMutex);
    if (queue == VK_NULL_HANDLE || fn.vkQueueSubmit == nullptr) return VK_ERROR_INITIALIZATION_FAILED;
    return fn.vkQueueSubmit(queue, 1, &info, fence);
}

VkResult FgDevice::present(const VkPresentInfoKHR& info) {
    std::lock_guard<std::mutex> lock(queueMutex);
    if (queue == VK_NULL_HANDLE || fn.vkQueuePresentKHR == nullptr) return VK_ERROR_INITIALIZATION_FAILED;
    return fn.vkQueuePresentKHR(queue, &info);
}

int32_t probeDeviceInfo(std::string& out) {
    out.clear();
    if (volkInitialize() != VK_SUCCESS) {
        EFG_DEV_LOGE("probe: volkInitialize failed");
        return FgResult::VK_INIT;
    }
    std::vector<VkExtensionProperties> instAvail;
    if (!instanceExtensions(instAvail)) return FgResult::VK_INIT;
    bool surfaceExts = true;
    for (const char* name : kRequiredInstanceExt) {
        if (!hasExt(instAvail, name)) surfaceExts = false;
    }

    // Temporary instance without extensions and without a device. The volk
    // globals are not reloaded for it: every call goes through a local table.
    VkInstance instance = VK_NULL_HANDLE;
    const std::vector<const char*> noExts;
    if (makeInstance(noExts, instance) != VK_SUCCESS || instance == VK_NULL_HANDLE) {
        EFG_DEV_LOGE("probe: vkCreateInstance failed");
        return FgResult::VK_INIT;
    }
    InstanceFns ifn{};
    const bool loaded = loadInstanceFns(instance, ifn);
    int32_t rc = FgResult::VK_INIT;
    if (loaded) {
        const VkPhysicalDevice phys = pickPhysicalDevice(ifn, instance, true);
        std::vector<VkExtensionProperties> devAvail;
        if (phys != VK_NULL_HANDLE && deviceExtensions(ifn, phys, devAvail)) {
            Caps caps{};
            fillCaps(ifn, phys, devAvail, surfaceExts, caps);
            std::string name = caps.gpuName;
            for (char& c : name) {
                if (c == ';') c = ',';
            }
            char buf[96];
            std::snprintf(buf, sizeof(buf), "apiVersion=%u.%u.%u;gpuName=", VK_API_VERSION_MAJOR(caps.apiVersion),
                          VK_API_VERSION_MINOR(caps.apiVersion), VK_API_VERSION_PATCH(caps.apiVersion));
            out = buf;
            out += name;
            std::snprintf(buf, sizeof(buf), ";vendor=0x%x;device=0x%x;ahb=%d;swapchain=%d;displayTiming=%d;fp16=%d",
                          caps.vendorId, caps.deviceId, caps.ahb ? 1 : 0, caps.swapchain ? 1 : 0,
                          caps.displayTiming ? 1 : 0, caps.fp16 ? 1 : 0);
            out += buf;
            rc = FgResult::OK;
        } else {
            EFG_DEV_LOGE("probe: no usable GPU");
        }
    } else {
        EFG_DEV_LOGE("probe: instance entry points unresolvable");
    }
    PFN_vkDestroyInstance destroyInstance = ifn.destroyInstance;
    if (destroyInstance == nullptr && ifn.getInstanceProcAddr != nullptr) {
        destroyInstance =
            reinterpret_cast<PFN_vkDestroyInstance>(ifn.getInstanceProcAddr(instance, "vkDestroyInstance"));
    }
    if (destroyInstance != nullptr) destroyInstance(instance, nullptr);
    return rc;
}

}  // namespace efg
