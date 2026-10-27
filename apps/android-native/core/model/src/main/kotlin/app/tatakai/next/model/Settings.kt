package app.tatakai.next.model

enum class ContrastPref { STANDARD, MEDIUM, HIGH }

/** [paletteId] is a key of packages/tokens palettes (default "tatakai-violet"). */
data class AppSettings(
    val paletteId: String = DEFAULT_PALETTE_ID,
    val amoled: Boolean = false,
    val contrast: ContrastPref = ContrastPref.STANDARD,
    val dynamicColor: Boolean = true,
    val providerPriority: List<String> = DEFAULT_PROVIDER_PRIORITY,
    val playerHeadersDebug: Boolean = false,
) {
    companion object {
        const val DEFAULT_PALETTE_ID = "tatakai-violet"
        val DEFAULT_PROVIDER_PRIORITY: List<String> = listOf("anilist", "anizip", "kitsu")
    }
}

/** Only non-secret session info is exposed; tokens stay inside :core:supabase. */
data class UserSession(val userId: String, val email: String?)
