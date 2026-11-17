// SPDX-License-Identifier: GPL-3.0-only
//
// Ported from Enigmata (app/src/main/cpp/fg) to Tatakai Next 0.4.0 (core/framegen): JNI class, library name,
// log tags and debug properties renamed; no other change.
//
// Adapted from FrankBarretta/LSFG-Android-Application
// (commit b84754199823615d32d65fe33ea59481dca88dcf), app/src/main/cpp/lsfg_render_loop.cpp, GPL-3.0
// (dlsym loading of AHardwareBuffer_getId with fallback, getAhbId :40-80).
// Modifications: Enigmata, AGPL-3.0-or-later (combined under GPL-3.0 section 13).
//
// Enigmata frame generation (R13): FrameSource on top of AImageReader (API 26).
//
// Semantics (frame_source.h is the contract):
// - open(): AImageReader_newWithUsage(RGBX_8888, GPU_COLOR_OUTPUT |
//   GPU_SAMPLED_IMAGE | CPU_READ_RARELY, maxImages) + image and buffer-removed
//   listeners. The CPU usage bit is never exercised (the buffers are not
//   locked); it makes gralloc allocate the reader buffers linear instead of
//   UBWC-compressed on Qualcomm. The GLES-written UBWC buffer decoded wrongly
//   when imported into Vulkan on Adreno 740 (FG-F, reports fg-d / fg-d-fable),
//   and it was the only compressed buffer on the FG path (our own AHBs carry
//   no UBWC usage). Applied on every device. If the reader cannot be created
//   with it, open() retries with the GPU-only usage and logs that once per
//   process. The system property debug.tatakai.fg.reader_linear ("" or 1 =
//   linear, the default; 0 = GPU-only usage as before) is read on every open()
//   for on-device A/B tests; the effective mode and its source are logged.
//   RGBX_8888 because mpv's vo_gpu creates its EGL surface without alpha: an
//   RGBA_8888 reader rejected every buffer on device ("acquireImageLocked:
//   Output buffer format: 0x2, ImageReader configured format: 0x1"). The
//   driver maps R8G8B8X8_UNORM to VK_FORMAT_R8G8B8A8_UNORM on import; if it
//   reports UNDEFINED instead, ahb_image.cpp refuses the buffer (no hint);
//   alpha is unused on the FG path (the presenter is opaque). The producer
//   can still queue another format: on Adreno 740 mpv's 10-bit EGL config
//   gives RGBA_1010102 buffers (FG-F2); the engine blit-converts those.
//   Returns OK, READER (reader creation / window / listener failure) or
//   INVALID_ARG (width, height or maxImages <= 0). Reopening an open source
//   closes it first (close() then open); BUSY is never returned by open().
//   The window belongs to the reader and is never released here.
// - setListener(): the user listener is guarded by a mutex that is also held
//   while a callback runs, so once setListener({}) (or close()) returns no
//   callback can reach the previous listener. Callbacks must therefore not
//   call setListener()/close() on this source (self-deadlock).
// - acquire(): AImageReader_acquireNextImageAsync (fence fd, -1 if already
//   signalled), AImage_getHardwareBuffer, AImage_getTimestamp, seq = monotonic
//   counter (starts at 1, never reset). If no image is available it waits up
//   to timeoutMs for the reader's image-available signal (independent of the
//   user listener) and retries; timeoutMs <= 0 means a single non-blocking
//   try. No image within the timeout, or maxImages already acquired -> BUSY.
//   Source not open or any other reader error -> READER. On every non-OK
//   return `out` is empty and nothing is owned by the caller. READER errors
//   are logged only the first time per open() so a persistent failure does
//   not flood logcat at the frame rate; return codes are unaffected.
// - release(): via InFrameHandle (close(fenceFd) if >= 0, AImage_delete);
//   idempotent, safe on an empty frame.
// - close(): I3 order: reader listeners to null -> own listener dropped ->
//   AImageReader_delete. AImageReader_delete also tears down AImages it still
//   tracks, but the engine must have released (release()/InFrameHandle) every
//   InFrame it acquired before close() (I3: engine join precedes close).
//   Idempotent; also called by the destructor.

#include "frame_source.h"

#include <android/log.h>
#include <dlfcn.h>
#include <media/NdkImage.h>
#include <media/NdkImageReader.h>
#include <sys/system_properties.h>
#include <unistd.h>

#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstdint>
#include <cstring>
#include <mutex>
#include <utility>

namespace efg {

namespace {

constexpr const char* kTag = "efg-reader";
#define EFG_RDR_LOGE(...) __android_log_print(ANDROID_LOG_ERROR, kTag, __VA_ARGS__)
#define EFG_RDR_LOGW(...) __android_log_print(ANDROID_LOG_WARN, kTag, __VA_ARGS__)
#define EFG_RDR_LOGI(...) __android_log_print(ANDROID_LOG_INFO, kTag, __VA_ARGS__)
#define EFG_RDR_LOGD(...) __android_log_print(ANDROID_LOG_DEBUG, kTag, __VA_ARGS__)

// GPU-only usage (the reader usage before FG-F; reader_linear=0 and fallback).
constexpr uint64_t kReaderGpuUsage =
    AHARDWAREBUFFER_USAGE_GPU_COLOR_OUTPUT | AHARDWAREBUFFER_USAGE_GPU_SAMPLED_IMAGE;
// Linear reader buffers: a CPU usage bit rules out UBWC compression in
// Qualcomm's gralloc. Never locked on the CPU; costs only bandwidth.
constexpr uint64_t kReaderUsage = kReaderGpuUsage | AHARDWAREBUFFER_USAGE_CPU_READ_RARELY;

// A/B switch for on-device tests: "" (unset) or "1" = linear, "0" = GPU-only.
constexpr const char* kReaderLinearProp = "debug.tatakai.fg.reader_linear";

struct ReaderMode {
    bool linear;
    const char* source;  // for the log line only
};

ReaderMode readerModeFromProperty() {
    char value[PROP_VALUE_MAX] = {};
    if (__system_property_get(kReaderLinearProp, value) <= 0) return {true, "default"};
    if (std::strcmp(value, "0") == 0) return {false, "prop"};
    if (std::strcmp(value, "1") == 0) return {true, "prop"};
    return {true, "default (unrecognized prop value)"};
}

// The linear -> GPU-only fallback is logged once per process.
std::atomic<bool> linearFallbackLogged{false};

// AHardwareBuffer_getId is API 31; minSdk is 26, so it is resolved at runtime
// and its absence is tolerated (falls back to the pointer value). Used only
// for diagnostics: the AHB -> VkImage cache is keyed by the AHardwareBuffer*
// that onBufferRemoved reports.
using PfnAhbGetId = int (*)(const AHardwareBuffer*, uint64_t*);

PfnAhbGetId resolveAhbGetId() {
    static std::once_flag once;
    static PfnAhbGetId fn = nullptr;
    std::call_once(once, [] {
        fn = reinterpret_cast<PfnAhbGetId>(dlsym(RTLD_DEFAULT, "AHardwareBuffer_getId"));
    });
    return fn;
}

uint64_t ahbId(const AHardwareBuffer* ahb) {
    uint64_t id = 0;
    if (PfnAhbGetId fn = resolveAhbGetId(); fn != nullptr && fn(ahb, &id) == 0) {
        return id;
    }
    return static_cast<uint64_t>(reinterpret_cast<uintptr_t>(ahb));
}

}  // namespace

struct FrameSource::Impl {
    AImageReader* reader = nullptr;
    ANativeWindow* window = nullptr;  // owned by `reader`

    // User listener; held while a callback runs (see setListener()).
    std::mutex listenerMutex;
    FrameSourceListener listener;

    // Image-available signal for acquire() waits.
    std::mutex waitMutex;
    std::condition_variable waitCv;
    uint64_t imageSignals = 0;  // guarded by waitMutex

    uint64_t nextSeq = 1;  // acquire() thread only

    // acquire() READER errors are logged once per open(); reset by open().
    std::atomic<bool> readerErrorLogged{false};

    // True only for the first READER error since the last open().
    bool firstReaderError() { return !readerErrorLogged.exchange(true); }

    static void onImageAvailable(void* context, AImageReader* /*reader*/) {
        auto* self = static_cast<Impl*>(context);
        if (self == nullptr) return;
        {
            std::lock_guard<std::mutex> lock(self->waitMutex);
            ++self->imageSignals;
        }
        self->waitCv.notify_all();
        std::lock_guard<std::mutex> lock(self->listenerMutex);
        if (self->listener.onImageAvailable) self->listener.onImageAvailable();
    }

    static void onBufferRemoved(void* context, AImageReader* /*reader*/, AHardwareBuffer* buffer) {
        auto* self = static_cast<Impl*>(context);
        if (self == nullptr || buffer == nullptr) return;
        EFG_RDR_LOGD("buffer removed id=%llu", static_cast<unsigned long long>(ahbId(buffer)));
        std::lock_guard<std::mutex> lock(self->listenerMutex);
        if (self->listener.onBufferRemoved) self->listener.onBufferRemoved(buffer);
    }

    // I3 order: reader listeners to null, own listener dropped, reader deleted.
    void closeReader() {
        if (reader != nullptr) {
            AImageReader_setImageListener(reader, nullptr);
            AImageReader_setBufferRemovedListener(reader, nullptr);
        }
        {
            std::lock_guard<std::mutex> lock(listenerMutex);
            listener = FrameSourceListener{};
        }
        if (reader != nullptr) {
            // Also stops the reader's callback looper; callbacks queued before
            // the listeners were nulled may still run until here, and they find
            // an empty listener.
            AImageReader_delete(reader);
            reader = nullptr;
        }
        window = nullptr;
    }
};

FrameSource::FrameSource() : impl_(std::make_unique<Impl>()) {}

FrameSource::~FrameSource() {
    close();
}

int32_t FrameSource::open(int32_t width, int32_t height, int32_t maxImages) {
    if (width <= 0 || height <= 0 || maxImages <= 0) return FgResult::INVALID_ARG;
    if (impl_->reader != nullptr) close();  // reopen: close first (never BUSY)
    impl_->readerErrorLogged.store(false);

    ReaderMode mode = readerModeFromProperty();
    uint64_t usage = mode.linear ? kReaderUsage : kReaderGpuUsage;
    AImageReader* reader = nullptr;
    media_status_t st = AImageReader_newWithUsage(width, height, AIMAGE_FORMAT_RGBX_8888, usage,
                                                  maxImages, &reader);
    if ((st != AMEDIA_OK || reader == nullptr) && mode.linear) {
        if (!linearFallbackLogged.exchange(true)) {
            EFG_RDR_LOGW("AImageReader_newWithUsage %dx%d usage=0x%llx failed: %d; retrying GPU-only",
                         width, height, static_cast<unsigned long long>(usage), static_cast<int>(st));
        }
        if (reader != nullptr) AImageReader_delete(reader);
        reader = nullptr;
        mode = ReaderMode{false, "fallback"};
        usage = kReaderGpuUsage;
        st = AImageReader_newWithUsage(width, height, AIMAGE_FORMAT_RGBX_8888, usage, maxImages, &reader);
    }
    if (st != AMEDIA_OK || reader == nullptr) {
        EFG_RDR_LOGE("AImageReader_newWithUsage %dx%d max=%d usage=0x%llx failed: %d", width, height,
                     maxImages, static_cast<unsigned long long>(usage), static_cast<int>(st));
        if (reader != nullptr) AImageReader_delete(reader);
        return FgResult::READER;
    }
    impl_->reader = reader;

    ANativeWindow* window = nullptr;
    st = AImageReader_getWindow(reader, &window);
    if (st != AMEDIA_OK || window == nullptr) {
        EFG_RDR_LOGE("AImageReader_getWindow failed: %d", static_cast<int>(st));
        impl_->closeReader();
        return FgResult::READER;
    }

    AImageReader_ImageListener imageListener{impl_.get(), &Impl::onImageAvailable};
    st = AImageReader_setImageListener(reader, &imageListener);
    if (st != AMEDIA_OK) {
        EFG_RDR_LOGE("AImageReader_setImageListener failed: %d", static_cast<int>(st));
        impl_->closeReader();
        return FgResult::READER;
    }
    AImageReader_BufferRemovedListener removedListener{impl_.get(), &Impl::onBufferRemoved};
    st = AImageReader_setBufferRemovedListener(reader, &removedListener);
    if (st != AMEDIA_OK) {
        EFG_RDR_LOGE("AImageReader_setBufferRemovedListener failed: %d", static_cast<int>(st));
        impl_->closeReader();
        return FgResult::READER;
    }

    impl_->window = window;
    EFG_RDR_LOGI("reader %dx%d max=%d: linear=%d (source=%s) usage=0x%llx", width, height, maxImages,
                 mode.linear ? 1 : 0, mode.source, static_cast<unsigned long long>(usage));
    return FgResult::OK;
}

ANativeWindow* FrameSource::window() const {
    return impl_->window;
}

void FrameSource::setListener(FrameSourceListener fn) {
    // Blocks while a callback is running, so the old listener is never invoked
    // after this returns.
    std::lock_guard<std::mutex> lock(impl_->listenerMutex);
    impl_->listener = std::move(fn);
}

int32_t FrameSource::acquire(InFrame& out, int32_t timeoutMs) {
    out = InFrame{};
    AImageReader* reader = impl_->reader;
    if (reader == nullptr) return FgResult::READER;

    const auto deadline =
        std::chrono::steady_clock::now() + std::chrono::milliseconds(timeoutMs > 0 ? timeoutMs : 0);

    AImage* image = nullptr;
    int fenceFd = -1;
    for (;;) {
        uint64_t signalsBefore = 0;
        {
            std::lock_guard<std::mutex> lock(impl_->waitMutex);
            signalsBefore = impl_->imageSignals;
        }
        image = nullptr;
        fenceFd = -1;
        const media_status_t st = AImageReader_acquireNextImageAsync(reader, &image, &fenceFd);
        if (st == AMEDIA_OK && image != nullptr) break;
        if (st == AMEDIA_OK) {  // defensive: OK without an image
            if (fenceFd >= 0) ::close(fenceFd);
            if (impl_->firstReaderError()) {
                EFG_RDR_LOGE("AImageReader_acquireNextImageAsync returned OK without an image");
            }
            return FgResult::READER;
        }
        if (st == AMEDIA_IMGREADER_MAX_IMAGES_ACQUIRED) return FgResult::BUSY;
        if (st != AMEDIA_IMGREADER_NO_BUFFER_AVAILABLE) {
            if (impl_->firstReaderError()) {
                EFG_RDR_LOGE("AImageReader_acquireNextImageAsync failed: %d", static_cast<int>(st));
            }
            return FgResult::READER;
        }
        // No image yet: wait for the next image-available signal or the deadline.
        std::unique_lock<std::mutex> lock(impl_->waitMutex);
        const bool signalled = impl_->waitCv.wait_until(
            lock, deadline, [&] { return impl_->imageSignals != signalsBefore; });
        if (!signalled) return FgResult::BUSY;
    }

    // From here the image and fence are owned by `owner` on every path (I2).
    InFrame frame{};
    frame.image = image;
    frame.fenceFd = fenceFd;
    InFrameHandle owner(frame);

    AHardwareBuffer* buffer = nullptr;
    media_status_t st = AImage_getHardwareBuffer(image, &buffer);
    if (st != AMEDIA_OK || buffer == nullptr) {
        if (impl_->firstReaderError()) {
            EFG_RDR_LOGE("AImage_getHardwareBuffer failed: %d", static_cast<int>(st));
        }
        return FgResult::READER;  // owner: close(fence) + AImage_delete
    }
    int64_t tsNs = 0;
    st = AImage_getTimestamp(image, &tsNs);
    if (st != AMEDIA_OK) {
        if (impl_->firstReaderError()) {
            EFG_RDR_LOGE("AImage_getTimestamp failed: %d", static_cast<int>(st));
        }
        return FgResult::READER;  // owner: close(fence) + AImage_delete
    }

    out = owner.detach();  // ownership moves to the caller
    out.buffer = buffer;
    out.tsNs = tsNs;
    out.seq = impl_->nextSeq++;
    return FgResult::OK;
}

void FrameSource::release(InFrame& frame) {
    InFrameHandle owner(frame);  // close(fenceFd) if >= 0, AImage_delete if set
    frame = InFrame{};
}

void FrameSource::close() {
    if (!impl_) return;
    impl_->closeReader();
}

}  // namespace efg
