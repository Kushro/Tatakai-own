//! `LocalStore` methods over `ext_servers`, `stremio_addons`, `ext_repos`, `ext_prefs`, `ext_updates` and
//! `provider_bookmarks` (wave B, worker **j**; docs/redesign/16 section 3.5). Child module of `store`.
//! `ExtServerRow` + `upsert_ext_server` / `ext_servers()` / `ext_server` / `set_ext_server_enabled` /
//! `set_ext_server_result(id, ok_at, error)` / `delete_ext_server`; `StremioRow` + `upsert_stremio` /
//! `stremio_addons()` / `set_stremio_enabled` / `delete_stremio`; `ExtRepoRow` + `upsert_ext_repo` / `ext_repos()` /
//! `set_ext_repo_catalog(id, json, fetched_at, error)` / `delete_ext_repo`; `ext_prefs(ext_id) -> Vec<(key, value)>` /
//! `set_ext_pref` / `clear_ext_prefs(ext_id)`; `ExtUpdateRow` + `set_ext_update` / `ext_updates()` /
//! `clear_ext_update(ext_id)`; `set_provider_bookmark` / `remove_provider_bookmark` / `provider_bookmarks()` /
//! `is_provider_bookmarked` / `set_provider_bookmark_completed`. The kill switch's "previously enabled" list goes in
//! the `settings` table under [`KILL_SWITCH_KEY`] (`set_setting` / `get_setting` / [`LocalStore::delete_setting`]).

use rusqlite::{OptionalExtension, Row, params};

use crate::error::StoreError;
use crate::store::LocalStore;

/// Tables of this file (the contract's `SCHEMA` creates them; the test below checks they exist).
pub const TABLES: &[&str] = &["ext_servers", "stremio_addons", "ext_repos", "ext_prefs", "ext_updates", "provider_bookmarks"];

/// `settings` key of the kill switch: ids that were enabled when "Disable all" ran (`ext.kill-switch`).
pub const KILL_SWITCH_KEY: &str = "ext.kill_switch.previous";
/// `settings` key prefix of the URL an extension was installed from (`ext.install-url`, update origin).
pub const ORIGIN_KEY_PREFIX: &str = "ext.origin.";
/// `settings` key prefix of the remembered Stremio title matches of one add-on (`ext.stremio.manage`).
pub const STREMIO_MATCH_PREFIX: &str = "stremio.matches.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtServerRow {
    pub id: String,
    pub name: String,
    pub url: String,
    pub role: String,
    pub enabled: bool,
    /// The server's own id (`/health` `serverId`).
    pub server_id: Option<String>,
    pub last_ok_at: Option<i64>,
    pub last_error: Option<String>,
    pub added_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StremioRow {
    pub id: String,
    pub manifest_url: String,
    pub manifest_json: String,
    pub enabled: bool,
    pub added_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtRepoRow {
    pub id: String,
    pub name: String,
    pub url: String,
    pub catalog_json: Option<String>,
    pub fetched_at: Option<i64>,
    pub last_error: Option<String>,
    pub added_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtUpdateRow {
    pub ext_id: String,
    pub current: String,
    pub latest: Option<String>,
    pub url: Option<String>,
    pub notes: Option<String>,
    pub checked_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderBookmarkRow {
    /// `ext:<ext_id>:<key>`.
    pub media_id: String,
    pub ext_id: String,
    pub title: String,
    pub poster: Option<String>,
    pub completed: bool,
    pub added_at: i64,
}

fn server_row(r: &Row<'_>) -> rusqlite::Result<ExtServerRow> {
    Ok(ExtServerRow {
        id: r.get("id")?,
        name: r.get("name")?,
        url: r.get("url")?,
        role: r.get("role")?,
        enabled: r.get::<_, i64>("enabled")? != 0,
        server_id: r.get("server_id")?,
        last_ok_at: r.get("last_ok_at")?,
        last_error: r.get("last_error")?,
        added_at: r.get("added_at")?,
    })
}

fn stremio_row(r: &Row<'_>) -> rusqlite::Result<StremioRow> {
    Ok(StremioRow {
        id: r.get("id")?,
        manifest_url: r.get("manifest_url")?,
        manifest_json: r.get("manifest_json")?,
        enabled: r.get::<_, i64>("enabled")? != 0,
        added_at: r.get("added_at")?,
    })
}

fn repo_row(r: &Row<'_>) -> rusqlite::Result<ExtRepoRow> {
    Ok(ExtRepoRow {
        id: r.get("id")?,
        name: r.get("name")?,
        url: r.get("url")?,
        catalog_json: r.get("catalog_json")?,
        fetched_at: r.get("fetched_at")?,
        last_error: r.get("last_error")?,
        added_at: r.get("added_at")?,
    })
}

fn update_row(r: &Row<'_>) -> rusqlite::Result<ExtUpdateRow> {
    Ok(ExtUpdateRow {
        ext_id: r.get("ext_id")?,
        current: r.get("current")?,
        latest: r.get("latest")?,
        url: r.get("url")?,
        notes: r.get("notes")?,
        checked_at: r.get("checked_at")?,
    })
}

fn bookmark_row(r: &Row<'_>) -> rusqlite::Result<ProviderBookmarkRow> {
    Ok(ProviderBookmarkRow {
        media_id: r.get("media_id")?,
        ext_id: r.get("ext_id")?,
        title: r.get("title")?,
        poster: r.get("poster")?,
        completed: r.get::<_, i64>("completed")? != 0,
        added_at: r.get("added_at")?,
    })
}

impl LocalStore {
    /// Remove one `settings` key (no-op when absent).
    pub fn delete_setting(&self, key: &str) -> Result<(), StoreError> {
        self.with(|c| c.execute("DELETE FROM settings WHERE key = ?1", [key]).map(|_| ()))
    }

    // ---- ext_servers -----------------------------------------------------------------------------

    /// Insert or replace (by `id`); `added_at` of an existing row is kept.
    pub fn upsert_ext_server(&self, s: &ExtServerRow) -> Result<(), StoreError> {
        self.with(|c| {
            c.execute(
                "INSERT INTO ext_servers (id, name, url, role, enabled, server_id, last_ok_at, last_error, added_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(id) DO UPDATE SET name = ?2, url = ?3, role = ?4, enabled = ?5, server_id = ?6,
                   last_ok_at = ?7, last_error = ?8",
                params![s.id, s.name, s.url, s.role, i64::from(s.enabled), s.server_id, s.last_ok_at, s.last_error, s.added_at],
            )
            .map(|_| ())
        })
    }

    pub fn ext_servers(&self) -> Result<Vec<ExtServerRow>, StoreError> {
        self.with(|c| {
            let mut st = c.prepare("SELECT * FROM ext_servers ORDER BY added_at ASC, id ASC")?;
            st.query_map([], server_row)?.collect()
        })
    }

    pub fn ext_server(&self, id: &str) -> Result<Option<ExtServerRow>, StoreError> {
        self.with(|c| c.query_row("SELECT * FROM ext_servers WHERE id = ?1", [id], server_row).optional())
    }

    /// `true` when the row exists.
    pub fn set_ext_server_enabled(&self, id: &str, enabled: bool) -> Result<bool, StoreError> {
        self.with(|c| c.execute("UPDATE ext_servers SET enabled = ?2 WHERE id = ?1", params![id, i64::from(enabled)]).map(|n| n > 0))
    }

    /// Outcome of a contact: `ok_at` set on success (error cleared), `error` set on failure (last ok kept).
    pub fn set_ext_server_result(&self, id: &str, ok_at: Option<i64>, error: Option<&str>) -> Result<(), StoreError> {
        self.with(|c| {
            match ok_at {
                Some(at) => c.execute("UPDATE ext_servers SET last_ok_at = ?2, last_error = NULL WHERE id = ?1", params![id, at])?,
                None => c.execute("UPDATE ext_servers SET last_error = ?2 WHERE id = ?1", params![id, error])?,
            };
            Ok(())
        })
    }

    pub fn delete_ext_server(&self, id: &str) -> Result<bool, StoreError> {
        self.with(|c| c.execute("DELETE FROM ext_servers WHERE id = ?1", [id]).map(|n| n > 0))
    }

    // ---- stremio_addons --------------------------------------------------------------------------

    /// Insert or replace by `manifest_url` (a re-added add-on keeps its id and `enabled`).
    pub fn upsert_stremio(&self, s: &StremioRow) -> Result<(), StoreError> {
        self.with(|c| {
            c.execute(
                "INSERT INTO stremio_addons (id, manifest_url, manifest_json, enabled, added_at) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(manifest_url) DO UPDATE SET manifest_json = ?3",
                params![s.id, s.manifest_url, s.manifest_json, i64::from(s.enabled), s.added_at],
            )
            .map(|_| ())
        })
    }

    pub fn stremio_addons(&self) -> Result<Vec<StremioRow>, StoreError> {
        self.with(|c| {
            let mut st = c.prepare("SELECT * FROM stremio_addons ORDER BY added_at ASC, id ASC")?;
            st.query_map([], stremio_row)?.collect()
        })
    }

    pub fn stremio_by_url(&self, manifest_url: &str) -> Result<Option<StremioRow>, StoreError> {
        self.with(|c| c.query_row("SELECT * FROM stremio_addons WHERE manifest_url = ?1", [manifest_url], stremio_row).optional())
    }

    pub fn set_stremio_enabled(&self, id: &str, enabled: bool) -> Result<bool, StoreError> {
        self.with(|c| c.execute("UPDATE stremio_addons SET enabled = ?2 WHERE id = ?1", params![id, i64::from(enabled)]).map(|n| n > 0))
    }

    pub fn delete_stremio(&self, id: &str) -> Result<bool, StoreError> {
        self.with(|c| c.execute("DELETE FROM stremio_addons WHERE id = ?1", [id]).map(|n| n > 0))
    }

    // ---- ext_repos -------------------------------------------------------------------------------

    /// Insert by `url`; an existing URL keeps its row (and is returned by the caller through `ext_repos`).
    pub fn upsert_ext_repo(&self, r: &ExtRepoRow) -> Result<(), StoreError> {
        self.with(|c| {
            c.execute(
                "INSERT INTO ext_repos (id, name, url, catalog_json, fetched_at, last_error, added_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(url) DO UPDATE SET name = ?2",
                params![r.id, r.name, r.url, r.catalog_json, r.fetched_at, r.last_error, r.added_at],
            )
            .map(|_| ())
        })
    }

    pub fn ext_repos(&self) -> Result<Vec<ExtRepoRow>, StoreError> {
        self.with(|c| {
            let mut st = c.prepare("SELECT * FROM ext_repos ORDER BY added_at ASC, id ASC")?;
            st.query_map([], repo_row)?.collect()
        })
    }

    /// Store a fetch: the catalogue on success (`json` = `Some`), else only the error (old catalogue kept).
    pub fn set_ext_repo_catalog(&self, id: &str, json: Option<&str>, fetched_at: i64, error: Option<&str>) -> Result<(), StoreError> {
        self.with(|c| {
            match json {
                Some(j) => c.execute(
                    "UPDATE ext_repos SET catalog_json = ?2, fetched_at = ?3, last_error = NULL WHERE id = ?1",
                    params![id, j, fetched_at],
                )?,
                None => c.execute("UPDATE ext_repos SET last_error = ?2 WHERE id = ?1", params![id, error])?,
            };
            Ok(())
        })
    }

    pub fn delete_ext_repo(&self, id: &str) -> Result<bool, StoreError> {
        self.with(|c| c.execute("DELETE FROM ext_repos WHERE id = ?1", [id]).map(|n| n > 0))
    }

    // ---- ext_prefs -------------------------------------------------------------------------------

    pub fn ext_prefs(&self, ext_id: &str) -> Result<Vec<(String, String)>, StoreError> {
        self.with(|c| {
            let mut st = c.prepare("SELECT key, value FROM ext_prefs WHERE ext_id = ?1 ORDER BY key ASC")?;
            st.query_map([ext_id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect()
        })
    }

    pub fn set_ext_pref(&self, ext_id: &str, key: &str, value: &str, now: i64) -> Result<(), StoreError> {
        self.with(|c| {
            c.execute(
                "INSERT INTO ext_prefs (ext_id, key, value, updated_at) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(ext_id, key) DO UPDATE SET value = ?3, updated_at = ?4",
                params![ext_id, key, value, now],
            )
            .map(|_| ())
        })
    }

    pub fn clear_ext_prefs(&self, ext_id: &str) -> Result<(), StoreError> {
        self.with(|c| c.execute("DELETE FROM ext_prefs WHERE ext_id = ?1", [ext_id]).map(|_| ()))
    }

    // ---- ext_updates -----------------------------------------------------------------------------

    pub fn set_ext_update(&self, u: &ExtUpdateRow) -> Result<(), StoreError> {
        self.with(|c| {
            c.execute(
                "INSERT INTO ext_updates (ext_id, current, latest, url, notes, checked_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(ext_id) DO UPDATE SET current = ?2, latest = ?3, url = ?4, notes = ?5, checked_at = ?6",
                params![u.ext_id, u.current, u.latest, u.url, u.notes, u.checked_at],
            )
            .map(|_| ())
        })
    }

    pub fn ext_updates(&self) -> Result<Vec<ExtUpdateRow>, StoreError> {
        self.with(|c| {
            let mut st = c.prepare("SELECT * FROM ext_updates ORDER BY ext_id ASC")?;
            st.query_map([], update_row)?.collect()
        })
    }

    pub fn clear_ext_update(&self, ext_id: &str) -> Result<(), StoreError> {
        self.with(|c| c.execute("DELETE FROM ext_updates WHERE ext_id = ?1", [ext_id]).map(|_| ()))
    }

    // ---- provider_bookmarks ----------------------------------------------------------------------

    /// Bookmark (or refresh the title / poster of) a provider title; `completed` and `added_at` survive.
    pub fn set_provider_bookmark(&self, b: &ProviderBookmarkRow) -> Result<(), StoreError> {
        self.with(|c| {
            c.execute(
                "INSERT INTO provider_bookmarks (media_id, ext_id, title, poster, completed, added_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(media_id) DO UPDATE SET title = ?3, poster = ?4",
                params![b.media_id, b.ext_id, b.title, b.poster, i64::from(b.completed), b.added_at],
            )
            .map(|_| ())
        })
    }

    pub fn remove_provider_bookmark(&self, media_id: &str) -> Result<bool, StoreError> {
        self.with(|c| c.execute("DELETE FROM provider_bookmarks WHERE media_id = ?1", [media_id]).map(|n| n > 0))
    }

    /// Newest first.
    pub fn provider_bookmarks(&self) -> Result<Vec<ProviderBookmarkRow>, StoreError> {
        self.with(|c| {
            let mut st = c.prepare("SELECT * FROM provider_bookmarks ORDER BY added_at DESC, media_id ASC")?;
            st.query_map([], bookmark_row)?.collect()
        })
    }

    pub fn provider_bookmark(&self, media_id: &str) -> Result<Option<ProviderBookmarkRow>, StoreError> {
        self.with(|c| c.query_row("SELECT * FROM provider_bookmarks WHERE media_id = ?1", [media_id], bookmark_row).optional())
    }

    /// `true` when the bookmark exists.
    pub fn set_provider_bookmark_completed(&self, media_id: &str, completed: bool) -> Result<bool, StoreError> {
        self.with(|c| {
            c.execute("UPDATE provider_bookmarks SET completed = ?2 WHERE media_id = ?1", params![media_id, i64::from(completed)]).map(|n| n > 0)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> LocalStore {
        LocalStore::open_in_memory().unwrap()
    }

    #[test]
    fn tables_exist() {
        let s = store();
        for t in TABLES {
            assert!(s.table_exists(t).unwrap(), "{t} missing");
        }
    }

    #[test]
    fn servers_round_trip() {
        let s = store();
        let row = ExtServerRow {
            id: "a".into(),
            name: "PC".into(),
            url: "http://10.0.0.2:8099".into(),
            role: "viewer".into(),
            enabled: true,
            server_id: Some("srv".into()),
            last_ok_at: None,
            last_error: None,
            added_at: 10,
        };
        s.upsert_ext_server(&row).unwrap();
        assert_eq!(s.ext_servers().unwrap(), vec![row.clone()]);
        assert!(s.set_ext_server_enabled("a", false).unwrap());
        s.set_ext_server_result("a", None, Some("offline")).unwrap();
        let got = s.ext_server("a").unwrap().unwrap();
        assert!(!got.enabled);
        assert_eq!(got.last_error.as_deref(), Some("offline"));
        s.set_ext_server_result("a", Some(99), None).unwrap();
        let got = s.ext_server("a").unwrap().unwrap();
        assert_eq!((got.last_ok_at, got.last_error), (Some(99), None));
        assert!(s.delete_ext_server("a").unwrap());
        assert!(s.ext_servers().unwrap().is_empty());
    }

    #[test]
    fn stremio_upsert_keeps_id_and_enabled() {
        let s = store();
        let row = StremioRow { id: "x".into(), manifest_url: "https://a/manifest.json".into(), manifest_json: "{}".into(), enabled: true, added_at: 1 };
        s.upsert_stremio(&row).unwrap();
        s.set_stremio_enabled("x", false).unwrap();
        s.upsert_stremio(&StremioRow { id: "y".into(), manifest_json: "{\"v\":2}".into(), ..row.clone() }).unwrap();
        let all = s.stremio_addons().unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!((all[0].id.as_str(), all[0].enabled, all[0].manifest_json.as_str()), ("x", false, "{\"v\":2}"));
        assert!(s.stremio_by_url("https://a/manifest.json").unwrap().is_some());
        assert!(s.delete_stremio("x").unwrap());
    }

    #[test]
    fn repos_prefs_updates_and_bookmarks() {
        let s = store();
        let repo = ExtRepoRow { id: "r".into(), name: "Repo".into(), url: "https://r/index.json".into(), catalog_json: None, fetched_at: None, last_error: None, added_at: 1 };
        s.upsert_ext_repo(&repo).unwrap();
        s.set_ext_repo_catalog("r", Some("[]"), 5, None).unwrap();
        s.set_ext_repo_catalog("r", None, 6, Some("down")).unwrap();
        let r = &s.ext_repos().unwrap()[0];
        assert_eq!((r.catalog_json.as_deref(), r.fetched_at, r.last_error.as_deref()), (Some("[]"), Some(5), Some("down")));

        s.set_ext_pref("e", "quality", "720", 1).unwrap();
        s.set_ext_pref("e", "quality", "1080", 2).unwrap();
        s.set_ext_pref("e", "apiKey", "k", 2).unwrap();
        assert_eq!(s.ext_prefs("e").unwrap(), vec![("apiKey".into(), "k".into()), ("quality".into(), "1080".into())]);
        s.clear_ext_prefs("e").unwrap();
        assert!(s.ext_prefs("e").unwrap().is_empty());

        let u = ExtUpdateRow { ext_id: "e".into(), current: "1.0.0".into(), latest: Some("1.1.0".into()), url: None, notes: None, checked_at: 3 };
        s.set_ext_update(&u).unwrap();
        assert_eq!(s.ext_updates().unwrap(), vec![u]);
        s.clear_ext_update("e").unwrap();
        assert!(s.ext_updates().unwrap().is_empty());

        let b = ProviderBookmarkRow { media_id: "ext:e:s:1".into(), ext_id: "e".into(), title: "T".into(), poster: None, completed: false, added_at: 4 };
        s.set_provider_bookmark(&b).unwrap();
        assert!(s.set_provider_bookmark_completed("ext:e:s:1", true).unwrap());
        s.set_provider_bookmark(&ProviderBookmarkRow { title: "T2".into(), ..b.clone() }).unwrap();
        let got = s.provider_bookmark("ext:e:s:1").unwrap().unwrap();
        assert_eq!((got.title.as_str(), got.completed), ("T2", true));
        assert!(s.remove_provider_bookmark("ext:e:s:1").unwrap());
        assert!(!s.set_provider_bookmark_completed("ext:e:s:1", true).unwrap());

        s.set_setting(KILL_SWITCH_KEY, &vec!["a".to_owned()]).unwrap();
        s.delete_setting(KILL_SWITCH_KEY).unwrap();
        assert!(s.get_setting::<Vec<String>>(KILL_SWITCH_KEY).unwrap().is_none());
    }
}
