package app.tatakai.next.data

import kotlin.test.Test
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class AniListAuthorizeUrlTest {
    @Test
    fun `implicit grant url carries no redirect_uri`() {
        // AniList answers `unsupported_grant_type` to an implicit-grant request with redirect_uri (it always uses the
        // client's registered URL), so linking on Android failed while the desktop (same URL minus the param) worked.
        val url = AniListListClient.authorizeUrl("56528", "s1")
        assertTrue(url.startsWith("https://anilist.co/api/v2/oauth/authorize?client_id=56528"), url)
        assertTrue("response_type=token" in url, url)
        assertTrue("state=s1" in url, url)
        assertFalse("redirect_uri" in url, url)
    }
}
