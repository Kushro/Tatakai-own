// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Ported from Enigmata (app/src/main/cpp/fg) to Tatakai Next 0.4.0 (core/framegen): JNI class, library name,
// log tags and debug properties renamed; no other change.
//
// Enigmata frame generation (R13): JNI bridge for app.tatakai.next.framegen.FgNative.
// Written for Enigmata (no code adapted from LSFG-Android-Application).
//
// Natives are registered in JNI_OnLoad with RegisterNatives from ONE table
// (kMethods). FgNative is a Kotlin `object`, the natives are
// `private external fun` members of it (NOT @JvmStatic), so every C entry
// receives `jobject thiz` (the object instance), not a jclass. The proguard
// rule must keep the class name and the native member names.
//
// No C++ exception crosses JNI: every entry has try / catch(const
// std::exception&) / catch(...) and maps failures to an FgResult code (or
// null / false / no-op by return type). Pending Java exceptions raised by JNI
// helpers (OOM in GetStringUTFChars etc.) are cleared, so Kotlin always gets a
// code; the only exception is nativeStats when NewLongArray itself fails (OOM
// stays pending because a non-null LongArray cannot be returned).
//
// ---------------------------------------------------------------------------
// Native table (name | JNI descriptor | Kotlin method (signature))
// ---------------------------------------------------------------------------
// nativeDeviceInfo         | ()Ljava/lang/String;                           | private external fun nativeDeviceInfo(): String?
// nativeExtractShaders     | (Ljava/lang/String;Ljava/lang/String;)I        | private external fun nativeExtractShaders(dllPath: String, outDir: String): Int
// nativeProbeShaders       | (Ljava/lang/String;)Z                          | private external fun nativeProbeShaders(dir: String): Boolean
// nativeCreateInputSurface | (II)Landroid/view/Surface;                     | private external fun nativeCreateInputSurface(width: Int, height: Int): Surface?
// nativeStart              | (Landroid/view/Surface;[I[FLjava/lang/String;)I | private external fun nativeStart(outSurface: Surface, ints: IntArray, floats: FloatArray, shaderDir: String): Int
// nativeUpdateConfig       | ([I[FLjava/lang/String;)I                      | private external fun nativeUpdateConfig(ints: IntArray, floats: FloatArray, shaderDir: String): Int
// nativeSetSourcePeriodNs  | (J)V                                           | private external fun nativeSetSourcePeriodNs(ns: Long)
// nativeFlush              | ()V                                            | private external fun nativeFlush()
// nativeSetPaused          | (Z)V                                           | private external fun nativeSetPaused(paused: Boolean)
// nativeStop               | ()V                                            | private external fun nativeStop()
// nativeStats              | ()[J                                           | private external fun nativeStats(): LongArray
//
// Semantics:
// - nativeDeviceInfo: "k=v;..." from efg::probeDeviceInfo
//   (apiVersion,gpuName,vendor,device,ahb,swapchain,displayTiming,fp16);
//   null on error, and also null while a pipeline is running (the probe would
//   touch volk's global pointers that framegen owns at that point).
// - nativeExtractShaders: >= 0 number of FP32 shaders written; < 0 FgResult
//   (DLL_UNREADABLE, MISSING_SPIRV, BINDING_REWRITE, WRITE_FAILED;
//   INVALID_ARG for null/empty paths; DLL_UNREADABLE on internal exception).
// - nativeProbeShaders: true when dir/fp32 holds the complete FP32 set.
// - nativeCreateInputSurface: opens the AImageReader (FrameSource, maxImages
//   kReaderMaxImages) of width x height and returns its window as a Surface
//   (ANativeWindow_toSurface). Null if width/height <= 0, a pipeline is
//   running, or the reader fails. An already open (not started) source is
//   closed and replaced. The Surface is invalid after nativeStop(); Kotlin
//   calls Surface.release().
// - nativeStart: needs an open input source (else NOT_INITIALIZED); BUSY if a
//   pipeline is running; INVALID_ARG on bad arrays/values or when
//   inputWidth/inputHeight differ from the nativeCreateInputSurface size;
//   SHADERS_MISSING if shaderDir lacks the FP32 set. Order:
//   FgDevice::create -> ANativeWindow_fromSurface(outSurface) (SURFACE if null)
//   -> FgPresenter::start (takes the window) -> FgEngine::start(makeLoader(
//   shaderDir, allowFp16)). On failure everything start created is undone in
//   reverse order and the code is returned; the input source stays open (a
//   retry may reuse it; nativeStop() releases it). Internal exception: CONTEXT.
// - nativeUpdateConfig: OK / NOT_INITIALIZED (no running pipeline) /
//   INVALID_ARG (bad arrays/values, or input or output size different from the
//   running pipeline: a size change needs a stop/start cycle). Queued by the
//   engine (finalize + initialize on its thread). Internal exception: CONTEXT.
// - nativeSetSourcePeriodNs / nativeFlush / nativeSetPaused: no-op without a
//   running pipeline; negative ns is passed as 0 (estimate only).
// - nativeStop: blocking, idempotent; I3 order: reader listener to null ->
//   engine stop (join) -> presenter stop (join) -> FrameSource close (pending
//   AImages + AImageReader) -> ring / slot images -> FgDevice::destroy. Also
//   releases an input source that was opened but never (successfully) started.
// - nativeStats: always 12 elements (FgStatIndex). Running: engine counters
//   with index 7 from FgPresenter::path(). After nativeStop: the last snapshot
//   taken during stop, with state Running/Paused reported as Stopped (Failed is
//   kept, I9). Never started: all zeros.
//
// ---------------------------------------------------------------------------
// FgConfig packing (nativeStart / nativeUpdateConfig)
// ---------------------------------------------------------------------------
// ints (IntArray, length exactly kCfgIntCount = 9):
//   0 kCfgMultiplier       multiplier            kMinMultiplier..kMaxMultiplier (2..5)
//   1 kCfgPerformanceMode  performanceMode       0 | 1
//   2 kCfgInputWidth       inputWidth            > 0
//   3 kCfgInputHeight      inputHeight           > 0
//   4 kCfgOutputWidth      outputWidth           > 0
//   5 kCfgOutputHeight     outputHeight          > 0
//   6 kCfgAllowFp16        allowFp16             0 | 1
//   7 kCfgAntiArtifacts    antiArtifacts         0 | 1
//   8 kCfgVerbose          verbose               0 | 1
// floats (FloatArray, length exactly kCfgFloatCount = 1):
//   0 kCfgFlowScale        flowScale             finite, > 0
// shaderDir (String): non-null, non-empty.
// Any other length, value or null -> FgResult.INVALID_ARG (-11).
//
// ---------------------------------------------------------------------------
// nativeStats indices (efg::FgStatIndex, Kotlin FgStats.from(LongArray))
// ---------------------------------------------------------------------------
//   0 inFrames  1 outFrames  2 generated  3 droppedLate  4 latencyNs
//   5 computeNsAvg  6 periodNs  7 presentPath (0 none, 1 display_timing,
//   2 sleep-vsync, 3 CPU)  8 state (0 stopped, 1 running, 2 paused, 3 failed)
//   9 lastError  10 inW  11 inH
//
// ---------------------------------------------------------------------------
// FgResult codes returned from here (values from fg_types.h)
// ---------------------------------------------------------------------------
//   OK 0, NOT_INITIALIZED -1, VK_INIT -2, MISSING_EXTENSION -3,
//   SHADERS_MISSING -4, LSFG_INIT -5, CONTEXT -6, READER -7, SURFACE -8,
//   BUSY -10, INVALID_ARG -11, DLL_UNREADABLE -20, MISSING_SPIRV -21,
//   BINDING_REWRITE -22, WRITE_FAILED -23 (the last four from extractShaders;
//   VK_INIT/MISSING_EXTENSION from FgDevice::create, LSFG_INIT/CONTEXT/
//   SHADERS_MISSING from FgEngine::start). DEVICE_LOST -9 only appears in
//   stats lastError.

#include "fg_device.h"
#include "fg_engine.h"
#include "fg_presenter.h"
#include "fg_shader_loader.h"
#include "fg_types.h"
#include "frame_source.h"

#include <android/log.h>
#include <android/native_window.h>
#include <android/native_window_jni.h>
#include <jni.h>

#include <chrono>
#include <cmath>
#include <cstdint>
#include <exception>
#include <memory>
#include <mutex>
#include <string>
#include <utility>

namespace {

using namespace efg;

constexpr const char* kTag = "TatakaiFG";
constexpr const char* kFgNativeClass = "app/tatakai/next/framegen/FgNative";

// AImageReader depth (design: AImageReader_newWithUsage(..., 8)).
constexpr int32_t kReaderMaxImages = 8;

// FgConfig ints[] indices.
constexpr jsize kCfgMultiplier = 0;
constexpr jsize kCfgPerformanceMode = 1;
constexpr jsize kCfgInputWidth = 2;
constexpr jsize kCfgInputHeight = 3;
constexpr jsize kCfgOutputWidth = 4;
constexpr jsize kCfgOutputHeight = 5;
constexpr jsize kCfgAllowFp16 = 6;
constexpr jsize kCfgAntiArtifacts = 7;
constexpr jsize kCfgVerbose = 8;
constexpr jsize kCfgIntCount = 9;
// FgConfig floats[] indices.
constexpr jsize kCfgFlowScale = 0;
constexpr jsize kCfgFloatCount = 1;

void logError(const char* where, const char* what) {
    __android_log_print(ANDROID_LOG_ERROR, kTag, "%s: %s", where, what);
}

// Clears any Java exception left pending by a JNI helper.
void clearPending(JNIEnv* env) {
    if (env->ExceptionCheck()) {
        env->ExceptionClear();
    }
}

// RAII GetStringUTFChars / ReleaseStringUTFChars.
class Utf {
public:
    Utf(JNIEnv* env, jstring s) : env_(env), s_(s) {
        if (s_ != nullptr) {
            chars_ = env_->GetStringUTFChars(s_, nullptr);
            if (chars_ == nullptr) clearPending(env_);
        }
    }
    ~Utf() {
        if (chars_ != nullptr) env_->ReleaseStringUTFChars(s_, chars_);
    }
    Utf(const Utf&) = delete;
    Utf& operator=(const Utf&) = delete;

    bool ok() const { return chars_ != nullptr; }
    std::string str() const { return chars_ != nullptr ? std::string(chars_) : std::string(); }

private:
    JNIEnv* env_;
    jstring s_;
    const char* chars_ = nullptr;
};

// RAII Get/ReleaseIntArrayElements (read only: JNI_ABORT).
class IntsRead {
public:
    IntsRead(JNIEnv* env, jintArray a) : env_(env), a_(a) {
        if (a_ != nullptr) {
            len_ = env_->GetArrayLength(a_);
            elems_ = env_->GetIntArrayElements(a_, nullptr);
            if (elems_ == nullptr) clearPending(env_);
        }
    }
    ~IntsRead() {
        if (elems_ != nullptr) env_->ReleaseIntArrayElements(a_, elems_, JNI_ABORT);
    }
    IntsRead(const IntsRead&) = delete;
    IntsRead& operator=(const IntsRead&) = delete;

    bool ok() const { return elems_ != nullptr; }
    jsize size() const { return len_; }
    jint operator[](jsize i) const { return elems_[i]; }

private:
    JNIEnv* env_;
    jintArray a_;
    jint* elems_ = nullptr;
    jsize len_ = 0;
};

// RAII Get/ReleaseFloatArrayElements (read only: JNI_ABORT).
class FloatsRead {
public:
    FloatsRead(JNIEnv* env, jfloatArray a) : env_(env), a_(a) {
        if (a_ != nullptr) {
            len_ = env_->GetArrayLength(a_);
            elems_ = env_->GetFloatArrayElements(a_, nullptr);
            if (elems_ == nullptr) clearPending(env_);
        }
    }
    ~FloatsRead() {
        if (elems_ != nullptr) env_->ReleaseFloatArrayElements(a_, elems_, JNI_ABORT);
    }
    FloatsRead(const FloatsRead&) = delete;
    FloatsRead& operator=(const FloatsRead&) = delete;

    bool ok() const { return elems_ != nullptr; }
    jsize size() const { return len_; }
    jfloat operator[](jsize i) const { return elems_[i]; }

private:
    JNIEnv* env_;
    jfloatArray a_;
    jfloat* elems_ = nullptr;
    jsize len_ = 0;
};

// Owns an ANativeWindow reference until it is handed over to the presenter.
class WindowRef {
public:
    explicit WindowRef(ANativeWindow* w) : w_(w) {}
    ~WindowRef() {
        if (w_ != nullptr) ANativeWindow_release(w_);
    }
    WindowRef(const WindowRef&) = delete;
    WindowRef& operator=(const WindowRef&) = delete;

    ANativeWindow* get() const { return w_; }
    // Ownership passes to the caller (FgPresenter::start releases it).
    ANativeWindow* handOver() {
        ANativeWindow* out = w_;
        w_ = nullptr;
        return out;
    }

private:
    ANativeWindow* w_;
};

bool isBool01(jint v) { return v == 0 || v == 1; }

// Unpacks ints/floats/shaderDir into `out`. Returns OK or INVALID_ARG.
int32_t unpackConfig(JNIEnv* env, jintArray jints, jfloatArray jfloats, jstring jshaderDir,
                     FgConfigN& out) {
    if (jints == nullptr || jfloats == nullptr || jshaderDir == nullptr) {
        return FgResult::INVALID_ARG;
    }
    IntsRead ints(env, jints);
    FloatsRead floats(env, jfloats);
    Utf dir(env, jshaderDir);
    if (!ints.ok() || !floats.ok() || !dir.ok()) return FgResult::INVALID_ARG;
    if (ints.size() != kCfgIntCount || floats.size() != kCfgFloatCount) {
        return FgResult::INVALID_ARG;
    }

    FgConfigN cfg;
    cfg.multiplier = ints[kCfgMultiplier];
    const jint perf = ints[kCfgPerformanceMode];
    cfg.inputWidth = ints[kCfgInputWidth];
    cfg.inputHeight = ints[kCfgInputHeight];
    cfg.outputWidth = ints[kCfgOutputWidth];
    cfg.outputHeight = ints[kCfgOutputHeight];
    const jint fp16 = ints[kCfgAllowFp16];
    const jint anti = ints[kCfgAntiArtifacts];
    const jint verbose = ints[kCfgVerbose];
    cfg.flowScale = floats[kCfgFlowScale];
    cfg.shaderDir = dir.str();

    if (cfg.multiplier < kMinMultiplier || cfg.multiplier > kMaxMultiplier) {
        return FgResult::INVALID_ARG;
    }
    if (!isBool01(perf) || !isBool01(fp16) || !isBool01(anti) || !isBool01(verbose)) {
        return FgResult::INVALID_ARG;
    }
    if (cfg.inputWidth <= 0 || cfg.inputHeight <= 0 || cfg.outputWidth <= 0 ||
        cfg.outputHeight <= 0) {
        return FgResult::INVALID_ARG;
    }
    if (!std::isfinite(cfg.flowScale) || cfg.flowScale <= 0.0f) return FgResult::INVALID_ARG;
    if (cfg.shaderDir.empty()) return FgResult::INVALID_ARG;

    cfg.performanceMode = perf == 1;
    cfg.allowFp16 = fp16 == 1;
    cfg.antiArtifacts = anti == 1;
    cfg.verbose = verbose == 1;
    out = std::move(cfg);
    return FgResult::OK;
}

// The single global session. Every field is guarded by gMutex.
struct Session {
    std::unique_ptr<FgDevice> device;
    std::unique_ptr<FrameSource> source;  // opened by nativeCreateInputSurface
    int32_t sourceW = 0;
    int32_t sourceH = 0;
    std::unique_ptr<PresentRing> ring;
    std::unique_ptr<FreeSlotQueue> freeSlots;
    std::unique_ptr<FgEngine> engine;
    std::unique_ptr<FgPresenter> presenter;
    bool running = false;  // nativeStart succeeded and nativeStop not yet called
    FgConfigN cfg;         // config of the running pipeline
    FgStatsArray lastStats{};  // retained after nativeStop
};

std::mutex gMutex;
Session gSession;

// Fills `out` from the live pipeline (engine counters + presenter path).
void liveStatsLocked(Session& s, FgStatsArray& out) {
    out.fill(0);
    if (s.engine) s.engine->stats(out);
    out[kStatPresentPath] =
        s.presenter ? static_cast<int64_t>(s.presenter->path()) : static_cast<int64_t>(PresentPath::None);
}

// Tears the pipeline down in the I3 order. Every step is isolated so that one
// failure never skips the following ones. With closeSource == false the input
// FrameSource stays open (rollback of a failed nativeStart). noexcept.
void teardownLocked(Session& s, bool closeSource) noexcept {
    // 1. Reader listener to null.
    try {
        if (s.source) s.source->setListener(FrameSourceListener{});
    } catch (...) {
        logError("teardown", "setListener failed");
    }
    // 2. Engine join (deleteContext + finalize on its own thread).
    try {
        if (s.engine) s.engine->stop();
    } catch (...) {
        logError("teardown", "engine stop failed");
    }
    // Final counters, taken before the presenter loses its path.
    try {
        if (s.running) {
            FgStatsArray snap{};
            liveStatsLocked(s, snap);
            const int64_t st = snap[kStatState];
            if (st == static_cast<int64_t>(FgStatus::Running) ||
                st == static_cast<int64_t>(FgStatus::Paused)) {
                snap[kStatState] = static_cast<int64_t>(FgStatus::Stopped);
            }
            s.lastStats = snap;
        }
    } catch (...) {
        logError("teardown", "stats snapshot failed");
    }
    // 3. Presenter join (vkDeviceWaitIdle, swapchain, VkSurfaceKHR, window).
    try {
        if (s.presenter) s.presenter->stop();
    } catch (...) {
        logError("teardown", "presenter stop failed");
    }
    // 4. Pending AImages and the AImageReader.
    if (closeSource) {
        try {
            if (s.source) s.source->close();
        } catch (...) {
            logError("teardown", "source close failed");
        }
    }
    // 5. Thread objects, then the ring (slot images on our device).
    try {
        s.engine.reset();
    } catch (...) {
        logError("teardown", "engine release failed");
    }
    try {
        s.presenter.reset();
    } catch (...) {
        logError("teardown", "presenter release failed");
    }
    if (closeSource) {
        try {
            s.source.reset();
        } catch (...) {
            logError("teardown", "source release failed");
        }
        s.sourceW = 0;
        s.sourceH = 0;
    }
    try {
        s.ring.reset();
        s.freeSlots.reset();
    } catch (...) {
        logError("teardown", "ring release failed");
    }
    // 6. Device last.
    try {
        if (s.device) s.device->destroy();
        s.device.reset();
    } catch (...) {
        logError("teardown", "device destroy failed");
    }
    s.running = false;
}

// ---------------------------------------------------------------------------
// Entries
// ---------------------------------------------------------------------------

jstring nativeDeviceInfo(JNIEnv* env, jobject /*thiz*/) {
    try {
        std::lock_guard<std::mutex> lock(gMutex);
        if (gSession.running) return nullptr;
        std::string info;
        if (probeDeviceInfo(info) != FgResult::OK) return nullptr;
        jstring out = env->NewStringUTF(info.c_str());
        if (out == nullptr) clearPending(env);
        return out;
    } catch (const std::exception& e) {
        logError("nativeDeviceInfo", e.what());
    } catch (...) {
        logError("nativeDeviceInfo", "unknown exception");
    }
    return nullptr;
}

jint nativeExtractShaders(JNIEnv* env, jobject /*thiz*/, jstring jdllPath, jstring joutDir) {
    try {
        if (jdllPath == nullptr || joutDir == nullptr) return FgResult::INVALID_ARG;
        Utf dll(env, jdllPath);
        Utf outDir(env, joutDir);
        if (!dll.ok() || !outDir.ok()) return FgResult::INVALID_ARG;
        const std::string dllPath = dll.str();
        const std::string outPath = outDir.str();
        if (dllPath.empty() || outPath.empty()) return FgResult::INVALID_ARG;
        return extractShaders(dllPath, outPath);
    } catch (const std::exception& e) {
        logError("nativeExtractShaders", e.what());
    } catch (...) {
        logError("nativeExtractShaders", "unknown exception");
    }
    return FgResult::DLL_UNREADABLE;
}

jboolean nativeProbeShaders(JNIEnv* env, jobject /*thiz*/, jstring jdir) {
    try {
        if (jdir == nullptr) return JNI_FALSE;
        Utf dir(env, jdir);
        if (!dir.ok()) return JNI_FALSE;
        const std::string path = dir.str();
        if (path.empty()) return JNI_FALSE;
        return probeShaders(path) ? JNI_TRUE : JNI_FALSE;
    } catch (const std::exception& e) {
        logError("nativeProbeShaders", e.what());
    } catch (...) {
        logError("nativeProbeShaders", "unknown exception");
    }
    return JNI_FALSE;
}

jobject nativeCreateInputSurface(JNIEnv* env, jobject /*thiz*/, jint width, jint height) {
    try {
        if (width <= 0 || height <= 0) return nullptr;
        std::lock_guard<std::mutex> lock(gMutex);
        Session& s = gSession;
        if (s.running) return nullptr;  // BUSY: a pipeline owns the source
        // Replace a source that was opened but never started.
        teardownLocked(s, /*closeSource=*/true);

        auto source = std::make_unique<FrameSource>();
        if (source->open(width, height, kReaderMaxImages) != FgResult::OK) {
            source->close();
            return nullptr;
        }
        ANativeWindow* window = source->window();
        if (window == nullptr) {
            source->close();
            return nullptr;
        }
        jobject surface = ANativeWindow_toSurface(env, window);
        if (surface == nullptr) {
            clearPending(env);
            source->close();
            return nullptr;
        }
        s.source = std::move(source);
        s.sourceW = width;
        s.sourceH = height;
        return surface;  // local reference, owned by the caller frame
    } catch (const std::exception& e) {
        logError("nativeCreateInputSurface", e.what());
    } catch (...) {
        logError("nativeCreateInputSurface", "unknown exception");
    }
    clearPending(env);
    return nullptr;
}

// Body of nativeStart; called with gMutex held. On a non-OK return the caller
// rolls back with teardownLocked(s, false).
int32_t startLocked(JNIEnv* env, Session& s, jobject outSurface, const FgConfigN& cfg) {
    // 1. Device.
    s.device = std::make_unique<FgDevice>();
    int32_t rc = s.device->create();
    if (rc != FgResult::OK) return rc;

    s.ring = std::make_unique<PresentRing>();
    s.freeSlots = std::make_unique<FreeSlotQueue>();

    // 2. The FrameSource was opened by nativeCreateInputSurface (checked by
    //    the caller). 3. Output window.
    WindowRef window(ANativeWindow_fromSurface(env, outSurface));
    if (window.get() == nullptr) {
        clearPending(env);
        return FgResult::SURFACE;
    }

    // 4. Presenter (takes the window, also on failure).
    s.presenter = std::make_unique<FgPresenter>();
    rc = s.presenter->start(*s.device, window.handOver(), static_cast<uint32_t>(cfg.outputWidth),
                            static_cast<uint32_t>(cfg.outputHeight), *s.ring, *s.freeSlots);
    if (rc != FgResult::OK) return rc;

    // 5. Engine.
    s.engine = std::make_unique<FgEngine>();
    rc = s.engine->start(*s.device, *s.source, *s.ring, *s.freeSlots, cfg,
                         makeLoader(cfg.shaderDir, cfg.allowFp16));
    return rc;
}

jint nativeStartBody(JNIEnv* env, jobject outSurface, jintArray jints, jfloatArray jfloats,
                     jstring jshaderDir) {
    try {
        if (outSurface == nullptr) return FgResult::INVALID_ARG;
        FgConfigN cfg;
        int32_t rc = unpackConfig(env, jints, jfloats, jshaderDir, cfg);
        if (rc != FgResult::OK) return rc;

        std::lock_guard<std::mutex> lock(gMutex);
        Session& s = gSession;
        if (s.running) return FgResult::BUSY;
        if (!s.source) return FgResult::NOT_INITIALIZED;
        if (cfg.inputWidth != s.sourceW || cfg.inputHeight != s.sourceH) {
            return FgResult::INVALID_ARG;
        }
        if (!probeShaders(cfg.shaderDir)) return FgResult::SHADERS_MISSING;

        s.lastStats.fill(0);
        try {
            rc = startLocked(env, s, outSurface, cfg);
        } catch (const std::exception& e) {
            logError("nativeStart", e.what());
            rc = FgResult::CONTEXT;
        } catch (...) {
            logError("nativeStart", "unknown exception");
            rc = FgResult::CONTEXT;
        }
        if (rc != FgResult::OK) {
            teardownLocked(s, /*closeSource=*/false);
            s.lastStats.fill(0);
            return rc;
        }
        s.cfg = cfg;
        s.running = true;
        return FgResult::OK;
    } catch (const std::exception& e) {
        logError("nativeStart", e.what());
    } catch (...) {
        logError("nativeStart", "unknown exception");
    }
    clearPending(env);
    return FgResult::CONTEXT;
}

// REV 5 (measure in the pre-release): total nativeStart duration (gMutex wait
// included) and its result code, on every return path.
jint nativeStart(JNIEnv* env, jobject /*thiz*/, jobject outSurface, jintArray jints,
                 jfloatArray jfloats, jstring jshaderDir) {
    const auto t0 = std::chrono::steady_clock::now();
    const jint rc = nativeStartBody(env, outSurface, jints, jfloats, jshaderDir);
    const double ms =
        std::chrono::duration<double, std::milli>(std::chrono::steady_clock::now() - t0).count();
    __android_log_print(ANDROID_LOG_INFO, kTag, "nativeStart: %.1f ms, rc=%d", ms, static_cast<int>(rc));
    return rc;
}

jint nativeUpdateConfig(JNIEnv* env, jobject /*thiz*/, jintArray jints, jfloatArray jfloats,
                        jstring jshaderDir) {
    try {
        FgConfigN cfg;
        int32_t rc = unpackConfig(env, jints, jfloats, jshaderDir, cfg);
        if (rc != FgResult::OK) return rc;

        std::lock_guard<std::mutex> lock(gMutex);
        Session& s = gSession;
        if (!s.running || !s.engine) return FgResult::NOT_INITIALIZED;
        if (cfg.inputWidth != s.cfg.inputWidth || cfg.inputHeight != s.cfg.inputHeight ||
            cfg.outputWidth != s.cfg.outputWidth || cfg.outputHeight != s.cfg.outputHeight) {
            return FgResult::INVALID_ARG;
        }
        rc = s.engine->updateConfig(cfg);
        if (rc == FgResult::OK) s.cfg = cfg;
        return rc;
    } catch (const std::exception& e) {
        logError("nativeUpdateConfig", e.what());
    } catch (...) {
        logError("nativeUpdateConfig", "unknown exception");
    }
    clearPending(env);
    return FgResult::CONTEXT;
}

void nativeSetSourcePeriodNs(JNIEnv* /*env*/, jobject /*thiz*/, jlong ns) {
    try {
        std::lock_guard<std::mutex> lock(gMutex);
        if (gSession.running && gSession.engine) {
            gSession.engine->setSourcePeriodNs(ns > 0 ? static_cast<int64_t>(ns) : 0);
        }
    } catch (const std::exception& e) {
        logError("nativeSetSourcePeriodNs", e.what());
    } catch (...) {
        logError("nativeSetSourcePeriodNs", "unknown exception");
    }
}

void nativeFlush(JNIEnv* /*env*/, jobject /*thiz*/) {
    try {
        std::lock_guard<std::mutex> lock(gMutex);
        if (gSession.running && gSession.engine) gSession.engine->flush();
    } catch (const std::exception& e) {
        logError("nativeFlush", e.what());
    } catch (...) {
        logError("nativeFlush", "unknown exception");
    }
}

void nativeSetPaused(JNIEnv* /*env*/, jobject /*thiz*/, jboolean paused) {
    try {
        std::lock_guard<std::mutex> lock(gMutex);
        if (gSession.running && gSession.engine) gSession.engine->setPaused(paused == JNI_TRUE);
    } catch (const std::exception& e) {
        logError("nativeSetPaused", e.what());
    } catch (...) {
        logError("nativeSetPaused", "unknown exception");
    }
}

void nativeStop(JNIEnv* /*env*/, jobject /*thiz*/) {
    try {
        std::lock_guard<std::mutex> lock(gMutex);
        teardownLocked(gSession, /*closeSource=*/true);
    } catch (const std::exception& e) {
        logError("nativeStop", e.what());
    } catch (...) {
        logError("nativeStop", "unknown exception");
    }
}

jlongArray nativeStats(JNIEnv* env, jobject /*thiz*/) {
    static_assert(sizeof(jlong) == sizeof(int64_t), "jlong must be int64_t");
    FgStatsArray values{};
    try {
        std::lock_guard<std::mutex> lock(gMutex);
        if (gSession.running) {
            liveStatsLocked(gSession, values);
        } else {
            values = gSession.lastStats;
        }
    } catch (const std::exception& e) {
        logError("nativeStats", e.what());
        values.fill(0);
    } catch (...) {
        logError("nativeStats", "unknown exception");
        values.fill(0);
    }
    jlongArray out = env->NewLongArray(static_cast<jsize>(kStatCount));
    if (out == nullptr) return nullptr;  // OutOfMemoryError stays pending
    env->SetLongArrayRegion(out, 0, static_cast<jsize>(kStatCount),
                            reinterpret_cast<const jlong*>(values.data()));
    return out;
}

// The single native table (see the header comment).
const JNINativeMethod kMethods[] = {
    {"nativeDeviceInfo", "()Ljava/lang/String;", reinterpret_cast<void*>(nativeDeviceInfo)},
    {"nativeExtractShaders", "(Ljava/lang/String;Ljava/lang/String;)I",
     reinterpret_cast<void*>(nativeExtractShaders)},
    {"nativeProbeShaders", "(Ljava/lang/String;)Z", reinterpret_cast<void*>(nativeProbeShaders)},
    {"nativeCreateInputSurface", "(II)Landroid/view/Surface;",
     reinterpret_cast<void*>(nativeCreateInputSurface)},
    {"nativeStart", "(Landroid/view/Surface;[I[FLjava/lang/String;)I",
     reinterpret_cast<void*>(nativeStart)},
    {"nativeUpdateConfig", "([I[FLjava/lang/String;)I", reinterpret_cast<void*>(nativeUpdateConfig)},
    {"nativeSetSourcePeriodNs", "(J)V", reinterpret_cast<void*>(nativeSetSourcePeriodNs)},
    {"nativeFlush", "()V", reinterpret_cast<void*>(nativeFlush)},
    {"nativeSetPaused", "(Z)V", reinterpret_cast<void*>(nativeSetPaused)},
    {"nativeStop", "()V", reinterpret_cast<void*>(nativeStop)},
    {"nativeStats", "()[J", reinterpret_cast<void*>(nativeStats)},
};

}  // namespace

// Registers kMethods on app.tatakai.next.framegen.FgNative. Returning JNI_ERR makes
// System.loadLibrary throw UnsatisfiedLinkError, which FgNative catches
// (isAvailable() == false).
extern "C" JNIEXPORT jint JNICALL JNI_OnLoad(JavaVM* vm, void* /*reserved*/) {
    try {
        JNIEnv* env = nullptr;
        if (vm->GetEnv(reinterpret_cast<void**>(&env), JNI_VERSION_1_6) != JNI_OK || env == nullptr) {
            return JNI_ERR;
        }
        jclass cls = env->FindClass(kFgNativeClass);
        if (cls == nullptr) {
            clearPending(env);
            logError("JNI_OnLoad", "FgNative class not found");
            return JNI_ERR;
        }
        const jint rc = env->RegisterNatives(cls, kMethods,
                                             static_cast<jint>(sizeof(kMethods) / sizeof(kMethods[0])));
        env->DeleteLocalRef(cls);
        if (rc != JNI_OK) {
            clearPending(env);
            logError("JNI_OnLoad", "RegisterNatives failed");
            return JNI_ERR;
        }
        return JNI_VERSION_1_6;
    } catch (const std::exception& e) {
        logError("JNI_OnLoad", e.what());
    } catch (...) {
        logError("JNI_OnLoad", "unknown exception");
    }
    return JNI_ERR;
}
