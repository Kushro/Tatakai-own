package app.tatakai.next.feature.search

import android.content.Context

/** Recent searches (prototype `settings.searchHistory`: newest first, at most [SEARCH_HISTORY_SIZE], local to the device). */
interface SearchHistoryStore {
    fun load(): List<String>

    fun save(entries: List<String>)
}

const val SEARCH_HISTORY_SIZE = 6

/** Module-private persistence (12 §4 c: "persistencia privada del módulo"): one SharedPreferences file, entries joined by newlines. */
class SharedPrefsSearchHistoryStore(context: Context) : SearchHistoryStore {
    private val prefs = context.applicationContext.getSharedPreferences(FILE, Context.MODE_PRIVATE)

    override fun load(): List<String> =
        prefs.getString(KEY, null)
            ?.split('\n')
            ?.map { it.trim() }
            ?.filter { it.isNotEmpty() }
            ?.take(SEARCH_HISTORY_SIZE)
            .orEmpty()

    override fun save(entries: List<String>) {
        prefs.edit().putString(KEY, entries.take(SEARCH_HISTORY_SIZE).joinToString("\n")).apply()
    }

    private companion object {
        const val FILE = "tatakai_search_history"
        const val KEY = "entries"
    }
}

/** Default for tests and previews: keeps the history in memory only. */
class InMemorySearchHistoryStore(initial: List<String> = emptyList()) : SearchHistoryStore {
    private var entries = initial

    override fun load(): List<String> = entries

    override fun save(entries: List<String>) {
        this.entries = entries
    }
}

/** Newest first, case-sensitive dedupe on the trimmed text, capped to [SEARCH_HISTORY_SIZE]. Queries shorter than 2 chars are ignored. */
fun List<String>.withSearch(query: String): List<String> {
    val q = query.trim()
    if (q.length < 2) return this
    return (listOf(q) + filter { it != q }).take(SEARCH_HISTORY_SIZE)
}
