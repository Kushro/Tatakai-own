//! `LocalStore` methods over `torrent_episode_map`, `debrid_cache` and `torrent_sessions` (wave A, worker **i**;
//! docs/redesign/15 section 3). Child module of `store` (uses its private helpers).
//!
//! * `torrent_episode_map`: the manual `(file index, episode)` alignment of a torrent (Torrents > Library "Add
//!   torrent", Files "Play this file"); `torrent_play` consults it before `pick_episode_file`. Episode `0` is never
//!   stored (it means "selected, no episode").
//! * `debrid_cache`: one row per `(service, info-hash)` with the last answer of the service and when it was asked.
//! * `torrent_sessions`: one row per torrent the engine ran (classic "Session history"), opened when it enters the
//!   engine and closed when it leaves (or at the next engine start, for a run that died with it open).

use rusqlite::{OptionalExtension, params};

use crate::error::StoreError;
use crate::store::LocalStore;

/// One row of `torrent_sessions` (classic "Session history": every torrent the engine ran, newest first).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TorrentSessionRow {
    pub id: i64,
    pub torrent_id: String,
    pub info_hash: String,
    pub name: String,
    pub media_id: Option<String>,
    pub series: Option<String>,
    pub episode: Option<u32>,
    pub started_at: i64,
    /// `None` while the torrent is still in the engine (or the app died with it: i closes those rows at start).
    pub ended_at: Option<i64>,
    pub downloaded_bytes: u64,
    pub uploaded_bytes: u64,
    pub completed: bool,
}

fn to_i64(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

fn to_u64(v: i64) -> u64 {
    u64::try_from(v).unwrap_or(0)
}

fn session_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<TorrentSessionRow> {
    let episode: Option<i64> = r.get("episode")?;
    Ok(TorrentSessionRow {
        id: r.get("id")?,
        torrent_id: r.get("torrent_id")?,
        info_hash: r.get("info_hash")?,
        name: r.get("name")?,
        media_id: r.get("media_id")?,
        series: r.get("series")?,
        episode: episode.and_then(|e| u32::try_from(e).ok()),
        started_at: r.get("started_at")?,
        ended_at: r.get("ended_at")?,
        downloaded_bytes: to_u64(r.get("downloaded_bytes")?),
        uploaded_bytes: to_u64(r.get("uploaded_bytes")?),
        completed: r.get::<_, i64>("completed")? != 0,
    })
}

impl LocalStore {
    /// Replace the alignment of a torrent (`(file index, episode)` pairs). Pairs with episode `0` are skipped; when a
    /// file index repeats, the last pair wins.
    pub fn set_torrent_episode_map(&self, torrent_id: &str, media_id: Option<&str>, map: &[(usize, u32)]) -> Result<(), StoreError> {
        self.with(|c| {
            let tx = c.unchecked_transaction()?;
            tx.execute("DELETE FROM torrent_episode_map WHERE torrent_id = ?1", [torrent_id])?;
            for (index, episode) in map.iter().filter(|(_, e)| *e > 0) {
                tx.execute(
                    "INSERT OR REPLACE INTO torrent_episode_map (torrent_id, file_index, media_id, episode) VALUES (?1, ?2, ?3, ?4)",
                    params![torrent_id, to_i64(*index as u64), media_id, i64::from(*episode)],
                )?;
            }
            tx.commit()
        })
    }

    /// `(file index, episode)` pairs of a torrent, file index ascending.
    pub fn torrent_episode_map(&self, torrent_id: &str) -> Result<Vec<(usize, u32)>, StoreError> {
        self.with(|c| {
            let mut st =
                c.prepare("SELECT file_index, episode FROM torrent_episode_map WHERE torrent_id = ?1 ORDER BY file_index ASC")?;
            let rows = st.query_map([torrent_id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
            let mut out = Vec::new();
            for row in rows {
                let (index, episode) = row?;
                if let (Ok(index), Ok(episode)) = (usize::try_from(index), u32::try_from(episode)) {
                    out.push((index, episode));
                }
            }
            Ok(out)
        })
    }

    /// `(torrent id, file index)` of every manual alignment that maps `episode` of `media_id`, by torrent id.
    pub fn torrents_for_episode(&self, media_id: &str, episode: u32) -> Result<Vec<(String, usize)>, StoreError> {
        self.with(|c| {
            let mut st = c.prepare(
                "SELECT torrent_id, file_index FROM torrent_episode_map WHERE media_id = ?1 AND episode = ?2 ORDER BY torrent_id ASC",
            )?;
            let rows = st.query_map(params![media_id, i64::from(episode)], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
            let mut out = Vec::new();
            for row in rows {
                let (torrent_id, index) = row?;
                if let Ok(index) = usize::try_from(index) {
                    out.push((torrent_id, index));
                }
            }
            Ok(out)
        })
    }

    pub fn clear_torrent_episode_map(&self, torrent_id: &str) -> Result<(), StoreError> {
        self.with(|c| c.execute("DELETE FROM torrent_episode_map WHERE torrent_id = ?1", [torrent_id]).map(|_| ()))
    }

    /// `Some(cached)` when checked within `max_age_secs` of `now`.
    pub fn debrid_cached(&self, service: &str, info_hash: &str, now: i64, max_age_secs: i64) -> Result<Option<bool>, StoreError> {
        let hash = info_hash.to_ascii_lowercase();
        let row: Option<(i64, i64)> = self.with(|c| {
            c.query_row(
                "SELECT cached, checked_at FROM debrid_cache WHERE service = ?1 AND info_hash = ?2",
                params![service, hash],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
        })?;
        Ok(row.filter(|(_, at)| now.saturating_sub(*at) <= max_age_secs && *at <= now.saturating_add(60)).map(|(c, _)| c != 0))
    }

    pub fn set_debrid_cached(&self, service: &str, info_hash: &str, cached: bool, now: i64) -> Result<(), StoreError> {
        let hash = info_hash.to_ascii_lowercase();
        self.with(|c| {
            c.execute(
                "INSERT OR REPLACE INTO debrid_cache (service, info_hash, cached, checked_at) VALUES (?1, ?2, ?3, ?4)",
                params![service, hash, i64::from(cached), now],
            )
            .map(|_| ())
        })
    }

    // ---- torrent session history (plan 14 section 0.1 #11; checklist `torrent.session-history`) ----

    /// Open a session row when a torrent enters the engine; returns the row id. A torrent that already has an open
    /// row keeps it (its id is returned and the labels are refreshed): one open row per torrent at most.
    pub fn open_torrent_session(&self, row: &TorrentSessionRow) -> Result<i64, StoreError> {
        self.with(|c| {
            let open: Option<i64> = c
                .query_row(
                    "SELECT id FROM torrent_sessions WHERE torrent_id = ?1 AND ended_at IS NULL ORDER BY id DESC LIMIT 1",
                    [&row.torrent_id],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(id) = open {
                c.execute(
                    "UPDATE torrent_sessions SET name = ?2, media_id = COALESCE(?3, media_id), series = COALESCE(?4, series),
                     episode = COALESCE(?5, episode) WHERE id = ?1",
                    params![id, row.name, row.media_id, row.series, row.episode.map(i64::from)],
                )?;
                return Ok(id);
            }
            c.execute(
                "INSERT INTO torrent_sessions (torrent_id, info_hash, name, media_id, series, episode, started_at, ended_at,
                 downloaded_bytes, uploaded_bytes, completed) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL, ?8, ?9, ?10)",
                params![
                    row.torrent_id,
                    row.info_hash.to_ascii_lowercase(),
                    row.name,
                    row.media_id,
                    row.series,
                    row.episode.map(i64::from),
                    row.started_at,
                    to_i64(row.downloaded_bytes),
                    to_i64(row.uploaded_bytes),
                    i64::from(row.completed),
                ],
            )?;
            Ok(c.last_insert_rowid())
        })
    }

    /// Close the open row of `torrent_id` with the final counters (no open row: nothing happens).
    pub fn close_torrent_session(&self, torrent_id: &str, ended_at: i64, downloaded_bytes: u64, uploaded_bytes: u64, completed: bool) -> Result<(), StoreError> {
        self.with(|c| {
            c.execute(
                "UPDATE torrent_sessions SET ended_at = MAX(?2, started_at), downloaded_bytes = MAX(downloaded_bytes, ?3),
                 uploaded_bytes = MAX(uploaded_bytes, ?4), completed = MAX(completed, ?5)
                 WHERE torrent_id = ?1 AND ended_at IS NULL",
                params![torrent_id, ended_at, to_i64(downloaded_bytes), to_i64(uploaded_bytes), i64::from(completed)],
            )
            .map(|_| ())
        })
    }

    /// Newest first, at most `limit`.
    pub fn torrent_sessions(&self, limit: u32) -> Result<Vec<TorrentSessionRow>, StoreError> {
        self.with(|c| {
            let mut st = c.prepare("SELECT * FROM torrent_sessions ORDER BY started_at DESC, id DESC LIMIT ?1")?;
            let rows = st.query_map([i64::from(limit)], session_row)?;
            rows.collect()
        })
    }

    /// Delete every closed row (open ones stay). Returns the rows removed.
    pub fn clear_torrent_sessions(&self) -> Result<usize, StoreError> {
        self.with(|c| c.execute("DELETE FROM torrent_sessions WHERE ended_at IS NOT NULL", []))
    }

    /// Close every open row except those of `keep` (torrents still in the engine) at `ended_at`. Called when the
    /// engine starts: rows left open by a run that ended abruptly. Returns how many rows were closed.
    pub fn close_stale_torrent_sessions(&self, keep: &[String], ended_at: i64) -> Result<usize, StoreError> {
        self.with(|c| {
            let mut st = c.prepare("SELECT id, torrent_id FROM torrent_sessions WHERE ended_at IS NULL")?;
            let open: Vec<(i64, String)> =
                st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
            let mut closed = 0;
            for (id, torrent_id) in open {
                if keep.iter().any(|k| k == &torrent_id) {
                    continue;
                }
                closed += c.execute("UPDATE torrent_sessions SET ended_at = MAX(?2, started_at) WHERE id = ?1", params![id, ended_at])?;
            }
            Ok(closed)
        })
    }

    /// Ids of the torrents that have an open session row.
    pub fn open_torrent_session_ids(&self) -> Result<Vec<String>, StoreError> {
        self.with(|c| {
            let mut st = c.prepare("SELECT DISTINCT torrent_id FROM torrent_sessions WHERE ended_at IS NULL")?;
            let rows = st.query_map([], |r| r.get(0))?;
            rows.collect()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(torrent: &str, started: i64) -> TorrentSessionRow {
        TorrentSessionRow {
            id: 0,
            torrent_id: torrent.to_owned(),
            info_hash: torrent.to_ascii_uppercase(),
            name: format!("[Group] {torrent}"),
            media_id: Some("21".into()),
            series: Some("Show".into()),
            episode: Some(3),
            started_at: started,
            ended_at: None,
            downloaded_bytes: 10,
            uploaded_bytes: 0,
            completed: false,
        }
    }

    #[test]
    fn episode_map_replaces_and_skips_zero() {
        let s = LocalStore::open_in_memory().unwrap();
        s.set_torrent_episode_map("t", Some("21"), &[(3, 2), (1, 1), (4, 0)]).unwrap();
        assert_eq!(s.torrent_episode_map("t").unwrap(), vec![(1, 1), (3, 2)]);
        s.set_torrent_episode_map("t", None, &[(7, 9), (7, 10)]).unwrap();
        assert_eq!(s.torrent_episode_map("t").unwrap(), vec![(7, 10)]);
        assert!(s.torrent_episode_map("other").unwrap().is_empty());
        s.clear_torrent_episode_map("t").unwrap();
        assert!(s.torrent_episode_map("t").unwrap().is_empty());
    }

    #[test]
    fn episode_lookup_by_title() {
        let s = LocalStore::open_in_memory().unwrap();
        s.set_torrent_episode_map("b", Some("21"), &[(0, 1), (1, 2)]).unwrap();
        s.set_torrent_episode_map("a", Some("21"), &[(5, 2)]).unwrap();
        s.set_torrent_episode_map("c", Some("99"), &[(0, 2)]).unwrap();
        assert_eq!(s.torrents_for_episode("21", 2).unwrap(), vec![("a".to_owned(), 5), ("b".to_owned(), 1)]);
        assert!(s.torrents_for_episode("21", 3).unwrap().is_empty());
    }

    #[test]
    fn debrid_cache_honours_the_ttl() {
        let s = LocalStore::open_in_memory().unwrap();
        assert_eq!(s.debrid_cached("torbox", "ABC", 100, 600).unwrap(), None);
        s.set_debrid_cached("torbox", "ABC", true, 100).unwrap();
        assert_eq!(s.debrid_cached("torbox", "abc", 500, 600).unwrap(), Some(true));
        assert_eq!(s.debrid_cached("torbox", "abc", 701, 600).unwrap(), None, "stale");
        assert_eq!(s.debrid_cached("real_debrid", "abc", 500, 600).unwrap(), None, "per service");
        s.set_debrid_cached("torbox", "abc", false, 800).unwrap();
        assert_eq!(s.debrid_cached("torbox", "abc", 810, 600).unwrap(), Some(false));
    }

    #[test]
    fn sessions_open_close_list_and_clear() {
        let s = LocalStore::open_in_memory().unwrap();
        let a = s.open_torrent_session(&session("a", 10)).unwrap();
        assert_eq!(s.open_torrent_session(&session("a", 15)).unwrap(), a, "one open row per torrent");
        let b = s.open_torrent_session(&session("b", 20)).unwrap();
        assert_ne!(a, b);
        s.close_torrent_session("a", 30, 500, 40, true).unwrap();
        let rows = s.torrent_sessions(10).unwrap();
        assert_eq!(rows.iter().map(|r| r.torrent_id.as_str()).collect::<Vec<_>>(), ["b", "a"]);
        let closed = &rows[1];
        assert_eq!((closed.ended_at, closed.downloaded_bytes, closed.uploaded_bytes, closed.completed), (Some(30), 500, 40, true));
        assert_eq!(closed.info_hash, "a", "hashes are stored lowercase");
        assert_eq!(rows[0].ended_at, None);
        assert_eq!(s.open_torrent_session_ids().unwrap(), vec!["b".to_owned()]);
        // A new run of `a` gets a new row.
        let again = s.open_torrent_session(&session("a", 40)).unwrap();
        assert_ne!(again, a);
        assert_eq!(s.torrent_sessions(1).unwrap()[0].id, again);
        assert_eq!(s.clear_torrent_sessions().unwrap(), 1, "only closed rows go");
        assert_eq!(s.torrent_sessions(10).unwrap().len(), 2);
    }

    #[test]
    fn stale_open_rows_close_at_engine_start() {
        let s = LocalStore::open_in_memory().unwrap();
        s.open_torrent_session(&session("a", 10)).unwrap();
        s.open_torrent_session(&session("b", 20)).unwrap();
        assert_eq!(s.close_stale_torrent_sessions(&["b".to_owned()], 5).unwrap(), 1);
        let rows = s.torrent_sessions(10).unwrap();
        let a = rows.iter().find(|r| r.torrent_id == "a").unwrap();
        assert_eq!(a.ended_at, Some(10), "never before it started");
        assert!(rows.iter().find(|r| r.torrent_id == "b").unwrap().ended_at.is_none());
    }
}
