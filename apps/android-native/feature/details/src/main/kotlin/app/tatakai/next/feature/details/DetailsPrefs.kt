// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.details

import android.content.Context

/**
 * Module-private persistence of Details (15 §3.5: a field that AppSettings does not carry is kept privately by its owner):
 * the 18+ gate decisions per title (`details.adult-gate`) and the episode list view mode (`details.episodes.view-mode`).
 */
interface DetailsPrefs {
    fun adultAccepted(anilistId: Int): Boolean

    fun acceptAdult(anilistId: Int)

    fun episodeView(): EpisodeView

    fun setEpisodeView(view: EpisodeView)
}

/** LIST = thumbnails with title and pills (prototype `.ep`); COMPACT = dense number grid (classic compact mode). */
enum class EpisodeView { LIST, COMPACT }

class SharedPrefsDetailsPrefs(context: Context) : DetailsPrefs {
    private val prefs = context.applicationContext.getSharedPreferences(FILE, Context.MODE_PRIVATE)

    override fun adultAccepted(anilistId: Int): Boolean =
        prefs.getStringSet(KEY_ADULT, emptySet()).orEmpty().contains(anilistId.toString())

    override fun acceptAdult(anilistId: Int) {
        val next = prefs.getStringSet(KEY_ADULT, emptySet()).orEmpty().toMutableSet().apply { add(anilistId.toString()) }
        prefs.edit().putStringSet(KEY_ADULT, next).apply()
    }

    override fun episodeView(): EpisodeView =
        prefs.getString(KEY_VIEW, null)?.let { v -> EpisodeView.entries.firstOrNull { it.name == v } } ?: EpisodeView.LIST

    override fun setEpisodeView(view: EpisodeView) {
        prefs.edit().putString(KEY_VIEW, view.name).apply()
    }

    private companion object {
        const val FILE = "tatakai_details"
        const val KEY_ADULT = "adult_ok"
        const val KEY_VIEW = "episode_view"
    }
}

/** Default for tests and previews. */
class InMemoryDetailsPrefs : DetailsPrefs {
    private val adult = mutableSetOf<Int>()
    private var view = EpisodeView.LIST

    override fun adultAccepted(anilistId: Int): Boolean = anilistId in adult

    override fun acceptAdult(anilistId: Int) {
        adult += anilistId
    }

    override fun episodeView(): EpisodeView = view

    override fun setEpisodeView(view: EpisodeView) {
        this.view = view
    }
}
