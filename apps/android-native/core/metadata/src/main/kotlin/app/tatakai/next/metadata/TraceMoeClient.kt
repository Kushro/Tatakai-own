// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.metadata

import app.tatakai.next.model.ImageMatch
import okhttp3.MediaType.Companion.toMediaTypeOrNull
import okhttp3.MultipartBody
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody

/**
 * trace.moe scene search (classic `/image-search`, docs/redesign/15 §5 o.1): `POST https://api.trace.moe/search?anilistInfo` with the
 * picture as multipart field `image`. Anonymous use (no key); trace.moe answers 402 / 429 when the quota or the queue is full, which
 * surfaces as a [ProviderException] with that status.
 */
class TraceMoeClient(
    private val client: OkHttpClient,
    private val endpoint: String = "https://api.trace.moe",
) {
    suspend fun search(image: ByteArray, mimeType: String): List<ImageMatch> {
        require(image.isNotEmpty()) { "empty image" }
        if (image.size > MAX_BYTES) throw ProviderException(ID, 413, "image larger than ${MAX_BYTES / (1024 * 1024)} MiB")
        val type = mimeType.toMediaTypeOrNull() ?: "image/jpeg".toMediaTypeOrNull()
        val body = MultipartBody.Builder()
            .setType(MultipartBody.FORM)
            .addFormDataPart("image", "image", image.toRequestBody(type))
            .build()
        val request = Request.Builder()
            .url("$endpoint/search?anilistInfo")
            .post(body)
            .header("Accept", "application/json")
            .build()
        val doc = client.fetchJson(ID, request)
        DiscoverMappers.traceMoeError(doc)?.let { throw ProviderException(ID, 502, it) }
        return DiscoverMappers.traceMoe(doc)
    }

    companion object {
        const val ID = "tracemoe"

        /** trace.moe rejects uploads over 25 MiB. */
        const val MAX_BYTES = 25 * 1024 * 1024
    }
}
