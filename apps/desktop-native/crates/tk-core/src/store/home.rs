//! `LocalStore` methods over `tracked_shows`, `airing_reminders`, `recs_feedback` and `home_visits` (wave B, worker
//! **b**; docs/redesign/16 section 3.5). Child module of `store`: it uses the private `with` helper of `mod.rs`.
//!
//! * `tracked_shows`: "My shows" of the airing calendar (`details.track-show`, `home.calendar.my-shows`).
//! * `airing_reminders`: which `(media, episode)` reminders were already shown (`home.calendar.reminders`), so a
//!   reminder fires once; rows older than [`REMINDER_RETENTION_SECS`] are pruned when a new one is written.
//! * `recs_feedback`: local "Not interested" / "Skip" / "Like" on recommendations (`home.recs.feedback`).
//! * `home_visits`: last visit per section (`home`, `manga`, `novel`) for "new since your last visit" rows.

use rusqlite::{OptionalExtension, params};

use crate::error::StoreError;
use crate::store::LocalStore;

/// Tables of this file (the contract's `SCHEMA` creates them; the test below checks they exist).
pub const TABLES: &[&str] = &["tracked_shows", "airing_reminders", "recs_feedback", "home_visits"];

/// Announced reminders are kept this long (30 days), then pruned.
pub const REMINDER_RETENTION_SECS: i64 = 30 * 86_400;

/// One row of `tracked_shows`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackedShowRow {
    /// UI media id (`"21"`, `"mal:5114"`).
    pub media_id: String,
    pub title: String,
    pub poster: Option<String>,
    pub anilist_id: Option<u32>,
    pub added_at: i64,
}

/// One row of `recs_feedback` (`feedback` = `RecsFeedbackVm::id()`: `not_interested | skip | like`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecsFeedbackRow {
    pub media_id: String,
    pub feedback: String,
    pub created_at: i64,
}

impl LocalStore {
    /// Add (or refresh the title / poster of) a tracked show; the first `added_at` is kept.
    pub fn set_tracked(&self, row: &TrackedShowRow) -> Result<(), StoreError> {
        self.with(|c| {
            c.execute(
                "INSERT INTO tracked_shows (media_id, title, poster, anilist_id, added_at) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(media_id) DO UPDATE SET title = ?2, poster = COALESCE(?3, poster),
                   anilist_id = COALESCE(?4, anilist_id)",
                params![row.media_id, row.title, row.poster, row.anilist_id, row.added_at],
            )?;
            Ok(())
        })
    }

    pub fn remove_tracked(&self, media_id: &str) -> Result<(), StoreError> {
        self.with(|c| c.execute("DELETE FROM tracked_shows WHERE media_id = ?1", [media_id]).map(|_| ()))
    }

    pub fn is_tracked(&self, media_id: &str) -> Result<bool, StoreError> {
        self.with(|c| {
            c.query_row("SELECT 1 FROM tracked_shows WHERE media_id = ?1", [media_id], |_| Ok(()))
                .optional()
                .map(|r| r.is_some())
        })
    }

    /// Every tracked show, most recently added first.
    pub fn tracked_shows(&self) -> Result<Vec<TrackedShowRow>, StoreError> {
        self.with(|c| {
            let mut st = c.prepare(
                "SELECT media_id, title, poster, anilist_id, added_at FROM tracked_shows ORDER BY added_at DESC, media_id",
            )?;
            st.query_map([], |r| {
                Ok(TrackedShowRow {
                    media_id: r.get(0)?,
                    title: r.get(1)?,
                    poster: r.get(2)?,
                    anilist_id: r.get(3)?,
                    added_at: r.get(4)?,
                })
            })?
            .collect()
        })
    }

    /// Remember that the reminder of `(media_id, episode)` was shown. Returns `false` when it already was (so the
    /// caller announces each episode once even with several pollers). Prunes rows older than the retention.
    pub fn reminder_announced(&self, media_id: &str, episode: u32, now: i64) -> Result<bool, StoreError> {
        self.with(|c| {
            c.execute("DELETE FROM airing_reminders WHERE announced_at < ?1", [now - REMINDER_RETENTION_SECS])?;
            let n = c.execute(
                "INSERT OR IGNORE INTO airing_reminders (media_id, episode, announced_at) VALUES (?1, ?2, ?3)",
                params![media_id, episode, now],
            )?;
            Ok(n > 0)
        })
    }

    pub fn was_announced(&self, media_id: &str, episode: u32) -> Result<bool, StoreError> {
        self.with(|c| {
            c.query_row(
                "SELECT 1 FROM airing_reminders WHERE media_id = ?1 AND episode = ?2",
                params![media_id, episode],
                |_| Ok(()),
            )
            .optional()
            .map(|r| r.is_some())
        })
    }

    /// Store (replace) the feedback on a recommended title.
    pub fn set_recs_feedback(&self, media_id: &str, feedback: &str, now: i64) -> Result<(), StoreError> {
        self.with(|c| {
            c.execute(
                "INSERT INTO recs_feedback (media_id, feedback, created_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(media_id) DO UPDATE SET feedback = ?2, created_at = ?3",
                params![media_id, feedback, now],
            )?;
            Ok(())
        })
    }

    /// Every feedback row, newest first.
    pub fn recs_feedback(&self) -> Result<Vec<RecsFeedbackRow>, StoreError> {
        self.with(|c| {
            let mut st = c.prepare("SELECT media_id, feedback, created_at FROM recs_feedback ORDER BY created_at DESC, media_id")?;
            st.query_map([], |r| Ok(RecsFeedbackRow { media_id: r.get(0)?, feedback: r.get(1)?, created_at: r.get(2)? }))?
                .collect()
        })
    }

    pub fn clear_recs_feedback(&self) -> Result<usize, StoreError> {
        self.with(|c| c.execute("DELETE FROM recs_feedback", []))
    }

    /// Record a visit to `section` at `now`.
    pub fn home_visited(&self, section: &str, now: i64) -> Result<(), StoreError> {
        self.with(|c| {
            c.execute(
                "INSERT INTO home_visits (section, visited_at) VALUES (?1, ?2)
                 ON CONFLICT(section) DO UPDATE SET visited_at = ?2",
                params![section, now],
            )?;
            Ok(())
        })
    }

    pub fn last_home_visit(&self, section: &str) -> Result<Option<i64>, StoreError> {
        self.with(|c| {
            c.query_row("SELECT visited_at FROM home_visits WHERE section = ?1", [section], |r| r.get::<_, i64>(0))
                .optional()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, at: i64) -> TrackedShowRow {
        TrackedShowRow { media_id: id.into(), title: format!("T{id}"), poster: None, anilist_id: id.parse().ok(), added_at: at }
    }

    #[test]
    fn tables_exist() {
        let s = LocalStore::open_in_memory().unwrap();
        for t in TABLES {
            assert!(s.table_exists(t).unwrap(), "{t} missing");
        }
    }

    #[test]
    fn tracked_shows_round_trip() {
        let s = LocalStore::open_in_memory().unwrap();
        s.set_tracked(&row("21", 10)).unwrap();
        s.set_tracked(&row("1", 20)).unwrap();
        // re-tracking keeps the first added_at and refreshes the poster
        s.set_tracked(&TrackedShowRow { poster: Some("p".into()), ..row("21", 99) }).unwrap();
        let all = s.tracked_shows().unwrap();
        assert_eq!(all.iter().map(|r| r.media_id.as_str()).collect::<Vec<_>>(), ["1", "21"]);
        assert_eq!(all[1].added_at, 10);
        assert_eq!(all[1].poster.as_deref(), Some("p"));
        assert!(s.is_tracked("21").unwrap());
        s.remove_tracked("21").unwrap();
        assert!(!s.is_tracked("21").unwrap());
    }

    #[test]
    fn reminders_fire_once_and_are_pruned() {
        let s = LocalStore::open_in_memory().unwrap();
        assert!(s.reminder_announced("21", 5, 1_000).unwrap());
        assert!(!s.reminder_announced("21", 5, 1_100).unwrap());
        assert!(s.was_announced("21", 5).unwrap());
        assert!(s.reminder_announced("21", 6, 1_000 + REMINDER_RETENTION_SECS + 1).unwrap());
        assert!(!s.was_announced("21", 5).unwrap(), "old row pruned");
    }

    #[test]
    fn feedback_replaces_and_clears() {
        let s = LocalStore::open_in_memory().unwrap();
        s.set_recs_feedback("5", "skip", 1).unwrap();
        s.set_recs_feedback("5", "not_interested", 2).unwrap();
        s.set_recs_feedback("6", "like", 3).unwrap();
        let rows = s.recs_feedback().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0].media_id.as_str(), rows[1].feedback.as_str()), ("6", "not_interested"));
        assert_eq!(s.clear_recs_feedback().unwrap(), 2);
    }

    #[test]
    fn home_visits_are_per_section() {
        let s = LocalStore::open_in_memory().unwrap();
        assert_eq!(s.last_home_visit("home").unwrap(), None);
        s.home_visited("home", 5).unwrap();
        s.home_visited("home", 9).unwrap();
        s.home_visited("manga", 7).unwrap();
        assert_eq!(s.last_home_visit("home").unwrap(), Some(9));
        assert_eq!(s.last_home_visit("manga").unwrap(), Some(7));
    }
}
