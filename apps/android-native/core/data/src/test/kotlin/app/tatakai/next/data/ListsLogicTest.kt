// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.data

import app.tatakai.next.model.ListEntry
import app.tatakai.next.model.ListProvider
import app.tatakai.next.model.ListStatus
import app.tatakai.next.model.MediaRef
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** parity wave A (worker q, 15 §5 q (i)): OAuth redirect, PKCE, AniList / MAL mapping, merge rule and import / export. */
class ListsLogicTest {

    private fun resource(name: String): String =
        requireNotNull(javaClass.classLoader?.getResource("lists/$name")) { "missing fixture $name" }.readText()

    // ------------------------------------------------------------------------------------------------ OAuth

    @Test
    fun providerOfAcceptsOnlyOurRedirects() {
        assertEquals(ListProvider.ANILIST, OAuthRedirect.providerOf("tatakai://oauth/anilist?code=abc&state=xyz"))
        assertEquals(ListProvider.MAL, OAuthRedirect.providerOf("TATAKAI://OAUTH/mal#access_token=1"))
        assertNull(OAuthRedirect.providerOf("tatakai://oauth/kitsu?code=1"))
        assertNull(OAuthRedirect.providerOf("https://evil.example/oauth/anilist"))
        assertNull(OAuthRedirect.providerOf("tatakai://other/anilist"))
        assertEquals("tatakai://oauth/mal", OAuthRedirect.redirectUri(ListProvider.MAL))
    }

    @Test
    fun paramsDecodeQueryAndFragmentFirstValueWins() {
        val p = OAuthRedirect.params("tatakai://oauth/anilist?code=a%2Bb&state=s1&state=s2#access_token=t%20k&expires_in=3600")
        assertEquals("a+b", p["code"])
        assertEquals("s1", p["state"])
        assertEquals("t k", p["access_token"])
        assertEquals("3600", p["expires_in"])
        assertTrue(OAuthRedirect.params("tatakai://oauth/mal").isEmpty())
    }

    @Test
    fun pkcePlainVerifierIsItsOwnChallenge() {
        val v = Pkce.verifier()
        assertEquals(64, v.length)
        assertTrue(Pkce.isValidVerifier(v))
        assertEquals(v, Pkce.plainChallenge(v))
        assertFalse(Pkce.isValidVerifier("short"))
        assertFalse(Pkce.isValidVerifier("a".repeat(42) + " "))
        assertTrue(Pkce.state() != Pkce.state(), "state must be random")
    }

    @Test
    fun pendingOAuthExpires() {
        val p = PendingOAuth("mal", "s", "v", createdAtMs = 1_000_000)
        assertFalse(p.isExpired(1_000_000 + 60_000))
        assertTrue(p.isExpired(1_000_000 + 24L * 3600 * 1000))
    }

    @Test
    fun tokenSetExpiryUsesSkew() {
        val t = TokenSet("a", "r", expiresAtMs = 100_000)
        assertFalse(t.isExpired(nowMs = 10_000))
        assertTrue(t.isExpired(nowMs = 50_000))
        assertFalse(TokenSet("a").isExpired(Long.MAX_VALUE / 2))
    }

    // ------------------------------------------------------------------------------------------------ mapping

    @Test
    fun aniListCollectionMapsAndDeduplicates() {
        val (entries, more) = parseAniListCollection(jsonObjectOf(resource("anilist-collection.json")))
        assertFalse(more)
        assertEquals(2, entries.size)
        val frieren = entries.first { it.ref.anilistId == 154587 }
        assertEquals(MediaRef(anilistId = 154587, malId = 52991), frieren.ref)
        assertEquals("Sousou no Frieren", frieren.title)
        assertEquals(ListStatus.WATCHING, frieren.status)
        assertEquals(7, frieren.progress)
        assertEquals(28, frieren.totalEpisodes)
        assertEquals(85, frieren.score)
        assertTrue(frieren.favorite)
        assertEquals(1762300000L * 1000, frieren.updatedAtMs)
        assertEquals("2026-10-03", frieren.startedAt)
        assertNull(frieren.completedAt)
        assertNull(frieren.notes)
        val op = entries.first { it.ref.anilistId == 21 }
        assertEquals(ListStatus.REPEATING, op.status)
        assertNull(op.score)
        assertEquals("https://s4.anilist.co/op.jpg", op.poster)
    }

    @Test
    fun malPageMapsStatusesScoresAndRewatching() {
        val (entries, next) = parseMalPage(jsonObjectOf(resource("mal-page.json")))
        assertEquals("https://api.myanimelist.net/v2/users/@me/animelist?offset=100", next)
        assertEquals(3, entries.size)
        val frieren = entries[0]
        assertEquals(MediaRef(malId = 52991), frieren.ref)
        assertEquals(ListStatus.WATCHING, frieren.status)
        assertEquals(90, frieren.score)
        assertEquals("https://cdn.myanimelist.net/l.jpg", frieren.poster)
        assertEquals(java.time.Instant.parse("2026-11-05T10:00:00Z").toEpochMilli(), frieren.updatedAtMs)
        assertEquals(ListStatus.REPEATING, entries[1].status)
        assertNull(entries[1].totalEpisodes)
        assertEquals(ListStatus.PLANNING, entries[2].status)
    }

    @Test
    fun malNextPageNeverLeavesTheApiHost() {
        val (_, next) = parseMalPage(jsonObjectOf("""{"data":[],"paging":{"next":"https://evil.example/steal"}}"""))
        assertNull(next)
    }

    @Test
    fun statusMappingsRoundTrip() {
        ListStatus.entries.forEach { s ->
            assertEquals(s, aniListStatus(s.toAniList()))
            val (mal, rewatch) = s.toMal()
            assertEquals(s, malStatus(mal, rewatch))
        }
        assertEquals(9, toMalScore(85))
        assertEquals(0, toMalScore(null))
        assertEquals("4★", formatScore(80, "POINT_5"))
        assertEquals("8.5", formatScore(85, "POINT_10_DECIMAL"))
    }

    // ------------------------------------------------------------------------------------------------ merge rule (conflicts)

    private val watching3 = ListValues(ListStatus.WATCHING, 3, null)
    private val watching4 = ListValues(ListStatus.WATCHING, 4, null)
    private val completed = ListValues(ListStatus.COMPLETED, 12, 80)

    @Test
    fun threeWayMergeDecisions() {
        assertEquals(SyncAction.NOOP, decideSync(watching3, watching3, watching3, queued = false))
        assertEquals(SyncAction.PUSH, decideSync(watching4, watching3, watching3, queued = false))
        assertEquals(SyncAction.PULL, decideSync(watching3, watching4, watching3, queued = false))
        assertEquals(SyncAction.ADOPT, decideSync(watching4, watching4, watching3, queued = false))
        assertEquals(SyncAction.CONFLICT, decideSync(watching4, completed, watching3, queued = false))
    }

    @Test
    fun firstSyncMergeImportExport() {
        assertEquals(SyncAction.PUSH, decideSync(watching3, null, null, queued = false))
        assertEquals(SyncAction.PULL, decideSync(null, watching3, null, queued = false))
        assertEquals(SyncAction.CONFLICT, decideSync(watching3, completed, null, queued = false))
        assertEquals(SyncAction.PULL, decideSync(watching3, completed, null, queued = false, mode = InitialSyncMode.IMPORT))
        assertEquals(SyncAction.PUSH, decideSync(watching3, completed, null, queued = false, mode = InitialSyncMode.EXPORT))
        assertEquals(SyncAction.NOOP, decideSync(watching3, null, null, queued = false, mode = InitialSyncMode.IMPORT))
    }

    @Test
    fun queuedChangeWinsUnlessTheRemoteMovedToo() {
        assertEquals(SyncAction.PUSH, decideSync(watching4, watching3, watching3, queued = true))
        assertEquals(SyncAction.CONFLICT, decideSync(watching4, completed, watching3, queued = true))
        assertEquals(SyncAction.ADOPT, decideSync(watching4, watching4, watching3, queued = true))
    }

    @Test
    fun scoreStepHidesDifferencesTheAccountCannotStore() {
        val a = ListValues(ListStatus.WATCHING, 1, 85)
        val b = ListValues(ListStatus.WATCHING, 1, 80)
        assertFalse(sameValues(a, b, step = 1))
        assertTrue(sameValues(ListValues(ListStatus.WATCHING, 1, 81), b, step = scoreStep(ListProvider.MAL, null)))
    }

    // ------------------------------------------------------------------------------------------------ import / export

    private fun entry(id: Int, status: ListStatus, updated: Long) = ListEntry(
        ref = MediaRef(anilistId = id),
        title = "T$id",
        poster = null,
        status = status,
        progress = 2,
        totalEpisodes = 12,
        score = 70,
        favorite = id % 2 == 0,
        updatedAtMs = updated,
    )

    @Test
    fun exportImportRoundTripUsesTheDesktopShape() {
        val list = listOf(entry(1, ListStatus.WATCHING, 10), entry(2, ListStatus.REPEATING, 20))
        val json = exportEntries(list)
        assertTrue("\"status\":\"repeating\"" in json, json)
        assertTrue("\"updatedAtMs\":20" in json, json)
        assertEquals(list, parseExport(json))
        assertEquals(list, parseExport("""{"entries":$json}"""))
    }

    @Test
    fun importDropsEntriesWithoutIdsAndRejectsGarbage() {
        val parsed = parseExport("""[{"ref":{},"title":"x","poster":null,"status":"watching","updatedAtMs":1},{"bad":true}]""")
        assertTrue(parsed.isEmpty())
        assertFailsWith<IllegalArgumentException> { parseExport("\"text\"") }
    }

    @Test
    fun importReplacesOnlyNewer() {
        val incoming = entry(1, ListStatus.COMPLETED, updated = 50)
        assertTrue(shouldImport(null, incoming))
        val older = incoming.toRow("1").copy(updatedAtMs = 10)
        val newer = incoming.toRow("1").copy(updatedAtMs = 100)
        assertTrue(shouldImport(older, incoming))
        assertFalse(shouldImport(newer, incoming))
    }

    @Test
    fun errorCodesAreStable() {
        assertEquals("oauth:invalid_state", ListErrors.INVALID_STATE)
        assertTrue("${ListErrors.NO_CLIENT_ID}:mal".startsWith(ListErrors.NO_CLIENT_ID))
    }

    // ------------------------------------------------------------------------------------------------ first sync choice (lists.import-export)

    private fun row(key: String, status: String?, favorite: Boolean = false) =
        app.tatakai.next.database.ListEntryRow(key = key, anilistId = key.toIntOrNull(), malId = null, title = key, poster = null,
            status = status, favorite = favorite, updatedAtMs = 0L)

    private fun account(provider: ListProvider, lastSynced: Long?) =
        app.tatakai.next.database.LinkedAccountRow(provider = provider.id, userId = "1", username = "u", avatar = null, linkedAtMs = 0L,
            expiresAtMs = null, lastSyncedAtMs = lastSynced)

    @Test
    fun firstSyncChoiceOnlyWhenTheLocalListHasSomething() {
        assertFalse(needsFirstSyncChoice(emptyList()))
        assertFalse(needsFirstSyncChoice(listOf(row("1", status = null))))
        assertTrue(needsFirstSyncChoice(listOf(row("1", status = "watching"))))
        assertTrue(needsFirstSyncChoice(listOf(row("2", status = null, favorite = true))))
    }

    @Test
    fun pendingFirstSyncsAreTheNeverSyncedAccountsWithALocalList() {
        val accounts = listOf(account(ListProvider.ANILIST, lastSynced = null), account(ListProvider.MAL, lastSynced = 5L))
        assertEquals(setOf(ListProvider.ANILIST), pendingFirstSyncs(accounts, listOf(row("1", "planning"))))
        assertEquals(emptySet(), pendingFirstSyncs(accounts, emptyList()))
        assertEquals(emptySet(), pendingFirstSyncs(emptyList(), listOf(row("1", "planning"))))
    }

    @Test
    fun firstSyncModesDecideTheDisagreements() {
        val local = ListValues(status = ListStatus.WATCHING, progress = 3, score = null)
        val remote = ListValues(status = ListStatus.COMPLETED, progress = 12, score = 80)
        assertEquals(SyncAction.CONFLICT, decideSync(local, remote, null, queued = false, mode = InitialSyncMode.MERGE))
        assertEquals(SyncAction.PULL, decideSync(local, remote, null, queued = false, mode = InitialSyncMode.IMPORT))
        assertEquals(SyncAction.PUSH, decideSync(local, remote, null, queued = false, mode = InitialSyncMode.EXPORT))
        // local-only titles: kept local on import, pushed otherwise; remote-only titles always come down
        assertEquals(SyncAction.NOOP, decideSync(local, null, null, queued = false, mode = InitialSyncMode.IMPORT))
        assertEquals(SyncAction.PUSH, decideSync(local, null, null, queued = false, mode = InitialSyncMode.EXPORT))
        assertEquals(SyncAction.PULL, decideSync(null, remote, null, queued = false, mode = InitialSyncMode.EXPORT))
    }
}
