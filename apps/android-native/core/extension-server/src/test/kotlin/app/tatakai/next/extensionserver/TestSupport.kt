// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensionserver

import mockwebserver3.Dispatcher
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import mockwebserver3.RecordedRequest
import java.net.InetAddress

/** A fake PC extension server on 127.0.0.1 (passes [HostGuard] as loopback). */
class FakeServer(handler: (RecordedRequest) -> MockResponse) : AutoCloseable {
    private val server = MockWebServer().apply {
        dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse = handler(request)
        }
        start(InetAddress.getByName("127.0.0.1"), 0)
    }

    /** Origin without trailing slash, built from the literal address (the MockWebServer host name may not be a loopback name). */
    val origin: String get() = "http://127.0.0.1:${server.port}"

    fun take(): RecordedRequest = server.takeRequest()

    override fun close() = server.close()

    companion object {
        fun json(body: String, code: Int = 200): MockResponse =
            MockResponse.Builder().code(code).addHeader("Content-Type", "application/json").body(body).build()

        fun ok(data: String): MockResponse = json("""{"success":true,"data":$data}""")

        fun fail(code: Int, error: String): MockResponse =
            json("""{"success":false,"error":{"code":"$error","message":"secret detail http://x"}}""", code)

        fun ndjson(lines: List<String>): MockResponse =
            MockResponse.Builder().code(200).addHeader("Content-Type", "application/x-ndjson")
                .body(lines.joinToString("\n", postfix = "\n")).build()

        fun health(serverId: String = "srv-1", name: String = "Tatakai (desk)", pairingRequired: Boolean = true): String =
            """{"protocolVersion":1,"serverId":"$serverId","name":"$name","platform":"desktop","setupRequired":false,
               "accessPasswordRequired":false,"adminPasswordRequired":false,"pairingRequired":$pairingRequired,
               "dashboardPath":"/server/","capabilities":[],"proxy":{"hls":true},"desktop":true}"""
    }
}
