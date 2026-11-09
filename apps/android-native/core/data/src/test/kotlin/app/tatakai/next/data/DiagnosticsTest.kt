// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.data

import app.tatakai.next.model.LogLevel
import kotlinx.coroutines.runBlocking
import org.junit.Rule
import org.junit.rules.TemporaryFolder
import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** parity wave A (worker q): app log rotation / scrubbing and the new-version comparison. */
class DiagnosticsTest {
    @get:Rule
    val tmp = TemporaryFolder()

    @Test
    fun logRotatesIntoOneBackupAndKeepsTheNewestLines() = runBlocking {
        val dir = File(tmp.root, "logs")
        val store = AppLogStore(dir, maxBytes = 1_000, mirror = false, clock = { 0L })
        repeat(60) { i -> store.append(AppLogStore.format(0L, LogLevel.INFO, "t", "line $i ".padEnd(40, 'x'), null)) }
        assertTrue(File(dir, AppLogStore.ROTATED).isFile)
        assertTrue(File(dir, AppLogStore.FILE).length() <= 1_000)
        val lines = store.recentLines(10)
        assertEquals(10, lines.size)
        assertTrue(lines.last().contains("line 59"), lines.last())
        assertFalse(dir.listFiles().orEmpty().any { it.name.startsWith("app.2") }, "only one backup is kept")
        store.clear()
        assertTrue(store.recentLines(10).isEmpty())
        assertNull(store.logFile())
    }

    @Test
    fun formatHasDateLevelAndTag() {
        val line = AppLogStore.format(0L, LogLevel.ERROR, "player", "boom", IllegalStateException("bad"))
        assertEquals("1970-01-01T00:00:00.000Z E/player: boom (IllegalStateException: bad)", line)
    }

    @Test
    fun scrubDropsQueriesTokensAndKeys() {
        val s = AppLogStore.scrub("GET https://api.example/x?token=abc&b=1 Authorization: Bearer eyJhbGci.x.y code=12345 api_key=zzz")
        assertFalse("abc" in s, s)
        assertFalse("eyJhbGci" in s, s)
        assertFalse("12345" in s, s)
        assertFalse("zzz" in s, s)
        assertTrue("https://api.example/x?…" in s, s)
    }

    @Test
    fun versionComparison() {
        assertTrue(UpdateRepositoryImpl.isNewer("1.10.0", "1.9.3"))
        assertTrue(UpdateRepositoryImpl.isNewer("v2.0", "1.99.99"))
        assertFalse(UpdateRepositoryImpl.isNewer("1.2.0", "1.2.0"))
        assertFalse(UpdateRepositoryImpl.isNewer("1.2.0-beta.1", "1.2.0"))
        assertTrue(UpdateRepositoryImpl.isNewer("1.2.0", "1.2.0-beta.1"))
        assertFalse(UpdateRepositoryImpl.isNewer("nightly", "1.0.0"))
        assertTrue(UpdateRepositoryImpl.isNewer("0.2.0", ""))
    }

    @Test
    fun releaseParsingNeedsAnApkAndSkipsPreReleases() {
        val ok = UpdateRepositoryImpl.parseRelease(
            """{"tag_name":"v0.3.0","draft":false,"prerelease":false,"html_url":"https://github.com/Kushro/Tatakai-own/releases/tag/v0.3.0",
              "published_at":"2026-11-01T00:00:00Z","body":"notes","assets":[{"name":"tatakai-next-0.3.0.apk"}]}""",
        )
        assertNotNull(ok)
        assertEquals("0.3.0", ok.version)
        assertEquals("notes", ok.notes)
        assertNull(UpdateRepositoryImpl.parseRelease("""{"tag_name":"v0.3.0","assets":[{"name":"setup.exe"}]}"""))
        assertNull(UpdateRepositoryImpl.parseRelease("""{"tag_name":"v0.4.0","prerelease":true,"assets":[{"name":"a.apk"}]}"""))
    }
}
