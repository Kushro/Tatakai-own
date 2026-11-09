// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.model

import kotlinx.coroutines.flow.StateFlow
import kotlinx.serialization.Serializable

// Parity wave B contract (docs/redesign/16-parity-wave-b-contract-android.md §3.4, plan §7 D4): native Stremio addons. Desktop
// counterpart: tk-ext (worker j). Signatures FINAL; bodies: worker t (`StremioAddonRepositoryImpl` + `StremioCandidateSource :
// ExternalCandidateSource` in :core:extensions-quickjs, bound in `extensionsModule`). Addon streams enter `resolveCandidates` with
// `extId = "stremio:<addonId>"` and `extName = addon.name` (no `|` in ids: it separates `sourceKey`).

/** One installed Stremio addon (its `manifest.json`). [id] = manifest `id`; [manifestUrl] is where it was installed from. */
@Serializable
data class StremioAddon(
    val id: String,
    val name: String,
    val version: String,
    val description: String = "",
    val manifestUrl: String,
    val logo: String? = null,
    val enabled: Boolean = true,
    /** Manifest `types` (`movie`, `series`, `anime`...). Only addons with a `stream` resource are useful here. */
    val types: List<String> = emptyList(),
    val hasStreams: Boolean = true,
    val installedAtMs: Long,
    val updatedAtMs: Long? = null,
)

interface StremioAddonRepository {
    val addons: StateFlow<List<StremioAddon>>

    /** Fetches and validates the manifest (`https://…/manifest.json` or `stremio://…`), stores the addon. */
    suspend fun install(manifestUrl: String): Result<StremioAddon>

    suspend fun setEnabled(id: String, enabled: Boolean)

    suspend fun remove(id: String)

    /** Re-fetches the manifest; returns the refreshed addon (version may change). */
    suspend fun update(id: String): Result<StremioAddon>

    /** Forgets the cached title -> IMDb / Kitsu id matches of this addon (classic "reset title matches"). */
    suspend fun resetMatches(id: String)
}
