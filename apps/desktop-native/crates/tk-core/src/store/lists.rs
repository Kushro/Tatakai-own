//! `LocalStore` methods over `list_entries`, `list_queue`, `list_conflicts` and `favorites` (wave A, worker **g**;
//! docs/redesign/15-parity-wave-a-contract.md section 3). Child module of `store`: it uses the private `with`
//! helper of `mod.rs`. Contract: the row types below are the final shape tk-app maps to the UI view-models; the
//! method bodies are stubs that g implements (plus tests on `LocalStore::open_in_memory`).

use rusqlite::{OptionalExtension, params};

use crate::error::StoreError;
use crate::store::LocalStore;

/// One row of `list_entries` (`status` is `ListStatusVm::id()`: `watching | planning | completed | paused | dropped |
/// repeating`; `score` is 0..=100 or `None`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListEntryRow {
    pub service: String,
    pub media_id: String,
    pub anilist_id: Option<u32>,
    pub mal_id: Option<u32>,
    pub title: String,
    pub poster: Option<String>,
    pub status: String,
    pub progress: u32,
    pub episodes: Option<u32>,
    pub score: Option<u8>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub repeats: u32,
    pub notes: Option<String>,
    pub remote_id: Option<u64>,
    pub updated_at: i64,
    pub dirty: bool,
}

/// One queued write (`op` = `upsert | delete | favorite | unfavorite`, `payload` = JSON of the fields).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListQueueRow {
    pub id: i64,
    pub service: String,
    pub media_id: String,
    pub op: String,
    pub payload: String,
    pub created_at: i64,
    pub attempts: u32,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListConflictRow {
    pub id: i64,
    pub service: String,
    pub media_id: String,
    pub local_json: String,
    pub remote_json: String,
    pub detected_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FavoriteRow {
    pub media_id: String,
    pub title: String,
    pub poster: Option<String>,
    pub added_at: i64,
}

impl LocalStore {
    /// Upsert a mirror row (`dirty` as given).
    pub fn put_list_entry(&self, row: &ListEntryRow) -> Result<(), StoreError> {
        let remote_id = row.remote_id.and_then(|v| i64::try_from(v).ok());
        self.with(|c| {
            c.execute(
                "INSERT INTO list_entries (service, media_id, anilist_id, mal_id, title, poster, status, progress, episodes, score,
                   started_at, finished_at, repeats, notes, remote_id, updated_at, dirty)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)
                 ON CONFLICT(service, media_id) DO UPDATE SET anilist_id=?3, mal_id=?4, title=?5, poster=?6, status=?7,
                   progress=?8, episodes=?9, score=?10, started_at=?11, finished_at=?12, repeats=?13, notes=?14,
                   remote_id=?15, updated_at=?16, dirty=?17",
                params![
                    row.service, row.media_id, row.anilist_id, row.mal_id, row.title, row.poster, row.status, row.progress,
                    row.episodes, row.score, row.started_at, row.finished_at, row.repeats, row.notes, remote_id,
                    row.updated_at, i64::from(row.dirty)
                ],
            )?;
            Ok(())
        })
    }

    pub fn list_entry(&self, service: &str, media_id: &str) -> Result<Option<ListEntryRow>, StoreError> {
        self.with(|c| {
            c.query_row(
                "SELECT * FROM list_entries WHERE service = ?1 AND media_id = ?2",
                params![service, media_id],
                entry_row,
            )
            .optional()
        })
    }

    /// Newest `updated_at` first; `status: None` = all.
    pub fn list_entries(&self, service: &str, status: Option<&str>) -> Result<Vec<ListEntryRow>, StoreError> {
        self.with(|c| match status {
            Some(st) => {
                let mut q = c.prepare(
                    "SELECT * FROM list_entries WHERE service = ?1 AND status = ?2 ORDER BY updated_at DESC, media_id ASC",
                )?;
                q.query_map(params![service, st], entry_row)?.collect()
            }
            None => {
                let mut q = c.prepare("SELECT * FROM list_entries WHERE service = ?1 ORDER BY updated_at DESC, media_id ASC")?;
                q.query_map(params![service], entry_row)?.collect()
            }
        })
    }

    pub fn delete_list_entry(&self, service: &str, media_id: &str) -> Result<(), StoreError> {
        self.with(|c| {
            c.execute(
                "DELETE FROM list_entries WHERE service = ?1 AND media_id = ?2",
                params![service, media_id],
            )?;
            Ok(())
        })
    }

    /// Every row of one service (disconnect); returns how many were removed.
    pub fn delete_list_entries(&self, service: &str) -> Result<usize, StoreError> {
        self.with(|c| c.execute("DELETE FROM list_entries WHERE service = ?1", params![service]))
    }

    /// Rows of `service` changed locally and not pushed yet.
    pub fn dirty_list_entries(&self, service: &str) -> Result<u32, StoreError> {
        self.with(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM list_entries WHERE service = ?1 AND dirty = 1",
                params![service],
                |r| r.get(0),
            )
        })
    }

    /// Append a write to the offline queue; returns its id.
    pub fn enqueue_list_op(
        &self,
        service: &str,
        media_id: &str,
        op: &str,
        payload: &str,
        now: i64,
    ) -> Result<i64, StoreError> {
        self.with(|c| {
            c.execute(
                "INSERT INTO list_queue (service, media_id, op, payload, created_at, attempts) VALUES (?1,?2,?3,?4,?5,0)",
                params![service, media_id, op, payload, now],
            )?;
            Ok(c.last_insert_rowid())
        })
    }

    /// Oldest first.
    pub fn list_queue(&self, service: Option<&str>) -> Result<Vec<ListQueueRow>, StoreError> {
        self.with(|c| match service {
            Some(s) => {
                let mut q = c.prepare("SELECT * FROM list_queue WHERE service = ?1 ORDER BY id ASC")?;
                q.query_map(params![s], queue_row)?.collect()
            }
            None => {
                let mut q = c.prepare("SELECT * FROM list_queue ORDER BY id ASC")?;
                q.query_map([], queue_row)?.collect()
            }
        })
    }

    /// Queued writes of one title on one service.
    pub fn has_list_ops(&self, service: &str, media_id: &str) -> Result<bool, StoreError> {
        self.with(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM list_queue WHERE service = ?1 AND media_id = ?2",
                params![service, media_id],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n > 0)
        })
    }

    pub fn count_list_queue(&self, service: &str) -> Result<u32, StoreError> {
        self.with(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM list_queue WHERE service = ?1",
                params![service],
                |r| r.get(0),
            )
        })
    }

    pub fn dequeue_list_op(&self, id: i64) -> Result<(), StoreError> {
        self.with(|c| {
            c.execute("DELETE FROM list_queue WHERE id = ?1", params![id])?;
            Ok(())
        })
    }

    /// Drop queued `ops` of one title (a newer write replaces them: the push always sends the current row).
    pub fn dequeue_list_ops_for(
        &self,
        service: &str,
        media_id: &str,
        ops: &[&str],
    ) -> Result<usize, StoreError> {
        self.with(|c| {
            let mut n = 0;
            for op in ops {
                n += c.execute(
                    "DELETE FROM list_queue WHERE service = ?1 AND media_id = ?2 AND op = ?3",
                    params![service, media_id, op],
                )?;
            }
            Ok(n)
        })
    }

    /// Every queued write of one service (disconnect).
    pub fn clear_list_queue(&self, service: &str) -> Result<usize, StoreError> {
        self.with(|c| c.execute("DELETE FROM list_queue WHERE service = ?1", params![service]))
    }

    pub fn mark_list_op_failed(&self, id: i64, error: &str) -> Result<(), StoreError> {
        self.with(|c| {
            c.execute(
                "UPDATE list_queue SET attempts = attempts + 1, last_error = ?2 WHERE id = ?1",
                params![id, error],
            )?;
            Ok(())
        })
    }

    pub fn add_list_conflict(&self, row: &ListConflictRow) -> Result<i64, StoreError> {
        self.with(|c| {
            c.execute(
                "INSERT INTO list_conflicts (service, media_id, local_json, remote_json, detected_at) VALUES (?1,?2,?3,?4,?5)",
                params![row.service, row.media_id, row.local_json, row.remote_json, row.detected_at],
            )?;
            Ok(c.last_insert_rowid())
        })
    }

    /// Oldest first.
    pub fn list_conflicts(&self) -> Result<Vec<ListConflictRow>, StoreError> {
        self.with(|c| {
            let mut q = c.prepare("SELECT * FROM list_conflicts ORDER BY id ASC")?;
            q.query_map([], conflict_row)?.collect()
        })
    }

    pub fn list_conflict(&self, id: i64) -> Result<Option<ListConflictRow>, StoreError> {
        self.with(|c| {
            c.query_row(
                "SELECT * FROM list_conflicts WHERE id = ?1",
                params![id],
                conflict_row,
            )
            .optional()
        })
    }

    pub fn has_list_conflict(&self, service: &str, media_id: &str) -> Result<bool, StoreError> {
        self.with(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM list_conflicts WHERE service = ?1 AND media_id = ?2",
                params![service, media_id],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n > 0)
        })
    }

    pub fn delete_list_conflict(&self, id: i64) -> Result<(), StoreError> {
        self.with(|c| {
            c.execute("DELETE FROM list_conflicts WHERE id = ?1", params![id])?;
            Ok(())
        })
    }

    /// Conflicts of one title (`service: None` = on every service).
    pub fn delete_list_conflicts_for(
        &self,
        service: Option<&str>,
        media_id: &str,
    ) -> Result<usize, StoreError> {
        self.with(|c| match service {
            Some(s) => c.execute(
                "DELETE FROM list_conflicts WHERE service = ?1 AND media_id = ?2",
                params![s, media_id],
            ),
            None => c.execute(
                "DELETE FROM list_conflicts WHERE media_id = ?1",
                params![media_id],
            ),
        })
    }

    pub fn clear_list_conflicts(&self, service: &str) -> Result<usize, StoreError> {
        self.with(|c| c.execute("DELETE FROM list_conflicts WHERE service = ?1", params![service]))
    }

    /// Insert or refresh a favourite (keeps the first `added_at`).
    pub fn set_favorite(&self, row: &FavoriteRow) -> Result<(), StoreError> {
        self.with(|c| {
            c.execute(
                "INSERT INTO favorites (media_id, title, poster, added_at) VALUES (?1,?2,?3,?4)
                 ON CONFLICT(media_id) DO UPDATE SET title = ?2, poster = COALESCE(?3, poster)",
                params![row.media_id, row.title, row.poster, row.added_at],
            )?;
            Ok(())
        })
    }

    pub fn remove_favorite(&self, media_id: &str) -> Result<(), StoreError> {
        self.with(|c| {
            c.execute("DELETE FROM favorites WHERE media_id = ?1", params![media_id])?;
            Ok(())
        })
    }

    pub fn is_favorite(&self, media_id: &str) -> Result<bool, StoreError> {
        self.with(|c| {
            c.query_row(
                "SELECT COUNT(*) FROM favorites WHERE media_id = ?1",
                params![media_id],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n > 0)
        })
    }

    /// Newest first (titles and `character:<id>` rows).
    pub fn favorites(&self) -> Result<Vec<FavoriteRow>, StoreError> {
        self.with(|c| {
            let mut q = c.prepare("SELECT * FROM favorites ORDER BY added_at DESC, media_id ASC")?;
            q.query_map([], |r| {
                Ok(FavoriteRow {
                    media_id: r.get("media_id")?,
                    title: r.get("title")?,
                    poster: r.get("poster")?,
                    added_at: r.get("added_at")?,
                })
            })?
            .collect()
        })
    }
}

fn entry_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ListEntryRow> {
    let remote: Option<i64> = r.get("remote_id")?;
    Ok(ListEntryRow {
        service: r.get("service")?,
        media_id: r.get("media_id")?,
        anilist_id: r.get("anilist_id")?,
        mal_id: r.get("mal_id")?,
        title: r.get("title")?,
        poster: r.get("poster")?,
        status: r.get("status")?,
        progress: r.get("progress")?,
        episodes: r.get("episodes")?,
        score: r.get("score")?,
        started_at: r.get("started_at")?,
        finished_at: r.get("finished_at")?,
        repeats: r.get("repeats")?,
        notes: r.get("notes")?,
        remote_id: remote.and_then(|v| u64::try_from(v).ok()),
        updated_at: r.get("updated_at")?,
        dirty: r.get::<_, i64>("dirty")? != 0,
    })
}

fn queue_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ListQueueRow> {
    Ok(ListQueueRow {
        id: r.get("id")?,
        service: r.get("service")?,
        media_id: r.get("media_id")?,
        op: r.get("op")?,
        payload: r.get("payload")?,
        created_at: r.get("created_at")?,
        attempts: r.get("attempts")?,
        last_error: r.get("last_error")?,
    })
}

fn conflict_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ListConflictRow> {
    Ok(ListConflictRow {
        id: r.get("id")?,
        service: r.get("service")?,
        media_id: r.get("media_id")?,
        local_json: r.get("local_json")?,
        remote_json: r.get("remote_json")?,
        detected_at: r.get("detected_at")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lists::sync::row;

    #[test]
    fn entries_upsert_filter_and_order() {
        let s = LocalStore::open_in_memory().unwrap();
        let mut a = row("local", "21", "watching", 3, 10);
        a.remote_id = Some(77);
        a.score = Some(85);
        a.dirty = true;
        s.put_list_entry(&a).unwrap();
        s.put_list_entry(&row("local", "1", "completed", 12, 20)).unwrap();
        s.put_list_entry(&row("anilist", "21", "watching", 2, 5)).unwrap();
        assert_eq!(s.list_entry("local", "21").unwrap(), Some(a.clone()));
        assert_eq!(
            s.list_entries("local", None)
                .unwrap()
                .iter()
                .map(|r| r.media_id.as_str())
                .collect::<Vec<_>>(),
            ["1", "21"]
        );
        assert_eq!(s.list_entries("local", Some("watching")).unwrap().len(), 1);
        assert_eq!(s.dirty_list_entries("local").unwrap(), 1);
        a.progress = 4;
        a.dirty = false;
        s.put_list_entry(&a).unwrap();
        assert_eq!(s.list_entry("local", "21").unwrap().unwrap().progress, 4);
        s.delete_list_entry("local", "21").unwrap();
        assert_eq!(s.list_entry("local", "21").unwrap(), None);
        assert_eq!(
            s.list_entry("anilist", "21").unwrap().unwrap().progress,
            2,
            "per service"
        );
        assert_eq!(s.delete_list_entries("anilist").unwrap(), 1);
    }

    #[test]
    fn queue_is_fifo_and_tracks_failures() {
        let s = LocalStore::open_in_memory().unwrap();
        let a = s.enqueue_list_op("anilist", "21", "upsert", "{}", 1).unwrap();
        let b = s.enqueue_list_op("mal", "21", "upsert", "{}", 2).unwrap();
        let c = s.enqueue_list_op("anilist", "1", "delete", "{}", 3).unwrap();
        assert!(a < b && b < c);
        assert_eq!(
            s.list_queue(None)
                .unwrap()
                .iter()
                .map(|q| q.id)
                .collect::<Vec<_>>(),
            [a, b, c]
        );
        assert_eq!(s.list_queue(Some("anilist")).unwrap().len(), 2);
        assert_eq!(s.count_list_queue("anilist").unwrap(), 2);
        assert!(s.has_list_ops("anilist", "21").unwrap());
        s.mark_list_op_failed(a, "offline").unwrap();
        s.mark_list_op_failed(a, "offline again").unwrap();
        let q = &s.list_queue(Some("anilist")).unwrap()[0];
        assert_eq!((q.attempts, q.last_error.as_deref()), (2, Some("offline again")));
        s.dequeue_list_op(a).unwrap();
        assert!(!s.has_list_ops("anilist", "21").unwrap());
        assert_eq!(
            s.dequeue_list_ops_for("anilist", "1", &["upsert", "delete"])
                .unwrap(),
            1
        );
        assert_eq!(s.clear_list_queue("mal").unwrap(), 1);
        assert!(s.list_queue(None).unwrap().is_empty());
    }

    #[test]
    fn conflicts_and_favorites() {
        let s = LocalStore::open_in_memory().unwrap();
        let id = s
            .add_list_conflict(&ListConflictRow {
                id: 0,
                service: "anilist".into(),
                media_id: "21".into(),
                local_json: "{}".into(),
                remote_json: "{}".into(),
                detected_at: 5,
            })
            .unwrap();
        assert!(s.has_list_conflict("anilist", "21").unwrap());
        assert_eq!(s.list_conflict(id).unwrap().unwrap().detected_at, 5);
        assert_eq!(s.list_conflicts().unwrap().len(), 1);
        s.delete_list_conflict(id).unwrap();
        assert!(s.list_conflicts().unwrap().is_empty());
        assert_eq!(s.delete_list_conflicts_for(None, "21").unwrap(), 0);

        let fav = |id: &str, at: i64| FavoriteRow {
            media_id: id.into(),
            title: format!("T{id}"),
            poster: None,
            added_at: at,
        };
        s.set_favorite(&fav("21", 1)).unwrap();
        s.set_favorite(&fav("character:40", 2)).unwrap();
        s.set_favorite(&FavoriteRow {
            poster: Some("p".into()),
            ..fav("21", 9)
        })
        .unwrap();
        assert!(s.is_favorite("21").unwrap());
        let all = s.favorites().unwrap();
        assert_eq!(
            all.iter().map(|f| f.media_id.as_str()).collect::<Vec<_>>(),
            ["character:40", "21"]
        );
        assert_eq!(
            (all[1].added_at, all[1].poster.as_deref()),
            (1, Some("p")),
            "first added_at is kept"
        );
        s.remove_favorite("21").unwrap();
        assert!(!s.is_favorite("21").unwrap());
    }
}
