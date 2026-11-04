// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.torrent.stream

import kotlin.test.Test
import kotlin.test.assertEquals

class HttpRangeTest {

    @Test
    fun `missing or foreign units serve the whole file`() {
        assertEquals(RangeRequest.Full, parseRange(null, 100))
        assertEquals(RangeRequest.Full, parseRange("items=0-1", 100))
        assertEquals(RangeRequest.Full, parseRange("bytes=abc", 100))
        assertEquals(RangeRequest.Full, parseRange("bytes=50-10", 100))
    }

    @Test
    fun `explicit, open and suffix ranges`() {
        assertEquals(RangeRequest.Partial(ByteRange(0, 9)), parseRange("bytes=0-9", 100))
        assertEquals(RangeRequest.Partial(ByteRange(90, 99)), parseRange("bytes=90-", 100))
        assertEquals(RangeRequest.Partial(ByteRange(90, 99)), parseRange("bytes=90-500", 100))
        assertEquals(RangeRequest.Partial(ByteRange(80, 99)), parseRange("bytes=-20", 100))
        assertEquals(RangeRequest.Partial(ByteRange(0, 99)), parseRange("bytes=-500", 100))
        assertEquals(RangeRequest.Partial(ByteRange(5, 6)), parseRange("BYTES=5-6, 10-20", 100))
    }

    @Test
    fun `unsatisfiable ranges`() {
        assertEquals(RangeRequest.Unsatisfiable, parseRange("bytes=100-", 100))
        assertEquals(RangeRequest.Unsatisfiable, parseRange("bytes=-0", 100))
    }

    @Test
    fun `content types and path encoding`() {
        assertEquals("video/x-matroska", contentTypeFor("a/B.MKV"))
        assertEquals("video/mp4", contentTypeFor("x.mp4"))
        assertEquals("video/webm", contentTypeFor("x.webm"))
        assertEquals("application/octet-stream", contentTypeFor("x"))
        assertEquals("%5BGroup%5D%20Show%20-%2001.mkv", encodePathSegment("[Group] Show - 01.mkv"))
        assertEquals("%C3%B1", encodePathSegment("ñ"))
    }
}
