// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.search

import kotlin.test.Test
import kotlin.test.assertEquals

/** parity wave B (worker r): the provider switches of Settings › Metadatos. */
class ProviderToggleTest {
    @Test fun enablingAppendsAndDisablingRemoves() {
        val p = listOf("anilist", "anizip", "kitsu")
        assertEquals(listOf("anilist", "anizip", "kitsu", "mal"), ProviderToggle.apply(p, "mal", true))
        assertEquals(p, ProviderToggle.apply(p, "anilist", true))
        assertEquals(listOf("anilist", "kitsu"), ProviderToggle.apply(p, "anizip", false))
    }

    @Test fun neverEmptiesThePriority() {
        assertEquals(listOf("anilist"), ProviderToggle.apply(listOf("anilist"), "anilist", false))
    }

    @Test fun cooldownMinutesRoundUp() {
        assertEquals(1, ProviderToggle.minutesLeft(10_000, 0))
        assertEquals(5, ProviderToggle.minutesLeft(4 * 60_000 + 1, 0))
    }
}
