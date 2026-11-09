// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.settings

import android.content.Context
import coil3.SingletonImageLoader
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

// parity wave A (worker q, checklist system.clear-cache): "Limpiar caché" also empties the image cache on disk (desktop clears the cache
// table and its disk caches). The app has no custom ImageLoader, so every cover / banner goes through Coil's singleton loader, whose disk
// cache lives under `cacheDir/image_cache`; this goes through Coil's own API instead of deleting files behind its journal.

/** Image cache seen by Settings › Datos: size on disk and a full clear. */
interface ImageCache {
    /** Bytes the image disk cache uses now; null when there is no disk cache. */
    suspend fun sizeBytes(): Long?

    /** Empties the disk cache (and the in-memory one, so a cleared image is really fetched again). */
    suspend fun clear()
}

/** [ImageCache] over Coil's singleton [coil3.ImageLoader] (the one `AsyncImage` uses everywhere in the app). */
class CoilImageCache(private val context: Context) : ImageCache {
    private val loader get() = SingletonImageLoader.get(context.applicationContext)

    override suspend fun sizeBytes(): Long? = withContext(Dispatchers.IO) { loader.diskCache?.size }

    override suspend fun clear() = withContext(Dispatchers.IO) {
        val l = loader
        l.memoryCache?.clear()
        l.diskCache?.clear()
        Unit
    }
}

/** Bytes freed by a clear, from the measured totals before and after (never negative: a cover may be written meanwhile). */
internal fun freedBytes(before: Long, after: Long): Long = (before - after).coerceAtLeast(0L)
