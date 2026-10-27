// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.link

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertTrue

class PairingPayloadTest {
    private val ca = "ab".repeat(32)
    private val now = 1_000_000L
    private fun payload(
        scheme: String = "tatakai-link:v1",
        h: String = "192.168.1.20", p: String = "47321", c: String = "0123456789", x: Long = now + 300, n: String = "My%20PC",
    ) = "$scheme?h=$h&p=$p&ca=$ca&c=$c&x=$x&n=$n"

    private fun error(text: String) = assertIs<PayloadResult.Invalid>(PairingPayload.parse(text, now)).error

    @Test
    fun v1PayloadParses() {
        val ok = assertIs<PayloadResult.Valid>(PairingPayload.parse("  " + payload() + "\n", now)).payload
        assertEquals("192.168.1.20", ok.host)
        assertEquals(47321, ok.port)
        assertEquals(ca, ok.caSha256)
        assertEquals("0123456789", ok.code)
        assertEquals("My PC", ok.pcName)
        assertEquals("https://192.168.1.20:47321", ok.baseUrl)
        assertFalse("0123456789" in ok.toString(), "the one-time code must never be printed")
    }

    @Test
    fun crockfordCodeIsNormalized() {
        val p = assertIs<PayloadResult.Valid>(PairingPayload.parse(payload(c = "oi-lA1b2c3D"), now)).payload
        assertEquals("011A1B2C3D", p.code)
    }

    @Test
    fun v0AndForeignSchemesAreRejected() {
        assertEquals(PayloadError.OldVersion, error(payload(scheme = "tatakai-link:v0")))
        assertEquals(PayloadError.NotAPayload, error(payload(scheme = "enigmata-temple:v1")))
        assertEquals(PayloadError.NotAPayload, error("https://example.com/?h=1.2.3.4"))
        assertEquals(PayloadError.NotAPayload, error(""))
    }

    @Test
    fun expiredPayloadsAreRejected() {
        assertEquals(PayloadError.Expired, error(payload(x = now)))
        assertEquals(PayloadError.Expired, error(payload(x = now - 1)))
        assertTrue(PairingPayload.parse(payload(x = now + 1), now) is PayloadResult.Valid)
    }

    @Test
    fun malformedPayloadsAreRejected() {
        assertEquals(PayloadError.InvalidHost, error(payload(h = "pc.local")))
        assertEquals(PayloadError.InvalidHost, error(payload(h = "0.0.0.0")))
        assertEquals(PayloadError.InvalidHost, error(payload(h = "999.1.1.1")))
        assertEquals(PayloadError.InvalidPort, error(payload(p = "70000")))
        assertEquals(PayloadError.InvalidCode, error(payload(c = "123")))
        assertEquals(PayloadError.InvalidCode, error(payload(c = "UUUUUUUUUU")))
        assertEquals(PayloadError.Missing("c"), error("tatakai-link:v1?h=1.2.3.4&p=1&ca=$ca&x=${now + 5}"))
        assertEquals(PayloadError.Malformed, error(payload() + "&h=1.2.3.5"))
        assertEquals(PayloadError.Malformed, error(payload() + "&evil=1"))
        assertEquals(PayloadError.InvalidCa, error("tatakai-link:v1?h=1.2.3.4&p=1&ca=zz&c=0123456789&x=${now + 5}"))
    }
}
