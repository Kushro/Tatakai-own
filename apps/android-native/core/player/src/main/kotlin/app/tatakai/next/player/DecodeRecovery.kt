// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

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
