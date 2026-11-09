//! Manga preferences and the reading view-models shared with novels (wave B, worker **l**;
//! docs/redesign/16-parity-wave-b-contract.md section 3.5). `MangaPrefs` is embedded in `Settings::manga`
//! (`SettingsAction::SetManga`). Worker l owns this file; `model/mod.rs` stays frozen. The chapter / progress
//! types here are used by worker m (novels) too: m never changes them.

use serde::{Deserialize, Serialize};

use crate::backend::{MediaVm, SectionVm};
use crate::model::UiError;

pub const PREFETCH_MAX_PAGES: u8 = 10;
pub const ZOOM_MIN_PERCENT: u16 = 50;
pub const ZOOM_MAX_PERCENT: u16 = 400;

/// Reader layout (classic `components/reader/*`). `id()` = i18n suffix `manga.reader.mode.<id>`, `data-mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReaderModeVm {
    #[default]
    Paged,
    DoublePage,
    Vertical,
    Webtoon,
}

impl ReaderModeVm {
    pub const ALL: [ReaderModeVm; 4] = [Self::Paged, Self::DoublePage, Self::Vertical, Self::Webtoon];

    pub fn id(self) -> &'static str {
        match self {
            Self::Paged => "paged",
            Self::DoublePage => "double",
            Self::Vertical => "vertical",
            Self::Webtoon => "webtoon",
        }
    }
}

/// How a page fits the viewport. `id()` = `manga.reader.fit.<id>`, `data-fit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReaderFitVm {
    #[default]
    Width,
    Height,
    Screen,
    Original,
}

impl ReaderFitVm {
    pub const ALL: [ReaderFitVm; 4] = [Self::Width, Self::Height, Self::Screen, Self::Original];

    pub fn id(self) -> &'static str {
        match self {
            Self::Width => "width",
            Self::Height => "height",
            Self::Screen => "screen",
            Self::Original => "original",
        }
    }
}

/// Page order in paged modes. `id()` = `manga.reader.direction.<id>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReaderDirectionVm {
    #[default]
    Rtl,
    Ltr,
}

impl ReaderDirectionVm {
    pub const ALL: [ReaderDirectionVm; 2] = [Self::Rtl, Self::Ltr];

    pub fn id(self) -> &'static str {
        match self {
            Self::Rtl => "rtl",
            Self::Ltr => "ltr",
        }
    }
}

/// Reader background. `id()` = `manga.reader.background.<id>`; the CSS maps it to tokens, never to literals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReaderBackgroundVm {
    #[default]
    Black,
    Gray,
    White,
}

impl ReaderBackgroundVm {
    pub const ALL: [ReaderBackgroundVm; 3] = [Self::Black, Self::Gray, Self::White];

    pub fn id(self) -> &'static str {
        match self {
            Self::Black => "black",
            Self::Gray => "gray",
            Self::White => "white",
        }
    }
}

/// `MangaPrefs::reader`: defaults of the reader (each title may override them in the reader's settings sheet; the
/// override is stored by worker l in `data.db`, not here).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MangaReaderPrefs {
    pub mode: ReaderModeVm,
    pub fit: ReaderFitVm,
    pub direction: ReaderDirectionVm,
    pub background: ReaderBackgroundVm,
    /// Pages fetched ahead of the current one, `0..=10`.
    pub prefetch_pages: u8,
    /// Left / right third of the page turn pages (else only keys and the rail).
    pub tap_zones: bool,
    pub show_page_number: bool,
    /// Zoom in `Original` fit, percent `50..=400`.
    pub zoom_percent: u16,
    /// Prefetch the next chapter's first pages near the end of the current one (`manga.reader.prefetch`).
    pub prefetch_next_chapter: bool,
}

impl Default for MangaReaderPrefs {
    fn default() -> Self {
        Self {
            mode: ReaderModeVm::Paged,
            fit: ReaderFitVm::Width,
            direction: ReaderDirectionVm::Rtl,
            background: ReaderBackgroundVm::Black,
            prefetch_pages: 3,
            tap_zones: true,
            show_page_number: true,
            zoom_percent: 100,
            prefetch_next_chapter: true,
        }
    }
}

impl MangaReaderPrefs {
    pub fn sanitized(mut self) -> Self {
        self.prefetch_pages = self.prefetch_pages.min(PREFETCH_MAX_PAGES);
        self.zoom_percent = self.zoom_percent.clamp(ZOOM_MIN_PERCENT, ZOOM_MAX_PERCENT);
        self
    }
}

/// `Settings::manga`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MangaPrefs {
    pub reader: MangaReaderPrefs,
    /// Preferred chapter language (ISO 639-1) when several providers / scanlations exist.
    pub preferred_lang: String,
    /// Preferred extension id for chapters; `None` = best available.
    pub preferred_source: Option<String>,
    /// Pick the best provider per chapter automatically (`manga.chapters.auto-best`).
    pub auto_best_source: bool,
    /// Show 18+ titles without the gate (`manga.adult-gate`).
    pub adult_ok: bool,
}

impl Default for MangaPrefs {
    fn default() -> Self {
        Self {
            reader: MangaReaderPrefs::default(),
            preferred_lang: "es".to_owned(),
            preferred_source: None,
            auto_best_source: true,
            adult_ok: false,
        }
    }
}

impl MangaPrefs {
    pub fn sanitized(mut self) -> Self {
        self.reader = self.reader.sanitized();
        self.preferred_lang = self.preferred_lang.trim().to_ascii_lowercase();
        if self.preferred_lang.is_empty() {
            self.preferred_lang = "es".to_owned();
        }
        self.preferred_source = self.preferred_source.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
        self
    }
}

/// Manga or novel (the `read_*` calls and the `read_progress` / `read_list_entries` tables take it). `id()` =
/// `data-kind` and the table column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReadKindVm {
    Manga,
    Novel,
}

impl ReadKindVm {
    pub const ALL: [ReadKindVm; 2] = [Self::Manga, Self::Novel];

    pub fn id(self) -> &'static str {
        match self {
            Self::Manga => "manga",
            Self::Novel => "novel",
        }
    }
}

/// A chapter of one provider: the pair every chapter call takes. `chapter_id` is the extension's own id (opaque);
/// routes carry both URL-encoded.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChapterRefVm {
    pub ext_id: String,
    pub chapter_id: String,
}

/// One chapter row (classic per-provider chapter lists).
#[derive(Debug, Clone, PartialEq)]
pub struct ChapterVm {
    pub reference: ChapterRefVm,
    /// Chapter number (fractional for extras: `12.5`); 0.0 when unknown.
    pub number: f32,
    pub volume: Option<u32>,
    pub title: Option<String>,
    /// ISO 639-1 lowercase.
    pub lang: Option<String>,
    /// Scanlation group / translator.
    pub group: Option<String>,
    pub published_unix: Option<i64>,
    /// Pages (manga) or paragraphs (novel) when the provider says.
    pub pages: Option<u32>,
    /// 0.0..=1.0 from `read_progress`.
    pub read: f64,
    /// Available offline (`offline_chapters`).
    pub downloaded: bool,
}

/// Chapters of one provider for a title; a failed provider carries its error so the others still show.
#[derive(Debug, Clone, PartialEq)]
pub struct ChapterProviderVm {
    pub ext_id: String,
    pub ext_name: String,
    pub lang: Option<String>,
    pub chapters: Vec<ChapterVm>,
    pub error: Option<UiError>,
}

/// One manga page (`pages` route of the extension, or the offline file served by the app).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageVm {
    pub index: u32,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// Reading position of a title (`read_progress` table): `position` is a page (manga) or paragraph (novel) index.
#[derive(Debug, Clone, PartialEq)]
pub struct ReadProgressVm {
    pub chapter: ChapterRefVm,
    pub chapter_number: f32,
    pub position: u32,
    pub total: Option<u32>,
    pub completed: bool,
    pub updated_unix: i64,
}

/// "Continue reading" card (Manga / Novels home, classic `LastReadMangaSection` / `LastReadNovelSection`).
#[derive(Debug, Clone, PartialEq)]
pub struct ContinueReadVm {
    pub media: MediaVm,
    pub kind: ReadKindVm,
    pub progress: ReadProgressVm,
}

/// Manga / Novels home: continue reading + feed rows (`SectionVm::title_key` = `manga.feed.<id>` /
/// `novel.feed.<id>`; rows degrade on their own like Home).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReadHomeVm {
    pub continue_reading: Vec<ContinueReadVm>,
    pub sections: Vec<SectionVm>,
}

/// Manga details (`manga.details`): metadata router (AniList, MangaDex, MangaUpdates) + local progress.
#[derive(Debug, Clone, PartialEq)]
pub struct MangaDetailsVm {
    pub media: MediaVm,
    pub banner: Option<String>,
    pub synopsis: Option<String>,
    pub genres: Vec<String>,
    pub status: Option<String>,
    /// `MANGA`, `ONE_SHOT`, `MANHWA` ... display-ready.
    pub format: Option<String>,
    pub chapters_total: Option<u32>,
    pub volumes_total: Option<u32>,
    pub authors: Vec<String>,
    /// Adaptation (`manga.details.anime-link`): the anime's `MediaVm::id`.
    pub anime_id: Option<String>,
    /// `(site, url)`.
    pub external: Vec<(String, String)>,
    /// `(site, score 0..=100)` cross-site ratings.
    pub ratings: Vec<(String, u32)>,
    pub adult: bool,
    pub resume: Option<ReadProgressVm>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sanitized() {
        let d = MangaPrefs::default();
        assert_eq!(d.clone().sanitized(), d);
    }

    #[test]
    fn bounds() {
        let p = MangaPrefs {
            reader: MangaReaderPrefs { prefetch_pages: 99, zoom_percent: 1, ..MangaReaderPrefs::default() },
            preferred_lang: " ".into(),
            ..MangaPrefs::default()
        }
        .sanitized();
        assert_eq!(p.reader.prefetch_pages, PREFETCH_MAX_PAGES);
        assert_eq!(p.reader.zoom_percent, ZOOM_MIN_PERCENT);
        assert_eq!(p.preferred_lang, "es");
    }
}
