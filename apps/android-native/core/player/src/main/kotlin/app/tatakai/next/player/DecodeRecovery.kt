// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import app.tatakai.next.model.Container

// BUG-020 / BUG-042 (plan §11.2): what the player does when the engine reports a decoder error, before the source counts as failed.
// Pure, unit tested; the view-model applies it.

/** Media3 `PlaybackException` decoder errors (`ERROR_CODE_DECODER_INIT_FAILED` 4001 … `ERROR_CODE_DECODING_RESOURCES_RECLAIMED`). */
fun isDecoderError(code: Int): Boolean = code in 4000..4999

enum class DecodeRecovery {
    /** Reload the same source on the same engine at the same position, without telling the user (a transient decoder error). */
    RETRY_SAME,

    /** Reload the same source with libmpv (FFmpeg software decoding) at the same position and say so discreetly. */
    SWITCH_TO_MPV,

    /** Nothing more to try: the source fails as usual. */
    NONE,
}

/**
 * @param code the engine's error code (Media3 `errorCode`; mpv reports -1).
 * @param onMedia3 the error comes from the Media3 engine.
 * @param mpvAvailable libmpv can take over (bundled, and not a Link session, which needs Media3's mTLS client).
 * @param hardwareDecodable the source's tags say a hardware decoder handles it ([decodesInHardware]); unknown = true.
 * @param silentRetries silent retries already spent on this source.
 */
fun decodeRecovery(code: Int, onMedia3: Boolean, mpvAvailable: Boolean, hardwareDecodable: Boolean, silentRetries: Int): DecodeRecovery {
    if (!onMedia3 || !isDecoderError(code)) return DecodeRecovery.NONE
    // A video the device decodes in hardware failing once (e.g. right after a seek) is usually transient: retry it as "Retry" would.
    if (hardwareDecodable && silentRetries < MAX_SILENT_RETRIES) return DecodeRecovery.RETRY_SAME
    if (mpvAvailable) return DecodeRecovery.SWITCH_TO_MPV
    return if (silentRetries < MAX_SILENT_RETRIES) DecodeRecovery.RETRY_SAME else DecodeRecovery.NONE
}

const val MAX_SILENT_RETRIES = 1

// BUG-059: libmpv (FFmpeg's HLS demuxer) opens every rendition of a master playlist before the first frame and waits for all of them. A
// stream whose renditions are still being produced upstream (seen through the PC's extension-server proxy: 15 audio renditions, one
// without segments for ~95 s) leaves mpv at "0:00 / --:--" forever, while Media3 picks one rendition and plays. The mpv session gives
// up after [MPV_START_TIMEOUT_MS] with [MPV_START_TIMEOUT_CODE]; the view-model then plays the same source with Media3.

/** [PlaybackError.code] of an mpv load that produced no frame within [MPV_START_TIMEOUT_MS]. */
const val MPV_START_TIMEOUT_CODE = -2

/** How long a network (non-torrent) stream may take in libmpv before its first frame. */
const val MPV_START_TIMEOUT_MS = 15_000L

/** Whether an mpv load of a source of [container] (a torrent loopback stream when [torrent]) is watched by [MPV_START_TIMEOUT_MS]. */
fun mpvStartWatched(container: Container, torrent: Boolean): Boolean =
    !torrent && container != Container.PROGRESSIVE

/**
 * Whether an mpv error moves the same source to Media3 (BUG-059): only the start timeout, outside a Link session (those already play in
 * Media3), with Media3 not tried yet for this source.
 */
fun switchToMedia3OnStall(code: Int, onMpv: Boolean, remote: Boolean, triedMedia3: Boolean): Boolean =
    onMpv && code == MPV_START_TIMEOUT_CODE && !remote && !triedMedia3
