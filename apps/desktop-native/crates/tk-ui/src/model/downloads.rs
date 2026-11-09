//! Downloads preferences and view-models (wave B, worker **h**; docs/redesign/16-parity-wave-b-contract.md
//! section 3.2): HLS / MP4 engine queue, Downloads page, local library, auto-download. `DownloadPrefs` is embedded
//! in `Settings::downloads` (`SettingsAction::SetDownloads`). Worker h owns this file; `model/mod.rs` stays frozen.

use serde::{Deserialize, Serialize};

use crate::backend::MediaVm;

/// Bounds of [`DownloadPrefs`].
pub const CONCURRENT_MIN: u8 = 1;
pub const CONCURRENT_MAX: u8 = 6;
pub const AUTO_CHECK_MIN_MINUTES: u16 = 15;
pub const AUTO_CHECK_MAX_MINUTES: u16 = 1440;

/// Auto-download quality profile (classic `AutomationProfiles`: best / balanced 1080p / data saver 720p).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoProfileVm {
    Best,
    #[default]
    Balanced,
    DataSaver,
}

impl AutoProfileVm {
    pub const ALL: [AutoProfileVm; 3] = [Self::Best, Self::Balanced, Self::DataSaver];

    /// i18n suffix `downloads.profile.<id>`.
    pub fn id(self) -> &'static str {
        match self {
            Self::Best => "best",
            Self::Balanced => "balanced",
            Self::DataSaver => "data_saver",
        }
    }
}

/// `Settings::downloads`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DownloadPrefs {
    /// Download folder; empty = `<user Downloads>/Tatakai/Episodes` (`Backend::download_storage().dir` shows it).
    pub dir: String,
    /// Parallel downloads, `1..=6`.
    pub concurrent: u8,
    /// KiB/s for the whole queue, 0 = unlimited.
    pub speed_limit_kib: u32,
    /// Disk cap for completed web downloads (GiB), 0 = none; the oldest watched ones go first.
    pub storage_limit_gib: u32,
    /// Auto-download new episodes of the titles in the "Watching" list (`downloads.auto.new-episodes`).
    pub auto_new_episodes: bool,
    pub auto_profile: AutoProfileVm,
    /// How often the auto-downloader looks for new episodes, minutes `15..=1440`.
    pub auto_check_minutes: u16,
    /// Preferred extension / server for downloads; `None` = the ranked best source.
    pub preferred_server: Option<String>,
    /// Preferred audio language (ISO 639-1) for downloads.
    pub preferred_audio: String,
    /// Folders scanned into the local library (`downloads.local-library.scan`).
    pub library_dirs: Vec<String>,
    /// Delete completed downloads this many days after they were watched; 0 = never.
    pub delete_watched_days: u16,
}

impl Default for DownloadPrefs {
    fn default() -> Self {
        Self {
            dir: String::new(),
            concurrent: 2,
            speed_limit_kib: 0,
            storage_limit_gib: 0,
            auto_new_episodes: false,
            auto_profile: AutoProfileVm::Balanced,
            auto_check_minutes: 30,
            preferred_server: None,
            preferred_audio: "ja".to_owned(),
            library_dirs: Vec::new(),
            delete_watched_days: 0,
        }
    }
}

impl DownloadPrefs {
    pub fn sanitized(mut self) -> Self {
        self.dir = self.dir.trim().to_owned();
        self.concurrent = self.concurrent.clamp(CONCURRENT_MIN, CONCURRENT_MAX);
        self.auto_check_minutes = self.auto_check_minutes.clamp(AUTO_CHECK_MIN_MINUTES, AUTO_CHECK_MAX_MINUTES);
        self.preferred_server = self.preferred_server.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
        self.preferred_audio = self.preferred_audio.trim().to_ascii_lowercase();
        if self.preferred_audio.is_empty() {
            self.preferred_audio = "ja".to_owned();
        }
        self.library_dirs = self.library_dirs.into_iter().map(|d| d.trim().to_owned()).filter(|d| !d.is_empty()).collect();
        self.library_dirs.dedup();
        self
    }
}

/// What a queue row downloads. `id()` = i18n suffix `downloads.kind.<id>` and `data-kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DownloadKindVm {
    Hls,
    Mp4,
    /// Background torrent (mirrored from the torrent engine so the page unifies both).
    Torrent,
    MangaChapter,
    NovelChapter,
}

impl DownloadKindVm {
    pub const ALL: [DownloadKindVm; 5] = [Self::Hls, Self::Mp4, Self::Torrent, Self::MangaChapter, Self::NovelChapter];

    pub fn id(self) -> &'static str {
        match self {
            Self::Hls => "hls",
            Self::Mp4 => "mp4",
            Self::Torrent => "torrent",
            Self::MangaChapter => "manga",
            Self::NovelChapter => "novel",
        }
    }
}

/// Queue state. `id()` = i18n suffix `downloads.state.<id>` and `data-state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DownloadStateVm {
    Queued,
    Downloading,
    Paused,
    Completed,
    Failed,
    Cancelled,
    /// Completed before, but the file is missing or truncated (`downloads.integrity-check`,
    /// `Backend::download_verify`): the row offers "Download again" / "Remove".
    Broken,
}

impl DownloadStateVm {
    pub const ALL: [DownloadStateVm; 7] =
        [Self::Queued, Self::Downloading, Self::Paused, Self::Completed, Self::Failed, Self::Cancelled, Self::Broken];

    pub fn id(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Downloading => "downloading",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Broken => "broken",
        }
    }

    /// Shown in the Downloads page "Active" tab (everything else is History).
    pub fn is_active(self) -> bool {
        matches!(self, Self::Queued | Self::Downloading | Self::Paused)
    }
}

/// One row of the download queue / history (`downloads` table).
#[derive(Debug, Clone, PartialEq)]
pub struct DownloadVm {
    pub id: i64,
    pub media_id: String,
    pub title: String,
    pub poster: Option<String>,
    /// Anime episode; `None` for manga / novel chapters.
    pub episode: Option<u32>,
    /// Chapter label ("Ch. 12") for manga / novel rows.
    pub chapter: Option<String>,
    pub kind: DownloadKindVm,
    pub state: DownloadStateVm,
    /// 0.0..=1.0
    pub progress: f64,
    pub bytes_done: u64,
    pub bytes_total: Option<u64>,
    pub speed_bps: u64,
    pub eta_secs: Option<u64>,
    /// Display-ready, when `state == Failed`.
    pub error: Option<String>,
    /// Final file / folder (display form) once completed.
    pub path: Option<String>,
    pub added_unix: i64,
    pub completed_unix: Option<i64>,
}

/// Options of `Backend::download_enqueue` / `download_enqueue_many`; `None` = the ranked best source with the
/// download prefs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DownloadRequestVm {
    /// A `SourceVm::id` to download exactly that source.
    pub source_id: Option<String>,
    /// `"1080p"` ...
    pub quality: Option<String>,
    pub audio_lang: Option<String>,
    pub sub_lang: Option<String>,
}

/// Disk use of the download folder (Downloads page header, Settings > Downloads).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DownloadStorageVm {
    /// Folder in use (display form).
    pub dir: String,
    pub used_bytes: u64,
    /// `storage_limit_gib` in bytes, `None` = no cap.
    pub limit_bytes: Option<u64>,
    pub free_bytes: Option<u64>,
    /// Completed items on disk.
    pub items: u32,
}

/// One file of the local library (`local_library` table; classic `OfflineLibraryPage` + `ImportAnimeModal`).
#[derive(Debug, Clone, PartialEq)]
pub struct LocalItemVm {
    pub id: i64,
    /// Absolute path (display form).
    pub path: String,
    pub file_name: String,
    pub size_bytes: u64,
    /// Matched title, `None` = unmatched (the row offers "Match...").
    pub media: Option<MediaVm>,
    pub episode: Option<u32>,
    pub added_unix: i64,
    /// The file still exists and has a playable extension.
    pub playable: bool,
}

/// Result of a folder scan / import (`downloads.local-library.scan` / `import`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LibraryScanVm {
    pub scanned: u32,
    pub added: u32,
    pub matched: u32,
    /// File names the release-name parser could not place.
    pub unmatched: Vec<String>,
}

/// One auto-download pass (`downloads.auto.new-episodes`, "Check now").
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AutoDownloadRunVm {
    pub ran_unix: i64,
    /// Titles of the Watching list that were checked.
    pub checked: u32,
    pub enqueued: u32,
    /// Display-ready reasons ("<title>: no source with 1080p").
    pub skipped: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sanitized() {
        let d = DownloadPrefs::default();
        assert_eq!(d.clone().sanitized(), d);
    }

    #[test]
    fn bounds_and_blank_values() {
        let p = DownloadPrefs {
            concurrent: 0,
            auto_check_minutes: 1,
            preferred_audio: "  ".into(),
            library_dirs: vec![" /a ".into(), "".into()],
            ..DownloadPrefs::default()
        }
        .sanitized();
        assert_eq!(p.concurrent, CONCURRENT_MIN);
        assert_eq!(p.auto_check_minutes, AUTO_CHECK_MIN_MINUTES);
        assert_eq!(p.preferred_audio, "ja");
        assert_eq!(p.library_dirs, vec!["/a".to_owned()]);
    }
}
