//! Manga preferences and the reading view-models shared with novels (wave B, worker **l**;
//! docs/redesign/16-parity-wave-b-contract.md section 3.5). `MangaPrefs` is embedded in `Settings::manga`
//! (`SettingsAction::SetManga`). Worker l owns this file; `model/mod.rs` stays frozen. The chapter / progress
//! types here are used by worker m (novels) too: m never changes them.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::backend::{CharacterRoleVm, MediaVm, RelatedVm, SectionVm};
use crate::model::UiError;

pub const PREFETCH_MAX_PAGES: u8 = 10;
pub const ZOOM_MIN_PERCENT: u16 = 50;
pub const ZOOM_MAX_PERCENT: u16 = 400;
/// Page width of the vertical / webtoon modes, percent of the reader column.
pub const WIDTH_MIN_PERCENT: u8 = 30;
pub const WIDTH_MAX_PERCENT: u8 = 100;
/// Max page width in px (0 = no cap).
pub const MAX_WIDTH_PX_MAX: u16 = 4000;
/// Gap between pages in the vertical mode, px.
pub const GAP_MAX_PX: u8 = 48;
/// Page brightness, percent.
pub const BRIGHTNESS_MIN: u8 = 20;
pub const BRIGHTNESS_MAX: u8 = 100;
/// Auto-scroll speed steps.
pub const AUTO_SCROLL_MIN: u8 = 1;
pub const AUTO_SCROLL_MAX: u8 = 10;
/// Values of `MangaPrefs::title_language`: `""` follows `Settings::title_language`.
pub const MANGA_TITLE_LANGUAGES: [&str; 4] = ["", "english", "romaji", "native"];

/// Rebindable reader actions (classic `lib/reader/keybindings.ts`, minus the comments drawer). `id()` = i18n suffix
/// `manga.reader.action.<id>`, the key of `MangaReaderPrefs::keybinds` and `data-reader-shortcut`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReaderActionVm {
    NextPage,
    PrevPage,
    ScrollDown,
    ScrollUp,
    NextChapter,
    PrevChapter,
    Fullscreen,
    ToggleMode,
    ZoomIn,
    ZoomOut,
    Reload,
    AutoScroll,
    ToggleChrome,
}

impl ReaderActionVm {
    pub const ALL: [ReaderActionVm; 13] = [
        Self::NextPage,
        Self::PrevPage,
        Self::ScrollDown,
        Self::ScrollUp,
        Self::NextChapter,
        Self::PrevChapter,
        Self::Fullscreen,
        Self::ToggleMode,
        Self::ZoomIn,
        Self::ZoomOut,
        Self::Reload,
        Self::AutoScroll,
        Self::ToggleChrome,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::NextPage => "next_page",
            Self::PrevPage => "prev_page",
            Self::ScrollDown => "scroll_down",
            Self::ScrollUp => "scroll_up",
            Self::NextChapter => "next_chapter",
            Self::PrevChapter => "prev_chapter",
            Self::Fullscreen => "fullscreen",
            Self::ToggleMode => "toggle_mode",
            Self::ZoomIn => "zoom_in",
            Self::ZoomOut => "zoom_out",
            Self::Reload => "reload",
            Self::AutoScroll => "auto_scroll",
            Self::ToggleChrome => "toggle_chrome",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.id() == id)
    }

    /// Default key token (lowercase `KeyboardEvent.key`, `space` for the space bar).
    pub fn default_key(self) -> &'static str {
        match self {
            Self::NextPage => "arrowright",
            Self::PrevPage => "arrowleft",
            Self::ScrollDown => "arrowdown",
            Self::ScrollUp => "arrowup",
            Self::NextChapter => "n",
            Self::PrevChapter => "p",
            Self::Fullscreen => "f",
            Self::ToggleMode => "d",
            Self::ZoomIn => "=",
            Self::ZoomOut => "-",
            Self::Reload => "r",
            Self::AutoScroll => "a",
            Self::ToggleChrome => "h",
        }
    }
}

/// Key token of a `KeyboardEvent.key` (`" "` -> `space`, letters lowercase); `None` for bare modifiers and keys the
/// shell reserves (digits switch rail destinations, Escape leaves the reader).
pub fn key_token(key: &str) -> Option<String> {
    if key.is_empty() {
        return None;
    }
    if key == " " || key == "Spacebar" {
        return Some("space".to_owned());
    }
    let l = key.to_lowercase();
    if matches!(l.as_str(), "control" | "shift" | "alt" | "meta" | "escape" | "tab" | "dead" | "unidentified") {
        return None;
    }
    if l.len() == 1 && l.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(l)
}

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
    // --- added by l in wave B (all `serde(default)` through the struct attribute) ---
    /// Page width in the vertical / webtoon modes, percent `30..=100`.
    pub width_percent: u8,
    /// Max page width in px, `0` = no cap.
    pub max_width_px: u16,
    /// Gap between pages in the vertical mode, px `0..=48` (webtoon is always 0).
    pub gap_px: u8,
    /// Page brightness, percent `20..=100`.
    pub brightness: u8,
    /// Render every page at once (eager) instead of the current one +- `prefetch_pages` (lazy).
    pub eager_loading: bool,
    /// Vertical / webtoon: reaching the end opens the next chapter (`manga.reader.loading` infinite scroll).
    pub continuous_scroll: bool,
    /// Auto-scroll speed `1..=10` (`manga.reader.behavior`).
    pub auto_scroll_speed: u8,
    /// Ctrl + wheel changes the zoom.
    pub zoom_with_wheel: bool,
    /// Enter full screen when the reader opens.
    pub auto_fullscreen: bool,
    /// Hide the reader chrome while in full screen (it shows on mouse move).
    pub hide_chrome_fullscreen: bool,
    /// Thin progress bar at the bottom.
    pub show_progress_bar: bool,
    /// "Save page" button in the chrome (`manga.reader.behavior` page capture).
    pub show_capture_button: bool,
    /// Full-bleed pages: no side margins in vertical modes, height fit in paged ones (classic `panelCrop`).
    pub panel_crop: bool,
    /// Rebound keys: `ReaderActionVm::id()` -> key token; missing actions use `default_key`.
    pub keybinds: BTreeMap<String, String>,
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
            width_percent: 70,
            max_width_px: 0,
            gap_px: 8,
            brightness: 100,
            eager_loading: false,
            continuous_scroll: true,
            auto_scroll_speed: 3,
            zoom_with_wheel: true,
            auto_fullscreen: false,
            hide_chrome_fullscreen: true,
            show_progress_bar: true,
            show_capture_button: true,
            panel_crop: false,
            keybinds: BTreeMap::new(),
        }
    }
}

impl MangaReaderPrefs {
    pub fn sanitized(mut self) -> Self {
        self.prefetch_pages = self.prefetch_pages.min(PREFETCH_MAX_PAGES);
        self.zoom_percent = self.zoom_percent.clamp(ZOOM_MIN_PERCENT, ZOOM_MAX_PERCENT);
        self.width_percent = self.width_percent.clamp(WIDTH_MIN_PERCENT, WIDTH_MAX_PERCENT);
        self.max_width_px = self.max_width_px.min(MAX_WIDTH_PX_MAX);
        self.gap_px = self.gap_px.min(GAP_MAX_PX);
        self.brightness = self.brightness.clamp(BRIGHTNESS_MIN, BRIGHTNESS_MAX);
        self.auto_scroll_speed = self.auto_scroll_speed.clamp(AUTO_SCROLL_MIN, AUTO_SCROLL_MAX);
        // Unknown actions, invalid tokens and defaults are dropped; a key bound twice keeps the first action.
        let mut seen = std::collections::BTreeSet::new();
        let binds = std::mem::take(&mut self.keybinds);
        for action in ReaderActionVm::ALL {
            let Some(raw) = binds.get(action.id()) else { continue };
            let Some(tok) = key_token(raw).or_else(|| (raw == "space").then(|| raw.clone())) else { continue };
            if tok != action.default_key() && seen.insert(tok.clone()) {
                self.keybinds.insert(action.id().to_owned(), tok);
            }
        }
        self
    }

    /// The key bound to `action`.
    pub fn key_for(&self, action: ReaderActionVm) -> String {
        self.keybinds.get(action.id()).cloned().unwrap_or_else(|| action.default_key().to_owned())
    }

    /// The action bound to key `token` (rebinds first, then defaults not shadowed by a rebind).
    pub fn action_for(&self, token: &str) -> Option<ReaderActionVm> {
        ReaderActionVm::ALL.into_iter().find(|a| self.key_for(*a) == token)
    }

    /// `self` with `action` bound to `token`; the action that had `token` before gets its default back (or nothing
    /// when its default is `token` itself).
    pub fn with_binding(&self, action: ReaderActionVm, token: &str) -> Self {
        let mut next = self.clone();
        if let Some(other) = self.action_for(token).filter(|o| *o != action) {
            next.keybinds.remove(other.id());
            if other.default_key() == token {
                next.keybinds.insert(other.id().to_owned(), self.key_for(action));
            }
        }
        next.keybinds.insert(action.id().to_owned(), token.to_owned());
        next.sanitized()
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
    /// Title language of manga cards and details, separate from the anime one (`manga.title-language`): one of
    /// [`MANGA_TITLE_LANGUAGES`]; `""` follows `Settings::title_language`.
    pub title_language: String,
}

impl Default for MangaPrefs {
    fn default() -> Self {
        Self {
            reader: MangaReaderPrefs::default(),
            preferred_lang: "es".to_owned(),
            preferred_source: None,
            auto_best_source: true,
            adult_ok: false,
            title_language: String::new(),
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
        if !MANGA_TITLE_LANGUAGES.contains(&self.title_language.as_str()) {
            self.title_language = String::new();
        }
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
    // --- added by l in wave B (only l constructs this type) ---
    /// `(tag, rank 0..=100)`, spoilers excluded.
    pub tags: Vec<(String, u8)>,
    pub characters: Vec<CharacterRoleVm>,
    pub recommendations: Vec<MediaVm>,
    /// Series tree: related manga / novels / anime (`RelatedVm::kind` = `manga` | `anime`).
    pub relations: Vec<RelatedVm>,
    /// Display-ready facts `(i18n key suffix of manga.details.fact.<id>, value)`: start / end, origin, publisher,
    /// serialisation, publication status (MangaUpdates), licensed, popularity, favourites.
    pub facts: Vec<(String, String)>,
    /// Alternative titles.
    pub synonyms: Vec<String>,
    /// AniList / MangaDex / MangaUpdates ids as `(site, id)` for the "open on" links.
    pub source_ids: Vec<(String, String)>,
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
    fn keybinds_rebind_and_swap() {
        let p = MangaReaderPrefs::default();
        assert_eq!(p.action_for("arrowright"), Some(ReaderActionVm::NextPage));
        let q = p.with_binding(ReaderActionVm::NextPage, "l");
        assert_eq!(q.key_for(ReaderActionVm::NextPage), "l");
        assert_eq!(q.action_for("arrowright"), None);
        // taking the key of another action hands that action the old key
        let r = q.with_binding(ReaderActionVm::PrevPage, "l");
        assert_eq!(r.action_for("l"), Some(ReaderActionVm::PrevPage));
        assert_eq!(r.key_for(ReaderActionVm::NextPage), "arrowright");
        // binding the default again leaves no entry
        let back = r.with_binding(ReaderActionVm::PrevPage, "arrowleft");
        assert!(back.keybinds.is_empty() || back.keybinds.values().all(|k| k != "arrowleft"));
    }

    #[test]
    fn key_tokens() {
        assert_eq!(key_token(" ").as_deref(), Some("space"));
        assert_eq!(key_token("ArrowRight").as_deref(), Some("arrowright"));
        assert_eq!(key_token("Shift"), None);
        assert_eq!(key_token("5"), None);
        assert_eq!(key_token("Escape"), None);
        for a in ReaderActionVm::ALL {
            assert_eq!(ReaderActionVm::from_id(a.id()), Some(a));
            assert_eq!(key_token(a.default_key()).as_deref(), Some(a.default_key()));
        }
    }

    #[test]
    fn sanitize_drops_bad_binds_and_clamps() {
        let mut p = MangaReaderPrefs { width_percent: 5, brightness: 0, gap_px: 200, auto_scroll_speed: 0, ..MangaReaderPrefs::default() };
        p.keybinds.insert("bogus".into(), "x".into());
        p.keybinds.insert("reload".into(), "7".into());
        p.keybinds.insert("next_page".into(), "j".into());
        p.keybinds.insert("prev_page".into(), "j".into());
        let s = p.sanitized();
        assert_eq!(s.width_percent, WIDTH_MIN_PERCENT);
        assert_eq!(s.brightness, BRIGHTNESS_MIN);
        assert_eq!(s.gap_px, GAP_MAX_PX);
        assert_eq!(s.auto_scroll_speed, AUTO_SCROLL_MIN);
        assert_eq!(s.keybinds.len(), 1);
        assert_eq!(s.keybinds.get("next_page").map(String::as_str), Some("j"));
        let t = MangaPrefs { title_language: "klingon".into(), ..MangaPrefs::default() }.sanitized();
        assert_eq!(t.title_language, "");
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
