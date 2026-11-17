// SPDX-License-Identifier: GPL-3.0-only
//
// Ported from Enigmata (app/src/main/cpp/fg) to Tatakai Next 0.4.0 (core/framegen): JNI class, library name,
// log tags and debug properties renamed; no other change.
//
// Adapted from FrankBarretta/LSFG-Android-Application
// (commit b84754199823615d32d65fe33ea59481dca88dcf), app/src/main/cpp/lsfg_render_loop.cpp
// (sleepUntilVsyncAligned :483-516; EMA with outlier rejection :2046-2083), GPL-3.0.
// Modifications: Enigmata, AGPL-3.0-or-later (combined under GPL-3.0 section 13).
//
// Pure pacing math (unit W3a). All times are int64 nanoseconds on
// CLOCK_MONOTONIC; no Vulkan, Android or clock calls here, so every function is
// deterministic and testable off-device.

#include "fg_pacing.h"

#include <algorithm>
#include <cstdint>
#include <limits>

namespace efg {

namespace {

constexpr int64_t kI64Max = std::numeric_limits<int64_t>::max();
constexpr int64_t kI64Min = std::numeric_limits<int64_t>::min();

// EMA smoothing factor alpha = 1 / kEmaAlphaDen (= 1/8, the molde's default:
// TCP-RTT style, reacts in ~8 frames and smooths ordinary jitter).
constexpr int64_t kEmaAlphaDen = 8;

// An interval is accepted into the EMA only inside
// [period / kOutlierRatio, period * kOutlierRatio] = [0.5 period, 2 period].
// Outside it the interval is an outlier: ignored by the EMA, but it counts
// towards the regime change below. A forward jump above 2 period and below
// kDiscontinuityNs (e.g. one or a few lost frames) is such an outlier, NOT a
// discontinuity (Enmienda 1, REV 15).
constexpr int64_t kOutlierRatio = 2;

// Enmienda 1 (REV 15): a timestamp is a discontinuity (the caller flushes, I4)
// only when it goes backwards or jumps forward by >= 1 s. This replaces the
// previous "> 2 period" rule; the fg_pacing.h signature is unchanged.
constexpr int64_t kDiscontinuityNs = 1'000'000'000;

// Consecutive rejected intervals after which the source is considered to have
// changed regime (e.g. 30 -> 120 fps) and the EMA is re-seeded with the new
// interval (molde: "persistent outliers (>=3 in a row) ... reset").
constexpr uint32_t kRegimeOutliers = 3;

// samples_ packing (the header's private layout is fixed): low 24 bits are the
// accepted interval count (saturating), high 8 bits the consecutive-outlier run.
constexpr uint32_t kCountMask = 0x00FFFFFFu;
constexpr uint32_t kOutlierShift = 24;

int64_t satAdd(int64_t a, int64_t b) {
    if (b > 0 && a > kI64Max - b) return kI64Max;
    if (b < 0 && a < kI64Min - b) return kI64Min;
    return a + b;
}

int64_t satSub(int64_t a, int64_t b) {
    if (b == kI64Min) return a >= 0 ? kI64Max : a - b;
    return satAdd(a, -b);
}

}  // namespace

// ---------------------------------------------------------------------------
// PeriodEstimator
// ---------------------------------------------------------------------------

void PeriodEstimator::reset() {
    lastTsNs_ = -1;
    periodNs_ = 0;
    samples_ = 0;
}

void PeriodEstimator::update(int64_t tsNs) {
    if (lastTsNs_ < 0) {
        // First timestamp (or after reset): nothing to measure yet.
        lastTsNs_ = tsNs;
        return;
    }
    if (isDiscontinuity(tsNs)) {
        // Backwards or >= 1 s jump (REV 15): restart the estimate from tsNs.
        lastTsNs_ = tsNs;
        periodNs_ = 0;
        samples_ = 0;
        return;
    }
    const int64_t interval = tsNs - lastTsNs_;  // >= 0: backwards is a discontinuity
    lastTsNs_ = tsNs;
    if (interval <= 0) return;  // duplicate timestamp: carries no period information

    uint32_t count = samples_ & kCountMask;
    uint32_t outliers = samples_ >> kOutlierShift;

    if (periodNs_ <= 0) {
        // First valid interval initializes the EMA.
        periodNs_ = interval;
        count = 1;
        outliers = 0;
    } else {
        // Both bounds reject: since REV 15 a forward jump > 2 period and < 1 s
        // is an outlier here (the estimate is kept), not a discontinuity.
        const bool outlier = interval * kOutlierRatio < periodNs_ || interval > periodNs_ * kOutlierRatio;
        if (outlier) {
            ++outliers;
            if (outliers >= kRegimeOutliers) {
                periodNs_ = interval;  // new regime: re-seed
                count = 1;
                outliers = 0;
            }
        } else {
            periodNs_ += (interval - periodNs_) / kEmaAlphaDen;
            if (periodNs_ <= 0) periodNs_ = 1;
            if (count < kCountMask) ++count;
            outliers = 0;
        }
    }
    samples_ = (count & kCountMask) | (std::min(outliers, 0xFFu) << kOutlierShift);
}

int64_t PeriodEstimator::periodNs() const {
    return periodNs_;
}

// REV 15: backwards, or a forward jump of >= kDiscontinuityNs (1 s). Shorter
// forward jumps (> 2 period included) are not discontinuities; the engine shows
// the real frame without generating that interval and keeps the epoch.
bool PeriodEstimator::isDiscontinuity(int64_t tsNs) const {
    if (lastTsNs_ < 0) return false;    // no previous timestamp
    if (tsNs < lastTsNs_) return true;  // goes backwards
    return satSub(tsNs, lastTsNs_) >= kDiscontinuityNs;  // jump >= 1 s
}

// ---------------------------------------------------------------------------
// Free functions
// ---------------------------------------------------------------------------

// L = period + margin + compute; negative inputs count as 0, the sum saturates.
int64_t latencyNs(int64_t periodNs, int64_t computeNs, int64_t marginNs) {
    int64_t l = std::max<int64_t>(periodNs, 0);
    l = satAdd(l, std::max<int64_t>(marginNs, 0));
    l = satAdd(l, std::max<int64_t>(computeNs, 0));
    return l;
}

// out[i] = tPrev + L + (i + 1) * period / n, i = 0..n-1: out[0..n-2] are the
// generated frames, out[n-1] = tPrev + L + period is the real one. The engine
// passes tPrev = ts - period, so the real frame lands on ts + L and the
// generated ones evenly spread over the period before it (plan: real k-1 at
// T_{k-1} + L, generated i at T_{k-1} + L + i * period / N). n < 1 writes
// nothing; a negative period counts as 0 (every entry at tPrev + L).
void schedule(int64_t tPrevNs, int64_t latencyNs, int64_t periodNs, int n, int64_t out[]) {
    if (n < 1 || out == nullptr) return;
    const int64_t base = satAdd(tPrevNs, std::max<int64_t>(latencyNs, 0));
    const int64_t period = std::max<int64_t>(periodNs, 0);
    const int64_t step = period / n;
    const int64_t rem = period % n;
    for (int i = 0; i < n; ++i) {
        const int64_t k = static_cast<int64_t>(i) + 1;
        // (k * period) / n without overflowing k * period.
        const int64_t offset = satAdd(step > 0 && k > kI64Max / step ? kI64Max : k * step, (k * rem) / n);
        out[i] = satAdd(base, offset);
    }
}

// A frame due at atNs is late once more than half a vsync has passed since
// then: it would land on (at least) the next refresh. Without a known vsync
// (vsyncNs <= 0) it is late as soon as atNs is in the past.
bool isLate(int64_t atNs, int64_t nowNs, int64_t vsyncNs) {
    if (vsyncNs <= 0) return atNs < nowNs;
    return atNs < satSub(nowNs, vsyncNs / 2);
}

// Port of sleepUntilVsyncAligned without the sleep: the earliest allowed time
// is one vsync (minus slack) after the last post, so two buffers never collapse
// onto one flip. Returns max(target, lastPost + vsync - slack). Unchanged when
// vsyncNs <= 0 or lastPostNs < 0 (no post yet). The slack is clamped to
// [0, vsync]; the molde's slotBudget guard has no equivalent in this signature.
int64_t alignToVsync(int64_t targetNs, int64_t lastPostNs, int64_t vsyncNs, int64_t slackNs) {
    if (vsyncNs <= 0 || lastPostNs < 0) return targetNs;
    const int64_t slack = std::clamp<int64_t>(slackNs, 0, vsyncNs);
    const int64_t earliest = satAdd(lastPostNs, vsyncNs - slack);
    return std::max(targetNs, earliest);
}

}  // namespace efg
