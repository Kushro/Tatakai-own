package app.tatakai.next.metadata

import app.tatakai.next.model.Episode
import app.tatakai.next.model.FeedPage
import app.tatakai.next.model.Media
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import java.io.File

object Fixtures {
    private val json = Json { encodeDefaults = true; explicitNulls = true }

    private val dir: File by lazy {
        var d: File? = File(System.getProperty("user.dir")).absoluteFile
        while (d != null) {
            val c = File(d, "contracts/fixtures/metadata")
            if (c.isDirectory) return@lazy c
            d = d.parentFile
        }
        error("contracts/fixtures/metadata not found above ${System.getProperty("user.dir")}")
    }

    fun text(name: String): String = File(dir, name).readText(Charsets.UTF_8)

    fun element(name: String): JsonElement = Json.parseToJsonElement(text(name))

    fun expected(name: String): JsonObject = (element("expected.json") as JsonObject)[name] as JsonObject

    /**
     * Fields Android / desktop add beyond the shared contract (2026-11 UI refresh: card format, episode synopsis / length / rating).
     * expected.json is shared with the Rust tests and is not edited, so they are compared by leaving them out (same as
     * `tk-core/tests/metadata_fixtures.rs`); [MappersExtrasTest] checks their values.
     */
    private val APP_ONLY = setOf("format", "summary", "runtimeMin", "rating")

    private fun JsonElement.stripAppOnly(): JsonElement = when (this) {
        is JsonObject -> JsonObject(filterKeys { it !in APP_ONLY }.mapValues { it.value.stripAppOnly() })
        is JsonArray -> JsonArray(map { it.stripAppOnly() })
        else -> this
    }

    fun encode(m: Media): JsonElement = json.encodeToJsonElement(Media.serializer(), m).stripAppOnly()

    fun encode(e: Episode): JsonElement = json.encodeToJsonElement(Episode.serializer(), e).stripAppOnly()

    fun encode(p: FeedPage): JsonObject = JsonObject(
        mapOf(
            "items" to JsonArray(p.items.map(::encode)),
            "page" to JsonPrimitive(p.page),
            "hasNext" to JsonPrimitive(p.hasNext),
        ),
    )

    fun encodeEpisodes(list: List<Episode>): JsonElement = JsonArray(list.map(::encode))
}
