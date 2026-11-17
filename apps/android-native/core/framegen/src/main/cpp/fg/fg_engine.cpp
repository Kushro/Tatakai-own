// SPDX-License-Identifier: GPL-3.0-only
//
// Ported from Enigmata (app/src/main/cpp/fg) to Tatakai Next 0.4.0 (core/framegen): JNI class, library name,
// log tags and debug properties renamed; no other change.
//
// Adapted from FrankBarretta/LSFG-Android-Application
// (commit b84754199823615d32d65fe33ea59481dca88dcf), app/src/main/cpp/lsfg_render_loop.cpp, GPL-3.0:
// :991-1172 (initInSlotImageLayout, copyAhbImage: acquire/release barriers through
// VK_QUEUE_FAMILY_EXTERNAL), :1820-1979 (worker: presentsDone % 2 parity, bootstrap
// :1892-1903, presentContext + waitIdle), :2046-2083 (EMA smoothing, used here for the
// compute time), :2551-2589 (shutdown order: deleteContext -> finalize -> images),
// :425-481 (anti-artifacts luma threshold).
// Modifications: Enigmata, AGPL-3.0-or-later (combined under GPL-3.0 section 13).
//
// Enigmata frame generation (R13): engine thread (fg_engine.h is the contract).
//
// Threading
// - start() validates the config, allocates every GPU resource of the engine on
//   the caller thread (our device only, through dev.fn; no LSFG call), installs
//   the FrameSource listener and launches ONE std::thread. That thread runs
//   LSFG initialize + createContextFromAHB and reports the result through a
//   std::promise; start() waits for it and returns its code.
// - Every LSFG_3_1 / LSFG_3_1P call lives in three Impl methods (lsfgOpen,
//   lsfgPresent, lsfgClose). They are reached only from threadMain (and from
//   runLoop / applyPendingConfig / processFrame, which threadMain runs), and
//   each one checks onEngineThread() before touching LSFG.
// - updateConfig / setSourcePeriodNs / flush / setPaused only store requests
//   (atomics or a mutex-guarded std::optional); the engine thread applies them
//   between frames.
// - FrameSource::acquire() is single-threaded: only the engine thread calls it.
//
// Ownership
// - in0/in1 and outN (N-1 images, input size, RGBA8) are created here and
//   destroyed in stop() after the join, i.e. after deleteContext + finalize ran
//   on the engine thread (I3).
// - PresentRing slots (Enmienda 1, REV 13): start() allocates
//   requiredSlots(N) = min(2N + 1, kMaxPresentSlots) images of the INPUT size
//   (N = effective multiplier of the start config) and sets ring.slotCount
//   before any OutFrame is published. updateConfig with an N' whose
//   requiredSlots(N') exceeds ring.slotCount returns INVALID_ARG without
//   queueing anything; FgController.applyHot answers INVALID_ARG with a
//   stop/start cycle(), which allocates the ring for the new N. The engine never
//   destroys the slots: the owner (fg_jni teardown) destroys the ring after the
//   presenter has stopped (I3).
// - Slot images, as published to the presenter: VK_IMAGE_LAYOUT_GENERAL, owned
//   by dev.family, transfer writes made visible to later commands on dev.queue
//   (barrier dst = ALL_COMMANDS / MEMORY_READ). The engine writes a returned
//   slot from oldLayout UNDEFINED (full overwrite), so the presenter may return
//   it in any layout.
// - Free slots: an engine-local free list, seeded with every slot at start and
//   refilled from FreeSlotQueue (the presenter is its producer, we consume it).
//   No free slot -> the frame is not shown and droppedLate is incremented.
// - Stats (REV 17, E21): outFrames counts the frames really presented, i.e. the
//   SlotReturn entries with dropped == 0 (any epoch); generated counts those of
//   them with kind == kOutKindGenerated (late generated frames the presenter
//   discards are not counted, so generated <= outFrames). Neither is
//   incremented on publish. droppedLate counts the discards (current-epoch
//   SlotReturn with dropped == 1, queue full, no free slot).
//
// FrameSource listener
// - fg_jni never installs a listener (it only detaches it as the first step of
//   teardown), so the engine installs one in start() with onBufferRemoved only.
//   The callback only queues the AHardwareBuffer* under a mutex; the engine
//   thread erases the cached VkImage between frames (never while a copy using
//   it may run). Importing an AHB into Vulkan memory takes a reference on it,
//   so a cached pointer cannot be recycled for another buffer while cached.
//   stop() detaches the listener again (idempotent with fg_jni's step 1).
//
// Input fence: the AImage acquire fence (sync_fd) is waited with poll() and a
// 100 ms timeout on the engine thread (no external semaphore extension is
// enabled on our device). A fence that does not signal in time drops the frame.
//
// Anti-artifacts (cfg.antiArtifacts): in the same command buffer as the input
// copy, the new input is blitted with VK_FILTER_LINEAR into an 8x8 RGBA8 image
// of our own, copied into a 256-byte host-visible VkBuffer and read after the
// fence. No CPU access to any AHardwareBuffer (no CPU_READ usage). The mean
// absolute luma difference of the 64 cells against the previous real frame
// >= 26 (threshold of the reference, :425-481) is a scene cut: LSFG still
// presents (keeps the presentsDone parity) but only the real frame is shown
// for that interval. The 8x8 cells are bilinear samples (4 texels each), not
// exact box means.
//
// I4 (flush, or a PeriodEstimator discontinuity = timestamp backwards or a
// forward jump >= 1 s, REV 15): ring.epoch++ (the presenter drops older
// OutFrames and returns their slots), and the next frame seeds BOTH input slots
// (bootstrap) without changing presentsDone, so the parity with LSFG's frameIdx
// is kept. The external flush (seek) drains the reader backlog and releases
// EVERY image, the newest included (REV 16): the bootstrap seeds with the first
// frame that arrives after the flush.
// Lost frames (REV 15): a forward jump > 2 period and < 1 s is not a
// discontinuity. The real frame is presented, no generated frame is shown for
// that interval, the epoch and the queue are kept and the estimator only drops
// that interval as an outlier.
// I5 (pause): on the pause edge ring.epoch++ and the last real frame is shown
// once; while paused LSFG is not called and new frames (seek while paused) are
// copied straight into a slot and shown once as real frames; the input slots
// are re-seeded (bootstrap) on the next generated interval.
// Reader format (FG-F2): the producer may queue buffers in another 32-bit
// format than the RGBA8 of in[] and the slots (Adreno 740: mpv's 10-bit EGL
// config gives RGBA_1010102 -> VK_FORMAT_A2B10G10R10_UNORM_PACK32).
// vkCmdCopyImage between size-compatible formats copies raw bits (10-bit
// channels read as 8-bit -> folded tones), so a reader image whose format
// differs from the destination is transferred with vkCmdBlitImage over the
// same region as the copy (1:1, VK_FILTER_NEAREST), which converts UNORM to
// UNORM. Equal formats keep the plain copy. The first mismatch of a session
// logs one efg-engine line; a source without BLIT_SRC (or a destination
// without BLIT_DST) in its AHB formatFeatures fails the session with VK_INIT.
// I9: VK_ERROR_DEVICE_LOST (our device or an LSFG exception) -> state Failed,
// lastError DEVICE_LOST; any other error -> Failed with its code. The thread
// then runs deleteContext + finalize and exits; stop() joins it.

#include "fg_engine.h"

#include "ahb_image.h"
#include "fg_pacing.h"

#include "common/exception.hpp"
#include "lsfg_3_1.hpp"
#include "lsfg_3_1p.hpp"

#include <android/log.h>
#include <poll.h>
#include <time.h>

#include <algorithm>
#include <array>
#include <atomic>
#include <cerrno>
#include <cmath>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <exception>
#include <future>
#include <mutex>
#include <optional>
#include <string>
#include <system_error>
#include <thread>
#include <unordered_map>
#include <utility>
#include <vector>

namespace efg {

namespace {

constexpr const char* kTag = "efg-engine";
#define EFG_ENG_LOGE(...) __android_log_print(ANDROID_LOG_ERROR, kTag, __VA_ARGS__)
#define EFG_ENG_LOGW(...) __android_log_print(ANDROID_LOG_WARN, kTag, __VA_ARGS__)
#define EFG_ENG_LOGI(...) __android_log_print(ANDROID_LOG_INFO, kTag, __VA_ARGS__)

constexpr VkFormat kFormat = VK_FORMAT_R8G8B8A8_UNORM;
constexpr int32_t kAcquireTimeoutMs = 10;       // bounds the reaction time to stop/flush/pause
constexpr int kFencePollTimeoutMs = 100;        // AImage acquire fence
constexpr uint64_t kGpuWaitTimeoutNs = 2'000'000'000ull;
constexpr uint64_t kStopWaitTimeoutNs = 1'000'000'000ull;
constexpr int64_t kBaseMarginNs = 4'000'000;    // fixed part of the L safety margin
constexpr int64_t kMaxAdaptiveMarginNs = 50'000'000;
constexpr int64_t kDropMarginStepNs = 1'000'000;
constexpr int64_t kDefaultPeriodNs = 16'666'667;
constexpr int64_t kMinPeriodNs = 4'000'000;
constexpr int64_t kMaxPeriodNs = 250'000'000;
constexpr int64_t kTsSanityNs = 1'000'000'000;  // AImage timestamp farther than this from now -> use now
constexpr int kMaxReaderErrors = 100;           // consecutive acquire() READER results before failing
constexpr size_t kMaxCachedImports = 16;
constexpr uint32_t kLumaGrid = 8;
constexpr uint32_t kLumaCells = kLumaGrid * kLumaGrid;
constexpr uint32_t kLumaCutThreshold = 26;
constexpr uint64_t kVerboseEveryFrames = 300;
constexpr size_t kMaxCopyJobs = static_cast<size_t>(kMaxMultiplier);  // N-1 generated + 1 real

// REV 13: presenter slots for multiplier N, 2N + 1 capped at kMaxPresentSlots.
uint32_t requiredSlots(int32_t multiplier) {
    const uint32_t n = multiplier > 0 ? static_cast<uint32_t>(multiplier) : 0u;
    return std::min<uint32_t>(2u * n + 1u, kMaxPresentSlots);
}

int64_t nowNs() {
    timespec ts{};
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return static_cast<int64_t>(ts.tv_sec) * 1'000'000'000LL + static_cast<int64_t>(ts.tv_nsec);
}

bool validConfig(const FgConfigN& c) {
    return c.multiplier >= kMinMultiplier && c.multiplier <= kMaxMultiplier && c.inputWidth > 0 &&
           c.inputHeight > 0 && c.outputWidth > 0 && c.outputHeight > 0 && std::isfinite(c.flowScale) &&
           c.flowScale > 0.0f && !c.shaderDir.empty();
}

// The user flow scale (0.25..1.0, larger = finer) is the reciprocal of what
// LSFG takes (lsfg-vk-android/src/context.cpp:103, render_loop :2311-2315).
float lsfgFlowScale(float user) {
    const float u = std::clamp(user, 0.25f, 1.0f);
    return 1.0f / u;
}

int32_t vkResultCode(VkResult r) {
    return r == VK_ERROR_DEVICE_LOST ? FgResult::DEVICE_LOST : FgResult::CONTEXT;
}

// LSFG reports failures as LSFG::vulkan_error (VkResult) or plain std::exception.
int32_t lsfgErrorCode(const std::exception& e, int32_t fallback) {
    if (const auto* ve = dynamic_cast<const LSFG::vulkan_error*>(&e); ve != nullptr) {
        if (ve->error() == VK_ERROR_DEVICE_LOST) return FgResult::DEVICE_LOST;
    }
    const char* what = e.what();
    if (what != nullptr &&
        (std::strstr(what, "DEVICE_LOST") != nullptr || std::strstr(what, "error -4") != nullptr)) {
        return FgResult::DEVICE_LOST;
    }
    return fallback;
}

VkImageMemoryBarrier imageBarrier(VkImage image, VkAccessFlags srcAccess, VkAccessFlags dstAccess,
                                  VkImageLayout oldLayout, VkImageLayout newLayout, uint32_t srcFamily,
                                  uint32_t dstFamily) {
    VkImageMemoryBarrier b{};
    b.sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER;
    b.pNext = nullptr;
    b.srcAccessMask = srcAccess;
    b.dstAccessMask = dstAccess;
    b.oldLayout = oldLayout;
    b.newLayout = newLayout;
    b.srcQueueFamilyIndex = srcFamily;
    b.dstQueueFamilyIndex = dstFamily;
    b.image = image;
    b.subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
    return b;
}

VkImageCopy fullCopy(VkExtent2D src, VkExtent2D dst) {
    VkImageCopy region{};
    region.srcSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1};
    region.srcOffset = {0, 0, 0};
    region.dstSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1};
    region.dstOffset = {0, 0, 0};
    region.extent = {std::min(src.width, dst.width), std::min(src.height, dst.height), 1};
    return region;
}

// FG-F2: a format-converting blit needs BLIT_SRC on the source and BLIT_DST on
// the destination. Features the driver did not report (0) count as missing.
bool canBlitConvert(const AhbImage& src, const AhbImage& dst) {
    return (src.features & VK_FORMAT_FEATURE_BLIT_SRC_BIT) != 0 &&
           (dst.features & VK_FORMAT_FEATURE_BLIT_DST_BIT) != 0;
}

// Waits for an AImage acquire fence (sync_fd). The fd stays owned by the
// caller (InFrameHandle closes it). True when signalled (or no fence).
bool waitFenceFd(int fd) {
    if (fd < 0) return true;
    pollfd p{};
    p.fd = fd;
    p.events = POLLIN;
    for (;;) {
        p.revents = 0;
        const int r = ::poll(&p, 1, kFencePollTimeoutMs);
        if (r > 0) return (p.revents & POLLIN) != 0;
        if (r == 0) return false;
        if (errno == EINTR) continue;
        return false;
    }
}

}  // namespace

struct FgEngine::Impl {
    // 8x8 luma probe (anti-artifacts), on our device.
    struct LumaProbe {
        VkImage image = VK_NULL_HANDLE;
        VkDeviceMemory imageMemory = VK_NULL_HANDLE;
        VkBuffer buffer = VK_NULL_HANDLE;
        VkDeviceMemory bufferMemory = VK_NULL_HANDLE;
        void* mapped = nullptr;
        bool coherent = false;
        bool ok = false;
        bool prevValid = false;
        std::array<uint8_t, kLumaCells> prev{};
    };

    struct CopyJob {
        const AhbImage* src = nullptr;
        bool srcFromReader = false;  // AImageReader buffer: acquired with oldLayout UNDEFINED
        uint32_t slot = 0;
    };

    FgStatsSnapshot stats;

    // Borrowed from the caller of start(); valid until stop() returns.
    FgDevice* dev = nullptr;
    FrameSource* src = nullptr;
    PresentRing* ring = nullptr;
    FreeSlotQueue* freeSlots = nullptr;

    // Control (written by the JNI thread, read by the engine thread).
    std::thread thread;
    std::thread::id engineThreadId{};
    std::atomic<bool> running{false};
    std::atomic<bool> failed{false};
    std::atomic<bool> stopReq{false};
    std::atomic<bool> pausedReq{false};
    std::atomic<uint32_t> flushSeq{0};
    std::atomic<int64_t> periodHintNs{0};
    std::mutex cfgMutex;
    std::optional<FgConfigN> pendingCfg;  // guarded by cfgMutex
    int32_t startInW = 0;
    int32_t startInH = 0;
    int32_t startOutW = 0;
    int32_t startOutH = 0;
    bool listenerInstalled = false;

    // Buffers dropped by the AImageReader (listener thread -> engine thread).
    std::mutex removedMutex;
    std::vector<AHardwareBuffer*> removed;  // guarded by removedMutex

    // Engine thread (or start()/stop() while the thread does not run).
    FgConfigN cfg;
    ShaderLoader loader;
    VkCommandPool pool = VK_NULL_HANDLE;
    VkCommandBuffer cb = VK_NULL_HANDLE;
    VkFence fence = VK_NULL_HANDLE;
    bool fenceInFlight = false;
    std::array<AhbImage, 2> in{};
    std::vector<AhbImage> outN;
    std::unordered_map<AHardwareBuffer*, AhbImage> cache;
    LumaProbe luma;
    std::vector<uint32_t> freeList;
    std::array<bool, kMaxPresentSlots> slotFree{};

    // LSFG state (engine thread only).
    bool lsfgInitialized = false;
    bool lsfgPerf = true;  // variant that is initialized (3.1P when true)
    bool haveCtx = false;
    int32_t ctxId = 0;
    uint64_t presentsDone = 0;  // mirrors LSFG's frameIdx of the current context

    // Pipeline state (engine thread only).
    uint32_t epoch = 0;
    uint32_t seenFlushSeq = 0;
    bool pausedNow = false;
    bool bootstrapPending = true;
    bool haveLastReal = false;
    int lastRealSlot = 0;
    bool guardArmed = false;
    int64_t guardPeriodNs = 0;
    PeriodEstimator estimator;
    int64_t prevTsNs = -1;
    int64_t lastPeriodNs = 0;
    int64_t computeAvgNs = 0;
    int64_t errEmaNs = 0;
    int readerErrors = 0;
    uint64_t framesDone = 0;
    bool blitConvertLogged = false;  // FG-F2: one efg-engine line per session

    // ---------------------------------------------------------------------
    // Small helpers
    // ---------------------------------------------------------------------

    bool onEngineThread() const { return std::this_thread::get_id() == engineThreadId; }

    void fail(int32_t code) {
        failed.store(true, std::memory_order_relaxed);
        stats.set(kStatLastError, code);
        stats.set(kStatState, static_cast<int64_t>(FgStatus::Failed));
        EFG_ENG_LOGE("engine failed: %d", static_cast<int>(code));
    }

    uint32_t pickMemoryType(uint32_t typeBits, VkMemoryPropertyFlags want) const {
        VkPhysicalDeviceMemoryProperties props{};
        dev->ifn.getPhysicalDeviceMemoryProperties(dev->phys, &props);
        for (uint32_t i = 0; i < props.memoryTypeCount; ++i) {
            if ((typeBits & (1u << i)) == 0) continue;
            if ((props.memoryTypes[i].propertyFlags & want) == want) return i;
        }
        return UINT32_MAX;
    }

    bool memoryTypeHas(uint32_t index, VkMemoryPropertyFlags flags) const {
        VkPhysicalDeviceMemoryProperties props{};
        dev->ifn.getPhysicalDeviceMemoryProperties(dev->phys, &props);
        return index < props.memoryTypeCount && (props.memoryTypes[index].propertyFlags & flags) == flags;
    }

    // ---------------------------------------------------------------------
    // Command buffer + fence (our device)
    // ---------------------------------------------------------------------

    int32_t createCommandObjects() {
        VkCommandPoolCreateInfo pci{};
        pci.sType = VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO;
        pci.flags = VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT;
        pci.queueFamilyIndex = dev->family;
        if (dev->fn.vkCreateCommandPool(dev->device, &pci, nullptr, &pool) != VK_SUCCESS) {
            pool = VK_NULL_HANDLE;
            return FgResult::CONTEXT;
        }
        VkCommandBufferAllocateInfo cai{};
        cai.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO;
        cai.commandPool = pool;
        cai.level = VK_COMMAND_BUFFER_LEVEL_PRIMARY;
        cai.commandBufferCount = 1;
        if (dev->fn.vkAllocateCommandBuffers(dev->device, &cai, &cb) != VK_SUCCESS) {
            cb = VK_NULL_HANDLE;
            return FgResult::CONTEXT;
        }
        VkFenceCreateInfo fci{};
        fci.sType = VK_STRUCTURE_TYPE_FENCE_CREATE_INFO;
        if (dev->fn.vkCreateFence(dev->device, &fci, nullptr, &fence) != VK_SUCCESS) {
            fence = VK_NULL_HANDLE;
            return FgResult::CONTEXT;
        }
        return FgResult::OK;
    }

    bool beginCmd() {
        if (dev->fn.vkResetCommandBuffer(cb, 0) != VK_SUCCESS) return false;
        VkCommandBufferBeginInfo bi{};
        bi.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO;
        bi.flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT;
        return dev->fn.vkBeginCommandBuffer(cb, &bi) == VK_SUCCESS;
    }

    // Ends `cb`, submits it through dev.submit (queueMutex) with our fence and
    // waits for the fence. Returns OK, DEVICE_LOST or CONTEXT.
    int32_t submitAndWait() {
        VkResult r = dev->fn.vkEndCommandBuffer(cb);
        if (r != VK_SUCCESS) return vkResultCode(r);
        r = dev->fn.vkResetFences(dev->device, 1, &fence);
        if (r != VK_SUCCESS) return vkResultCode(r);
        VkSubmitInfo si{};
        si.sType = VK_STRUCTURE_TYPE_SUBMIT_INFO;
        si.commandBufferCount = 1;
        si.pCommandBuffers = &cb;
        r = dev->submit(si, fence);
        if (r != VK_SUCCESS) {
            EFG_ENG_LOGE("vkQueueSubmit failed: %d", static_cast<int>(r));
            return vkResultCode(r);
        }
        fenceInFlight = true;
        r = dev->fn.vkWaitForFences(dev->device, 1, &fence, VK_TRUE, kGpuWaitTimeoutNs);
        if (r != VK_SUCCESS) {
            EFG_ENG_LOGE("vkWaitForFences failed: %d", static_cast<int>(r));
            return vkResultCode(r);
        }
        fenceInFlight = false;
        return FgResult::OK;
    }

    // Transitions freshly created images: the LSFG-shared ones (in0/in1/outN)
    // to GENERAL released to EXTERNAL (render_loop :991-1034), the presenter
    // slots to GENERAL on our family.
    int32_t initLayouts(const std::vector<VkImage>& shared, const std::vector<VkImage>& own) {
        if (shared.empty() && own.empty()) return FgResult::OK;
        if (!beginCmd()) return FgResult::CONTEXT;
        std::vector<VkImageMemoryBarrier> barriers;
        barriers.reserve(shared.size() + own.size());
        for (VkImage img : shared) {
            barriers.push_back(imageBarrier(img, 0, 0, VK_IMAGE_LAYOUT_UNDEFINED, VK_IMAGE_LAYOUT_GENERAL,
                                            dev->family, VK_QUEUE_FAMILY_EXTERNAL));
        }
        for (VkImage img : own) {
            barriers.push_back(imageBarrier(img, 0, VK_ACCESS_MEMORY_READ_BIT, VK_IMAGE_LAYOUT_UNDEFINED,
                                            VK_IMAGE_LAYOUT_GENERAL, VK_QUEUE_FAMILY_IGNORED,
                                            VK_QUEUE_FAMILY_IGNORED));
        }
        dev->fn.vkCmdPipelineBarrier(cb, VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT, VK_PIPELINE_STAGE_BOTTOM_OF_PIPE_BIT, 0,
                                     0, nullptr, 0, nullptr, static_cast<uint32_t>(barriers.size()),
                                     barriers.data());
        return submitAndWait();
    }

    // (Re)creates outN with the N-1 images of the current config.
    int32_t createOutputs() {
        outN.clear();
        const int count = cfg.multiplier - 1;
        outN.resize(static_cast<size_t>(count));
        std::vector<VkImage> shared;
        for (int i = 0; i < count; ++i) {
            if (createAhbImage(*dev, static_cast<uint32_t>(cfg.inputWidth), static_cast<uint32_t>(cfg.inputHeight),
                               kFormat, outN[static_cast<size_t>(i)]) != FgResult::OK) {
                EFG_ENG_LOGE("createAhbImage(outN %d) failed", i);
                return FgResult::CONTEXT;
            }
            shared.push_back(outN[static_cast<size_t>(i)].image);
        }
        return initLayouts(shared, {});
    }

    // ---------------------------------------------------------------------
    // Luma probe
    // ---------------------------------------------------------------------

    void createLuma() {
        auto& fn = dev->fn;
        const VkDevice d = dev->device;
        VkImageCreateInfo ici{};
        ici.sType = VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO;
        ici.imageType = VK_IMAGE_TYPE_2D;
        ici.format = kFormat;
        ici.extent = {kLumaGrid, kLumaGrid, 1};
        ici.mipLevels = 1;
        ici.arrayLayers = 1;
        ici.samples = VK_SAMPLE_COUNT_1_BIT;
        ici.tiling = VK_IMAGE_TILING_OPTIMAL;
        ici.usage = VK_IMAGE_USAGE_TRANSFER_DST_BIT | VK_IMAGE_USAGE_TRANSFER_SRC_BIT;
        ici.sharingMode = VK_SHARING_MODE_EXCLUSIVE;
        ici.initialLayout = VK_IMAGE_LAYOUT_UNDEFINED;
        if (fn.vkCreateImage(d, &ici, nullptr, &luma.image) != VK_SUCCESS) {
            luma.image = VK_NULL_HANDLE;
            return;
        }
        VkMemoryRequirements req{};
        fn.vkGetImageMemoryRequirements(d, luma.image, &req);
        uint32_t type = pickMemoryType(req.memoryTypeBits, VK_MEMORY_PROPERTY_DEVICE_LOCAL_BIT);
        if (type == UINT32_MAX) type = pickMemoryType(req.memoryTypeBits, 0);
        if (type == UINT32_MAX) return;
        VkMemoryAllocateInfo mai{};
        mai.sType = VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO;
        mai.allocationSize = req.size;
        mai.memoryTypeIndex = type;
        if (fn.vkAllocateMemory(d, &mai, nullptr, &luma.imageMemory) != VK_SUCCESS) {
            luma.imageMemory = VK_NULL_HANDLE;
            return;
        }
        if (fn.vkBindImageMemory(d, luma.image, luma.imageMemory, 0) != VK_SUCCESS) return;

        VkBufferCreateInfo bci{};
        bci.sType = VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO;
        bci.size = static_cast<VkDeviceSize>(kLumaCells) * 4u;
        bci.usage = VK_BUFFER_USAGE_TRANSFER_DST_BIT;
        bci.sharingMode = VK_SHARING_MODE_EXCLUSIVE;
        if (fn.vkCreateBuffer(d, &bci, nullptr, &luma.buffer) != VK_SUCCESS) {
            luma.buffer = VK_NULL_HANDLE;
            return;
        }
        fn.vkGetBufferMemoryRequirements(d, luma.buffer, &req);
        type = pickMemoryType(req.memoryTypeBits,
                              VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT);
        if (type == UINT32_MAX) type = pickMemoryType(req.memoryTypeBits, VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT);
        if (type == UINT32_MAX) return;
        luma.coherent = memoryTypeHas(type, VK_MEMORY_PROPERTY_HOST_COHERENT_BIT);
        mai.allocationSize = req.size;
        mai.memoryTypeIndex = type;
        if (fn.vkAllocateMemory(d, &mai, nullptr, &luma.bufferMemory) != VK_SUCCESS) {
            luma.bufferMemory = VK_NULL_HANDLE;
            return;
        }
        if (fn.vkBindBufferMemory(d, luma.buffer, luma.bufferMemory, 0) != VK_SUCCESS) return;
        if (fn.vkMapMemory(d, luma.bufferMemory, 0, VK_WHOLE_SIZE, 0, &luma.mapped) != VK_SUCCESS) {
            luma.mapped = nullptr;
            return;
        }
        luma.ok = true;
    }

    void destroyLuma() {
        if (dev == nullptr || dev->device == VK_NULL_HANDLE) {
            luma = LumaProbe{};
            return;
        }
        auto& fn = dev->fn;
        const VkDevice d = dev->device;
        if (luma.mapped != nullptr) fn.vkUnmapMemory(d, luma.bufferMemory);
        if (luma.buffer != VK_NULL_HANDLE) fn.vkDestroyBuffer(d, luma.buffer, nullptr);
        if (luma.bufferMemory != VK_NULL_HANDLE) fn.vkFreeMemory(d, luma.bufferMemory, nullptr);
        if (luma.image != VK_NULL_HANDLE) fn.vkDestroyImage(d, luma.image, nullptr);
        if (luma.imageMemory != VK_NULL_HANDLE) fn.vkFreeMemory(d, luma.imageMemory, nullptr);
        luma = LumaProbe{};
    }

    // Records the 8x8 downscale of `srcImage` (already in TRANSFER_SRC_OPTIMAL)
    // and its copy into the host-visible buffer.
    void recordLuma(VkImage srcImage, VkExtent2D ext) {
        auto& fn = dev->fn;
        VkImageMemoryBarrier toDst =
            imageBarrier(luma.image, 0, VK_ACCESS_TRANSFER_WRITE_BIT, VK_IMAGE_LAYOUT_UNDEFINED,
                         VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, VK_QUEUE_FAMILY_IGNORED, VK_QUEUE_FAMILY_IGNORED);
        fn.vkCmdPipelineBarrier(cb, VK_PIPELINE_STAGE_TRANSFER_BIT, VK_PIPELINE_STAGE_TRANSFER_BIT, 0, 0, nullptr, 0,
                                nullptr, 1, &toDst);
        VkImageBlit blit{};
        blit.srcSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1};
        blit.srcOffsets[0] = {0, 0, 0};
        blit.srcOffsets[1] = {static_cast<int32_t>(ext.width), static_cast<int32_t>(ext.height), 1};
        blit.dstSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1};
        blit.dstOffsets[0] = {0, 0, 0};
        blit.dstOffsets[1] = {static_cast<int32_t>(kLumaGrid), static_cast<int32_t>(kLumaGrid), 1};
        fn.vkCmdBlitImage(cb, srcImage, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, luma.image,
                          VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1, &blit, VK_FILTER_LINEAR);
        VkImageMemoryBarrier toSrc = imageBarrier(
            luma.image, VK_ACCESS_TRANSFER_WRITE_BIT, VK_ACCESS_TRANSFER_READ_BIT, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
            VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, VK_QUEUE_FAMILY_IGNORED, VK_QUEUE_FAMILY_IGNORED);
        fn.vkCmdPipelineBarrier(cb, VK_PIPELINE_STAGE_TRANSFER_BIT, VK_PIPELINE_STAGE_TRANSFER_BIT, 0, 0, nullptr, 0,
                                nullptr, 1, &toSrc);
        VkBufferImageCopy region{};
        region.bufferOffset = 0;
        region.bufferRowLength = 0;
        region.bufferImageHeight = 0;
        region.imageSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1};
        region.imageOffset = {0, 0, 0};
        region.imageExtent = {kLumaGrid, kLumaGrid, 1};
        fn.vkCmdCopyImageToBuffer(cb, luma.image, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, luma.buffer, 1, &region);
        VkBufferMemoryBarrier hostRead{};
        hostRead.sType = VK_STRUCTURE_TYPE_BUFFER_MEMORY_BARRIER;
        hostRead.srcAccessMask = VK_ACCESS_TRANSFER_WRITE_BIT;
        hostRead.dstAccessMask = VK_ACCESS_HOST_READ_BIT;
        hostRead.srcQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
        hostRead.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
        hostRead.buffer = luma.buffer;
        hostRead.offset = 0;
        hostRead.size = VK_WHOLE_SIZE;
        fn.vkCmdPipelineBarrier(cb, VK_PIPELINE_STAGE_TRANSFER_BIT, VK_PIPELINE_STAGE_HOST_BIT, 0, 0, nullptr, 1,
                                &hostRead, 0, nullptr);
    }

    // Reads the probe written by the last submit. True = scene cut against the
    // previous real frame (only evaluated when `compare`).
    bool readLuma(bool compare) {
        if (!luma.coherent) {
            VkMappedMemoryRange range{};
            range.sType = VK_STRUCTURE_TYPE_MAPPED_MEMORY_RANGE;
            range.memory = luma.bufferMemory;
            range.offset = 0;
            range.size = VK_WHOLE_SIZE;
            dev->fn.vkInvalidateMappedMemoryRanges(dev->device, 1, &range);
        }
        const auto* px = static_cast<const uint8_t*>(luma.mapped);
        std::array<uint8_t, kLumaCells> cur{};
        for (uint32_t i = 0; i < kLumaCells; ++i) {
            const uint8_t* p = px + static_cast<size_t>(i) * 4u;
            cur[i] = static_cast<uint8_t>((77u * p[0] + 150u * p[1] + 29u * p[2]) >> 8);
        }
        bool cut = false;
        if (compare && luma.prevValid) {
            uint32_t total = 0;
            for (uint32_t i = 0; i < kLumaCells; ++i) {
                total += static_cast<uint32_t>(std::abs(static_cast<int>(cur[i]) - static_cast<int>(luma.prev[i])));
            }
            cut = (total / kLumaCells) >= kLumaCutThreshold;
        }
        luma.prev = cur;
        luma.prevValid = true;
        return cut;
    }

    // ---------------------------------------------------------------------
    // Copies
    // ---------------------------------------------------------------------

    // srcImg (TRANSFER_SRC_OPTIMAL) -> dst (TRANSFER_DST_OPTIMAL) over the
    // fullCopy region. Same format: raw copy. Different format (FG-F2): 1:1
    // blit of the same region, which converts; checkReaderFormat() verified
    // the blit features before any reader image gets here.
    void recordTransfer(const AhbImage& srcImg, const AhbImage& dst) {
        const VkImageCopy region = fullCopy(srcImg.extent, dst.extent);
        if (srcImg.format == dst.format) {
            dev->fn.vkCmdCopyImage(cb, srcImg.image, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, dst.image,
                                   VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1, &region);
            return;
        }
        const VkOffset3D end{static_cast<int32_t>(region.extent.width), static_cast<int32_t>(region.extent.height),
                             1};
        VkImageBlit blit{};
        blit.srcSubresource = region.srcSubresource;
        blit.srcOffsets[0] = region.srcOffset;
        blit.srcOffsets[1] = end;
        blit.dstSubresource = region.dstSubresource;
        blit.dstOffsets[0] = region.dstOffset;
        blit.dstOffsets[1] = end;
        dev->fn.vkCmdBlitImage(cb, srcImg.image, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, dst.image,
                               VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1, &blit, VK_FILTER_NEAREST);
    }

    // FG-F2: a reader image in another format than in[] / the slots must be
    // blit-convertible into both. Logs the first mismatch of the session once;
    // returns VK_INIT (session fails, FG stops) when the blit is not supported.
    int32_t checkReaderFormat(const AhbImage& srcImg) {
        if (srcImg.format == kFormat) return FgResult::OK;
        if (!canBlitConvert(srcImg, in[0]) || ring->slotCount == 0 || !canBlitConvert(srcImg, ring->slots[0])) {
            EFG_ENG_LOGE("reader format %d (features 0x%x) cannot be blit-converted to %d (features 0x%x)",
                         static_cast<int>(srcImg.format), static_cast<unsigned>(srcImg.features),
                         static_cast<int>(kFormat), static_cast<unsigned>(in[0].features));
            return FgResult::VK_INIT;
        }
        if (!blitConvertLogged) {
            blitConvertLogged = true;
            EFG_ENG_LOGI("reader format %d != slot %d -> blit-convert (%ux%u)", static_cast<int>(srcImg.format),
                         static_cast<int>(kFormat), srcImg.extent.width, srcImg.extent.height);
        }
        return FgResult::OK;
    }

    // Reader image -> in[newSlot] (or both slots on bootstrap), barriers
    // acquire from / release to VK_QUEUE_FAMILY_EXTERNAL (render_loop :1045-1172).
    int32_t copyInput(const AhbImage& srcImg, bool both, int newSlot, bool withLuma) {
        auto& fn = dev->fn;
        if (!beginCmd()) return FgResult::CONTEXT;
        std::array<VkImageMemoryBarrier, 3> pre{};
        uint32_t np = 0;
        pre[np++] = imageBarrier(srcImg.image, 0, VK_ACCESS_TRANSFER_READ_BIT, VK_IMAGE_LAYOUT_UNDEFINED,
                                 VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, VK_QUEUE_FAMILY_EXTERNAL, dev->family);
        std::array<int, 2> targets{newSlot, 1 - newSlot};
        const int targetCount = both ? 2 : 1;
        for (int t = 0; t < targetCount; ++t) {
            pre[np++] = imageBarrier(in[static_cast<size_t>(targets[static_cast<size_t>(t)])].image, 0,
                                     VK_ACCESS_TRANSFER_WRITE_BIT, VK_IMAGE_LAYOUT_GENERAL,
                                     VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, VK_QUEUE_FAMILY_EXTERNAL, dev->family);
        }
        fn.vkCmdPipelineBarrier(cb, VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT, VK_PIPELINE_STAGE_TRANSFER_BIT, 0, 0, nullptr,
                                0, nullptr, np, pre.data());
        for (int t = 0; t < targetCount; ++t) {
            recordTransfer(srcImg, in[static_cast<size_t>(targets[static_cast<size_t>(t)])]);
        }
        if (withLuma) recordLuma(srcImg.image, srcImg.extent);

        std::array<VkImageMemoryBarrier, 3> post{};
        uint32_t nq = 0;
        for (int t = 0; t < targetCount; ++t) {
            post[nq++] = imageBarrier(in[static_cast<size_t>(targets[static_cast<size_t>(t)])].image,
                                      VK_ACCESS_TRANSFER_WRITE_BIT, 0, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
                                      VK_IMAGE_LAYOUT_GENERAL, dev->family, VK_QUEUE_FAMILY_EXTERNAL);
        }
        post[nq++] = imageBarrier(srcImg.image, 0, 0, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, VK_IMAGE_LAYOUT_GENERAL,
                                  dev->family, VK_QUEUE_FAMILY_EXTERNAL);
        fn.vkCmdPipelineBarrier(cb, VK_PIPELINE_STAGE_TRANSFER_BIT, VK_PIPELINE_STAGE_BOTTOM_OF_PIPE_BIT, 0, 0,
                                nullptr, 0, nullptr, nq, post.data());
        return submitAndWait();
    }

    // External images (outN, in[x]: GENERAL owned by EXTERNAL; or a reader
    // image) -> presenter slots (own family, left in GENERAL).
    int32_t copyToSlots(const CopyJob* jobs, size_t count) {
        if (count == 0) return FgResult::OK;
        if (count > kMaxCopyJobs) return FgResult::CONTEXT;
        auto& fn = dev->fn;
        if (!beginCmd()) return FgResult::CONTEXT;
        std::array<VkImageMemoryBarrier, 2 * kMaxCopyJobs> pre{};
        std::array<VkImageMemoryBarrier, 2 * kMaxCopyJobs> post{};
        uint32_t n = 0;
        for (size_t j = 0; j < count; ++j) {
            const CopyJob& job = jobs[j];
            const AhbImage& slot = ring->slots[job.slot];
            pre[n] = imageBarrier(job.src->image, 0, VK_ACCESS_TRANSFER_READ_BIT,
                                  job.srcFromReader ? VK_IMAGE_LAYOUT_UNDEFINED : VK_IMAGE_LAYOUT_GENERAL,
                                  VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, VK_QUEUE_FAMILY_EXTERNAL, dev->family);
            post[n] = imageBarrier(job.src->image, 0, 0, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, VK_IMAGE_LAYOUT_GENERAL,
                                   dev->family, VK_QUEUE_FAMILY_EXTERNAL);
            ++n;
            pre[n] = imageBarrier(slot.image, 0, VK_ACCESS_TRANSFER_WRITE_BIT, VK_IMAGE_LAYOUT_UNDEFINED,
                                  VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, VK_QUEUE_FAMILY_IGNORED,
                                  VK_QUEUE_FAMILY_IGNORED);
            post[n] = imageBarrier(
                slot.image, VK_ACCESS_TRANSFER_WRITE_BIT,
                VK_ACCESS_TRANSFER_READ_BIT | VK_ACCESS_SHADER_READ_BIT | VK_ACCESS_MEMORY_READ_BIT,
                VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, VK_IMAGE_LAYOUT_GENERAL, VK_QUEUE_FAMILY_IGNORED,
                VK_QUEUE_FAMILY_IGNORED);
            ++n;
        }
        // ALL_COMMANDS as source: the slot may have been read by the presenter
        // on the same queue just before it was returned (WAR).
        fn.vkCmdPipelineBarrier(cb, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, VK_PIPELINE_STAGE_TRANSFER_BIT, 0, 0, nullptr,
                                0, nullptr, n, pre.data());
        for (size_t j = 0; j < count; ++j) {
            recordTransfer(*jobs[j].src, ring->slots[jobs[j].slot]);
        }
        fn.vkCmdPipelineBarrier(cb, VK_PIPELINE_STAGE_TRANSFER_BIT, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, 0, 0, nullptr,
                                0, nullptr, n, post.data());
        return submitAndWait();
    }

    // ---------------------------------------------------------------------
    // Import cache (AHardwareBuffer* -> VkImage), engine thread only
    // ---------------------------------------------------------------------

    void processRemoved() {
        std::vector<AHardwareBuffer*> gone;
        {
            std::lock_guard<std::mutex> lock(removedMutex);
            gone.swap(removed);
        }
        for (AHardwareBuffer* b : gone) cache.erase(b);  // AhbImage dtor: vkDestroyImage + vkFreeMemory
    }

    const AhbImage* importCached(AHardwareBuffer* ahb, int32_t& rc) {
        rc = FgResult::OK;
        auto it = cache.find(ahb);
        if (it != cache.end()) return &it->second;
        if (cache.size() >= kMaxCachedImports) cache.clear();  // bounded; GPU is idle between frames
        AhbImage img;
        rc = importAhbImage(*dev, ahb, img);
        if (rc != FgResult::OK) return nullptr;
        auto res = cache.emplace(ahb, std::move(img));
        return &res.first->second;
    }

    // ---------------------------------------------------------------------
    // Presenter slots
    // ---------------------------------------------------------------------

    void noteReturn(const SlotReturn& r) {
        // REV 17 / E21: outFrames and generated = frames the presenter really
        // presented (any epoch).
        if (r.dropped == 0) {
            stats.add(kStatOutFrames, 1);
            if (r.kind == kOutKindGenerated) stats.add(kStatGenerated, 1);
        }
        if (r.epoch != epoch) return;  // old-epoch drops are I4 flushes, not lateness
        if (r.dropped != 0) {
            stats.add(kStatDroppedLate, 1);
            errEmaNs = std::min(kMaxAdaptiveMarginNs, errEmaNs + kDropMarginStepNs);
        } else if (r.presentErrorNs != 0) {
            const int64_t e = std::clamp<int64_t>(r.presentErrorNs, 0, kMaxAdaptiveMarginNs);
            errEmaNs += (e - errEmaNs) / 8;
        }
    }

    void drainReturns() {
        SlotReturn r{};
        while (freeSlots->pop(r)) {
            if (r.slot < ring->slotCount && !slotFree[r.slot]) {
                slotFree[r.slot] = true;
                freeList.push_back(r.slot);
            }
            noteReturn(r);
        }
    }

    uint32_t takeSlot() {
        const uint32_t s = freeList.back();
        freeList.pop_back();
        slotFree[s] = false;
        return s;
    }

    void giveBackSlot(uint32_t s) {
        if (s < ring->slotCount && !slotFree[s]) {
            slotFree[s] = true;
            freeList.push_back(s);
        }
    }

    void publish(uint32_t slot, int64_t atNs, uint64_t seq, uint8_t kind, uint8_t index) {
        OutFrame f{};
        f.slot = slot;
        f.presentAtNs = atNs;
        f.seq = seq;
        f.kind = kind;
        f.index = index;
        f.epoch = epoch;
        if (!ring->queue.push(f)) {
            giveBackSlot(slot);
            stats.add(kStatDroppedLate, 1);
            return;
        }
        // outFrames and generated are counted when the SlotReturn arrives
        // (noteReturn, REV 17 / E21).
    }

    // ---------------------------------------------------------------------
    // LSFG (engine thread only)
    // ---------------------------------------------------------------------

    // initialize + createContextFromAHB with the current cfg. Returns OK,
    // LSFG_INIT, CONTEXT or DEVICE_LOST.
    int32_t lsfgOpen() {
        if (!onEngineThread()) return FgResult::CONTEXT;
        const bool perf = cfg.performanceMode;
        const float flow = lsfgFlowScale(cfg.flowScale);
        const uint64_t generationCount = static_cast<uint64_t>(cfg.multiplier - 1);
        try {
            if (perf) {
                LSFG_3_1P::initialize(dev->lsfgUuid, /*isHdr=*/false, flow, generationCount, loader);
            } else {
                LSFG_3_1::initialize(dev->lsfgUuid, /*isHdr=*/false, flow, generationCount, loader);
            }
        } catch (const std::exception& e) {
            EFG_ENG_LOGE("LSFG initialize threw: %s", e.what());
            return lsfgErrorCode(e, FgResult::LSFG_INIT);
        } catch (...) {
            EFG_ENG_LOGE("LSFG initialize threw an unknown exception");
            return FgResult::LSFG_INIT;
        }
        lsfgInitialized = true;
        lsfgPerf = perf;

        std::vector<AHardwareBuffer*> outs;
        outs.reserve(outN.size());
        for (const AhbImage& o : outN) outs.push_back(o.ahb);
        const VkExtent2D extent = in[0].extent;
        try {
            if (perf) {
                ctxId = LSFG_3_1P::createContextFromAHB(in[0].ahb, in[1].ahb, outs, extent, kFormat);
            } else {
                ctxId = LSFG_3_1::createContextFromAHB(in[0].ahb, in[1].ahb, outs, extent, kFormat);
            }
        } catch (const std::exception& e) {
            EFG_ENG_LOGE("LSFG createContextFromAHB threw: %s", e.what());
            return lsfgErrorCode(e, FgResult::CONTEXT);
        } catch (...) {
            EFG_ENG_LOGE("LSFG createContextFromAHB threw an unknown exception");
            return FgResult::CONTEXT;
        }
        haveCtx = true;
        presentsDone = 0;  // new context: LSFG frameIdx starts at 0
        bootstrapPending = true;
        EFG_ENG_LOGI("LSFG %s ready: %ux%u N=%d flow=%.2f (internal %.2f)", perf ? "3.1P" : "3.1", extent.width,
                     extent.height, cfg.multiplier, static_cast<double>(cfg.flowScale), static_cast<double>(flow));
        return FgResult::OK;
    }

    // presentContext(id, inSem = -1, {}) + waitIdle. Returns OK, CONTEXT or DEVICE_LOST.
    int32_t lsfgPresent() {
        if (!onEngineThread() || !haveCtx) return FgResult::CONTEXT;
        static const std::vector<int> kNoSemaphores;
        try {
            if (lsfgPerf) {
                LSFG_3_1P::presentContext(ctxId, -1, kNoSemaphores);
                LSFG_3_1P::waitIdle();
            } else {
                LSFG_3_1::presentContext(ctxId, -1, kNoSemaphores);
                LSFG_3_1::waitIdle();
            }
        } catch (const std::exception& e) {
            EFG_ENG_LOGE("LSFG presentContext threw: %s", e.what());
            return lsfgErrorCode(e, FgResult::CONTEXT);
        } catch (...) {
            EFG_ENG_LOGE("LSFG presentContext threw an unknown exception");
            return FgResult::CONTEXT;
        }
        ++presentsDone;  // keeps newSlot = presentsDone % 2 in sync with frameIdx
        return FgResult::OK;
    }

    // deleteContext + finalize (render_loop :2564-2577). Never throws.
    void lsfgClose() {
        if (!onEngineThread()) return;
        if (haveCtx) {
            try {
                if (lsfgPerf) {
                    LSFG_3_1P::deleteContext(ctxId);
                } else {
                    LSFG_3_1::deleteContext(ctxId);
                }
            } catch (...) {
                EFG_ENG_LOGW("LSFG deleteContext threw");
            }
            haveCtx = false;
        }
        if (lsfgInitialized) {
            try {
                if (lsfgPerf) {
                    LSFG_3_1P::finalize();
                } else {
                    LSFG_3_1::finalize();
                }
            } catch (...) {
                EFG_ENG_LOGW("LSFG finalize threw");
            }
            lsfgInitialized = false;
        }
    }

    // ---------------------------------------------------------------------
    // Engine thread
    // ---------------------------------------------------------------------

    // I4: newer epoch, both input slots re-seeded by the next frame.
    void beginEpoch() {
        epoch = ring->epoch.fetch_add(1, std::memory_order_acq_rel) + 1;
        bootstrapPending = true;
        luma.prevValid = false;
        guardPeriodNs = lastPeriodNs;
        guardArmed = guardPeriodNs > 0;
        estimator.reset();
        prevTsNs = -1;
    }

    int64_t currentPeriod(int64_t ts) {
        int64_t p = estimator.periodNs();
        if (p <= 0) p = periodHintNs.load(std::memory_order_relaxed);
        if (p <= 0 && prevTsNs > 0 && ts > prevTsNs) p = ts - prevTsNs;
        if (p <= 0) p = kDefaultPeriodNs;
        p = std::clamp(p, kMinPeriodNs, kMaxPeriodNs);
        lastPeriodNs = p;
        return p;
    }

    // updateConfig (queued): deleteContext + finalize, new outN, initialize +
    // createContextFromAHB on this thread.
    int32_t applyPendingConfig() {
        std::optional<FgConfigN> next;
        {
            std::lock_guard<std::mutex> lock(cfgMutex);
            next.swap(pendingCfg);
        }
        if (!next) return FgResult::OK;
        lsfgClose();
        cfg = std::move(*next);
        if (!probeShaders(cfg.shaderDir)) return FgResult::SHADERS_MISSING;
        loader = makeLoader(cfg.shaderDir, cfg.allowFp16);
        int32_t rc = createOutputs();
        if (rc != FgResult::OK) return rc;
        if (!cfg.antiArtifacts) luma.prevValid = false;
        rc = lsfgOpen();
        if (rc == FgResult::OK) {
            // E18: the compute-time EMA belongs to the previous config (other
            // multiplier / flow scale); restart it so latency adapts at once.
            computeAvgNs = 0;
            EFG_ENG_LOGI("config applied");
        }
        return rc;
    }

    // Pause edges (I5). Returns OK or an error code.
    int32_t handlePauseEdge() {
        const bool want = pausedReq.load(std::memory_order_relaxed);
        if (want == pausedNow) return FgResult::OK;
        pausedNow = want;
        stats.set(kStatState, static_cast<int64_t>(want ? FgStatus::Paused : FgStatus::Running));
        if (!want) {
            // Resume: the time base restarts (the pause gap is no discontinuity).
            estimator.reset();
            prevTsNs = -1;
            return FgResult::OK;
        }
        // Pause: drop the queued (generated) frames and show the last real one once.
        epoch = ring->epoch.fetch_add(1, std::memory_order_acq_rel) + 1;
        drainReturns();
        if (!haveLastReal || freeList.empty()) return FgResult::OK;
        CopyJob job{};
        job.src = &in[static_cast<size_t>(lastRealSlot)];
        job.srcFromReader = false;
        job.slot = takeSlot();
        const int32_t rc = copyToSlots(&job, 1);
        if (rc != FgResult::OK) return rc;
        publish(job.slot, nowNs(), 0, kOutKindReal, static_cast<uint8_t>(cfg.multiplier - 1));
        return FgResult::OK;
    }

    // While paused: no LSFG; the frame goes straight to a slot, shown once now.
    int32_t presentPausedFrame(InFrameHandle& h, const AhbImage& srcImg) {
        const uint64_t seq = h.get().seq;
        drainReturns();
        if (freeList.empty()) {
            stats.add(kStatDroppedLate, 1);
            return FgResult::OK;
        }
        CopyJob job{};
        job.src = &srcImg;
        job.srcFromReader = true;
        job.slot = takeSlot();
        const int32_t rc = copyToSlots(&job, 1);
        h.reset();  // I2
        if (rc != FgResult::OK) return rc;
        publish(job.slot, nowNs(), seq, kOutKindReal, static_cast<uint8_t>(cfg.multiplier - 1));
        bootstrapPending = true;  // the input slots do not hold this frame
        haveLastReal = false;
        luma.prevValid = false;
        return FgResult::OK;
    }

    int32_t processFrame(InFrameHandle& h) {
        stats.add(kStatInFrames, 1);
        const InFrame& f = h.get();
        if (!waitFenceFd(f.fenceFd)) {
            EFG_ENG_LOGW("input fence not signalled in %d ms; frame dropped", kFencePollTimeoutMs);
            return FgResult::OK;  // h releases AImage + fence
        }
        const int64_t t0 = nowNs();
        const int64_t ts = f.tsNs;
        const uint64_t seq = f.seq;

        int32_t rc = FgResult::OK;
        const AhbImage* srcImg = importCached(f.buffer, rc);
        if (srcImg == nullptr) {
            EFG_ENG_LOGE("importAhbImage failed: %d", static_cast<int>(rc));
            return rc != FgResult::OK ? rc : FgResult::CONTEXT;
        }
        rc = checkReaderFormat(*srcImg);
        if (rc != FgResult::OK) return rc;
        if (pausedNow) return presentPausedFrame(h, *srcImg);

        // I4 on a timestamp discontinuity (PeriodEstimator: backwards or >= 1 s).
        if (!bootstrapPending && estimator.isDiscontinuity(ts)) beginEpoch();
        // After a flush, the frame following the seed frame also re-seeds when it
        // comes after a gap (the seed may still be a pre-seek frame).
        if (!bootstrapPending && guardArmed) {
            if (prevTsNs > 0 && ts - prevTsNs > 2 * guardPeriodNs) bootstrapPending = true;
            guardArmed = false;
        }
        // REV 15: forward jump > 2 period (and < 1 s, else it was a discontinuity
        // above): lost frame(s). Present the real frame, generate nothing for this
        // interval, keep the epoch and the queue; the estimator ignores the
        // interval as an outlier (no reset).
        const int64_t estPeriodNs = estimator.periodNs();
        const bool lostFrames = !bootstrapPending && prevTsNs > 0 && estPeriodNs > 0 && ts > prevTsNs &&
                                ts - prevTsNs > 2 * estPeriodNs;
        estimator.update(ts);

        const bool bootstrap = bootstrapPending;
        const int newSlot = (presentsDone % 2 == 0) ? 0 : 1;
        const bool withLuma = cfg.antiArtifacts && luma.ok;
        rc = copyInput(*srcImg, bootstrap, newSlot, withLuma);
        h.reset();  // I2: AImage and fence go back as soon as the copy is done
        if (rc != FgResult::OK) return rc;
        const bool cut = withLuma ? readLuma(!bootstrap) : false;
        if (!withLuma) luma.prevValid = false;

        rc = lsfgPresent();
        if (rc != FgResult::OK) return rc;
        bootstrapPending = false;
        haveLastReal = true;
        lastRealSlot = newSlot;

        // Outputs: N-1 generated + the real one, or only the real one on a
        // bootstrap (both inputs equal), a scene cut or lost frames (REV 15).
        drainReturns();
        size_t gens = (!bootstrap && !cut && !lostFrames) ? outN.size() : 0;
        if (freeList.size() < gens + 1) {
            const size_t missing = freeList.empty() ? gens + 1 : gens;
            stats.add(kStatDroppedLate, static_cast<int64_t>(missing));
            gens = 0;
        }
        std::array<CopyJob, kMaxCopyJobs> jobs{};
        size_t nj = 0;
        if (!freeList.empty()) {
            for (size_t i = 0; i < gens; ++i) {
                jobs[nj].src = &outN[i];
                jobs[nj].srcFromReader = false;
                jobs[nj].slot = takeSlot();
                ++nj;
            }
            jobs[nj].src = &in[static_cast<size_t>(newSlot)];
            jobs[nj].srcFromReader = false;
            jobs[nj].slot = takeSlot();
            ++nj;
            rc = copyToSlots(jobs.data(), nj);
            if (rc != FgResult::OK) return rc;
        }

        // Pacing: L = period + compute + margin; real frame at ts + L, the
        // generated ones evenly spread over the period before it.
        const int64_t computeNs = nowNs() - t0;
        computeAvgNs = computeAvgNs == 0 ? computeNs : computeAvgNs + (computeNs - computeAvgNs) / 8;
        const int64_t period = currentPeriod(ts);
        const int64_t latency = efg::latencyNs(period, computeAvgNs, kBaseMarginNs + errEmaNs);
        const int64_t now = nowNs();
        int64_t base = ts;
        if (base <= 0 || base > now + kTsSanityNs || base < now - kTsSanityNs) base = now;
        if (nj > 0) {
            std::array<int64_t, kMaxCopyJobs> times{};
            schedule(base - period, latency, period, static_cast<int>(nj), times.data());
            for (size_t k = 0; k < nj; ++k) {
                const bool real = (k + 1 == nj);
                const int64_t at = times[k] > 0 ? times[k] : now;
                publish(jobs[k].slot, at, seq, real ? kOutKindReal : kOutKindGenerated,
                        static_cast<uint8_t>(real ? cfg.multiplier - 1 : static_cast<int>(k)));
            }
        }
        prevTsNs = ts;

        stats.set(kStatLatencyNs, latency);
        stats.set(kStatComputeNsAvg, computeAvgNs);
        stats.set(kStatPeriodNs, period);
        ++framesDone;
        if (cfg.verbose && framesDone % kVerboseEveryFrames == 0) {
            EFG_ENG_LOGI("in=%lld out=%lld gen=%lld dropped=%lld L=%lld compute=%lld period=%lld epoch=%u",
                         static_cast<long long>(stats.get(kStatInFrames)),
                         static_cast<long long>(stats.get(kStatOutFrames)),
                         static_cast<long long>(stats.get(kStatGenerated)),
                         static_cast<long long>(stats.get(kStatDroppedLate)), static_cast<long long>(latency),
                         static_cast<long long>(computeAvgNs), static_cast<long long>(period), epoch);
        }
        return FgResult::OK;
    }

    // flush() (REV 16): drain the reader backlog releasing EVERY image (the
    // newest may still be a pre-seek frame), then I4. The bootstrap seeds with
    // the first frame acquired after this flush.
    void doExternalFlush() {
        for (;;) {
            InFrame f{};
            if (src->acquire(f, 0) != FgResult::OK) break;
            InFrameHandle discard(f);  // releases AImage + fence at scope end (I2)
        }
        beginEpoch();
    }

    void runLoop() {
        while (!stopReq.load(std::memory_order_acquire)) {
            int32_t rc = applyPendingConfig();
            if (rc != FgResult::OK) {
                fail(rc);
                return;
            }
            processRemoved();
            drainReturns();
            rc = handlePauseEdge();
            if (rc != FgResult::OK) {
                fail(rc);
                return;
            }

            InFrameHandle frame;
            const uint32_t fs = flushSeq.load(std::memory_order_acquire);
            if (fs != seenFlushSeq) {
                seenFlushSeq = fs;
                doExternalFlush();
            }
            if (!frame) {
                InFrame f{};
                const int32_t ar = src->acquire(f, kAcquireTimeoutMs);
                if (ar == FgResult::BUSY) continue;
                if (ar != FgResult::OK) {
                    if (++readerErrors >= kMaxReaderErrors) {
                        fail(FgResult::READER);
                        return;
                    }
                    continue;
                }
                frame = InFrameHandle(f);
            }
            readerErrors = 0;
            rc = processFrame(frame);
            if (rc != FgResult::OK) {
                fail(rc);
                return;
            }
        }
    }

    // Body of the engine thread: every LSFG_* call happens below this frame.
    void threadMain(std::promise<int32_t> initResult) {
        engineThreadId = std::this_thread::get_id();
        bool reported = false;
        try {
            const int32_t rc = lsfgOpen();
            if (rc == FgResult::OK) stats.set(kStatState, static_cast<int64_t>(FgStatus::Running));
            reported = true;
            initResult.set_value(rc);
            if (rc == FgResult::OK) runLoop();
        } catch (const std::exception& e) {
            EFG_ENG_LOGE("engine thread exception: %s", e.what());
            const int32_t code = lsfgErrorCode(e, FgResult::CONTEXT);
            if (!reported) {
                reported = true;
                initResult.set_value(code);
            } else {
                fail(code);
            }
        } catch (...) {
            EFG_ENG_LOGE("engine thread unknown exception");
            if (!reported) {
                reported = true;
                initResult.set_value(FgResult::CONTEXT);
            } else {
                fail(FgResult::CONTEXT);
            }
        }
        lsfgClose();  // I3: deleteContext + finalize on this thread, before the join
    }

    // ---------------------------------------------------------------------
    // start / stop helpers (caller thread, engine thread not running)
    // ---------------------------------------------------------------------

    int32_t createResources() {
        int32_t rc = createCommandObjects();
        if (rc != FgResult::OK) return rc;
        const uint32_t w = static_cast<uint32_t>(cfg.inputWidth);
        const uint32_t h = static_cast<uint32_t>(cfg.inputHeight);
        std::vector<VkImage> shared;
        for (size_t i = 0; i < in.size(); ++i) {
            if (createAhbImage(*dev, w, h, kFormat, in[i]) != FgResult::OK) {
                EFG_ENG_LOGE("createAhbImage(in%zu) failed", i);
                return FgResult::CONTEXT;
            }
            shared.push_back(in[i].image);
        }
        // Presenter slots: owned by the ring (destroyed by its owner, I3).
        // REV 13: 2N + 1 slots (capped at kMaxPresentSlots) for the start N.
        const uint32_t slotsWanted = requiredSlots(cfg.multiplier);
        std::vector<VkImage> own;
        ring->slotCount = 0;
        for (uint32_t i = 0; i < slotsWanted; ++i) {
            if (createAhbImage(*dev, w, h, kFormat, ring->slots[i]) != FgResult::OK) {
                EFG_ENG_LOGE("createAhbImage(slot %u) failed", i);
                return FgResult::CONTEXT;
            }
            own.push_back(ring->slots[i].image);
        }
        rc = initLayouts(shared, own);
        if (rc != FgResult::OK) return rc;
        ring->slotCount = slotsWanted;  // published before any OutFrame
        rc = createOutputs();
        if (rc != FgResult::OK) return rc;
        createLuma();
        if (!luma.ok) {
            EFG_ENG_LOGW("8x8 luma probe unavailable: anti-artifacts disabled");
            destroyLuma();
        }
        freeList.clear();
        slotFree.fill(false);
        for (uint32_t i = 0; i < ring->slotCount; ++i) {
            slotFree[i] = true;
            freeList.push_back(i);
        }
        SlotReturn stale{};
        while (freeSlots->pop(stale)) {
        }
        epoch = ring->epoch.load(std::memory_order_acquire);
        return FgResult::OK;
    }

    // Engine-owned GPU objects; the ring slots are NOT touched (I3).
    void destroyResources() {
        if (dev == nullptr || dev->device == VK_NULL_HANDLE) return;
        if (fenceInFlight && fence != VK_NULL_HANDLE) {
            dev->fn.vkWaitForFences(dev->device, 1, &fence, VK_TRUE, kStopWaitTimeoutNs);
            fenceInFlight = false;
        }
        cache.clear();
        outN.clear();
        for (AhbImage& img : in) img.reset();
        destroyLuma();
        if (fence != VK_NULL_HANDLE) dev->fn.vkDestroyFence(dev->device, fence, nullptr);
        fence = VK_NULL_HANDLE;
        if (pool != VK_NULL_HANDLE) dev->fn.vkDestroyCommandPool(dev->device, pool, nullptr);  // frees cb
        pool = VK_NULL_HANDLE;
        cb = VK_NULL_HANDLE;
        freeList.clear();
        {
            std::lock_guard<std::mutex> lock(removedMutex);
            removed.clear();
        }
    }

    void resetRunState() {
        running.store(false);
        failed.store(false);
        stopReq.store(false);
        pausedReq.store(false);
        flushSeq.store(0);
        {
            std::lock_guard<std::mutex> lock(cfgMutex);
            pendingCfg.reset();
        }
        seenFlushSeq = 0;
        pausedNow = false;
        bootstrapPending = true;
        haveLastReal = false;
        lastRealSlot = 0;
        guardArmed = false;
        guardPeriodNs = 0;
        estimator.reset();
        prevTsNs = -1;
        lastPeriodNs = 0;
        computeAvgNs = 0;
        errEmaNs = 0;
        readerErrors = 0;
        framesDone = 0;
        blitConvertLogged = false;
        presentsDone = 0;
        haveCtx = false;
        lsfgInitialized = false;
        fenceInFlight = false;
    }
};

FgEngine::FgEngine() : impl_(std::make_unique<Impl>()) {}

FgEngine::~FgEngine() {
    stop();
}

int32_t FgEngine::start(FgDevice& dev, FrameSource& src, PresentRing& ring, FreeSlotQueue& freeSlots,
                        const FgConfigN& cfg, ShaderLoader loader) {
    Impl& m = *impl_;
    if (m.thread.joinable()) return FgResult::BUSY;  // running, or failed and not stopped yet
    if (!validConfig(cfg) || dev.device == VK_NULL_HANDLE) return FgResult::INVALID_ARG;
    if (!probeShaders(cfg.shaderDir)) return FgResult::SHADERS_MISSING;

    m.resetRunState();
    m.stats.reset();
    m.stats.set(kStatState, static_cast<int64_t>(FgStatus::Stopped));
    m.stats.set(kStatInW, cfg.inputWidth);
    m.stats.set(kStatInH, cfg.inputHeight);
    m.dev = &dev;
    m.src = &src;
    m.ring = &ring;
    m.freeSlots = &freeSlots;
    m.cfg = cfg;
    m.loader = loader ? std::move(loader) : makeLoader(cfg.shaderDir, cfg.allowFp16);
    m.startInW = cfg.inputWidth;
    m.startInH = cfg.inputHeight;
    m.startOutW = cfg.outputWidth;
    m.startOutH = cfg.outputHeight;

    // start() returns only the codes of fg_engine.h; DEVICE_LOST stays in
    // stats lastError and start() reports CONTEXT (fg_jni: DEVICE_LOST only in stats).
    auto failStart = [&m](int32_t code) -> int32_t {
        m.stats.set(kStatLastError, code);
        m.stats.set(kStatState, static_cast<int64_t>(FgStatus::Failed));
        m.failed.store(true);
        return code == FgResult::DEVICE_LOST ? FgResult::CONTEXT : code;
    };

    int32_t rc = m.createResources();
    if (rc != FgResult::OK) {
        m.destroyResources();
        return failStart(rc);
    }

    // fg_jni does not install a FrameSource listener; the engine does (cache
    // invalidation only). Detached again in stop().
    FrameSourceListener listener;
    Impl* self = &m;
    listener.onBufferRemoved = [self](AHardwareBuffer* buffer) {
        std::lock_guard<std::mutex> lock(self->removedMutex);
        self->removed.push_back(buffer);
    };
    src.setListener(std::move(listener));
    m.listenerInstalled = true;

    std::promise<int32_t> initPromise;
    std::future<int32_t> initResult = initPromise.get_future();
    try {
        m.thread = std::thread(&Impl::threadMain, &m, std::move(initPromise));
    } catch (const std::system_error& e) {
        EFG_ENG_LOGE("engine thread creation failed: %s", e.what());
        src.setListener(FrameSourceListener{});
        m.listenerInstalled = false;
        m.destroyResources();
        return failStart(FgResult::CONTEXT);
    }
    try {
        rc = initResult.get();
    } catch (...) {
        rc = FgResult::CONTEXT;  // broken promise
    }
    if (rc != FgResult::OK) {
        stop();  // joins (the thread already ran deleteContext + finalize) and frees
        return failStart(rc);
    }
    m.running.store(true);
    return FgResult::OK;
}

int32_t FgEngine::updateConfig(const FgConfigN& cfg) {
    Impl& m = *impl_;
    if (!m.thread.joinable() || !m.running.load() || m.failed.load()) return FgResult::NOT_INITIALIZED;
    if (!validConfig(cfg)) return FgResult::INVALID_ARG;
    if (cfg.inputWidth != m.startInW || cfg.inputHeight != m.startInH || cfg.outputWidth != m.startOutW ||
        cfg.outputHeight != m.startOutH) {
        return FgResult::INVALID_ARG;  // a size change needs stop/start
    }
    // REV 13: the ring was sized for the start N (2N + 1 slots, capped). An N'
    // that needs more slots is rejected without queueing anything; Kotlin
    // (FgController.applyHot) answers INVALID_ARG with a stop/start cycle.
    if (m.ring == nullptr || requiredSlots(cfg.multiplier) > m.ring->slotCount) return FgResult::INVALID_ARG;
    std::lock_guard<std::mutex> lock(m.cfgMutex);
    m.pendingCfg = cfg;  // latest wins; applied by the engine thread
    return FgResult::OK;
}

void FgEngine::setSourcePeriodNs(int64_t ns) {
    impl_->periodHintNs.store(ns > 0 ? ns : 0, std::memory_order_relaxed);
}

void FgEngine::flush() {
    impl_->flushSeq.fetch_add(1, std::memory_order_acq_rel);
}

void FgEngine::setPaused(bool paused) {
    impl_->pausedReq.store(paused, std::memory_order_relaxed);
}

void FgEngine::stop() {
    if (!impl_) return;
    Impl& m = *impl_;
    // 1. Our listener off (fg_jni already nulled it; idempotent).
    if (m.listenerInstalled && m.src != nullptr) {
        m.src->setListener(FrameSourceListener{});
        m.listenerInstalled = false;
    }
    // 2. Join: the thread runs deleteContext + finalize before it exits (I3).
    m.stopReq.store(true, std::memory_order_release);
    if (m.thread.joinable()) m.thread.join();
    m.running.store(false);
    // 3. Engine-owned images (in0/in1/outN, import cache, luma probe), command
    //    objects. The PresentRing slots stay: the owner destroys them after the
    //    presenter has stopped (I3).
    m.destroyResources();
    if (!m.failed.load()) m.stats.set(kStatState, static_cast<int64_t>(FgStatus::Stopped));
}

void FgEngine::stats(FgStatsArray& out) const {
    impl_->stats.copyTo(out);
}

}  // namespace efg
