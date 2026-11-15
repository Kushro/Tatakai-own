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
pub fn merge(store: &Arc<LocalStore>, mut rows: Vec<HistoryEntry>) -> usize {
    touch(store);
    let mut wins = 0;
    for r in &mut rows {
        let known = store.progress_for(&r.anime_id).unwrap_or_default();
        // BUG-096: a row played in a Link session can come without a poster; keep the one the PC already has (this
        // episode's, else any of the series) so "Continue watching" does not lose its cover on either side
        if r.poster.as_deref().is_none_or(str::is_empty) {
            r.poster = known_poster(&known, &r.episode_id);
        }
        let current = known.into_iter().find(|e| e.episode_id == r.episode_id);
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

/// 2026-11 history per device: rows pushed by the Link device `device_id` remember it as their origin
/// (`LocalStore::merge_remote_from`); with `authoritative` ("Use this device as the source", window open) they win
/// regardless of timestamps. Same BUG-096 poster rule as [`merge`]. Returns the rows stored.
pub fn merge_from(store: &Arc<LocalStore>, device_id: &str, mut rows: Vec<HistoryEntry>, authoritative: bool) -> usize {
    touch(store);
    for r in &mut rows {
        if r.poster.as_deref().is_none_or(str::is_empty) {
            let known = store.progress_for(&r.anime_id).unwrap_or_default();
            r.poster = known_poster(&known, &r.episode_id);
        }
    }
    match store.merge_remote_from(&rows, device_id, authoritative) {
        Ok(n) => n,
        Err(e) => {
            tracing::warn!(error = %e, "history merge from Link failed");
            0
        }
    }
}

/// Rows per origin device ("contributed N episodes" on the Link screen). Errors read as none.
pub fn origin_counts(store: &LocalStore) -> std::collections::HashMap<String, u64> {
    store.history_origin_counts().unwrap_or_else(|e| {
        tracing::debug!(error = %e, "history origin counts failed");
        Default::default()
    })
}

/// BUG-096: the poster to keep for a row of `episode_id` that came without one: that episode's, else the newest of the
/// series (rows of one anime). Pure.
pub fn known_poster(rows: &[HistoryEntry], episode_id: &str) -> Option<String> {
    let usable = |e: &&HistoryEntry| e.poster.as_deref().is_some_and(|p| !p.is_empty());
    rows.iter().filter(usable).find(|e| e.episode_id == episode_id).or_else(|| rows.iter().filter(usable).max_by_key(|e| e.watched_at)).and_then(|e| e.poster.clone())
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
    fn a_pushed_row_without_a_poster_keeps_the_one_the_pc_has() {
        // BUG-096: the phone played ep 5 in a Link session and pushed it without a poster
        let store = Arc::new(LocalStore::open_in_memory().unwrap());
        let mut seen = row(4, 1400, 100);
        seen.poster = Some("https://img.example/og.jpg".into());
        store.record_history(&seen).unwrap();
        assert_eq!(merge(&store, vec![row(5, 300, 200)]), 1);
        let got = since(&store, 150, 10);
        assert_eq!(got[0].poster.as_deref(), Some("https://img.example/og.jpg"));
        // a poster that comes with the row wins
        let mut fresh = row(5, 400, 300);
        fresh.poster = Some("https://img.example/new.jpg".into());
        merge(&store, vec![fresh]);
        assert_eq!(since(&store, 250, 10)[0].poster.as_deref(), Some("https://img.example/new.jpg"));
    }

    #[test]
    fn pushes_from_a_device_remember_it_and_the_source_wins_inside_its_window() {
        let store = Arc::new(LocalStore::open_in_memory().unwrap());
        let mut seen = row(4, 1400, 100);
        seen.poster = Some("https://img.example/og.jpg".into());
        store.record_history(&seen).unwrap();
        // the QA phone pushes a newer row of ep 4 and a new ep 5
        assert_eq!(merge_from(&store, "moto", vec![row(4, 10, 300), row(5, 20, 300)], false), 2);
        assert_eq!(origin_counts(&store).get("moto"), Some(&2));
        assert_eq!(since(&store, 0, 10)[0].poster.as_deref(), Some("https://img.example/og.jpg"), "BUG-096 still applies");
        // the tablet is the source: its older rows win while the window is open
        assert_eq!(merge_from(&store, "tab", vec![row(4, 1400, 200)], false), 0);
        assert_eq!(merge_from(&store, "tab", vec![row(4, 1400, 200)], true), 1);
        let counts = origin_counts(&store);
        assert_eq!((counts.get("tab"), counts.get("moto")), (Some(&1), Some(&1)));
        assert_eq!(store.delete_history_by_origin("moto").unwrap(), 1);
        assert_eq!(since(&store, 0, 10).iter().map(|e| (e.episode, e.progress_sec)).collect::<Vec<_>>(), [(4, 1400)]);
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
