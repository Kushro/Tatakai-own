package app.tatakai.next.model

import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.StateFlow

interface MetadataRepository {
    suspend fun feed(kind: MediaKind, feed: FeedKind, page: Int = 1): FeedPage
    suspend fun search(kind: MediaKind, query: String, page: Int = 1): FeedPage
    suspend fun media(kind: MediaKind, ref: MediaRef): Media?
    suspend fun episodes(ref: MediaRef): List<Episode>
}

interface HistoryRepository {
    fun continueWatching(limit: Int = 20): Flow<List<HistoryEntry>>
    suspend fun record(entry: HistoryEntry)
    fun progressFor(anilistId: Int): Flow<Map<Int, HistoryEntry>>
}

interface AuthRepository {
    val session: StateFlow<UserSession?>
    suspend fun signIn(email: String, password: String): Result<Unit>
    suspend fun signOut()
}

interface SettingsRepository {
    val settings: StateFlow<AppSettings>
    suspend fun update(transform: (AppSettings) -> AppSettings)
}

interface ExtensionRepository {
    val installed: StateFlow<List<InstalledExtension>>
    suspend fun installFromUri(uri: String): Result<InstalledExtension>
    suspend fun setEnabled(id: String, enabled: Boolean)
    suspend fun remove(id: String)
    suspend fun resolveStreams(media: Media, episode: Int): List<StreamSource>
}

interface LinkRepository {
    val state: StateFlow<LinkState>

    /** M0: always empty (mDNS is M1); pairing is by QR / pasted payload. */
    fun discover(): Flow<List<DiscoveredPc>>
    suspend fun pair(payload: String, deviceName: String): Result<PairedPc>
    suspend fun capabilities(): Result<LinkCapabilities>
    suspend fun startSession(source: LinkSource, profileId: String?): Result<RemoteSession>
    suspend fun stop(sessionId: String)
    suspend fun forget()
}
