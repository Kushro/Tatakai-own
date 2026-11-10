//! AniList-first watch progress (2026-11-10): the list mirror (`list_entries`, service `local`, kept equal to AniList by
//! the sync) and the local playback history are two views of the same thing. Before this, "continue watching", the
//! details "Watch episode N" button and the episode marks read only the history, so a series watched elsewhere (on
//! AniList: 2/12) still offered episode 1. Pure functions; `tk-app` feeds them rows.
//!
//! Rules (shared with the Android app):
//! - list progress P / total T: episode `P + 1` is next while the entry is not finished (`P < T` or T unknown);
//! - continue watching = history rows ∪ started list rows (`watching` / `repeating`, P > 0), one per title, the further
//!   episode wins (the history keeps its resume position only when its episode is the one shown), newest first;
//! - an episode `n <= P` counts as watched;
//! - marking episode n watched raises P to n (same status rules as the automatic progress); unwatching it lowers P to
//!   `n - 1` and reopens a completed entry.

use std::collections::HashMap;

use super::sync::{EntryPatch, auto_progress};
use crate::model::HistoryEntry;
use crate::store::lists::ListEntryRow;

/// One "continue watching" item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContinueRow {
    pub anime_id: String,
    pub title: String,
    pub poster: Option<String>,
    pub episode: u32,
    /// Resume position (0 when the item comes from the list).
    pub progress_sec: u32,
    pub duration_sec: Option<u32>,
    /// Unix seconds of the last activity (history `watched_at` or list `updated_at`), for ordering.
    pub at: i64,
}

/// Next episode by list progress, or `None` when nothing was watched or the entry is finished.
pub fn list_next(progress: u32, total: Option<u32>) -> Option<u32> {
    if progress == 0 || total.is_some_and(|t| t > 0 && progress >= t) {
        return None;
    }
    Some(progress + 1)
}

/// Whether a list row belongs in "continue watching" (and which episode it offers).
fn list_continue_episode(row: &ListEntryRow) -> Option<u32> {
    // Started only: a "watching" entry at 0 (added but never begun) is not something to continue.
    if !matches!(row.status.as_str(), "watching" | "repeating") || row.progress == 0 {
        return None;
    }
    if row.episodes.is_some_and(|t| t > 0 && row.progress >= t) {
        return None;
    }
    Some(row.progress + 1)
}

/// History rows (any order, several per title) merged with the list rows into "continue watching".
pub fn continue_rows(history: Vec<HistoryEntry>, list: &[ListEntryRow], limit: usize) -> Vec<ContinueRow> {
    let mut by_id: HashMap<String, ContinueRow> = HashMap::new();
    for h in crate::model::continue_watching(history, usize::MAX) {
        by_id.insert(
            h.anime_id.clone(),
            ContinueRow {
                anime_id: h.anime_id,
                title: h.title,
                poster: h.poster,
                episode: h.episode,
                progress_sec: h.progress_sec,
                duration_sec: h.duration_sec,
                at: h.watched_at.unix_timestamp(),
            },
        );
    }
    for l in list {
        let Some(next) = list_continue_episode(l) else { continue };
        match by_id.get_mut(&l.media_id) {
            Some(row) => {
                if next > row.episode {
                    row.episode = next;
                    row.progress_sec = 0;
                    row.duration_sec = None;
                }
                row.at = row.at.max(l.updated_at);
            }
            None => {
                by_id.insert(
                    l.media_id.clone(),
                    ContinueRow {
                        anime_id: l.media_id.clone(),
                        title: l.title.clone(),
                        poster: l.poster.clone(),
                        episode: next,
                        progress_sec: 0,
                        duration_sec: None,
                        at: l.updated_at,
                    },
                );
            }
        }
    }
    let mut rows: Vec<ContinueRow> = by_id.into_values().collect();
    rows.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| a.title.cmp(&b.title)));
    rows.truncate(limit);
    rows
}

/// Details "resume": `(episode, seconds)` from the title's history rows (any order) and its list progress.
/// `(0, 0)` = never watched (the UI offers episode 1).
pub fn resume(history: &[HistoryEntry], list: Option<&ListEntryRow>) -> (u32, u32) {
    let latest = history.iter().max_by(|a, b| a.watched_at.cmp(&b.watched_at));
    let from_history = match latest {
        Some(e) if e.completed => (e.episode + 1, 0),
        Some(e) => (e.episode, e.progress_sec),
        None => (0, 0),
    };
    match list.and_then(|l| list_next(l.progress, l.episodes)) {
        Some(next) if next > from_history.0 => (next, 0),
        _ => from_history,
    }
}

/// Watched fraction of episode `n`: full when the list progress covers it, else the history fraction.
pub fn watched_fraction(episode: u32, history_fraction: f64, list_progress: u32) -> f64 {
    if episode <= list_progress { 1.0 } else { history_fraction }
}

/// Patch for a manual "watched / not watched" toggle of episode `n`, or `None` when nothing changes.
pub fn toggle(current: Option<(&str, u32)>, episode: u32, watched: bool, total: Option<u32>) -> Option<EntryPatch> {
    if episode == 0 {
        return None;
    }
    if watched {
        // Raising progress: same status rules as the automatic progress, but a completed entry may not move.
        return auto_progress(current, episode, total);
    }
    let (status, progress) = current?;
    if progress < episode {
        return None;
    }
    Some(EntryPatch {
        status: (status == "completed").then(|| "watching".to_owned()),
        progress: Some(episode - 1),
        ..EntryPatch::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::OffsetDateTime;

    fn hist(id: &str, ep: u32, secs: u32, completed: bool, at: i64) -> HistoryEntry {
        HistoryEntry {
            anime_id: id.into(),
            title: format!("T{id}"),
            poster: None,
            episode_id: format!("{id}?ep={ep}"),
            episode: ep,
            progress_sec: secs,
            duration_sec: Some(1440),
            completed,
            watched_at: OffsetDateTime::from_unix_timestamp(at).unwrap(),
            anilist_id: id.parse().ok(),
            mal_id: None,
        }
    }

    fn list(id: &str, status: &str, progress: u32, total: Option<u32>, at: i64) -> ListEntryRow {
        ListEntryRow {
            service: "local".into(),
            media_id: id.into(),
            anilist_id: id.parse().ok(),
            mal_id: None,
            title: format!("L{id}"),
            poster: None,
            status: status.into(),
            progress,
            episodes: total,
            score: None,
            started_at: None,
            finished_at: None,
            repeats: 0,
            notes: None,
            remote_id: None,
            updated_at: at,
            dirty: false,
        }
    }

    #[test]
    fn list_progress_beats_an_older_history_episode() {
        // Frieren: history has episode 1 half-watched, AniList says 27/28
        let rows = continue_rows(vec![hist("154587", 1, 600, false, 200)], &[list("154587", "watching", 27, Some(28), 100)], 10);
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].episode, rows[0].progress_sec, rows[0].duration_sec), (28, 0, None));
        assert_eq!(rows[0].at, 200, "the newer activity orders the item");
    }

    #[test]
    fn history_keeps_its_position_when_it_is_further() {
        let rows = continue_rows(vec![hist("1", 5, 300, false, 50)], &[list("1", "watching", 3, Some(12), 100)], 10);
        assert_eq!((rows[0].episode, rows[0].progress_sec), (5, 300));
        assert_eq!(rows[0].at, 100);
    }

    #[test]
    fn only_unfinished_watching_or_repeating_list_rows_join() {
        let l = [
            list("1", "watching", 2, Some(12), 10),
            list("2", "watching", 12, Some(12), 20),
            list("3", "planning", 0, Some(12), 30),
            list("4", "repeating", 3, None, 40),
            list("6", "watching", 0, Some(12), 60),
            list("5", "paused", 4, Some(12), 50),
        ];
        let rows = continue_rows(vec![], &l, 10);
        let got: Vec<(&str, u32)> = rows.iter().map(|r| (r.anime_id.as_str(), r.episode)).collect();
        assert_eq!(got, [("4", 4), ("1", 3)], "newest first; finished, not started, planning and paused are left out");
    }

    #[test]
    fn limit_applies_after_the_merge() {
        let l: Vec<_> = (1..=30).map(|i| list(&i.to_string(), "watching", 1, Some(12), i64::from(i))).collect();
        let rows = continue_rows(vec![hist("99", 1, 10, false, 1000)], &l, 5);
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0].anime_id, "99");
        assert_eq!(rows[1].anime_id, "30");
    }

    #[test]
    fn resume_uses_the_list_when_it_is_ahead() {
        let l = list("1", "watching", 2, Some(12), 0);
        assert_eq!(resume(&[], Some(&l)), (3, 0));
        assert_eq!(resume(&[hist("1", 1, 600, false, 5)], Some(&l)), (3, 0));
        assert_eq!(resume(&[hist("1", 4, 600, false, 5)], Some(&l)), (4, 600));
        assert_eq!(resume(&[hist("1", 4, 0, true, 5)], Some(&l)), (5, 0));
        // finished on the list: history decides (re-watch flow unchanged)
        assert_eq!(resume(&[], Some(&list("1", "completed", 12, Some(12), 0))), (0, 0));
        assert_eq!(resume(&[], None), (0, 0));
    }

    #[test]
    fn episodes_covered_by_the_list_are_watched() {
        assert_eq!(watched_fraction(2, 0.0, 2), 1.0);
        assert_eq!(watched_fraction(3, 0.4, 2), 0.4);
    }

    #[test]
    fn toggling_moves_progress_both_ways() {
        let p = toggle(Some(("watching", 2)), 5, true, Some(12)).unwrap();
        assert_eq!((p.progress, p.status), (Some(5), None));
        let p = toggle(Some(("watching", 11)), 12, true, Some(12)).unwrap();
        assert_eq!(p.status.as_deref(), Some("completed"));
        let p = toggle(None, 1, true, Some(12)).unwrap();
        assert_eq!((p.progress, p.status.as_deref()), (Some(1), Some("watching")));
        assert!(toggle(Some(("watching", 5)), 3, true, Some(12)).is_none(), "already covered");

        let p = toggle(Some(("watching", 5)), 3, false, Some(12)).unwrap();
        assert_eq!((p.progress, p.status), (Some(2), None));
        let p = toggle(Some(("completed", 12)), 12, false, Some(12)).unwrap();
        assert_eq!((p.progress, p.status.as_deref()), (Some(11), Some("watching")));
        assert!(toggle(Some(("watching", 2)), 5, false, Some(12)).is_none(), "not watched yet");
        assert!(toggle(None, 5, false, Some(12)).is_none());
        assert!(toggle(Some(("watching", 2)), 0, true, None).is_none());
    }
}
