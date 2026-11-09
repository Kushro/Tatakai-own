//! `LocalStore` methods of worker **m** (wave B; docs/redesign/16 section 3.5). Child module of `store`.
//!
//! - `tts_cache`: synthesized narration clips (`TtsClipRow`, key = sha256(provider | voice | rate | text)); the files
//!   live under `<data>/tts/cache`, the caller deletes the paths eviction returns.
//! - Novel rows of the tables shared with worker l through their `kind` column (`kind = 'novel'`, the schema of
//!   `store/mod.rs` is the contract): cached chapter lists (`chapter_lists`), offline chapter text
//!   (`offline_chapters.text_json`), the newest reading positions (`read_progress`, written through
//!   `Backend::set_read_progress`, read here for "Continue reading" and the details resume), and the finished rows
//!   novel chapter downloads add to worker h's `downloads` table (`kind = 'novel'`) so the Downloads page lists them.
//!   Every method here is prefixed `novel_` / `tts_` so it never collides with worker l's or h's methods.

use rusqlite::{OptionalExtension, params};

use crate::error::StoreError;
use crate::store::LocalStore;

/// Tables of this file (the contract's `SCHEMA` creates them; the test below checks they exist).
pub const TABLES: &[&str] = &["tts_cache"];

/// `kind` value of every novel row in the shared reading tables.
pub const NOVEL_KIND: &str = "novel";

/// One cached narration clip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TtsClipRow {
    pub key: String,
    pub provider: String,
    pub voice: String,
    pub rate: u32,
    pub path: String,
    pub mime: String,
    pub duration_ms: Option<u32>,
    pub bytes: i64,
    pub created_at: i64,
}

/// One `read_progress` row of a novel.
#[derive(Debug, Clone, PartialEq)]
pub struct NovelProgressRow {
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

/// A finished (or failed) novel chapter download, as a `downloads` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NovelDownloadRow {
    pub media_id: String,
    pub title: String,
    pub poster: Option<String>,
    /// Display label of the chapter ("12" / "12 · Title").
    pub chapter: String,
    /// `novel:<ext_id>/<chapter_id>`.
    pub url: String,
    /// `completed` | `failed`.
    pub state: String,
    pub bytes: i64,
    pub error: Option<String>,
    pub at: i64,
}

fn clip_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<TtsClipRow> {
    Ok(TtsClipRow {
        key: r.get("key")?,
        provider: r.get("provider")?,
        voice: r.get("voice")?,
        rate: r.get::<_, i64>("rate")?.max(0) as u32,
        path: r.get("path")?,
        mime: r.get("mime")?,
        duration_ms: r.get::<_, Option<i64>>("duration_ms")?.map(|d| d.max(0) as u32),
        bytes: r.get("bytes")?,
        created_at: r.get("created_at")?,
    })
}

fn progress_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<NovelProgressRow> {
    Ok(NovelProgressRow {
        media_id: r.get("media_id")?,
        ext_id: r.get("ext_id")?,
        chapter_id: r.get("chapter_id")?,
        chapter_number: r.get::<_, f64>("chapter_number")? as f32,
        position: r.get::<_, i64>("position")?.max(0) as u32,
        total: r.get::<_, Option<i64>>("total")?.map(|t| t.max(0) as u32),
        completed: r.get::<_, i64>("completed")? != 0,
        title: r.get("title")?,
        poster: r.get("poster")?,
        updated_at: r.get("updated_at")?,
    })
}

const PROGRESS_COLUMNS: &str = "media_id, ext_id, chapter_id, chapter_number, position, total, completed, title, poster, updated_at";

impl LocalStore {
    // ---- tts_cache --------------------------------------------------------------------------------

    pub fn tts_clip(&self, key: &str) -> Result<Option<TtsClipRow>, StoreError> {
        self.with(|c| c.query_row("SELECT * FROM tts_cache WHERE key = ?1", [key], clip_row).optional())
    }

    pub fn put_tts_clip(&self, row: &TtsClipRow) -> Result<(), StoreError> {
        self.with(|c| {
            c.execute(
                "INSERT INTO tts_cache (key, provider, voice, rate, path, mime, duration_ms, bytes, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(key) DO UPDATE SET provider = ?2, voice = ?3, rate = ?4, path = ?5, mime = ?6,
                   duration_ms = ?7, bytes = ?8, created_at = ?9",
                params![
                    row.key,
                    row.provider,
                    row.voice,
                    i64::from(row.rate),
                    row.path,
                    row.mime,
                    row.duration_ms.map(i64::from),
                    row.bytes,
                    row.created_at
                ],
            )?;
            Ok(())
        })
    }

    pub fn delete_tts_clip(&self, key: &str) -> Result<(), StoreError> {
        self.with(|c| c.execute("DELETE FROM tts_cache WHERE key = ?1", [key]).map(|_| ()))
    }

    pub fn tts_cache_bytes(&self) -> Result<i64, StoreError> {
        self.with(|c| c.query_row("SELECT COALESCE(SUM(bytes), 0) FROM tts_cache", [], |r| r.get::<_, i64>(0)))
    }

    /// Drop the oldest clips until the cache holds at most `max_bytes`; returns the paths of the removed rows.
    pub fn evict_tts_cache(&self, max_bytes: i64) -> Result<Vec<String>, StoreError> {
        self.with(|c| {
            let total: i64 = c.query_row("SELECT COALESCE(SUM(bytes), 0) FROM tts_cache", [], |r| r.get(0))?;
            if total <= max_bytes {
                return Ok(Vec::new());
            }
            let mut stmt = c.prepare("SELECT key, path, bytes FROM tts_cache ORDER BY created_at ASC, key ASC")?;
            let rows = stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let mut left = total;
            let mut removed = Vec::new();
            for (key, path, bytes) in rows {
                if left <= max_bytes {
                    break;
                }
                c.execute("DELETE FROM tts_cache WHERE key = ?1", [&key])?;
                left -= bytes;
                removed.push(path);
            }
            Ok(removed)
        })
    }

    /// Remove every clip row; returns the paths.
    pub fn clear_tts_cache(&self) -> Result<Vec<String>, StoreError> {
        self.with(|c| {
            let paths = {
                let mut stmt = c.prepare("SELECT path FROM tts_cache")?;
                stmt.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?
            };
            c.execute("DELETE FROM tts_cache", [])?;
            Ok(paths)
        })
    }

    // ---- novel chapter lists (chapter_lists, kind = 'novel') ---------------------------------------

    /// Cached chapter JSON of one extension for a novel, when younger than `max_age_secs` at `now`.
    pub fn novel_chapter_list(&self, media_id: &str, ext_id: &str, max_age_secs: i64, now: i64) -> Result<Option<String>, StoreError> {
        self.with(|c| {
            c.query_row(
                "SELECT chapters_json FROM chapter_lists WHERE kind = ?1 AND media_id = ?2 AND ext_id = ?3 AND fetched_at >= ?4",
                params![NOVEL_KIND, media_id, ext_id, now - max_age_secs],
                |r| r.get::<_, String>(0),
            )
            .optional()
        })
    }

    /// Every cached chapter list of a novel regardless of age: `(ext_id, chapters_json)`.
    pub fn novel_chapter_lists(&self, media_id: &str) -> Result<Vec<(String, String)>, StoreError> {
        self.with(|c| {
            let mut stmt = c.prepare("SELECT ext_id, chapters_json FROM chapter_lists WHERE kind = ?1 AND media_id = ?2 ORDER BY ext_id")?;
            stmt.query_map(params![NOVEL_KIND, media_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
                .collect()
        })
    }

    pub fn set_novel_chapter_list(&self, media_id: &str, ext_id: &str, chapters_json: &str, now: i64) -> Result<(), StoreError> {
        self.with(|c| {
            c.execute(
                "INSERT INTO chapter_lists (kind, media_id, ext_id, chapters_json, fetched_at) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(kind, media_id, ext_id) DO UPDATE SET chapters_json = ?4, fetched_at = ?5",
                params![NOVEL_KIND, media_id, ext_id, chapters_json, now],
            )?;
            Ok(())
        })
    }

    // ---- offline chapters (offline_chapters, kind = 'novel', text inline) ----------------------------

    pub fn put_novel_offline(&self, media_id: &str, ext_id: &str, chapter_id: &str, text_json: &str, now: i64) -> Result<(), StoreError> {
        self.with(|c| {
            c.execute(
                "INSERT INTO offline_chapters (kind, media_id, ext_id, chapter_id, dir, text_json, pages, bytes, saved_at)
                 VALUES (?1, ?2, ?3, ?4, NULL, ?5, NULL, ?6, ?7)
                 ON CONFLICT(kind, media_id, ext_id, chapter_id) DO UPDATE SET text_json = ?5, bytes = ?6, saved_at = ?7",
                params![NOVEL_KIND, media_id, ext_id, chapter_id, text_json, text_json.len() as i64, now],
            )?;
            Ok(())
        })
    }

    pub fn novel_offline_text(&self, media_id: &str, ext_id: &str, chapter_id: &str) -> Result<Option<String>, StoreError> {
        self.with(|c| {
            c.query_row(
                "SELECT text_json FROM offline_chapters WHERE kind = ?1 AND media_id = ?2 AND ext_id = ?3 AND chapter_id = ?4",
                params![NOVEL_KIND, media_id, ext_id, chapter_id],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()
            .map(Option::flatten)
        })
    }

    /// `(ext_id, chapter_id)` of the downloaded chapters of a novel.
    pub fn novel_offline_ids(&self, media_id: &str) -> Result<Vec<(String, String)>, StoreError> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT ext_id, chapter_id FROM offline_chapters WHERE kind = ?1 AND media_id = ?2 AND text_json IS NOT NULL",
            )?;
            stmt.query_map(params![NOVEL_KIND, media_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
                .collect()
        })
    }

    // ---- reading positions (read_progress, kind = 'novel'; written by Backend::set_read_progress) ----

    /// Newest position per novel, newest first.
    pub fn novel_recent_progress(&self, limit: u32) -> Result<Vec<NovelProgressRow>, StoreError> {
        self.with(|c| {
            let sql = format!(
                "SELECT {PROGRESS_COLUMNS} FROM read_progress p WHERE kind = ?1 AND updated_at = (
                   SELECT MAX(updated_at) FROM read_progress q WHERE q.kind = p.kind AND q.media_id = p.media_id)
                 GROUP BY media_id ORDER BY updated_at DESC LIMIT ?2"
            );
            let mut stmt = c.prepare(&sql)?;
            stmt.query_map(params![NOVEL_KIND, i64::from(limit)], progress_row)?.collect()
        })
    }

    /// Every position of one novel, newest first.
    pub fn novel_progress_rows(&self, media_id: &str) -> Result<Vec<NovelProgressRow>, StoreError> {
        self.with(|c| {
            let sql = format!("SELECT {PROGRESS_COLUMNS} FROM read_progress WHERE kind = ?1 AND media_id = ?2 ORDER BY updated_at DESC");
            let mut stmt = c.prepare(&sql)?;
            stmt.query_map(params![NOVEL_KIND, media_id], progress_row)?.collect()
        })
    }

    // ---- downloads rows (worker h's table, kind = 'novel') ------------------------------------------

    /// Record a finished / failed novel chapter download; returns the row id.
    pub fn novel_insert_download(&self, row: &NovelDownloadRow) -> Result<i64, StoreError> {
        self.with(|c| {
            c.execute(
                "INSERT INTO downloads (media_id, title, poster, episode, chapter, kind, state, url, headers_json, path,
                   bytes_done, bytes_total, segments_done, segments_total, error, added_at, started_at, completed_at)
                 VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?6, ?7, '[]', NULL, ?8, ?8, 1, 1, ?9, ?10, ?10, ?11)",
                params![
                    row.media_id,
                    row.title,
                    row.poster,
                    row.chapter,
                    NOVEL_KIND,
                    row.state,
                    row.url,
                    row.bytes,
                    row.error,
                    row.at,
                    (row.state == "completed").then_some(row.at)
                ],
            )?;
            Ok(c.last_insert_rowid())
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

    fn clip(key: &str, bytes: i64, at: i64) -> TtsClipRow {
        TtsClipRow {
            key: key.into(),
            provider: "system".into(),
            voice: "".into(),
            rate: 100,
            path: format!("/tmp/{key}.wav"),
            mime: "audio/wav".into(),
            duration_ms: Some(1200),
            bytes,
            created_at: at,
        }
    }

    #[test]
    fn clip_cache_put_get_evict_clear() {
        let s = LocalStore::open_in_memory().unwrap();
        assert_eq!(s.tts_clip("a").unwrap(), None);
        s.put_tts_clip(&clip("a", 100, 1)).unwrap();
        s.put_tts_clip(&clip("b", 100, 2)).unwrap();
        s.put_tts_clip(&clip("c", 100, 3)).unwrap();
        assert_eq!(s.tts_clip("b").unwrap().unwrap().duration_ms, Some(1200));
        assert_eq!(s.tts_cache_bytes().unwrap(), 300);
        assert_eq!(s.evict_tts_cache(150).unwrap(), vec!["/tmp/a.wav".to_owned(), "/tmp/b.wav".to_owned()]);
        assert!(s.evict_tts_cache(150).unwrap().is_empty());
        s.delete_tts_clip("c").unwrap();
        s.put_tts_clip(&clip("d", 5, 4)).unwrap();
        assert_eq!(s.clear_tts_cache().unwrap(), vec!["/tmp/d.wav".to_owned()]);
        assert_eq!(s.tts_cache_bytes().unwrap(), 0);
    }

    #[test]
    fn chapter_lists_and_offline_text() {
        let s = LocalStore::open_in_memory().unwrap();
        s.set_novel_chapter_list("1", "toko", "[]", 100).unwrap();
        assert_eq!(s.novel_chapter_list("1", "toko", 50, 120).unwrap().as_deref(), Some("[]"));
        assert_eq!(s.novel_chapter_list("1", "toko", 10, 120).unwrap(), None);
        s.set_novel_chapter_list("1", "toko", "[1]", 130).unwrap();
        assert_eq!(s.novel_chapter_lists("1").unwrap(), vec![("toko".to_owned(), "[1]".to_owned())]);
        assert_eq!(s.novel_offline_text("1", "toko", "c1").unwrap(), None);
        s.put_novel_offline("1", "toko", "c1", "[\"p\"]", 5).unwrap();
        assert_eq!(s.novel_offline_text("1", "toko", "c1").unwrap().as_deref(), Some("[\"p\"]"));
        assert_eq!(s.novel_offline_ids("1").unwrap(), vec![("toko".to_owned(), "c1".to_owned())]);
    }

    #[test]
    fn recent_progress_is_newest_per_title() {
        let s = LocalStore::open_in_memory().unwrap();
        s.with(|c| {
            for (m, ch, at) in [("1", "a", 10), ("1", "b", 30), ("2", "x", 20)] {
                c.execute(
                    "INSERT INTO read_progress (kind, media_id, ext_id, chapter_id, chapter_number, position, updated_at)
                     VALUES ('novel', ?1, 'toko', ?2, 1.0, 4, ?3)",
                    params![m, ch, at],
                )?;
            }
            c.execute(
                "INSERT INTO read_progress (kind, media_id, ext_id, chapter_id, chapter_number, position, updated_at)
                 VALUES ('manga', '9', 'm', 'z', 1.0, 0, 99)",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        let rows = s.novel_recent_progress(10).unwrap();
        assert_eq!(rows.iter().map(|r| (r.media_id.as_str(), r.chapter_id.as_str())).collect::<Vec<_>>(), vec![("1", "b"), ("2", "x")]);
        assert_eq!(rows[0].position, 4);
        assert_eq!(s.novel_progress_rows("1").unwrap().len(), 2);
    }

    #[test]
    fn download_rows() {
        let s = LocalStore::open_in_memory().unwrap();
        let id = s
            .novel_insert_download(&NovelDownloadRow {
                media_id: "1".into(),
                title: "Overlord".into(),
                poster: None,
                chapter: "3".into(),
                url: "novel:toko/abc".into(),
                state: "completed".into(),
                bytes: 42,
                error: None,
                at: 7,
            })
            .unwrap();
        let (kind, state, done): (String, String, Option<i64>) = s
            .with(|c| c.query_row("SELECT kind, state, completed_at FROM downloads WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))))
            .unwrap();
        assert_eq!((kind.as_str(), state.as_str(), done), ("novel", "completed", Some(7)));
    }
}
