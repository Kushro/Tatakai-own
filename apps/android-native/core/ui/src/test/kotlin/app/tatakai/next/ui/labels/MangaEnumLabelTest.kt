// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.ui.labels

import app.tatakai.next.ui.R
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

/** BUG-098: the manga page showed "Fuente original: Other", "(Story) / (Art)" and "Main / Supporting" in English. */
class MangaEnumLabelTest {
    @Test
    fun sourcesRolesAndStaffRolesHaveLabels() {
        assertEquals(R.string.media_source_other, mediaSourceLabelRes("OTHER"))
        assertEquals(R.string.media_source_light_novel, mediaSourceLabelRes("LIGHT_NOVEL"))
        assertEquals(R.string.media_source_other, mediaSourceLabelRes("SOMETHING_NEW"))
        assertEquals(R.string.character_role_main, characterRoleLabelRes("MAIN"))
        assertEquals(R.string.character_role_supporting, characterRoleLabelRes("Supporting"))
        assertNull(characterRoleLabelRes("CAMEO"))
        assertEquals(R.string.staff_role_story, staffRoleLabelRes("Story"))
        assertEquals(R.string.staff_role_story_art, staffRoleLabelRes("Story & Art"))
        assertEquals(R.string.staff_role_original_story, staffRoleLabelRes("Original Story"))
        assertEquals(R.string.staff_role_art, staffRoleLabelRes("Art (ch 1-20)"))
        assertNull(staffRoleLabelRes("Touch-up Art & Lettering"))
    }
}
