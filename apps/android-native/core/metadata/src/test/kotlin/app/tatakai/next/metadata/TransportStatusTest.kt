package app.tatakai.next.metadata

import java.io.IOException
import kotlin.test.Test
import kotlin.test.assertEquals

/** BUG-146: an HTTPS proxy refusing the tunnel with 429 is a rate limit, not "offline". */
class TransportStatusTest {
    @Test
    fun aConnectRefusedWithAnHttpStatusKeepsThatStatus() {
        assertEquals(429, transportStatus(IOException("Unexpected response code for CONNECT: 429")))
        assertEquals(503, transportStatus(IOException("Unexpected response code for CONNECT: 503")))
    }

    @Test
    fun anyOtherTransportFailureIsOffline() {
        assertEquals(0, transportStatus(IOException("Unable to resolve host \"graphql.anilist.co\"")))
        assertEquals(0, transportStatus(IOException("timeout")))
        assertEquals(0, transportStatus(IOException()))
    }
}
