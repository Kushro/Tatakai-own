// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.model

import kotlinx.serialization.KSerializer
import kotlinx.serialization.Serializable
import kotlinx.serialization.descriptors.PrimitiveKind
import kotlinx.serialization.descriptors.PrimitiveSerialDescriptor
import kotlinx.serialization.descriptors.SerialDescriptor
import kotlinx.serialization.encoding.Decoder
import kotlinx.serialization.encoding.Encoder

// 2026-11 UI refresh: Settings › Apariencia › Diseño y estilo. Same option ids, presets and values as the desktop
// `tk-ui/src/model/look.rs` (`LookPrefs`), so both apps describe the same choices. Persisted by SettingsCodec as one JSON key
// (`look_json`); an unknown id reads as the option's default instead of dropping the whole object.

/** A persisted choice: [id] is the shared desktop / Android id (`spotlight`, `detailed`...). */
interface LookChoice {
    val id: String
}

/** How Home opens. */
@Serializable(with = HomeLayout.Serializer::class)
enum class HomeLayout(override val id: String) : LookChoice {
    /** Full-width banner of the trending titles (facts, synopsis, genres, Watch / list buttons, timed progress) with the rows below. */
    SPOTLIGHT("spotlight"),

    /** The multi-browse carousel: one wide slide and narrow previews (the previous Android Home). */
    CAROUSEL("carousel"),

    /** No hero: a greeting and the rows, denser. */
    ROWS("rows"),
    ;

    internal object Serializer : LookChoiceSerializer<HomeLayout>("HomeLayout", entries, SPOTLIGHT)
}

/** Poster cards in rows and grids. */
@Serializable(with = CardStyle.Serializer::class)
enum class CardStyle(override val id: String) : LookChoice {
    /** Poster, title and the `2024 · 12 ep` line. */
    CLASSIC("classic"),

    /** Poster, a status dot before the title, year and format under it. */
    DETAILED("detailed"),

    /** Poster only; the title is read by TalkBack and shown on long press. */
    MINIMAL("minimal"),
    ;

    internal object Serializer : LookChoiceSerializer<CardStyle>("CardStyle", entries, DETAILED)
}

/**
 * 0.3.2: size of the cards in Home rows (desktop `CardSize`). [SMALL] / [MEDIUM] are poster cards (MEDIUM = the previous width);
 * [LARGE] / [XL] are horizontal "detailed" cards: poster on the left, title, status / format / year / episodes, score, genres and, on
 * episode rows, "Episodio N · hace X" (phones: about 1.3 large cards or 1 XL card per screen width).
 */
@Serializable(with = CardSize.Serializer::class)
enum class CardSize(override val id: String) : LookChoice {
    SMALL("small"),
    MEDIUM("medium"),
    LARGE("large"),
    XL("xl"),
    ;

    /** Horizontal detailed card (poster + facts beside it). */
    val isWide: Boolean get() = this == LARGE || this == XL

    internal object Serializer : LookChoiceSerializer<CardSize>("CardSize", entries, MEDIUM)
}

/** Navigation rail (tablets / wide windows only: phones keep the bottom bar). */
@Serializable(with = RailStyle.Serializer::class)
enum class RailStyle(override val id: String) : LookChoice {
    /** Labels under the icons. */
    LABELED("labeled"),

    /** Icons only (labels for TalkBack). */
    SLIM("slim"),
    ;

    internal object Serializer : LookChoiceSerializer<RailStyle>("RailStyle", entries, LABELED)
}

/** Spacing and card sizes. */
@Serializable(with = Density.Serializer::class)
enum class Density(override val id: String) : LookChoice {
    COMFORTABLE("comfortable"),
    COMPACT("compact"),
    ;

    internal object Serializer : LookChoiceSerializer<Density>("Density", entries, COMFORTABLE)
}

/** Press / enter animations. [OFF] behaves like Settings › Reducir movimiento. */
@Serializable(with = Motion.Serializer::class)
enum class Motion(override val id: String) : LookChoice {
    /** Press scale, shape morphs, slides. */
    EXPRESSIVE("expressive"),

    /** Color and opacity only. */
    SUBTLE("subtle"),
    OFF("off"),
    ;

    internal object Serializer : LookChoiceSerializer<Motion>("Motion", entries, EXPRESSIVE)
}

/** Episode list of Details. */
@Serializable(with = EpisodeView.Serializer::class)
enum class EpisodeView(override val id: String) : LookChoice {
    /** Large cards: thumbnail with length and rating, title, synopsis, relative date. */
    CARDS("cards"),

    /** One row per episode with a small thumbnail. */
    LIST("list"),

    /** Numbers and titles only. */
    COMPACT("compact"),
    ;

    internal object Serializer : LookChoiceSerializer<EpisodeView>("EpisodeView", entries, CARDS)
}

/** Related titles of Details. */
@Serializable(with = RelationsView.Serializer::class)
enum class RelationsView(override val id: String) : LookChoice {
    /** Pan / zoom graph of the franchise. */
    GRAPH("graph"),

    /** Grouped list with covers. */
    LIST("list"),
    ;

    internal object Serializer : LookChoiceSerializer<RelationsView>("RelationsView", entries, GRAPH)
}

/** Default view of the airing calendar. */
@Serializable(with = CalendarView.Serializer::class)
enum class CalendarView(override val id: String) : LookChoice {
    /** Seven columns, one per day (wide windows; phones show the week as a day strip + that day). */
    WEEK("week"),

    /** One row per day. */
    AGENDA("agenda"),

    /** Month grid with posters per day. */
    MONTH("month"),

    /** Today as a time line with a "now" marker. */
    DAY("day"),
    ;

    internal object Serializer : LookChoiceSerializer<CalendarView>("CalendarView", entries, WEEK)
}

/** A named bundle of every [LookPrefs] field. */
@Serializable(with = LookPreset.Serializer::class)
enum class LookPreset(override val id: String) : LookChoice {
    /** The default: expressive Material 3 with the spotlight hero. */
    TATAKAI("tatakai"),

    /** Cinema: slim rail, spotlight hero, minimal posters. */
    CINEMA("cinema"),

    /** Dense: rows first, compact cards and lists. */
    DENSE("dense"),

    /** Calm: no autoplay, subtle motion, classic cards. */
    CALM("calm"),
    ;

    internal object Serializer : LookChoiceSerializer<LookPreset>("LookPreset", entries, TATAKAI)
}

/** The enum whose [LookChoice.id] is [id], or null. */
inline fun <reified E> lookChoiceOf(id: String?): E? where E : Enum<E>, E : LookChoice =
    enumValues<E>().firstOrNull { it.id == id }

@Serializable
data class LookPrefs(
    /** Last preset applied (shown as selected while the fields still match it). */
    val preset: LookPreset = LookPreset.TATAKAI,
    val homeLayout: HomeLayout = HomeLayout.SPOTLIGHT,
    val cardStyle: CardStyle = CardStyle.DETAILED,
    /** 0.3.2: card size of Home rows (desktop `card_size`). */
    val cardSize: CardSize = CardSize.MEDIUM,
    val railStyle: RailStyle = RailStyle.LABELED,
    val density: Density = Density.COMFORTABLE,
    val motion: Motion = Motion.EXPRESSIVE,
    /** Blur / glass effects (desktop). Not offered on Android (a real-time blur is too heavy for low-end phones); kept so the presets match. */
    val glass: Boolean = false,
    val episodeView: EpisodeView = EpisodeView.CARDS,
    val relationsView: RelationsView = RelationsView.GRAPH,
    val calendarView: CalendarView = CalendarView.WEEK,
    /** The Home hero rotates by itself. */
    val heroAutoplay: Boolean = true,
    /** Seconds per hero slide, [HERO_INTERVAL_MIN]..[HERO_INTERVAL_MAX]. */
    val heroIntervalSecs: Int = 9,
    /** The greeting ("Buenas noches") above Home. */
    val showGreeting: Boolean = false,
) {
    /** The preset whose fields all equal these ([preset] itself ignored); null = custom. */
    fun matchingPreset(): LookPreset? = LookPreset.entries.firstOrNull { of(it).copy(preset = preset) == this }

    fun sanitized(): LookPrefs = copy(heroIntervalSecs = heroIntervalSecs.coerceIn(HERO_INTERVAL_MIN, HERO_INTERVAL_MAX))

    /** Hero autoplay period in milliseconds, null when autoplay is off. */
    fun heroAutoplayMs(): Long? = if (heroAutoplay) heroIntervalSecs.coerceIn(HERO_INTERVAL_MIN, HERO_INTERVAL_MAX) * 1000L else null

    companion object {
        const val HERO_INTERVAL_MIN = 4
        const val HERO_INTERVAL_MAX = 30

        /** Every field of a preset (values of the desktop `LookPrefs::preset`). */
        fun of(p: LookPreset): LookPrefs {
            val base = LookPrefs(preset = p)
            return when (p) {
                LookPreset.TATAKAI -> base
                LookPreset.CINEMA -> base.copy(
                    cardStyle = CardStyle.MINIMAL,
                    cardSize = CardSize.LARGE,
                    railStyle = RailStyle.SLIM,
                    glass = true,
                    heroIntervalSecs = 12,
                )
                LookPreset.DENSE -> base.copy(
                    homeLayout = HomeLayout.ROWS,
                    cardStyle = CardStyle.CLASSIC,
                    cardSize = CardSize.SMALL,
                    railStyle = RailStyle.SLIM,
                    density = Density.COMPACT,
                    motion = Motion.SUBTLE,
                    episodeView = EpisodeView.LIST,
                    relationsView = RelationsView.LIST,
                    calendarView = CalendarView.AGENDA,
                    showGreeting = true,
                )
                LookPreset.CALM -> base.copy(
                    homeLayout = HomeLayout.CAROUSEL,
                    cardStyle = CardStyle.CLASSIC,
                    motion = Motion.SUBTLE,
                    heroAutoplay = false,
                    episodeView = EpisodeView.LIST,
                    calendarView = CalendarView.DAY,
                    showGreeting = true,
                )
            }
        }
    }
}

/** Lenient id serializer: writes [LookChoice.id], reads an unknown id as [default]. */
internal open class LookChoiceSerializer<E>(
    name: String,
    private val values: List<E>,
    private val default: E,
) : KSerializer<E> where E : Enum<E>, E : LookChoice {
    override val descriptor: SerialDescriptor = PrimitiveSerialDescriptor("app.tatakai.next.model.$name", PrimitiveKind.STRING)

    override fun serialize(encoder: Encoder, value: E) = encoder.encodeString(value.id)

    override fun deserialize(decoder: Decoder): E {
        val raw = decoder.decodeString()
        return values.firstOrNull { it.id == raw } ?: default
    }
}
