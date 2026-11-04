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
CREATE TABLE IF NOT EXISTS source_reliability (
  ext_id     TEXT NOT NULL,
  label      TEXT NOT NULL,
  value      REAL NOT NULL,
  updated_at INTEGER NOT NULL,
  PRIMARY KEY (ext_id, label)
);
CREATE TABLE IF NOT EXISTS picked_source (
  media_id   TEXT NOT NULL,
  episode    INTEGER NOT NULL,
  source_key TEXT NOT NULL,
  picked_at  INTEGER NOT NULL,
  PRIMARY KEY (media_id, episode)
);
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

// ---- sources (wave 1 C; contract in docs/redesign/08) ---------------------------------------------
// New tables go into SCHEMA as `CREATE TABLE IF NOT EXISTS` so existing databases upgrade in place.

impl LocalStore {
    /// Reliability of `(ext_id, label)`; [`crate::sources::INITIAL_RELIABILITY`] when never recorded.
    pub fn reliability(&self, ext_id: &str, label: &str) -> Result<f32, StoreError> {
        self.with(|c| read_reliability(c, ext_id, label))
    }

    /// Fold one playback result into the moving average ([`crate::sources::update_reliability`]) and
    /// return the new value.
    pub fn record_source_result(&self, ext_id: &str, label: &str, ok: bool) -> Result<f32, StoreError> {
        let now = OffsetDateTime::now_utc().unix_timestamp();
        // read + write under the same connection lock so two concurrent results never lose one
        self.with(|c| {
            let next = crate::sources::update_reliability(read_reliability(c, ext_id, label)?, ok);
            c.execute(
                "INSERT INTO source_reliability (ext_id, label, value, updated_at) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(ext_id, label) DO UPDATE SET value = ?3, updated_at = ?4",
                params![ext_id, label, f64::from(next), now],
            )?;
            Ok(next)
        })
    }

    /// The user's manual pick (a `source_key`) for this episode, if any.
    pub fn picked_source(&self, media_id: &str, episode: u32) -> Result<Option<String>, StoreError> {
        self.with(|c| {
            c.query_row("SELECT source_key FROM picked_source WHERE media_id = ?1 AND episode = ?2", params![media_id, episode], |r| r.get(0))
                .optional()
        })
    }

    pub fn set_picked_source(&self, media_id: &str, episode: u32, key: &str) -> Result<(), StoreError> {
        let now = OffsetDateTime::now_utc().unix_timestamp();
        self.with(|c| {
            c.execute(
                "INSERT INTO picked_source (media_id, episode, source_key, picked_at) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(media_id, episode) DO UPDATE SET source_key = ?3, picked_at = ?4",
                params![media_id, episode, key, now],
            )?;
            Ok(())
        })
    }

    /// Called when the episode is completed (the pick expires) or the user resets it.
    pub fn clear_picked_source(&self, media_id: &str, episode: u32) -> Result<(), StoreError> {
        self.with(|c| {
            c.execute("DELETE FROM picked_source WHERE media_id = ?1 AND episode = ?2", params![media_id, episode])?;
            Ok(())
        })
    }
}

fn read_reliability(c: &Connection, ext_id: &str, label: &str) -> rusqlite::Result<f32> {
    let v: Option<f64> =
        c.query_row("SELECT value FROM source_reliability WHERE ext_id = ?1 AND label = ?2", params![ext_id, label], |r| r.get(0)).optional()?;
    // a corrupt / out-of-range row behaves like a fresh one instead of poisoning the ranking
    Ok(v.map(|v| v as f32).filter(|v| v.is_finite() && (0.0..=1.0).contains(v)).unwrap_or(crate::sources::INITIAL_RELIABILITY))
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

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn reliability_starts_at_initial_and_follows_the_ema() {
        use crate::sources::INITIAL_RELIABILITY;
        let dir = tempfile::tempdir().unwrap();
        let s = LocalStore::open(&dir.path().join("data.db")).unwrap();
        assert!(close(s.reliability("io.github.toko", "Alpha").unwrap(), INITIAL_RELIABILITY));
        // 0.8 -> 0.84 -> 0.872 -> 0.6976
        assert!(close(s.record_source_result("io.github.toko", "Alpha", true).unwrap(), 0.84));
        assert!(close(s.record_source_result("io.github.toko", "Alpha", true).unwrap(), 0.872));
        assert!(close(s.record_source_result("io.github.toko", "Alpha", false).unwrap(), 0.6976));
        assert!(close(s.reliability("io.github.toko", "Alpha").unwrap(), 0.6976));
        // keyed per (ext, label)
        assert!(close(s.reliability("io.github.toko", "Beta").unwrap(), INITIAL_RELIABILITY));
        assert!(close(s.reliability("es.spanishpack", "Alpha").unwrap(), INITIAL_RELIABILITY));
    }

    #[test]
    fn reliability_survives_reopen_and_schema_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data.db");
        {
            let s = LocalStore::open(&path).unwrap();
            for _ in 0..30 {
                s.record_source_result("nyaa", "Anon", false).unwrap();
            }
            s.set_picked_source("21", 3, "nyaa|torrent|Anon").unwrap();
        }
        // second open re-runs SCHEMA on an existing database (upgrade in place)
        let s = LocalStore::open(&path).unwrap();
        let v = s.reliability("nyaa", "Anon").unwrap();
        assert!((0.0..0.01).contains(&v), "{v}");
        assert_eq!(s.picked_source("21", 3).unwrap().as_deref(), Some("nyaa|torrent|Anon"));
        // a long success streak converges to 1 and never exceeds it
        for _ in 0..200 {
            s.record_source_result("nyaa", "Anon", true).unwrap();
        }
        let v = s.reliability("nyaa", "Anon").unwrap();
        assert!(v > 0.999 && v <= 1.0, "{v}");
    }

    #[test]
    fn picked_source_set_replace_clear() {
        let dir = tempfile::tempdir().unwrap();
        let s = LocalStore::open(&dir.path().join("data.db")).unwrap();
        assert_eq!(s.picked_source("154587", 7).unwrap(), None);
        s.set_picked_source("154587", 7, "io.github.toko|web|Alpha").unwrap();
        s.set_picked_source("154587", 7, "io.github.toko|web|Beta").unwrap();
        s.set_picked_source("154587", 8, "io.github.nyaa-own|torrent|SubsPlease").unwrap();
        assert_eq!(s.picked_source("154587", 7).unwrap().as_deref(), Some("io.github.toko|web|Beta"));
        s.clear_picked_source("154587", 7).unwrap();
        s.clear_picked_source("154587", 7).unwrap(); // clearing twice is fine
        assert_eq!(s.picked_source("154587", 7).unwrap(), None);
        assert_eq!(s.picked_source("154587", 8).unwrap().as_deref(), Some("io.github.nyaa-own|torrent|SubsPlease"));
    }
}
