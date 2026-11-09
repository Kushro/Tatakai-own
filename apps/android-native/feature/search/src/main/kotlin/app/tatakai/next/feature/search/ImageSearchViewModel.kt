// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.search

import android.content.Context
import android.net.Uri
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.model.AppError
import app.tatakai.next.model.ImageMatch
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MetadataRepository
import app.tatakai.next.ui.state.runCatchingApp
import app.tatakai.next.ui.state.toAppError
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** Reads a picked / shared image (`content://`, `file://`) as bytes + MIME type; null when it cannot be opened. */
fun interface ImageBytesReader {
    suspend fun read(uri: String): PickedImage?
}

class PickedImage(val bytes: ByteArray, val mimeType: String)

/** ContentResolver-backed reader (SAF picker results and ACTION_SEND grants). */
class ContentImageReader(context: Context) : ImageBytesReader {
    private val resolver = context.applicationContext.contentResolver

    override suspend fun read(uri: String): PickedImage? = withContext(Dispatchers.IO) {
        runCatching {
            val parsed = Uri.parse(uri)
            val mime = resolver.getType(parsed) ?: "image/jpeg"
            val bytes = resolver.openInputStream(parsed)?.use { it.readBytes() } ?: return@runCatching null
            PickedImage(bytes, mime)
        }.getOrNull()
    }
}

sealed interface ImageSearchResults {
    /** Nothing picked yet. */
    data object Idle : ImageSearchResults

    data object Loading : ImageSearchResults

    data class Ok(val matches: List<ImageMatch>) : ImageSearchResults

    data class Err(val cause: AppError) : ImageSearchResults

    /** The picked file could not be read (permission revoked, not an image). */
    data object Unreadable : ImageSearchResults

    /** Larger than trace.moe accepts. */
    data object TooLarge : ImageSearchResults
}

data class ImageSearchUiState(
    val imageUri: String? = null,
    val results: ImageSearchResults = ImageSearchResults.Idle,
    /** Classic default: 85 % similarity. */
    val threshold: Float = DEFAULT_THRESHOLD,
    /** A match is being resolved to its Media before opening Details. */
    val opening: Boolean = false,
) {
    val shown: List<ImageMatch>
        get() = (results as? ImageSearchResults.Ok)?.matches?.let { rankMatches(it, threshold) }.orEmpty()

    /** Matches hidden by the threshold (shown as "N below X %"). */
    val hiddenCount: Int
        get() = ((results as? ImageSearchResults.Ok)?.matches?.let { bestPerTitle(it).size } ?: 0) - shown.size

    companion object {
        const val DEFAULT_THRESHOLD = 0.85f
    }
}

/** One hit per title (best similarity kept), best first (classic `SearchPage` dedupe by AniList id). */
internal fun bestPerTitle(matches: List<ImageMatch>): List<ImageMatch> =
    matches.groupBy { it.ref.anilistId ?: it.ref.malId ?: it.title.hashCode() }
        .values
        .map { group -> group.maxBy { it.similarity } }
        .sortedByDescending { it.similarity }

/** [bestPerTitle] at or above [threshold]. */
internal fun rankMatches(matches: List<ImageMatch>, threshold: Float): List<ImageMatch> =
    bestPerTitle(matches).filter { it.similarity >= threshold }

sealed interface ImageSearchEvent {
    data class Open(val media: Media) : ImageSearchEvent

    data class OpenFailed(val cause: AppError) : ImageSearchEvent
}

class ImageSearchViewModel(
    private val metadata: MetadataRepository,
    private val reader: ImageBytesReader,
) : ViewModel() {
    private val _state = MutableStateFlow(ImageSearchUiState())
    val state: StateFlow<ImageSearchUiState> = _state.asStateFlow()

    private val _events = Channel<ImageSearchEvent>(Channel.BUFFERED)
    val events: Flow<ImageSearchEvent> = _events.receiveAsFlow()

    private var job: Job? = null

    /** Shared image (Route.ImageSearch(uri)) or a picker result; the same uri twice is not searched again. */
    fun search(uri: String, force: Boolean = false) {
        if (!force && uri == _state.value.imageUri && _state.value.results !is ImageSearchResults.Idle) return
        job?.cancel()
        _state.update { it.copy(imageUri = uri, results = ImageSearchResults.Loading) }
        job = viewModelScope.launch {
            val image = reader.read(uri)
            if (image == null || image.bytes.isEmpty()) {
                _state.update { it.copy(results = ImageSearchResults.Unreadable) }
                return@launch
            }
            if (image.bytes.size > MAX_BYTES) {
                _state.update { it.copy(results = ImageSearchResults.TooLarge) }
                return@launch
            }
            val result = runCatchingApp { metadata.searchByImage(image.bytes, image.mimeType) }
            _state.update {
                it.copy(
                    results = result.fold(
                        onSuccess = { list -> ImageSearchResults.Ok(list) },
                        onFailure = { e -> ImageSearchResults.Err(e.toAppError()) },
                    ),
                )
            }
        }
    }

    fun retry() {
        _state.value.imageUri?.let { search(it, force = true) }
    }

    fun setThreshold(value: Float) = _state.update { it.copy(threshold = value.coerceIn(0f, 1f)) }

    /** Resolves the match's AniList entry to a [Media] (Details takes a Media) and emits [ImageSearchEvent.Open]. */
    fun open(match: ImageMatch) {
        if (_state.value.opening) return
        _state.update { it.copy(opening = true) }
        viewModelScope.launch {
            val result = runCatchingApp { metadata.media(MediaKind.ANIME, match.ref) }
            _state.update { it.copy(opening = false) }
            result.fold(
                onSuccess = { media ->
                    _events.trySend(if (media != null) ImageSearchEvent.Open(media) else ImageSearchEvent.OpenFailed(AppError.Http(404)))
                },
                onFailure = { e -> _events.trySend(ImageSearchEvent.OpenFailed(e.toAppError())) },
            )
        }
    }

    private companion object {
        /** trace.moe upload limit (TraceMoeClient.MAX_BYTES). */
        const val MAX_BYTES = 25 * 1024 * 1024
    }
}
