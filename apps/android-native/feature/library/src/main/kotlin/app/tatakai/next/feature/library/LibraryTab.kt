// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.library

// android wave contract (docs/redesign/12 §3.14). Owner: worker c; parity wave A (15 §3.3): worker q. Name and values FIXED (:app passes it).

/**
 * Tabs of Library: Continuar · Historial · Listas · Favoritos · Torrents. LISTS = AniList / MAL list by status (segmented Viendo /
 * Planeado / Completado / En pausa / Abandonado / Repitiendo); FAVORITES = `ListsRepository.favorites()`. The Torrents tab body is
 * TorrentsLibraryTab of worker f; badge = active downloads. Order = display order (`ordinal` is the saved tab index).
 */
enum class LibraryTab { CONTINUE, HISTORY, LISTS, FAVORITES, TORRENTS }
