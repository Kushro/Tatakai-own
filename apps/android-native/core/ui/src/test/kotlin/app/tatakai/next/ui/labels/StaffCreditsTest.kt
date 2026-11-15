// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.ui.labels

import app.tatakai.next.model.StaffEdge
import app.tatakai.next.model.StaffRef
import java.util.Locale
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** BUG-133: novel staff roles without AniList's notes, translators grouped by language. */
class StaffCreditsTest {
    private fun e(name: String, role: String?) = StaffEdge(StaffRef(name.hashCode(), name), role)

    @Test
    fun translatorRolesKeepTheirLanguageAndDropTheVolumes() {
        assertEquals(StaffRoleParts("Translator", "en"), parseStaffRole("Translator (English: vols 1, 3)"))
        assertEquals(StaffRoleParts("Translator", "th"), parseStaffRole("Translator (Thai)"))
        assertEquals(StaffRoleParts("Translator", "es"), parseStaffRole(" Translator (Spanish: vol 1) "))
        assertEquals(StaffRoleParts("Translator", null), parseStaffRole("Translator"))
        assertEquals(StaffRoleParts("Translator", null), parseStaffRole("Translator (vols 2-4)"))
        assertTrue(parseStaffRole("Translator (English: vol 1)").translation)
        // other roles: base only, the note is dropped
        assertEquals(StaffRoleParts("Story"), parseStaffRole("Story (ch 1-20)"))
        assertFalse(parseStaffRole("Illustration").translation)
    }

    @Test
    fun languageNamesAreInTheAppsLanguage() {
        val es = Locale.forLanguageTag("es")
        assertEquals("inglés", languageName("en", es))
        assertEquals("tailandés", languageName("th", es))
        assertEquals("español", languageName("es", es))
        assertEquals("English", languageName("en", Locale.ENGLISH))
        assertNull(languageCodeOf("Klingonese"))
    }

    @Test
    fun mushokuTenseiSplitsAuthorsAndTranslators() {
        // r7 (BUG-133): what AniList returns for Mushoku Tensei: Jobless Reincarnation
        val credits = staffCredits(
            listOf(
                e("Rifujin na Magonote", "Story"),
                e("Sirotaka", "Illustration"),
                e("Kevin Frane", "Translator (English: vol 1)"),
                e("Paul Cuneo", "Translator (English: vols 1, 3, 5, 7, 9, 11, 13, 15, 17, 19)"),
                e("Alyssa Orton-Niioka", "Translator (English: vols 2, 4, 6-8, 10, 12, 14, 16, 18, 19, 23)"),
                e("Sylvia Gallagher", "Translator (English: vols 9-10)"),
                e("airco", "Translator (English)"),
                e("Soraphong Kitbunchaporn", "Translator (Thai)"),
            ),
        )
        assertEquals(listOf(StaffCredit("Rifujin na Magonote", listOf("Story")), StaffCredit("Sirotaka", listOf("Illustration"))), credits.authors)
        assertEquals(
            listOf(
                TranslationCredit("en", listOf("Kevin Frane", "Paul Cuneo", "Alyssa Orton-Niioka", "Sylvia Gallagher", "airco")),
                TranslationCredit("th", listOf("Soraphong Kitbunchaporn")),
            ),
            credits.translations,
        )
        assertEquals("Kevin Frane, Paul Cuneo, Alyssa Orton-Niioka" to 2, shortNameList(credits.translations[0].names))
        assertEquals("Soraphong Kitbunchaporn" to 0, shortNameList(credits.translations[1].names))
    }

    @Test
    fun aPersonWithTwoRolesIsListedOnceAndUnknownLanguagesGoLast() {
        val credits = staffCredits(
            listOf(
                e("X", "Story"),
                e("Y", "Translator"),
                e("X", "Illustration"),
                e("X", "Story (vol 2)"),
                e("Z", "Translator (Spanish: vol 1)"),
                e("Z", "Translator (Spanish: vol 2)"),
            ),
        )
        assertEquals(listOf(StaffCredit("X", listOf("Story", "Illustration"))), credits.authors)
        assertEquals(listOf(TranslationCredit("es", listOf("Z")), TranslationCredit(null, listOf("Y"))), credits.translations)
    }
}
