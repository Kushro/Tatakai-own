// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.release

import app.tatakai.next.model.ReleaseTags
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.int
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** contracts/fixtures/release/{names,packs}.json: the same oracle as the tk-core release.rs tests. */
class ReleaseFixturesTest {
    private val json = Json { encodeDefaults = true; explicitNulls = true }

    private fun encode(t: ReleaseTags): JsonObject = json.encodeToJsonElement(ReleaseTags.serializer(), t).jsonObject

    @Test
    fun names() {
        val cases = ContractFixtures.element("release", "names.json").jsonObject["cases"]!!.jsonArray
        assertTrue("at least 40 name cases", cases.size >= 40)
        val failures = mutableListOf<String>()
        for (c in cases) {
            val o = c.jsonObject
            val name = o["name"]!!.jsonPrimitive.content
            val expected = o["expected"]!!.jsonObject
            val actual = encode(parseRelease(name))
            for ((key, want) in expected) {
                val got = actual[key] ?: JsonNull
                if (got != want) failures += "\"$name\" ($key): expected $want, got $got"
            }
        }
        assertTrue(failures.joinToString("\n"), failures.isEmpty())
    }

    @Test
    fun packs() {
        val cases = ContractFixtures.element("release", "packs.json").jsonObject["cases"]!!.jsonArray
        assertTrue("at least 10 pack cases", cases.size >= 10)
        val failures = mutableListOf<String>()
        for (c in cases) {
            val o = c.jsonObject
            val label = o["label"]!!.jsonPrimitive.content
            val files = o["files"]!!.jsonArray.map {
                val f = it.jsonObject
                CandidateFile(f["index"]!!.jsonPrimitive.int, f["path"]!!.jsonPrimitive.content, f["sizeBytes"]!!.jsonPrimitive.long)
            }
            val titles = o["titles"]!!.jsonArray.map { it.jsonPrimitive.content }
            val episode = o["episode"]!!.jsonPrimitive.int
            val want = o["expected"]!!.jsonPrimitive.intOrNull
            val got = pickEpisodeFile(files, episode, titles)
            if (got != want) failures += "$label: expected $want, got $got"
        }
        assertTrue(failures.joinToString("\n"), failures.isEmpty())
    }

    @Test
    fun videoPaths() {
        assertTrue(isVideoPath("a/B.MKV"))
        assertEquals(false, isVideoPath("a/b.ass"))
        assertEquals(false, isVideoPath("noext"))
    }

    @Test
    fun neverThrowsAndNeverSetsSeadex() {
        for (name in listOf("", " ", "-", "[]", "[", "]", "()", "【】", "[Group]", "....", "s", "e", "- - -", "01-", "~12", "v2", "😀 - 05", "[😀]")) {
            assertEquals(false, parseRelease(name).seadexBest)
        }
    }
}
