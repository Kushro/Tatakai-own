// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.link

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.boolean
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import java.io.IOException
import java.net.SocketTimeoutException
import javax.net.ssl.SSLException
import javax.net.ssl.SSLHandshakeException
import kotlin.test.Test
import kotlin.test.assertEquals

/** The Kotlin error table must be exactly the `x-errors` table of contracts/link/openapi.v1.json. */
class LinkErrorTableTest {
    @Test
    fun enumMatchesTheOpenApiCatalog() {
        val spec = Json.parseToJsonElement(contractFile("link/openapi.v1.json").readText()).jsonObject
        val rows = spec["x-errors"]!!.jsonObject["codes"]!!.jsonArray.map { it.jsonObject }
        assertEquals(26, rows.size)
        assertEquals(rows.size, LinkApiError.entries.size)
        for (row in rows) {
            val code = row["code"]!!.jsonPrimitive.content
            val status = row["http"]!!.jsonPrimitive.int
            val e = LinkApiError.of(code, status)
            assertEquals(code, e?.code, "code $code/$status missing or wrong status")
            assertEquals(row["m0"]!!.jsonPrimitive.boolean, e!!.m0, "m0 flag of $code")
        }
    }

    @Test
    fun transportClassification() {
        assertEquals(LinkError.Transport.Tls.Revoked, LinkHttp.classify(SSLHandshakeException("Received fatal alert: certificate_revoked")))
        assertEquals(LinkError.Transport.Tls.Expired, LinkHttp.classify(SSLException("SSLV3_ALERT_CERTIFICATE_EXPIRED: certificate_expired")))
        assertEquals(LinkError.Transport.Tls.Other, LinkHttp.classify(SSLHandshakeException("link: server chain does not contain the pinned CA")))
        assertEquals(LinkError.Transport.Timeout, LinkHttp.classify(SocketTimeoutException("timeout")))
        assertEquals(LinkError.Transport.Refused, LinkHttp.classify(IOException("Connection refused")))
    }
}
