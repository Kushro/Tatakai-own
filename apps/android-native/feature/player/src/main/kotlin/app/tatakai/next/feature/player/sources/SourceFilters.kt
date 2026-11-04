// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.player.sources

import app.tatakai.next.model.Quality
import app.tatakai.next.model.RankedSource
import app.tatakai.next.model.ReleaseSource
import app.tatakai.next.model.SourceKind
import app.tatakai.next.model.VideoCodec
import app.tatakai.next.player.CodecCaps
import app.tatakai.next.player.isDeviceCompatible

// android wave (docs/redesign/11 §3.6, 12 §3.11): filter state of the source picker, ported 1:1 from desktop
// apps/desktop-native/crates/tk-ui/src/components/sources.rs (SourceFilters, FilterOptions, tag_chips) plus the Android-only
// "Compatible with this device" filter. Filtering NEVER reorders: the order is the ranking. Pure, unit-tested.

/** Reliability floor of the filter, on the reliability tone thresholds. */
enum class RelFloor(val min: Float) {
    /** `>= 0.7` (medium or high). */
    MID(0.7f),

    /** `>= 0.9`. */
    HIGH(0.9f),
}

/** Resolutions the picker can filter by, best first. */
val FILTER_RESOLUTIONS: List<Quality> = listOf(Quality.P2160, Quality.P1440, Quality.P1080, Quality.P720, Quality.P480)

/** Empty sets = no constraint (a set is "any of"); a `true` flag = "must have". */
data class SourceFilters(
    val kind: SourceKind? = null,
    val sources: Set<ReleaseSource> = emptySet(),
    val resolutions: Set<Quality> = emptySet(),
    val codecs: Set<VideoCodec> = emptySet(),
    val tenBit: Boolean = false,
    val dualAudio: Boolean = false,
    val multiSub: Boolean = false,
    val batch: Boolean = false,
    val seadexBest: Boolean = false,
    val minReliability: RelFloor? = null,
    /** Android only: hide what the device cannot decode (HEVC / AV1 10-bit). */
    val deviceCompatible: Boolean = false,
) {
    fun matches(s: RankedSource, caps: CodecCaps = CodecCaps.ALL): Boolean {
        val c = s.candidate
        val tags = c.tags
        return (kind == null || c.kind == kind) &&
            (sources.isEmpty() || tags.source in sources) &&
            (resolutions.isEmpty() || c.quality in resolutions) &&
            (codecs.isEmpty() || tags.codec in codecs) &&
            (!tenBit || tags.bitDepth == 10) &&
            (!dualAudio || tags.dualAudio) &&
            (!multiSub || tags.multiSub) &&
            (!batch || tags.batch) &&
            (!seadexBest || tags.seadexBest) &&
            (minReliability == null || c.reliability >= minReliability.min) &&
            (!deviceCompatible || isDeviceCompatible(tags, caps))
    }

    /** The sources that pass, in the given (ranked) order. */
    fun apply(list: List<RankedSource>, caps: CodecCaps = CodecCaps.ALL): List<RankedSource> = list.filter { matches(it, caps) }

    /** Active chip filters (the kind segment is not counted: it is always visible). */
    fun active(): Int =
        sources.size + resolutions.size + codecs.size +
            listOf(tenBit, dualAudio, multiSub, batch, seadexBest, deviceCompatible).count { it } +
            (if (minReliability != null) 1 else 0)

    fun toggleFloor(floor: RelFloor): SourceFilters = copy(minReliability = if (minReliability == floor) null else floor)

    /** Everything but the kind segment back to default. */
    fun cleared(): SourceFilters = SourceFilters(kind = kind)

    /** Encoding for `rememberSaveable` (a flat list of strings). */
    fun encode(): List<String> = buildList {
        add("kind=${kind?.name ?: ""}")
        sources.forEach { add("src=${it.name}") }
        resolutions.forEach { add("res=${it.name}") }
        codecs.forEach { add("codec=${it.name}") }
        if (tenBit) add("tenBit")
        if (dualAudio) add("dualAudio")
        if (multiSub) add("multiSub")
        if (batch) add("batch")
        if (seadexBest) add("seadexBest")
        if (deviceCompatible) add("device")
        minReliability?.let { add("rel=${it.name}") }
    }

    companion object {
        fun decode(items: List<String>): SourceFilters {
            var f = SourceFilters()
            for (item in items) {
                val (k, v) = item.split('=', limit = 2).let { it[0] to it.getOrElse(1) { "" } }
                f = when (k) {
                    "kind" -> f.copy(kind = SourceKind.entries.firstOrNull { it.name == v })
                    "src" -> ReleaseSource.entries.firstOrNull { it.name == v }?.let { f.copy(sources = f.sources + it) } ?: f
                    "res" -> Quality.entries.firstOrNull { it.name == v }?.let { f.copy(resolutions = f.resolutions + it) } ?: f
                    "codec" -> VideoCodec.entries.firstOrNull { it.name == v }?.let { f.copy(codecs = f.codecs + it) } ?: f
                    "tenBit" -> f.copy(tenBit = true)
                    "dualAudio" -> f.copy(dualAudio = true)
                    "multiSub" -> f.copy(multiSub = true)
                    "batch" -> f.copy(batch = true)
                    "seadexBest" -> f.copy(seadexBest = true)
                    "device" -> f.copy(deviceCompatible = true)
                    "rel" -> f.copy(minReliability = RelFloor.entries.firstOrNull { it.name == v })
                    else -> f
                }
            }
            return f
        }
    }
}

fun <T> Set<T>.toggled(v: T): Set<T> = if (v in this) this - v else this + v

/** What the current list offers, so the filter bar only draws chips that can match something. */
data class FilterOptions(
    val sources: List<ReleaseSource> = emptyList(),
    val resolutions: List<Quality> = emptyList(),
    val codecs: List<VideoCodec> = emptyList(),
    val tenBit: Boolean = false,
    val dualAudio: Boolean = false,
    val multiSub: Boolean = false,
    val batch: Boolean = false,
    val seadexBest: Boolean = false,
    /** Some source would be hidden by the device filter. */
    val deviceIncompatible: Boolean = false,
) {
    val anyFeature: Boolean get() = tenBit || dualAudio || multiSub || batch || seadexBest

    companion object {
        fun of(list: List<RankedSource>, caps: CodecCaps = CodecCaps.ALL): FilterOptions {
            val tags = list.map { it.candidate.tags }
            return FilterOptions(
                sources = ReleaseSource.entries.filter { x -> tags.any { it.source == x } },
                resolutions = FILTER_RESOLUTIONS.filter { q -> list.any { it.candidate.quality == q } },
                codecs = VideoCodec.entries.filter { x -> tags.any { it.codec == x } },
                tenBit = tags.any { it.bitDepth == 10 },
                dualAudio = tags.any { it.dualAudio },
                multiSub = tags.any { it.multiSub },
                batch = tags.any { it.batch },
                seadexBest = tags.any { it.seadexBest },
                deviceIncompatible = tags.any { !isDeviceCompatible(it, caps) },
            )
        }
    }
}

/** Reliability tone of a row: >= 0.9 high (OK), >= 0.7 medium (WARN), else low (ERR). */
enum class RelTone { HIGH, MID, LOW }

fun reliabilityTone(r: Float): RelTone = when {
    r >= 0.9f -> RelTone.HIGH
    r >= 0.7f -> RelTone.MID
    else -> RelTone.LOW
}

/** One release-tag chip of a row; texts of technical codes (`BD`, `HEVC`, `1080p`) are not translated. */
sealed interface TagChip {
    data object SeadexBest : TagChip
    data class Group(val name: String) : TagChip
    data class Code(val text: String) : TagChip
    data class BitDepth(val bits: Int) : TagChip
    data object DualAudio : TagChip
    data object MultiSub : TagChip
    data class Batch(val from: Int?, val to: Int?) : TagChip
}

/** Desktop `tag_chips`: display order SeaDex best, group (when it differs from the label), source, quality, codec, bit depth, ... */
fun tagChips(s: RankedSource): List<TagChip> {
    val c = s.candidate
    val tags = c.tags
    val torrent = c.kind == SourceKind.TORRENT
    val out = mutableListOf<TagChip>()
    if (tags.seadexBest) out += TagChip.SeadexBest
    tags.group?.takeIf { it.isNotEmpty() && it != c.label }?.let { out += TagChip.Group(it) }
    tags.source?.let { out += TagChip.Code(it.label()) }
    if (torrent && c.quality != Quality.UNKNOWN) out += TagChip.Code(c.quality.label())
    val codec = tags.codec
    if (codec != null) {
        out += TagChip.Code(codec.label())
    } else if (torrent) {
        c.container?.takeIf { it.isNotEmpty() }?.let { out += TagChip.Code(it) }
    }
    tags.bitDepth?.let { out += TagChip.BitDepth(it) }
    if (tags.dualAudio) out += TagChip.DualAudio
    if (tags.multiSub) out += TagChip.MultiSub
    if (tags.batch) {
        val e = tags.episodes
        out += if (e != null && e.last > e.first) TagChip.Batch(e.first, e.last) else TagChip.Batch(null, null)
    }
    return out
}

/** `es, en` -> `ES · EN`; empty -> null (the row then prints "none"). */
fun langsLabel(list: List<String>): String? = list.takeIf { it.isNotEmpty() }?.joinToString(" · ") { it.uppercase() }
