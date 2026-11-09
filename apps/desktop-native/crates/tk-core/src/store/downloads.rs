//! `LocalStore` methods over `downloads`, `local_library` and `auto_download_runs` (wave B, worker **h**;
//! docs/redesign/16 section 3.5). Child module of `store`.
//!
//! * `downloads`: one row per queued / finished web download (HLS, MP4) or manga / novel chapter. `state` is
//!   `DownloadStateVm::id()` of tk-ui (`queued | downloading | paused | completed | failed | cancelled | broken`) and
//!   `kind` is `DownloadKindVm::id()` (`hls | mp4 | manga | novel`; torrents stay in the torrent engine). The
//!   resume checkpoint is `segments_done` (HLS segments / chapter pages already on disk) plus the `.part` length of
//!   an MP4: the engine never trusts `bytes_done` alone.
//! * `local_library`: video files imported or scanned from the library folders, with their (optional) match.
//! * `auto_download_runs`: one row per auto-download pass, newest first.
//!
//! The engine also marks finished manga / novel chapters in worker l's `offline_chapters` table
//! ([`LocalStore::mark_offline_chapter`]); the row shape is the contract's `SCHEMA`.

use rusqlite::{OptionalExtension, params};

use crate::error::StoreError;
use crate::store::LocalStore;

/// Tables of this file (the contract's `SCHEMA` creates them; the test below checks they exist).
pub const TABLES: &[&str] = &["downloads", "local_library", "auto_download_runs"];

/// `state` values that belong to the queue (everything else is history).
pub const ACTIVE_STATES: [&str; 3] = ["queued", "downloading", "paused"];

/// One row of `downloads`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DownloadRow {
    pub id: i64,
    pub media_id: String,
    pub title: String,
    pub poster: Option<String>,
    pub episode: Option<u32>,
    pub chapter: Option<String>,
    pub kind: String,
    pub state: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub source_id: Option<String>,
    pub quality: Option<String>,
    pub audio_lang: Option<String>,
    pub sub_lang: Option<String>,
    /// Target file (episodes) or directory (chapters).
    pub path: Option<String>,
    pub bytes_done: u64,
    pub bytes_total: Option<u64>,
    pub segments_done: u32,
    pub segments_total: Option<u32>,
    pub error: Option<String>,
    pub added_at: i64,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
    pub watched_at: Option<i64>,
}

/// One row of `local_library`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LocalItemRow {
    pub id: i64,
    pub path: String,
    pub file_name: String,
    pub size_bytes: u64,
    pub media_id: Option<String>,
    pub title: Option<String>,
    pub poster: Option<String>,
    pub episode: Option<u32>,
    pub added_at: i64,
    pub verified_at: Option<i64>,
}

/// One row of `auto_download_runs`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AutoRunRow {
    pub id: i64,
    pub ran_at: i64,
    pub checked: u32,
    pub enqueued: u32,
    pub skipped: Vec<String>,
}

/// A finished chapter for worker l's `offline_chapters` table (`kind` = `manga` | `novel`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OfflineChapterMark {
    pub kind: String,
    pub media_id: String,
    pub ext_id: String,
    pub chapter_id: String,
    pub dir: Option<String>,
    pub text_json: Option<String>,
    pub pages: Option<u32>,
    pub bytes: u64,
    pub saved_at: i64,
}

fn to_i64(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

fn to_u64(v: i64) -> u64 {
    u64::try_from(v).unwrap_or(0)
}

fn opt_u32(v: Option<i64>) -> Option<u32> {
    v.and_then(|n| u32::try_from(n).ok())
}

fn headers_json(h: &[(String, String)]) -> String {
    serde_json::to_string(h).unwrap_or_else(|_| "[]".to_owned())
}

fn parse_headers(s: &str) -> Vec<(String, String)> {
    serde_json::from_str(s).unwrap_or_default()
}

fn download_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<DownloadRow> {
    let headers: String = r.get("headers_json")?;
    Ok(DownloadRow {
        id: r.get("id")?,
        media_id: r.get("media_id")?,
        title: r.get("title")?,
        poster: r.get("poster")?,
        episode: opt_u32(r.get("episode")?),
        chapter: r.get("chapter")?,
        kind: r.get("kind")?,
        state: r.get("state")?,
        url: r.get("url")?,
        headers: parse_headers(&headers),
        source_id: r.get("source_id")?,
        quality: r.get("quality")?,
        audio_lang: r.get("audio_lang")?,
        sub_lang: r.get("sub_lang")?,
        path: r.get("path")?,
        bytes_done: to_u64(r.get("bytes_done")?),
        bytes_total: r.get::<_, Option<i64>>("bytes_total")?.map(to_u64),
        segments_done: opt_u32(Some(r.get("segments_done")?)).unwrap_or(0),
        segments_total: opt_u32(r.get("segments_total")?),
        error: r.get("error")?,
        added_at: r.get("added_at")?,
        started_at: r.get("started_at")?,
        completed_at: r.get("completed_at")?,
        watched_at: r.get("watched_at")?,
    })
}

fn local_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<LocalItemRow> {
    Ok(LocalItemRow {
        id: r.get("id")?,
        path: r.get("path")?,
        file_name: r.get("file_name")?,
        size_bytes: to_u64(r.get("size_bytes")?),
        media_id: r.get("media_id")?,
        title: r.get("title")?,
        poster: r.get("poster")?,
        episode: opt_u32(r.get("episode")?),
        added_at: r.get("added_at")?,
        verified_at: r.get("verified_at")?,
    })
}

fn auto_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<AutoRunRow> {
    let skipped: String = r.get("skipped_json")?;
    Ok(AutoRunRow {
        id: r.get("id")?,
        ran_at: r.get("ran_at")?,
        checked: opt_u32(Some(r.get("checked")?)).unwrap_or(0),
        enqueued: opt_u32(Some(r.get("enqueued")?)).unwrap_or(0),
        skipped: serde_json::from_str(&skipped).unwrap_or_default(),
    })
}

impl LocalStore {
    // ---- downloads ---------------------------------------------------------------------------------

    /// Insert a new row (`id` is ignored); returns the new id.
    pub fn insert_download(&self, d: &DownloadRow) -> Result<i64, StoreError> {
        self.with(|c| {
            c.execute(
                "INSERT INTO downloads (media_id, title, poster, episode, chapter, kind, state, url, headers_json, source_id,
                   quality, audio_lang, sub_lang, path, bytes_done, bytes_total, segments_done, segments_total, error,
                   added_at, started_at, completed_at, watched_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23)",
                params![
                    d.media_id,
                    d.title,
                    d.poster,
                    d.episode.map(i64::from),
                    d.chapter,
                    d.kind,
                    d.state,
                    d.url,
                    headers_json(&d.headers),
                    d.source_id,
                    d.quality,
                    d.audio_lang,
                    d.sub_lang,
                    d.path,
                    to_i64(d.bytes_done),
                    d.bytes_total.map(to_i64),
                    i64::from(d.segments_done),
                    d.segments_total.map(i64::from),
                    d.error,
                    d.added_at,
                    d.started_at,
                    d.completed_at,
                    d.watched_at,
                ],
            )?;
            Ok(c.last_insert_rowid())
        })
    }

    /// Progress checkpoint of a running download.
    pub fn update_download_progress(
        &self,
        id: i64,
        bytes_done: u64,
        bytes_total: Option<u64>,
        segments_done: u32,
        segments_total: Option<u32>,
    ) -> Result<(), StoreError> {
        self.with(|c| {
            c.execute(
                "UPDATE downloads SET bytes_done = ?2, bytes_total = COALESCE(?3, bytes_total), segments_done = ?4,
                   segments_total = COALESCE(?5, segments_total) WHERE id = ?1",
                params![id, to_i64(bytes_done), bytes_total.map(to_i64), i64::from(segments_done), segments_total.map(i64::from)],
            )
            .map(|_| ())
        })
    }

    /// Change the state. `error` replaces the stored one (`None` clears it); `path` / `completed_at` only overwrite
    /// when given. `downloading` also stamps `started_at` the first time.
    pub fn set_download_state(
        &self,
        id: i64,
        state: &str,
        error: Option<&str>,
        path: Option<&str>,
        completed_at: Option<i64>,
    ) -> Result<(), StoreError> {
        self.with(|c| {
            c.execute(
                "UPDATE downloads SET state = ?2, error = ?3, path = COALESCE(?4, path), completed_at = COALESCE(?5, completed_at),
                   started_at = CASE WHEN ?2 = 'downloading' AND started_at IS NULL THEN strftime('%s','now') ELSE started_at END
                 WHERE id = ?1",
                params![id, state, error, path, completed_at],
            )
            .map(|_| ())
        })
    }

    /// Replace the source of a row (a retry that resolved a fresh URL).
    pub fn set_download_source(&self, id: i64, kind: &str, url: &str, headers: &[(String, String)]) -> Result<(), StoreError> {
        self.with(|c| {
            c.execute(
                "UPDATE downloads SET kind = ?2, url = ?3, headers_json = ?4 WHERE id = ?1",
                params![id, kind, url, headers_json(headers)],
            )
            .map(|_| ())
        })
    }

    /// Forget the checkpoint (a download that starts over).
    pub fn reset_download_progress(&self, id: i64) -> Result<(), StoreError> {
        self.with(|c| {
            c.execute(
                "UPDATE downloads SET bytes_done = 0, segments_done = 0, completed_at = NULL, error = NULL WHERE id = ?1",
                [id],
            )
            .map(|_| ())
        })
    }

    pub fn download(&self, id: i64) -> Result<Option<DownloadRow>, StoreError> {
        self.with(|c| c.query_row("SELECT * FROM downloads WHERE id = ?1", [id], download_row).optional())
    }

    /// Every row; `active_first` puts queued / downloading / paused rows first (oldest first, the queue order), then
    /// the history newest first.
    pub fn downloads(&self, active_first: bool) -> Result<Vec<DownloadRow>, StoreError> {
        let sql = if active_first {
            "SELECT * FROM downloads ORDER BY CASE WHEN state IN ('queued','downloading','paused') THEN 0 ELSE 1 END,
               CASE WHEN state IN ('queued','downloading','paused') THEN added_at ELSE -COALESCE(completed_at, added_at) END,
               id ASC"
        } else {
            "SELECT * FROM downloads ORDER BY added_at DESC, id DESC"
        };
        self.with(|c| {
            let mut st = c.prepare(sql)?;
            st.query_map([], download_row)?.collect()
        })
    }

    /// Rows in `state`, oldest first (the scheduler takes `queued` in this order).
    pub fn downloads_in_state(&self, state: &str) -> Result<Vec<DownloadRow>, StoreError> {
        self.with(|c| {
            let mut st = c.prepare("SELECT * FROM downloads WHERE state = ?1 ORDER BY added_at ASC, id ASC")?;
            st.query_map([state], download_row)?.collect()
        })
    }

    /// Rows of one title, newest first.
    pub fn downloads_for(&self, media_id: &str) -> Result<Vec<DownloadRow>, StoreError> {
        self.with(|c| {
            let mut st = c.prepare("SELECT * FROM downloads WHERE media_id = ?1 ORDER BY added_at DESC, id DESC")?;
            st.query_map([media_id], download_row)?.collect()
        })
    }

    /// The newest completed row of an episode.
    pub fn completed_download_for(&self, media_id: &str, episode: u32) -> Result<Option<DownloadRow>, StoreError> {
        self.with(|c| {
            c.query_row(
                "SELECT * FROM downloads WHERE media_id = ?1 AND episode = ?2 AND state = 'completed'
                   ORDER BY completed_at DESC, id DESC LIMIT 1",
                params![media_id, i64::from(episode)],
                download_row,
            )
            .optional()
        })
    }

    pub fn delete_download(&self, id: i64) -> Result<(), StoreError> {
        self.with(|c| c.execute("DELETE FROM downloads WHERE id = ?1", [id]).map(|_| ()))
    }

    /// Drop every row that is not queued / downloading / paused; returns how many went.
    pub fn clear_download_history(&self) -> Result<usize, StoreError> {
        self.with(|c| c.execute("DELETE FROM downloads WHERE state NOT IN ('queued','downloading','paused')", []))
    }

    /// Stamp `watched_at` on the completed rows of an episode (first time only).
    pub fn mark_download_watched(&self, media_id: &str, episode: u32, now: i64) -> Result<(), StoreError> {
        self.with(|c| {
            c.execute(
                "UPDATE downloads SET watched_at = ?3 WHERE media_id = ?1 AND episode = ?2 AND watched_at IS NULL AND state = 'completed'",
                params![media_id, i64::from(episode), now],
            )
            .map(|_| ())
        })
    }

    /// Sum of `bytes_total` (else `bytes_done`) of the completed rows.
    pub fn completed_download_bytes(&self) -> Result<u64, StoreError> {
        self.with(|c| {
            c.query_row(
                "SELECT COALESCE(SUM(COALESCE(bytes_total, bytes_done)), 0) FROM downloads WHERE state = 'completed'",
                [],
                |r| r.get::<_, i64>(0),
            )
            .map(to_u64)
        })
    }

    // ---- local library ------------------------------------------------------------------------------

    /// Insert a file (by path) or refresh its size; the match of an existing row is kept unless `item` carries one.
    /// Returns `(id, inserted)`.
    pub fn upsert_local_item(&self, item: &LocalItemRow) -> Result<(i64, bool), StoreError> {
        self.with(|c| {
            let existing: Option<i64> =
                c.query_row("SELECT id FROM local_library WHERE path = ?1", [&item.path], |r| r.get(0)).optional()?;
            match existing {
                Some(id) => {
                    c.execute(
                        "UPDATE local_library SET size_bytes = ?2, file_name = ?3,
                           media_id = COALESCE(?4, media_id), title = COALESCE(?5, title), poster = COALESCE(?6, poster),
                           episode = COALESCE(?7, episode)
                         WHERE id = ?1",
                        params![
                            id,
                            to_i64(item.size_bytes),
                            item.file_name,
                            item.media_id,
                            item.title,
                            item.poster,
                            item.episode.map(i64::from)
                        ],
                    )?;
                    Ok((id, false))
                }
                None => {
                    c.execute(
                        "INSERT INTO local_library (path, file_name, size_bytes, media_id, title, poster, episode, added_at, verified_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                        params![
                            item.path,
                            item.file_name,
                            to_i64(item.size_bytes),
                            item.media_id,
                            item.title,
                            item.poster,
                            item.episode.map(i64::from),
                            item.added_at,
                            item.verified_at
                        ],
                    )?;
                    Ok((c.last_insert_rowid(), true))
                }
            }
        })
    }

    /// Every file, newest first.
    pub fn local_items(&self) -> Result<Vec<LocalItemRow>, StoreError> {
        self.with(|c| {
            let mut st = c.prepare("SELECT * FROM local_library ORDER BY added_at DESC, id DESC")?;
            st.query_map([], local_row)?.collect()
        })
    }

    pub fn local_item(&self, id: i64) -> Result<Option<LocalItemRow>, StoreError> {
        self.with(|c| c.query_row("SELECT * FROM local_library WHERE id = ?1", [id], local_row).optional())
    }

    /// The newest file matched to an episode.
    pub fn local_item_for(&self, media_id: &str, episode: u32) -> Result<Option<LocalItemRow>, StoreError> {
        self.with(|c| {
            c.query_row(
                "SELECT * FROM local_library WHERE media_id = ?1 AND episode = ?2 ORDER BY added_at DESC, id DESC LIMIT 1",
                params![media_id, i64::from(episode)],
                local_row,
            )
            .optional()
        })
    }

    /// Set (or fix) the match of a file.
    pub fn set_local_match(
        &self,
        id: i64,
        media_id: &str,
        title: &str,
        poster: Option<&str>,
        episode: Option<u32>,
    ) -> Result<(), StoreError> {
        self.with(|c| {
            c.execute(
                "UPDATE local_library SET media_id = ?2, title = ?3, poster = ?4, episode = ?5 WHERE id = ?1",
                params![id, media_id, title, poster, episode.map(i64::from)],
            )
            .map(|_| ())
        })
    }

    pub fn set_local_verified(&self, id: i64, at: i64, size_bytes: u64) -> Result<(), StoreError> {
        self.with(|c| {
            c.execute("UPDATE local_library SET verified_at = ?2, size_bytes = ?3 WHERE id = ?1", params![id, at, to_i64(size_bytes)])
                .map(|_| ())
        })
    }

    pub fn delete_local_item(&self, id: i64) -> Result<(), StoreError> {
        self.with(|c| c.execute("DELETE FROM local_library WHERE id = ?1", [id]).map(|_| ()))
    }

    // ---- auto-download runs -------------------------------------------------------------------------

    pub fn insert_auto_run(&self, run: &AutoRunRow) -> Result<i64, StoreError> {
        let skipped = serde_json::to_string(&run.skipped).unwrap_or_else(|_| "[]".to_owned());
        self.with(|c| {
            c.execute(
                "INSERT INTO auto_download_runs (ran_at, checked, enqueued, skipped_json) VALUES (?1, ?2, ?3, ?4)",
                params![run.ran_at, i64::from(run.checked), i64::from(run.enqueued), skipped],
            )?;
            // Keep the last 50 passes.
            c.execute("DELETE FROM auto_download_runs WHERE id NOT IN (SELECT id FROM auto_download_runs ORDER BY ran_at DESC, id DESC LIMIT 50)", [])?;
            Ok(c.last_insert_rowid())
        })
    }

    pub fn last_auto_run(&self) -> Result<Option<AutoRunRow>, StoreError> {
        self.with(|c| {
            c.query_row("SELECT * FROM auto_download_runs ORDER BY ran_at DESC, id DESC LIMIT 1", [], auto_row).optional()
        })
    }

    // ---- offline chapters (worker l's table; the engine marks finished chapter downloads) -----------------

    pub fn mark_offline_chapter(&self, m: &OfflineChapterMark) -> Result<(), StoreError> {
        self.with(|c| {
            c.execute(
                "INSERT OR REPLACE INTO offline_chapters (kind, media_id, ext_id, chapter_id, dir, text_json, pages, bytes, saved_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    m.kind,
                    m.media_id,
                    m.ext_id,
                    m.chapter_id,
                    m.dir,
                    m.text_json,
                    m.pages.map(i64::from),
                    to_i64(m.bytes),
                    m.saved_at
                ],
            )
            .map(|_| ())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::LocalStore;

    #[test]
    fn tables_exist() {
        let s = LocalStore::open_in_memory().unwrap();
        for t in TABLES {
            assert!(s.table_exists(t).unwrap(), "{t} missing");
        }
    }

    fn row(media: &str, ep: u32, state: &str, added: i64) -> DownloadRow {
        DownloadRow {
            media_id: media.into(),
            title: "Show".into(),
            episode: Some(ep),
            kind: "hls".into(),
            state: state.into(),
            url: "https://h/a.m3u8".into(),
            headers: vec![("Referer".into(), "https://h/".into())],
            added_at: added,
            ..DownloadRow::default()
        }
    }

    #[test]
    fn download_roundtrip_and_progress() {
        let s = LocalStore::open_in_memory().unwrap();
        let id = s.insert_download(&row("1", 3, "queued", 10)).unwrap();
        let r = s.download(id).unwrap().unwrap();
        assert_eq!(r.headers, vec![("Referer".to_owned(), "https://h/".to_owned())]);
        assert_eq!(r.episode, Some(3));
        s.update_download_progress(id, 500, Some(1000), 5, Some(10)).unwrap();
        s.set_download_state(id, "downloading", None, Some("/tmp/x.ts"), None).unwrap();
        let r = s.download(id).unwrap().unwrap();
        assert_eq!((r.bytes_done, r.bytes_total, r.segments_done, r.segments_total), (500, Some(1000), 5, Some(10)));
        assert_eq!(r.state, "downloading");
        assert!(r.started_at.is_some());
        assert_eq!(r.path.as_deref(), Some("/tmp/x.ts"));
        // a later progress without totals keeps them
        s.update_download_progress(id, 600, None, 6, None).unwrap();
        let r = s.download(id).unwrap().unwrap();
        assert_eq!((r.bytes_total, r.segments_total), (Some(1000), Some(10)));
    }

    #[test]
    fn active_rows_first_then_history() {
        let s = LocalStore::open_in_memory().unwrap();
        let done = s.insert_download(&row("1", 1, "completed", 1)).unwrap();
        s.set_download_state(done, "completed", None, None, Some(50)).unwrap();
        let q2 = s.insert_download(&row("1", 3, "queued", 30)).unwrap();
        let q1 = s.insert_download(&row("1", 2, "queued", 20)).unwrap();
        let ids: Vec<i64> = s.downloads(true).unwrap().iter().map(|r| r.id).collect();
        assert_eq!(ids, vec![q1, q2, done]);
        assert_eq!(s.downloads_in_state("queued").unwrap().len(), 2);
        assert_eq!(s.completed_download_for("1", 1).unwrap().map(|r| r.id), Some(done));
        assert_eq!(s.clear_download_history().unwrap(), 1);
        assert_eq!(s.downloads(true).unwrap().len(), 2);
    }

    #[test]
    fn local_library_upsert_keeps_match() {
        let s = LocalStore::open_in_memory().unwrap();
        let item = LocalItemRow { path: "/v/Show - 01.mkv".into(), file_name: "Show - 01.mkv".into(), size_bytes: 10, added_at: 5, ..Default::default() };
        let (id, inserted) = s.upsert_local_item(&item).unwrap();
        assert!(inserted);
        s.set_local_match(id, "21", "Show", None, Some(1)).unwrap();
        let (again, inserted) = s.upsert_local_item(&LocalItemRow { size_bytes: 20, ..item }).unwrap();
        assert_eq!((again, inserted), (id, false));
        let r = s.local_item(id).unwrap().unwrap();
        assert_eq!((r.size_bytes, r.media_id.as_deref(), r.episode), (20, Some("21"), Some(1)));
        assert_eq!(s.local_item_for("21", 1).unwrap().map(|r| r.id), Some(id));
        s.delete_local_item(id).unwrap();
        assert!(s.local_items().unwrap().is_empty());
    }

    #[test]
    fn auto_runs_newest_first() {
        let s = LocalStore::open_in_memory().unwrap();
        assert!(s.last_auto_run().unwrap().is_none());
        s.insert_auto_run(&AutoRunRow { ran_at: 1, checked: 2, enqueued: 1, skipped: vec![], ..Default::default() }).unwrap();
        s.insert_auto_run(&AutoRunRow { ran_at: 9, checked: 3, enqueued: 0, skipped: vec!["A: none".into()], ..Default::default() })
            .unwrap();
        let last = s.last_auto_run().unwrap().unwrap();
        assert_eq!((last.ran_at, last.checked, last.skipped.len()), (9, 3, 1));
    }

    #[test]
    fn offline_chapter_mark() {
        let s = LocalStore::open_in_memory().unwrap();
        let m = OfflineChapterMark { kind: "manga".into(), media_id: "1".into(), ext_id: "e".into(), chapter_id: "c".into(), pages: Some(3), saved_at: 1, ..Default::default() };
        s.mark_offline_chapter(&m).unwrap();
        s.mark_offline_chapter(&OfflineChapterMark { bytes: 9, ..m }).unwrap();
    }
}
