// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Ported from Enigmata (app/src/main/cpp/fg) to Tatakai Next 0.4.0 (core/framegen): JNI class, library name,
// log tags and debug properties renamed; no other change.
//
// Enigmata frame generation (R13): presenter thread. FIFO swapchain on the
// SurfaceView's ANativeWindow. With VK_GOOGLE_display_timing it sets
// desiredPresentTime and reports the measured error back to the engine;
// otherwise it sleeps until the vsync-aligned target. If vkQueuePresentKHR
// fails it falls back to a CPU blit through ANativeWindow_lock.
#pragma once

#include "ahb_image.h"
#include "fg_device.h"
#include "fg_types.h"

#include <android/native_window.h>

#include <array>
#include <atomic>
#include <cstdint>
#include <memory>

namespace efg {

// At least 2N presenter slots (decision 3): LSFG rewrites its outN images on
// every present, so real and generated frames are copied into these slots first.
inline constexpr uint32_t kMaxPresentSlots = 2 * static_cast<uint32_t>(kMaxMultiplier);
inline constexpr size_t kPresentQueueSize = 16;  // power of two > kMaxPresentSlots

// Presenter -> engine: a slot is free again (shown or dropped).
struct SlotReturn {
    uint32_t slot = 0;
    uint32_t epoch = 0;
    uint8_t dropped = 0;         // 1 = discarded as late or from an old epoch
    uint8_t kind = kOutKindReal; // OutFrame::kind of the returned frame (E21)
    int64_t presentErrorNs = 0;  // actual - desired present time (display_timing), 0 if unknown
};

using OutFrameQueue = SpscRing<OutFrame, kPresentQueueSize>;
using FreeSlotQueue = SpscRing<SlotReturn, kPresentQueueSize>;

// Presenter ring: slot images (input size, on our device) plus the
// engine -> presenter queue. FgEngine::start allocates the slot images; the
// owner destroys the ring only after the presenter has stopped (I3).
struct PresentRing {
    std::array<AhbImage, kMaxPresentSlots> slots{};
    uint32_t slotCount = 0;
    std::atomic<uint32_t> epoch{0};  // current epoch; older OutFrames are dropped (I4)
    OutFrameQueue queue;
};

class FgPresenter {
public:
    FgPresenter();
    ~FgPresenter();  // calls stop()

    FgPresenter(const FgPresenter&) = delete;
    FgPresenter& operator=(const FgPresenter&) = delete;

    // Starts the presenter thread. `window` was acquired by the caller
    // (ANativeWindow_fromSurface) and ownership passes to the presenter, which
    // releases it in stop() (also when start fails). w x h is the output size.
    // Returns FgResult::OK, SURFACE, VK_INIT or BUSY (already running).
    int32_t start(FgDevice& dev, ANativeWindow* window, uint32_t w, uint32_t h,
                  PresentRing& ring, FreeSlotQueue& freeSlots);

    // Joins the thread: vkDeviceWaitIdle, swapchain, VkSurfaceKHR,
    // ANativeWindow_release (I3). Idempotent.
    void stop();

    // Current presentation path (FgStats index 7).
    PresentPath path() const;

private:
    struct Impl;
    std::unique_ptr<Impl> impl_;
};

}  // namespace efg
