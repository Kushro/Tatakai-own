// SPDX-License-Identifier: GPL-3.0-only
//
// Ported from Enigmata (app/src/main/cpp/fg) to Tatakai Next 0.4.0 (core/framegen): JNI class, library name,
// log tags and debug properties renamed; no other change.
//
// Adapted from FrankBarretta/LSFG-Android-Application
// (commit b84754199823615d32d65fe33ea59481dca88dcf), app/src/main/cpp/ahb_image_bridge.cpp, GPL-3.0.
// Modifications: Enigmata, AGPL-3.0-or-later (combined under GPL-3.0 section 13).
//
// Enigmata frame generation (R13): AHardwareBuffer -> VkImage on our device.
// Taken from the reference: the AHB property query, the caller format hint
// when the driver reports VK_FORMAT_UNDEFINED, the dedicated import and the
// format tables. Not taken: the opaque-fd export (design decision 1) and the
// CPU_READ_OFTEN allocation usage (we never lock these buffers on the CPU).
//
// FG-F: no hint for RGBX (the AImageReader format). The Vulkan AHB format
// table maps R8G8B8X8_UNORM to VK_FORMAT_R8G8B8A8_UNORM, so a driver that
// reports UNDEFINED for it is describing something else (an implementation-
// defined layout that needs VkExternalFormatANDROID); reading it as RGBA8 is
// invalid usage. Such an import fails with VK_INIT and an explicit efg-ahb
// error: the engine fails the session (fail(rc)) and FG stops cleanly. Every
// distinct (AHB format, AHB usage) pair logs one efg-ahb line with what the
// driver reported for it (format, externalFormat, formatFeatures, hint).
// FG-F2: the image keeps the format the driver reported (e.g. A2B10G10R10 for
// an RGBA_1010102 reader buffer) and its formatFeatures (AhbImage::features),
// so the engine can tell a raw copy from a format-converting blit.
//
// Every Vulkan call goes through dev.fn / dev.ifn (design decision 2). The
// image usage is SAMPLED | STORAGE | TRANSFER_SRC | TRANSFER_DST restricted to
// what the AHB format supports (formatFeatures of the AHB, then a
// vkGetPhysicalDeviceImageFormatProperties2 check that drops STORAGE first).

#include "ahb_image.h"

#include "fg_types.h"

#include <android/log.h>

#include <mutex>
#include <set>
#include <utility>

namespace efg {

namespace {

constexpr const char* kTag = "efg-ahb";
#define EFG_AHB_LOGE(...) __android_log_print(ANDROID_LOG_ERROR, kTag, __VA_ARGS__)
#define EFG_AHB_LOGI(...) __android_log_print(ANDROID_LOG_INFO, kTag, __VA_ARGS__)

// True the first time a (format, usage) pair is wrapped in this process, so the
// diagnostic line is logged once per kind of buffer, not per import.
bool firstSighting(uint32_t ahbFormat, uint64_t ahbUsage) {
    static std::mutex mutex;
    static std::set<std::pair<uint32_t, uint64_t>> seen;
    std::lock_guard<std::mutex> lock(mutex);
    return seen.emplace(ahbFormat, ahbUsage).second;
}

uint32_t ahbFormatFor(VkFormat fmt) {
    switch (fmt) {
        case VK_FORMAT_R8G8B8A8_UNORM: return AHARDWAREBUFFER_FORMAT_R8G8B8A8_UNORM;
        case VK_FORMAT_R8G8B8_UNORM: return AHARDWAREBUFFER_FORMAT_R8G8B8_UNORM;
        case VK_FORMAT_R5G6B5_UNORM_PACK16: return AHARDWAREBUFFER_FORMAT_R5G6B5_UNORM;
        case VK_FORMAT_R16G16B16A16_SFLOAT: return AHARDWAREBUFFER_FORMAT_R16G16B16A16_FLOAT;
        case VK_FORMAT_A2B10G10R10_UNORM_PACK32: return AHARDWAREBUFFER_FORMAT_R10G10B10A2_UNORM;
        default: return 0;
    }
}

// Hint used when the driver reports VK_FORMAT_UNDEFINED for a plain RGB(A) AHB.
// RGBX has none on purpose (FG-F, see the file header): wrapAhb refuses it.
VkFormat vkFormatHintFor(uint32_t ahbFormat) {
    switch (ahbFormat) {
        case AHARDWAREBUFFER_FORMAT_R8G8B8A8_UNORM: return VK_FORMAT_R8G8B8A8_UNORM;
        case AHARDWAREBUFFER_FORMAT_R8G8B8_UNORM: return VK_FORMAT_R8G8B8_UNORM;
        case AHARDWAREBUFFER_FORMAT_R5G6B5_UNORM: return VK_FORMAT_R5G6B5_UNORM_PACK16;
        case AHARDWAREBUFFER_FORMAT_R16G16B16A16_FLOAT: return VK_FORMAT_R16G16B16A16_SFLOAT;
        case AHARDWAREBUFFER_FORMAT_R10G10B10A2_UNORM: return VK_FORMAT_A2B10G10R10_UNORM_PACK32;
        default: return VK_FORMAT_UNDEFINED;
    }
}

constexpr VkImageUsageFlags kWantedUsage = VK_IMAGE_USAGE_SAMPLED_BIT | VK_IMAGE_USAGE_STORAGE_BIT |
                                           VK_IMAGE_USAGE_TRANSFER_SRC_BIT | VK_IMAGE_USAGE_TRANSFER_DST_BIT;

VkImageUsageFlags usageFromFeatures(VkFormatFeatureFlags features) {
    if (features == 0) return kWantedUsage;  // driver did not report: let the format query decide
    VkImageUsageFlags usage = 0;
    if (features & VK_FORMAT_FEATURE_SAMPLED_IMAGE_BIT) usage |= VK_IMAGE_USAGE_SAMPLED_BIT;
    if (features & VK_FORMAT_FEATURE_STORAGE_IMAGE_BIT) usage |= VK_IMAGE_USAGE_STORAGE_BIT;
    if (features & VK_FORMAT_FEATURE_TRANSFER_SRC_BIT) usage |= VK_IMAGE_USAGE_TRANSFER_SRC_BIT;
    if (features & VK_FORMAT_FEATURE_TRANSFER_DST_BIT) usage |= VK_IMAGE_USAGE_TRANSFER_DST_BIT;
    return usage;
}

bool usageSupported(FgDevice& dev, VkFormat format, VkImageUsageFlags usage) {
    if (dev.ifn.getPhysicalDeviceImageFormatProperties2 == nullptr) return true;  // cannot check
    const VkPhysicalDeviceExternalImageFormatInfo extInfo{
        .sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_EXTERNAL_IMAGE_FORMAT_INFO,
        .handleType = VK_EXTERNAL_MEMORY_HANDLE_TYPE_ANDROID_HARDWARE_BUFFER_BIT_ANDROID,
    };
    const VkPhysicalDeviceImageFormatInfo2 info{
        .sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_IMAGE_FORMAT_INFO_2,
        .pNext = &extInfo,
        .format = format,
        .type = VK_IMAGE_TYPE_2D,
        .tiling = VK_IMAGE_TILING_OPTIMAL,
        .usage = usage,
        .flags = 0,
    };
    VkExternalImageFormatProperties extProps{
        .sType = VK_STRUCTURE_TYPE_EXTERNAL_IMAGE_FORMAT_PROPERTIES,
    };
    VkImageFormatProperties2 props{
        .sType = VK_STRUCTURE_TYPE_IMAGE_FORMAT_PROPERTIES_2,
        .pNext = &extProps,
    };
    if (dev.ifn.getPhysicalDeviceImageFormatProperties2(dev.phys, &info, &props) != VK_SUCCESS) return false;
    return (extProps.externalMemoryProperties.externalMemoryFeatures &
            VK_EXTERNAL_MEMORY_FEATURE_IMPORTABLE_BIT) != 0;
}

uint32_t pickMemoryType(FgDevice& dev, uint32_t typeBits) {
    VkPhysicalDeviceMemoryProperties memProps{};
    dev.ifn.getPhysicalDeviceMemoryProperties(dev.phys, &memProps);
    uint32_t first = UINT32_MAX;
    for (uint32_t i = 0; i < memProps.memoryTypeCount; ++i) {
        if ((typeBits & (1u << i)) == 0) continue;
        if (memProps.memoryTypes[i].propertyFlags & VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT) return i;
        if (first == UINT32_MAX) first = i;
    }
    return first;
}

}  // namespace

AhbImage::~AhbImage() {
    reset();
}

AhbImage::AhbImage(AhbImage&& other) noexcept
    : dev(other.dev),
      ahb(other.ahb),
      ownsAhb(other.ownsAhb),
      image(other.image),
      memory(other.memory),
      extent(other.extent),
      format(other.format),
      features(other.features) {
    other.dev = nullptr;
    other.ahb = nullptr;
    other.ownsAhb = false;
    other.image = VK_NULL_HANDLE;
    other.memory = VK_NULL_HANDLE;
}

AhbImage& AhbImage::operator=(AhbImage&& other) noexcept {
    if (this != &other) {
        reset();
        dev = other.dev;
        ahb = other.ahb;
        ownsAhb = other.ownsAhb;
        image = other.image;
        memory = other.memory;
        extent = other.extent;
        format = other.format;
        features = other.features;
        other.dev = nullptr;
        other.ahb = nullptr;
        other.ownsAhb = false;
        other.image = VK_NULL_HANDLE;
        other.memory = VK_NULL_HANDLE;
    }
    return *this;
}

void AhbImage::reset() {
    if (dev != nullptr && dev->device != VK_NULL_HANDLE) {
        if (image != VK_NULL_HANDLE && dev->fn.vkDestroyImage != nullptr) {
            dev->fn.vkDestroyImage(dev->device, image, nullptr);
        }
        if (memory != VK_NULL_HANDLE && dev->fn.vkFreeMemory != nullptr) {
            dev->fn.vkFreeMemory(dev->device, memory, nullptr);
        }
    }
    if (ownsAhb && ahb != nullptr) {
        AHardwareBuffer_release(ahb);
    }
    dev = nullptr;
    ahb = nullptr;
    ownsAhb = false;
    image = VK_NULL_HANDLE;
    memory = VK_NULL_HANDLE;
    extent = VkExtent2D{0, 0};
    format = VK_FORMAT_UNDEFINED;
    features = 0;
}

int32_t wrapAhb(FgDevice& dev, AhbImage& out) {
    if (out.ahb == nullptr || dev.device == VK_NULL_HANDLE || out.image != VK_NULL_HANDLE ||
        out.memory != VK_NULL_HANDLE || dev.fn.vkGetAndroidHardwareBufferPropertiesANDROID == nullptr ||
        dev.fn.vkCreateImage == nullptr || dev.fn.vkAllocateMemory == nullptr ||
        dev.fn.vkBindImageMemory == nullptr || dev.ifn.getPhysicalDeviceMemoryProperties == nullptr) {
        return FgResult::VK_INIT;
    }
    out.dev = &dev;  // from here on reset() frees what is created below

    AHardwareBuffer_Desc desc{};
    AHardwareBuffer_describe(out.ahb, &desc);

    VkAndroidHardwareBufferFormatPropertiesANDROID fmtProps{
        .sType = VK_STRUCTURE_TYPE_ANDROID_HARDWARE_BUFFER_FORMAT_PROPERTIES_ANDROID,
    };
    VkAndroidHardwareBufferPropertiesANDROID ahbProps{
        .sType = VK_STRUCTURE_TYPE_ANDROID_HARDWARE_BUFFER_PROPERTIES_ANDROID,
        .pNext = &fmtProps,
    };
    if (dev.fn.vkGetAndroidHardwareBufferPropertiesANDROID(dev.device, out.ahb, &ahbProps) != VK_SUCCESS) {
        EFG_AHB_LOGE("vkGetAndroidHardwareBufferPropertiesANDROID failed");
        return FgResult::VK_INIT;
    }

    // The driver may report UNDEFINED even for plain RGBA; then trust the
    // caller's hint. A YCbCr / external-format AHB has no hint and is refused,
    // and so is RGBX (never hinted, see the file header).
    const bool rgbx = desc.format == AHARDWAREBUFFER_FORMAT_R8G8B8X8_UNORM;
    VkFormat effective = fmtProps.format;
    const char* hint = "driver";
    if (effective == VK_FORMAT_UNDEFINED) {
        if (rgbx) {
            hint = "refused";
        } else if (out.format != VK_FORMAT_UNDEFINED) {
            effective = out.format;
            hint = "used";
        } else {
            hint = "none";
        }
    }
    if (firstSighting(desc.format, desc.usage)) {
        EFG_AHB_LOGI("AHB fmt=%u usage=0x%016llx stride=%u %ux%u -> vkFormat=%d externalFormat=0x%llx "
                     "formatFeatures=0x%x hint=%s effective=%d",
                     desc.format, static_cast<unsigned long long>(desc.usage), desc.stride, desc.width,
                     desc.height, static_cast<int>(fmtProps.format),
                     static_cast<unsigned long long>(fmtProps.externalFormat),
                     static_cast<unsigned>(fmtProps.formatFeatures), hint, static_cast<int>(effective));
    }
    if (effective == VK_FORMAT_UNDEFINED) {
        if (rgbx) {
            EFG_AHB_LOGE("RGBX AHB reported as VK_FORMAT_UNDEFINED (externalFormat=0x%llx): not read as "
                         "R8G8B8A8, import refused",
                         static_cast<unsigned long long>(fmtProps.externalFormat));
        } else {
            EFG_AHB_LOGE("AHB without Vulkan format (externalFormat=0x%llx)",
                         static_cast<unsigned long long>(fmtProps.externalFormat));
        }
        return FgResult::VK_INIT;
    }

    VkImageUsageFlags usage = usageFromFeatures(fmtProps.formatFeatures);
    if (!usageSupported(dev, effective, usage) && (usage & VK_IMAGE_USAGE_STORAGE_BIT) != 0) {
        usage &= ~static_cast<VkImageUsageFlags>(VK_IMAGE_USAGE_STORAGE_BIT);
        if (!usageSupported(dev, effective, usage)) usage = 0;
    }
    if (usage == 0) {
        EFG_AHB_LOGE("AHB format %d supports none of the wanted usages", static_cast<int>(effective));
        return FgResult::VK_INIT;
    }

    const VkExternalMemoryImageCreateInfo extImageInfo{
        .sType = VK_STRUCTURE_TYPE_EXTERNAL_MEMORY_IMAGE_CREATE_INFO,
        .handleTypes = VK_EXTERNAL_MEMORY_HANDLE_TYPE_ANDROID_HARDWARE_BUFFER_BIT_ANDROID,
    };
    const VkImageCreateInfo imageInfo{
        .sType = VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO,
        .pNext = &extImageInfo,
        .flags = 0,
        .imageType = VK_IMAGE_TYPE_2D,
        .format = effective,
        .extent = {desc.width, desc.height, 1},
        .mipLevels = 1,
        .arrayLayers = 1,
        .samples = VK_SAMPLE_COUNT_1_BIT,
        .tiling = VK_IMAGE_TILING_OPTIMAL,
        .usage = usage,
        .sharingMode = VK_SHARING_MODE_EXCLUSIVE,
        .initialLayout = VK_IMAGE_LAYOUT_UNDEFINED,
    };
    if (dev.fn.vkCreateImage(dev.device, &imageInfo, nullptr, &out.image) != VK_SUCCESS) {
        out.image = VK_NULL_HANDLE;
        EFG_AHB_LOGE("vkCreateImage failed for AHB %ux%u fmt=%d", desc.width, desc.height,
                     static_cast<int>(effective));
        return FgResult::VK_INIT;
    }

    const uint32_t typeIndex = pickMemoryType(dev, ahbProps.memoryTypeBits);
    if (typeIndex == UINT32_MAX) {
        EFG_AHB_LOGE("no memory type for AHB memoryTypeBits=0x%x", ahbProps.memoryTypeBits);
        return FgResult::VK_INIT;
    }
    const VkMemoryDedicatedAllocateInfo dedicated{
        .sType = VK_STRUCTURE_TYPE_MEMORY_DEDICATED_ALLOCATE_INFO,
        .image = out.image,
        .buffer = VK_NULL_HANDLE,
    };
    const VkImportAndroidHardwareBufferInfoANDROID importInfo{
        .sType = VK_STRUCTURE_TYPE_IMPORT_ANDROID_HARDWARE_BUFFER_INFO_ANDROID,
        .pNext = &dedicated,
        .buffer = out.ahb,
    };
    const VkMemoryAllocateInfo allocInfo{
        .sType = VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
        .pNext = &importInfo,
        .allocationSize = ahbProps.allocationSize,
        .memoryTypeIndex = typeIndex,
    };
    if (dev.fn.vkAllocateMemory(dev.device, &allocInfo, nullptr, &out.memory) != VK_SUCCESS) {
        out.memory = VK_NULL_HANDLE;
        EFG_AHB_LOGE("vkAllocateMemory (AHB import) failed");
        return FgResult::VK_INIT;
    }
    if (dev.fn.vkBindImageMemory(dev.device, out.image, out.memory, 0) != VK_SUCCESS) {
        EFG_AHB_LOGE("vkBindImageMemory failed");
        return FgResult::VK_INIT;
    }

    out.format = effective;
    out.features = fmtProps.formatFeatures;
    out.extent = VkExtent2D{desc.width, desc.height};
    return FgResult::OK;
}

int32_t createAhbImage(FgDevice& dev, uint32_t w, uint32_t h, VkFormat fmt, AhbImage& out) {
    out.reset();
    const uint32_t ahbFormat = ahbFormatFor(fmt);
    if (w == 0 || h == 0 || ahbFormat == 0) {
        EFG_AHB_LOGE("createAhbImage: invalid %ux%u fmt=%d", w, h, static_cast<int>(fmt));
        return FgResult::INVALID_ARG;
    }
    AHardwareBuffer_Desc desc{
        .width = w,
        .height = h,
        .layers = 1,
        .format = ahbFormat,
        .usage = AHARDWAREBUFFER_USAGE_GPU_SAMPLED_IMAGE | AHARDWAREBUFFER_USAGE_GPU_COLOR_OUTPUT,
        .stride = 0,
        .rfu0 = 0,
        .rfu1 = 0,
    };
    AHardwareBuffer* buffer = nullptr;
    if (AHardwareBuffer_allocate(&desc, &buffer) != 0 || buffer == nullptr) {
        EFG_AHB_LOGE("AHardwareBuffer_allocate(%ux%u, fmt=%u) failed", w, h, ahbFormat);
        return FgResult::VK_INIT;
    }
    out.ahb = buffer;
    out.ownsAhb = true;
    out.format = fmt;  // hint for drivers that report VK_FORMAT_UNDEFINED
    const int32_t rc = wrapAhb(dev, out);
    if (rc != FgResult::OK) out.reset();  // also releases the AHB
    return rc;
}

int32_t importAhbImage(FgDevice& dev, AHardwareBuffer* ahb, AhbImage& out) {
    out.reset();
    if (ahb == nullptr) return FgResult::INVALID_ARG;
    out.ahb = ahb;
    out.ownsAhb = false;
    // The AImageReader is RGBX_8888, which has no hint (FG-F): the driver must
    // report its Vulkan format itself.
    AHardwareBuffer_Desc desc{};
    AHardwareBuffer_describe(ahb, &desc);
    out.format = vkFormatHintFor(desc.format);
    const int32_t rc = wrapAhb(dev, out);
    if (rc != FgResult::OK) out.reset();  // the caller's AHB is not released
    return rc;
}

}  // namespace efg
