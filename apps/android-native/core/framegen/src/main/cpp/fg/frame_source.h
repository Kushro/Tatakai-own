// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Ported from Enigmata (app/src/main/cpp/fg) to Tatakai Next 0.4.0 (core/framegen): JNI class, library name,
// log tags and debug properties renamed; no other change.
//
// Enigmata frame generation (R13): input frame source. mpv renders into the
// window of this source; the engine acquires the frames as AHardwareBuffers.
// Implemented by image_reader_source.cpp on top of AImageReader:
// AImageReader_newWithUsage(RGBX_8888, GPU_COLOR_OUTPUT | GPU_SAMPLED_IMAGE |
// CPU_READ_RARELY, maxImages); the CPU bit only keeps the buffers linear (no
// UBWC) and falls back to GPU-only usage if refused. A buffer-removed listener
// invalidates the AHB -> VkImage cache.
#pragma once

#include "fg_types.h"

#include <android/hardware_buffer.h>
#include <android/native_window.h>

#include <cstdint>
#include <functional>
#include <memory>

namespace efg {

// Callbacks run on the AImageReader listener thread; keep them short.
struct FrameSourceListener {
    std::function<void()> onImageAvailable;
    // The reader dropped this buffer: forget any VkImage imported from it.
    std::function<void(AHardwareBuffer*)> onBufferRemoved;
};

class FrameSource {
public:
    FrameSource();
    ~FrameSource();  // calls close()

    FrameSource(const FrameSource&) = delete;
    FrameSource& operator=(const FrameSource&) = delete;

    // Creates the reader. Returns FgResult::OK or FgResult::READER.
    int32_t open(int32_t width, int32_t height, int32_t maxImages);

    // Producer window for mpv (owned by the reader, valid until close()).
    ANativeWindow* window() const;

    // Installs the listener; an empty FrameSourceListener detaches it
    // (first step of stop(), I3).
    void setListener(FrameSourceListener fn);

    // Acquires the next image, waiting up to timeoutMs for it. On OK `out`
    // owns an AImage and a fence that must go back through release() (or an
    // InFrameHandle). Returns FgResult::OK, FgResult::BUSY (none available) or
    // FgResult::READER.
    int32_t acquire(InFrame& out, int32_t timeoutMs);

    // AImage_delete + close(fenceFd); leaves `frame` empty. Safe on an empty frame.
    void release(InFrame& frame);

    // Listener to null, pending AImages, then the AImageReader (I2, I3).
    // Idempotent; window() is invalid afterwards.
    void close();

private:
    struct Impl;
    std::unique_ptr<Impl> impl_;
};

}  // namespace efg
