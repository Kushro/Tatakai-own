// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.data

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.longPreferencesKey
import androidx.datastore.preferences.core.stringPreferencesKey
import app.tatakai.next.database.TatakaiDatabase
import app.tatakai.next.link.AesGcmKeyCipher
import app.tatakai.next.link.KeyCipher
import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppException
import app.tatakai.next.model.AppReset
import app.tatakai.next.model.ExtensionRepository
import app.tatakai.next.model.LinkRepository
import app.tatakai.next.model.LogLevel
import app.tatakai.next.model.LogRepository
import app.tatakai.next.model.LogSink
import app.tatakai.next.model.MetadataRepository
import app.tatakai.next.model.SecretStore
import app.tatakai.next.model.UpdateInfo
import app.tatakai.next.model.UpdateRepository
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.serialization.Serializable
import kotlinx.serialization.builtins.MapSerializer
import kotlinx.serialization.builtins.serializer
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import okhttp3.OkHttpClient
import okhttp3.Request
import java.io.File
import java.security.KeyStore
import java.time.Instant
import java.time.OffsetDateTime
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter
import java.util.concurrent.Executors
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey

// Parity wave A (docs/redesign/15-parity-wave-a-contract-android.md §3.4). Owner: worker q.

// ==================================================================== app log

/**
 * File-backed app log under [dir] (`filesDir/logs/app.log`, rotated at [maxBytes] into `app.1.log`), installed as `AppLog.sink` by
 * `dataModule`. Settings › Acerca de › Abrir registros shows [recentLines] and shares [logFile] through the `FileProvider` declared in
 * the :core:data manifest (`${applicationId}.logs`). Lines are scrubbed ([scrub]): no query strings, no bearer tokens, no keys.
 * Writes happen on one background thread, so [log] never blocks the caller. [mirror] also sends the line to logcat (tag `Tatakai`).
 */
class AppLogStore(
    private val dir: File,
    private val maxBytes: Long = MAX_BYTES,
    private val mirror: Boolean = true,
    private val clock: () -> Long = System::currentTimeMillis,
) : LogRepository, LogSink {
    private val writer = Executors.newSingleThreadExecutor { r -> Thread(r, "tatakai-log").apply { isDaemon = true } }
    private val lock = Any()

    private val current: File get() = File(dir, FILE)
    private val rotated: File get() = File(dir, ROTATED)

    override fun log(level: LogLevel, tag: String, message: String, throwable: Throwable?) {
        val line = format(clock(), level, tag, message, throwable)
        if (mirror) {
            runCatching {
                when (level) {
                    LogLevel.INFO -> android.util.Log.i(LOGCAT_TAG, line)
                    LogLevel.WARN -> android.util.Log.w(LOGCAT_TAG, line)
                    LogLevel.ERROR -> android.util.Log.e(LOGCAT_TAG, line)
                }
            }
        }
        runCatching { writer.execute { append(line) } }
    }

    /** Synchronous append (tests and the writer thread). */
    internal fun append(line: String) {
        synchronized(lock) {
            try {
                dir.mkdirs()
                val file = current
                if (file.isFile && file.length() + line.length + 1 > maxBytes) {
                    rotated.delete()
                    file.renameTo(rotated)
                }
                current.appendText(line + "\n", Charsets.UTF_8)
            } catch (_: Exception) {
                // the log must never break the app
            }
        }
    }

    override suspend fun recentLines(max: Int): List<String> = withContext(Dispatchers.IO) {
        flush()
        synchronized(lock) {
            val lines = buildList {
                if (rotated.isFile) addAll(rotated.readLines(Charsets.UTF_8))
                if (current.isFile) addAll(current.readLines(Charsets.UTF_8))
            }.filter { it.isNotBlank() }
            lines.takeLast(max.coerceAtLeast(0))
        }
    }

    override fun logFile(): File? = current.takeIf { it.isFile && it.length() > 0 }

    override suspend fun clear() = withContext(Dispatchers.IO) {
        flush()
        synchronized(lock) {
            current.delete()
            rotated.delete()
        }
        Unit
    }

    /** Waits for the lines queued so far (bounded). */
    private fun flush() {
        runCatching { writer.submit {}.get(FLUSH_TIMEOUT_MS, java.util.concurrent.TimeUnit.MILLISECONDS) }
    }

    companion object {
        const val FILE = "app.log"
        const val ROTATED = "app.1.log"
        const val MAX_BYTES = 512L * 1024
        private const val LOGCAT_TAG = "Tatakai"
        private const val FLUSH_TIMEOUT_MS = 2_000L
        private val TIME: DateTimeFormatter = DateTimeFormatter.ofPattern("yyyy-MM-dd'T'HH:mm:ss.SSS'Z'").withZone(ZoneOffset.UTC)

        /** `2026-11-09T21:04:11.000Z I/player: message (IOException: reset)` with [scrub] applied. */
        fun format(epochMs: Long, level: LogLevel, tag: String, message: String, throwable: Throwable?): String {
            val letter = when (level) {
                LogLevel.INFO -> 'I'
                LogLevel.WARN -> 'W'
                LogLevel.ERROR -> 'E'
            }
            val cause = throwable?.let { t -> " (${t::class.java.simpleName}${t.message?.let { ": $it" }.orEmpty()})" }.orEmpty()
            val text = scrub("$message$cause").replace('\n', ' ').replace('\r', ' ')
            return "${TIME.format(Instant.ofEpochMilli(epochMs))} $letter/${tag.take(32)}: $text"
        }

        private val QUERY = Regex("""(https?://[^\s?#]+)\?[^\s]*""")
        private val BEARER = Regex("""(?i)(bearer\s+)[A-Za-z0-9._\-~+/=]+""")
        private val SECRET_PARAM = Regex("""(?i)\b(access_token|refresh_token|token|apikey|api_key|code|code_verifier|client_secret)=([^\s&]+)""")

        /** Drops URL query strings and redacts bearer tokens / key parameters (the file is meant to be shared in bug reports). */
        fun scrub(text: String): String = text
            .replace(QUERY) { it.groupValues[1] + "?…" }
            .replace(BEARER) { it.groupValues[1] + "…" }
            .replace(SECRET_PARAM) { it.groupValues[1] + "=…" }
    }
}

// ==================================================================== update check

/**
 * New-version notice: GitHub Releases API (`https://api.github.com/repos/$REPO/releases/latest`), compared with the installed
 * [installedVersion]; at most one request per 24 h (stamp + last answer in the settings DataStore), none when [enabled] is false
 * unless forced. Until a release with an APK exists the check answers "up to date" (null). Starts one unforced check at creation.
 */
class UpdateRepositoryImpl(
    private val client: OkHttpClient,
    private val installedVersion: String,
    private val enabled: () -> Boolean = { true },
    private val store: DataStore<Preferences>? = null,
    private val clock: () -> Long = System::currentTimeMillis,
    scope: CoroutineScope? = CoroutineScope(SupervisorJob() + Dispatchers.IO),
    private val endpoint: String = "https://api.github.com/repos/$REPO/releases/latest",
) : UpdateRepository {
    private val _latest = MutableStateFlow<UpdateInfo?>(null)
    override val latest: StateFlow<UpdateInfo?> = _latest.asStateFlow()
    private val mutex = Mutex()

    init {
        scope?.launch {
            delay(STARTUP_DELAY_MS)
            runCatching { check(force = false) }
        }
    }

    override suspend fun check(force: Boolean): Result<UpdateInfo?> = mutex.withLock {
        try {
            if (!force && !enabled()) return@withLock Result.success(null)
            val prefs = store?.let { runCatching { it.data.first() }.getOrNull() }
            val last = prefs?.get(KEY_LAST_CHECK) ?: 0L
            if (!force && clock() - last < DAY_MS) {
                // throttled: replay the last answer so the startup notice still shows
                val cached = prefs?.get(KEY_LAST_ANSWER)?.let { runCatching { json.decodeFromString(Cached.serializer(), it) }.getOrNull() }
                val info = cached?.toInfo()?.takeIf { isNewer(it.version, installedVersion) }
                _latest.value = info
                return@withLock Result.success(info)
            }
            val info = withContext(Dispatchers.IO) { fetch() }
            store?.let { s ->
                runCatching {
                    s.edit { p ->
                        p[KEY_LAST_CHECK] = clock()
                        if (info == null) p.remove(KEY_LAST_ANSWER) else p[KEY_LAST_ANSWER] = json.encodeToString(Cached.serializer(), Cached.of(info))
                    }
                }
            }
            val newer = info?.takeIf { isNewer(it.version, installedVersion) }
            _latest.value = newer
            Result.success(newer)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            Result.failure(if (e is AppException) e else AppException(AppError.Offline, e))
        }
    }

    private fun fetch(): UpdateInfo? {
        val request = Request.Builder().url(endpoint)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .build()
        client.newCall(request).execute().use { response ->
            if (response.code == 404) return null // no release yet: up to date
            if (!response.isSuccessful) throw AppException(AppError.Http(response.code))
            return parseRelease(response.body.string())
        }
    }

    @Serializable
    private data class Cached(val version: String, val url: String, val notes: String? = null, val publishedAtMs: Long? = null) {
        fun toInfo() = UpdateInfo(version, url, notes, publishedAtMs)

        companion object {
            fun of(i: UpdateInfo) = Cached(i.version, i.url, i.notes, i.publishedAtMs)
        }
    }

    companion object {
        const val REPO = "Kushro/Tatakai-own"
        private const val DAY_MS = 24L * 60 * 60 * 1000
        private const val STARTUP_DELAY_MS = 5_000L
        private val KEY_LAST_CHECK = longPreferencesKey("update_last_check_ms")
        private val KEY_LAST_ANSWER = stringPreferencesKey("update_last_answer_json")
        private val json = Json { ignoreUnknownKeys = true }

        /**
         * `releases/latest` JSON -> [UpdateInfo]; null for drafts, pre-releases and releases without an `.apk` asset (desktop-only
         * releases of the same repository). The URL is the release page (`html_url`), the user downloads from there.
         */
        fun parseRelease(body: String): UpdateInfo? {
            val o = json.parseToJsonElement(body).jsonObject
            if (o["draft"]?.jsonPrimitive?.booleanOrNull == true || o["prerelease"]?.jsonPrimitive?.booleanOrNull == true) return null
            val tag = o["tag_name"]?.jsonPrimitive?.contentOrNull?.takeIf { it.isNotBlank() } ?: return null
            val assets = (o["assets"] as? JsonArray)?.mapNotNull { a -> a.jsonObject["name"]?.jsonPrimitive?.contentOrNull }.orEmpty()
            if (assets.none { it.endsWith(".apk", ignoreCase = true) }) return null
            val url = o["html_url"]?.jsonPrimitive?.contentOrNull?.takeIf { it.startsWith("https://github.com/") }
                ?: "https://github.com/$REPO/releases/tag/$tag"
            val published = o["published_at"]?.jsonPrimitive?.contentOrNull?.let { runCatching { OffsetDateTime.parse(it).toInstant().toEpochMilli() }.getOrNull() }
            val notes = o["body"]?.jsonPrimitive?.contentOrNull?.takeIf { it.isNotBlank() }?.take(4_000)
            return UpdateInfo(version = tag.trim().removePrefix("v").removePrefix("V"), url = url, notes = notes, publishedAtMs = published)
        }

        /**
         * Numeric dotted comparison (`1.10.0 > 1.9.3`), leading `v` ignored; a pre-release suffix (`1.2.0-beta.1`) sorts below the
         * plain version. Unparseable versions are never "newer".
         */
        fun isNewer(candidate: String, installed: String): Boolean {
            val a = parseVersion(candidate) ?: return false
            val b = parseVersion(installed) ?: return true
            val n = maxOf(a.first.size, b.first.size)
            for (i in 0 until n) {
                val x = a.first.getOrElse(i) { 0 }
                val y = b.first.getOrElse(i) { 0 }
                if (x != y) return x > y
            }
            // same numbers: a release beats a pre-release of it
            return !a.second && b.second
        }

        /** (numbers, isPreRelease) */
        private fun parseVersion(v: String): Pair<List<Int>, Boolean>? {
            val clean = v.trim().removePrefix("v").removePrefix("V")
            val core = clean.substringBefore('-').substringBefore('+').substringBefore(' ')
            val numbers = core.split('.').map { it.toIntOrNull() ?: return null }
            if (numbers.isEmpty()) return null
            return numbers to clean.contains('-')
        }
    }
}

// ==================================================================== secrets

/** The non-exportable AES key of the secret store (alias [ALIAS]) in AndroidKeyStore; created on first use. */
object SecretsKeystoreKey {
    private const val ANDROID_KEYSTORE = "AndroidKeyStore"
    const val ALIAS = "tatakai-secrets"

    fun cipher(): KeyCipher = AesGcmKeyCipher(::secretKey)

    private fun secretKey(): SecretKey {
        val keyStore = KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }
        (keyStore.getKey(ALIAS, null) as? SecretKey)?.let { return it }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, ANDROID_KEYSTORE)
        generator.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        return generator.generateKey()
    }
}

/**
 * [SecretStore] sealed with an AndroidKeyStore AES-GCM key: one JSON map in `secrets.bin` under [dir] (`noBackupFilesDir`), the
 * `LinkKeyVault` / `ExtensionServerStore` pattern (key alias `tatakai-secrets`, random IV per write, atomic rename). Holds the AniList /
 * MAL token sets, debrid API keys and translation API keys (`SecretKeys`). A file that does not authenticate (key lost after a restore
 * to another device) reads as empty. [cipher] is injectable so the JVM tests use a plain AES key.
 */
class KeystoreSecretStore(
    private val dir: File,
    private val cipher: KeyCipher = SecretsKeystoreKey.cipher(),
) : SecretStore {
    private val mutex = Mutex()
    private var cache: Map<String, String>? = null
    private val file: File get() = File(dir, FILE)

    override suspend fun get(key: String): String? = mutex.withLock { load()[key] }

    override suspend fun put(key: String, value: String?) = mutex.withLock {
        val current = load()
        val next = if (value == null) current - key else current + (key to value)
        if (next != current) save(next)
    }

    override suspend fun clear() = mutex.withLock {
        withContext(Dispatchers.IO) {
            file.delete()
            File(dir, "$FILE.tmp").delete()
        }
        cache = emptyMap()
    }

    private suspend fun load(): Map<String, String> {
        cache?.let { return it }
        val loaded = withContext(Dispatchers.IO) {
            try {
                if (!file.isFile) return@withContext emptyMap()
                val plain = cipher.open(file.readBytes()) ?: return@withContext emptyMap()
                json.decodeFromString(MAP, plain.decodeToString())
            } catch (_: Exception) {
                emptyMap()
            }
        }
        cache = loaded
        return loaded
    }

    private suspend fun save(map: Map<String, String>) {
        withContext(Dispatchers.IO) {
            dir.mkdirs()
            val sealed = cipher.seal(json.encodeToString(MAP, map).toByteArray(Charsets.UTF_8))
            val tmp = File(dir, "$FILE.tmp")
            tmp.writeBytes(sealed)
            if (!tmp.renameTo(file)) {
                file.delete()
                tmp.renameTo(file)
            }
        }
        cache = map
    }

    private companion object {
        const val FILE = "secrets.bin"
        val MAP = MapSerializer(String.serializer(), String.serializer())
        val json = Json { ignoreUnknownKeys = true }
    }
}

// ==================================================================== reset

/**
 * "Restablecer la app" (checklist system.reset-app): every step is best effort (one failing collaborator never leaves the app half
 * reset without trying the rest) and the first failure is reported after all ran. Order: extensions, Link pairing (`forget`, which also
 * revokes this phone on the PC when reachable), history cursor, secrets, metadata cache, Room (`clearAllTables`), app log, shaders,
 * other modules' SharedPreferences, image cache, then the settings DataStore last (so `onboardingDone` reads false only when the rest
 * is gone). The UI recreates the activity afterwards and the shell shows the first-run wizard again.
 */
class AppResetImpl(
    private val context: Context,
    private val settingsStore: DataStore<Preferences>,
    private val database: TatakaiDatabase,
    private val secrets: SecretStore,
    private val metadata: MetadataRepository? = null,
    private val extensions: ExtensionRepository? = null,
    private val link: LinkRepository? = null,
    private val logs: LogRepository? = null,
    private val history: HistorySyncer? = null,
) : AppReset {
    override suspend fun resetAll(): Result<Unit> = withContext(Dispatchers.IO) {
        var failure: Throwable? = null
        suspend fun step(block: suspend () -> Unit) {
            try {
                block()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                if (failure == null) failure = e
            }
        }
        step { extensions?.let { repo -> repo.installed.value.forEach { ext -> repo.remove(ext.id) } } }
        step { link?.forget() }
        step { history?.resetStamp() }
        step { secrets.clear() }
        step { metadata?.clearCache() }
        step { database.clearAllTables() }
        step { logs?.clear() }
        step { File(context.filesDir, "shaders").deleteRecursively() }
        step {
            val prefsDir = File(context.applicationInfo.dataDir, "shared_prefs")
            prefsDir.listFiles()?.filter { it.name.endsWith(".xml") }?.forEach { f ->
                val name = f.name.removeSuffix(".xml")
                context.getSharedPreferences(name, Context.MODE_PRIVATE).edit().clear().commit()
                context.deleteSharedPreferences(name)
            }
        }
        step { context.cacheDir.listFiles()?.forEach { it.deleteRecursively() } }
        step {
            settingsStore.edit { it.clear() }
            // `ls files/` shows no settings file afterwards; DataStore recreates it on the next write.
            File(context.filesDir, "datastore/settings.preferences_pb").delete()
        }
        AppLogAfterReset.note()
        failure?.let { Result.failure(AppException(AppError.Unknown("reset:${it::class.java.simpleName}"), it)) } ?: Result.success(Unit)
    }
}

/** Writes one line into the (now empty) log so a shared log shows that the app was reset. */
private object AppLogAfterReset {
    fun note() = app.tatakai.next.model.AppLog.i("reset", "app reset to first run")
}

/** Shared JSON helpers of :core:data (lenient reads of provider answers). */
internal fun JsonObject.str(name: String): String? = this[name]?.let { runCatching { it.jsonPrimitive.contentOrNull }.getOrNull() }

internal fun JsonObject.obj(name: String): JsonObject? = this[name] as? JsonObject

internal fun JsonObject.arr(name: String): JsonArray? = this[name] as? JsonArray

internal fun JsonObject.int(name: String): Int? = str(name)?.toDoubleOrNull()?.toInt()

internal fun JsonObject.long(name: String): Long? = str(name)?.toDoubleOrNull()?.toLong()

internal fun JsonObject.bool(name: String): Boolean? = this[name]?.let { runCatching { it.jsonPrimitive.booleanOrNull }.getOrNull() }

internal fun JsonArray.objectList(): List<JsonObject> = mapNotNull { it as? JsonObject }
