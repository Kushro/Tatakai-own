// SPDX-License-Identifier: GPL-3.0-only
//
// Ported from Enigmata (app/src/main/cpp/fg) to Tatakai Next 0.4.0 (core/framegen): JNI class, library name,
// log tags and debug properties renamed; no other change.
//
// Adapted from FrankBarretta/LSFG-Android-Application
// (commit b84754199823615d32d65fe33ea59481dca88dcf), app/src/main/cpp/lsfg_render_loop.cpp, GPL-3.0:
// :483-516 (sleepUntilVsyncAligned, through fg_pacing alignToVsync), :547-797
// (destroySwapchain / createSwapchain / blitOutputToSwapchain), :1496-1665 (CPU
// blit through ANativeWindow_lock), :177-188 and :603-613 (CPU producer taint),
// :2046-2083 (pacing).
// Modifications: Enigmata, AGPL-3.0-or-later (combined under GPL-3.0 section 13).
//
// Enigmata frame generation (R13): presenter thread (fg_presenter.h is the
// contract).
//
// Threading
// - start() runs on the caller thread: it creates the VkSurfaceKHR (through
//   dev.ifn), a FIFO swapchain, its own command pool/buffer, fence and
//   semaphores, and launches ONE std::thread (threadMain).
// - The presenter is the consumer of ring.queue (OutFrameQueue, SPSC lock-free,
//   no event): when it is empty the thread sleeps 500 us and polls again.
// - The presenter is the producer of freeSlots (FreeSlotQueue, SPSC).
// - Without display_timing the session subscribes to ChoreoLooper: ONE
//   auxiliary looper thread per process (created lazily, never destroyed)
//   that owns an ALooper and an AChoreographer refresh-rate callback; it only
//   publishes the vsync period through an atomic (see "Refresh period" below).
// - Every queue operation goes through dev.submit / dev.present (queueMutex);
//   vkDeviceWaitIdle also takes dev.queueMutex (it needs every queue of the
//   device externally synchronized).
//
// Slots (convention of fg_engine.cpp): published in VK_IMAGE_LAYOUT_GENERAL,
// owned by dev.family, writes visible to later commands on dev.queue. The
// presenter transitions them GENERAL -> TRANSFER_SRC_OPTIMAL and leaves them
// there (the engine rewrites a returned slot from UNDEFINED). Slots have the
// INPUT size; the swapchain has the OUTPUT size (w x h) and vkCmdBlitImage
// scales with VK_FILTER_LINEAR. No device features: synchronization v1 only.
//
// Per OutFrame
// 1. epoch != ring.epoch -> slot returned with dropped = 1 (I4).
// 2. kind generated and isLate(presentAtNs, now, vsync) -> dropped = 1.
// 3. target = alignToVsync(presentAtNs, lastPost, vsync, 2 ms slack).
// 4. Path:
//    - DisplayTiming (dev.caps.displayTiming and the GOOGLE entry points are
//      loaded): no sleep; the present carries VkPresentTimesInfoGOOGLE with
//      desiredPresentTime = target. vsync = refreshDuration from
//      vkGetRefreshCycleDurationGOOGLE (see "Refresh period" below).
//      After each present vkGetPastPresentationTimingGOOGLE is drained;
//      presentErrorNs = actualPresentTime - desiredPresentTime, the maximum
//      over the newly reported presents (they belong to presents 1..3 frames
//      older than the current one; the timing of the current present is not
//      known yet). It is sent in the SlotReturn of the current frame; 0 when no
//      new timing was reported. lastPost = target.
//    - SleepVsync (no display_timing): clock_nanosleep(CLOCK_MONOTONIC,
//      TIMER_ABSTIME) until target (in <= 20 ms chunks so stop() is honoured,
//      capped at 500 ms ahead), epoch re-checked after the sleep. vsync comes
//      from the AChoreographer refresh-rate callback (ChoreoLooper, below), or a
//      60 Hz estimate (16 666 667 ns) when that API is missing.
//      lastPost = CLOCK_MONOTONIC after vkQueuePresentKHR returns.
//    - Both swapchain paths: vkAcquireNextImageKHR -> one command buffer
//      (slot GENERAL -> TRANSFER_SRC; swapchain image UNDEFINED -> TRANSFER_DST;
//      blit LINEAR; image -> PRESENT_SRC) -> dev.submit (waits the acquire
//      semaphore, signals the per-image render semaphore, own fence) ->
//      dev.present -> wait fence -> SlotReturn (the copy is complete).
//      VK_SUBOPTIMAL_KHR (acquire or present) counts as success and does NOT
//      recreate the swapchain (REV 1): preTransform is always IDENTITY when
//      supported, so with the device in landscape (transform hint != IDENTITY)
//      Android returns SUBOPTIMAL on every acquire/present; that is accepted
//      and SurfaceFlinger composes the rotation. Recreating on SUBOPTIMAL
//      would recreate on every frame (vkDeviceWaitIdle each time) and could
//      end on the CPU path. The swapchain is recreated only:
//        * after VK_ERROR_OUT_OF_DATE_KHR (acquire or present; that frame is
//          dropped), or
//        * when vkGetPhysicalDeviceSurfaceCapabilitiesKHR, polled at most once
//          per second (kExtentCheckPeriodNs), reports a currentExtent (not
//          0xFFFFFFFF) different from the currentExtent recorded when the
//          current swapchain was created. The reference is the recorded
//          currentExtent, not the swapchain extent: the swapchain is created
//          at the OUTPUT size (w x h) and the compositor scales it, so the two
//          legitimately differ and comparing them would loop. The same poll
//          also recreates when currentTransform differs from the one recorded
//          at creation (FG-F; e.g. a 180-degree turn keeps the extent), again
//          against the recorded currentTransform, never preTransform (which
//          stays IDENTITY; no pre-rotation pass). The log line carries both.
//        * (error recovery, unchanged) after a failed vkBeginCommandBuffer /
//          vkEndCommandBuffer / vkQueueSubmit, to reclaim the acquired image.
//      Any other error of vkQueuePresentKHR or vkAcquireNextImageKHR, or a
//      failed recreation -> CPU path (below); SUBOPTIMAL never leads there.
//      A driver crash inside vkQueuePresentKHR (R4) cannot be intercepted
//      in-process; only its error returns are.
//    - Cpu (fallback, sticky for the session): swapchain and VkSurfaceKHR are
//      destroyed (after vkDeviceWaitIdle), path() = Cpu. The slot AHBs have no
//      CPU_READ usage (GPU_SAMPLED_IMAGE | GPU_COLOR_OUTPUT, fg_engine), so
//      each frame is copied by the GPU (vkCmdCopyImageToBuffer) into an own
//      host-visible VkBuffer (input size, tightly packed RGBA8), the fence is
//      waited, the thread sleeps until target as in SleepVsync, and then
//      ANativeWindow_setBuffersGeometry(slot size, WINDOW_FORMAT_RGBA_8888;
//      only when it changes) + ANativeWindow_lock + row copy +
//      ANativeWindow_unlockAndPost; only then is the slot returned (shown).
//      The slot is not touched after the copy, but its SlotReturn has to say
//      whether the frame was really posted (REV 17). The compositor scales the
//      buffer to the view. Once ANativeWindow_lock ran, the window is bound to
//      a CPU producer and cannot host a Vulkan swapchain again (R4b): the
//      presenter only reports it through path() == Cpu; Kotlin recreates the
//      Surface before reconnecting.
// - VK_ERROR_DEVICE_LOST anywhere -> the slot is returned as dropped and the
//   thread exits. fg_presenter.h has no presenter error flag: the engine
//   reports DEVICE_LOST from its own queue operations (I9); path() keeps its
//   last value until stop().
//
// SlotReturn (REV 17): every slot goes back exactly once. dropped = 0 only when
// the frame was really presented: vkQueuePresentKHR returned VK_SUCCESS or
// VK_SUBOPTIMAL_KHR, or the CPU blit reached ANativeWindow_unlockAndPost.
// Everything else (old epoch, late generated frame, acquire timeout /
// NOT_READY, OUT_OF_DATE, command/submit failures, ANativeWindow_lock failure,
// stop while sleeping, device lost, fallback, pending frames at stop) returns
// dropped = 1, so the engine can count presented frames from dropped == 0.
//
// Refresh period (vsyncNs, used by alignToVsync / isLate; REV 2)
// - DisplayTiming: vkGetRefreshCycleDurationGOOGLE is read at swapchain
//   creation and then re-read after readPastTiming, throttled to at most once
//   per second (kRefreshReadPeriodNs). On Android the query goes through the
//   window to SurfaceFlinger, so it is not done on every frame; one second is
//   enough to follow a mode change (e.g. setFrameRate after start).
// - Without display_timing (SleepVsync, and Cpu): ChoreoLooper resolves with
//   dlsym(RTLD_DEFAULT) AChoreographer_getInstance,
//   AChoreographer_registerRefreshRateCallback and
//   AChoreographer_unregisterRefreshRateCallback (the last two are API 30;
//   minSdk is 26, hence dlsym). AChoreographer is per-thread and needs an
//   ALooper, so ChoreoLooper runs ONE looper thread per process (m3), started
//   by the first subscription and reused by every later session: ALooper_prepare
//   -> AChoreographer_getInstance -> ALooper_pollOnce loop. Sessions only
//   subscribe/unsubscribe (user count); the looper thread itself registers the
//   callback on 0 -> 1 and unregisters it on 1 -> 0, woken by ALooper_wake.
//   The thread is never destroyed, deliberately: AOSP's AChoreographer is
//   thread_local and never freed (fact 21), so one thread per session leaked
//   its fds on every start. The callback receives vsyncPeriodNanos
//   (= 1e9 / refresh rate) and stores it in an atomic; the presenter thread
//   loads it before each frame while subscribed. Until the first callback, or
//   when the API is missing (API < 30), vsync is 60 Hz.
// - The session unsubscribes in teardown() (non-blocking), right after the
//   presenter thread was joined and before vkDeviceWaitIdle / swapchain
//   destruction (I3); it stops reading the period at that point.
//
// Logging: the presenter receives no FgConfigN (start() has no config), so the
// periodic summary is logged every 2 s at ANDROID_LOG_DEBUG with tag
// "TatakaiFG" (visible with `adb shell setprop log.tag.TatakaiFG DEBUG` or a
// debug logcat filter). Path changes and errors are logged at INFO/WARN/ERROR.
//
// stop() (I3, idempotent): stop flag -> join -> ChoreoLooper unsubscribe -> vkDeviceWaitIdle -> swapchain
// (and its semaphores) -> VkSurfaceKHR -> ANativeWindow_release -> own command
// objects and readback buffer -> pending OutFrames returned as dropped. The
// ring slots are never destroyed here (they belong to the ring's owner).

#include "fg_presenter.h"

#include "fg_pacing.h"

#include <android/choreographer.h>
#include <android/log.h>
#include <android/looper.h>
#include <dlfcn.h>
#include <time.h>

#include <algorithm>
#include <array>
#include <atomic>
#include <cerrno>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <initializer_list>
#include <mutex>
#include <system_error>
#include <thread>
#include <vector>

namespace efg {

namespace {

constexpr const char* kTag = "TatakaiFG";
#define EFG_PRS_LOGE(...) __android_log_print(ANDROID_LOG_ERROR, kTag, __VA_ARGS__)
#define EFG_PRS_LOGW(...) __android_log_print(ANDROID_LOG_WARN, kTag, __VA_ARGS__)
#define EFG_PRS_LOGI(...) __android_log_print(ANDROID_LOG_INFO, kTag, __VA_ARGS__)
#define EFG_PRS_LOGD(...) __android_log_print(ANDROID_LOG_DEBUG, kTag, __VA_ARGS__)

constexpr int64_t kNsPerSec = 1'000'000'000;
constexpr int64_t kDefaultVsyncNs = 16'666'667;   // 60 Hz estimate without display_timing
constexpr int64_t kVsyncSlackNs = 2'000'000;      // molde default (1..5 ms, 2 ms)
constexpr int64_t kIdlePollNs = 500'000;          // empty OutFrameQueue: poll every 0.5 ms
constexpr int64_t kSleepChunkNs = 20'000'000;     // stop() latency bound while sleeping
constexpr int64_t kMaxSleepAheadNs = 500'000'000; // guard against a bogus presentAtNs
constexpr uint64_t kAcquireTimeoutNs = 250'000'000;
constexpr uint64_t kFenceSliceNs = 100'000'000;
constexpr int64_t kFenceStopGiveUpNs = 2'000'000'000;
constexpr int64_t kStatsLogPeriodNs = 2'000'000'000;
constexpr uint32_t kReturnRetries = 100;          // FreeSlotQueue full: 100 x 1 ms
constexpr uint32_t kMaxPastTimings = 16;
constexpr int64_t kRefreshReadPeriodNs = 1'000'000'000;  // vkGetRefreshCycleDurationGOOGLE re-read
constexpr int64_t kExtentCheckPeriodNs = 1'000'000'000;  // surface currentExtent poll
constexpr int kLooperPollMs = 250;                       // ChoreoLooper, callback registered: bounded wait
constexpr int kLooperIdlePollMs = -1;                    // ChoreoLooper, no subscribers: wait until woken
constexpr int64_t kMinVsyncNs = 2'000'000;               // sanity range for a reported period
constexpr int64_t kMaxVsyncNs = 100'000'000;

int64_t monoNowNs() {
    timespec ts{};
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return static_cast<int64_t>(ts.tv_sec) * kNsPerSec + ts.tv_nsec;
}

void sleepAbsNs(int64_t tNs) {
    timespec ts{};
    ts.tv_sec = static_cast<time_t>(tNs / kNsPerSec);
    ts.tv_nsec = static_cast<long>(tNs % kNsPerSec);
    while (clock_nanosleep(CLOCK_MONOTONIC, TIMER_ABSTIME, &ts, nullptr) == EINTR) {
    }
}

VkImageMemoryBarrier imageBarrier(VkImage image, VkAccessFlags srcAccess, VkAccessFlags dstAccess,
                                  VkImageLayout oldLayout, VkImageLayout newLayout) {
    VkImageMemoryBarrier b{};
    b.sType = VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER;
    b.srcAccessMask = srcAccess;
    b.dstAccessMask = dstAccess;
    b.oldLayout = oldLayout;
    b.newLayout = newLayout;
    b.srcQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
    b.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
    b.image = image;
    b.subresourceRange = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1};
    return b;
}

// Result of presenting one frame; the slot has already been returned.
enum class Outcome {
    Shown,
    Dropped,
    Fallback,    // switch to the CPU path
    DeviceLost,  // terminate the thread
};

bool plausibleVsync(int64_t ns) {
    return ns >= kMinVsyncNs && ns <= kMaxVsyncNs;
}

// Vsync period without display_timing: AChoreographer refresh-rate callback
// (API 30, resolved with dlsym) on ONE ALooper thread per process (m3).
//
// Lifecycle (deliberate): the looper thread is created lazily by the first
// subscribe() that finds the API, is detached, and lives until the process
// exits; the object itself is heap-allocated and never destroyed. In AOSP the
// AChoreographer returned by AChoreographer_getInstance is thread_local and
// is never freed (it keeps its DisplayEventReceiver fds, ~3 per thread, fact
// 21), so a thread per presenter session leaked those fds on every start;
// reusing one thread bounds the cost to one AChoreographer per process.
//
// Threading model
// - mutex_ guards users_, state_ and looper_. Sessions call subscribe() /
//   unsubscribe() from any thread; both only take mutex_ briefly, change
//   users_ and ALooper_wake the looper: they never wait for the looper thread.
// - The looper thread is the only caller of AChoreographer_getInstance,
//   register/unregister and the callback (dispatched inside ALooper_pollOnce),
//   as the per-thread AChoreographer requires. After every wake/timeout it
//   reads users_ under mutex_: users_ > 0 and not registered -> register;
//   users_ == 0 and registered -> unregister and period_ = 0. A wake sent
//   before the thread reaches pollOnce is not lost (ALooper_wake is sticky
//   until the next poll), and a change made before looper_ is published is
//   seen by the first users_ read. While registered, pollOnce is also bounded
//   (kLooperPollMs) as a safety net; with no subscribers it waits indefinitely
//   (kLooperIdlePollMs = -1, FIX-6/r5: no periodic wakeups while FG is off),
//   since subscribe()/unsubscribe() always ALooper_wake the looper.
// - period_ (atomic) is written only by the callback and by the looper thread
//   on unregister; presenter threads only load it, and only while subscribed.
// - The callback context is this never-destroyed object, so a callback that
//   AOSP still dispatches right after a 1 -> 0 transition cannot dangle.
// periodNs() is 0 until the first callback after a registration, or forever
// when the API is missing (callers then use 60 Hz).
class ChoreoLooper {
public:
    static ChoreoLooper& instance() {
        static ChoreoLooper* const self = new ChoreoLooper();  // never destroyed (see above)
        return *self;
    }

    ChoreoLooper(const ChoreoLooper&) = delete;
    ChoreoLooper& operator=(const ChoreoLooper&) = delete;

    // true: the caller holds a subscription and must unsubscribe() once.
    // false: API missing or the looper thread unavailable (60 Hz); nothing to undo.
    bool subscribe() {
        std::lock_guard<std::mutex> lock(mutex_);
        if (state_ == State::Unprobed) probeAndStartLocked();
        if (state_ != State::Running) return false;
        ++users_;
        if (looper_ != nullptr) ALooper_wake(looper_);  // 0 -> 1: the looper registers the callback
        return true;
    }

    // Non-blocking; the looper thread unregisters on 1 -> 0.
    void unsubscribe() {
        std::lock_guard<std::mutex> lock(mutex_);
        if (users_ > 0) --users_;
        if (looper_ != nullptr) ALooper_wake(looper_);
    }

    int64_t periodNs() const { return period_.load(std::memory_order_acquire); }

private:
    enum class State { Unprobed, Running, Unavailable };

    ChoreoLooper() = default;
    ~ChoreoLooper() = default;

    // mutex_ held.
    void probeAndStartLocked() {
        using GetInstanceFn = AChoreographer* (*)();
        using RegisterFn = void (*)(AChoreographer*, AChoreographer_refreshRateCallback, void*);
        getInstance_ = reinterpret_cast<GetInstanceFn>(dlsym(RTLD_DEFAULT, "AChoreographer_getInstance"));
        register_ = reinterpret_cast<RegisterFn>(dlsym(RTLD_DEFAULT, "AChoreographer_registerRefreshRateCallback"));
        unregister_ =
            reinterpret_cast<RegisterFn>(dlsym(RTLD_DEFAULT, "AChoreographer_unregisterRefreshRateCallback"));
        if (getInstance_ == nullptr || register_ == nullptr || unregister_ == nullptr) {
            EFG_PRS_LOGI("AChoreographer refresh-rate callback unavailable (API < 30): vsync fixed at 60 Hz");
            state_ = State::Unavailable;
            return;
        }
        try {
            std::thread([this] { run(); }).detach();  // process-lifetime thread
        } catch (const std::system_error&) {
            EFG_PRS_LOGW("ChoreoLooper: thread creation failed: vsync fixed at 60 Hz (retried on next start)");
            return;  // stays Unprobed
        }
        state_ = State::Running;
    }

    // Looper thread only: setup failed; later subscribe() calls return false.
    void markUnavailable() {
        std::lock_guard<std::mutex> lock(mutex_);
        state_ = State::Unavailable;
        users_ = 0;
        looper_ = nullptr;
    }

    static void onRefreshRate(int64_t vsyncPeriodNanos, void* data) {
        auto* self = static_cast<ChoreoLooper*>(data);
        if (!plausibleVsync(vsyncPeriodNanos)) return;
        const int64_t old = self->period_.exchange(vsyncPeriodNanos, std::memory_order_acq_rel);
        if (old != vsyncPeriodNanos) {
            EFG_PRS_LOGI("AChoreographer: vsync period %lld ns", static_cast<long long>(vsyncPeriodNanos));
        }
    }

    void run() {
        ALooper* looper = ALooper_prepare(0);
        if (looper == nullptr) {
            EFG_PRS_LOGW("ChoreoLooper: ALooper_prepare failed: vsync fixed at 60 Hz");
            markUnavailable();
            return;
        }
        AChoreographer* choreo = getInstance_();
        if (choreo == nullptr) {
            EFG_PRS_LOGW("ChoreoLooper: AChoreographer_getInstance returned null: vsync fixed at 60 Hz");
            markUnavailable();
            return;
        }
        ALooper_acquire(looper);  // held for the process lifetime (looper_ is used by ALooper_wake)
        {
            std::lock_guard<std::mutex> lock(mutex_);
            looper_ = looper;
        }
        bool registered = false;
        for (;;) {
            bool want;
            {
                std::lock_guard<std::mutex> lock(mutex_);
                want = users_ > 0;
            }
            if (want && !registered) {
                register_(choreo, &ChoreoLooper::onRefreshRate, this);
                registered = true;
            } else if (!want && registered) {
                unregister_(choreo, &ChoreoLooper::onRefreshRate, this);
                registered = false;
                period_.store(0, std::memory_order_release);  // next registration starts from 60 Hz
            }
            // FIX-6 (r5): 0 subscribers -> -1 (indefinite; subscribe/unsubscribe wake it).
            ALooper_pollOnce(registered ? kLooperPollMs : kLooperIdlePollMs, nullptr, nullptr, nullptr);
        }
    }

    AChoreographer* (*getInstance_)() = nullptr;
    void (*register_)(AChoreographer*, AChoreographer_refreshRateCallback, void*) = nullptr;
    void (*unregister_)(AChoreographer*, AChoreographer_refreshRateCallback, void*) = nullptr;
    std::mutex mutex_;
    State state_ = State::Unprobed;
    uint32_t users_ = 0;
    ALooper* looper_ = nullptr;
    std::atomic<int64_t> period_{0};
};

}  // namespace

struct FgPresenter::Impl {
    // Wiring
    FgDevice* dev = nullptr;
    ANativeWindow* window = nullptr;  // owned; released in teardown()
    uint32_t outW = 0;
    uint32_t outH = 0;
    PresentRing* ring = nullptr;
    FreeSlotQueue* freeSlots = nullptr;

    std::atomic<int32_t> path{static_cast<int32_t>(PresentPath::None)};
    std::atomic<bool> stopReq{false};
    std::thread thread;

    // WSI
    VkSurfaceKHR surface = VK_NULL_HANDLE;
    VkSwapchainKHR swapchain = VK_NULL_HANDLE;
    VkExtent2D swapExtent{0, 0};
    VkFormat swapFormat = VK_FORMAT_UNDEFINED;
    std::vector<VkImage> images;
    std::vector<VkSemaphore> acquireSems;  // images + 1, rotated
    std::vector<VkSemaphore> renderSems;   // one per swapchain image
    uint32_t acquireCursor = 0;
    bool needsRecreate = false;
    bool useTiming = false;
    uint32_t presentId = 0;

    // Own command objects
    VkCommandPool pool = VK_NULL_HANDLE;
    VkCommandBuffer cb = VK_NULL_HANDLE;
    VkFence fence = VK_NULL_HANDLE;
    bool fenceInFlight = false;

    // CPU path
    VkBuffer readBuf = VK_NULL_HANDLE;
    VkDeviceMemory readMem = VK_NULL_HANDLE;
    void* readPtr = nullptr;
    bool readCoherent = true;
    VkExtent2D readExtent{0, 0};
    uint32_t geomW = 0;
    uint32_t geomH = 0;
    bool cpuTainted = false;  // R4b: window bound to a CPU producer

    // Surface extent / transform reference for the recreation check (REV 1, FG-F)
    VkExtent2D createdCurrentExtent{0, 0};  // caps.currentExtent at swapchain creation
    VkSurfaceTransformFlagBitsKHR createdCurrentTransform = VK_SURFACE_TRANSFORM_IDENTITY_BIT_KHR;
    VkSurfaceTransformFlagBitsKHR createdPreTransform = VK_SURFACE_TRANSFORM_IDENTITY_BIT_KHR;
    int64_t lastExtentCheckNs = 0;

    // Pacing
    int64_t vsyncNs = kDefaultVsyncNs;
    int64_t lastPostNs = -1;
    int64_t lastRefreshReadNs = 0;  // last vkGetRefreshCycleDurationGOOGLE read
    int64_t loggedVsyncNs = 0;      // E22: last display_timing period logged (0 = none yet)
    // ChoreoLooper subscription (vsync source without display_timing). Touched
    // by start() before the presenter thread exists, by switchToCpu() on the
    // presenter thread, and by teardown() after it was joined (or never
    // started): thread creation/join order those accesses, no lock needed.
    bool choreoSubscribed = false;

    // Counters (presenter thread only; logged every 2 s)
    uint64_t shown = 0;
    uint64_t lateDrops = 0;
    uint64_t epochDrops = 0;
    uint64_t otherDrops = 0;
    int64_t lastErrNs = 0;
    int64_t lastLogNs = 0;

    PresentPath currentPath() const { return static_cast<PresentPath>(path.load(std::memory_order_relaxed)); }
    void setPath(PresentPath p) { path.store(static_cast<int32_t>(p), std::memory_order_relaxed); }

    void resetState() {
        stopReq.store(false, std::memory_order_relaxed);
        acquireCursor = 0;
        needsRecreate = false;
        useTiming = false;
        presentId = 0;
        fenceInFlight = false;
        readCoherent = true;
        readExtent = {0, 0};
        geomW = 0;
        geomH = 0;
        cpuTainted = false;
        vsyncNs = kDefaultVsyncNs;
        lastPostNs = -1;
        lastRefreshReadNs = 0;
        loggedVsyncNs = 0;
        createdCurrentExtent = {0, 0};
        createdCurrentTransform = VK_SURFACE_TRANSFORM_IDENTITY_BIT_KHR;
        createdPreTransform = VK_SURFACE_TRANSFORM_IDENTITY_BIT_KHR;
        lastExtentCheckNs = 0;
        shown = lateDrops = epochDrops = otherDrops = 0;
        lastErrNs = 0;
        lastLogNs = 0;
    }

    bool deviceAlive() const { return dev != nullptr && dev->device != VK_NULL_HANDLE; }

    // ---------------------------------------------------------------------
    // Slot return (presenter = producer of freeSlots)
    // ---------------------------------------------------------------------

    void giveBack(const OutFrame& f, bool dropped, int64_t errNs) {
        if (freeSlots == nullptr) return;
        SlotReturn r{};
        r.slot = f.slot;
        r.epoch = f.epoch;
        r.dropped = dropped ? 1 : 0;
        r.kind = f.kind;
        r.presentErrorNs = dropped ? 0 : errNs;
        for (uint32_t i = 0; i < kReturnRetries; ++i) {
            if (freeSlots->push(r)) return;
            if (stopReq.load(std::memory_order_relaxed)) break;
            sleepAbsNs(monoNowNs() + 1'000'000);
        }
        EFG_PRS_LOGW("FreeSlotQueue full: slot %u not returned", f.slot);
    }

    // ---------------------------------------------------------------------
    // Synchronization helpers
    // ---------------------------------------------------------------------

    // vkDeviceWaitIdle needs every queue of the device externally synchronized.
    void waitIdle() {
        if (!deviceAlive() || dev->fn.vkDeviceWaitIdle == nullptr) return;
        std::lock_guard<std::mutex> lock(dev->queueMutex);
        dev->fn.vkDeviceWaitIdle(dev->device);
    }

    // Waits for the in-flight submission of `cb`. VK_SUCCESS, VK_TIMEOUT (only
    // when stopping) or VK_ERROR_DEVICE_LOST / another error.
    VkResult waitFence() {
        if (!fenceInFlight) return VK_SUCCESS;
        const int64_t start = monoNowNs();
        for (;;) {
            const VkResult r = dev->fn.vkWaitForFences(dev->device, 1, &fence, VK_TRUE, kFenceSliceNs);
            if (r == VK_SUCCESS) {
                fenceInFlight = false;
                dev->fn.vkResetFences(dev->device, 1, &fence);
                return VK_SUCCESS;
            }
            if (r != VK_TIMEOUT) return r;
            if (stopReq.load(std::memory_order_relaxed) && monoNowNs() - start > kFenceStopGiveUpNs) {
                return VK_TIMEOUT;
            }
        }
    }

    // Stop-aware absolute sleep on CLOCK_MONOTONIC.
    void sleepUntil(int64_t targetNs) {
        const int64_t cap = monoNowNs() + kMaxSleepAheadNs;
        if (targetNs > cap) targetNs = cap;
        while (!stopReq.load(std::memory_order_relaxed)) {
            const int64_t now = monoNowNs();
            if (now >= targetNs) return;
            sleepAbsNs(std::min(targetNs, now + kSleepChunkNs));
        }
    }

    // ---------------------------------------------------------------------
    // Own command objects
    // ---------------------------------------------------------------------

    bool createCommandObjects() {
        auto& fn = dev->fn;
        VkCommandPoolCreateInfo pci{};
        pci.sType = VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO;
        pci.flags = VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT;
        pci.queueFamilyIndex = dev->family;
        if (fn.vkCreateCommandPool(dev->device, &pci, nullptr, &pool) != VK_SUCCESS) {
            pool = VK_NULL_HANDLE;
            return false;
        }
        VkCommandBufferAllocateInfo cai{};
        cai.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO;
        cai.commandPool = pool;
        cai.level = VK_COMMAND_BUFFER_LEVEL_PRIMARY;
        cai.commandBufferCount = 1;
        if (fn.vkAllocateCommandBuffers(dev->device, &cai, &cb) != VK_SUCCESS) {
            cb = VK_NULL_HANDLE;
            return false;
        }
        VkFenceCreateInfo fci{};
        fci.sType = VK_STRUCTURE_TYPE_FENCE_CREATE_INFO;
        if (fn.vkCreateFence(dev->device, &fci, nullptr, &fence) != VK_SUCCESS) {
            fence = VK_NULL_HANDLE;
            return false;
        }
        return true;
    }

    void destroyCommandObjects() {
        if (!deviceAlive()) return;
        auto& fn = dev->fn;
        if (fence != VK_NULL_HANDLE) fn.vkDestroyFence(dev->device, fence, nullptr);
        if (pool != VK_NULL_HANDLE) fn.vkDestroyCommandPool(dev->device, pool, nullptr);  // frees cb
        fence = VK_NULL_HANDLE;
        pool = VK_NULL_HANDLE;
        cb = VK_NULL_HANDLE;
        fenceInFlight = false;
    }

    bool beginCmd() {
        auto& fn = dev->fn;
        if (fn.vkResetCommandBuffer(cb, 0) != VK_SUCCESS) return false;
        VkCommandBufferBeginInfo bi{};
        bi.sType = VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO;
        bi.flags = VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT;
        return fn.vkBeginCommandBuffer(cb, &bi) == VK_SUCCESS;
    }

    // ---------------------------------------------------------------------
    // Surface and swapchain (render_loop :547-797)
    // ---------------------------------------------------------------------

    int32_t createSurface() {
        VkAndroidSurfaceCreateInfoKHR sci{};
        sci.sType = VK_STRUCTURE_TYPE_ANDROID_SURFACE_CREATE_INFO_KHR;
        sci.window = window;
        const VkResult r = dev->ifn.createAndroidSurfaceKHR(dev->instance, &sci, nullptr, &surface);
        if (r != VK_SUCCESS) {
            surface = VK_NULL_HANDLE;
            EFG_PRS_LOGE("vkCreateAndroidSurfaceKHR failed (rc=%d)%s", static_cast<int>(r),
                         r == VK_ERROR_NATIVE_WINDOW_IN_USE_KHR ? ": window already has a producer" : "");
            return FgResult::SURFACE;
        }
        VkBool32 canPresent = VK_FALSE;
        if (dev->ifn.getPhysicalDeviceSurfaceSupportKHR(dev->phys, dev->family, surface, &canPresent) !=
                VK_SUCCESS ||
            canPresent != VK_TRUE) {
            EFG_PRS_LOGE("queue family %u cannot present on this surface", dev->family);
            return FgResult::SURFACE;
        }
        return FgResult::OK;
    }

    void destroySurface() {
        if (surface != VK_NULL_HANDLE && dev != nullptr && dev->instance != VK_NULL_HANDLE &&
            dev->ifn.destroySurfaceKHR != nullptr) {
            dev->ifn.destroySurfaceKHR(dev->instance, surface, nullptr);
        }
        surface = VK_NULL_HANDLE;
    }

    // Swapchain and its semaphores. The caller has waited for the device.
    void destroySwapchain() {
        if (deviceAlive()) {
            auto& fn = dev->fn;
            for (VkSemaphore s : acquireSems) {
                if (s != VK_NULL_HANDLE) fn.vkDestroySemaphore(dev->device, s, nullptr);
            }
            for (VkSemaphore s : renderSems) {
                if (s != VK_NULL_HANDLE) fn.vkDestroySemaphore(dev->device, s, nullptr);
            }
            if (swapchain != VK_NULL_HANDLE) fn.vkDestroySwapchainKHR(dev->device, swapchain, nullptr);
        }
        acquireSems.clear();
        renderSems.clear();
        images.clear();
        swapchain = VK_NULL_HANDLE;
        swapExtent = {0, 0};
        swapFormat = VK_FORMAT_UNDEFINED;
        acquireCursor = 0;
        needsRecreate = false;
    }

    // FIFO swapchain on `surface`, output size, TRANSFER_DST usage,
    // minImageCount + 1 images. Returns OK, SURFACE (surface/format/usage) or
    // VK_INIT (our objects). On failure the swapchain is destroy()-safe.
    int32_t createSwapchain() {
        auto& fn = dev->fn;
        auto& ifn = dev->ifn;
        VkSurfaceCapabilitiesKHR caps{};
        if (ifn.getPhysicalDeviceSurfaceCapabilitiesKHR(dev->phys, surface, &caps) != VK_SUCCESS) {
            EFG_PRS_LOGE("vkGetPhysicalDeviceSurfaceCapabilitiesKHR failed");
            return FgResult::SURFACE;
        }
        if ((caps.supportedUsageFlags & VK_IMAGE_USAGE_TRANSFER_DST_BIT) == 0) {
            EFG_PRS_LOGE("surface does not support TRANSFER_DST swapchain images");
            return FgResult::SURFACE;
        }

        // Format: RGBA8 or BGRA8 UNORM with SRGB_NONLINEAR (the blit swizzles).
        uint32_t fmtCount = 0;
        if (ifn.getPhysicalDeviceSurfaceFormatsKHR(dev->phys, surface, &fmtCount, nullptr) != VK_SUCCESS) {
            return FgResult::SURFACE;
        }
        std::vector<VkSurfaceFormatKHR> fmts(fmtCount);
        if (fmtCount > 0) {
            const VkResult r = ifn.getPhysicalDeviceSurfaceFormatsKHR(dev->phys, surface, &fmtCount, fmts.data());
            if (r != VK_SUCCESS && r != VK_INCOMPLETE) return FgResult::SURFACE;
            fmts.resize(fmtCount);
        }
        VkSurfaceFormatKHR chosen{VK_FORMAT_UNDEFINED, VK_COLOR_SPACE_SRGB_NONLINEAR_KHR};
        if (fmts.size() == 1 && fmts[0].format == VK_FORMAT_UNDEFINED) {
            chosen.format = VK_FORMAT_R8G8B8A8_UNORM;  // any format allowed
        } else {
            for (VkFormat want : {VK_FORMAT_R8G8B8A8_UNORM, VK_FORMAT_B8G8R8A8_UNORM}) {
                for (const auto& f : fmts) {
                    if (f.format == want && f.colorSpace == VK_COLOR_SPACE_SRGB_NONLINEAR_KHR) {
                        chosen = f;
                        break;
                    }
                }
                if (chosen.format != VK_FORMAT_UNDEFINED) break;
            }
        }
        if (chosen.format == VK_FORMAT_UNDEFINED) {
            EFG_PRS_LOGE("no RGBA8/BGRA8 UNORM SRGB_NONLINEAR surface format");
            return FgResult::SURFACE;
        }

        // FIFO is always supported (and is what the pacing expects).
        uint32_t imageCount = caps.minImageCount + 1;
        if (caps.maxImageCount > 0 && imageCount > caps.maxImageCount) imageCount = caps.maxImageCount;
        if (imageCount < 2) imageCount = 2;

        // Output size (w x h), clamped to the surface limits; the compositor
        // scales it to the view.
        VkExtent2D extent{outW, outH};
        if (extent.width == 0 || extent.height == 0) extent = caps.currentExtent;
        if (extent.width == 0 || extent.width == UINT32_MAX) extent.width = 1920;
        if (extent.height == 0 || extent.height == UINT32_MAX) extent.height = 1080;
        extent.width = std::clamp(extent.width, std::max(caps.minImageExtent.width, 1u),
                                  std::max(caps.maxImageExtent.width, std::max(caps.minImageExtent.width, 1u)));
        extent.height = std::clamp(extent.height, std::max(caps.minImageExtent.height, 1u),
                                   std::max(caps.maxImageExtent.height, std::max(caps.minImageExtent.height, 1u)));

        // IDENTITY when available: the compositor applies the rotation, so the
        // blit never has to pre-rotate (the molde used currentTransform). With
        // currentTransform != IDENTITY (landscape) Android then returns
        // VK_SUBOPTIMAL_KHR on acquire/present; it is accepted, never a reason
        // to recreate (REV 1).
        const VkSurfaceTransformFlagBitsKHR transform =
            (caps.supportedTransforms & VK_SURFACE_TRANSFORM_IDENTITY_BIT_KHR) != 0
                ? VK_SURFACE_TRANSFORM_IDENTITY_BIT_KHR
                : caps.currentTransform;
        VkCompositeAlphaFlagBitsKHR alpha = VK_COMPOSITE_ALPHA_OPAQUE_BIT_KHR;
        if ((caps.supportedCompositeAlpha & VK_COMPOSITE_ALPHA_OPAQUE_BIT_KHR) == 0) {
            alpha = (caps.supportedCompositeAlpha & VK_COMPOSITE_ALPHA_INHERIT_BIT_KHR) != 0
                        ? VK_COMPOSITE_ALPHA_INHERIT_BIT_KHR
                        : static_cast<VkCompositeAlphaFlagBitsKHR>(caps.supportedCompositeAlpha &
                                                                   (~caps.supportedCompositeAlpha + 1));
        }

        VkSwapchainCreateInfoKHR sci{};
        sci.sType = VK_STRUCTURE_TYPE_SWAPCHAIN_CREATE_INFO_KHR;
        sci.surface = surface;
        sci.minImageCount = imageCount;
        sci.imageFormat = chosen.format;
        sci.imageColorSpace = chosen.colorSpace;
        sci.imageExtent = extent;
        sci.imageArrayLayers = 1;
        sci.imageUsage = VK_IMAGE_USAGE_TRANSFER_DST_BIT;
        sci.imageSharingMode = VK_SHARING_MODE_EXCLUSIVE;
        sci.preTransform = transform;
        sci.compositeAlpha = alpha;
        sci.presentMode = VK_PRESENT_MODE_FIFO_KHR;
        sci.clipped = VK_TRUE;
        const VkResult cr = fn.vkCreateSwapchainKHR(dev->device, &sci, nullptr, &swapchain);
        if (cr != VK_SUCCESS) {
            swapchain = VK_NULL_HANDLE;
            EFG_PRS_LOGE("vkCreateSwapchainKHR failed (rc=%d)", static_cast<int>(cr));
            return FgResult::SURFACE;
        }

        uint32_t realCount = 0;
        if (fn.vkGetSwapchainImagesKHR(dev->device, swapchain, &realCount, nullptr) != VK_SUCCESS ||
            realCount == 0) {
            return FgResult::VK_INIT;
        }
        images.resize(realCount);
        if (fn.vkGetSwapchainImagesKHR(dev->device, swapchain, &realCount, images.data()) != VK_SUCCESS) {
            return FgResult::VK_INIT;
        }
        images.resize(realCount);

        VkSemaphoreCreateInfo semInfo{};
        semInfo.sType = VK_STRUCTURE_TYPE_SEMAPHORE_CREATE_INFO;
        acquireSems.assign(realCount + 1, VK_NULL_HANDLE);
        renderSems.assign(realCount, VK_NULL_HANDLE);
        for (auto& s : acquireSems) {
            if (fn.vkCreateSemaphore(dev->device, &semInfo, nullptr, &s) != VK_SUCCESS) {
                s = VK_NULL_HANDLE;
                return FgResult::VK_INIT;
            }
        }
        for (auto& s : renderSems) {
            if (fn.vkCreateSemaphore(dev->device, &semInfo, nullptr, &s) != VK_SUCCESS) {
                s = VK_NULL_HANDLE;
                return FgResult::VK_INIT;
            }
        }
        swapExtent = extent;
        swapFormat = chosen.format;
        acquireCursor = 0;
        needsRecreate = false;
        createdCurrentExtent = caps.currentExtent;  // reference for surfaceExtentChanged()
        createdCurrentTransform = caps.currentTransform;
        createdPreTransform = transform;
        lastExtentCheckNs = monoNowNs();

        // Refresh period for alignToVsync / isLate.
        lastRefreshReadNs = 0;  // a new swapchain always reads it
        if (useTiming) refreshTimingVsync(monoNowNs());
        EFG_PRS_LOGI("swapchain ready: %ux%u fmt=%d images=%u FIFO displayTiming=%d vsync=%lld ns "
                     "preTransform=%d currentTransform=%d currentExtent=%ux%u",
                     extent.width, extent.height, static_cast<int>(chosen.format), realCount, useTiming ? 1 : 0,
                     static_cast<long long>(vsyncNs), static_cast<int>(transform),
                     static_cast<int>(caps.currentTransform), caps.currentExtent.width, caps.currentExtent.height);
        return FgResult::OK;
    }

    bool recreateSwapchain() {
        waitIdle();
        destroySwapchain();
        const int32_t rc = createSwapchain();
        if (rc != FgResult::OK) {
            EFG_PRS_LOGW("swapchain recreation failed (rc=%d)", rc);
            return false;
        }
        return true;
    }

    // REV 1: true when the surface's currentExtent (not the 0xFFFFFFFF "any"
    // value) differs from the one recorded at swapchain creation; FG-F: or its
    // currentTransform does. Polled at most once per kExtentCheckPeriodNs; a
    // failed query changes nothing (the next acquire/present reports a lost or
    // out-of-date surface).
    bool surfaceExtentChanged(int64_t nowNs) {
        if (surface == VK_NULL_HANDLE || nowNs - lastExtentCheckNs < kExtentCheckPeriodNs) return false;
        lastExtentCheckNs = nowNs;
        VkSurfaceCapabilitiesKHR caps{};
        if (dev->ifn.getPhysicalDeviceSurfaceCapabilitiesKHR(dev->phys, surface, &caps) != VK_SUCCESS) {
            return false;
        }
        const VkExtent2D e = caps.currentExtent;
        const bool extentKnown = e.width != UINT32_MAX && e.height != UINT32_MAX;
        const bool extentChanged =
            extentKnown && (e.width != createdCurrentExtent.width || e.height != createdCurrentExtent.height);
        const bool transformChanged = caps.currentTransform != createdCurrentTransform;
        if (!extentChanged && !transformChanged) return false;
        EFG_PRS_LOGI("surface currentExtent %ux%u -> %ux%u, currentTransform %d -> %d (preTransform=%d): "
                     "recreating the swapchain",
                     createdCurrentExtent.width, createdCurrentExtent.height, e.width, e.height,
                     static_cast<int>(createdCurrentTransform), static_cast<int>(caps.currentTransform),
                     static_cast<int>(createdPreTransform));
        return true;
    }

    // REV 2, display_timing: re-reads vkGetRefreshCycleDurationGOOGLE, at most
    // once per kRefreshReadPeriodNs (it queries SurfaceFlinger through the
    // window), so a refresh-rate change after start is followed within ~1 s.
    void refreshTimingVsync(int64_t nowNs) {
        if (!useTiming || swapchain == VK_NULL_HANDLE) return;
        if (lastRefreshReadNs != 0 && nowNs - lastRefreshReadNs < kRefreshReadPeriodNs) return;
        lastRefreshReadNs = nowNs;
        VkRefreshCycleDurationGOOGLE rc{};
        if (dev->fn.vkGetRefreshCycleDurationGOOGLE(dev->device, swapchain, &rc) != VK_SUCCESS) return;
        const int64_t ns = static_cast<int64_t>(rc.refreshDuration);
        if (!plausibleVsync(ns)) return;
        if (ns != vsyncNs) {
            // E22: log only the first period or a change of more than 1 % against
            // the last logged one (jitter of the reading does not spam logcat).
            if (loggedVsyncNs == 0 || std::llabs(ns - loggedVsyncNs) * 100 > loggedVsyncNs) {
                // mi-5: the origin is the last LOGGED period (or the previous
                // vsyncNs before the first log), so sub-1 % drift accumulated
                // between logs is shown against what the reader last saw.
                const int64_t fromNs = loggedVsyncNs != 0 ? loggedVsyncNs : vsyncNs;
                EFG_PRS_LOGI("display_timing: vsync period %lld -> %lld ns", static_cast<long long>(fromNs),
                             static_cast<long long>(ns));
                loggedVsyncNs = ns;
            }
            vsyncNs = ns;
        }
    }

    // m3: subscribe once per session to the process-wide ChoreoLooper (no-op if
    // already subscribed); without the API the session stays at 60 Hz.
    void subscribeChoreo() {
        if (!choreoSubscribed) choreoSubscribed = ChoreoLooper::instance().subscribe();
    }

    // Idempotent and non-blocking; after it the session no longer reads the period.
    void unsubscribeChoreo() {
        if (!choreoSubscribed) return;
        choreoSubscribed = false;
        ChoreoLooper::instance().unsubscribe();
    }

    // REV 2, no display_timing: latest AChoreographer period, else 60 Hz.
    void refreshChoreoVsync() {
        if (useTiming) return;
        const int64_t ns = choreoSubscribed ? ChoreoLooper::instance().periodNs() : 0;
        vsyncNs = plausibleVsync(ns) ? ns : kDefaultVsyncNs;
    }

    // Present failure (R4): leave WSI for good on this window.
    void switchToCpu() {
        waitIdle();
        destroySwapchain();
        destroySurface();
        useTiming = false;
        vsyncNs = kDefaultVsyncNs;
        lastPostNs = -1;
        subscribeChoreo();  // no display_timing any more: AChoreographer or 60 Hz (no-op if subscribed)
        refreshChoreoVsync();
        setPath(PresentPath::Cpu);
        EFG_PRS_LOGW("presenter: switching to CPU blit (ANativeWindow_lock); the window will be bound to a CPU "
                     "producer (R4b, Kotlin must recreate the Surface)");
    }

    // ---------------------------------------------------------------------
    // Display timing (VK_GOOGLE_display_timing)
    // ---------------------------------------------------------------------

    // Drains vkGetPastPresentationTimingGOOGLE; returns the largest
    // actualPresentTime - desiredPresentTime among the new records, 0 if none.
    int64_t readPastTiming() {
        auto& fn = dev->fn;
        uint32_t count = 0;
        if (fn.vkGetPastPresentationTimingGOOGLE(dev->device, swapchain, &count, nullptr) != VK_SUCCESS ||
            count == 0) {
            return 0;
        }
        std::array<VkPastPresentationTimingGOOGLE, kMaxPastTimings> past{};
        count = std::min<uint32_t>(count, kMaxPastTimings);
        const VkResult r = fn.vkGetPastPresentationTimingGOOGLE(dev->device, swapchain, &count, past.data());
        if (r != VK_SUCCESS && r != VK_INCOMPLETE) return 0;
        bool have = false;
        int64_t worst = 0;
        for (uint32_t i = 0; i < count; ++i) {
            if (past[i].desiredPresentTime == 0 || past[i].actualPresentTime == 0) continue;
            const int64_t e =
                static_cast<int64_t>(past[i].actualPresentTime) - static_cast<int64_t>(past[i].desiredPresentTime);
            worst = have ? std::max(worst, e) : e;
            have = true;
        }
        if (have) lastErrNs = worst;
        return have ? worst : 0;
    }

    // ---------------------------------------------------------------------
    // Swapchain present (render_loop blitOutputToSwapchain)
    // ---------------------------------------------------------------------

    // REV 1: polls the surface currentExtent (at most once per second) and
    // flags a recreation when it changed. Called by handleFrame() before
    // presentSwapchain(); on the SleepVsync path BEFORE sleeping until the
    // target (m9), so the query never sits between the wake-up and the acquire.
    void pollSurfaceExtent() {
        if (!needsRecreate && swapchain != VK_NULL_HANDLE && surfaceExtentChanged(monoNowNs())) {
            needsRecreate = true;
        }
    }

    Outcome presentSwapchain(const OutFrame& f, int64_t targetNs) {
        auto& fn = dev->fn;
        // REV 1: the only recreation triggers are OUT_OF_DATE (sets
        // needsRecreate below), a changed surface currentExtent (polled by
        // pollSurfaceExtent() in handleFrame(), at most once per second) and
        // the command/submit error recovery.
        if (needsRecreate && !recreateSwapchain()) {
            giveBack(f, true, 0);
            return Outcome::Fallback;
        }
        if (swapchain == VK_NULL_HANDLE) {
            giveBack(f, true, 0);
            return Outcome::Fallback;
        }
        const VkResult fr = waitFence();
        if (fr == VK_ERROR_DEVICE_LOST) {
            giveBack(f, true, 0);
            return Outcome::DeviceLost;
        }
        if (fr != VK_SUCCESS) {
            giveBack(f, true, 0);
            return Outcome::Dropped;
        }

        VkSemaphore acquireSem = acquireSems[acquireCursor];
        acquireCursor = (acquireCursor + 1) % static_cast<uint32_t>(acquireSems.size());
        uint32_t idx = 0;
        const VkResult ar =
            fn.vkAcquireNextImageKHR(dev->device, swapchain, kAcquireTimeoutNs, acquireSem, VK_NULL_HANDLE, &idx);
        if (ar == VK_ERROR_OUT_OF_DATE_KHR) {
            needsRecreate = true;
            giveBack(f, true, 0);
            return Outcome::Dropped;
        }
        if (ar == VK_TIMEOUT || ar == VK_NOT_READY) {
            giveBack(f, true, 0);
            return Outcome::Dropped;
        }
        if (ar == VK_ERROR_DEVICE_LOST) {
            giveBack(f, true, 0);
            return Outcome::DeviceLost;
        }
        // VK_SUBOPTIMAL_KHR: the image was acquired; treated exactly as
        // VK_SUCCESS, no recreation (REV 1, preTransform IDENTITY in landscape).
        if (ar != VK_SUCCESS && ar != VK_SUBOPTIMAL_KHR) {
            EFG_PRS_LOGW("vkAcquireNextImageKHR failed (rc=%d)", static_cast<int>(ar));
            giveBack(f, true, 0);
            return Outcome::Fallback;
        }

        const AhbImage& slot = ring->slots[f.slot];
        const VkImage dst = images[idx];
        if (!beginCmd()) {
            needsRecreate = true;  // the acquired image is reclaimed by the recreation
            giveBack(f, true, 0);
            return Outcome::Dropped;
        }
        std::array<VkImageMemoryBarrier, 2> pre{
            imageBarrier(slot.image, 0, VK_ACCESS_TRANSFER_READ_BIT, VK_IMAGE_LAYOUT_GENERAL,
                         VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL),
            imageBarrier(dst, 0, VK_ACCESS_TRANSFER_WRITE_BIT, VK_IMAGE_LAYOUT_UNDEFINED,
                         VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL),
        };
        // ALL_COMMANDS: chains with the engine's publish barrier and with the
        // acquire semaphore wait (TRANSFER stage).
        fn.vkCmdPipelineBarrier(cb, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, VK_PIPELINE_STAGE_TRANSFER_BIT, 0, 0,
                                nullptr, 0, nullptr, static_cast<uint32_t>(pre.size()), pre.data());
        VkImageBlit region{};
        region.srcSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1};
        region.srcOffsets[1] = {static_cast<int32_t>(slot.extent.width), static_cast<int32_t>(slot.extent.height),
                                1};
        region.dstSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1};
        region.dstOffsets[1] = {static_cast<int32_t>(swapExtent.width), static_cast<int32_t>(swapExtent.height), 1};
        fn.vkCmdBlitImage(cb, slot.image, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, dst,
                          VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, 1, &region, VK_FILTER_LINEAR);
        const VkImageMemoryBarrier toPresent = imageBarrier(dst, VK_ACCESS_TRANSFER_WRITE_BIT, 0,
                                                            VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL,
                                                            VK_IMAGE_LAYOUT_PRESENT_SRC_KHR);
        fn.vkCmdPipelineBarrier(cb, VK_PIPELINE_STAGE_TRANSFER_BIT, VK_PIPELINE_STAGE_BOTTOM_OF_PIPE_BIT, 0, 0,
                                nullptr, 0, nullptr, 1, &toPresent);
        if (fn.vkEndCommandBuffer(cb) != VK_SUCCESS) {
            needsRecreate = true;
            giveBack(f, true, 0);
            return Outcome::Dropped;
        }

        VkSemaphore renderSem = renderSems[idx];
        const VkPipelineStageFlags waitStage = VK_PIPELINE_STAGE_TRANSFER_BIT;
        VkSubmitInfo si{};
        si.sType = VK_STRUCTURE_TYPE_SUBMIT_INFO;
        si.waitSemaphoreCount = 1;
        si.pWaitSemaphores = &acquireSem;
        si.pWaitDstStageMask = &waitStage;
        si.commandBufferCount = 1;
        si.pCommandBuffers = &cb;
        si.signalSemaphoreCount = 1;
        si.pSignalSemaphores = &renderSem;
        const VkResult sr = dev->submit(si, fence);
        if (sr == VK_ERROR_DEVICE_LOST) {
            giveBack(f, true, 0);
            return Outcome::DeviceLost;
        }
        if (sr != VK_SUCCESS) {
            EFG_PRS_LOGW("vkQueueSubmit(present blit) failed (rc=%d)", static_cast<int>(sr));
            needsRecreate = true;
            giveBack(f, true, 0);
            return Outcome::Dropped;
        }
        fenceInFlight = true;

        VkPresentInfoKHR pi{};
        pi.sType = VK_STRUCTURE_TYPE_PRESENT_INFO_KHR;
        pi.waitSemaphoreCount = 1;
        pi.pWaitSemaphores = &renderSem;
        pi.swapchainCount = 1;
        pi.pSwapchains = &swapchain;
        pi.pImageIndices = &idx;
        VkPresentTimeGOOGLE when{};
        VkPresentTimesInfoGOOGLE times{};
        if (useTiming) {
            presentId = presentId + 1 == 0 ? 1 : presentId + 1;
            when.presentID = presentId;
            when.desiredPresentTime = targetNs > 0 ? static_cast<uint64_t>(targetNs) : 0;  // CLOCK_MONOTONIC
            times.sType = VK_STRUCTURE_TYPE_PRESENT_TIMES_INFO_GOOGLE;
            times.swapchainCount = 1;
            times.pTimes = &when;
            pi.pNext = &times;
        }
        const VkResult pr = dev->present(pi);
        const int64_t postedNs = monoNowNs();

        // VK_SUBOPTIMAL_KHR = presented, same as VK_SUCCESS, no recreation (REV 1).
        const bool presented = pr == VK_SUCCESS || pr == VK_SUBOPTIMAL_KHR;

        // The slot goes back only once the blit has completed.
        const VkResult wr = waitFence();
        if (wr == VK_ERROR_DEVICE_LOST || pr == VK_ERROR_DEVICE_LOST) {
            giveBack(f, true, 0);
            return Outcome::DeviceLost;
        }
        if (wr != VK_SUCCESS) {
            // Only when stopping (waitFence gives up): dropped = 0 only if the
            // present itself went through (REV 17).
            giveBack(f, !presented, 0);
            return presented ? Outcome::Shown : Outcome::Dropped;
        }
        if (presented) {
            lastPostNs = useTiming ? targetNs : postedNs;
            int64_t err = 0;
            if (useTiming) {
                err = readPastTiming();
                refreshTimingVsync(monoNowNs());  // REV 2: after readPastTiming, <= once per second
            }
            giveBack(f, false, err);
            return Outcome::Shown;
        }
        if (pr == VK_ERROR_OUT_OF_DATE_KHR) {
            needsRecreate = true;
            giveBack(f, true, 0);
            return Outcome::Dropped;
        }
        EFG_PRS_LOGE("vkQueuePresentKHR failed (rc=%d)", static_cast<int>(pr));
        giveBack(f, true, 0);
        return Outcome::Fallback;
    }

    // ---------------------------------------------------------------------
    // CPU path (render_loop :1496-1665)
    // ---------------------------------------------------------------------

    uint32_t pickHostMemoryType(uint32_t bits, VkMemoryPropertyFlags flags) const {
        VkPhysicalDeviceMemoryProperties props{};
        dev->ifn.getPhysicalDeviceMemoryProperties(dev->phys, &props);
        for (uint32_t i = 0; i < props.memoryTypeCount; ++i) {
            if ((bits & (1u << i)) != 0 && (props.memoryTypes[i].propertyFlags & flags) == flags) return i;
        }
        return UINT32_MAX;
    }

    void destroyReadback() {
        if (deviceAlive()) {
            auto& fn = dev->fn;
            if (readPtr != nullptr) fn.vkUnmapMemory(dev->device, readMem);
            if (readBuf != VK_NULL_HANDLE) fn.vkDestroyBuffer(dev->device, readBuf, nullptr);
            if (readMem != VK_NULL_HANDLE) fn.vkFreeMemory(dev->device, readMem, nullptr);
        }
        readPtr = nullptr;
        readBuf = VK_NULL_HANDLE;
        readMem = VK_NULL_HANDLE;
        readExtent = {0, 0};
    }

    // Host-visible VkBuffer of extent.width * extent.height * 4 bytes (RGBA8).
    bool ensureReadback(VkExtent2D extent) {
        if (readBuf != VK_NULL_HANDLE && readExtent.width == extent.width && readExtent.height == extent.height) {
            return true;
        }
        destroyReadback();
        auto& fn = dev->fn;
        VkBufferCreateInfo bci{};
        bci.sType = VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO;
        bci.size = static_cast<VkDeviceSize>(extent.width) * extent.height * 4u;
        bci.usage = VK_BUFFER_USAGE_TRANSFER_DST_BIT;
        bci.sharingMode = VK_SHARING_MODE_EXCLUSIVE;
        if (bci.size == 0 || fn.vkCreateBuffer(dev->device, &bci, nullptr, &readBuf) != VK_SUCCESS) {
            readBuf = VK_NULL_HANDLE;
            return false;
        }
        VkMemoryRequirements req{};
        fn.vkGetBufferMemoryRequirements(dev->device, readBuf, &req);
        uint32_t type = pickHostMemoryType(req.memoryTypeBits,
                                           VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_CACHED_BIT);
        if (type == UINT32_MAX) {
            type = pickHostMemoryType(req.memoryTypeBits,
                                      VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT);
        }
        if (type == UINT32_MAX) type = pickHostMemoryType(req.memoryTypeBits, VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT);
        if (type == UINT32_MAX) {
            destroyReadback();
            return false;
        }
        readCoherent = pickHostMemoryType(1u << type, VK_MEMORY_PROPERTY_HOST_COHERENT_BIT) == type;
        VkMemoryAllocateInfo mai{};
        mai.sType = VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO;
        mai.allocationSize = req.size;
        mai.memoryTypeIndex = type;
        if (fn.vkAllocateMemory(dev->device, &mai, nullptr, &readMem) != VK_SUCCESS) {
            readMem = VK_NULL_HANDLE;
            destroyReadback();
            return false;
        }
        if (fn.vkBindBufferMemory(dev->device, readBuf, readMem, 0) != VK_SUCCESS ||
            fn.vkMapMemory(dev->device, readMem, 0, VK_WHOLE_SIZE, 0, &readPtr) != VK_SUCCESS) {
            readPtr = nullptr;
            destroyReadback();
            return false;
        }
        readExtent = extent;
        return true;
    }

    Outcome presentCpu(const OutFrame& f, int64_t targetNs) {
        auto& fn = dev->fn;
        const AhbImage& slot = ring->slots[f.slot];
        const VkResult fr = waitFence();
        if (fr == VK_ERROR_DEVICE_LOST) {
            giveBack(f, true, 0);
            return Outcome::DeviceLost;
        }
        if (fr != VK_SUCCESS || !ensureReadback(slot.extent) || !beginCmd()) {
            giveBack(f, true, 0);
            return Outcome::Dropped;
        }
        const VkImageMemoryBarrier toSrc = imageBarrier(slot.image, 0, VK_ACCESS_TRANSFER_READ_BIT,
                                                        VK_IMAGE_LAYOUT_GENERAL, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL);
        fn.vkCmdPipelineBarrier(cb, VK_PIPELINE_STAGE_ALL_COMMANDS_BIT, VK_PIPELINE_STAGE_TRANSFER_BIT, 0, 0,
                                nullptr, 0, nullptr, 1, &toSrc);
        VkBufferImageCopy region{};
        region.imageSubresource = {VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1};
        region.imageExtent = {slot.extent.width, slot.extent.height, 1};
        fn.vkCmdCopyImageToBuffer(cb, slot.image, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, readBuf, 1, &region);
        VkBufferMemoryBarrier toHost{};
        toHost.sType = VK_STRUCTURE_TYPE_BUFFER_MEMORY_BARRIER;
        toHost.srcAccessMask = VK_ACCESS_TRANSFER_WRITE_BIT;
        toHost.dstAccessMask = VK_ACCESS_HOST_READ_BIT;
        toHost.srcQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
        toHost.dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED;
        toHost.buffer = readBuf;
        toHost.offset = 0;
        toHost.size = VK_WHOLE_SIZE;
        fn.vkCmdPipelineBarrier(cb, VK_PIPELINE_STAGE_TRANSFER_BIT, VK_PIPELINE_STAGE_HOST_BIT, 0, 0, nullptr, 1,
                                &toHost, 0, nullptr);
        if (fn.vkEndCommandBuffer(cb) != VK_SUCCESS) {
            giveBack(f, true, 0);
            return Outcome::Dropped;
        }
        VkSubmitInfo si{};
        si.sType = VK_STRUCTURE_TYPE_SUBMIT_INFO;
        si.commandBufferCount = 1;
        si.pCommandBuffers = &cb;
        const VkResult sr = dev->submit(si, fence);
        if (sr == VK_ERROR_DEVICE_LOST) {
            giveBack(f, true, 0);
            return Outcome::DeviceLost;
        }
        if (sr != VK_SUCCESS) {
            giveBack(f, true, 0);
            return Outcome::Dropped;
        }
        fenceInFlight = true;
        const VkResult wr = waitFence();
        if (wr == VK_ERROR_DEVICE_LOST) {
            giveBack(f, true, 0);
            return Outcome::DeviceLost;
        }
        if (wr != VK_SUCCESS) {
            giveBack(f, true, 0);
            return Outcome::Dropped;
        }
        if (!readCoherent) {
            VkMappedMemoryRange range{};
            range.sType = VK_STRUCTURE_TYPE_MAPPED_MEMORY_RANGE;
            range.memory = readMem;
            range.offset = 0;
            range.size = VK_WHOLE_SIZE;
            fn.vkInvalidateMappedMemoryRanges(dev->device, 1, &range);
        }
        // The copy is complete; the slot is no longer read, but it is returned
        // only after the post so that dropped says whether it was shown (REV 17).
        sleepUntil(targetNs);
        if (stopReq.load(std::memory_order_relaxed)) {
            giveBack(f, true, 0);
            return Outcome::Dropped;
        }

        const uint32_t w = readExtent.width;
        const uint32_t h = readExtent.height;
        if (w != geomW || h != geomH) {
            ANativeWindow_setBuffersGeometry(window, static_cast<int32_t>(w), static_cast<int32_t>(h),
                                             WINDOW_FORMAT_RGBA_8888);
            geomW = w;
            geomH = h;
            EFG_PRS_LOGI("CPU blit: setBuffersGeometry %ux%u RGBA_8888", w, h);
        }
        ANativeWindow_Buffer buf{};
        if (ANativeWindow_lock(window, &buf, nullptr) != 0 || buf.bits == nullptr) {
            if (otherDrops < 5 || otherDrops % 300 == 0) EFG_PRS_LOGW("ANativeWindow_lock failed");
            giveBack(f, true, 0);
            return Outcome::Dropped;
        }
        if (!cpuTainted) {
            cpuTainted = true;
            EFG_PRS_LOGW("window bound to a CPU producer (R4b)");
        }
        if (buf.format == WINDOW_FORMAT_RGBA_8888 || buf.format == WINDOW_FORMAT_RGBX_8888) {
            const uint32_t rows = std::min<uint32_t>(h, static_cast<uint32_t>(std::max(buf.height, 0)));
            const uint32_t cols = std::min<uint32_t>(w, static_cast<uint32_t>(std::max(buf.width, 0)));
            const auto* src = static_cast<const uint8_t*>(readPtr);
            auto* dstBytes = static_cast<uint8_t*>(buf.bits);
            const size_t srcStride = static_cast<size_t>(w) * 4u;
            const size_t dstStride = static_cast<size_t>(buf.stride) * 4u;
            for (uint32_t y = 0; y < rows; ++y) {
                std::memcpy(dstBytes + y * dstStride, src + y * srcStride, static_cast<size_t>(cols) * 4u);
            }
        }
        ANativeWindow_unlockAndPost(window);
        lastPostNs = monoNowNs();
        giveBack(f, false, 0);  // CPU blit done: presented
        return Outcome::Shown;
    }

    // ---------------------------------------------------------------------
    // Thread
    // ---------------------------------------------------------------------

    void maybeLogStats() {
        const int64_t now = monoNowNs();
        if (lastLogNs == 0) lastLogNs = now;
        if (now - lastLogNs < kStatsLogPeriodNs) return;
        lastLogNs = now;
        EFG_PRS_LOGD("presenter: path=%d shown=%llu late=%llu epoch=%llu other=%llu vsync=%lld err=%lld epochNow=%u",
                     static_cast<int>(currentPath()), static_cast<unsigned long long>(shown),
                     static_cast<unsigned long long>(lateDrops), static_cast<unsigned long long>(epochDrops),
                     static_cast<unsigned long long>(otherDrops), static_cast<long long>(vsyncNs),
                     static_cast<long long>(lastErrNs), ring->epoch.load(std::memory_order_relaxed));
    }

    // false -> the thread must exit (device lost).
    bool handleFrame(const OutFrame& f) {
        if (f.epoch != ring->epoch.load(std::memory_order_acquire)) {  // I4
            ++epochDrops;
            giveBack(f, true, 0);
            return true;
        }
        if (f.slot >= ring->slotCount || ring->slots[f.slot].image == VK_NULL_HANDLE) {
            ++otherDrops;
            giveBack(f, true, 0);
            return true;
        }
        refreshChoreoVsync();  // REV 2: live period without display_timing (no-op with it)
        const int64_t now = monoNowNs();
        if (f.kind == kOutKindGenerated && isLate(f.presentAtNs, now, vsyncNs)) {
            ++lateDrops;
            giveBack(f, true, 0);
            return true;
        }
        const int64_t target = alignToVsync(f.presentAtNs, lastPostNs, vsyncNs, kVsyncSlackNs);

        Outcome o;
        switch (currentPath()) {
            case PresentPath::DisplayTiming:
                pollSurfaceExtent();
                o = presentSwapchain(f, target);
                break;
            case PresentPath::SleepVsync:
                pollSurfaceExtent();  // m9: before the sleep, not between wake-up and acquire
                sleepUntil(target);
                if (stopReq.load(std::memory_order_relaxed)) {
                    ++otherDrops;
                    giveBack(f, true, 0);
                    return true;
                }
                if (f.epoch != ring->epoch.load(std::memory_order_acquire)) {  // flushed while sleeping
                    ++epochDrops;
                    giveBack(f, true, 0);
                    return true;
                }
                o = presentSwapchain(f, target);
                break;
            case PresentPath::Cpu:
                o = presentCpu(f, target);
                break;
            default:
                giveBack(f, true, 0);
                return true;
        }
        switch (o) {
            case Outcome::Shown:
                ++shown;
                return true;
            case Outcome::Dropped:
                ++otherDrops;
                return true;
            case Outcome::Fallback:
                ++otherDrops;
                switchToCpu();
                return true;
            case Outcome::DeviceLost:
                EFG_PRS_LOGE("VK_ERROR_DEVICE_LOST: presenter thread exits");
                return false;
        }
        return true;
    }

    void threadMain() {
        while (!stopReq.load(std::memory_order_relaxed)) {
            maybeLogStats();
            OutFrame f{};
            if (!ring->queue.pop(f)) {
                sleepAbsNs(monoNowNs() + kIdlePollNs);
                continue;
            }
            if (!handleFrame(f)) break;
        }
    }

    // I3 order. Safe on a partially started presenter; idempotent.
    void teardown() {
        unsubscribeChoreo();  // presenter thread already joined; before the swapchain goes (I3)
        waitIdle();
        destroySwapchain();
        destroySurface();
        if (window != nullptr) {
            if (cpuTainted) EFG_PRS_LOGI("releasing a window bound to a CPU producer (R4b)");
            ANativeWindow_release(window);
            window = nullptr;
        }
        destroyReadback();
        destroyCommandObjects();
        // Pending frames go back as dropped; the slots stay owned by the ring.
        if (ring != nullptr) {
            OutFrame f{};
            while (ring->queue.pop(f)) giveBack(f, true, 0);
        }
        setPath(PresentPath::None);
    }

    void detach() {
        dev = nullptr;
        ring = nullptr;
        freeSlots = nullptr;
    }

    // start() failure: everything released (the window too), code passed on.
    int32_t failStart(int32_t rc) {
        teardown();
        detach();
        return rc;
    }
};

FgPresenter::FgPresenter() : impl_(std::make_unique<Impl>()) {}

FgPresenter::~FgPresenter() {
    stop();
}

int32_t FgPresenter::start(FgDevice& dev, ANativeWindow* window, uint32_t w, uint32_t h, PresentRing& ring,
                           FreeSlotQueue& freeSlots) {
    Impl& m = *impl_;
    if (m.thread.joinable() || m.window != nullptr) {
        // Already running: the new window is still ours to release.
        if (window != nullptr) ANativeWindow_release(window);
        return FgResult::BUSY;
    }
    m.resetState();
    m.dev = &dev;
    m.window = window;  // ownership passes here, also on failure
    m.outW = w;
    m.outH = h;
    m.ring = &ring;
    m.freeSlots = &freeSlots;

    if (window == nullptr) return m.failStart(FgResult::SURFACE);
    const auto& fn = dev.fn;
    const auto& ifn = dev.ifn;
    if (dev.device == VK_NULL_HANDLE || dev.queue == VK_NULL_HANDLE || dev.instance == VK_NULL_HANDLE ||
        fn.vkCreateSwapchainKHR == nullptr || fn.vkAcquireNextImageKHR == nullptr ||
        fn.vkGetSwapchainImagesKHR == nullptr || fn.vkDestroySwapchainKHR == nullptr ||
        fn.vkDeviceWaitIdle == nullptr || ifn.createAndroidSurfaceKHR == nullptr ||
        ifn.destroySurfaceKHR == nullptr || ifn.getPhysicalDeviceSurfaceSupportKHR == nullptr ||
        ifn.getPhysicalDeviceSurfaceCapabilitiesKHR == nullptr ||
        ifn.getPhysicalDeviceSurfaceFormatsKHR == nullptr || ifn.getPhysicalDeviceMemoryProperties == nullptr) {
        EFG_PRS_LOGE("presenter: device or WSI entry points missing");
        return m.failStart(FgResult::VK_INIT);
    }
    if (!m.createCommandObjects()) {
        EFG_PRS_LOGE("presenter: command pool/buffer/fence creation failed");
        return m.failStart(FgResult::VK_INIT);
    }
    m.useTiming = dev.caps.displayTiming && fn.vkGetPastPresentationTimingGOOGLE != nullptr &&
                  fn.vkGetRefreshCycleDurationGOOGLE != nullptr;
    int32_t rc = m.createSurface();
    if (rc == FgResult::OK) rc = m.createSwapchain();
    if (rc != FgResult::OK) return m.failStart(rc);
    if (!m.useTiming) m.subscribeChoreo();  // REV 2: AChoreographer period, else 60 Hz
    m.setPath(m.useTiming ? PresentPath::DisplayTiming : PresentPath::SleepVsync);
    try {
        m.thread = std::thread([&m] { m.threadMain(); });
    } catch (const std::system_error&) {
        EFG_PRS_LOGE("presenter: thread creation failed");
        return m.failStart(FgResult::VK_INIT);
    }
    EFG_PRS_LOGI("presenter started: out=%ux%u path=%d", w, h, static_cast<int>(m.currentPath()));
    return FgResult::OK;
}

void FgPresenter::stop() {
    Impl& m = *impl_;
    m.stopReq.store(true, std::memory_order_relaxed);
    if (m.thread.joinable()) m.thread.join();
    if (m.dev == nullptr) return;  // never started, or already stopped
    // vkDeviceWaitIdle -> swapchain -> VkSurfaceKHR -> ANativeWindow_release (I3),
    // then own objects and the pending frames: see Impl::teardown().
    m.teardown();
    m.detach();
}

PresentPath FgPresenter::path() const {
    return static_cast<PresentPath>(impl_->path.load(std::memory_order_relaxed));
}

}  // namespace efg
