// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.ui.look

import app.tatakai.next.model.CardSize

/**
 * 0.3.2: widths of the Home row cards for Diseño y estilo › Tamaño de tarjetas (desktop `media.css` `data-card-size`), in dp.
 * Pure so the phone math is unit-tested: on a phone a [CardSize.LARGE] card shows about 1.3 cards per screen width and an
 * [CardSize.XL] one about 1 card (the next one peeks); wide windows (>= 600 dp) use fixed widths (about 3-5 large cards per row).
 */
object CardSizing {
    /** Width of the previous (medium) poster card. */
    const val MEDIUM_DP = 120f

    /** Desktop small / medium = 118 / 150 px. */
    const val SMALL_DP = 96f

    /** Compact density packs poster cards ~13 % narrower (as before). */
    const val COMPACT_FACTOR = 0.87f

    /** Row content padding (start) and gap between cards. */
    const val ROW_PADDING_DP = 16f
    const val ROW_GAP_DP = 12f

    /** Wide windows (tablets, landscape, desktop mode): fixed widths. */
    const val WIDE_WINDOW_DP = 600f
    const val LARGE_WIDE_WINDOW_DP = 320f
    const val XL_WIDE_WINDOW_DP = 420f

    /** Poster width of [size] (SMALL / MEDIUM); the wide sizes return their [wideCardDp]. */
    fun posterDp(size: CardSize, compact: Boolean): Float {
        val base = when (size) {
            CardSize.SMALL -> SMALL_DP
            else -> MEDIUM_DP
        }
        return if (compact) base * COMPACT_FACTOR else base
    }

    /**
     * Width of a horizontal detailed card ([CardSize.LARGE] / [CardSize.XL]) in a row [viewportDp] wide. Phones: LARGE so that
     * (viewport − padding) holds 1.3 cards with their gaps, XL one card with the next one peeking by ~32 dp.
     */
    fun wideCardDp(size: CardSize, viewportDp: Float): Float {
        if (viewportDp >= WIDE_WINDOW_DP) return if (size == CardSize.XL) XL_WIDE_WINDOW_DP else LARGE_WIDE_WINDOW_DP
        val usable = viewportDp - ROW_PADDING_DP
        return when (size) {
            CardSize.XL -> (usable - ROW_GAP_DP - 32f).coerceIn(260f, XL_WIDE_WINDOW_DP)
            else -> (usable / 1.3f - ROW_GAP_DP).coerceIn(220f, LARGE_WIDE_WINDOW_DP)
        }
    }

    /** Cards visible in a row of [viewportDp] at [cardDp] (for tests / docs). */
    fun visibleCards(viewportDp: Float, cardDp: Float): Float = (viewportDp - ROW_PADDING_DP) / (cardDp + ROW_GAP_DP)

    /** Poster width inside a wide card: about a third of the card. */
    fun widePosterDp(size: CardSize): Float = if (size == CardSize.XL) 124f else 100f
}

/** What "hace X" says for an event [secondsAgo] seconds in the past (pure; future or 0 = [Now]). */
sealed interface Ago {
    data object Now : Ago
    data class Minutes(val n: Int) : Ago
    data class Hours(val n: Int) : Ago

    /** Days (>= 1). */
    data class Days(val n: Int) : Ago

    companion object {
        fun of(secondsAgo: Long): Ago = when {
            secondsAgo < 60 -> Now
            secondsAgo < 3_600 -> Minutes((secondsAgo / 60).toInt())
            secondsAgo < 86_400 -> Hours((secondsAgo / 3_600).toInt())
            else -> Days((secondsAgo / 86_400).toInt())
        }
    }
}
