// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.torrent.debrid

import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppException
import app.tatakai.next.model.AppLog
import app.tatakai.next.model.Container
import app.tatakai.next.model.SecretKeys
import app.tatakai.next.model.SecretStore
import app.tatakai.next.model.SourceCandidate
import app.tatakai.next.model.SourceKind
import app.tatakai.next.model.StreamSource
import app.tatakai.next.model.torrent.DebridAccount
import app.tatakai.next.model.torrent.DebridFile
import app.tatakai.next.model.torrent.DebridRepository
import app.tatakai.next.model.torrent.DebridService
import app.tatakai.next.model.torrent.DebridStatus
import app.tatakai.next.release.parseRelease
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.serialization.Serializable
import kotlinx.serialization.builtins.ListSerializer
import okhttp3.FormBody
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.MultipartBody
import okhttp3.OkHttpClient
import okhttp3.Request
import java.io.IOException
import java.util.Locale
import java.util.concurrent.TimeUnit

/**
 * Parity wave A (docs/redesign/15-parity-wave-a-contract-android.md §3.9, 2026-11-09 amendment). Owner: worker q.
 *
 * One client per service behind the same interface, ported from the classic `src/core/providers/*`:
 * - Real-Debrid: `/rest/1.0/user`, `/torrents/addMagnet`, `/torrents/info`, `/torrents/selectFiles`, `/unrestrict/link`
 *   (its `instantAvailability` endpoint is gone, so availability is learnt at resolve time: a torrent that RD starts downloading
 *   instead of answering "downloaded" is deleted again and reported as not cached).
 * - TorBox: `/v1/api/user/me`, `/torrents/checkcached`, `/createtorrent`, `/mylist`, `/requestdl`.
 * - AllDebrid: `/v4/user`, `/v4/magnet/upload`, `/v4.1/magnet/status`, `/v4/magnet/files`, `/v4/link/unlock`.
 * - Premiumize: `/api/account/info`, `/api/cache/check`, `/api/transfer/directdl`.
 * API keys come from [SecretStore] (`SecretKeys.debridApiKey`), never from settings; the account list (username, premium end,
 * preference order) is kept sealed in the same store under [KEY_ACCOUNTS] and rebuilt at start. [client] is the shared app client.
 */
class DebridRepositoryImpl(
    client: OkHttpClient,
    private val secrets: SecretStore,
    private val clock: () -> Long = System::currentTimeMillis,
    scope: CoroutineScope? = CoroutineScope(SupervisorJob() + Dispatchers.IO),
    private val pollDelayMs: Long = POLL_MS,
) : DebridRepository {

    private val http: OkHttpClient = client.newBuilder().callTimeout(CALL_TIMEOUT_S, TimeUnit.SECONDS).build()
    private val _accounts = MutableStateFlow<List<DebridAccount>>(emptyList())
    override val accounts: StateFlow<List<DebridAccount>> = _accounts.asStateFlow()
    private val mutex = Mutex()

    @Serializable
    private data class StoredAccount(val service: DebridService, val username: String? = null, val premiumUntilMs: Long? = null, val linkedAtMs: Long)

    init {
        scope?.launch { runCatching { load() } }
    }

    /** Reads the sealed account list (only services whose key is still stored). Public for tests / a reset. */
    suspend fun load() = mutex.withLock {
        val raw = secrets.get(KEY_ACCOUNTS)
        val stored = raw?.let { runCatching { debridJson.decodeFromString(ListSerializer(StoredAccount.serializer()), it) }.getOrNull() }.orEmpty()
        _accounts.value = stored.filter { secrets.get(SecretKeys.debridApiKey(it.service.id)) != null }.map { it.toModel() }
    }

    // ------------------------------------------------------------------------------------------------ accounts

    override suspend fun link(service: DebridService, apiKey: String): Result<DebridAccount> = try {
        val key = apiKey.trim()
        if (key.isEmpty() || key.any { it.isWhitespace() }) throw AppException(AppError.Unknown("debrid:invalid_key"))
        val user = fetchUser(service, key) ?: throw AppException(AppError.Unknown("debrid:invalid_key"))
        val account = DebridAccount(service, user.username, user.premiumUntilMs, clock())
        mutex.withLock {
            secrets.put(SecretKeys.debridApiKey(service.id), key)
            _accounts.value = _accounts.value.filter { it.service != service } + account
            persist()
        }
        AppLog.i(TAG, "linked ${service.id}")
        Result.success(account)
    } catch (e: CancellationException) {
        throw e
    } catch (e: AppException) {
        // 401 / 403 from /user = the key is wrong
        val code = (e.error as? AppError.Http)?.code
        Result.failure(if (code == 401 || code == 403) AppException(AppError.Unknown("debrid:invalid_key"), e) else e)
    } catch (e: Exception) {
        Result.failure(e.toApp())
    }

    override suspend fun unlink(service: DebridService) {
        mutex.withLock {
            secrets.put(SecretKeys.debridApiKey(service.id), null)
            _accounts.value = _accounts.value.filter { it.service != service }
            persist()
        }
        AppLog.i(TAG, "unlinked ${service.id}")
    }

    /** Moves [service] to the front: the first linked service that has a torrent is the one that plays it (preference order). */
    suspend fun prefer(service: DebridService) {
        mutex.withLock {
            val list = _accounts.value
            val hit = list.firstOrNull { it.service == service } ?: return
            _accounts.value = listOf(hit) + list.filter { it.service != service }
            persist()
        }
    }

    // ------------------------------------------------------------------------------------------------ availability

    override suspend fun cached(infoHashes: List<String>): Map<String, Set<DebridService>> {
        val hashes = infoHashes.map { it.lowercase(Locale.ROOT) }.filter { HEX40.matches(it) }.distinct()
        if (hashes.isEmpty()) return emptyMap()
        val out = HashMap<String, MutableSet<DebridService>>()
        for (account in _accounts.value) {
            val key = keyOf(account.service) ?: continue
            val hits = try {
                when (account.service) {
                    DebridService.TORBOX -> hashes.chunked(CACHE_BATCH).flatMap { batch ->
                        val url = "$TORBOX/api/torrents/checkcached".toHttpUrl().newBuilder()
                            .addQueryParameter("hash", batch.joinToString(","))
                            .addQueryParameter("format", "object")
                            .build()
                        parseTorboxCached(get(Request.Builder().url(url).bearer(key).build()), batch)
                    }.toSet()
                    DebridService.PREMIUMIZE -> hashes.chunked(CACHE_BATCH).flatMap { batch ->
                        val form = FormBody.Builder().add("apikey", key).apply { batch.forEach { add("items[]", it) } }.build()
                        parsePremiumizeCached(get(Request.Builder().url("$PREMIUMIZE/cache/check").post(form).build()), batch)
                    }.toSet()
                    // no instant-availability endpoint: learnt when playing (fastResolve)
                    DebridService.REAL_DEBRID, DebridService.ALLDEBRID -> emptySet()
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                AppLog.w(TAG, "cache check failed on ${account.service.id}", e)
                emptySet()
            }
            hits.forEach { out.getOrPut(it) { LinkedHashSet() } += account.service }
        }
        return out
    }

    override suspend fun markCached(candidates: List<SourceCandidate>): List<SourceCandidate> {
        val linked = _accounts.value.map { it.service }
        if (linked.isEmpty()) return candidates
        val hashes = candidates.filter { it.kind == SourceKind.TORRENT }.mapNotNull { it.torrent?.infoHash }
        if (hashes.isEmpty()) return candidates
        val map = runCatching { cached(hashes) }.getOrDefault(emptyMap())
        return candidates.map { c -> c.copy(debrid = debridStatusFor(c, linked, map) ?: c.debrid) }
    }

    // ------------------------------------------------------------------------------------------------ files / resolve

    override suspend fun files(service: DebridService, infoHash: String, magnet: String?): Result<List<DebridFile>> = try {
        val prepared = prepare(service, infoHash, magnet, maxPolls = FULL_POLLS)
        Result.success(
            prepared.files.mapIndexed { i, f ->
                DebridFile(index = i, name = f.name.substringAfterLast('/'), sizeBytes = f.size, episode = episodeOf(f.name))
            },
        )
    } catch (e: CancellationException) {
        throw e
    } catch (e: Exception) {
        Result.failure(e.toApp())
    }

    override suspend fun resolve(service: DebridService, infoHash: String, magnet: String?, fileIndex: Int?, episode: Int?): Result<StreamSource> =
        resolveWith(service, infoHash, magnet, fileIndex, episode, FULL_POLLS)

    /**
     * Used by the torrent engine before starting libtorrent (checklist sources.debrid.realdebrid etc.): the first linked service, in
     * preference order, that serves [infoHash] instantly. Services with a cache endpoint are asked first; Real-Debrid / AllDebrid are
     * probed with a short poll. Null = play it with libtorrent.
     */
    suspend fun instantStream(infoHash: String, magnet: String?, episode: Int?): StreamSource? {
        val linked = _accounts.value.map { it.service }
        if (linked.isEmpty()) return null
        val hash = infoHash.lowercase(Locale.ROOT)
        val cachedOn = runCatching { cached(listOf(hash))[hash].orEmpty() }.getOrDefault(emptySet())
        val order = linked.filter { it in cachedOn } + linked.filter { it in PROBED && it !in cachedOn }
        for (service in order) {
            val result = resolveWith(service, hash, magnet, null, episode, if (service in cachedOn) FULL_POLLS else PROBE_POLLS)
            result.getOrNull()?.let { return it }
            AppLog.i(TAG, "${service.id} cannot serve it now: ${result.exceptionOrNull()?.message}")
        }
        return null
    }

    private suspend fun resolveWith(
        service: DebridService,
        infoHash: String,
        magnet: String?,
        fileIndex: Int?,
        episode: Int?,
        maxPolls: Int,
    ): Result<StreamSource> = try {
        val prepared = prepare(service, infoHash, magnet, maxPolls)
        val file = fileIndex?.let { prepared.files.getOrNull(it) } ?: selectBestVideo(prepared.files, episode)
            ?: throw AppException(AppError.Unknown("debrid:no_file"))
        val url = prepared.unlock(file)
        if (!url.startsWith("https://") && !url.startsWith("http://")) throw AppException(AppError.Unknown("debrid:bad_link"))
        val tags = runCatching { parseRelease(file.name.substringAfterLast('/')) }.getOrNull()
        Result.success(
            StreamSource(
                url = url,
                headers = emptyMap(),
                container = Container.PROGRESSIVE,
                quality = tags?.resolution?.label()?.takeIf { it.isNotEmpty() },
                provider = service.label,
                extensionId = TORRENT_EXTENSION_ID,
            ),
        )
    } catch (e: CancellationException) {
        throw e
    } catch (e: Exception) {
        Result.failure(e.toApp())
    }

    /** The torrent added (or found) on the service, its files and how to turn one into an HTTPS link. */
    private class Prepared(val files: List<RemoteFile>, val unlock: suspend (RemoteFile) -> String)

    private suspend fun prepare(service: DebridService, infoHash: String, magnet: String?, maxPolls: Int): Prepared {
        val key = keyOf(service) ?: throw AppException(AppError.Unknown("debrid:not_linked:${service.id}"))
        val hash = infoHash.lowercase(Locale.ROOT)
        val magnetUri = magnet?.takeIf { it.startsWith("magnet:") } ?: "magnet:?xt=urn:btih:$hash"
        return when (service) {
            DebridService.REAL_DEBRID -> prepareRealDebrid(key, magnetUri, maxPolls)
            DebridService.TORBOX -> prepareTorbox(key, hash, magnetUri, maxPolls)
            DebridService.ALLDEBRID -> prepareAllDebrid(key, magnetUri, maxPolls)
            DebridService.PREMIUMIZE -> {
                val form = FormBody.Builder().add("apikey", key).add("src", magnetUri).build()
                val files = parsePremiumizeContent(get(Request.Builder().url("$PREMIUMIZE/transfer/directdl").post(form).build()))
                if (files.isEmpty()) throw NotCachedException(service)
                Prepared(files) { it.id }
            }
        }
    }

    private suspend fun prepareRealDebrid(key: String, magnet: String, maxPolls: Int): Prepared {
        val added = parseObject(get(Request.Builder().url("$RD/torrents/addMagnet").bearer(key).post(FormBody.Builder().add("magnet", magnet).build()).build()))
        val id = added?.get("id").str()?.takeIf { it.isNotBlank() } ?: throw AppException(AppError.Unknown("debrid:rejected"))
        var info: RdInfo? = null
        for (attempt in 0 until maxPolls) {
            info = parseRdInfo(get(Request.Builder().url("$RD/torrents/info/$id").bearer(key).build()))
            if (info != null && info.status in RD_FAILED) throw AppException(AppError.Unknown("debrid:failed:${info.status}"))
            if (info != null && info.files.isNotEmpty() && info.status != "magnet_conversion") break
            delay(pollDelayMs)
        }
        val files = info?.files.orEmpty()
        if (files.isEmpty()) {
            rdDelete(key, id)
            throw NotCachedException(DebridService.REAL_DEBRID)
        }
        return Prepared(files) { file ->
            get(Request.Builder().url("$RD/torrents/selectFiles/$id").bearer(key).post(FormBody.Builder().add("files", file.id).build()).build())
            var links: List<String> = emptyList()
            for (attempt in 0 until maxPolls) {
                val now = parseRdInfo(get(Request.Builder().url("$RD/torrents/info/$id").bearer(key).build()))
                if (now != null && now.status in RD_FAILED) throw AppException(AppError.Unknown("debrid:failed:${now.status}"))
                if (now != null && now.status == "downloaded" && now.links.isNotEmpty()) {
                    links = now.links
                    break
                }
                if (now != null && now.status in RD_NOT_CACHED && attempt >= RD_NOT_CACHED_AFTER) {
                    // RD started downloading: not instantly available; do not leave a half torrent on the account
                    rdDelete(key, id)
                    throw NotCachedException(DebridService.REAL_DEBRID)
                }
                delay(pollDelayMs)
            }
            val link = links.firstOrNull() ?: run {
                rdDelete(key, id)
                throw NotCachedException(DebridService.REAL_DEBRID)
            }
            val unrestricted = parseObject(get(Request.Builder().url("$RD/unrestrict/link").bearer(key).post(FormBody.Builder().add("link", link).build()).build()))
            unrestricted?.get("download").str() ?: throw AppException(AppError.Unknown("debrid:bad_link"))
        }
    }

    private suspend fun rdDelete(key: String, id: String) {
        runCatching { get(Request.Builder().url("$RD/torrents/delete/$id").bearer(key).delete().build()) }
    }

    private suspend fun prepareTorbox(key: String, hash: String, magnet: String, maxPolls: Int): Prepared {
        val existing = runCatching {
            torboxExistingId(get(Request.Builder().url("$TORBOX/api/torrents/mylist?bypass_cache=true").bearer(key).build()), hash)
        }.getOrNull()
        val id = existing ?: run {
            val body = MultipartBody.Builder().setType(MultipartBody.FORM).addFormDataPart("magnet", magnet).build()
            val o = parseObject(get(Request.Builder().url("$TORBOX/api/torrents/createtorrent").bearer(key).post(body).build()))
            val data = o?.get("data").obj()
            (data?.get("torrent_id").str() ?: data?.get("id").str())?.takeIf { it.isNotBlank() } ?: throw AppException(AppError.Unknown("debrid:rejected"))
        }
        var info: TorboxInfo? = null
        for (attempt in 0 until maxPolls) {
            val url = "$TORBOX/api/torrents/mylist".toHttpUrl().newBuilder().addQueryParameter("id", id).addQueryParameter("bypass_cache", "true").build()
            info = parseTorboxInfo(get(Request.Builder().url(url).bearer(key).build()), id)
            info?.error?.let { throw AppException(AppError.Unknown("debrid:failed:$it")) }
            if (info?.present == true && info.files.isNotEmpty()) break
            delay(pollDelayMs)
        }
        val ready = info?.takeIf { it.present && it.files.isNotEmpty() } ?: throw NotCachedException(DebridService.TORBOX)
        return Prepared(ready.files) { file ->
            val url = "$TORBOX/api/torrents/requestdl".toHttpUrl().newBuilder()
                // TorBox requires the token in this endpoint's query
                .addQueryParameter("token", key)
                .addQueryParameter("torrent_id", id)
                .addQueryParameter("file_id", file.id)
                .addQueryParameter("redirect", "false")
                .build()
            parseObject(get(Request.Builder().url(url).bearer(key).build()))?.get("data").str() ?: throw AppException(AppError.Unknown("debrid:bad_link"))
        }
    }

    private suspend fun prepareAllDebrid(key: String, magnet: String, maxPolls: Int): Prepared {
        val upload = parseObject(get(Request.Builder().url("$ALLDEBRID_V4/magnet/upload").bearer(key).post(FormBody.Builder().add("magnets[]", magnet).build()).build()))
        val first = upload?.get("data").obj()?.get("magnets").arr()?.firstOrNull().obj()
        if (first?.get("error") != null) throw AppException(AppError.Unknown("debrid:rejected"))
        val id = first?.get("id").str() ?: throw AppException(AppError.Unknown("debrid:rejected"))
        var ready = false
        for (attempt in 0 until maxPolls) {
            val status = parseAllDebridStatus(
                get(Request.Builder().url("$ALLDEBRID_V41/magnet/status").bearer(key).post(FormBody.Builder().add("id", id).build()).build()),
                id,
            )
            if (status != null && status > 4) throw AppException(AppError.Unknown("debrid:failed:$status"))
            if (status == 4) {
                ready = true
                break
            }
            delay(pollDelayMs)
        }
        if (!ready) throw NotCachedException(DebridService.ALLDEBRID)
        val files = parseAllDebridFiles(
            get(Request.Builder().url("$ALLDEBRID_V4/magnet/files").bearer(key).post(FormBody.Builder().add("id[]", id).build()).build()),
            id,
        )
        if (files.isEmpty()) throw AppException(AppError.Unknown("debrid:no_file"))
        return Prepared(files) { file ->
            val o = parseObject(get(Request.Builder().url("$ALLDEBRID_V4/link/unlock").bearer(key).post(FormBody.Builder().add("link", file.id).build()).build()))
            o?.get("data").obj()?.get("link").str() ?: throw AppException(AppError.Unknown("debrid:bad_link"))
        }
    }

    // ------------------------------------------------------------------------------------------------ helpers

    private suspend fun fetchUser(service: DebridService, key: String): DebridUser? {
        val request = when (service) {
            DebridService.REAL_DEBRID -> Request.Builder().url("$RD/user").bearer(key).build()
            DebridService.TORBOX -> Request.Builder().url("$TORBOX/api/user/me?settings=false").bearer(key).build()
            DebridService.ALLDEBRID -> Request.Builder().url("$ALLDEBRID_V4/user").bearer(key).build()
            DebridService.PREMIUMIZE -> Request.Builder()
                .url("$PREMIUMIZE/account/info".toHttpUrl().newBuilder().addQueryParameter("apikey", key).build())
                .build()
        }
        return parseUser(service, get(request))
    }

    private suspend fun keyOf(service: DebridService): String? = secrets.get(SecretKeys.debridApiKey(service.id))?.takeIf { it.isNotBlank() }

    /** Body of a successful answer; `AppError.Http(code)` otherwise, `AppError.Offline` on network failures. Never logs the URL. */
    private suspend fun get(request: Request): String = withContext(Dispatchers.IO) {
        try {
            http.newCall(request.newBuilder().header("Accept", "application/json").build()).execute().use { response ->
                val body = response.body.string()
                if (!response.isSuccessful) throw AppException(AppError.Http(response.code))
                body
            }
        } catch (e: IOException) {
            throw AppException(AppError.Offline, e)
        }
    }

    private fun Request.Builder.bearer(key: String): Request.Builder = header("Authorization", "Bearer $key")

    private suspend fun persist() {
        val list = _accounts.value.map { StoredAccount(it.service, it.username, it.premiumUntilMs, it.linkedAtMs) }
        secrets.put(KEY_ACCOUNTS, debridJson.encodeToString(ListSerializer(StoredAccount.serializer()), list))
    }

    private fun StoredAccount.toModel() = DebridAccount(service, username, premiumUntilMs, linkedAtMs)

    private fun Exception.toApp(): AppException = when (this) {
        is AppException -> this
        is NotCachedException -> AppException(AppError.Unknown(message ?: "debrid:not_cached"), this)
        is IOException -> AppException(AppError.Offline, this)
        else -> AppException(AppError.Unknown(message ?: "debrid"), this)
    }

    private fun episodeOf(name: String): Int? = runCatching { parseRelease(name.substringAfterLast('/')).episodes?.first }.getOrNull()

    companion object {
        private const val TAG = "debrid"
        const val KEY_ACCOUNTS = "debrid.accounts"
        const val TORRENT_EXTENSION_ID = "torrent"
        private const val RD = "https://api.real-debrid.com/rest/1.0"
        private const val TORBOX = "https://api.torbox.app/v1"
        private const val ALLDEBRID_V4 = "https://api.alldebrid.com/v4"
        private const val ALLDEBRID_V41 = "https://api.alldebrid.com/v4.1"
        private const val PREMIUMIZE = "https://www.premiumize.me/api"
        private const val CALL_TIMEOUT_S = 20L
        private const val POLL_MS = 1_000L
        private const val FULL_POLLS = 30
        private const val PROBE_POLLS = 6
        private const val RD_NOT_CACHED_AFTER = 2
        private const val CACHE_BATCH = 50
        private val HEX40 = Regex("[0-9a-f]{40}")
        private val PROBED = setOf(DebridService.REAL_DEBRID, DebridService.ALLDEBRID)

        /**
         * [SourceCandidate.debrid] for one candidate: the first linked service (preference order) that has it cached; otherwise
         * "not cached" on the first linked service that can tell; otherwise null (only RD / AllDebrid linked: unknown until played).
         */
        internal fun debridStatusFor(c: SourceCandidate, linked: List<DebridService>, cached: Map<String, Set<DebridService>>): DebridStatus? {
            if (c.kind != SourceKind.TORRENT) return null
            val hash = c.torrent?.infoHash?.lowercase(Locale.ROOT) ?: return null
            val on = cached[hash].orEmpty()
            linked.firstOrNull { it in on }?.let { return DebridStatus(it, cached = true) }
            linked.firstOrNull { it !in PROBED }?.let { return DebridStatus(it, cached = false) }
            return null
        }
    }
}
