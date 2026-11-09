// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.manga

import kotlinx.serialization.Serializable

// Reader keyboard shortcuts (checklist manga.reader.keybinds; classic src/lib/reader/keybindings.ts). `ReaderSettings.keymap` stores only
// the overrides (`action id -> key name`). Key names are Android `KeyEvent.keyCodeToString` without the `KEYCODE_` prefix (`DPAD_RIGHT`,
// `N`, `EQUALS`), the same convention as the player keymap, so one key press maps with one call.

object ReaderKeymap {
    const val NEXT_PAGE = "next_page"
    const val PREV_PAGE = "prev_page"
    const val SCROLL_DOWN = "scroll_down"
    const val SCROLL_UP = "scroll_up"
    const val NEXT_CHAPTER = "next_chapter"
    const val PREV_CHAPTER = "prev_chapter"
    const val TOGGLE_FULLSCREEN = "toggle_fullscreen"
    const val TOGGLE_MODE = "toggle_mode"
    const val ZOOM_IN = "zoom_in"
    const val ZOOM_OUT = "zoom_out"
    const val TOGGLE_CHROME = "toggle_chrome"
    const val RELOAD = "reload"

    val ACTIONS: List<String> = listOf(
        NEXT_PAGE, PREV_PAGE, SCROLL_DOWN, SCROLL_UP, NEXT_CHAPTER, PREV_CHAPTER, TOGGLE_FULLSCREEN, TOGGLE_MODE, ZOOM_IN, ZOOM_OUT,
        TOGGLE_CHROME, RELOAD,
    )

    val DEFAULTS: Map<String, String> = mapOf(
        NEXT_PAGE to "DPAD_RIGHT",
        PREV_PAGE to "DPAD_LEFT",
        SCROLL_DOWN to "DPAD_DOWN",
        SCROLL_UP to "DPAD_UP",
        NEXT_CHAPTER to "N",
        PREV_CHAPTER to "P",
        TOGGLE_FULLSCREEN to "F",
        TOGGLE_MODE to "D",
        ZOOM_IN to "EQUALS",
        ZOOM_OUT to "MINUS",
        TOGGLE_CHROME to "SPACE",
        RELOAD to "R",
    )

    /** Keys the reader always handles itself (leaving the reader). */
    val RESERVED: Set<String> = setOf("BACK", "ESCAPE", "HOME")

    /** Every action with its effective key. */
    fun resolve(overrides: Map<String, String>): Map<String, String> =
        ACTIONS.associateWith { action -> overrides[action]?.takeIf { it.isNotBlank() } ?: DEFAULTS.getValue(action) }

    /** The action bound to [key], if any (first in [ACTIONS] order on a conflict). */
    fun actionFor(overrides: Map<String, String>, key: String): String? {
        val resolved = resolve(overrides)
        return ACTIONS.firstOrNull { resolved[it] == key }
    }

    /** Keys used by more than one action. */
    fun conflicts(overrides: Map<String, String>): Set<String> =
        resolve(overrides).values.groupingBy { it }.eachCount().filterValues { it > 1 }.keys

    /** Other actions that already use [key]. */
    fun conflictsWith(overrides: Map<String, String>, action: String, key: String): List<String> =
        resolve(overrides).filter { (a, k) -> a != action && k == key }.keys.toList()

    /** Binds [key] to [action]; a key equal to the default drops the override. */
    fun bind(overrides: Map<String, String>, action: String, key: String): Map<String, String> =
        if (DEFAULTS[action] == key) overrides - action else overrides + (action to key)

    fun reset(): Map<String, String> = emptyMap()

    /** `KEYCODE_DPAD_RIGHT` -> `DPAD_RIGHT`. */
    fun keyName(keyCodeString: String): String = keyCodeString.removePrefix("KEYCODE_").uppercase()
}

/**
 * Private preferences of the readers that `AppSettings.reader` does not carry (classic MangaPage "idioma preferido" / "fuente preferida"),
 * stored in `filesDir/manga/prefs.json` (16 §3.9: a field the codec does not know lives in the owner module).
 */
@Serializable
data class MangaPrefs(
    /** Extra preferred chapter languages, best first (ISO 639-1); the app language and English are always appended. */
    val languages: List<String> = emptyList(),
    /** Preferred source key (`<ext>/<source>`) for every title; null = automatic. */
    val preferredSource: String? = null,
    /** Per title overrides: mediaId -> source key. */
    val preferredSourceByMedia: Map<String, String> = emptyMap(),
    /** The 18+ zone gate was accepted once (checklist manga.adult-gate). */
    val adultGateAccepted: Boolean = false,
    /** Titles whose mature warning was accepted (WARN mode asks once per title). */
    val matureAccepted: Set<Int> = emptySet(),
)
