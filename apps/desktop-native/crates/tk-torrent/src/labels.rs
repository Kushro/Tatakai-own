//! `<persistence_dir>/labels.json`: what the engine remembers per info-hash next to the librqbit session (app label,
//! keep-after-watching flag, first-added time). Small file, rewritten atomically (temp file + rename) on every change.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::types::TorrentLabel;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct LabelRecord {
    pub label: Option<TorrentLabel>,
    pub persist: bool,
    pub added_unix: i64,
}

impl LabelRecord {
    pub fn new() -> Self {
        Self { label: None, persist: false, added_unix: now_unix() }
    }
}

pub(crate) struct LabelStore {
    path: PathBuf,
    map: Mutex<HashMap<String, LabelRecord>>,
}

impl LabelStore {
    /// Read the file; a missing or unreadable file is an empty store (logged).
    pub fn load(path: PathBuf) -> Self {
        let map = match std::fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<HashMap<String, LabelRecord>>(&bytes) {
                Ok(m) => m,
                Err(e) => {
                    tracing::warn!(path = %path.display(), error = %e, "torrent labels unreadable; starting empty");
                    HashMap::new()
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "cannot read torrent labels; starting empty");
                HashMap::new()
            }
        };
        Self { path, map: Mutex::new(map) }
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, LabelRecord>> {
        self.map.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn get(&self, id: &str) -> Option<LabelRecord> {
        self.lock().get(id).cloned()
    }

    pub fn ids(&self) -> Vec<String> {
        self.lock().keys().cloned().collect()
    }

    /// Replace (or create) the record of `id`.
    pub fn set(&self, id: &str, record: LabelRecord) {
        let mut map = self.lock();
        map.insert(id.to_owned(), record);
        self.save(&map);
    }

    /// Edit the record of `id`, creating it (added now, not persisted) when missing.
    pub fn update(&self, id: &str, f: impl FnOnce(&mut LabelRecord)) {
        let mut map = self.lock();
        f(map.entry(id.to_owned()).or_insert_with(LabelRecord::new));
        self.save(&map);
    }

    pub fn remove(&self, id: &str) {
        let mut map = self.lock();
        if map.remove(id).is_some() {
            self.save(&map);
        }
    }

    fn save(&self, map: &HashMap<String, LabelRecord>) {
        // Sorted for stable diffs of the file.
        let sorted: BTreeMap<&String, &LabelRecord> = map.iter().collect();
        let bytes = match serde_json::to_vec_pretty(&sorted) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(error = %e, "cannot serialize torrent labels");
                return;
            }
        };
        if let Some(dir) = self.path.parent()
            && let Err(e) = std::fs::create_dir_all(dir)
        {
            tracing::warn!(dir = %dir.display(), error = %e, "cannot create the torrent state folder");
            return;
        }
        let tmp = self.path.with_extension("json.tmp");
        let result = std::fs::write(&tmp, &bytes).and_then(|()| std::fs::rename(&tmp, &self.path));
        if let Err(e) = result {
            tracing::warn!(path = %self.path.display(), error = %e, "cannot write torrent labels");
        }
    }
}

pub(crate) fn now_unix() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX)).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("labels.json");
        let store = LabelStore::load(path.clone());
        assert!(store.get("abc").is_none());
        store.update("abc", |r| {
            r.persist = true;
            r.label = Some(TorrentLabel { media_id: "21".into(), title: "One Piece".into(), episode: Some(3), poster: None });
        });
        let reloaded = LabelStore::load(path);
        let rec = reloaded.get("abc").unwrap();
        assert!(rec.persist);
        assert_eq!(rec.label.unwrap().episode, Some(3));
        assert!(rec.added_unix > 0);
        reloaded.remove("abc");
        assert!(reloaded.ids().is_empty());
    }
}
