// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.release

import app.tatakai.next.model.EpisodeRange
import app.tatakai.next.model.Quality
import app.tatakai.next.model.ReleaseSource
import app.tatakai.next.model.ReleaseTags
import app.tatakai.next.model.VideoCodec
import java.util.Locale

// 1:1 port of the body of apps/desktop-native/crates/tk-core/src/release.rs (parse_release, pick_episode_file and helpers).
// Rust works on byte offsets of UTF-8; this port works on UTF-16 indices of the same string, which slice the same text because
// every offset is taken from the string itself. Comments keep the Rust step numbers.
//
// How a name is read (every step is best effort, nothing fails):
// 1. the directory part and a known extension are dropped; a leading `[Group]` is the group (unless it only holds tags such as
//    `[1080p]`), else a trailing scene group (`...x264-GROUP`);
// 2. `_` becomes a space, and a name without spaces but with dots (`Show.S01E05.1080p`) is split on the dots;
// 3. the name is cut into alphanumeric words, each one knowing whether it sits inside brackets;
// 4. season / episode, in order: `S01E05`, `S01` / `Season 2` / `2nd Season`, `Title - 05` (also `- 01-12`, `- 01 ~ 12`), a bare
//    range `01-12`, `EP05` / `Episode 5`, the final bare number before the tags, and last a bracket holding only the number;
// 5. the title is the text before the first episode / season marker, bracket or tag word;
// 6. tags come from the words.

/** Groups that only publish streaming-service rips, so a name of theirs without a source tag is `WEB`. */
private val WEB_GROUPS = setOf("subsplease", "horriblesubs", "erai-raws", "tsundere-raws", "varyg", "toonshub", "asw", "yameii")

/** ISO 639-2 subtitle codes release names list in brackets; more than two = multi-sub. */
private val SUB_LANGS = setOf(
    "eng", "spa", "por", "ara", "fre", "fra", "ger", "deu", "ita", "rus", "chi", "zho", "chs", "cht", "kor", "pol", "tur", "ind",
    "vie", "tha", "msa", "may", "hin", "ukr", "cze", "ces", "hun", "rum", "ron", "dut", "nld", "swe", "fin", "nor", "dan", "heb",
    "gre", "ell",
)

private val SEASON_WORDS = setOf("season", "seasons", "saison", "temporada")
private val GROUP_TAIL_STOP = setOf("dl", "rip", "raws", "audio", "sub", "subs", "bit", "bits", "ray", "hen")

internal fun parseReleaseImpl(name: String): ReleaseTags {
    val base = name.substring(maxOf(name.lastIndexOf('/'), name.lastIndexOf('\\')) + 1)
    val stem = stripExt(base).trim()
    var group: String? = null

    // 1. leading [Group] (a bracket that only holds tags is moved to the end instead)
    var text = stem
    val lead = leadingBracket(stem)
    if (lead != null) {
        val (inner, rest) = lead
        if (looksLikeTags(inner)) {
            text = "${rest.trim()} [$inner]"
        } else {
            if (inner.isNotEmpty()) group = inner
            text = rest.trim()
        }
    }

    // 2. separators
    text = text.replace('_', ' ').replace('–', '-').replace('—', '-')
    val scene = !text.trim().contains(' ') && text.count { it == '.' } >= 2
    if (scene) text = text.replace('.', ' ')
    if (group == null) {
        val i = text.lastIndexOf('-')
        if (i > 0 && !text.substring(0, i).endsWith(' ')) {
            val tail = text.substring(i + 1).trim()
            val lower = tail.lowercase(Locale.ROOT)
            if (tail.isNotEmpty() &&
                tail.length <= 24 &&
                tail.all { it.isAsciiAlnum() } &&
                !tail.all { it in '0'..'9' } &&
                !isTagWord(lower) &&
                lower !in GROUP_TAIL_STOP
            ) {
                group = tail
                text = text.substring(0, i)
            }
        }
    }

    // 3. words
    val sp = spans(text)
    var tagZone = text.indexOfAny(charArrayOf('[', '(', '{', '【')).let { if (it < 0) text.length else it }
    sp.drop(1).firstOrNull { !it.bracketed && isTagWord(it.word) }?.let { tagZone = minOf(tagZone, it.start) }
    var titleEnd = tagZone

    // 4. season / episode
    var season: Int? = null
    var episodes: Pair<Int, Int>? = null
    var multiSeason = false
    for (i in sp.indices) {
        val s = sp[i]
        val se = seasonEpisode(s.word)
        if (se != null) {
            val (sea, ep) = se
            if (season == null) season = sea
            if (episodes == null) {
                var last = ep
                val n = sp.getOrNull(i + 1)
                if (n != null && rangeGap(text.substring(s.end, n.start))) {
                    val b = epWord(n.word) ?: epNumber(n.word)
                    if (b != null && b > ep) last = b
                }
                episodes = ep to last
            }
            titleEnd = minOf(titleEnd, s.start)
            continue
        }
        val sw = seasonWord(s.word)
        if (sw != null) {
            if (season == null) season = sw
            val n = sp.getOrNull(i + 1)
            if (n != null) {
                val gap = text.substring(s.end, n.start)
                if (episodes == null && gap.isBlank()) {
                    val ep = epWord(n.word)
                    if (ep != null) episodes = ep to ep
                }
                if (rangeGap(gap) && seasonWord(n.word) != null) multiSeason = true
            }
            if (!s.bracketed) titleEnd = minOf(titleEnd, s.start)
            continue
        }
        if (s.word in SEASON_WORDS) {
            var start = s.start
            val n = sp.getOrNull(i + 1)
            if (n != null && text.substring(s.end, n.start).isBlank()) {
                val d = digits(n.word)
                if (d != null) {
                    if (season == null) season = d
                    val m = sp.getOrNull(i + 2)
                    if (m != null && rangeGap(text.substring(n.end, m.start)) && digits(m.word) != null) multiSeason = true
                }
            }
            val p = if (i > 0) sp[i - 1] else null
            val ord = p?.let { ordinal(it.word) }
            if (p != null && ord != null) {
                if (season == null) season = ord
                start = p.start
            }
            if (!s.bracketed) titleEnd = minOf(titleEnd, start)
        }
    }
    // `Title - 05`, `Title - 01-12`, `Title - 01 ~ 12`
    if (episodes == null) {
        for (i in 1 until sp.size) {
            val s = sp[i]
            if (s.bracketed) continue
            val a = epNumber(s.word) ?: continue
            val gap = text.substring(sp[i - 1].end, s.start).trimEnd()
            if (!(gap.length >= 2 && gap.endsWith('-') && gap.substring(0, gap.length - 1).let { it.endsWith(' ') || it.endsWith(')') || it.endsWith(']') })) {
                continue
            }
            var b = a
            val n = sp.getOrNull(i + 1)
            if (n != null && rangeGap(text.substring(s.end, n.start))) {
                val x = epNumber(n.word)
                if (x != null && x > a) b = x
            }
            episodes = a to b
            titleEnd = minOf(titleEnd, sp[i - 1].end)
            break
        }
    }
    // bare range anywhere: `01-12`, `(01~24)`
    if (episodes == null) {
        for (i in 0 until maxOf(sp.size - 1, 0)) {
            val s = sp[i]
            val n = sp[i + 1]
            if (i == 0 && !s.bracketed) continue
            val a = epNumber(s.word) ?: continue
            val b = epNumber(n.word) ?: continue
            if (!rangeGap(text.substring(s.end, n.start)) || a >= b || (s.word.length == 4 && n.word.length == 4)) continue
            if (i > 0 && sp[i - 1].word in RANGE_PREFIX_STOP) continue
            episodes = a to b
            if (!s.bracketed) titleEnd = minOf(titleEnd, s.start)
            break
        }
    }
    // `EP05`, `E05`, `Episode 5`
    if (episodes == null) {
        for (i in sp.indices) {
            val s = sp[i]
            val found: Pair<Int, Int>? = if (s.word in EPISODE_WORDS) {
                sp.getOrNull(i + 1)?.let { n -> epNumber(n.word) }?.let { it to (i + 1) }
            } else {
                epWord(s.word)?.let { it to i }
            }
            if (found == null) continue
            val (a, last) = found
            var b = a
            val n = sp.getOrNull(last + 1)
            if (n != null && rangeGap(text.substring(sp[last].end, n.start))) {
                val x = epWord(n.word) ?: epNumber(n.word)
                if (x != null && x > a) b = x
            }
            episodes = a to b
            if (!s.bracketed) titleEnd = minOf(titleEnd, s.start)
            break
        }
    }
    // last bare number before the tags: `Show 05 [1080p]`
    if (episodes == null) {
        var found: Pair<Span, Int>? = null
        for (i in 1 until sp.size) {
            val s = sp[i]
            if (s.bracketed || s.start >= tagZone) continue
            val a = epNumber(s.word) ?: continue
            val year = s.word.length == 4 && a in 1900..2100
            val prev = sp[i - 1].word
            val next = sp.getOrNull(i + 1)?.word ?: ""
            if (!year && prev !in BARE_NUMBER_PREV_STOP && next != "season" && next != "seasons") found = s to a
        }
        if (found != null) {
            episodes = found.second to found.second
            titleEnd = minOf(titleEnd, found.first.start)
        }
    }
    // a bracket holding only the number: `Show_[05]_[Group]_[1080p]` (AniLibria and similar)
    if (episodes == null) {
        val bn = bracketedNumber(text)
        if (bn != null) {
            episodes = bn.second to bn.second
            titleEnd = minOf(titleEnd, bn.first)
        }
    }

    // 5. title
    val title = text.substring(0, minOf(titleEnd, text.length))
        .trim()
        .trimEnd { it == '-' || it == '~' || it == ':' || it == ',' || it == '.' || it.isWhitespace() }
        .trim()

    // 6. tags
    val resolution = sp.asSequence()
        .map { s ->
            val q = resolutionOf(s.word)
            if (q == Quality.UNKNOWN && s.bracketed && s.word in BARE_HEIGHTS) Quality.parse(s.word) else q
        }
        .firstOrNull { it != Quality.UNKNOWN } ?: Quality.UNKNOWN
    var codec: VideoCodec? = null
    for (i in sp.indices) {
        val s = sp[i]
        val next = sp.getOrNull(i + 1)?.word
        codec = codecWord(s.word) ?: when {
            (s.word == "h" || s.word == "x") && next == "264" -> VideoCodec.H264
            (s.word == "h" || s.word == "x") && next == "265" -> VideoCodec.HEVC
            else -> null
        }
        if (codec != null) break
    }
    var bitDepth: Int? = null
    for (i in sp.indices) {
        val next = sp.getOrNull(i + 1)?.word ?: ""
        bitDepth = when (val w = sp[i].word) {
            "hi10p", "hi10", "10bit", "10bits", "10bpp", "main10", "yuv420p10", "yuv420p10le" -> 10
            "8bit", "8bits" -> 8
            "12bit", "12bits" -> 12
            "10", "8", "12" -> if (next == "bit" || next == "bits") w.toInt() else null
            else -> null
        }
        if (bitDepth != null) break
    }
    val dualAudio = sp.indices.any { i ->
        val w = sp[i].word
        val next = sp.getOrNull(i + 1)?.word ?: ""
        w == "dualaudio" || w == "multiaudio" || ((w == "dual" || w == "multi") && (next == "audio" || next == "audios"))
    }
    val langs = LinkedHashSet<String>()
    for (s in sp) if (s.bracketed && s.word in SUB_LANGS) langs.add(s.word)
    val multiSub = langs.size > 2 || sp.indices.any { i ->
        val w = sp[i].word
        val next = sp.getOrNull(i + 1)?.word ?: ""
        w in MULTISUB_WORDS || ((w == "multi" || w == "multiple") && next in SUB_NEXT_WORDS)
    }
    var source = sourceOf(sp, scene)
    if (source == null && group != null && group.lowercase(Locale.ROOT) in WEB_GROUPS) source = ReleaseSource.WEB
    val batchWord = sp.any { it.word in BATCH_WORDS }
    val eps = episodes
    val batch = batchWord || multiSeason || (eps != null && eps.second > eps.first) || (season != null && eps == null)

    return ReleaseTags(
        group = group,
        title = title.ifEmpty { null },
        season = season,
        episodes = eps?.let { EpisodeRange(it.first, it.second) },
        source = source,
        resolution = resolution,
        codec = codec,
        bitDepth = bitDepth,
        dualAudio = dualAudio,
        multiSub = multiSub,
        batch = batch,
        seadexBest = false,
    )
}

private val RANGE_PREFIX_STOP = setOf("season", "seasons", "saison", "temporada", "part", "vol", "volume")
private val EPISODE_WORDS = setOf("ep", "eps", "episode", "episodes", "episodio", "capitulo")
private val BARE_NUMBER_PREV_STOP = setOf("no", "season", "seasons", "saison", "temporada", "part", "vol", "volume", "cour", "movie")
private val BARE_HEIGHTS = setOf("480", "720", "1080", "1440", "2160")
private val MULTISUB_WORDS = setOf("multisub", "multisubs", "multisubtitle", "multisubtitles")
private val SUB_NEXT_WORDS = setOf("sub", "subs", "subtitle", "subtitles", "subbed")
private val BATCH_WORDS = setOf("batch", "complete", "bdbox", "boxset")

// ---------------------------------------------------------------------------------------------
// pick_episode_file
// ---------------------------------------------------------------------------------------------

internal fun pickEpisodeFileImpl(files: List<CandidateFile>, episode: Int, titles: List<String>): Int? {
    val titleNorms = titles.map(::normTitle).filter { it.isNotEmpty() }
    val wantsExtras = titles.any { t -> words(t).any { it in EXTRA_REQUEST_WORDS } }
    val allVideos = files.filter { isVideoPath(it.path) }
    val videos: List<Pair<CandidateFile, ReleaseTags>> = allVideos
        .filter { !isExtra(it.path, wantsExtras, titleNorms) }
        .map { it to fileTags(it.path) }
    if (videos.size == 1) {
        val (only, tags) = videos[0]
        val r = tags.episodes
        if (r == null || (r.first <= episode && episode <= r.last)) return only.index
    }
    if (videos.isEmpty() && allVideos.size == 1) return allVideos[0].index
    val season = titles.firstNotNullOfOrNull(::titleSeason)

    fun best(strict: Boolean): Int? {
        // (score, size, index); max by score, then size, then the SMALLEST index (Rust: then(y.2.cmp(&x.2)))
        var top: Triple<Int, Long, Int>? = null
        for ((f, t) in videos) {
            val r = t.episodes ?: continue
            if (episode < r.first || episode > r.last) continue
            var score = if (r.first == r.last) 100 else 50
            val have = t.season
            if (season != null && have != null) {
                if (season == have) score += 20 else if (strict) continue
            } else if (season == null && have != null && have > 1) {
                score -= 10
            }
            if (titleMatches(t.title, titleNorms)) score += 10
            val cand = Triple(score, f.sizeBytes, f.index)
            val cur = top
            // Rust max_by keeps the LAST maximum: replace on >= (only equal when every key is equal)
            if (cur == null || compareCandidates(cand, cur) >= 0) top = cand
        }
        return top?.third
    }
    (best(true) ?: best(false))?.let { return it }

    // absolute numbering inside a later-season pack: `Show - 13.mkv` .. `Show - 24.mkv` for episodes 1..12
    val singles: List<Pair<CandidateFile, Int>> = videos
        .filter { (_, t) -> season == null || t.season == null || t.season == season }
        .mapNotNull { (f, t) -> t.episodes?.takeIf { it.first == it.last }?.let { f to it.first } }
    if (singles.size >= 2) {
        val first = singles.minOf { it.second }
        if (first > 1) {
            val target = episode + (first - 1)
            var pick: CandidateFile? = null
            for ((f, a) in singles) {
                if (a != target) continue
                val cur = pick
                // Rust max_by_key keeps the LAST maximum
                if (cur == null || f.sizeBytes >= cur.sizeBytes) pick = f
            }
            return pick?.index
        }
    }
    return null
}

private fun compareCandidates(x: Triple<Int, Long, Int>, y: Triple<Int, Long, Int>): Int {
    val a = x.first.compareTo(y.first)
    if (a != 0) return a
    val b = x.second.compareTo(y.second)
    if (b != 0) return b
    return y.third.compareTo(x.third)
}

private val EXTRA_REQUEST_WORDS = setOf("ova", "oad", "special", "specials", "sp", "omake")

// ---------------------------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------------------------

/** One alphanumeric run of a (normalized) name: index range, lowercase text, and whether it sits inside brackets. */
internal data class Span(val start: Int, val end: Int, val word: String, val bracketed: Boolean)

/** Rust `char::is_alphanumeric`: Alphabetic or Numeric (Nd, Nl, No). */
private fun isAlphanumeric(cp: Int): Boolean {
    if (Character.isAlphabetic(cp)) return true
    return when (Character.getType(cp)) {
        Character.DECIMAL_DIGIT_NUMBER.toInt(), Character.LETTER_NUMBER.toInt(), Character.OTHER_NUMBER.toInt() -> true
        else -> false
    }
}

internal fun spans(s: String): List<Span> {
    val out = ArrayList<Span>()
    var depth = 0
    var curStart = -1
    var curBracketed = false
    var i = 0
    while (i < s.length) {
        val cp = s.codePointAt(i)
        val len = Character.charCount(cp)
        if (isAlphanumeric(cp)) {
            if (curStart < 0) {
                curStart = i
                curBracketed = depth > 0
            }
            i += len
            continue
        }
        if (curStart >= 0) {
            out.add(Span(curStart, i, s.substring(curStart, i).lowercase(Locale.ROOT), curBracketed))
            curStart = -1
        }
        when (cp) {
            '['.code, '('.code, '{'.code, 0x3010 -> depth += 1
            ']'.code, ')'.code, '}'.code, 0x3011 -> depth = maxOf(depth - 1, 0)
        }
        i += len
    }
    if (curStart >= 0) out.add(Span(curStart, s.length, s.substring(curStart).lowercase(Locale.ROOT), curBracketed))
    return out
}

/** Lowercase alphanumeric words. */
private fun words(s: String): List<String> = spans(s).map { it.word }

/** Lowercase words joined by one space (`"Re:Zero - 2nd Season"` -> `"re zero 2nd season"`). */
private fun normTitle(s: String): String = words(s).joinToString(" ")

private fun titleMatches(title: String?, titleNorms: List<String>): Boolean {
    if (title == null) return false
    val n = normTitle(title)
    return n.isNotEmpty() && titleNorms.any { it.contains(n) || n.contains(it) }
}

/** The season a series title names (`Season 3`, `2nd Season`, `S2`, a trailing `II` / `III` / `IV`). */
private fun titleSeason(title: String): Int? {
    parseReleaseImpl(title).season?.let { return it }
    return when (words(title).lastOrNull()) {
        "ii" -> 2
        "iii" -> 3
        "iv" -> 4
        "vi" -> 6
        else -> null
    }
}

private fun pathSegments(path: String): List<String> = path.split('/', '\\').filter { it.isNotEmpty() }

/** Parsed tags of a torrent file; the season falls back to the nearest parent folder (`Season 2/`, `Show S02/`). */
private fun fileTags(path: String): ReleaseTags {
    val segs = pathSegments(path)
    val file = segs.lastOrNull() ?: path
    val tags = parseReleaseImpl(file)
    if (tags.season != null) return tags
    val folderSeason = segs.dropLast(1).asReversed().firstNotNullOfOrNull { parseReleaseImpl(it).season }
    return tags.copy(season = folderSeason)
}

/** Sample / NCOP / NCED / PV / CM / menu / extras file or folder (OVA / SP / specials too unless asked for). */
private fun isExtra(path: String, wantsExtras: Boolean, titleNorms: List<String>): Boolean {
    val segs = pathSegments(path)
    if (segs.isEmpty()) return false
    val file = segs.last()
    val folders = segs.dropLast(1)
    fun inTitles(w: String) = titleNorms.any { t -> t.split(' ').any { it == w } }
    fun hit(w: String) = w.startsWith("sample") || (isExtraWord(w, wantsExtras) && !inTitles(w))
    if (words(stripExt(file)).any(::hit)) return true
    // long folder names are torrent names ("Show + Extras [BD]"), not extras folders
    return folders.any { f ->
        val ws = words(f)
        ws.size <= 3 && ws.any(::hit)
    }
}

private fun isExtraWord(w: String, wantsExtras: Boolean): Boolean = when (w.trimEnd { it in '0'..'9' }) {
    "sample", "samples", "ncop", "nced", "nc", "creditless", "pv", "cm", "cms", "preview", "previews", "trailer",
    "trailers", "teaser", "menu", "menus", "extra", "extras", "bonus", "featurette", "featurettes", "interview",
    -> true
    "sp", "sps", "ova", "ovas", "oad", "special", "specials", "omake" -> !wantsExtras
    else -> false
}

/** Drops a known media / torrent extension (`.mkv`, `.torrent` ...); anything else stays. */
private fun stripExt(name: String): String {
    val dot = name.lastIndexOf('.')
    if (dot > 0) {
        val ext = name.substring(dot + 1).lowercase(Locale.ROOT)
        if (isVideoPath(name) || ext in SIDE_EXTENSIONS) return name.substring(0, dot)
    }
    return name
}

private val SIDE_EXTENSIONS = setOf("torrent", "mka", "ass", "srt", "ssa", "m4v")

/** `[inner]` (or `【inner】`, `(inner)`) at the start of [s] and the rest after it. */
private fun leadingBracket(s: String): Pair<String, String>? {
    val t = s.trimStart()
    if (t.isEmpty()) return null
    val close = when (t[0]) {
        '[' -> ']'
        '(' -> ')'
        '【' -> '】'
        else -> return null
    }
    val end = t.indexOf(close, startIndex = 1)
    if (end < 0) return null
    return t.substring(1, end).trim() to t.substring(end + 1)
}

/** `[1080p]`, `[BD 1080p HEVC]`: a bracket made only of tag words is not a group. */
private fun looksLikeTags(inner: String): Boolean {
    val ws = words(inner)
    return ws.isNotEmpty() && ws.all(::isTagWord)
}

private val TAG_WORDS = setOf(
    "remux", "bdremux", "bd", "bdrip", "bluray", "bdmv", "brrip", "web", "webrip", "webdl", "hdtv", "tvrip", "dvd", "dvdrip", "amzn",
    "hi10p", "hi10", "10bit", "10bits", "8bit", "8bits", "12bit", "aac", "flac", "opus", "ac3", "eac3", "ddp", "dts", "truehd",
    "batch", "multisub", "multisubs", "dualaudio",
)

/** A word that belongs to the tag part of a name (resolution, codec, source, audio, bit depth, batch). */
private fun isTagWord(w: String): Boolean = resolutionOf(w) != Quality.UNKNOWN || codecWord(w) != null || w in TAG_WORDS

private fun String.allAsciiDigits(): Boolean = all { it in '0'..'9' }

private fun Char.isAsciiAlnum(): Boolean = this in '0'..'9' || this in 'a'..'z' || this in 'A'..'Z'

/** `1080p`, `720i`, `1920x1080`, `BD1080p`, `4K`, `UHD`, `FHD`. */
private fun resolutionOf(word: String): Quality {
    val w = word.removePrefix("bd").takeIf { word.startsWith("bd") && it.endsWith('p') } ?: word
    val d = when {
        w.endsWith('p') -> w.dropLast(1)
        w.endsWith('i') -> w.dropLast(1)
        else -> null
    }
    if (d != null && d.length in 3..4 && d.allAsciiDigits()) {
        return when (d) {
            "576", "540", "360" -> Quality.P480
            else -> Quality.parse(d)
        }
    }
    val x = w.indexOf('x')
    if (x >= 0) {
        val a = w.substring(0, x)
        val b = w.substring(x + 1)
        if (a.length in 3..4 && b.length in 3..4 && a.allAsciiDigits() && b.allAsciiDigits()) return Quality.parse(b)
    }
    return when (w) {
        "4k", "uhd" -> Quality.P2160
        "fhd", "fullhd" -> Quality.P1080
        else -> Quality.UNKNOWN
    }
}

private fun codecWord(w: String): VideoCodec? = when (w) {
    "av1" -> VideoCodec.AV1
    "x265", "h265", "hevc" -> VideoCodec.HEVC
    "vp9" -> VideoCodec.VP9
    "x264", "h264", "avc" -> VideoCodec.H264
    else -> null
}

/**
 * `REMUX` wins over any other source word; otherwise the first one. Streaming-service codes that are also common words
 * (`NF`, `CR`, `ADN` ...) only count inside brackets or in scene names.
 */
private fun sourceOf(sp: List<Span>, scene: Boolean): ReleaseSource? {
    var found: ReleaseSource? = null
    for (i in sp.indices) {
        val s = sp[i]
        val next = sp.getOrNull(i + 1)?.word ?: ""
        val tagged = s.bracketed || scene
        val w = s.word
        val src = when {
            w == "remux" || w == "bdremux" -> ReleaseSource.REMUX
            w == "dvdremux" -> ReleaseSource.DVD
            w in setOf("bd", "bdrip", "bluray", "bdmv", "brrip", "bdr") -> ReleaseSource.BD
            w == "blu" && next == "ray" -> ReleaseSource.BD
            w in setOf("web", "webrip", "webdl", "amzn", "crunchyroll", "funimation", "hidive") -> ReleaseSource.WEB
            tagged && w in setOf("nf", "cr", "dsnp", "adn", "baha", "bilibili", "abema") -> ReleaseSource.WEB
            w in setOf("hdtv", "tvrip", "hdtvrip", "dtv") -> ReleaseSource.TV
            w in setOf("dvd", "dvdrip", "dvd5", "dvd9", "r2dvd", "r1dvd", "dvdr") -> ReleaseSource.DVD
            w.startsWith("bd") && resolutionOf(w.substring(2)) != Quality.UNKNOWN -> ReleaseSource.BD
            else -> null
        }
        if (src == ReleaseSource.REMUX) return ReleaseSource.REMUX
        if (src != null && found == null) found = src
    }
    return found
}

/** `-`, `~` (with any spaces) between the two ends of a range. */
private fun rangeGap(gap: String): Boolean = when (gap.trim()) {
    "-", "~", "〜", "～" -> true
    else -> false
}

/** 1 to 4 ASCII digits. */
private fun digits(w: String): Int? {
    if (w.isEmpty() || w.length > 4 || !w.allAsciiDigits()) return null
    return w.toInt()
}

/** An episode number, tolerating a version suffix (`05`, `05v2`). */
private fun epNumber(w: String): Int? {
    val i = w.lastIndexOf('v')
    val base = if (i > 0 && i + 1 < w.length && w.substring(i + 1).allAsciiDigits()) w.substring(0, i) else w
    return digits(base)
}

/** First `[05]` / `(05v2)` whose brackets hold only an episode number (not a year): its offset and the number. */
private fun bracketedNumber(text: String): Pair<Int, Int>? {
    for (start in text.indices) {
        val open = text[start]
        if (open != '[' && open != '(') continue
        val close = if (open == '[') ']' else ')'
        val innerStart = start + 1
        val end = text.indexOf(close, startIndex = innerStart)
        if (end < 0) continue
        val inner = text.substring(innerStart, end).trim()
        if (inner.isEmpty() || inner.toByteArray(Charsets.UTF_8).size > 7 || inner[0] !in '0'..'9') continue
        val n = epNumber(inner.lowercase(Locale.ROOT)) ?: continue
        val digitsLen = inner.takeWhile { it in '0'..'9' }.length
        if (digitsLen == 4 && n in 1900..2100) continue
        return start to n
    }
    return null
}

/** `ep05`, `e05`, `e5v2`. */
private fun epWord(w: String): Int? = when {
    w.startsWith("ep") -> epNumber(w.substring(2))
    w.startsWith("e") -> epNumber(w.substring(1))
    else -> null
}

/** `s01e05` / `s1e5v2` -> `(season, episode)`. */
private fun seasonEpisode(w: String): Pair<Int, Int>? {
    if (!w.startsWith("s")) return null
    val rest = w.substring(1)
    val e = rest.indexOf('e')
    if (e <= 0 || e > 2) return null
    val season = digits(rest.substring(0, e)) ?: return null
    val ep = epNumber(rest.substring(e + 1)) ?: return null
    return season to ep
}

/** `s01`, `s2`. */
private fun seasonWord(w: String): Int? {
    if (!w.startsWith("s")) return null
    val d = w.substring(1)
    return if (d.length <= 2) digits(d) else null
}

/** `2nd`, `3rd`, `4th`, `1st`. */
private fun ordinal(w: String): Int? {
    val suf = listOf("st", "nd", "rd", "th").firstOrNull { w.endsWith(it) } ?: return null
    return digits(w.removeSuffix(suf))
}
