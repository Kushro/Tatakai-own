// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

// parity wave A (docs/redesign/15 §3.2, checklist player.shortcuts.*). Owner: worker p. Pure, no Android.
//
// Hardware keyboard / remote shortcuts of the player, ported from the classic `src/lib/video/keybindings.ts`. A key is named like
// Android's `KeyEvent.keyCodeToString` without the `KEYCODE_` prefix (`SPACE`, `DPAD_LEFT`, `F`, `SLASH`), so the screen maps a key press
// with one call and the names survive in `AppSettings.playerKeymap` (`action id -> key name`, only the entries that differ from
// [PlayerKeymap.DEFAULTS]).

object PlayerKeymap {
    const val PLAY_PAUSE = "play_pause"
    const val SEEK_BACK = "seek_back"
    const val SEEK_FORWARD = "seek_forward"
    const val FULLSCREEN = "fullscreen"
    const val MUTE = "mute"
    const val PIP = "pip"
    const val PREV_EPISODE = "prev_episode"
    const val NEXT_EPISODE = "next_episode"
    const val VOLUME_UP = "volume_up"
    const val VOLUME_DOWN = "volume_down"
    const val SCREENSHOT = "screenshot"
    const val SUBS_TOGGLE = "subs_toggle"
    const val HELP = "help"

    /** Every action, in the order the editor and the `?` panel list them. */
    val ACTIONS: List<String> = listOf(
        PLAY_PAUSE, SEEK_BACK, SEEK_FORWARD, FULLSCREEN, MUTE, PIP, PREV_EPISODE, NEXT_EPISODE, VOLUME_UP, VOLUME_DOWN, SCREENSHOT,
        SUBS_TOGGLE, HELP,
    )

    /** Classic defaults: Space, arrows, F, M, P, J / K for the episodes, S screenshot, C subtitles, ? help. */
    val DEFAULTS: Map<String, String> = mapOf(
        PLAY_PAUSE to "SPACE",
        SEEK_BACK to "DPAD_LEFT",
        SEEK_FORWARD to "DPAD_RIGHT",
        FULLSCREEN to "F",
        MUTE to "M",
        PIP to "P",
        PREV_EPISODE to "J",
        NEXT_EPISODE to "K",
        VOLUME_UP to "DPAD_UP",
        VOLUME_DOWN to "DPAD_DOWN",
        SCREENSHOT to "S",
        SUBS_TOGGLE to "C",
        HELP to "SLASH",
    )

    /** Keys the editor refuses: they belong to the system or to text entry. */
    val RESERVED: Set<String> = setOf("BACK", "HOME", "APP_SWITCH", "POWER", "ESCAPE", "TAB", "ENTER", "DPAD_CENTER", "MENU", "UNKNOWN")

    /** The effective map: [custom] (from settings) over [DEFAULTS]; unknown actions and blank keys are ignored. */
    fun resolve(custom: Map<String, String>): Map<String, String> =
        ACTIONS.associateWith { action -> custom[action]?.trim()?.uppercase()?.takeIf { it.isNotEmpty() } ?: DEFAULTS.getValue(action) }

    /** The action bound to [key] in [custom], or null. When two actions share a key (a conflict), the first in [ACTIONS] wins. */
    fun actionFor(key: String, custom: Map<String, String>): String? {
        val k = key.uppercase()
        val map = resolve(custom)
        return ACTIONS.firstOrNull { map[it] == k }
    }

    /** key -> actions bound to it, only for keys bound more than once. */
    fun conflicts(custom: Map<String, String>): Map<String, List<String>> =
        resolve(custom).entries.groupBy({ it.value }, { it.key }).filterValues { it.size > 1 }

    /** The actions other than [action] that already use [key] (what the editor warns about before saving). */
    fun conflictsWith(custom: Map<String, String>, action: String, key: String): List<String> {
        val k = key.uppercase()
        return resolve(custom).filter { (a, bound) -> a != action && bound == k }.keys.toList()
    }

    /**
     * New custom map with [action] bound to [key]. Entries equal to the default are dropped, so an untouched keymap stays empty and a
     * future change of [DEFAULTS] reaches it. Unknown actions and [RESERVED] keys leave the map as it was.
     */
    fun bind(custom: Map<String, String>, action: String, key: String): Map<String, String> {
        val k = key.trim().uppercase()
        if (action !in ACTIONS || k.isEmpty() || k in RESERVED) return compact(custom)
        return compact(custom + (action to k))
    }

    /** "Restablecer": back to the defaults (an empty custom map). */
    fun reset(): Map<String, String> = emptyMap()

    /** Drops unknown actions and entries equal to their default. */
    fun compact(custom: Map<String, String>): Map<String, String> =
        custom.mapValues { it.value.trim().uppercase() }
            .filter { (action, key) -> action in ACTIONS && key.isNotEmpty() && DEFAULTS[action] != key }

    /** Short label of a key name: arrows as symbols, `SLASH` as `?`, `NUMPAD_5` as `Num 5`, letters as they are. `SPACE` is localised by the UI. */
    fun displayName(key: String): String = when (key) {
        "DPAD_LEFT" -> "←"
        "DPAD_RIGHT" -> "→"
        "DPAD_UP" -> "↑"
        "DPAD_DOWN" -> "↓"
        "SLASH" -> "?"
        "PERIOD" -> "."
        "COMMA" -> ","
        "MINUS" -> "-"
        "EQUALS", "PLUS" -> "+"
        "PAGE_UP" -> "PgUp"
        "PAGE_DOWN" -> "PgDn"
        "MOVE_HOME" -> "Home"
        "MOVE_END" -> "End"
        else -> when {
            key.startsWith("NUMPAD_") -> "Num " + key.removePrefix("NUMPAD_")
            key.startsWith("MEDIA_") -> key.removePrefix("MEDIA_").lowercase().replaceFirstChar { it.uppercase() }.replace('_', ' ')
            else -> key
        }
    }

    /** `KEYCODE_SPACE` -> `SPACE` (Android `KeyEvent.keyCodeToString`). */
    fun keyName(keyCodeString: String): String = keyCodeString.removePrefix("KEYCODE_").uppercase()
}
