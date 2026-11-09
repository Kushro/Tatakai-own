// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.settings

import app.tatakai.next.data.ListErrors
import app.tatakai.next.model.AppError
import kotlin.test.Test
import kotlin.test.assertEquals

/** parity wave A (worker q, account.oauth.errors): every lists / OAuth failure has its own readable text. */
class AccountsErrorTest {
    @Test
    fun mapsListErrorCodes() {
        assertEquals(AccountsError.NO_CLIENT_ID, AppError.Unknown("${ListErrors.NO_CLIENT_ID}:anilist").toAccountsError())
        assertEquals(AccountsError.INVALID_STATE, AppError.Unknown(ListErrors.INVALID_STATE).toAccountsError())
        assertEquals(AccountsError.CANCELLED, AppError.Unknown(ListErrors.CANCELLED).toAccountsError())
        assertEquals(AccountsError.DENIED, AppError.Unknown("${ListErrors.DENIED}:server_error").toAccountsError())
        assertEquals(AccountsError.EXPIRED, AppError.Unknown(ListErrors.EXPIRED).toAccountsError())
        assertEquals(AccountsError.EXCHANGE, AppError.Unknown(ListErrors.EXCHANGE).toAccountsError())
        assertEquals(AccountsError.BAD_FILE, AppError.Unknown(ListErrors.BAD_FILE).toAccountsError())
        assertEquals(AccountsError.NOT_LINKED, AppError.Unknown(ListErrors.NOT_LINKED).toAccountsError())
        assertEquals(AccountsError.UNKNOWN, AppError.Unknown("something").toAccountsError())
    }

    @Test
    fun mapsTransportErrors() {
        assertEquals(AccountsError.OFFLINE, AppError.Offline.toAccountsError())
        assertEquals(AccountsError.RATE_LIMITED, AppError.RateLimited("anilist").toAccountsError())
        assertEquals(AccountsError.EXPIRED, AppError.Http(401).toAccountsError())
        assertEquals(AccountsError.HTTP, AppError.Http(500).toAccountsError())
    }

    @Test
    fun everyErrorHasAText() {
        AccountsError.entries.forEach { it.messageRes() }
    }
}
