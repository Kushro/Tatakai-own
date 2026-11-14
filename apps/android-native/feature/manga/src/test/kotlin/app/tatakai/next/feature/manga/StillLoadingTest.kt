// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.manga

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** Desktop BUG-121 parity: Manga › Discover says why it is still in skeletons. */
class StillLoadingTest {
    @Test
    fun theHintAppearsAtSixSecondsAndExplainsAtTwenty() {
        assertNull(stillLoadingRes(0))
        assertNull(stillLoadingRes(5_999))
        assertEquals(R.string.manga_home_still_loading, stillLoadingRes(6_000))
        assertEquals(R.string.manga_home_still_loading_slow, stillLoadingRes(20_000))
    }
}
