// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import app.tatakai.next.model.ExtensionPrefType
import app.tatakai.next.model.ExtensionPreference
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.doubleOrNull

enum class ExtensionType(val wire: String) {
    TORRENT("torrent"),
    ONLINESTREAM("onlinestream"),
    CUSTOM("custom"),
    ;

    companion object {
        fun fromWire(value: String): ExtensionType? = entries.firstOrNull { it.wire == value }
    }
}

data class ApiServer(
    val namespace: String?,
    val contract: String?,
    val routes: List<String>,
    val healthPath: String?,
    /** True when the manifest listed `apiServer.routes` itself ([routes] is then not the default list); `candidateRoutes` needs it. */
    val routesExplicit: Boolean = false,
)

data class CustomSourceDescriptor(
    val id: String,
    val kind: String,
    val name: String,
    val icon: String? = null,
    val description: String? = null,
)

/** Parsed `manifest.json` of a `.kai` package (contracts/extension-host/routes.v1.json, `package.manifest`). */
data class ExtensionManifest(
    val id: String,
    val name: String,
    val version: String,
    val type: ExtensionType,
    val description: String,
    val permissions: List<String>,
    val apiServer: ApiServer?,
    val capabilities: List<String>,
    val customSources: List<CustomSourceDescriptor>,
    val priority: Int,
    /** android wave (12 §3.7): raw manifest `browserDomains`; parsing is worker e's (ManifestParser). Filter with [BrowserPolicy.browserDomains]. */
    val browserDomains: List<String> = emptyList(),
    /** parity wave B (16 §3.4, worker t, checklist ext.preferences): manifest `preferences[]` (alias `settings[]`); empty = no form. */
    val preferences: List<ExtensionPreference> = emptyList(),
) {
    /** Namespace of the legacy mount: `apiServer.namespace`, falling back to the id. */
    val namespace: String get() = apiServer?.namespace?.takeIf { it.isNotBlank() } ?: id

    /** Routes the manifest enables for `sources`/`stream`/`torrent` (default per routes.v1.json). */
    val enabledAggregateRoutes: List<String> get() = apiServer?.routes ?: DEFAULT_ROUTES

    val networkPolicy: NetworkPolicy get() = NetworkPolicy.fromPermissions(permissions)

    companion object {
        val DEFAULT_ROUTES = listOf("sources", "stream", "torrent")
        val ID_PATTERN = Regex("^[a-z0-9][a-z0-9._-]{0,63}$")
    }
}

/** Install-time validation failure. [field] names the failing manifest field (globals.v1.md, section 4). */
class ManifestException(val field: String?, message: String) : Exception(message)

object ManifestParser {
    private val json = Json

    /** @throws ManifestException with a message naming the failing field. */
    fun parse(text: String): ExtensionManifest {
        val root = try {
            json.parseToJsonElement(text)
        } catch (e: Exception) {
            throw ManifestException(null, "manifest.json is not valid JSON")
        }
        val obj = (root as? JsonObject) ?: throw ManifestException(null, "manifest.json must be a JSON object")

        val id = requiredString(obj, "id")
        if (!ExtensionManifest.ID_PATTERN.matches(id)) {
            throw ManifestException("id", "Field 'id' must match ${ExtensionManifest.ID_PATTERN.pattern}")
        }
        val name = requiredString(obj, "name")
        val version = requiredString(obj, "version")
        val typeText = requiredString(obj, "type")
        val type = ExtensionType.fromWire(typeText)
            ?: throw ManifestException("type", "Field 'type' must be one of torrent, onlinestream, custom")
        val description = requiredString(obj, "description", allowEmpty = true)
        val permissionsEl = obj["permissions"] ?: throw ManifestException("permissions", "Missing required field 'permissions'")
        val permissions = stringList(permissionsEl, "permissions")
        // Unknown permission kinds are tolerated (forward compatibility); they grant nothing.

        val capabilities = obj["capabilities"]?.let { stringList(it, "capabilities") } ?: emptyList()
        val customSources = obj["customSources"]?.let { parseCustomSources(it) } ?: emptyList()
        if ("custom-source" in capabilities && customSources.isEmpty()) {
            throw ManifestException("customSources", "Field 'customSources' must be non-empty when capabilities has 'custom-source'")
        }
        val priority = (obj["priority"] as? JsonPrimitive)?.doubleOrNull?.toInt() ?: 100

        return ExtensionManifest(
            id = id,
            name = name,
            version = version,
            type = type,
            description = description,
            permissions = permissions,
            apiServer = obj["apiServer"]?.let { parseApiServer(it) },
            capabilities = capabilities,
            customSources = customSources,
            priority = priority,
            browserDomains = obj["browserDomains"]?.takeUnless { it is JsonNull }?.let { stringList(it, "browserDomains") } ?: emptyList(),
            preferences = PreferenceParser.parse(obj["preferences"] ?: obj["settings"]),
        )
    }

    private fun requiredString(obj: JsonObject, field: String, allowEmpty: Boolean = false): String {
        val el = obj[field] ?: throw ManifestException(field, "Missing required field '$field'")
        val value = (el as? JsonPrimitive)?.takeIf { it.isString }?.contentOrNull
            ?: throw ManifestException(field, "Field '$field' must be a string")
        if (!allowEmpty && value.isBlank()) throw ManifestException(field, "Field '$field' must not be empty")
        return value
    }

    private fun stringList(el: JsonElement, field: String): List<String> {
        val arr = el as? JsonArray ?: throw ManifestException(field, "Field '$field' must be an array of strings")
        return arr.map { item ->
            (item as? JsonPrimitive)?.takeIf { it.isString }?.content
                ?: throw ManifestException(field, "Field '$field' must be an array of strings")
        }
    }

    private fun parseApiServer(el: JsonElement): ApiServer {
        val obj = el as? JsonObject ?: throw ManifestException("apiServer", "Field 'apiServer' must be an object")
        val routes = obj["routes"]?.takeUnless { it is JsonNull }?.let { stringList(it, "apiServer.routes") } ?: ExtensionManifest.DEFAULT_ROUTES
        return ApiServer(
            namespace = (obj["namespace"] as? JsonPrimitive)?.contentOrNull,
            contract = (obj["contract"] as? JsonPrimitive)?.contentOrNull,
            routes = routes,
            healthPath = (obj["healthPath"] as? JsonPrimitive)?.contentOrNull,
            routesExplicit = obj["routes"] != null && obj["routes"] !is JsonNull,
        )
    }

    private fun parseCustomSources(el: JsonElement): List<CustomSourceDescriptor> {
        val arr = el as? JsonArray ?: throw ManifestException("customSources", "Field 'customSources' must be an array")
        return arr.map { item ->
            val o = item as? JsonObject ?: throw ManifestException("customSources", "customSources[] must be objects")
            fun str(k: String) = (o[k] as? JsonPrimitive)?.contentOrNull
            val kind = str("kind")
            if (kind != "read" && kind != "watch" && kind != "novel") {
                throw ManifestException("customSources", "customSources[].kind must be 'read', 'watch' or 'novel'")
            }
            CustomSourceDescriptor(
                id = str("id") ?: throw ManifestException("customSources", "customSources[].id is required"),
                kind = kind,
                name = str("name") ?: throw ManifestException("customSources", "customSources[].name is required"),
                icon = str("icon"),
                description = str("description"),
            )
        }
    }
}

/**
 * parity wave B (16 §3.4, worker t, checklist ext.preferences): tolerant parser of manifest `preferences[]` / `settings[]`. Each entry
 * is `{ key | name | id, title | label, type: text|string|secret|password|toggle|boolean|bool|select|enum, description?, options?, default? }`;
 * `options` may be strings or `{ value, label }`. Invalid entries are skipped (never an install error: preferences are optional).
 */
object PreferenceParser {
    private val KEY = Regex("^[A-Za-z0-9_.-]{1,64}$")
    const val MAX_PREFERENCES = 32

    fun parse(el: JsonElement?): List<ExtensionPreference> {
        val arr = when (el) {
            is JsonArray -> el
            is JsonObject -> el["fields"] as? JsonArray ?: return emptyList()
            else -> return emptyList()
        }
        val out = ArrayList<ExtensionPreference>()
        for (item in arr) {
            val o = item as? JsonObject ?: continue
            fun str(k: String) = (o[k] as? JsonPrimitive)?.takeUnless { it is JsonNull }?.contentOrNull?.trim()?.takeIf { it.isNotEmpty() }
            val key = (str("key") ?: str("name") ?: str("id"))?.takeIf { KEY.matches(it) } ?: continue
            if (out.any { it.key == key }) continue
            val options = (o["options"] as? JsonArray).orEmpty().mapNotNull { opt ->
                when (opt) {
                    is JsonPrimitive -> opt.contentOrNull?.takeIf { it.isNotBlank() }
                    is JsonObject -> (opt["value"] as? JsonPrimitive)?.contentOrNull?.takeIf { it.isNotBlank() }
                    else -> null
                }
            }.distinct()
            val type = when (str("type")?.lowercase()) {
                "secret", "password", "apikey", "api_key", "token" -> ExtensionPrefType.SECRET
                "toggle", "boolean", "bool", "switch", "checkbox" -> ExtensionPrefType.TOGGLE
                "select", "enum", "choice", "list" -> if (options.isEmpty()) ExtensionPrefType.TEXT else ExtensionPrefType.SELECT
                else -> if (options.isNotEmpty()) ExtensionPrefType.SELECT else ExtensionPrefType.TEXT
            }
            val default = when (val d = o["default"]) {
                is JsonPrimitive -> d.takeUnless { it is JsonNull }?.contentOrNull
                else -> null
            }
            out += ExtensionPreference(
                key = key,
                title = str("title") ?: str("label") ?: key,
                type = type,
                description = str("description") ?: str("hint"),
                options = options,
                default = if (type == ExtensionPrefType.SECRET) null else default,
            )
            if (out.size >= MAX_PREFERENCES) break
        }
        return out
    }
}
