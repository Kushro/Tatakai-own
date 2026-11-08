// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensionserver

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class HostGuardTest {
    @Test
    fun httpsIsAllowedForAnyHost() {
        assertTrue(HostGuard.isAllowed("https://my-tunnel.trycloudflare.com"))
        assertTrue(HostGuard.isAllowed("https://example.org:8443/api/server/v1/health"))
        assertTrue(HostGuard.isAllowed("HTTPS://Example.ORG"))
    }

    @Test
    fun httpIsAllowedOnlyForLocalHosts() {
        listOf(
            "http://127.0.0.1:8787", "http://127.10.0.1", "http://localhost:8787", "http://pc.localhost", "http://[::1]:8787",
            "http://10.0.0.5", "http://172.16.0.1", "http://172.31.255.254", "http://192.168.1.20:8090",
            "http://169.254.10.10", "http://[fe80::1]:80", "http://100.64.0.1", "http://100.127.255.255",
            "http://[fd7a:115c:a1e0::1234]:8787", "http://[fc00::1]", "http://mipc.local:8787", "http://MIPC.LOCAL.",
            "http://[::ffff:192.168.1.2]",
        ).forEach { assertTrue(HostGuard.isAllowed(it), it) }
    }

    @Test
    fun httpIsBlockedForPublicAndAmbiguousHosts() {
        listOf(
            "http://example.org", "http://8.8.8.8", "http://172.15.0.1", "http://172.32.0.1", "http://192.169.0.1",
            "http://100.63.255.255", "http://100.128.0.1", "http://169.253.0.1", "http://[2001:db8::1]", "http://[fec0::1]",
            "http://mipc", "http://local", "http://192.168.1.20.nip.io", "http://0x7f.0.0.1", "http://0177.0.0.1",
            "http://192.168.001.1", "http://256.1.1.1",
        ).forEach { assertFalse(HostGuard.isAllowed(it), it) }
    }

    @Test
    fun otherSchemesCredentialsAndGarbageAreBlocked() {
        listOf(
            "ftp://192.168.1.2", "file:///etc/hosts", "tatakai://extensionserver/connect", "javascript:alert(1)", "",
            "not a url", "http://user:pass@192.168.1.2", "https://user@example.org", "//192.168.1.2",
        ).forEach { assertFalse(HostGuard.isAllowed(it), it) }
    }

    @Test
    fun requireThrowsInsecureHost() {
        val e = assertFailsWith<ExtensionServerException> { HostGuard.require("http://example.org") }
        assertEquals(ExtensionServerException.INSECURE_HOST, e.code)
        HostGuard.require("http://192.168.1.5:8787")
    }

    @Test
    fun originOfNormalizesBaseUrls() {
        assertEquals("http://192.168.1.20:8090", HostGuard.originOf("http://192.168.1.20:8090/"))
        assertEquals("http://192.168.1.20:8090", HostGuard.originOf("  HTTP://192.168.1.20:8090  "))
        assertEquals("https://pc.example.org", HostGuard.originOf("https://PC.example.org:443"))
        assertEquals("http://[fd7a::1]:8787", HostGuard.originOf("http://[fd7a::1]:8787"))
        assertNull(HostGuard.originOf("http://192.168.1.20:8090/api"))
        assertNull(HostGuard.originOf("http://192.168.1.20:8090?x=1"))
        assertNull(HostGuard.originOf("http://a:b@192.168.1.20"))
        assertNull(HostGuard.originOf("ws://192.168.1.20"))
        assertNull(HostGuard.originOf("192.168.1.20:8090"))
    }

    @Test
    fun sameOriginComparesSchemeHostAndEffectivePort() {
        val origin = "http://192.168.1.20:8090"
        assertTrue(HostGuard.sameOrigin("http://192.168.1.20:8090/api/server/v1/media/abc", origin))
        assertFalse(HostGuard.sameOrigin("http://192.168.1.21:8090/api/server/v1/media/abc", origin))
        assertFalse(HostGuard.sameOrigin("https://192.168.1.20:8090/x", origin))
        assertFalse(HostGuard.sameOrigin("http://192.168.1.20:8091/x", origin))
        assertTrue(HostGuard.sameOrigin("https://pc.example.org/x", "https://pc.example.org:443"))
        assertFalse(HostGuard.sameOrigin("http://u@192.168.1.20:8090/x", origin))
    }
}
