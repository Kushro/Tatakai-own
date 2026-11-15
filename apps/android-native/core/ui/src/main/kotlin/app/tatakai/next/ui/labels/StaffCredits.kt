// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.ui.labels

import app.tatakai.next.model.StaffEdge
import java.util.Locale

// BUG-133: AniList staff roles of a novel come with notes in English ("Translator (English: vols 1, 3, 5)", "Story (ch 1-20)"). The
// page shows the base role translated without the note, and the translators apart, grouped by language ("Traducción al inglés:
// Kevin Frane, Paul Cuneo, Alyssa Orton-Niioka y 3 más"). Pure, unit tested; same rules as the desktop (tk-ui novel details).

/** An AniList role split into its base ("Translator") and, for translators, the language of the note (ISO 639 code, "en"). */
data class StaffRoleParts(val base: String, val languageCode: String? = null) {
    val translation: Boolean get() = isTranslationRole(base)
}

/** "Translator (English: vols 1, 3)" -> base "Translator", language "en"; "Story (ch 1-20)" -> base "Story", no language. */
fun parseStaffRole(role: String): StaffRoleParts {
    val text = role.trim()
    val open = text.indexOf('(')
    val base = (if (open >= 0) text.substring(0, open) else text).trim()
    if (open < 0 || !isTranslationRole(base)) return StaffRoleParts(base)
    val note = text.substring(open + 1).substringBeforeLast(')')
    val language = note.substringBefore(':').substringBefore(',').substringBefore(';').trim()
    return StaffRoleParts(base, languageCodeOf(language))
}

/** AniList's translation roles ("Translator", "Translation"; "Translator & …" too). */
fun isTranslationRole(base: String): Boolean = base.trim().lowercase(Locale.ROOT).let { it.startsWith("translat") }

/** One author of the header line and their base roles (AniList's words, in order, without notes or repeats). */
data class StaffCredit(val name: String, val roles: List<String>)

/** The translators of one language ([languageCode] null = AniList does not say), in AniList order. */
data class TranslationCredit(val languageCode: String?, val names: List<String>)

data class StaffCredits(val authors: List<StaffCredit>, val translations: List<TranslationCredit>)

/**
 * Splits AniList's staff edges (one per person and role) into the authors (every role that is not translation, one entry per
 * person with all their roles) and the translators grouped by language (in order of first appearance; the group without a known
 * language last). A person who only translated is not an author.
 */
fun staffCredits(edges: List<StaffEdge>): StaffCredits {
    val authors = LinkedHashMap<String, MutableList<String>>()
    val translations = LinkedHashMap<String?, MutableList<String>>()
    edges.forEach { edge ->
        val name = edge.staff.name.trim().takeIf { it.isNotEmpty() } ?: return@forEach
        val parts = edge.role?.takeIf { it.isNotBlank() }?.let(::parseStaffRole)
        if (parts?.translation == true) {
            val names = translations.getOrPut(parts.languageCode) { mutableListOf() }
            if (name !in names) names += name
        } else {
            val roles = authors.getOrPut(name) { mutableListOf() }
            parts?.base?.takeIf { it.isNotEmpty() && it !in roles }?.let { roles += it }
        }
    }
    val groups = translations.entries.sortedBy { it.key == null }.map { TranslationCredit(it.key, it.value) }
    return StaffCredits(authors.map { StaffCredit(it.key, it.value) }, groups)
}

/** At most [shown] names, then how many more ("Kevin Frane, Paul Cuneo, Alyssa Orton-Niioka" + 3). */
fun shortNameList(names: List<String>, shown: Int = 3): Pair<String, Int> =
    names.take(shown).joinToString(", ") to (names.size - shown).coerceAtLeast(0)

/** ISO 639 code of an English language name as AniList writes it ("English", "Thai", "Spanish"); null when unknown. */
fun languageCodeOf(englishName: String): String? {
    val key = englishName.trim().lowercase(Locale.ROOT).takeIf { it.isNotEmpty() } ?: return null
    return LANGUAGE_CODES[key]
}

private val LANGUAGE_CODES: Map<String, String> by lazy {
    Locale.getISOLanguages().associateBy { code -> Locale.forLanguageTag(code).getDisplayLanguage(Locale.ENGLISH).lowercase(Locale.ROOT) }
        .filterKeys { it.isNotEmpty() } + mapOf("brazilian portuguese" to "pt", "portuguese (brazilian)" to "pt", "filipino" to "fil", "tagalog" to "tl")
}

/** The language's name in [locale] ("en" -> "inglés" in Spanish, "English" in English). */
fun languageName(code: String, locale: Locale): String = Locale.forLanguageTag(code).getDisplayLanguage(locale).let { name ->
    if (locale.language == "es") name.lowercase(locale) else name.replaceFirstChar { it.titlecase(locale) }
}
