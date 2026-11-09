//! Watch-history sync over Tatakai Link (wave A, worker **f**; plan 14 section 0.1 #9): the `LinkHost` hooks that
//! `live/link.rs::AppLinkHost` delegates here, backing `GET` / `POST` / `DELETE /v1/history` of `tk-link`
//! (`tk_link::history`; wire shape = the Android client's `LinkHistoryClient.kt`). The PC is the server; the phone
//! keeps a `since` cursor (`next_since_unix` of its last page, unix seconds) and pushes its own rows.
//!
//! Every request a phone makes here also stamps the "last sync" time ([`last_sync_unix`]): kept in memory and in
//! the store's `settings` table (key [`LAST_SYNC_KEY`]) so Settings > Link / Data can say when a paired phone last
//! synced, also after a restart. `LinkHost` does not say which device asked, so the UI pairs the time with the
//! most recently seen device of `LinkStatusVm`.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use time::OffsetDateTime;
use tk_core::{HistoryEntry, LocalStore};

/// Store `settings` key of the last history request from a phone (unix seconds).
pub const LAST_SYNC_KEY: &str = "link.history.last_sync_unix";
/// 0 = not known in this process yet (read from the store).
static LAST_SYNC: AtomicI64 = AtomicI64::new(0);

fn touch(store: &LocalStore) {
    let now = OffsetDateTime::now_utc().unix_timestamp();
    LAST_SYNC.store(now, Ordering::Relaxed);
    if let Err(e) = store.set_setting(LAST_SYNC_KEY, &now) {
        tracing::debug!(error = %e, "could not persist the last history sync time");
    }
}

/// When a paired phone last used `/v1/history` (unix seconds), `None` = never.
pub fn last_sync_unix(store: &LocalStore) -> Option<i64> {
    let mem = LAST_SYNC.load(Ordering::Relaxed);
    if mem > 0 {
        return Some(mem);
    }
    let stored = store.get_setting::<i64>(LAST_SYNC_KEY).ok().flatten().filter(|t| *t > 0)?;
    LAST_SYNC.store(stored, Ordering::Relaxed);
    Some(stored)
}

/// Forget the in-memory stamp ("Reset app": the store row goes with the database).
pub fn forget_last_sync() {
    LAST_SYNC.store(0, Ordering::Relaxed);
}

/// Rows with `watched_at > since_unix`, oldest first, at most `limit` (`tk-link` asks `limit + 1` to detect
/// truncation). Store errors are logged and answered as an empty page (the phone retries on its next pass).
pub fn since(store: &Arc<LocalStore>, since_unix: i64, limit: usize) -> Vec<HistoryEntry> {
    touch(store);
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
    touch(store);
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
    touch(store);
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
        // pages are oldest first and the cursor is unix seconds, exclusive (rows were watched at 10 s and 20 s)
        let page = since(&store, 0, 10);
        assert_eq!(page.iter().map(|e| (e.episode, e.progress_sec)).collect::<Vec<_>>(), [(1, 100), (2, 300)]);
        assert_eq!(since(&store, 10, 10).iter().map(|e| e.episode).collect::<Vec<_>>(), [2]);
        assert!(since(&store, 20, 10).is_empty());
        assert!(clear(&store));
        assert!(since(&store, 0, 10).is_empty());
    }

    #[test]
    fn a_completed_episode_never_goes_back_to_in_progress_through_an_older_row() {
        let store = Arc::new(LocalStore::open_in_memory().unwrap());
        let mut done = row(5, 1400, 100);
        done.completed = true;
        store.record_history(&done).unwrap();
        // the phone pushes an older, unfinished row of the same episode: it loses
        assert_eq!(merge(&store, vec![row(5, 300, 50)]), 0);
        let got = since(&store, 0, 10);
        assert_eq!(got.len(), 1);
        assert!(got[0].completed && got[0].progress_sec == 1400);
        // a newer row wins (the user re-watched it on the phone): the exact position travels
        assert_eq!(merge(&store, vec![row(5, 383, 200)]), 1);
        let got = since(&store, 0, 10);
        assert_eq!((got[0].progress_sec, got[0].completed), (383, false));
        // same instant: more progress wins, nothing is lost
        assert_eq!(merge(&store, vec![row(5, 380, 200)]), 0);
        assert_eq!(since(&store, 0, 10)[0].progress_sec, 383);
    }

    #[test]
    fn every_request_stamps_the_last_sync_time_and_it_survives_in_the_store() {
        let store = Arc::new(LocalStore::open_in_memory().unwrap());
        forget_last_sync();
        let before = OffsetDateTime::now_utc().unix_timestamp();
        let _ = since(&store, 0, 1);
        let t = last_sync_unix(&store).unwrap();
        assert!(t >= before);
        // a restart (empty memory) reads it back from the store (other tests may stamp a later time meanwhile)
        let stored = store.get_setting::<i64>(LAST_SYNC_KEY).unwrap().unwrap();
        assert!(stored >= before);
        forget_last_sync();
        assert!(last_sync_unix(&store).is_some_and(|x| x >= stored));
    }
}
