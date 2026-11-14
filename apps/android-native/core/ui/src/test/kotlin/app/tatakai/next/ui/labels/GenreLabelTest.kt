// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.ui.labels

import app.tatakai.next.ui.R
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class GenreLabelTest {
    @Test
    fun aniListGenresMapToTheDesktopKeys() {
        // BUG-072: Detalles showed "Action, Adventure, Comedy" on the phone; the desktop says "Acción · Aventura · Comedia".
        assertEquals(R.string.genre_action, genreLabelRes("Action"))
        assertEquals(R.string.genre_sci_fi, genreLabelRes("Sci-Fi"))
        assertEquals(R.string.genre_slice_of_life, genreLabelRes("Slice of Life"))
        assertEquals(R.string.genre_mahou_shoujo, genreLabelRes("Mahou Shoujo"))
        assertNull(genreLabelRes("Isekai"))
    }
}
