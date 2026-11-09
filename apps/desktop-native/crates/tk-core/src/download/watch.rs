//! Watcher of the local-library folders (worker **h**; checklist `downloads.local-library.scan`; classic
//! `desktop/ipc/ipc-library.cjs` re-scans on change). `notify` (inotify / ReadDirectoryChangesW / FSEvents) reports
//! new or renamed video files; tk-app debounces them and imports them like a scan (`live/downloads.rs`).

use std::path::{Path, PathBuf};

use notify::event::ModifyKind;
use notify::{EventKind, RecursiveMode, Watcher};

use super::library::is_library_video;

/// Keeps the OS watch alive; dropping it stops watching.
pub struct LibraryWatcher {
    _inner: notify::RecommendedWatcher,
    dirs: Vec<PathBuf>,
}

impl LibraryWatcher {
    /// Watch every existing folder of `dirs` recursively; `on_video` gets each finished video path that appears
    /// (called on the watcher's own thread: keep it cheap, e.g. send into a channel). Folders that cannot be watched
    /// are skipped with a log line; the scan still covers them.
    pub fn start(dirs: &[PathBuf], on_video: impl Fn(PathBuf) + Send + 'static) -> Result<Self, notify::Error> {
        let mut inner = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let Ok(ev) = res else { return };
            if !is_relevant(&ev.kind) {
                return;
            }
            for p in ev.paths {
                if is_library_video(&p) {
                    on_video(p);
                }
            }
        })?;
        let mut watched = Vec::new();
        for d in dirs {
            if !d.is_dir() {
                continue;
            }
            match inner.watch(d, RecursiveMode::Recursive) {
                Ok(()) => watched.push(d.clone()),
                Err(e) => tracing::warn!(dir = %d.display(), error = %e, "library folder not watched"),
            }
        }
        Ok(Self { _inner: inner, dirs: watched })
    }

    /// Folders actually watched.
    pub fn dirs(&self) -> &[PathBuf] {
        &self.dirs
    }

    pub fn watches(&self, dir: &Path) -> bool {
        self.dirs.iter().any(|d| d == dir)
    }
}

/// Events that may mean "a new video is there": creation, a rename into place (engines and browsers write
/// `name.part` then rename) and data writes (copies in progress; the importer reads the size again later).
pub fn is_relevant(kind: &EventKind) -> bool {
    matches!(kind, EventKind::Create(_) | EventKind::Modify(ModifyKind::Name(_)) | EventKind::Modify(ModifyKind::Data(_)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{CreateKind, DataChange, MetadataKind, RemoveKind, RenameMode};

    #[test]
    fn relevant_events() {
        assert!(is_relevant(&EventKind::Create(CreateKind::File)));
        assert!(is_relevant(&EventKind::Modify(ModifyKind::Name(RenameMode::To))));
        assert!(is_relevant(&EventKind::Modify(ModifyKind::Data(DataChange::Content))));
        assert!(!is_relevant(&EventKind::Modify(ModifyKind::Metadata(MetadataKind::Permissions))));
        assert!(!is_relevant(&EventKind::Remove(RemoveKind::File)));
        assert!(!is_relevant(&EventKind::Access(notify::event::AccessKind::Any)));
    }

    #[test]
    fn missing_folders_are_skipped() {
        let w = LibraryWatcher::start(&[PathBuf::from("/definitely/not/here/tatakai")], |_| {});
        // Some CI sandboxes have no inotify: only check the folder list when the watcher could start.
        if let Ok(w) = w {
            assert!(w.dirs().is_empty());
        }
    }
}
