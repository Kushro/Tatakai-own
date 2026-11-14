// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.link

import kotlin.test.Test
import kotlin.test.assertEquals

/** BUG-131: a profile change waits for `ready` before the player opens the new session's playlist. */
class SessionReadinessTest {
    private fun state(s: String) = LinkResult.Ok(SessionStateDto(state = s, generation = 1, instanceId = "pc", profile = "p-none"))

    @Test
    fun readyPreparingAndFailedStates() {
        assertEquals(true, LinkRepositoryImpl.sessionReadiness(state("ready")))
        assertEquals(null, LinkRepositoryImpl.sessionReadiness(state("preparing")))
        assertEquals(false, LinkRepositoryImpl.sessionReadiness(state("failed")))
        assertEquals(false, LinkRepositoryImpl.sessionReadiness(state("closed")))
    }

    @Test
    fun aGoneSessionIsNotReadyAndAnUnreachablePcIsUnknown() {
        assertEquals(false, LinkRepositoryImpl.sessionReadiness(LinkResult.Failure(LinkError.Api(LinkApiError.SessionGone))))
        assertEquals(false, LinkRepositoryImpl.sessionReadiness(LinkResult.Failure(LinkError.Api(LinkApiError.SessionNotFound))))
        assertEquals(null, LinkRepositoryImpl.sessionReadiness(LinkResult.Failure(LinkError.Transport.Timeout)))
    }
}
