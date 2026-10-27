package app.tatakai.next.model

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
)

@Serializable
data class Episode(
    val number: Int,
    val title: String?,
    val image: String?,
    val airDate: String?,
    val isFiller: Boolean = false,
)

data class FeedPage(val items: List<Media>, val page: Int, val hasNext: Boolean)
