// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.network

import app.tatakai.next.model.AppLog
import app.tatakai.next.model.RateLimitHeaders
import app.tatakai.next.model.RequestPriority
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import okhttp3.Headers
import okhttp3.Interceptor
import okhttp3.MediaType
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.Protocol
import okhttp3.Request
import okhttp3.Response
import okhttp3.ResponseBody.Companion.toResponseBody
import okio.Buffer
import java.io.IOException
import java.util.concurrent.CompletableFuture
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.ExecutionException
import java.util.concurrent.TimeUnit
import java.util.concurrent.TimeoutException
import java.util.concurrent.locks.ReentrantLock
import kotlin.concurrent.withLock

/** What the diagnostics line shows ("AniList: N pedidos/min, último 429 hace X"). */
data class AniListTrafficStats(
    val requestsLastMinute: Int = 0,
    val total: Long = 0,
    val limit: Int = AniListLimiter.DEFAULT_LIMIT,
    val remaining: Int? = null,
    val last429AtMs: Long? = null,
    val cooldownUntilMs: Long? = null,
    val deduped: Long = 0,
    val localRateLimited: Long = 0,
)

/**
 * 0.3.1: the ONE gateway every AniList request of the app goes through (an application interceptor of the shared OkHttp client of
 * [HttpClientFactory]; metadata, lists, social, airing, manga and novels all use that client). For requests to [hosts]:
 * - **concurrency cap** [maxInFlight] (the moto g51 is low-end), interactive waiters served before background ones;
 * - **budget** from [AniListLimiter] (`X-RateLimit-*`, sliding window, background reserve); a wait up to [maxQueueWaitMs] is slept,
 *   a longer one answers at once with a local 429 carrying `Retry-After` (and [LOCAL_HEADER]) so callers surface `RateLimited`
 *   with the wait and the router cools AniList down for that long instead of hammering it;
 * - **429**: `Retry-After` (else `X-RateLimit-Reset`) starts a cool-down; a short one ([AniListRetryPolicy]) is waited and the
 *   request retried once, a long one is returned (with a normalised `Retry-After` in seconds);
 * - **de-duplication**: identical in-flight queries (same URL, body and `Authorization`) share one HTTP call; mutations never.
 * Priority comes from the request tag `RequestPriority::class.java` (absent = interactive).
 */
class AniListGateway(
    private val hosts: Set<String> = setOf(ANILIST_HOST),
    private val maxInFlight: Int = MAX_IN_FLIGHT,
    private val maxQueueWaitMs: Long = MAX_QUEUE_WAIT_MS,
    private val shortRetryMs: Long = AniListRetryPolicy.SHORT_WAIT_MS,
    private val clock: () -> Long = System::currentTimeMillis,
    /** Sleeps up to the given millis; returns early only by throwing (cancellation). Injectable for tests. */
    private val sleeper: ((Long) -> Unit)? = null,
    val limiter: AniListLimiter = AniListLimiter(),
) : Interceptor {
    private val lock = ReentrantLock()
    private val slotFreed = lock.newCondition()
    private var inFlight = 0
    private var interactiveWaiting = 0
    private val pending = ConcurrentHashMap<String, CompletableFuture<Shared>>()
    @Volatile private var deduped = 0L
    @Volatile private var localRateLimited = 0L

    private val _stats = MutableStateFlow(AniListTrafficStats())
    /** Refreshed after every AniList request. */
    val stats: StateFlow<AniListTrafficStats> = _stats.asStateFlow()

    fun snapshot(): AniListTrafficStats = lock.withLock {
        val now = clock()
        AniListTrafficStats(
            requestsLastMinute = limiter.sentInWindow(now),
            total = limiter.total,
            limit = limiter.limit,
            remaining = limiter.remaining,
            last429AtMs = limiter.last429AtMs,
            cooldownUntilMs = limiter.cooldownUntilMs.takeIf { it > now },
            deduped = deduped,
            localRateLimited = localRateLimited,
        )
    }

    private fun publish() {
        _stats.value = snapshot()
    }

    override fun intercept(chain: Interceptor.Chain): Response {
        val request = chain.request()
        if (request.url.host !in hosts) return chain.proceed(request)
        val key = dedupeKey(request) ?: return gated(chain, request)
        val mine = CompletableFuture<Shared>()
        val leader = pending.putIfAbsent(key, mine)
        if (leader != null) {
            deduped++
            return follow(chain, leader).toResponse(request)
        }
        try {
            val response = gated(chain, request)
            val shared = Shared.of(response)
            mine.complete(shared)
            return shared.toResponse(request)
        } catch (t: Throwable) {
            mine.completeExceptionally(t)
            throw t
        } finally {
            pending.remove(key, mine)
        }
    }

    private fun follow(chain: Interceptor.Chain, leader: CompletableFuture<Shared>): Shared {
        while (true) {
            if (chain.call().isCanceled()) throw IOException("Canceled")
            try {
                return leader.get(POLL_MS, TimeUnit.MILLISECONDS)
            } catch (_: TimeoutException) {
                // poll the cancellation again
            } catch (e: ExecutionException) {
                val cause = e.cause
                throw if (cause is IOException) IOException(cause.message, cause) else IOException("AniList request failed", cause)
            }
        }
    }

    private fun gated(chain: Interceptor.Chain, request: Request): Response {
        val priority = request.tag(RequestPriority::class.java) ?: RequestPriority.INTERACTIVE
        var attempt = 0
        while (true) {
            acquireSlot(chain, priority)
            val wait: Long
            try {
                wait = lock.withLock { limiter.tryAcquire(clock(), priority) }
            } catch (t: Throwable) {
                releaseSlot()
                throw t
            }
            if (wait > 0) {
                releaseSlot()
                if (wait > maxQueueWaitMs) {
                    localRateLimited++
                    publish()
                    AppLog.i(TAG, "local budget: ${priority.name.lowercase()} request refused, wait ${wait}ms")
                    return localRateLimit(request, wait)
                }
                sleep(chain, wait)
                continue
            }
            val response = try {
                chain.proceed(request)
            } finally {
                releaseSlot()
            }
            val now = clock()
            val cooldown = lock.withLock {
                limiter.onResponse(
                    now = now,
                    code = response.code,
                    limit = RateLimitHeaders.count(response.header(RateLimitHeaders.LIMIT)),
                    remaining = RateLimitHeaders.count(response.header(RateLimitHeaders.REMAINING)),
                    resetAtMs = RateLimitHeaders.resetAtMs(response.header(RateLimitHeaders.RESET)),
                    retryAfterMs = RateLimitHeaders.retryAfterMs(response.header(RateLimitHeaders.RETRY_AFTER), now),
                )
            }
            publish()
            if (response.code != 429) return response
            if (AniListRetryPolicy.shouldRetry(attempt, cooldown, shortRetryMs)) {
                AppLog.i(TAG, "429: retrying once after ${cooldown}ms")
                response.close()
                attempt++
                continue
            }
            AppLog.i(TAG, "429: cooling down ${cooldown}ms")
            return response.newBuilder()
                .header(RateLimitHeaders.RETRY_AFTER, AniListRetryPolicy.retryAfterSeconds(cooldown).toString())
                .header(RateLimitHeaders.GATEWAY, "1")
                .build()
        }
    }

    private fun acquireSlot(chain: Interceptor.Chain, priority: RequestPriority) {
        lock.withLock {
            val interactive = priority == RequestPriority.INTERACTIVE
            if (interactive) interactiveWaiting++
            try {
                while (inFlight >= maxInFlight || (!interactive && interactiveWaiting > 0)) {
                    if (chain.call().isCanceled()) throw IOException("Canceled")
                    slotFreed.await(POLL_MS, TimeUnit.MILLISECONDS)
                }
                inFlight++
            } finally {
                if (interactive) interactiveWaiting--
            }
        }
    }

    private fun releaseSlot() {
        lock.withLock {
            inFlight--
            slotFreed.signalAll()
        }
    }

    private fun sleep(chain: Interceptor.Chain, ms: Long) {
        sleeper?.let { return it(ms) }
        val until = System.nanoTime() + TimeUnit.MILLISECONDS.toNanos(ms)
        while (true) {
            if (chain.call().isCanceled()) throw IOException("Canceled")
            val left = TimeUnit.NANOSECONDS.toMillis(until - System.nanoTime())
            if (left <= 0) return
            try {
                Thread.sleep(minOf(left, POLL_MS))
            } catch (e: InterruptedException) {
                Thread.currentThread().interrupt()
                throw IOException("Interrupted", e)
            }
        }
    }

    private fun localRateLimit(request: Request, waitMs: Long): Response =
        Response.Builder()
            .request(request)
            .protocol(Protocol.HTTP_1_1)
            .code(429)
            .message("Too Many Requests (local AniList budget)")
            .header(RateLimitHeaders.RETRY_AFTER, AniListRetryPolicy.retryAfterSeconds(waitMs).toString())
            .header(LOCAL_HEADER, "1")
            .header(RateLimitHeaders.GATEWAY, "1")
            .body(LOCAL_429_BODY.toResponseBody(JSON))
            .build()

    /** A fully buffered response that several identical calls can each turn into their own [Response]. */
    private class Shared(
        val code: Int,
        val message: String,
        val protocol: Protocol,
        val headers: Headers,
        val contentType: MediaType?,
        val bytes: ByteArray,
        val sentAt: Long,
        val receivedAt: Long,
    ) {
        fun toResponse(request: Request): Response = Response.Builder()
            .request(request)
            .protocol(protocol)
            .code(code)
            .message(message)
            .headers(headers)
            .body(bytes.toResponseBody(contentType))
            .sentRequestAtMillis(sentAt)
            .receivedResponseAtMillis(receivedAt)
            .build()

        companion object {
            fun of(response: Response): Shared = response.use { r ->
                Shared(r.code, r.message, r.protocol, r.headers, r.body.contentType(), r.body.bytes(), r.sentRequestAtMillis, r.receivedResponseAtMillis)
            }
        }
    }

    companion object {
        const val ANILIST_HOST = "graphql.anilist.co"
        const val MAX_IN_FLIGHT = 2
        const val MAX_QUEUE_WAIT_MS = 10_000L
        /** Marks a 429 the gateway produced itself (budget exhausted / cool-down running): AniList was not asked. */
        const val LOCAL_HEADER = "X-Tatakai-Local-RateLimit"
        private const val POLL_MS = 200L
        private const val MAX_DEDUPE_BODY = 64 * 1024L
        private const val TAG = "anilist"
        private val JSON = "application/json".toMediaType()
        private const val LOCAL_429_BODY = """{"data":null,"errors":[{"message":"Too Many Requests.","status":429}]}"""
        private val Lenient = Json { ignoreUnknownKeys = true; isLenient = true }

        /**
         * De-duplication key of an AniList request: method, URL, `Authorization` and body. Null (never shared) for mutations, for
         * bodies that are not a GraphQL JSON document or are too large, and for methods other than GET / POST.
         */
        internal fun dedupeKey(request: Request): String? {
            val auth = request.header("Authorization").orEmpty()
            return when (request.method) {
                "GET" -> "GET ${request.url} $auth"
                "POST" -> {
                    val body = request.body ?: return null
                    val length = body.contentLength()
                    if (length < 0 || length > MAX_DEDUPE_BODY || body.isOneShot()) return null
                    val text = Buffer().also { body.writeTo(it) }.readUtf8()
                    if (isMutation(text)) return null
                    "POST ${request.url} ${auth.hashCode()} $text"
                }
                else -> null
            }
        }

        /** True when the GraphQL document of a JSON request body is a mutation (or the body is not a GraphQL request). */
        internal fun isMutation(body: String): Boolean {
            val query = runCatching { (Lenient.parseToJsonElement(body) as? JsonObject)?.get("query") as? JsonPrimitive }.getOrNull()
                ?.takeIf { it.isString }?.content ?: return true
            return query.trimStart().startsWith("mutation")
        }
    }
}
