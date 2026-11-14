// SPDX-License-Identifier: MIT
//
// temple-lsfg: LSFG frame generation helper used by Tatakai Link (Windows and Linux).
//
// Reads raw RGBA8 frames from stdin, runs the lsfg-vk framegen core on the PC GPU through
// Vulkan and writes the interpolated frames to stdout:
//
//   temple-lsfg --probe --dll <Lossless.dll>
//   temple-lsfg --dll <Lossless.dll> --width W --height H --multiplier N
//               [--flow-scale F] [--performance] < frames.rgba > out.rgba
//
// Output: frame 0 as is; then, for every frame k >= 1, the N-1 generated frames between k-1 and k
// followed by frame k. For n input frames that is (n - 1) * N + 1 output frames.
//
// Exit codes: 0 ok; 1 usage; 2 start-up error (dll_unreadable, missing_spirv, no_vulkan,
// device_unsupported); 3 GPU error while running (device_lost, lsfg_failed); 4 bad input
// (truncated_frame); 5 stdout closed. stderr carries one line per event and never a user path.
//
// The shaders come from the user's own Lossless.dll (read only, parsed in memory, never copied).

#include <volk.h>

#include "common/exception.hpp"
#include "lsfg_3_1.hpp"
#include "lsfg_3_1p.hpp"
#include "temple_local.hpp"
#include "shader_loader.h"

#if defined(_WIN32)
#include <fcntl.h>
#include <io.h>
#else
#include <signal.h>
#endif

#include <algorithm>
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <deque>
#include <functional>
#include <mutex>
#include <thread>
#include <cerrno>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cwchar>
#include <cstring>
#include <exception>
#include <filesystem>
#include <memory>
#include <optional>
#include <string>
#include <vector>

namespace {

constexpr int kExitUsage = 1;
constexpr int kExitStart = 2;
constexpr int kExitGpu = 3;
constexpr int kExitInput = 4;
constexpr int kExitOutput = 5;

constexpr VkFormat kFormat = VK_FORMAT_R8G8B8A8_UNORM;
constexpr size_t kIoBuffer = 4u << 20;

// ---------------------------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------------------------

// A failure with the code the caller sees (probe JSON and the "error:" line on stderr).
struct HelperError {
    const char* code;
    int exitCode;
    std::string detail;
};

void logLine(const std::string& s) {
    std::fprintf(stderr, "temple-lsfg: %s\n", s.c_str());
    std::fflush(stderr);
}

std::string jsonEscape(const std::string& s) {
    std::string out;
    for (const unsigned char c : s) {
        switch (c) {
            case '"': out += "\\\""; break;
            case '\\': out += "\\\\"; break;
            case '\n': out += "\\n"; break;
            case '\r': out += "\\r"; break;
            case '\t': out += "\\t"; break;
            default:
                if (c < 0x20) {
                    char buf[8];
                    std::snprintf(buf, sizeof(buf), "\\u%04x", c);
                    out += buf;
                } else {
                    out += static_cast<char>(c);
                }
        }
    }
    return out;
}

// ---------------------------------------------------------------------------------------------
// GPU selection (before LSFG creates its own device)
// ---------------------------------------------------------------------------------------------

struct GpuChoice {
    uint64_t lsfgUuid = 0;  // vendorID << 32 | deviceID, as LSFG initialize expects
    std::string name;
    bool fp16 = false;      // shaderFloat16
    bool storageOk = false; // storage image features LSFG needs
};

// Instance version LSFG itself will use: min(loader, 1.3), floor 1.1.
uint32_t lsfgInstanceVersion() {
    const uint32_t loader = volkGetInstanceVersion();
    if (loader < VK_API_VERSION_1_1) return VK_API_VERSION_1_1;
    return loader < VK_API_VERSION_1_3 ? loader : VK_API_VERSION_1_3;
}

// Picks the GPU: discrete before integrated before virtual, then the larger device-local heap.
// CPU rasterizers are never used. Throws HelperError(no_vulkan | device_unsupported).
GpuChoice pickGpu() {
    if (volkInitialize() != VK_SUCCESS) throw HelperError{"no_vulkan", kExitStart, "Vulkan loader not found"};

    const uint32_t instVersion = lsfgInstanceVersion();
    const VkApplicationInfo app{
        .sType = VK_STRUCTURE_TYPE_APPLICATION_INFO,
        .pApplicationName = "temple-lsfg",
        .apiVersion = instVersion,
    };
    const VkInstanceCreateInfo ici{
        .sType = VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO,
        .pApplicationInfo = &app,
    };
    VkInstance inst = VK_NULL_HANDLE;
    if (vkCreateInstance(&ici, nullptr, &inst) != VK_SUCCESS || inst == VK_NULL_HANDLE)
        throw HelperError{"no_vulkan", kExitStart, "vkCreateInstance failed"};
    volkLoadInstance(inst);

    struct Guard {
        VkInstance i;
        ~Guard() { if (vkDestroyInstance) vkDestroyInstance(i, nullptr); }
    } guard{inst};

    uint32_t count = 0;
    vkEnumeratePhysicalDevices(inst, &count, nullptr);
    std::vector<VkPhysicalDevice> devices(count);
    if (count > 0) vkEnumeratePhysicalDevices(inst, &count, devices.data());
    if (count == 0) throw HelperError{"no_vulkan", kExitStart, "no Vulkan physical device"};

    auto rank = [](VkPhysicalDeviceType t) {
        switch (t) {
            case VK_PHYSICAL_DEVICE_TYPE_DISCRETE_GPU: return 3;
            case VK_PHYSICAL_DEVICE_TYPE_INTEGRATED_GPU: return 2;
            case VK_PHYSICAL_DEVICE_TYPE_VIRTUAL_GPU: return 1;
            default: return 0;
        }
    };

    std::optional<GpuChoice> best;
    int bestRank = 0;
    uint64_t bestHeap = 0;
    for (const VkPhysicalDevice dev : devices) {
        VkPhysicalDeviceProperties props{};
        vkGetPhysicalDeviceProperties(dev, &props);
        const int r = rank(props.deviceType);
        if (r == 0) continue;
        VkPhysicalDeviceMemoryProperties mem{};
        vkGetPhysicalDeviceMemoryProperties(dev, &mem);
        uint64_t heap = 0;
        for (uint32_t i = 0; i < mem.memoryHeapCount; ++i)
            if (mem.memoryHeaps[i].flags & VK_MEMORY_HEAP_DEVICE_LOCAL_BIT) heap = std::max<uint64_t>(heap, mem.memoryHeaps[i].size);
        if (best && (r < bestRank || (r == bestRank && heap <= bestHeap))) continue;

        GpuChoice c;
        c.lsfgUuid = (static_cast<uint64_t>(props.vendorID) << 32) | props.deviceID;
        c.name = props.deviceName;

        const uint32_t usable = std::min(instVersion, props.apiVersion);
        VkPhysicalDeviceVulkan12Features f12{.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_2_FEATURES};
        VkPhysicalDeviceFeatures2 f2{.sType = VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2};
        if (usable >= VK_API_VERSION_1_2) f2.pNext = &f12;
        if (usable >= VK_API_VERSION_1_1) vkGetPhysicalDeviceFeatures2(dev, &f2);
        c.fp16 = usable >= VK_API_VERSION_1_2 && f12.shaderFloat16 == VK_TRUE;
        c.storageOk = usable >= VK_API_VERSION_1_1 && f2.features.shaderStorageImageExtendedFormats &&
                      f2.features.shaderStorageImageReadWithoutFormat && f2.features.shaderStorageImageWriteWithoutFormat;
        best = c;
        bestRank = r;
        bestHeap = heap;
    }
    if (!best) throw HelperError{"no_vulkan", kExitStart, "no hardware Vulkan device"};
    if (!best->storageOk) throw HelperError{"device_unsupported", kExitStart, "storage image features missing"};
    return *best;
}

// ---------------------------------------------------------------------------------------------
// LSFG backend (3.1 or 3.1P)
// ---------------------------------------------------------------------------------------------

struct Backend {
    void (*initialize)(uint64_t, bool, float, uint64_t, const std::function<std::vector<uint8_t>(const std::string&)>&);
    TempleLsfg::LocalContext (*createContext)(VkExtent2D, VkFormat);
    void (*present)(int32_t, int, const std::vector<int>&);
    void (*waitIdle)();
    void (*deleteContext)(int32_t);
    void (*finalize)();
};

const Backend kBackend31{LSFG_3_1::initialize, LSFG_3_1::createContextLocal, LSFG_3_1::presentContext,
                         LSFG_3_1::waitIdle, LSFG_3_1::deleteContext, LSFG_3_1::finalize};
const Backend kBackend31p{LSFG_3_1P::initialize, LSFG_3_1P::createContextLocal, LSFG_3_1P::presentContext,
                          LSFG_3_1P::waitIdle, LSFG_3_1P::deleteContext, LSFG_3_1P::finalize};

// The user flow scale (0.25..1.0, larger = finer) is the reciprocal of what LSFG takes.
float lsfgFlowScale(float user) { return 1.0f / std::clamp(user, 0.25f, 1.0f); }

// Maps an exception thrown while LSFG starts up (initialize / createContext).
HelperError classifyStart(const std::exception& e) {
    const std::string what = e.what();
    if (what.find("Shader code is empty") != std::string::npos) return {"missing_spirv", kExitStart, what};
    return {"device_unsupported", kExitStart, what};
}

// Maps an exception thrown while frames are running.
HelperError classifyRun(const std::exception& e) {
    if (const auto* ve = dynamic_cast<const LSFG::vulkan_error*>(&e); ve && ve->error() == VK_ERROR_DEVICE_LOST)
        return {"device_lost", kExitGpu, e.what()};
    return {"lsfg_failed", kExitGpu, e.what()};
}

// ---------------------------------------------------------------------------------------------
// Staging: host buffers <-> the images of the LSFG context, on LSFG's own device and queue
// ---------------------------------------------------------------------------------------------

uint32_t findMemoryType(const VkPhysicalDeviceMemoryProperties& mp, uint32_t bits, VkMemoryPropertyFlags want) {
    for (uint32_t i = 0; i < mp.memoryTypeCount; ++i)
        if ((bits & (1u << i)) && (mp.memoryTypes[i].propertyFlags & want) == want) return i;
    return UINT32_MAX;
}

struct HostBuffer {
    VkBuffer buffer = VK_NULL_HANDLE;
    VkDeviceMemory memory = VK_NULL_HANDLE;
    uint8_t* ptr = nullptr;
};

void check(VkResult r, const char* what) {
    if (r != VK_SUCCESS) throw LSFG::vulkan_error(r, what);
}

HostBuffer createHostBuffer(const TempleLsfg::LocalContext& lc, VkDeviceSize size, VkBufferUsageFlags usage,
                            bool preferCached) {
    HostBuffer hb;
    const VkBufferCreateInfo bci{
        .sType = VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO,
        .size = size,
        .usage = usage,
        .sharingMode = VK_SHARING_MODE_EXCLUSIVE,
    };
    check(vkCreateBuffer(lc.device, &bci, nullptr, &hb.buffer), "vkCreateBuffer");
    VkMemoryRequirements req{};
    vkGetBufferMemoryRequirements(lc.device, hb.buffer, &req);
    VkPhysicalDeviceMemoryProperties mp{};
    vkGetPhysicalDeviceMemoryProperties(lc.physicalDevice, &mp);
    const VkMemoryPropertyFlags base = VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT;
    uint32_t type = UINT32_MAX;
    if (preferCached) type = findMemoryType(mp, req.memoryTypeBits, base | VK_MEMORY_PROPERTY_HOST_CACHED_BIT);
    if (type == UINT32_MAX) type = findMemoryType(mp, req.memoryTypeBits, base);
    if (type == UINT32_MAX) throw LSFG::vulkan_error(VK_ERROR_UNKNOWN, "no host visible memory type");
    const VkMemoryAllocateInfo mai{
        .sType = VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO,
        .allocationSize = req.size,
        .memoryTypeIndex = type,
    };
    check(vkAllocateMemory(lc.device, &mai, nullptr, &hb.memory), "vkAllocateMemory");
    check(vkBindBufferMemory(lc.device, hb.buffer, hb.memory, 0), "vkBindBufferMemory");
    void* p = nullptr;
    check(vkMapMemory(lc.device, hb.memory, 0, VK_WHOLE_SIZE, 0, &p), "vkMapMemory");
    hb.ptr = static_cast<uint8_t*>(p);
    return hb;
}

// Ring of host-visible buffers plus per-frame command buffers and fences. Several frames are
// in flight: the CPU records and submits frame k+1 while the GPU still works on frame k.
class Staging {
public:
    static constexpr uint32_t kInFlight = 3;

    Staging(const TempleLsfg::LocalContext& lc, uint32_t width, uint32_t height, size_t upSlots, size_t downSlots)
        : lc_(lc), width_(width), height_(height), frameBytes_(static_cast<size_t>(width) * height * 4) {
        const VkCommandPoolCreateInfo cpi{
            .sType = VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO,
            .flags = VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT,
            .queueFamilyIndex = lc.queueFamily,
        };
        check(vkCreateCommandPool(lc.device, &cpi, nullptr, &pool_), "vkCreateCommandPool");
        const VkCommandBufferAllocateInfo cai{
            .sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO,
            .commandPool = pool_,
            .level = VK_COMMAND_BUFFER_LEVEL_PRIMARY,
            .commandBufferCount = 2 * kInFlight,
        };
        check(vkAllocateCommandBuffers(lc.device, &cai, cbs_), "vkAllocateCommandBuffers");
        const VkFenceCreateInfo fci{.sType = VK_STRUCTURE_TYPE_FENCE_CREATE_INFO};
        for (uint32_t i = 0; i < kInFlight; ++i) check(vkCreateFence(lc.device, &fci, nullptr, &fences_[i]), "vkCreateFence");
        for (size_t i = 0; i < upSlots; ++i)
            up_.push_back(createHostBuffer(lc, frameBytes_, VK_BUFFER_USAGE_TRANSFER_SRC_BIT, true));
        for (size_t i = 0; i < downSlots; ++i)
            down_.push_back(createHostBuffer(lc, frameBytes_ * lc.out.size(), VK_BUFFER_USAGE_TRANSFER_DST_BIT, true));
    }

    ~Staging() {
        if (lc_.device == VK_NULL_HANDLE) return;
        vkDeviceWaitIdle(lc_.device);
        for (auto* v : {&up_, &down_}) {
            for (HostBuffer& hb : *v) {
                if (hb.ptr) vkUnmapMemory(lc_.device, hb.memory);
                if (hb.buffer) vkDestroyBuffer(lc_.device, hb.buffer, nullptr);
                if (hb.memory) vkFreeMemory(lc_.device, hb.memory, nullptr);
            }
        }
        for (VkFence f : fences_) if (f) vkDestroyFence(lc_.device, f, nullptr);
        if (pool_) vkDestroyCommandPool(lc_.device, pool_, nullptr);
    }
    Staging(const Staging&) = delete;
    Staging& operator=(const Staging&) = delete;

    size_t frameBytes() const { return frameBytes_; }
    uint8_t* up(size_t slot) { return up_[slot].ptr; }
    const uint8_t* generated(size_t downSlot, size_t i) const { return down_[downSlot].ptr + i * frameBytes_; }

    // Submits the copy of up slot `slot` into in0 and/or in1 on in-flight index f. No wait. With
    // signalFence the frame fence is signaled by this submission (frames with no download).
    void submitUpload(uint32_t f, size_t slot, bool toIn0, bool toIn1, bool signalFence) {
        VkCommandBuffer cb = cbs_[2 * f];
        begin(cb);
        // Waits for everything queued before (previous frame generation reading the inputs).
        memBarrier(cb, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, VK_ACCESS_SHADER_READ_BIT | VK_ACCESS_SHADER_WRITE_BIT,
                   VK_PIPELINE_STAGE_TRANSFER_BIT, VK_ACCESS_TRANSFER_WRITE_BIT);
        const VkBufferImageCopy region = copyRegion();
        if (toIn0) vkCmdCopyBufferToImage(cb, up_[slot].buffer, lc_.in0, VK_IMAGE_LAYOUT_GENERAL, 1, &region);
        if (toIn1) vkCmdCopyBufferToImage(cb, up_[slot].buffer, lc_.in1, VK_IMAGE_LAYOUT_GENERAL, 1, &region);
        memBarrier(cb, VK_PIPELINE_STAGE_TRANSFER_BIT, VK_ACCESS_TRANSFER_WRITE_BIT,
                   VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT, VK_ACCESS_SHADER_READ_BIT | VK_ACCESS_SHADER_WRITE_BIT);
        check(vkEndCommandBuffer(cb), "vkEndCommandBuffer");
        submit(cb, signalFence ? fences_[f] : VK_NULL_HANDLE, signalFence);
    }

    // Submits the copy of the generated images into down slot `slot`; signals the frame fence.
    void submitDownload(uint32_t f, size_t slot) {
        VkCommandBuffer cb = cbs_[2 * f + 1];
        begin(cb);
        memBarrier(cb, VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT, VK_ACCESS_SHADER_WRITE_BIT,
                   VK_PIPELINE_STAGE_TRANSFER_BIT, VK_ACCESS_TRANSFER_READ_BIT);
        for (size_t i = 0; i < lc_.out.size(); ++i) {
            VkBufferImageCopy region = copyRegion();
            region.bufferOffset = i * frameBytes_;
            vkCmdCopyImageToBuffer(cb, lc_.out[i], VK_IMAGE_LAYOUT_GENERAL, down_[slot].buffer, 1, &region);
        }
        memBarrier(cb, VK_PIPELINE_STAGE_TRANSFER_BIT, VK_ACCESS_TRANSFER_WRITE_BIT,
                   VK_PIPELINE_STAGE_HOST_BIT, VK_ACCESS_HOST_READ_BIT);
        check(vkEndCommandBuffer(cb), "vkEndCommandBuffer");
        submit(cb, fences_[f], true);
    }

    // Blocks until the frame on in-flight index f has finished on the GPU.
    void wait(uint32_t f) {
        check(vkWaitForFences(lc_.device, 1, &fences_[f], VK_TRUE, UINT64_MAX), "vkWaitForFences");
    }

private:
    VkBufferImageCopy copyRegion() const {
        return VkBufferImageCopy{
            .bufferOffset = 0,
            .bufferRowLength = 0,
            .bufferImageHeight = 0,
            .imageSubresource = {.aspectMask = VK_IMAGE_ASPECT_COLOR_BIT, .mipLevel = 0, .baseArrayLayer = 0, .layerCount = 1},
            .imageOffset = {0, 0, 0},
            .imageExtent = {width_, height_, 1},
        };
    }

    void begin(VkCommandBuffer cb) {
        check(vkResetCommandBuffer(cb, 0), "vkResetCommandBuffer");
        const VkCommandBufferBeginInfo bi{
            .sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO,
            .flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT,
        };
        check(vkBeginCommandBuffer(cb, &bi), "vkBeginCommandBuffer");
    }

    static void memBarrier(VkCommandBuffer cb, VkPipelineStageFlags srcStage, VkAccessFlags srcAccess,
                           VkPipelineStageFlags dstStage, VkAccessFlags dstAccess) {
        const VkMemoryBarrier mb{
            .sType = VK_STRUCTURE_TYPE_MEMORY_BARRIER,
            .srcAccessMask = srcAccess,
            .dstAccessMask = dstAccess,
        };
        vkCmdPipelineBarrier(cb, srcStage, dstStage, 0, 1, &mb, 0, nullptr, 0, nullptr);
    }

    void submit(VkCommandBuffer cb, VkFence fence, bool resetFirst) {
        if (resetFirst) check(vkResetFences(lc_.device, 1, &fence), "vkResetFences");
        const VkSubmitInfo si{
            .sType = VK_STRUCTURE_TYPE_SUBMIT_INFO,
            .commandBufferCount = 1,
            .pCommandBuffers = &cb,
        };
        check(vkQueueSubmit(lc_.queue, 1, &si, fence), "vkQueueSubmit");
    }

    TempleLsfg::LocalContext lc_;
    uint32_t width_;
    uint32_t height_;
    size_t frameBytes_;
    VkCommandPool pool_ = VK_NULL_HANDLE;
    VkCommandBuffer cbs_[2 * kInFlight]{};
    VkFence fences_[kInFlight]{};
    std::vector<HostBuffer> up_;
    std::vector<HostBuffer> down_;
};

// ---------------------------------------------------------------------------------------------
// Threads: reader (stdin) -> GPU (main thread) -> writer (stdout), bounded by slot pools
// ---------------------------------------------------------------------------------------------

// Shared failure state: the first error wins and wakes every waiter.
class Shared {
public:
    void fail(HelperError e) {
        {
            std::lock_guard<std::mutex> lock(m_);
            if (!err_) err_ = std::move(e);
        }
        abort_ = true;
        for (auto& w : wakers_) w();
    }
    // Records an error without stopping the stages (frames already read still go through).
    void note(HelperError e) {
        std::lock_guard<std::mutex> lock(m_);
        if (!err_) err_ = std::move(e);
    }
    bool aborted() const { return abort_.load(); }
    std::optional<HelperError> error() {
        std::lock_guard<std::mutex> lock(m_);
        return err_;
    }
    void addWaker(std::function<void()> w) { wakers_.push_back(std::move(w)); }

private:
    std::mutex m_;
    std::optional<HelperError> err_;
    std::atomic<bool> abort_{false};
    std::vector<std::function<void()>> wakers_;  // registered before any thread starts
};

// FIFO. pop() returns nullopt once closed and drained, or on abort.
template <class T>
class Queue {
public:
    explicit Queue(Shared& s) : s_(s) { s.addWaker([this] { cv_.notify_all(); }); }
    void push(T v) {
        {
            std::lock_guard<std::mutex> lock(m_);
            q_.push_back(std::move(v));
        }
        cv_.notify_one();
    }
    void close() {
        {
            std::lock_guard<std::mutex> lock(m_);
            closed_ = true;
        }
        cv_.notify_all();
    }
    std::optional<T> pop(bool block = true) {
        std::unique_lock<std::mutex> lock(m_);
        if (block) cv_.wait(lock, [&] { return !q_.empty() || closed_ || s_.aborted(); });
        if (s_.aborted() || q_.empty()) return std::nullopt;
        T v = std::move(q_.front());
        q_.pop_front();
        return v;
    }

private:
    Shared& s_;
    std::mutex m_;
    std::condition_variable cv_;
    std::deque<T> q_;
    bool closed_ = false;
};

// Pool of slot indices; acquire() blocks until one is free (or abort: -1).
class SlotPool {
public:
    SlotPool(Shared& s, size_t n) : s_(s) {
        for (size_t i = 0; i < n; ++i) free_.push_back(static_cast<int>(i));
        s.addWaker([this] { cv_.notify_all(); });
    }
    int acquire() {
        std::unique_lock<std::mutex> lock(m_);
        cv_.wait(lock, [&] { return !free_.empty() || s_.aborted(); });
        if (s_.aborted()) return -1;
        const int i = free_.front();
        free_.pop_front();
        return i;
    }
    void release(int i) {
        {
            std::lock_guard<std::mutex> lock(m_);
            free_.push_back(i);
        }
        cv_.notify_one();
    }

private:
    Shared& s_;
    std::mutex m_;
    std::condition_variable cv_;
    std::deque<int> free_;
};

struct InItem {
    int up;
    uint64_t k;
};
struct OutItem {
    int up;
    int down;  // -1: only the raw frame (bootstrap)
    uint64_t k;
};

using Clock = std::chrono::steady_clock;
double secondsSince(Clock::time_point t) { return std::chrono::duration<double>(Clock::now() - t).count(); }

// Seconds each stage spent waiting or working; printed with TEMPLE_LSFG_PROFILE=1.
struct Profile {
    std::atomic<double> readerRead{0}, readerWaitSlot{0}, gpuWaitInput{0}, gpuWaitDown{0}, gpuWaitFence{0},
        gpuSubmit{0}, writerWrite{0}, writerWaitInput{0};
    static void add(std::atomic<double>& a, double v) {
        double cur = a.load();
        while (!a.compare_exchange_weak(cur, cur + v)) {}
    }
};

bool writeAll(const uint8_t* data, size_t n) {
    return std::fwrite(data, 1, n, stdout) == n;
}

// Runs the three stages until EOF or failure; returns the first error, if any.
std::optional<HelperError> runPipeline(const Backend& be, int32_t ctxId, const TempleLsfg::LocalContext& lc,
                                       uint32_t width, uint32_t height, std::atomic<uint64_t>& framesIn,
                                       std::atomic<uint64_t>& framesOut, Profile& prof) {
    const size_t frameBytes = static_cast<size_t>(width) * height * 4;
    const size_t gens = lc.out.size();
    const size_t upSlots = 6, downSlots = 4;

    Shared shared;
    Staging staging(lc, width, height, upSlots, downSlots);
    SlotPool upPool(shared, upSlots), downPool(shared, downSlots);
    Queue<InItem> inQ(shared);
    Queue<OutItem> outQ(shared);

    std::thread reader([&] {
        uint64_t k = 0;
        for (;;) {
            auto t0 = Clock::now();
            const int slot = upPool.acquire();
            Profile::add(prof.readerWaitSlot, secondsSince(t0));
            if (slot < 0) break;
            t0 = Clock::now();
            const size_t got = std::fread(staging.up(static_cast<size_t>(slot)), 1, frameBytes, stdin);
            Profile::add(prof.readerRead, secondsSince(t0));
            if (got == 0) {
                if (std::ferror(stdin)) shared.note({"truncated_frame", kExitInput, "stdin read error"});
                break;  // EOF on a frame boundary
            }
            if (got != frameBytes) {
                shared.note({"truncated_frame", kExitInput, "input ended inside a frame"});
                break;
            }
            inQ.push({slot, k++});
        }
        inQ.close();
    });

    std::thread writer([&] {
        bool broken = false;
        for (;;) {
            auto t0 = Clock::now();
            auto item = outQ.pop();
            Profile::add(prof.writerWaitInput, secondsSince(t0));
            if (!item) break;
            t0 = Clock::now();
            if (!broken) {
                bool ok = true;
                if (item->down >= 0) {
                    for (size_t i = 0; ok && i < gens; ++i) {
                        ok = writeAll(staging.generated(static_cast<size_t>(item->down), i), frameBytes);
                        if (ok) ++framesOut;
                    }
                }
                if (ok) ok = writeAll(staging.up(static_cast<size_t>(item->up)), frameBytes);
                if (ok) ++framesOut;
                if (ok) ok = std::fflush(stdout) == 0;
                if (!ok) {
                    broken = true;
                    shared.fail({"output_closed", kExitOutput, "stdout closed"});
                }
            }
            Profile::add(prof.writerWrite, secondsSince(t0));
            upPool.release(item->up);
            if (item->down >= 0) downPool.release(item->down);
        }
    });

    // GPU stage (this thread). Up to kInFlight frames are submitted before the oldest is awaited.
    struct Pending {
        OutItem item;
        uint32_t f;
    };
    std::deque<Pending> inflight;
    std::vector<uint32_t> freeF;
    for (uint32_t i = Staging::kInFlight; i-- > 0;) freeF.push_back(i);

    auto retire = [&] {
        const Pending p = inflight.front();
        inflight.pop_front();
        const auto t0 = Clock::now();
        staging.wait(p.f);
        Profile::add(prof.gpuWaitFence, secondsSince(t0));
        freeF.push_back(p.f);
        outQ.push(p.item);
    };

    try {
        while (!shared.aborted()) {
            std::optional<InItem> in = inQ.pop(false);
            if (!in) {
                if (!inflight.empty()) {  // nothing to feed the GPU yet: deliver what is ready
                    retire();
                    continue;
                }
                const auto t0 = Clock::now();
                in = inQ.pop(true);
                Profile::add(prof.gpuWaitInput, secondsSince(t0));
                if (!in) break;
            }
            ++framesIn;
            if (freeF.empty()) retire();
            const uint32_t f = freeF.back();
            freeF.pop_back();

            if (in->k == 0) {
                // Bootstrap: both inputs hold the first frame; its outputs are discarded.
                const auto t0 = Clock::now();
                staging.submitUpload(f, static_cast<size_t>(in->up), true, true, true);
                be.present(ctxId, -1, {});
                Profile::add(prof.gpuSubmit, secondsSince(t0));
                inflight.push_back({{in->up, -1, in->k}, f});
            } else {
                const auto t1 = Clock::now();
                const int down = downPool.acquire();
                Profile::add(prof.gpuWaitDown, secondsSince(t1));
                if (down < 0) break;
                const auto t0 = Clock::now();
                staging.submitUpload(f, static_cast<size_t>(in->up), in->k % 2 == 0, in->k % 2 == 1, false);
                be.present(ctxId, -1, {});
                staging.submitDownload(f, static_cast<size_t>(down));
                Profile::add(prof.gpuSubmit, secondsSince(t0));
                inflight.push_back({{in->up, down, in->k}, f});
            }
        }
        while (!inflight.empty() && !shared.aborted()) retire();
    } catch (const HelperError& e) {
        shared.fail(e);
    } catch (const std::exception& e) {
        shared.fail(classifyRun(e));
    }

    // On a clean end everything was delivered; after an abort the pending frames are dropped.
    outQ.close();
    writer.join();
    if (shared.aborted()) {
        // The reader may be blocked in fread on stdin and cannot be cancelled: report and leave.
        const HelperError e = *shared.error();
        logLine(std::string("error: ") + e.code + (e.detail.empty() ? "" : " (" + e.detail + ")"));
        std::fflush(stderr);
        std::_Exit(e.exitCode);
    }
    reader.join();
    try { vkDeviceWaitIdle(lc.device); } catch (...) {}
    return shared.error();
}

// ---------------------------------------------------------------------------------------------
// Command line
// ---------------------------------------------------------------------------------------------

struct Options {
    bool probe = false;
    bool performance = false;
    std::filesystem::path dll;
    long width = 0;
    long height = 0;
    long multiplier = 0;
    double flowScale = 1.0;
};

// Command-line characters: the Windows entry point is wmain (UTF-16 arguments, so paths with any
// character survive); on Linux it is main with the raw bytes of the arguments, which path() takes as is.
#if defined(_WIN32)
using ArgChar = wchar_t;
#define TLSFG_ARG(s) L##s
#else
using ArgChar = char;
#define TLSFG_ARG(s) s
#endif
using ArgString = std::basic_string<ArgChar>;

long argToLong(const char* s, char** end) { return std::strtol(s, end, 10); }
long argToLong(const wchar_t* s, wchar_t** end) { return std::wcstol(s, end, 10); }
double argToDouble(const char* s, char** end) { return std::strtod(s, end); }
double argToDouble(const wchar_t* s, wchar_t** end) { return std::wcstod(s, end); }

bool parseLong(const ArgChar* s, long& out) {
    ArgChar* end = nullptr;
    errno = 0;
    const long v = argToLong(s, &end);
    if (end == s || *end != ArgChar(0) || errno != 0) return false;
    out = v;
    return true;
}

bool parseDouble(const ArgChar* s, double& out) {
    ArgChar* end = nullptr;
    errno = 0;
    const double v = argToDouble(s, &end);
    if (end == s || *end != ArgChar(0) || errno != 0 || !std::isfinite(v)) return false;
    out = v;
    return true;
}

void usage() {
    std::fputs(
        "usage: temple-lsfg --probe --dll <Lossless.dll>\n"
        "       temple-lsfg --dll <Lossless.dll> --width W --height H --multiplier N\n"
        "                   [--flow-scale F] [--performance]   (RGBA8 frames on stdin and stdout)\n",
        stderr);
}

bool parseArgs(int argc, ArgChar** argv, Options& o) {
    for (int i = 1; i < argc; ++i) {
        const ArgString a = argv[i];
        auto next = [&](const ArgChar*& v) {
            if (i + 1 >= argc) return false;
            v = argv[++i];
            return true;
        };
        const ArgChar* v = nullptr;
        if (a == TLSFG_ARG("--probe")) o.probe = true;
        else if (a == TLSFG_ARG("--performance")) o.performance = true;
        else if (a == TLSFG_ARG("--dll")) { if (!next(v)) return false; o.dll = std::filesystem::path(v); }
        else if (a == TLSFG_ARG("--width")) { if (!next(v) || !parseLong(v, o.width)) return false; }
        else if (a == TLSFG_ARG("--height")) { if (!next(v) || !parseLong(v, o.height)) return false; }
        else if (a == TLSFG_ARG("--multiplier")) { if (!next(v) || !parseLong(v, o.multiplier)) return false; }
        else if (a == TLSFG_ARG("--flow-scale")) { if (!next(v) || !parseDouble(v, o.flowScale)) return false; }
        else return false;
    }
    return true;
}

// ---------------------------------------------------------------------------------------------
// Start-up shared by probe and run
// ---------------------------------------------------------------------------------------------

int32_t loadShaders(const std::filesystem::path& dll, std::shared_ptr<tlsfg::ShaderSet>& set) {
    set = std::make_shared<tlsfg::ShaderSet>();
    return tlsfg::extractShaders(dll, *set);
}

[[noreturn]] void throwShaderError(int32_t rc) {
    if (rc == tlsfg::FgResult::DLL_UNREADABLE) throw HelperError{"dll_unreadable", kExitStart, "cannot read the DLL"};
    throw HelperError{"missing_spirv", kExitStart, "SPIR-V shader set not found in the DLL"};
}

bool wantFp16(const GpuChoice& gpu, const tlsfg::ShaderSet& set) {
    if (!(gpu.fp16 && set.hasFp16())) return false;
    const char* env = std::getenv("TEMPLE_LSFG_PRECISION");  // debugging aid: fp32 | fp16
    return !(env && std::strcmp(env, "fp32") == 0);
}

int runProbe(const Options& o) {
    try {
        std::shared_ptr<tlsfg::ShaderSet> set;
        const int32_t rc = loadShaders(o.dll, set);
        if (rc != tlsfg::FgResult::OK) throwShaderError(rc);

        const GpuChoice gpu = pickGpu();
        // Real check: LSFG creates its Vulkan device with this GPU and the extracted shader loader.
        try {
            LSFG_3_1::initialize(gpu.lsfgUuid, false, 1.0f, 1, tlsfg::makeLoader(set, false));
            LSFG_3_1::finalize();
        } catch (const std::exception& e) {
            try { LSFG_3_1::finalize(); } catch (...) {}
            throw HelperError{"device_unsupported", kExitStart, e.what()};
        }
        std::printf("{\"ok\":true,\"device\":\"%s\",\"fp16\":%s}\n", jsonEscape(gpu.name).c_str(),
                    wantFp16(gpu, *set) ? "true" : "false");
        return 0;
    } catch (const HelperError& e) {
        logLine(std::string("error: ") + e.code);
        std::printf("{\"ok\":false,\"error\":\"%s\"}\n", e.code);
        return e.exitCode;
    }
}

int runFrames(const Options& o) {
    const uint32_t width = static_cast<uint32_t>(o.width);
    const uint32_t height = static_cast<uint32_t>(o.height);
    const int multiplier = static_cast<int>(o.multiplier);
    const Backend& be = o.performance ? kBackend31p : kBackend31;

#if defined(_WIN32)
    _setmode(_fileno(stdin), _O_BINARY);
    _setmode(_fileno(stdout), _O_BINARY);
#endif
    std::setvbuf(stdin, nullptr, _IOFBF, kIoBuffer);
    std::setvbuf(stdout, nullptr, _IOFBF, kIoBuffer);

    bool lsfgUp = false;
    bool haveCtx = false;
    int32_t ctxId = 0;
    int exitCode = 0;
    uint64_t framesIn = 0;
    uint64_t framesOut = 0;

    try {
        std::shared_ptr<tlsfg::ShaderSet> set;
        const int32_t rc = loadShaders(o.dll, set);
        if (rc != tlsfg::FgResult::OK) throwShaderError(rc);
        const GpuChoice gpu = pickGpu();
        const bool fp16 = wantFp16(gpu, *set);
        logLine("device: " + gpu.name + ", shaders " + (fp16 ? "fp16" : "fp32") + ", " +
                (o.performance ? "3.1P" : "3.1"));

        TempleLsfg::LocalContext lc;
        try {
            be.initialize(gpu.lsfgUuid, false, lsfgFlowScale(static_cast<float>(o.flowScale)),
                          static_cast<uint64_t>(multiplier - 1), tlsfg::makeLoader(set, fp16));
            lsfgUp = true;
            lc = be.createContext(VkExtent2D{width, height}, kFormat);
            ctxId = lc.id;
            haveCtx = true;
        } catch (const HelperError&) {
            throw;
        } catch (const std::exception& e) {
            throw classifyStart(e);
        }
        set.reset();  // the DLL contents are no longer needed

        logLine("ready: " + std::to_string(width) + "x" + std::to_string(height) + " x" + std::to_string(multiplier));
        Profile prof;
        std::atomic<uint64_t> in{0}, out{0};
        const auto t0 = Clock::now();
        const std::optional<HelperError> pe = runPipeline(be, ctxId, lc, width, height, in, out, prof);
        framesIn = in;
        framesOut = out;
        if (std::getenv("TEMPLE_LSFG_PROFILE")) {
            char buf[512];
            std::snprintf(buf, sizeof(buf),
                          "profile: total %.2fs | reader read %.2f wait-slot %.2f | gpu wait-input %.2f wait-down-slot %.2f "
                          "wait-fence %.2f submit %.2f | writer write %.2f wait-input %.2f",
                          secondsSince(t0), prof.readerRead.load(), prof.readerWaitSlot.load(), prof.gpuWaitInput.load(),
                          prof.gpuWaitDown.load(), prof.gpuWaitFence.load(), prof.gpuSubmit.load(), prof.writerWrite.load(),
                          prof.writerWaitInput.load());
            logLine(buf);
        }
        if (pe) throw *pe;
        logLine("done: " + std::to_string(framesIn) + " frames in, " + std::to_string(framesOut) + " frames out");
    } catch (const HelperError& e) {
        logLine(std::string("error: ") + e.code + (e.detail.empty() ? "" : " (" + e.detail + ")"));
        exitCode = e.exitCode;
    } catch (const std::exception& e) {
        logLine(std::string("error: lsfg_failed (") + e.what() + ")");
        exitCode = kExitGpu;
    }

    // Shutdown order: context, then the library (the staging objects are already gone).
    try { if (haveCtx) be.deleteContext(ctxId); } catch (...) {}
    try { if (lsfgUp) be.finalize(); } catch (...) {}
    std::fflush(stdout);
    return exitCode;
}

}  // namespace

#if defined(_WIN32)
int wmain(int argc, wchar_t** argv) {
#else
int main(int argc, char** argv) {
    // A closed stdout must surface as a write error (exit 5, output_closed), not kill us with SIGPIPE.
    signal(SIGPIPE, SIG_IGN);
#endif
    Options o;
    if (!parseArgs(argc, argv, o)) {
        usage();
        return kExitUsage;
    }
    if (o.probe) {
        if (o.dll.empty()) {
            std::printf("{\"ok\":false,\"error\":\"dll_unreadable\"}\n");
            return kExitStart;
        }
        return runProbe(o);
    }
    if (o.dll.empty() || o.width < 64 || o.height < 64 || o.width > 16384 || o.height > 16384 ||
        (o.width % 2) != 0 || (o.height % 2) != 0 || o.multiplier < 2 || o.multiplier > 5 ||
        !(o.flowScale > 0.0 && o.flowScale <= 1.0)) {
        logLine("error: invalid arguments (width/height even and >= 64, multiplier 2..5, flow-scale in (0, 1])");
        usage();
        return kExitUsage;
    }
    return runFrames(o);
}
