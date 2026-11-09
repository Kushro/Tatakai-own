//! `LocalStore` methods over `read_progress`, `chapter_lists`, `offline_chapters`, `read_list_entries` and
//! `read_list_queue` (wave B, worker **l**; worker **m** calls them with `kind = "novel"` and never edits this file;
//! docs/redesign/16 section 3.5). Child module of `store`.
//!
//! - Progress: [`ReadProgressRow`], [`LocalStore::set_read_progress`] / [`LocalStore::read_progress`] /
//!   [`LocalStore::continue_reading`] (newest row per title).
//! - Chapter-list cache: [`LocalStore::chapter_list`] (with a max age) / [`LocalStore::set_chapter_list`] /
//!   [`LocalStore::clear_chapter_lists`].
//! - Offline chapters: [`OfflineChapterRow`], `set_offline_chapter` / `offline_chapter` / `offline_chapters` /
//!   `delete_offline_chapter` / `offline_bytes`.
//! - Read list mirror: [`ReadListRow`], `put_read_list_entry` / `read_list_entry` / `read_list_entries` /
//!   `delete_read_list_entry`; queue `enqueue_read_list_op` / `read_list_queue` / `dequeue_read_list_op` /
//!   `mark_read_list_op_failed` (same shape as worker g's `store/lists.rs` queue).
//! - [`LocalStore::record_chapter_download`]: a finished chapter download as a `downloads` row (kind `manga` /
//!   `novel`, state `completed` / `failed`) so the Downloads page of worker h lists it.

use rusqlite::{OptionalExtension, params};

use crate::error::StoreError;
use crate::store::LocalStore;

/// Tables of this file (the contract's `SCHEMA` creates them; the test below checks they exist).
pub const TABLES: &[&str] = &["read_progress", "chapter_lists", "offline_chapters", "read_list_entries", "read_list_queue"];

/// One row of `read_progress` (`kind` = `manga | novel`; `position` = page or paragraph index).
#[derive(Debug, Clone, PartialEq)]
pub struct ReadProgressRow {
    pub kind: String,
    pub media_id: String,
    pub ext_id: String,
    pub chapter_id: String,
    pub chapter_number: f32,
    pub position: u32,
    pub total: Option<u32>,
    pub completed: bool,
    pub title: Option<String>,
    pub poster: Option<String>,
    pub updated_at: i64,
}

/// One row of `offline_chapters`: `dir` holds the page files (manga), `text_json` the paragraphs (novel).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfflineChapterRow {
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

/// One row of `read_list_entries` (`status` = `ListStatusVm::id()`; `progress` = chapters read).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadListRow {
    pub kind: String,
    pub media_id: String,
    pub anilist_id: Option<u32>,
    pub title: String,
    pub poster: Option<String>,
    pub status: String,
    pub progress: u32,
    pub chapters: Option<u32>,
    pub score: Option<u8>,
    pub notes: Option<String>,
    pub remote_id: Option<u64>,
    pub updated_at: i64,
    pub dirty: bool,
}

/// One queued AniList write of the read list (`op` = `upsert | delete`, `payload` = JSON).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadListQueueRow {
    pub id: i64,
    pub kind: String,
    pub media_id: String,
    pub op: String,
    pub payload: String,
    pub created_at: i64,
    pub attempts: u32,
    pub last_error: Option<String>,
}

/// A finished chapter download written to the `downloads` table of worker h.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChapterDownloadRow {
    /// `manga` | `novel` (`DownloadKindVm::id()`).
    pub kind: String,
    pub media_id: String,
    pub title: String,
    pub poster: Option<String>,
    /// Display label of the chapter (`"12"`, `"12.5"`).
    pub chapter: String,
    /// `<ext_id>/<chapter_id>`: identifies the chapter (the row is never re-executed by the engine).
    pub source_id: String,
    /// `completed` | `failed`.
    pub state: String,
    pub path: Option<String>,
    pub bytes: u64,
    pub error: Option<String>,
    pub added_at: i64,
    pub completed_at: Option<i64>,
}

fn opt_u32(v: Option<i64>) -> Option<u32> {
    v.and_then(|n| u32::try_from(n).ok())
}

fn progress_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ReadProgressRow> {
    Ok(ReadProgressRow {
        kind: r.get(0)?,
        media_id: r.get(1)?,
        ext_id: r.get(2)?,
        chapter_id: r.get(3)?,
        chapter_number: r.get::<_, f64>(4)? as f32,
        position: opt_u32(r.get(5)?).unwrap_or(0),
        total: opt_u32(r.get(6)?),
        completed: r.get::<_, i64>(7)? != 0,
        title: r.get(8)?,
        poster: r.get(9)?,
        updated_at: r.get(10)?,
    })
}

const PROGRESS_COLS: &str =
    "kind, media_id, ext_id, chapter_id, chapter_number, position, total, completed, title, poster, updated_at";

fn offline_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<OfflineChapterRow> {
    Ok(OfflineChapterRow {
        kind: r.get(0)?,
        media_id: r.get(1)?,
        ext_id: r.get(2)?,
        chapter_id: r.get(3)?,
        dir: r.get(4)?,
        text_json: r.get(5)?,
        pages: opt_u32(r.get(6)?),
        bytes: u64::try_from(r.get::<_, i64>(7)?).unwrap_or(0),
        saved_at: r.get(8)?,
    })
}

const OFFLINE_COLS: &str = "kind, media_id, ext_id, chapter_id, dir, text_json, pages, bytes, saved_at";

fn list_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ReadListRow> {
    Ok(ReadListRow {
        kind: r.get(0)?,
        media_id: r.get(1)?,
        anilist_id: opt_u32(r.get(2)?),
        title: r.get(3)?,
        poster: r.get(4)?,
        status: r.get(5)?,
        progress: opt_u32(r.get(6)?).unwrap_or(0),
        chapters: opt_u32(r.get(7)?),
        score: r.get::<_, Option<i64>>(8)?.and_then(|s| u8::try_from(s).ok()),
        notes: r.get(9)?,
        remote_id: r.get::<_, Option<i64>>(10)?.and_then(|v| u64::try_from(v).ok()),
        updated_at: r.get(11)?,
        dirty: r.get::<_, i64>(12)? != 0,
    })
}

const LIST_COLS: &str =
    "kind, media_id, anilist_id, title, poster, status, progress, chapters, score, notes, remote_id, updated_at, dirty";

fn queue_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ReadListQueueRow> {
    Ok(ReadListQueueRow {
        id: r.get(0)?,
        kind: r.get(1)?,
        media_id: r.get(2)?,
        op: r.get(3)?,
        payload: r.get(4)?,
        created_at: r.get(5)?,
        attempts: opt_u32(r.get(6)?).unwrap_or(0),
        last_error: r.get(7)?,
    })
}

impl LocalStore {
    // ---- progress --------------------------------------------------------------------------------

    /// Upsert the position of one chapter. A completed chapter stays completed when an older position comes in.
    pub fn set_read_progress(&self, row: &ReadProgressRow) -> Result<(), StoreError> {
        self.with(|c| {
            c.execute(
                "INSERT INTO read_progress (kind, media_id, ext_id, chapter_id, chapter_number, position, total, completed, title, poster, updated_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
                 ON CONFLICT(kind, media_id, ext_id, chapter_id) DO UPDATE SET chapter_number=?5, position=?6,
                   total=COALESCE(?7, total), completed=MAX(completed, ?8), title=COALESCE(?9, title),
                   poster=COALESCE(?10, poster), updated_at=?11",
                params![
                    row.kind,
                    row.media_id,
                    row.ext_id,
                    row.chapter_id,
                    f64::from(row.chapter_number),
                    row.position,
                    row.total,
                    i64::from(row.completed),
                    row.title,
                    row.poster,
                    row.updated_at
                ],
            )?;
            Ok(())
        })
    }

    /// Every chapter position of a title, newest first.
    pub fn read_progress(&self, kind: &str, media_id: &str) -> Result<Vec<ReadProgressRow>, StoreError> {
        self.with(|c| {
            let mut st = c.prepare(&format!(
                "SELECT {PROGRESS_COLS} FROM read_progress WHERE kind = ?1 AND media_id = ?2 ORDER BY updated_at DESC, chapter_number DESC"
            ))?;
            st.query_map(params![kind, media_id], progress_row)?.collect()
        })
    }

    /// Newest position per title (Continue reading), newest title first; titles whose newest chapter is completed
    /// still show (the next chapter is the way on).
    pub fn continue_reading(&self, kind: &str, limit: u32) -> Result<Vec<ReadProgressRow>, StoreError> {
        self.with(|c| {
            let mut st = c.prepare(&format!(
                "SELECT {PROGRESS_COLS} FROM read_progress p WHERE kind = ?1 AND updated_at = (
                   SELECT MAX(updated_at) FROM read_progress q WHERE q.kind = p.kind AND q.media_id = p.media_id)
                 GROUP BY media_id ORDER BY updated_at DESC LIMIT ?2"
            ))?;
            st.query_map(params![kind, limit], progress_row)?.collect()
        })
    }

    /// Drop the reading positions of a title.
    pub fn clear_read_progress(&self, kind: &str, media_id: &str) -> Result<usize, StoreError> {
        self.with(|c| c.execute("DELETE FROM read_progress WHERE kind = ?1 AND media_id = ?2", params![kind, media_id]))
    }

    // ---- chapter-list cache ----------------------------------------------------------------------

    /// Cached chapters JSON of one provider when younger than `max_age_secs` at `now`: `(json, fetched_at)`.
    pub fn chapter_list(&self, kind: &str, media_id: &str, ext_id: &str, max_age_secs: i64, now: i64) -> Result<Option<(String, i64)>, StoreError> {
        self.with(|c| {
            c.query_row(
                "SELECT chapters_json, fetched_at FROM chapter_lists WHERE kind = ?1 AND media_id = ?2 AND ext_id = ?3 AND fetched_at > ?4",
                params![kind, media_id, ext_id, now - max_age_secs],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
        })
    }

    /// Every cached list of a title regardless of age: `(ext_id, json, fetched_at)`.
    pub fn chapter_lists(&self, kind: &str, media_id: &str) -> Result<Vec<(String, String, i64)>, StoreError> {
        self.with(|c| {
            let mut st = c.prepare("SELECT ext_id, chapters_json, fetched_at FROM chapter_lists WHERE kind = ?1 AND media_id = ?2 ORDER BY ext_id")?;
            st.query_map(params![kind, media_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect()
        })
    }

    pub fn set_chapter_list(&self, kind: &str, media_id: &str, ext_id: &str, json: &str, now: i64) -> Result<(), StoreError> {
        self.with(|c| {
            c.execute(
                "INSERT INTO chapter_lists (kind, media_id, ext_id, chapters_json, fetched_at) VALUES (?1,?2,?3,?4,?5)
                 ON CONFLICT(kind, media_id, ext_id) DO UPDATE SET chapters_json=?4, fetched_at=?5",
                params![kind, media_id, ext_id, json, now],
            )?;
            Ok(())
        })
    }

    pub fn clear_chapter_lists(&self, kind: &str, media_id: &str) -> Result<usize, StoreError> {
        self.with(|c| c.execute("DELETE FROM chapter_lists WHERE kind = ?1 AND media_id = ?2", params![kind, media_id]))
    }

    // ---- offline chapters ------------------------------------------------------------------------

    pub fn set_offline_chapter(&self, row: &OfflineChapterRow) -> Result<(), StoreError> {
        let bytes = i64::try_from(row.bytes).unwrap_or(i64::MAX);
        self.with(|c| {
            c.execute(
                "INSERT INTO offline_chapters (kind, media_id, ext_id, chapter_id, dir, text_json, pages, bytes, saved_at)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
                 ON CONFLICT(kind, media_id, ext_id, chapter_id) DO UPDATE SET dir=?5, text_json=?6, pages=?7, bytes=?8, saved_at=?9",
                params![row.kind, row.media_id, row.ext_id, row.chapter_id, row.dir, row.text_json, row.pages, bytes, row.saved_at],
            )?;
            Ok(())
        })
    }

    /// The offline copy of one chapter; `media_id = None` matches any title (the reader knows only the chapter).
    pub fn offline_chapter(&self, kind: &str, media_id: Option<&str>, ext_id: &str, chapter_id: &str) -> Result<Option<OfflineChapterRow>, StoreError> {
        self.with(|c| {
            c.query_row(
                &format!(
                    "SELECT {OFFLINE_COLS} FROM offline_chapters WHERE kind = ?1 AND (?2 IS NULL OR media_id = ?2) AND ext_id = ?3 AND chapter_id = ?4
                     ORDER BY saved_at DESC LIMIT 1"
                ),
                params![kind, media_id, ext_id, chapter_id],
                offline_row,
            )
            .optional()
        })
    }

    pub fn offline_chapters(&self, kind: &str, media_id: &str) -> Result<Vec<OfflineChapterRow>, StoreError> {
        self.with(|c| {
            let mut st = c.prepare(&format!("SELECT {OFFLINE_COLS} FROM offline_chapters WHERE kind = ?1 AND media_id = ?2 ORDER BY saved_at"))?;
            st.query_map(params![kind, media_id], offline_row)?.collect()
        })
    }

    /// Remove the row of one chapter (any title); returns the removed rows so the caller deletes their files.
    pub fn delete_offline_chapter(&self, kind: &str, ext_id: &str, chapter_id: &str) -> Result<Vec<OfflineChapterRow>, StoreError> {
        self.with(|c| {
            let rows: Vec<OfflineChapterRow> = {
                let mut st = c.prepare(&format!("SELECT {OFFLINE_COLS} FROM offline_chapters WHERE kind = ?1 AND ext_id = ?2 AND chapter_id = ?3"))?;
                st.query_map(params![kind, ext_id, chapter_id], offline_row)?.collect::<rusqlite::Result<_>>()?
            };
            c.execute("DELETE FROM offline_chapters WHERE kind = ?1 AND ext_id = ?2 AND chapter_id = ?3", params![kind, ext_id, chapter_id])?;
            Ok(rows)
        })
    }

    /// Bytes stored offline for a kind.
    pub fn offline_bytes(&self, kind: &str) -> Result<u64, StoreError> {
        self.with(|c| {
            c.query_row("SELECT COALESCE(SUM(bytes), 0) FROM offline_chapters WHERE kind = ?1", [kind], |r| r.get::<_, i64>(0))
                .map(|n| u64::try_from(n).unwrap_or(0))
        })
    }

    // ---- read list mirror ------------------------------------------------------------------------

    pub fn put_read_list_entry(&self, row: &ReadListRow) -> Result<(), StoreError> {
        let remote_id = row.remote_id.and_then(|v| i64::try_from(v).ok());
        self.with(|c| {
            c.execute(
                "INSERT INTO read_list_entries (kind, media_id, anilist_id, title, poster, status, progress, chapters, score, notes, remote_id, updated_at, dirty)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
                 ON CONFLICT(kind, media_id) DO UPDATE SET anilist_id=?3, title=?4, poster=?5, status=?6, progress=?7,
                   chapters=?8, score=?9, notes=?10, remote_id=?11, updated_at=?12, dirty=?13",
                params![
                    row.kind,
                    row.media_id,
                    row.anilist_id,
                    row.title,
                    row.poster,
                    row.status,
                    row.progress,
                    row.chapters,
                    row.score,
                    row.notes,
                    remote_id,
                    row.updated_at,
                    i64::from(row.dirty)
                ],
            )?;
            Ok(())
        })
    }

    pub fn read_list_entry(&self, kind: &str, media_id: &str) -> Result<Option<ReadListRow>, StoreError> {
        self.with(|c| {
            c.query_row(&format!("SELECT {LIST_COLS} FROM read_list_entries WHERE kind = ?1 AND media_id = ?2"), params![kind, media_id], list_row)
                .optional()
        })
    }

    /// Entries of a kind, newest update first; `status: None` = all.
    pub fn read_list_entries(&self, kind: &str, status: Option<&str>) -> Result<Vec<ReadListRow>, StoreError> {
        self.with(|c| {
            let mut st = c.prepare(&format!(
                "SELECT {LIST_COLS} FROM read_list_entries WHERE kind = ?1 AND (?2 IS NULL OR status = ?2) ORDER BY updated_at DESC"
            ))?;
            st.query_map(params![kind, status], list_row)?.collect()
        })
    }

    pub fn delete_read_list_entry(&self, kind: &str, media_id: &str) -> Result<(), StoreError> {
        self.with(|c| {
            c.execute("DELETE FROM read_list_entries WHERE kind = ?1 AND media_id = ?2", params![kind, media_id])?;
            Ok(())
        })
    }

    /// Replace the clean (pushed) rows of a kind with the remote list; dirty rows are kept (their op is queued).
    pub fn replace_read_list(&self, kind: &str, rows: &[ReadListRow]) -> Result<(), StoreError> {
        let dirty: Vec<String> = self.read_list_entries(kind, None)?.into_iter().filter(|r| r.dirty).map(|r| r.media_id).collect();
        self.with(|c| {
            c.execute("DELETE FROM read_list_entries WHERE kind = ?1 AND dirty = 0", [kind])?;
            Ok(())
        })?;
        for r in rows.iter().filter(|r| !dirty.contains(&r.media_id)) {
            self.put_read_list_entry(r)?;
        }
        Ok(())
    }

    // ---- read list queue -------------------------------------------------------------------------

    /// Queue one write; an earlier queued op of the same title is replaced (only the newest state matters).
    pub fn enqueue_read_list_op(&self, kind: &str, media_id: &str, op: &str, payload: &str, now: i64) -> Result<i64, StoreError> {
        self.with(|c| {
            c.execute("DELETE FROM read_list_queue WHERE kind = ?1 AND media_id = ?2", params![kind, media_id])?;
            c.execute(
                "INSERT INTO read_list_queue (kind, media_id, op, payload, created_at) VALUES (?1,?2,?3,?4,?5)",
                params![kind, media_id, op, payload, now],
            )?;
            Ok(c.last_insert_rowid())
        })
    }

    /// Queued ops, oldest first; `kind: None` = both kinds.
    pub fn read_list_queue(&self, kind: Option<&str>) -> Result<Vec<ReadListQueueRow>, StoreError> {
        self.with(|c| {
            let mut st = c.prepare(
                "SELECT id, kind, media_id, op, payload, created_at, attempts, last_error FROM read_list_queue
                 WHERE (?1 IS NULL OR kind = ?1) ORDER BY id",
            )?;
            st.query_map(params![kind], queue_row)?.collect()
        })
    }

    pub fn dequeue_read_list_op(&self, id: i64) -> Result<(), StoreError> {
        self.with(|c| {
            c.execute("DELETE FROM read_list_queue WHERE id = ?1", [id])?;
            Ok(())
        })
    }

    pub fn mark_read_list_op_failed(&self, id: i64, error: &str) -> Result<(), StoreError> {
        self.with(|c| {
            c.execute("UPDATE read_list_queue SET attempts = attempts + 1, last_error = ?2 WHERE id = ?1", params![id, error])?;
            Ok(())
        })
    }

    // ---- downloads table (worker h's) --------------------------------------------------------------

    /// Insert (or update, by `kind` + `source_id`) the `downloads` row of a finished chapter download.
    pub fn record_chapter_download(&self, row: &ChapterDownloadRow) -> Result<i64, StoreError> {
        let bytes = i64::try_from(row.bytes).unwrap_or(i64::MAX);
        let url = format!("{}:{}", row.kind, row.source_id);
        self.with(|c| {
            let existing: Option<i64> = c
                .query_row("SELECT id FROM downloads WHERE kind = ?1 AND source_id = ?2", params![row.kind, row.source_id], |r| r.get(0))
                .optional()?;
            match existing {
                Some(id) => {
                    c.execute(
                        "UPDATE downloads SET state=?2, path=?3, bytes_done=?4, bytes_total=?4, error=?5, completed_at=?6, title=?7, poster=?8 WHERE id=?1",
                        params![id, row.state, row.path, bytes, row.error, row.completed_at, row.title, row.poster],
                    )?;
                    Ok(id)
                }
                None => {
                    c.execute(
                        "INSERT INTO downloads (media_id, title, poster, chapter, kind, state, url, source_id, path, bytes_done, bytes_total, error, added_at, started_at, completed_at)
                         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?10,?11,?12,?12,?13)",
                        params![
                            row.media_id,
                            row.title,
                            row.poster,
                            row.chapter,
                            row.kind,
                            row.state,
                            url,
                            row.source_id,
                            row.path,
                            bytes,
                            row.error,
                            row.added_at,
                            row.completed_at
                        ],
                    )?;
                    Ok(c.last_insert_rowid())
                }
            }
        })
    }

    /// Drop the `downloads` row of a chapter (its offline copy was removed).
    pub fn delete_chapter_download(&self, kind: &str, source_id: &str) -> Result<(), StoreError> {
        self.with(|c| {
            c.execute("DELETE FROM downloads WHERE kind = ?1 AND source_id = ?2", params![kind, source_id])?;
            Ok(())
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

    fn progress(media: &str, chapter: &str, number: f32, position: u32, at: i64) -> ReadProgressRow {
        ReadProgressRow {
            kind: "manga".into(),
            media_id: media.into(),
            ext_id: "ext".into(),
            chapter_id: chapter.into(),
            chapter_number: number,
            position,
            total: Some(20),
            completed: false,
            title: Some(format!("T{media}")),
            poster: None,
            updated_at: at,
        }
    }

    #[test]
    fn progress_upserts_and_continue_reading_takes_the_newest_per_title() {
        let s = LocalStore::open_in_memory().unwrap();
        s.set_read_progress(&progress("1", "c1", 1.0, 3, 10)).unwrap();
        s.set_read_progress(&progress("1", "c2", 2.0, 5, 20)).unwrap();
        s.set_read_progress(&progress("2", "x1", 1.0, 1, 15)).unwrap();
        let mut done = progress("1", "c1", 1.0, 19, 5);
        done.completed = true;
        s.set_read_progress(&done).unwrap();
        // a later, incomplete write never un-completes
        s.set_read_progress(&progress("1", "c1", 1.0, 2, 6)).unwrap();
        let rows = s.read_progress("manga", "1").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].chapter_id, "c2");
        assert!(rows.iter().find(|r| r.chapter_id == "c1").unwrap().completed);
        let cont = s.continue_reading("manga", 10).unwrap();
        assert_eq!(cont.iter().map(|r| r.media_id.as_str()).collect::<Vec<_>>(), vec!["1", "2"]);
        assert_eq!(cont[0].chapter_id, "c2");
        assert!(s.continue_reading("novel", 10).unwrap().is_empty());
        assert_eq!(s.clear_read_progress("manga", "1").unwrap(), 2);
    }

    #[test]
    fn chapter_list_cache_respects_age() {
        let s = LocalStore::open_in_memory().unwrap();
        s.set_chapter_list("manga", "1", "ext", "[]", 1000).unwrap();
        assert_eq!(s.chapter_list("manga", "1", "ext", 100, 1050).unwrap(), Some(("[]".into(), 1000)));
        assert_eq!(s.chapter_list("manga", "1", "ext", 100, 1200).unwrap(), None);
        assert_eq!(s.chapter_lists("manga", "1").unwrap().len(), 1);
        assert_eq!(s.clear_chapter_lists("manga", "1").unwrap(), 1);
    }

    #[test]
    fn offline_chapters_roundtrip() {
        let s = LocalStore::open_in_memory().unwrap();
        let row = OfflineChapterRow {
            kind: "manga".into(),
            media_id: "1".into(),
            ext_id: "ext".into(),
            chapter_id: "c1".into(),
            dir: Some("/tmp/x".into()),
            text_json: None,
            pages: Some(12),
            bytes: 4096,
            saved_at: 7,
        };
        s.set_offline_chapter(&row).unwrap();
        assert_eq!(s.offline_chapter("manga", None, "ext", "c1").unwrap(), Some(row.clone()));
        assert_eq!(s.offline_chapter("manga", Some("2"), "ext", "c1").unwrap(), None);
        assert_eq!(s.offline_chapters("manga", "1").unwrap().len(), 1);
        assert_eq!(s.offline_bytes("manga").unwrap(), 4096);
        assert_eq!(s.delete_offline_chapter("manga", "ext", "c1").unwrap(), vec![row]);
        assert_eq!(s.offline_bytes("manga").unwrap(), 0);
    }

    fn entry(media: &str, status: &str, at: i64, dirty: bool) -> ReadListRow {
        ReadListRow {
            kind: "manga".into(),
            media_id: media.into(),
            anilist_id: media.parse().ok(),
            title: "T".into(),
            poster: None,
            status: status.into(),
            progress: 3,
            chapters: Some(100),
            score: Some(80),
            notes: None,
            remote_id: Some(9),
            updated_at: at,
            dirty,
        }
    }

    #[test]
    fn read_list_mirror_and_replace_keep_dirty_rows() {
        let s = LocalStore::open_in_memory().unwrap();
        s.put_read_list_entry(&entry("1", "watching", 1, false)).unwrap();
        s.put_read_list_entry(&entry("2", "planning", 2, true)).unwrap();
        assert_eq!(s.read_list_entries("manga", Some("watching")).unwrap().len(), 1);
        assert_eq!(s.read_list_entries("manga", None).unwrap()[0].media_id, "2");
        s.replace_read_list("manga", &[entry("3", "completed", 5, false), entry("2", "dropped", 6, false)]).unwrap();
        let all = s.read_list_entries("manga", None).unwrap();
        assert_eq!(all.len(), 2);
        assert!(s.read_list_entry("manga", "1").unwrap().is_none());
        assert_eq!(s.read_list_entry("manga", "2").unwrap().unwrap().status, "planning");
        s.delete_read_list_entry("manga", "3").unwrap();
        assert!(s.read_list_entry("manga", "3").unwrap().is_none());
    }

    #[test]
    fn queue_keeps_only_the_newest_op_per_title() {
        let s = LocalStore::open_in_memory().unwrap();
        s.enqueue_read_list_op("manga", "1", "upsert", "{}", 1).unwrap();
        let id = s.enqueue_read_list_op("manga", "1", "delete", "{\"x\":1}", 2).unwrap();
        s.enqueue_read_list_op("novel", "9", "upsert", "{}", 3).unwrap();
        let q = s.read_list_queue(Some("manga")).unwrap();
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].op, "delete");
        s.mark_read_list_op_failed(id, "offline").unwrap();
        assert_eq!(s.read_list_queue(Some("manga")).unwrap()[0].attempts, 1);
        s.dequeue_read_list_op(id).unwrap();
        assert_eq!(s.read_list_queue(None).unwrap().len(), 1);
    }

    #[test]
    fn chapter_download_rows_upsert() {
        let s = LocalStore::open_in_memory().unwrap();
        let mut row = ChapterDownloadRow {
            kind: "manga".into(),
            media_id: "1".into(),
            title: "T".into(),
            poster: None,
            chapter: "12".into(),
            source_id: "ext/c12".into(),
            state: "failed".into(),
            path: None,
            bytes: 0,
            error: Some("x".into()),
            added_at: 1,
            completed_at: None,
        };
        let a = s.record_chapter_download(&row).unwrap();
        row.state = "completed".into();
        row.bytes = 10;
        let b = s.record_chapter_download(&row).unwrap();
        assert_eq!(a, b);
        s.delete_chapter_download("manga", "ext/c12").unwrap();
    }
}
