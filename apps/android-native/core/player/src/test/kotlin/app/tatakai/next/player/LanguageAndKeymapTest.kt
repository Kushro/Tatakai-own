// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

// parity wave A (15 §5 p (g)): language normalisation (bug player.subs.preferred-lang) and PlayerKeymap (defaults, conflicts, reset).
class LanguageAndKeymapTest {
    @Test
    fun normalisesTagsAndNames() {
        listOf("es", "es-419", "es_ES", "spa", "Spanish", "Español", "Spanish (Latin America)", "[Castellano]", "ESL").forEach {
            assertEquals("es", LanguageCodes.normalize(it), it)
        }
        assertEquals("ja", LanguageCodes.normalize("jpn"))
        assertEquals("ja", LanguageCodes.normalize("jp"))
        assertEquals("pt", LanguageCodes.normalize("pt-BR"))
        assertEquals("en", LanguageCodes.normalize("English"))
        assertNull(LanguageCodes.normalize("und"))
        assertNull(LanguageCodes.normalize(""))
        assertNull(LanguageCodes.normalize(null))
    }

    @Test
    fun engineVariantsCoverRegionalAndIso3() {
        val v = LanguageCodes.engineVariants("es")
        assertEquals("es-419", v.first())
        assertTrue("es" in v && "spa" in v)
        assertTrue(LanguageCodes.engineVariants(null).isEmpty())
    }

    @Test
    fun firstMatchFindsTheEs419Track() {
        data class T(val lang: String?, val label: String?)
        val tracks = listOf(T("en", "English"), T("es-419", "Español (Latinoamérica)"), T("es", "Español"))
        assertEquals(1, LanguageCodes.firstMatch(tracks, "es", { it.lang }, { it.label }))
        assertNull(LanguageCodes.firstMatch(tracks, "fr", { it.lang }, { it.label }))
        assertTrue(LanguageCodes.matches(null, "Spanish", "es"))
        assertFalse(LanguageCodes.matches("ru", "Russian", "es"))
    }

    @Test
    fun keymapDefaultsAreTheClassicOnes() {
        val map = PlayerKeymap.resolve(emptyMap())
        assertEquals("SPACE", map[PlayerKeymap.PLAY_PAUSE])
        assertEquals("M", map[PlayerKeymap.MUTE])
        assertEquals("P", map[PlayerKeymap.PIP])
        assertEquals("J", map[PlayerKeymap.PREV_EPISODE])
        assertEquals("K", map[PlayerKeymap.NEXT_EPISODE])
        assertEquals("DPAD_UP", map[PlayerKeymap.VOLUME_UP])
        assertEquals("S", map[PlayerKeymap.SCREENSHOT])
        assertEquals(PlayerKeymap.ACTIONS.size, map.size)
        assertTrue(PlayerKeymap.conflicts(emptyMap()).isEmpty())
        assertEquals(PlayerKeymap.FULLSCREEN, PlayerKeymap.actionFor("f", emptyMap()))
    }

    @Test
    fun bindDetectsConflictsAndCompacts() {
        val custom = PlayerKeymap.bind(emptyMap(), PlayerKeymap.MUTE, "f")
        assertEquals(mapOf(PlayerKeymap.MUTE to "F"), custom)
        assertEquals(listOf(PlayerKeymap.FULLSCREEN, PlayerKeymap.MUTE), PlayerKeymap.conflicts(custom)["F"])
        assertEquals(listOf(PlayerKeymap.FULLSCREEN), PlayerKeymap.conflictsWith(emptyMap(), PlayerKeymap.MUTE, "F"))
        // Binding back to the default drops the entry; reserved keys and unknown actions are refused.
        assertTrue(PlayerKeymap.bind(custom, PlayerKeymap.MUTE, "M").isEmpty())
        assertEquals(custom, PlayerKeymap.bind(custom, PlayerKeymap.PIP, "BACK"))
        assertEquals(custom, PlayerKeymap.bind(custom, "nope", "Q"))
        assertTrue(PlayerKeymap.reset().isEmpty())
        assertEquals("SPACE", PlayerKeymap.keyName("KEYCODE_SPACE"))
        assertEquals("←", PlayerKeymap.displayName("DPAD_LEFT"))
    }
}
