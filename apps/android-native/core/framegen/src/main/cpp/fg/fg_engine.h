// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Ported from Enigmata (app/src/main/cpp/fg) to Tatakai Next 0.4.0 (core/framegen): JNI class, library name,
// log tags and debug properties renamed; no other change.
//
// Enigmata frame generation (R13): engine thread. Imports each input AHB,
// copies it into the in0/in1 slot (ping-pong with QUEUE_FAMILY_EXTERNAL
// barriers), runs LSFG presentContext + waitIdle (createContextFromAHB with
// inSem = -1) and copies the real frame and the N-1 generated ones into the
// presenter ring. Every LSFG_* call happens on this single thread, including
// initialize/finalize for updateConfig and deleteContext/finalize on stop().
// Anti-artifacts: 8x8 luma difference between consecutive real frames.
#pragma once

#include "fg_device.h"
#include "fg_presenter.h"
#include "fg_shader_loader.h"
#include "fg_types.h"
#include "frame_source.h"

#include <cstdint>
#include <memory>

namespace efg {

class FgEngine {
public:
    FgEngine();
    ~FgEngine();  // calls stop()

    FgEngine(const FgEngine&) = delete;
    FgEngine& operator=(const FgEngine&) = delete;

    // Starts the engine thread: allocates the ring slots, LSFG initialize with
    // dev.lsfgUuid and `loader`, then consumes `src`. Returns FgResult::OK,
    // BUSY, INVALID_ARG, SHADERS_MISSING, LSFG_INIT or CONTEXT.
    int32_t start(FgDevice& dev, FrameSource& src, PresentRing& ring, FreeSlotQueue& freeSlots,
                  const FgConfigN& cfg, ShaderLoader loader);

    // Queued; the engine thread applies it with finalize + initialize.
    // Returns FgResult::OK, NOT_INITIALIZED or INVALID_ARG.
    int32_t updateConfig(const FgConfigN& cfg);

    // Source period hint from Kotlin (container fps); 0 = estimate only.
    void setSourcePeriodNs(int64_t ns);

    // Seek or discontinuity (I4): drains the queues, releases the AImages,
    // bumps the epoch and reseeds the two input slots keeping the
    // presentsDone % 2 parity.
    void flush();

    // Paused (I5): no generation; the last real frame stays on screen.
    void setPaused(bool paused);

    // Joins the engine thread after deleteContext + finalize on it (I3).
    // Idempotent. Any error or DEVICE_LOST leaves state = Failed (I9).
    void stop();

    // Copies the FgStats counters (index 7, presentPath, is filled by the
    // caller from FgPresenter::path()).
    void stats(FgStatsArray& out) const;

private:
    struct Impl;
    std::unique_ptr<Impl> impl_;
};

}  // namespace efg
