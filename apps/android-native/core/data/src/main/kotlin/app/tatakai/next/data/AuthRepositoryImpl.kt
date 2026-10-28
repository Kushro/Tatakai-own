package app.tatakai.next.data

import app.tatakai.next.database.HistoryDao
import app.tatakai.next.model.AccountSuspendedException
import app.tatakai.next.model.AuthRepository
import app.tatakai.next.model.UserSession
import app.tatakai.next.supabase.SupabaseAuthSource
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.flow.StateFlow

class AuthRepositoryImpl(
    private val source: SupabaseAuthSource,
    private val syncer: HistorySyncer,
    private val scheduler: HistorySyncScheduler,
    private val history: HistoryDao,
) : AuthRepository {
    override val session: StateFlow<UserSession?> = source.session

    override suspend fun signIn(email: String, password: String): Result<Unit> {
        val result = source.signIn(email, password)
        if (result.isFailure) return result
        if (source.isBanned()) {
            source.signOut()
            return Result.failure(AccountSuspendedException())
        }
        scheduler.requestSync(immediate = true) // upload what was watched while signed out, pull the rest
        return result
    }

    override suspend fun signOut() {
        // Best effort: push pending rows first, then drop the local copy so the next account never inherits it.
        try {
            syncer.syncOnce()
        } catch (e: CancellationException) {
            throw e
        } catch (_: Exception) {
            // offline or token gone: unsynced rows of this session are discarded with the sign-out
        }
        source.signOut()
        history.clear()
    }
}
