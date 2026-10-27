package app.tatakai.next.supabase

import app.tatakai.next.model.UserSession
import io.github.jan.supabase.SupabaseClient
import io.github.jan.supabase.auth.Auth
import io.github.jan.supabase.auth.auth
import io.github.jan.supabase.auth.providers.builtin.Email
import io.github.jan.supabase.auth.status.SessionStatus
import io.github.jan.supabase.createSupabaseClient
import io.github.jan.supabase.exceptions.RestException
import io.github.jan.supabase.postgrest.Postgrest
import io.github.jan.supabase.postgrest.exception.PostgrestRestException
import io.github.jan.supabase.postgrest.from
import io.github.jan.supabase.postgrest.query.Columns
import io.github.jan.supabase.postgrest.query.Order
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import kotlinx.serialization.Serializable
import kotlinx.serialization.SerialName
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

/** Values come from `:app` BuildConfig and are injected through Koin properties (never committed). */
data class SupabaseConfig(val url: String, val anonKey: String) {
    val isConfigured: Boolean
        get() = url.startsWith("https://") && !url.contains("example.invalid") && anonKey.isNotBlank() && anonKey != "dummy"
}

/**
 * Owns the one Supabase client. It talks only to [SupabaseConfig.url] (Ktor OkHttp engine of its own, separate from the
 * shared third-party client in :core:network), so the session can never reach third-party hosts.
 * Session persistence uses supabase-kt's default SharedPreferences-backed session manager (app-private storage).
 */
class SupabaseGateway(val config: SupabaseConfig) {
    internal val client: SupabaseClient? by lazy {
        if (!config.isConfigured) {
            null
        } else {
            createSupabaseClient(config.url, config.anonKey) {
                install(Auth)
                install(Postgrest)
            }
        }
    }

    internal fun requireClient(): SupabaseClient = client ?: throw SupabaseNotConfiguredException()
}

class SupabaseAuthSourceImpl(
    private val gateway: SupabaseGateway,
    scope: CoroutineScope = CoroutineScope(SupervisorJob() + Dispatchers.Default),
) : SupabaseAuthSource {
    private val state = MutableStateFlow<UserSession?>(null)
    override val session: StateFlow<UserSession?> = state

    init {
        gateway.client?.let { client ->
            scope.launch {
                client.auth.sessionStatus.collect { status ->
                    when (status) {
                        is SessionStatus.Authenticated -> {
                            val user = status.session.user
                            state.value = user?.let { UserSession(it.id, it.email) }
                        }
                        is SessionStatus.NotAuthenticated -> state.value = null
                        // Initializing / RefreshFailure: keep the last known value (refresh retries on its own).
                        else -> Unit
                    }
                }
            }
        }
    }

    override suspend fun signIn(email: String, password: String): Result<Unit> = runCatchingNonCancel {
        val client = gateway.requireClient()
        client.auth.signInWith(Email) {
            this.email = email.trim()
            this.password = password
        }
    }

    override suspend fun signOut() {
        val client = gateway.client ?: return
        try {
            client.auth.signOut()
        } catch (e: CancellationException) {
            throw e
        } catch (_: Exception) {
            // Offline: the local session is cleared by signOut(LOCAL) regardless; nothing else to do.
        }
        state.value = null
    }

    override suspend fun isBanned(): Boolean {
        val client = gateway.client ?: return false
        val uid = client.auth.currentUserOrNull()?.id ?: return false
        return try {
            client.from("profiles").select(columns = Columns.list("user_id", "is_banned")) {
                filter { eq("user_id", uid) }
            }.decodeList<BannedProbe>().firstOrNull()?.isBanned == true
        } catch (e: CancellationException) {
            throw e
        } catch (_: Exception) {
            false
        }
    }

    @Serializable
    private data class BannedProbe(
        @SerialName("user_id") val userId: String,
        @SerialName("is_banned") val isBanned: Boolean? = null,
    )
}

class WatchHistoryRemoteImpl(private val gateway: SupabaseGateway) : WatchHistoryRemote {

    override suspend fun upsert(rows: List<RemoteWatchRow>) {
        if (rows.isEmpty()) return
        val uid = userId()
        // PostgREST derives the written columns from the union of keys in a bulk body, so group rows by key set
        // to keep "omit mal_id/anilist_id when null" true per row.
        val bodies = rows.map { body(uid, it) }.groupBy { it.keys }.values
        withAuthRetry {
            for (group in bodies) {
                gateway.requireClient().from("watch_history").upsert(JsonArray(group)) {
                    onConflict = "user_id,episode_id"
                    ignoreDuplicates = false // => Prefer: resolution=merge-duplicates
                }
            }
        }
    }

    override suspend fun recent(limit: Int): List<RemoteWatchRow> {
        val uid = userId()
        val n = limit.toLong()
        return withAuthRetry {
            gateway.requireClient().from("watch_history").select {
                filter { eq("user_id", uid) }
                order("watched_at", Order.DESCENDING)
                limit(n)
            }.decodeList<RemoteWatchRow>()
        }
    }

    private fun userId(): String =
        gateway.requireClient().auth.currentUserOrNull()?.id ?: throw SessionExpiredException()

    private suspend fun <T> withAuthRetry(block: suspend () -> T): T {
        try {
            return block()
        } catch (e: CancellationException) {
            throw e
        } catch (e: RestException) {
            if (!e.looksLikeExpiredToken()) throw e
        }
        // 401 / PGRST301: refresh the token once, then surface "signed out".
        try {
            gateway.requireClient().auth.refreshCurrentSession()
        } catch (e: CancellationException) {
            throw e
        } catch (_: Exception) {
            throw SessionExpiredException()
        }
        try {
            return block()
        } catch (e: RestException) {
            if (e.looksLikeExpiredToken()) throw SessionExpiredException() else throw e
        }
    }

    private fun RestException.looksLikeExpiredToken(): Boolean {
        if ((this as? PostgrestRestException)?.code == "PGRST301") return true
        val text = (message.orEmpty() + " " + description.orEmpty()).lowercase()
        return "jwt" in text || "401" in text || "unauthorized" in text
    }

    internal companion object {
        /** Builds the PostgREST row: nullable ids / duration are omitted entirely (E9). */
        fun body(userId: String, r: RemoteWatchRow): JsonObject {
            val m = linkedMapOf<String, JsonElement>(
                "user_id" to JsonPrimitive(userId),
                "anime_id" to JsonPrimitive(r.animeId),
                "anime_name" to JsonPrimitive(r.animeName),
                "anime_poster" to JsonPrimitive(r.animePoster ?: ""),
                "episode_id" to JsonPrimitive(r.episodeId),
                "episode_number" to JsonPrimitive(r.episodeNumber),
                "progress_seconds" to JsonPrimitive(r.progressSeconds),
                "completed" to JsonPrimitive(r.completed),
                "watched_at" to JsonPrimitive(r.watchedAt),
            )
            r.durationSeconds?.let { m["duration_seconds"] = JsonPrimitive(it) }
            r.anilistId?.let { m["anilist_id"] = JsonPrimitive(it) }
            r.malId?.let { m["mal_id"] = JsonPrimitive(it) }
            return JsonObject(m)
        }
    }
}

private inline fun <T> runCatchingNonCancel(block: () -> T): Result<Unit> = try {
    block()
    Result.success(Unit)
} catch (e: CancellationException) {
    throw e
} catch (e: Exception) {
    Result.failure(e)
}
