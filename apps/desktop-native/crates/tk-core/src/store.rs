//! Local SQLite store (history, settings, TTL cache). The path is injected by the caller: the app resolves
//! `%APPDATA%\Tatakai-Next\data.db`, tests pass a tempdir.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde::de::DeserializeOwned;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::error::StoreError;
use crate::model::{HistoryEntry, continue_watching};

const SCHEMA: &str = "
PRAGMA journal_mode = WAL;
CREATE TABLE IF NOT EXISTS history (
  episode_id   TEXT PRIMARY KEY NOT NULL,
  anime_id     TEXT NOT NULL,
  title        TEXT NOT NULL,
  poster       TEXT,
  episode      INTEGER NOT NULL,
  progress_sec INTEGER NOT NULL DEFAULT 0,
  duration_sec INTEGER,
  completed    INTEGER NOT NULL DEFAULT 0,
  watched_at   TEXT NOT NULL,
  anilist_id   INTEGER,
  mal_id       INTEGER,
  dirty        INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX IF NOT EXISTS idx_history_watched ON history (watched_at DESC);
CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS cache (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL, expires_at INTEGER NOT NULL);
";

pub struct LocalStore {
    conn: Mutex<Connection>,
}

fn ts(t: &OffsetDateTime) -> String {
    t.to_offset(time::UtcOffset::UTC).format(&Rfc3339).unwrap_or_default()
}

fn row_to_entry(r: &rusqlite::Row<'_>) -> rusqlite::Result<HistoryEntry> {
    let watched: String = r.get("watched_at")?;
    Ok(HistoryEntry {
        anime_id: r.get("anime_id")?,
        title: r.get("title")?,
        poster: r.get("poster")?,
        episode_id: r.get("episode_id")?,
        episode: r.get("episode")?,
        progress_sec: r.get("progress_sec")?,
        duration_sec: r.get("duration_sec")?,
        completed: r.get::<_, i64>("completed")? != 0,
        watched_at: OffsetDateTime::parse(&watched, &Rfc3339).unwrap_or(OffsetDateTime::UNIX_EPOCH),
        anilist_id: r.get("anilist_id")?,
        mal_id: r.get("mal_id")?,
    })
}

impl LocalStore {
    /// Open (creating parent directories and the schema) the database at `path`.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self, StoreError> {
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn with<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> Result<T, StoreError> {
        let g = self.conn.lock().map_err(|_| StoreError::Poisoned)?;
        Ok(f(&g)?)
    }

    // ---- history ---------------------------------------------------------------------------------

    /// Local write (marks the row dirty for the next Supabase sync). Newest `watched_at` wins over the stored row.
    pub fn record_history(&self, e: &HistoryEntry) -> Result<(), StoreError> {
        self.put_history(e, true)
    }

    fn put_history(&self, e: &HistoryEntry, dirty: bool) -> Result<(), StoreError> {
        self.with(|c| {
            let existing: Option<HistoryEntry> =
                c.query_row("SELECT * FROM history WHERE episode_id = ?1", [&e.episode_id], row_to_entry).optional()?;
            if let Some(old) = &existing {
                // keep the stored row when it is strictly newer (or equal with more progress), except for local writes
                if dirty { /* a local write is by definition the newest */ } else if !e.wins_over(old) {
                    return Ok(());
                }
            }
            c.execute(
                "INSERT INTO history (episode_id, anime_id, title, poster, episode, progress_sec, duration_sec, completed, watched_at, anilist_id, mal_id, dirty)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
                 ON CONFLICT(episode_id) DO UPDATE SET anime_id=?2, title=?3, poster=?4, episode=?5, progress_sec=?6, duration_sec=?7,
                   completed=?8, watched_at=?9, anilist_id=COALESCE(?10, anilist_id), mal_id=COALESCE(?11, mal_id), dirty=?12",
                params![
                    e.episode_id, e.anime_id, e.title, e.poster, e.episode, e.progress_sec, e.duration_sec,
                    i64::from(e.completed), ts(&e.watched_at), e.anilist_id, e.mal_id, i64::from(dirty)
                ],
            )?;
            Ok(())
        })
    }

    /// Merge rows pulled from Supabase (newest wins, tie -> higher progress). Merged rows are not dirty.
    pub fn merge_remote(&self, remote: &[HistoryEntry]) -> Result<(), StoreError> {
        for e in remote {
            self.put_history(e, false)?;
        }
        Ok(())
    }

    pub fn history(&self, limit: u32) -> Result<Vec<HistoryEntry>, StoreError> {
        self.with(|c| {
            let mut st = c.prepare("SELECT * FROM history ORDER BY watched_at DESC LIMIT ?1")?;
            st.query_map([limit], row_to_entry)?.collect()
        })
    }

    pub fn continue_watching(&self, limit: usize) -> Result<Vec<HistoryEntry>, StoreError> {
        Ok(continue_watching(self.history(500)?, limit))
    }

    pub fn progress_for(&self, anime_id: &str) -> Result<Vec<HistoryEntry>, StoreError> {
        self.with(|c| {
            let mut st = c.prepare("SELECT * FROM history WHERE anime_id = ?1 ORDER BY episode ASC")?;
            st.query_map([anime_id], row_to_entry)?.collect()
        })
    }

    pub fn dirty_history(&self) -> Result<Vec<HistoryEntry>, StoreError> {
        self.with(|c| {
            let mut st = c.prepare("SELECT * FROM history WHERE dirty = 1 ORDER BY watched_at ASC")?;
            st.query_map([], row_to_entry)?.collect()
        })
    }

    /// Clear the dirty flag, but only for rows not modified since `synced` was read.
    pub fn mark_synced(&self, synced: &[HistoryEntry]) -> Result<(), StoreError> {
        self.with(|c| {
            for e in synced {
                c.execute(
                    "UPDATE history SET dirty = 0 WHERE episode_id = ?1 AND watched_at = ?2 AND progress_sec = ?3",
                    params![e.episode_id, ts(&e.watched_at), e.progress_sec],
                )?;
            }
            Ok(())
        })
    }

    // ---- settings --------------------------------------------------------------------------------

    pub fn set_setting<T: Serialize>(&self, key: &str, value: &T) -> Result<(), StoreError> {
        let json = serde_json::to_string(value)?;
        self.with(|c| {
            c.execute("INSERT INTO settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = ?2", params![key, json])?;
            Ok(())
        })
    }

    pub fn get_setting<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, StoreError> {
        let raw: Option<String> =
            self.with(|c| c.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0)).optional())?;
        Ok(match raw {
            Some(s) => Some(serde_json::from_str(&s)?),
            None => None,
        })
    }

    // ---- cache (TTL) -----------------------------------------------------------------------------

    pub fn cache_put<T: Serialize>(&self, key: &str, value: &T, ttl_secs: i64) -> Result<(), StoreError> {
        self.cache_put_at(key, value, ttl_secs, OffsetDateTime::now_utc().unix_timestamp())
    }

    pub fn cache_put_at<T: Serialize>(&self, key: &str, value: &T, ttl_secs: i64, now: i64) -> Result<(), StoreError> {
        let json = serde_json::to_string(value)?;
        self.with(|c| {
            c.execute(
                "INSERT INTO cache (key, value, expires_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(key) DO UPDATE SET value = ?2, expires_at = ?3",
                params![key, json, now + ttl_secs],
            )?;
            Ok(())
        })
    }

    pub fn cache_get<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, StoreError> {
        self.cache_get_at(key, OffsetDateTime::now_utc().unix_timestamp())
    }

    pub fn cache_get_at<T: DeserializeOwned>(&self, key: &str, now: i64) -> Result<Option<T>, StoreError> {
        let raw: Option<String> = self.with(|c| {
            c.query_row("SELECT value FROM cache WHERE key = ?1 AND expires_at > ?2", params![key, now], |r| r.get(0)).optional()
        })?;
        Ok(match raw {
            Some(s) => Some(serde_json::from_str(&s)?),
            None => None,
        })
    }

    /// Delete expired cache rows; returns how many were removed.
    pub fn cache_purge(&self, now: i64) -> Result<usize, StoreError> {
        self.with(|c| c.execute("DELETE FROM cache WHERE expires_at <= ?1", [now]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::MediaRef;

    fn at(secs: i64) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(secs).unwrap()
    }

    fn entry(anime: u32, ep: u32, progress: u32, when: i64) -> HistoryEntry {
        HistoryEntry::for_playback(&MediaRef::anilist(anime), "T", None, ep, progress, Some(1000), at(when)).unwrap()
    }

    #[test]
    fn history_continue_watching_dedupes_per_anime_and_drops_completed() {
        let dir = tempfile::tempdir().unwrap();
        let s = LocalStore::open(&dir.path().join("sub").join("data.db")).unwrap();
        s.record_history(&entry(1, 1, 100, 10)).unwrap();
        s.record_history(&entry(1, 2, 200, 20)).unwrap();
        s.record_history(&entry(2, 1, 950, 30)).unwrap(); // completed (>= 90 %)
        s.record_history(&entry(3, 4, 10, 5)).unwrap();
        let cw = s.continue_watching(10).unwrap();
        let ids: Vec<_> = cw.iter().map(|e| e.episode_id.as_str()).collect();
        assert_eq!(ids, ["1?ep=2", "3?ep=4"]);
        assert_eq!(s.progress_for("1").unwrap().len(), 2);
    }

    #[test]
    fn merge_remote_newest_wins_and_tie_prefers_progress() {
        let s = LocalStore::open_in_memory().unwrap();
        s.record_history(&entry(1, 1, 300, 100)).unwrap();
        s.merge_remote(&[entry(1, 1, 50, 90)]).unwrap(); // older remote loses
        assert_eq!(s.history(5).unwrap()[0].progress_sec, 300);
        s.merge_remote(&[entry(1, 1, 400, 100)]).unwrap(); // tie -> higher progress wins
        assert_eq!(s.history(5).unwrap()[0].progress_sec, 400);
        s.merge_remote(&[entry(1, 1, 10, 200)]).unwrap(); // newer wins even with less progress
        assert_eq!(s.history(5).unwrap()[0].progress_sec, 10);
    }

    #[test]
    fn dirty_tracking() {
        let s = LocalStore::open_in_memory().unwrap();
        let e = entry(1, 1, 100, 10);
        s.record_history(&e).unwrap();
        assert_eq!(s.dirty_history().unwrap().len(), 1);
        s.mark_synced(std::slice::from_ref(&e)).unwrap();
        assert!(s.dirty_history().unwrap().is_empty());
        s.merge_remote(&[entry(2, 1, 1, 1)]).unwrap();
        assert!(s.dirty_history().unwrap().is_empty());
        // a newer local write after the snapshot stays dirty
        s.record_history(&e).unwrap();
        s.record_history(&entry(1, 1, 500, 20)).unwrap();
        s.mark_synced(&[e]).unwrap();
        assert_eq!(s.dirty_history().unwrap().len(), 1);
    }

    #[test]
    fn settings_and_cache_ttl() {
        let s = LocalStore::open_in_memory().unwrap();
        s.set_setting("priority", &vec!["kitsu", "anilist"]).unwrap();
        assert_eq!(s.get_setting::<Vec<String>>("priority").unwrap().unwrap(), ["kitsu", "anilist"]);
        assert!(s.get_setting::<String>("missing").unwrap().is_none());
        s.cache_put_at("k", &42u32, 60, 1000).unwrap();
        assert_eq!(s.cache_get_at::<u32>("k", 1059).unwrap(), Some(42));
        assert_eq!(s.cache_get_at::<u32>("k", 1060).unwrap(), None);
        assert_eq!(s.cache_purge(1060).unwrap(), 1);
    }
}
