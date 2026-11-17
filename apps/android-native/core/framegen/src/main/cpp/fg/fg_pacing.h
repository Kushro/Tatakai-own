// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Ported from Enigmata (app/src/main/cpp/fg) to Tatakai Next 0.4.0 (core/framegen): JNI class, library name,
// log tags and debug properties renamed; no other change.
//
// Enigmata frame generation (R13): pure pacing math. No Vulkan or Android
// dependencies; every time is in nanoseconds on CLOCK_MONOTONIC.
#pragma once

#include <cstdint>

namespace efg {

// Estimates the source frame period from AImage timestamps: EMA with outlier
// rejection. A timestamp that goes backwards, or a forward jump of 1 s or
// more, is a discontinuity (the caller flushes, I4). A forward jump above
// twice the period and below 1 s is lost frames, not a discontinuity: the
// real frame is shown alone (REV 15).
class PeriodEstimator {
public:
    void reset();
    // Feeds one timestamp. Discontinuities restart the estimate from tsNs.
    void update(int64_t tsNs);
    // Current estimate; 0 while unknown.
    int64_t periodNs() const;
    bool isDiscontinuity(int64_t tsNs) const;

private:
    int64_t lastTsNs_ = -1;
    int64_t periodNs_ = 0;
    uint32_t samples_ = 0;
};

// Pipeline latency L added to every real frame: one source period plus the
// compute time plus a safety margin.
int64_t latencyNs(int64_t periodNs, int64_t computeNs, int64_t marginNs);

// Present times for one generation step: out[0..n-2] are the generated frames
// and out[n-1] the real one, evenly spread over one period after tPrevNs + L.
// `out` must hold n entries.
void schedule(int64_t tPrevNs, int64_t latencyNs, int64_t periodNs, int n, int64_t out[]);

// True when a frame due at atNs can no longer make its vsync.
bool isLate(int64_t atNs, int64_t nowNs, int64_t vsyncNs);

// Moves targetNs so that it does not land in the same vsync slot as lastPostNs
// (port of lsfg_render_loop.cpp:483-516). Returns the adjusted target; with
// vsyncNs <= 0 the target is returned unchanged.
int64_t alignToVsync(int64_t targetNs, int64_t lastPostNs, int64_t vsyncNs, int64_t slackNs);

}  // namespace efg
