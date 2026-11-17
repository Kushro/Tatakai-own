// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player.mpv.quality

import app.tatakai.next.model.AspectMode
import app.tatakai.next.model.DebandConfig
import app.tatakai.next.model.DecoderMode
import app.tatakai.next.model.ImageAdjust
import app.tatakai.next.model.PlayerEngine
import app.tatakai.next.model.QualityPresets
import app.tatakai.next.model.QualityTier
import app.tatakai.next.model.SharpenLevel
import app.tatakai.next.player.AutoDowngrade
import app.tatakai.next.player.PresetOption
import app.tatakai.next.player.QualityEvent
import app.tatakai.next.player.QualityEventKind
import app.tatakai.next.player.QualityLevel
import app.tatakai.next.player.QualityReason
import app.tatakai.next.player.QualityRequest
import app.tatakai.next.player.QualityUiState
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

// 0.4.0 (worker A): the one owner of every quality property of a running mpv, adapted from Enigmata
// `domain/quality/QualityController.kt` + `QualityRuntime.kt` (AGPL-3.0-or-later, same author). Much smaller than Enigmata's: Tatakai
// has no personal shaders, no effect layers, no mpv.conf and no content detection, so one diff-based write path is enough. Enigmata's
// rules kept: the frame-drop ceiling is session-only and undoable (B4), the FP16 Anime4K copies fall back to full precision live on a
// shader error (D3/D5, with a grace window for errors of the chain being replaced), nothing is ever persisted from here.

/** The mpv calls the controller needs (fakes in tests never load libmpv). */
interface QualityMpvPort {
    fun setProperty(name: String, value: String)

    fun getProperty(name: String): String?
}

/**
 * Plans with [QualityPlanner], writes only what changed to mpv, publishes [state]. Thread-safe (every entry point is synchronized):
 * the UI calls the setters on the main thread, the session's monitor and the thermal / battery signals come from coroutines.
 */
class MpvQualityController(
    private val port: QualityMpvPort,
    private val shaders: ShaderResolver,
    initial: QualityRequest,
    /** Use the FP16 (mediump) Anime4K copies while they work on this GPU. */
    fp16: Boolean = true,
    private val now: () -> Long = { System.nanoTime() / 1_000_000 },
) {
    private var request = initial.sanitized()
    private var signals = QualitySignals()
    private var displayHz: Float? = null
    private var fp16 = fp16
    private var fp16FallbackAt: Long? = null
    private var dropsSuppressed = false
    private val drops = FrameDropMonitor()
    private var plan: QualityPlan = QualityPlanner.plan(request, signals)
    private var resolvedFor: Pair<List<ShaderSpec>, Boolean>? = null
    private var resolvedPaths: List<String> = emptyList()
    private var activeFp16 = false
    private val written = HashMap<String, String>()
    private var writtenShaders: String? = null
    private var seq = 0
    private var lastEvent: QualityEvent? = null
    private var autoDowngrade: AutoDowngrade? = null
    private var telemetry = Telemetry()

    private val _state = MutableStateFlow(QualityUiState(available = true, playerEngine = PlayerEngine.MPV))
    val state: StateFlow<QualityUiState> = _state.asStateFlow()

    init {
        synchronized(this) { apply(cause = null) }
    }

    // ---- user commands (no event: the user sees what they chose) ---------------------------------------------------------------

    /** A new preset / tier is a fresh start: the frame-drop ceiling and a shader error of the old chain no longer apply. */
    @Synchronized
    fun setQuality(preset: String, tier: QualityTier) {
        request = request.copy(preset = QualityPresets.sanitize(preset), tier = tier)
        freshChain()
        apply(cause = null)
    }

    @Synchronized
    fun setRequest(r: QualityRequest) {
        val next = r.sanitized()
        if (next.preset != request.preset || next.tier != request.tier) freshChain()
        request = next
        apply(cause = null)
    }

    @Synchronized
    fun setSharpen(level: SharpenLevel) = user { it.copy(sharpen = level) }

    @Synchronized
    fun setDeband(deband: DebandConfig) = user { it.copy(deband = deband) }

    @Synchronized
    fun setImage(image: ImageAdjust) = user { it.copy(image = image) }

    @Synchronized
    fun setAspect(mode: AspectMode, zoomPercent: Int) = user { it.copy(aspect = mode, zoomPercent = zoomPercent) }

    @Synchronized
    fun setDecoder(mode: DecoderMode) = user { it.copy(decoder = mode) }

    @Synchronized
    fun setSmoothing(enabled: Boolean) = user { it.copy(smoothing = enabled) }

    @Synchronized
    fun setAutoAdjust(enabled: Boolean) {
        drops.reset()
        user { it.copy(autoAdjust = enabled) }
    }

    /** Enigmata B4: back to the chosen quality, and no further frame-drop step-down until the next file. */
    @Synchronized
    fun undoAutoDowngrade() {
        signals = signals.copy(dropCeiling = null)
        autoDowngrade = null
        dropsSuppressed = true
        drops.reset()
        apply(cause = null)
    }

    // ---- signals (automatic: each change of the result emits an event) ----------------------------------------------------------

    @Synchronized
    fun setPictureInPicture(active: Boolean) = signal(QualityReason.PIP) { it.copy(pip = active) }

    @Synchronized
    fun setRemoteProcessing(active: Boolean) = signal(QualityReason.REMOTE_PROCESSING) { it.copy(remoteProcessing = active) }

    @Synchronized
    fun setFrameGenActive(active: Boolean) = signal(QualityReason.FRAME_GEN) { it.copy(frameGenActive = active) }

    @Synchronized
    fun onThermal(active: Boolean) = signal(QualityReason.THERMAL) { it.copy(thermal = active) }

    /** Battery Eco (Enigmata): <= 20 % and not charging. Null = unknown (no change). */
    @Synchronized
    fun onBattery(percent: Int?, charging: Boolean) {
        if (percent == null || percent < 0) return
        signal(QualityReason.BATTERY_LOW) { it.copy(batteryLow = percent <= LOW_BATTERY_PERCENT && !charging) }
    }

    @Synchronized
    fun onVideoSize(width: Int, height: Int) = signal(null) { it.copy(videoWidth = width.coerceAtLeast(0), videoHeight = height.coerceAtLeast(0)) }

    @Synchronized
    fun onViewport(width: Int, height: Int) =
        signal(QualityReason.VIEWPORT) { it.copy(viewportWidth = width.coerceAtLeast(0), viewportHeight = height.coerceAtLeast(0)) }

    @Synchronized
    fun onDisplayRefreshRate(hz: Float?) {
        val v = hz?.takeIf { it.isFinite() && it > 0f }
        if (v == displayHz) return
        displayHz = v
        apply(cause = null)
    }

    /** A new file: its size is unknown (shaders wait), the drop window and a shader error of the previous file are forgotten. */
    @Synchronized
    fun onFileStarted() {
        drops.reset()
        dropsSuppressed = false
        telemetry = Telemetry()
        signals = signals.copy(videoWidth = 0, videoHeight = 0, shaderError = false)
        apply(cause = null)
    }

    /** Paused / buffering / seeking: the next drop window starts fresh. */
    @Synchronized
    fun onPlaybackIdle() = drops.reset()

    /**
     * One frame-drop sample while playing ([cumulativeDrops] since the file started). A too-slow window lowers the quality one step
     * ([QualityPlanner.nextDropCeiling]) for the rest of this playback, with an undoable [QualityReason.FRAME_DROPS] event.
     */
    @Synchronized
    fun onDropSample(cumulativeDrops: Long, fps: Double, nowMs: Long) {
        if (!request.autoAdjust || dropsSuppressed) return
        if (!drops.sample(cumulativeDrops, fps, nowMs)) return
        val next = QualityPlanner.nextDropCeiling(signals.dropCeiling, plan) ?: return
        val from = plan.level
        signals = signals.copy(dropCeiling = next)
        drops.reset()
        apply(cause = QualityReason.FRAME_DROPS)
        autoDowngrade = AutoDowngrade(from, plan.level, QualityReason.FRAME_DROPS)
        publish()
    }

    /**
     * mpv logged a shader compile / link error ([isShaderCompilationError]). An FP16 Anime4K chain first falls back to full precision
     * (same chain, new paths); otherwise the chain is turned off for this file. Errors within [FP16_GRACE_MS] of a fallback belong to
     * the chain being replaced and are ignored.
     */
    @Synchronized
    fun onShaderError() {
        if (resolvedPaths.isEmpty()) return
        val t = now()
        fp16FallbackAt?.let { if (t >= it && t - it < FP16_GRACE_MS) return }
        if (activeFp16) {
            fp16 = false
            fp16FallbackAt = t
            val level = plan.level
            apply(cause = null)
            emit(QualityEvent(++seq, QualityReason.SHADER_ERROR, QualityEventKind.PRECISION_FALLBACK, level, plan.level))
            return
        }
        signal(QualityReason.SHADER_ERROR) { it.copy(shaderError = true) }
    }

    /** Polls what mpv reports (decoder in use, drops, fps, renderer). Cheap property reads; the session calls it every couple of seconds. */
    @Synchronized
    fun refreshTelemetry() {
        val vo = port.getProperty("current-vo")?.takeIf { it.isNotBlank() }
        val api = port.getProperty("gpu-api")?.takeIf { it.isNotBlank() && it != "auto" } ?: "opengl"
        telemetry = Telemetry(
            decoder = port.getProperty("hwdec-current")?.takeIf { it.isNotBlank() },
            droppedFrames = (port.getProperty("frame-drop-count")?.toLongOrNull() ?: 0) +
                (port.getProperty("decoder-frame-drop-count")?.toLongOrNull() ?: 0),
            fps = (port.getProperty("container-fps") ?: port.getProperty("estimated-vf-fps"))?.toDoubleOrNull()?.takeIf { it > 0 },
            renderer = vo?.let { "$it / $api" },
        )
        publish()
    }

    // ---- internals -------------------------------------------------------------------------------------------------------------

    private fun freshChain() {
        signals = signals.copy(dropCeiling = null, shaderError = false)
        autoDowngrade = null
        dropsSuppressed = false
        drops.reset()
    }

    private inline fun user(change: (QualityRequest) -> QualityRequest) {
        request = change(request).sanitized()
        apply(cause = null)
    }

    private inline fun signal(cause: QualityReason?, change: (QualitySignals) -> QualitySignals) {
        val next = change(signals)
        if (next == signals) return
        signals = next
        apply(cause = cause, automatic = true)
    }

    private fun apply(cause: QualityReason?, automatic: Boolean = cause != null) {
        val old = plan
        val new = QualityPlanner.plan(request, signals)
        plan = new
        writeShaders(new)
        val props = LinkedHashMap<String, String>()
        props += MpvOptions.deband(new.deband, request.deband)
        props += MpvOptions.image(request.image)
        props += MpvOptions.aspect(request.aspect, request.zoomPercent)
        props["hwdec"] = request.decoder.hwdec
        props += MpvOptions.interpolation(new.interpolation, displayHz)
        for ((k, v) in props) {
            if (written[k] == v) continue
            runCatching { port.setProperty(k, v) }.onSuccess { written[k] = v }
        }
        val event = if (automatic) eventFor(old, new, cause) else null
        if (event != null) emit(event) else publish()
    }

    private fun writeShaders(p: QualityPlan) {
        val key = p.shaders to fp16
        if (key != resolvedFor) {
            val paths = shaders.resolve(p.shaders, fp16)
            resolvedPaths = paths.orEmpty()
            activeFp16 = paths != null && ShaderNames.usesFp16(p.shaders, fp16)
            resolvedFor = key
        }
        val list = MpvOptions.shaderList(resolvedPaths)
        if (list != writtenShaders) {
            runCatching { port.setProperty("glsl-shaders", list) }.onSuccess { writtenShaders = list }
            android.util.Log.i("mpv", "glsl-shaders (${p.level.preset}/${p.level.tier}${if (activeFp16) "/fp16" else ""}): ${p.shaders.joinToString(",") { it.asset }}")
        }
    }

    private fun emit(e: QualityEvent) {
        lastEvent = e
        publish()
    }

    private fun publish() {
        val p = plan
        _state.value = QualityUiState(
            available = true,
            playerEngine = PlayerEngine.MPV,
            requested = request,
            effective = p.level,
            family = p.family,
            activeShaders = if (resolvedPaths.isEmpty()) emptyList() else p.shaders.map { it.asset },
            reasons = p.reasons,
            debandActive = p.deband,
            sharpenActive = p.sharpen && resolvedPaths.isNotEmpty(),
            smoothingActive = p.interpolation,
            autoDowngrade = autoDowngrade.takeIf { request.autoAdjust && signals.dropCeiling != null },
            decoderInUse = telemetry.decoder,
            droppedFrames = telemetry.droppedFrames,
            videoFps = telemetry.fps,
            displayHz = displayHz,
            renderer = telemetry.renderer,
            videoWidth = signals.videoWidth,
            videoHeight = signals.videoHeight,
            viewportWidth = signals.viewportWidth,
            viewportHeight = signals.viewportHeight,
            thermal = signals.thermal,
            batteryLow = signals.batteryLow,
            pip = signals.pip,
            remoteProcessing = signals.remoteProcessing,
            frameGenActive = signals.frameGenActive,
            fp16 = activeFp16,
            presets = QualityPresets.ALL.map { id ->
                val why = ShaderCatalog.unsupported(id, signals.supportsCompute)
                PresetOption(id, available = why == null, unavailableReason = why)
            },
            lastEvent = lastEvent,
        )
    }

    private fun eventFor(old: QualityPlan, new: QualityPlan, cause: QualityReason?): QualityEvent? =
        qualityEvent(old, new, cause, seq + 1)?.also { seq = it.seq }

    private data class Telemetry(
        val decoder: String? = null,
        val droppedFrames: Long = 0,
        val fps: Double? = null,
        val renderer: String? = null,
    )

    companion object {
        const val LOW_BATTERY_PERCENT = 20
        const val FP16_GRACE_MS = 3_000L
    }
}

/**
 * The event of an automatic re-plan, or null when nothing the user can see changed. The reason is [cause] when it entered / left the
 * plan's reasons, else the most important reason that entered (a downgrade / suspension) or left (a restore). Waiting for the video
 * size ([QualityReason.METADATA_PENDING]) is never announced.
 */
internal fun qualityEvent(old: QualityPlan, new: QualityPlan, cause: QualityReason?, seq: Int): QualityEvent? {
    val visible = old.level != new.level || old.deband != new.deband || old.interpolation != new.interpolation || old.sharpen != new.sharpen
    if (!visible) return null
    val added = (new.reasons - old.reasons.toSet()).filter { it != QualityReason.METADATA_PENDING }
    val removed = (old.reasons - new.reasons.toSet()).filter { it != QualityReason.METADATA_PENDING }
    val reason = cause?.takeIf { it in added || it in removed } ?: added.firstOrNull() ?: removed.firstOrNull() ?: return null
    val rankOld = rank(old.level)
    val rankNew = rank(new.level)
    val extrasOff = (old.deband && !new.deband) || (old.interpolation && !new.interpolation) || (old.sharpen && !new.sharpen)
    val kind = when {
        reason in removed && reason !in added -> QualityEventKind.RESTORED
        new.level.off && !old.level.off -> QualityEventKind.SUSPENDED
        rankNew < rankOld -> QualityEventKind.DOWNGRADED
        rankNew > rankOld -> QualityEventKind.RESTORED
        extrasOff -> QualityEventKind.SUSPENDED
        else -> QualityEventKind.RESTORED
    }
    val undoable = reason == QualityReason.FRAME_DROPS && kind != QualityEventKind.RESTORED
    return QualityEvent(seq, reason, kind, old.level, new.level, undoable)
}

/** -1 = off, else the tier (untiered presets count as 0). */
private fun rank(level: QualityLevel): Int = if (level.off) -1 else level.tier?.ordinal ?: 0
