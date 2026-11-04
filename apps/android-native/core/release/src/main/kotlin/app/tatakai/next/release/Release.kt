// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.release

import app.tatakai.next.model.ReleaseTags
import java.util.Locale

// Port of apps/desktop-native/crates/tk-core/src/release.rs (docs/redesign/10 §3, 11 §2.3, 12 §3.1). Signatures FINAL; bodies: worker a.
// Oracle: contracts/fixtures/release/{names,packs}.json, generated from the Rust tests (same results in Kotlin and Rust).

/** One file of a torrent as [pickEpisodeFile] sees it. [index] is the torrent's file index (returned as is). */
data class CandidateFile(val index: Int, val path: String, val sizeBytes: Long)

/** Parse a torrent or file name (anitomy-like, dependency-free). Never throws; unknown parts stay null / false. */
fun parseRelease(name: String): ReleaseTags = parseReleaseImpl(name)

/** Video extensions a batch file can have: mkv mp4 avi webm m2ts ts mov wmv flv (same list as Rust). Implemented here (contract). */
fun isVideoPath(path: String): Boolean {
    val dot = path.lastIndexOf('.')
    if (dot < 0) return false
    return path.substring(dot + 1).lowercase(Locale.ROOT) in VIDEO_EXTENSIONS
}

private val VIDEO_EXTENSIONS = setOf("mkv", "mp4", "avi", "webm", "m2ts", "ts", "mov", "wmv", "flv")

/**
 * The file of [files] holding [episode] (1-based) of the series known by [titles] (main title first, then synonyms); skips non-video,
 * samples, NCOP/NCED/PV/CM/Extras/SP/OVA unless asked; S02E03 vs absolute numbering; ties -> largest file. Returns [CandidateFile.index]
 * or null (the UI then opens the Files tab of the torrent).
 */
fun pickEpisodeFile(files: List<CandidateFile>, episode: Int, titles: List<String>): Int? =
    pickEpisodeFileImpl(files, episode, titles)
