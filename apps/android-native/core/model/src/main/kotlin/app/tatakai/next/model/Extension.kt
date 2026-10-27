package app.tatakai.next.model

enum class ExtensionCompat { FULL, PARTIAL, UNSUPPORTED }

data class InstalledExtension(
    val id: String,
    val name: String,
    val version: String,
    val type: String,
    val permissions: List<String>,
    val compat: ExtensionCompat,
    val enabled: Boolean,
)
