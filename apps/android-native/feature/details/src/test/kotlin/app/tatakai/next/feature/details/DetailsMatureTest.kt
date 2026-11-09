// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.details

import app.tatakai.next.model.MatureContentMode
import kotlin.test.Test
import kotlin.test.assertEquals

class DetailsMatureTest {
    @Test
    fun `non adult titles are never gated or blurred`() {
        MatureContentMode.entries.forEach { mode ->
            assertEquals(AdultPresentation.NONE, adultPresentation(mode, adult = false, accepted = false))
        }
    }

    @Test
    fun `show mode opens adult titles directly`() {
        assertEquals(AdultPresentation.NONE, adultPresentation(MatureContentMode.SHOW, adult = true, accepted = false))
    }

    @Test
    fun `blur mode blurs instead of asking`() {
        assertEquals(AdultPresentation.BLUR, adultPresentation(MatureContentMode.BLUR, adult = true, accepted = false))
        assertEquals(AdultPresentation.BLUR, adultPresentation(MatureContentMode.BLUR, adult = true, accepted = true))
    }

    @Test
    fun `warn mode asks once per title`() {
        assertEquals(AdultPresentation.GATE, adultPresentation(MatureContentMode.WARN, adult = true, accepted = false))
        assertEquals(AdultPresentation.NONE, adultPresentation(MatureContentMode.WARN, adult = true, accepted = true))
    }
}
