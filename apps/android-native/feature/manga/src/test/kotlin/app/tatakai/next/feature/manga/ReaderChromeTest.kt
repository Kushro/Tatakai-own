// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.manga

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** BUG-127: in landscape the bars took ~48 % of the height and stayed on every page turn. */
class ReaderChromeTest {
    @Test
    fun landscapeHidesTheBarsOnRotationAndOnEveryPageTurn() {
        assertTrue(ReaderChrome.hideOnRotate(landscape = true))
        assertFalse(ReaderChrome.hideOnRotate(landscape = false))
        assertTrue(ReaderChrome.hideOnPageTurn(landscape = true, previousPage = 3, page = 4))
        assertTrue(ReaderChrome.hideOnPageTurn(landscape = true, previousPage = 4, page = 3))
        assertFalse("same page: nothing to hide", ReaderChrome.hideOnPageTurn(landscape = true, previousPage = 4, page = 4))
        assertFalse("portrait keeps them until the centre tap", ReaderChrome.hideOnPageTurn(landscape = false, previousPage = 3, page = 4))
    }
}
