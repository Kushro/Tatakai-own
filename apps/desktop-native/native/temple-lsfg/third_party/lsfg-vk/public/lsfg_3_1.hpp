#pragma once

#include <vulkan/vulkan_core.h>

// Temple modification: portable export macro (was __attribute__((visibility("default")))) and the
// local image API for Windows.
#ifndef TEMPLE_LSFG_API
#define TEMPLE_LSFG_API
#endif
#ifdef TEMPLE_LSFG_LOCAL
#include "temple_local.hpp"
#endif

#include <functional>
#include <cstdint>
#include <string>
#include <vector>

#ifdef __ANDROID__
struct AHardwareBuffer;
#endif

namespace LSFG_3_1 {

    ///
    /// Initialize the LSFG library.
    ///
    /// @param deviceUUID The UUID of the Vulkan device to use.
    /// @param isHdr Whether the images are in HDR format.
    /// @param flowScale Internal flow scale factor.
    /// @param generationCount Number of frames to generate.
    /// @param loader Function to load shader source code by name.
    ///
    /// @throws LSFG::vulkan_error if Vulkan objects fail to initialize.
    ///
    TEMPLE_LSFG_API
    void initialize(uint64_t deviceUUID,
        bool isHdr, float flowScale, uint64_t generationCount,
        const std::function<std::vector<uint8_t>(const std::string&)>& loader);

    ///
    /// Create a new LSFG context on a swapchain.
    ///
    /// @param in0 File descriptor for the first input image.
    /// @param in1 File descriptor for the second input image.
    /// @param outN File descriptor for each output image. This defines the LSFG level.
    /// @param extent The size of the images
    /// @param format The format of the images.
    /// @return A unique identifier for the created context.
    ///
    /// @throws LSFG::vulkan_error if the context cannot be created.
    ///
    TEMPLE_LSFG_API
    int32_t createContext(
        int in0, int in1, const std::vector<int>& outN,
        VkExtent2D extent, VkFormat format);

#ifdef __ANDROID__
    ///
    /// Android-specific variant: share input/output images via AHardwareBuffer
    /// instead of opaque file descriptors. Required because Adreno/Mali drivers
    /// refuse vkGetMemoryFdKHR(OPAQUE_FD) on AHB-imported memory, breaking the
    /// FD-based path. The caller retains ownership of all AHBs and must keep
    /// them alive for the lifetime of the context.
    ///
    /// @param in0 First input image's AHardwareBuffer.
    /// @param in1 Second input image's AHardwareBuffer.
    /// @param outN Output image AHardwareBuffers, one per generated frame.
    /// @param extent Image dimensions.
    /// @param format Vulkan format of all images (must match the AHB format).
    /// @return Unique context identifier.
    ///
    TEMPLE_LSFG_API
    int32_t createContextFromAHB(
        AHardwareBuffer* in0, AHardwareBuffer* in1,
        const std::vector<AHardwareBuffer*>& outN,
        VkExtent2D extent, VkFormat format);
#endif

    ///
    /// Present a context.
    ///
    /// @param id Unique identifier of the context to present.
    /// @param inSem Semaphore to wait on before starting the generation.
    /// @param outSem Semaphores to signal once each output image is ready.
    ///
    /// @throws LSFG::vulkan_error if the context cannot be presented.
    ///
    TEMPLE_LSFG_API
    void presentContext(int32_t id, int inSem, const std::vector<int>& outSem);

    ///
    /// Delete an LSFG context.
    ///
    /// @param id Unique identifier of the context to delete.
    ///
    TEMPLE_LSFG_API
    void deleteContext(int32_t id);

    ///
    /// Deinitialize the LSFG library.
    ///
    TEMPLE_LSFG_API
    void finalize();

#if defined(__ANDROID__) || defined(TEMPLE_LSFG_LOCAL)
    /// Block until framegen's internal Vulkan device is idle. Used by the
    /// Android wrapper to sync between its own device (which writes input
    /// AHBs) and framegen's device (which reads them) — without an explicit
    /// shared semaphore this is the only safe way to avoid a write-after-read
    /// race on the shared AHardwareBuffer storage.
    TEMPLE_LSFG_API
    void waitIdle();
#endif

#ifdef TEMPLE_LSFG_LOCAL
    ///
    /// Temple modification: create a context on images owned by the context itself (no fd and no
    /// AHardwareBuffer sharing) and return the Vulkan handles so the caller can upload the input
    /// frames and read back the generated ones with its own staging buffers.
    ///
    /// @param extent The size of the images.
    /// @param format The format of the images (VK_FORMAT_R8G8B8A8_UNORM).
    ///
    /// @throws LSFG::vulkan_error if the context cannot be created.
    ///
    TEMPLE_LSFG_API
    TempleLsfg::LocalContext createContextLocal(VkExtent2D extent, VkFormat format);
#endif

}
