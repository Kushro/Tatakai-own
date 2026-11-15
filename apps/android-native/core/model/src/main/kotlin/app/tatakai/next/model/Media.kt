package app.tatakai.next.model

import kotlinx.serialization.EncodeDefault
import kotlinx.serialization.ExperimentalSerializationApi
import kotlinx.serialization.Serializable

enum class MediaKind { ANIME, MANGA }

enum class FeedKind { TRENDING, POPULAR, TOP_RATED, AIRING, UPCOMING, SEASONAL, RECENTLY_COMPLETED }

@Serializable
data class MediaRef(val anilistId: Int? = null, val malId: Int? = null, val kitsuId: Int? = null)

@Serializable
data class Media(
    val ref: MediaRef,
    val kind: MediaKind,
    val title: String,
    val titles: Map<String, String>,
    val cover: String?,
    val banner: String?,
    val color: String?,
    val synopsis: String?,
    val genres: List<String>,
    val episodes: Int?,
    val status: String?,
    val season: String?,
    val year: Int?,
    val score: Int?,
    val sourceProvider: String,
    /**
     * BUG-143: titles known from another provider than the one that answered (e.g. Kitsu's spelling when AniList answered before,
     * or AniList's titles kept when a fallback answered). Only filled by `MetadataRepositoryImpl` when a fallback provider answers;
     * a normal answer has none, so [searchTitles] keeps its order. Never encoded when empty: the JSON of a normal Media is unchanged.
     */
    @OptIn(ExperimentalSerializationApi::class)
    @EncodeDefault(EncodeDefault.Mode.NEVER)
    val altTitles: List<String> = emptyList(),
)

/**
 * The titles sent to the extensions / the PC when looking for sources: [Media.title], then [Media.titles] in their order, then
 * [Media.altTitles]; blanks and duplicates dropped. Without alt titles this is exactly the list the app always sent (BUG-143).
 */
fun Media.searchTitles(): List<String> =
    (listOf(title) + titles.values + altTitles).filter { it.isNotBlank() }.distinct()

/** [extra] titles not already in [searchTitles] (case-insensitive) appended to [Media.altTitles]. */
fun Media.withAltTitles(extra: List<String>): Media {
    val seen = searchTitles().mapTo(HashSet()) { it.trim().lowercase() }
    val add = extra.filter { it.isNotBlank() && seen.add(it.trim().lowercase()) }
    return if (add.isEmpty()) this else copy(altTitles = altTitles + add)
}

@Serializable
data class Episode(
    val number: Int,
    val title: String?,
    val image: String?,
    val airDate: String?,
    val isFiller: Boolean = false,
)

data class FeedPage(val items: List<Media>, val page: Int, val hasNext: Boolean)
