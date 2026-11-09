//! Watch-history sync over Tatakai Link (wave A, worker **f**; plan 14 section 0.1 #9): the `LinkHost` hooks that
//! `live/link.rs::AppLinkHost` delegates here, backing `GET` / `POST` / `DELETE /v1/history` of `tk-link`
//! (`tk_link::history`; wire shape = the Android client's `LinkHistoryClient.kt`). The PC is the server; the phone
//! keeps a `since` cursor (`next_since_unix` of its last page, unix seconds) and pushes its own rows. All three bodies are
//! implemented (the store already had the merge rule); f keeps them and may add a "last sync" counter for the
//! Settings > Link row.

use std::sync::Arc;

use tk_core::{HistoryEntry, LocalStore};

/// Rows with `watched_at > since_unix`, oldest first, at most `limit` (`tk-link` asks `limit + 1` to detect
/// truncation). Store errors are logged and answered as an empty page (the phone retries on its next pass).
pub fn since(store: &Arc<LocalStore>, since_unix: i64, limit: usize) -> Vec<HistoryEntry> {
    match store.history_since_unix(since_unix, u32::try_from(limit).unwrap_or(u32::MAX)) {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!(error = %e, "history page for Link failed");
            Vec::new()
        }
    }
}

/// Merge rows pushed by a phone (`LocalStore::merge_remote`: newest `watched_at` wins, tie -> higher progress).
/// Returns the rows accepted: `merge_remote` applies the rule per row and tells nothing back, so this counts the
/// rows that win against the current store before merging. Rows without an AniList / MAL id never get here
/// (`tk_link::history::HistoryRow::into_entry` drops them).
pub fn merge(store: &Arc<LocalStore>, rows: Vec<HistoryEntry>) -> usize {
    let mut wins = 0;
    for r in &rows {
        let current = store.progress_for(&r.anime_id).unwrap_or_default().into_iter().find(|e| e.episode_id == r.episode_id);
        if current.is_none_or(|c| r.wins_over(&c)) {
            wins += 1;
        }
    }
    if let Err(e) = store.merge_remote(&rows) {
        tracing::warn!(error = %e, "history merge from Link failed");
        return 0;
    }
    wins
}

/// `DELETE /v1/history`: drop the PC history. `false` on a store error (the route answers `internal`).
pub fn clear(store: &Arc<LocalStore>) -> bool {
    match store.clear_history() {
        Ok(_) => true,
        Err(e) => {
            tracing::warn!(error = %e, "history clear from Link failed");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::OffsetDateTime;
    use tk_core::MediaRef;

    fn row(ep: u32, progress: u32, at: i64) -> HistoryEntry {
        HistoryEntry::for_playback(&MediaRef::anilist(21), "One Piece", None, ep, progress, Some(1440), OffsetDateTime::from_unix_timestamp(at).unwrap()).unwrap()
    }

    #[test]
    fn pushes_merge_by_newest_pages_follow_the_cursor_and_clear_empties() {
        let store = Arc::new(LocalStore::open_in_memory().unwrap());
        store.record_history(&row(1, 100, 10)).unwrap();
        // older row for ep 1 loses, newer row for ep 2 is new: one row counts
        assert_eq!(merge(&store, vec![row(1, 50, 5), row(2, 300, 20)]), 1);
        let page = since(&store, 0, 10);
        assert_eq!(page.iter().map(|e| (e.episode, e.progress_sec)).collect::<Vec<_>>(), [(2, 300), (1, 100)]);
        assert_eq!(since(&store, 10_000, 10).len(), 1);
        assert!(since(&store, 20_000, 10).is_empty());
        assert!(clear(&store));
        assert!(since(&store, 0, 10).is_empty());
    }
}
