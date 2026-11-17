// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Ported from Enigmata (app/src/main/cpp/fg) to Tatakai Next 0.4.0 (core/framegen): JNI class, library name,
// log tags and debug properties renamed; no other change.
//
// Enigmata frame generation (R13): shared native types.
#pragma once

#include <android/hardware_buffer.h>
#include <media/NdkImage.h>
#include <unistd.h>

#include <array>
#include <atomic>
#include <cstddef>
#include <cstdint>
#include <string>
#include <type_traits>
#include <utility>

namespace efg {

// Result codes. Mirror of the Kotlin enum app.tatakai.next.framegen.FgResult; the values
// cross JNI unchanged and must stay in sync with it.
namespace FgResult {
inline constexpr int32_t OK = 0;
inline constexpr int32_t NOT_INITIALIZED = -1;
inline constexpr int32_t VK_INIT = -2;
inline constexpr int32_t MISSING_EXTENSION = -3;
inline constexpr int32_t SHADERS_MISSING = -4;
inline constexpr int32_t LSFG_INIT = -5;
inline constexpr int32_t CONTEXT = -6;
inline constexpr int32_t READER = -7;
inline constexpr int32_t SURFACE = -8;
inline constexpr int32_t DEVICE_LOST = -9;
inline constexpr int32_t BUSY = -10;
inline constexpr int32_t INVALID_ARG = -11;
inline constexpr int32_t DLL_UNREADABLE = -20;
inline constexpr int32_t MISSING_SPIRV = -21;
inline constexpr int32_t BINDING_REWRITE = -22;
inline constexpr int32_t WRITE_FAILED = -23;
}  // namespace FgResult

// Multiplier range (fact 3: 2x..5x; Auto is resolved in Kotlin).
inline constexpr int32_t kMinMultiplier = 2;
inline constexpr int32_t kMaxMultiplier = 5;

// One frame acquired from the AImageReader. `buffer` is borrowed from `image`
// (AImage_getHardwareBuffer) and is valid only while `image` is alive.
struct InFrame {
    AImage* image = nullptr;
    AHardwareBuffer* buffer = nullptr;
    int fenceFd = -1;   // acquire fence (sync_fd) or -1
    int64_t tsNs = 0;   // AImage timestamp
    uint64_t seq = 0;   // monotonic acquire counter
};

// RAII owner of an InFrame (I2): AImage_delete for the image and close() for
// the fence on every path. Move-only.
class InFrameHandle {
public:
    InFrameHandle() = default;
    explicit InFrameHandle(const InFrame& frame) : frame_(frame) {}
    ~InFrameHandle() { reset(); }

    InFrameHandle(const InFrameHandle&) = delete;
    InFrameHandle& operator=(const InFrameHandle&) = delete;
    InFrameHandle(InFrameHandle&& other) noexcept : frame_(other.detach()) {}
    InFrameHandle& operator=(InFrameHandle&& other) noexcept {
        if (this != &other) {
            reset();
            frame_ = other.detach();
        }
        return *this;
    }

    const InFrame& get() const { return frame_; }
    InFrame& get() { return frame_; }
    explicit operator bool() const { return frame_.image != nullptr; }

    // Gives up ownership without releasing anything.
    InFrame detach() {
        InFrame out = frame_;
        frame_ = InFrame{};
        return out;
    }

    // Releases the image and the fence; idempotent.
    void reset() {
        if (frame_.fenceFd >= 0) {
            ::close(frame_.fenceFd);
        }
        if (frame_.image != nullptr) {
            AImage_delete(frame_.image);
        }
        frame_ = InFrame{};
    }

private:
    InFrame frame_{};
};

// Kind of an OutFrame.
inline constexpr uint8_t kOutKindReal = 0;
inline constexpr uint8_t kOutKindGenerated = 1;

// Engine -> presenter: a presenter slot ready to show at presentAtNs.
// The presenter drops frames whose epoch is older than the current one (I4).
struct OutFrame {
    uint32_t slot;
    int64_t presentAtNs;   // CLOCK_MONOTONIC
    uint64_t seq;
    uint8_t kind;          // kOutKindReal / kOutKindGenerated
    uint8_t index;         // 0..N-2 for generated frames, N-1 for the real one
    uint32_t epoch;
};

// Single-producer single-consumer lock-free ring. N must be a power of two;
// capacity is N - 1. push() only from the producer thread, pop() only from the
// consumer thread. clear() is a consumer-side drain.
template <typename T, size_t N>
class SpscRing {
    static_assert(N >= 2 && (N & (N - 1)) == 0, "SpscRing size must be a power of two");
    static_assert(std::is_nothrow_move_assignable_v<T>, "SpscRing element must be nothrow movable");

public:
    static constexpr size_t kCapacity = N - 1;

    bool push(const T& value) {
        const size_t head = head_.load(std::memory_order_relaxed);
        const size_t next = (head + 1) & (N - 1);
        if (next == tail_.load(std::memory_order_acquire)) return false;
        items_[head] = value;
        head_.store(next, std::memory_order_release);
        return true;
    }

    bool push(T&& value) {
        const size_t head = head_.load(std::memory_order_relaxed);
        const size_t next = (head + 1) & (N - 1);
        if (next == tail_.load(std::memory_order_acquire)) return false;
        items_[head] = std::move(value);
        head_.store(next, std::memory_order_release);
        return true;
    }

    bool pop(T& out) {
        const size_t tail = tail_.load(std::memory_order_relaxed);
        if (tail == head_.load(std::memory_order_acquire)) return false;
        out = std::move(items_[tail]);
        tail_.store((tail + 1) & (N - 1), std::memory_order_release);
        return true;
    }

    // Approximate size; exact only when called from one of the two sides
    // while the other is idle.
    size_t size() const {
        const size_t head = head_.load(std::memory_order_acquire);
        const size_t tail = tail_.load(std::memory_order_acquire);
        return (head - tail) & (N - 1);
    }

    bool empty() const { return size() == 0; }

    void clear() {
        T sink{};
        while (pop(sink)) {
        }
    }

private:
    std::array<T, N> items_{};
    alignas(64) std::atomic<size_t> head_{0};
    alignas(64) std::atomic<size_t> tail_{0};
};

// Native mirror of the Kotlin FgConfig (same fields, same order).
struct FgConfigN {
    int32_t multiplier = kMinMultiplier;
    bool performanceMode = true;
    float flowScale = 0.5f;
    int32_t inputWidth = 0;
    int32_t inputHeight = 0;
    int32_t outputWidth = 0;
    int32_t outputHeight = 0;
    bool allowFp16 = false;
    bool antiArtifacts = false;
    bool verbose = false;
    std::string shaderDir;
};

// Pipeline state (FgStats index 8). Failed is terminal for the current file (I9).
enum class FgStatus : int32_t {
    Stopped = 0,
    Running = 1,
    Paused = 2,
    Failed = 3,
};

// Presentation path (FgStats index 7).
enum class PresentPath : int32_t {
    None = 0,
    DisplayTiming = 1,
    SleepVsync = 2,
    Cpu = 3,
};

// Fixed FgStats indices (Kotlin FgStats.from(LongArray)).
enum FgStatIndex : size_t {
    kStatInFrames = 0,
    kStatOutFrames = 1,
    kStatGenerated = 2,
    kStatDroppedLate = 3,
    kStatLatencyNs = 4,
    kStatComputeNsAvg = 5,
    kStatPeriodNs = 6,
    kStatPresentPath = 7,
    kStatState = 8,
    kStatLastError = 9,
    kStatInW = 10,
    kStatInH = 11,
    kStatCount = 12,
};

using FgStatsArray = std::array<int64_t, kStatCount>;

// Live counters written by the pipeline threads and read by JNI (stats()).
struct FgStatsSnapshot {
    std::array<std::atomic<int64_t>, kStatCount> values{};

    void set(FgStatIndex index, int64_t value) {
        values[index].store(value, std::memory_order_relaxed);
    }
    void add(FgStatIndex index, int64_t delta) {
        values[index].fetch_add(delta, std::memory_order_relaxed);
    }
    int64_t get(FgStatIndex index) const {
        return values[index].load(std::memory_order_relaxed);
    }
    void copyTo(FgStatsArray& out) const {
        for (size_t i = 0; i < kStatCount; ++i) {
            out[i] = values[i].load(std::memory_order_relaxed);
        }
    }
    void reset() {
        for (auto& v : values) v.store(0, std::memory_order_relaxed);
    }
};

}  // namespace efg
