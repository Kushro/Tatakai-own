// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import androidx.media3.common.C
import androidx.media3.datasource.DataSpec
import androidx.media3.datasource.HttpDataSource
import androidx.media3.exoplayer.source.LoadEventInfo
import androidx.media3.exoplayer.source.MediaLoadData
import androidx.media3.exoplayer.upstream.LoadErrorHandlingPolicy
import java.io.IOException
import java.net.SocketTimeoutException
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/** BUG-138: a Link session the PC no longer has fails at once before its first frame; everything transient keeps retrying. */
@androidx.annotation.OptIn(androidx.media3.common.util.UnstableApi::class)
class RemoteLoadErrorPolicyTest {
    // android.net.Uri is a stub in JVM tests (Uri.EMPTY is null and DataSpec rejects it): the policy never reads the spec
    private val spec: DataSpec = run {
        val f = Class.forName("sun.misc.Unsafe").getDeclaredField("theUnsafe").apply { isAccessible = true }
        val unsafe = f.get(null)
        unsafe.javaClass.getMethod("allocateInstance", Class::class.java).invoke(unsafe, DataSpec::class.java) as DataSpec
    }

    private fun status(code: Int): IOException =
        HttpDataSource.InvalidResponseCodeException(code, null, null, emptyMap(), spec, ByteArray(0))

    private fun info(e: IOException, errorCount: Int = 1) = LoadErrorHandlingPolicy.LoadErrorInfo(
        LoadEventInfo(0L, spec, 0L),
        MediaLoadData(C.DATA_TYPE_MEDIA),
        e,
        errorCount,
    )

    @Test
    fun aClosedOrUnknownSessionIsNotRetriedBeforeTheFirstFrame() {
        val policy = RemoteLoadErrorPolicy { true }
        assertEquals(C.TIME_UNSET, policy.getRetryDelayMsFor(info(status(410))))
        assertEquals(C.TIME_UNSET, policy.getRetryDelayMsFor(info(status(404))))
    }

    @Test
    fun whilePlayingAClosedSessionKeepsRetrying() {
        // a profile change: the PC closed the old session before the new one is ready, the buffered video plays on
        val policy = RemoteLoadErrorPolicy { false }
        assertEquals(0L, policy.getRetryDelayMsFor(info(status(410))))
        assertEquals(5_000L, policy.getRetryDelayMsFor(info(status(404), errorCount = 9)))
    }

    @Test
    fun transientFailuresAreAlwaysRetried() {
        // e572682f: a wifi cut / the PC waking up still gets about a minute
        val policy = RemoteLoadErrorPolicy { true }
        assertEquals(1_000L, policy.getRetryDelayMsFor(info(SocketTimeoutException("timeout"), errorCount = 2)))
        assertEquals(2_000L, policy.getRetryDelayMsFor(info(status(503), errorCount = 3)))
        assertEquals(0L, policy.getRetryDelayMsFor(info(status(500))))
        assertEquals(0L, policy.getRetryDelayMsFor(info(status(409))))
        assertEquals(RemoteLoadErrorPolicy.RETRIES, policy.getMinimumLoadableRetryCount(C.DATA_TYPE_MEDIA))
    }

    @Test
    fun onlyNotFoundAndGoneEndASession() {
        assertTrue(RemoteLoadErrorPolicy.endsSession(status(410)))
        assertTrue(RemoteLoadErrorPolicy.endsSession(status(404)))
        assertFalse(RemoteLoadErrorPolicy.endsSession(status(503)))
        assertFalse(RemoteLoadErrorPolicy.endsSession(SocketTimeoutException("timeout")))
        assertFalse(RemoteLoadErrorPolicy.endsSession(IOException("no route to host")))
    }
}
