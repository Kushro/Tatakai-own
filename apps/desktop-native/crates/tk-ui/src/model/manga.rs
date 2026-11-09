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


// ---------------------------------------------------------------------------------------------
// Conventions shared by tk-app's `live/manga.rs` and the manga screens (both owned by l)
// ---------------------------------------------------------------------------------------------

/// Prefix of `MediaVm::subtitle` of an 18+ manga card (tk-app's manga mappers set it); the UI blurs or gates such
/// cards according to `Settings::home.mature_content` (`manga.adult-gate`).
pub const ADULT_BADGE: &str = "18+";
/// `SearchQueryVm::tags` entry that makes `Backend::manga_search` return 18+ titles only (the 18+ zone of the manga
/// home). Never sent to AniList as a tag.
pub const ADULT_ONLY_TAG: &str = "@adult-only";
/// Origins of a manga search (AniList `countryOfOrigin`, lowercase). A manga search carries the origin in
/// `SearchQueryVm::season` (anime seasons do not apply to manga). `jp` = manga, `kr` = manhwa, `cn` / `tw` = manhua.
pub const MANGA_ORIGINS: [&str; 4] = ["jp", "kr", "cn", "tw"];
/// Publication statuses of a manga search (`SearchQueryVm::status`); i18n `manga.pub.<id>`.
pub const MANGA_STATUSES: [&str; 5] = ["releasing", "finished", "hiatus", "cancelled", "upcoming"];
/// Formats of a manga search (`SearchQueryVm::format`, AniList ids); i18n `manga.format.<lowercase id>`.
pub const MANGA_FORMATS: [&str; 2] = ["MANGA", "ONE_SHOT"];
/// Chapter languages offered in Settings > Manga reader (ISO 639-1).
pub const MANGA_LANGS: [&str; 11] = ["es", "en", "pt", "fr", "it", "de", "ru", "id", "ja", "ko", "zh"];
/// Built-in MangaDex chapter provider (`ChapterRefVm::ext_id`), next to the installed extensions.
pub const MANGADEX_PROVIDER: &str = "@mangadex";
/// `PageVm::url` prefix of a page stored on disk (offline chapter, `manga.download.offline-read`).
pub const OFFLINE_URL_PREFIX: &str = "file://";

/// Whether a card is an 18+ title (see [`ADULT_BADGE`]).
pub fn is_adult_card(m: &MediaVm) -> bool {
    m.subtitle.starts_with(ADULT_BADGE)
}

/// Whether every page of a chapter comes from disk.
pub fn pages_are_offline(pages: &[PageVm]) -> bool {
    !pages.is_empty() && pages.iter().all(|p| p.url.starts_with(OFFLINE_URL_PREFIX))
}

/// `12`, `12.5`; `?` when unknown.
pub fn chapter_label(n: f32) -> String {
    if n <= 0.0 {
        return "?".to_owned();
    }
    if (n - n.round()).abs() < 0.005 { format!("{}", n.round() as i64) } else { format!("{:.1}", n).trim_end_matches('0').to_owned() }
}

/// Same chapter number (fractional chapters compare to the hundredth).
pub fn same_chapter(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.005
}

/// One row of the merged chapter list: the chosen source of a chapter number and how many sources exist.
#[derive(Debug, Clone, PartialEq)]
pub struct MergedChapterVm {
    pub number: f32,
    /// The chapter of the chosen source.
    pub chapter: ChapterVm,
    /// `ChapterProviderVm::ext_id` of the chosen source.
    pub provider: String,
    /// Sources that have this chapter.
    pub sources: usize,
}

/// Score of a chapter of the provider at `rank` (of `count`; tk-app orders providers by reliability, best first):
/// preferred source, then preferred language, English as the universal fallback, then the provider's rank.
fn chapter_score(provider: &ChapterProviderVm, rank: usize, count: usize, ch: &ChapterVm, lang: &str, source: Option<&str>) -> f32 {
    let mut s = 1.0 - rank as f32 / count.max(1) as f32;
    if source.is_some_and(|src| src == provider.ext_id || src == ch.reference.ext_id) {
        s += 100.0;
    }
    match ch.lang.as_deref() {
        Some(l) if l == lang => s += 10.0,
        Some("en") => s += 3.0,
        None => s += 1.0,
        _ => {}
    }
    if ch.group.is_some() {
        s += 0.1;
    }
    s
}

/// One row per chapter number across `providers` (ascending), each with its best source for `lang` / `source`
/// (`manga.chapters.auto-best`). Chapters numbered 0 (unknown) are appended per provider as they come.
pub fn merge_chapters(providers: &[ChapterProviderVm], lang: &str, source: Option<&str>) -> Vec<MergedChapterVm> {
    let count = providers.len();
    let mut best: Vec<(f32, MergedChapterVm)> = Vec::new();
    let mut unknown = Vec::new();
    for (rank, p) in providers.iter().enumerate() {
        for ch in &p.chapters {
            if ch.number <= 0.0 {
                unknown.push(MergedChapterVm { number: 0.0, chapter: ch.clone(), provider: p.ext_id.clone(), sources: 1 });
                continue;
            }
            let sc = chapter_score(p, rank, count, ch, lang, source);
            match best.iter_mut().find(|(_, m)| same_chapter(m.number, ch.number)) {
                Some((s, m)) => {
                    m.sources += 1;
                    if sc > *s {
                        *s = sc;
                        m.chapter = ch.clone();
                        m.provider = p.ext_id.clone();
                    }
                }
                None => best.push((sc, MergedChapterVm { number: ch.number, chapter: ch.clone(), provider: p.ext_id.clone(), sources: 1 })),
            }
        }
    }
    best.sort_by(|a, b| a.1.number.total_cmp(&b.1.number));
    best.into_iter().map(|(_, m)| m).chain(unknown).collect()
}

/// Best source of chapter `number` skipping the providers in `exclude` (the reader's "try another source" after a
/// failure, `manga.reader.provider-switch`): `(provider key, chapter)`.
pub fn best_source(providers: &[ChapterProviderVm], number: f32, lang: &str, source: Option<&str>, exclude: &[String]) -> Option<(String, ChapterVm)> {
    let count = providers.len();
    providers
        .iter()
        .enumerate()
        .filter(|(_, p)| !exclude.contains(&p.ext_id))
        .flat_map(|(rank, p)| {
            p.chapters
                .iter()
                .filter(move |c| c.number > 0.0 && same_chapter(c.number, number))
                .map(move |c| (chapter_score(p, rank, count, c, lang, source), p.ext_id.clone(), c.clone()))
        })
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, k, c)| (k, c))
}

/// Every source of chapter `number`: `(provider key, provider name, chapter)` in provider order.
pub fn sources_of(providers: &[ChapterProviderVm], number: f32) -> Vec<(String, String, ChapterVm)> {
    providers
        .iter()
        .flat_map(|p| {
            p.chapters
                .iter()
                .filter(move |c| c.number > 0.0 && same_chapter(c.number, number))
                .map(move |c| (p.ext_id.clone(), p.ext_name.clone(), c.clone()))
        })
        .collect()
}

/// Index of the best provider of a title: preferred source, then most chapters in `lang`, then most chapters, then
/// the backend's order (`data-best`).
pub fn best_provider(providers: &[ChapterProviderVm], lang: &str, source: Option<&str>) -> Option<usize> {
    providers
        .iter()
        .enumerate()
        .filter(|(_, p)| p.error.is_none() && !p.chapters.is_empty())
        .max_by_key(|(i, p)| {
            let preferred = source.is_some_and(|s| s == p.ext_id || p.chapters.first().is_some_and(|c| c.reference.ext_id == s));
            let in_lang = p.chapters.iter().filter(|c| c.lang.as_deref() == Some(lang)).count();
            (preferred, in_lang, p.chapters.len(), std::cmp::Reverse(*i))
        })
        .map(|(i, _)| i)
}

/// The chapter after / before `number` in a merged, ascending list (numbered chapters only).
pub fn neighbour_chapter(merged: &[MergedChapterVm], number: f32, forward: bool) -> Option<&MergedChapterVm> {
    if forward {
        merged.iter().filter(|c| c.number > 0.0).find(|c| c.number > number + 0.004)
    } else {
        merged.iter().filter(|c| c.number > 0.0).rev().find(|c| c.number < number - 0.004)
    }
}

/// Chapters grouped by volume in order of appearance (`None` = no volume), for the "group by volume" tool.
pub fn group_by_volume<T: Clone>(rows: &[T], volume: impl Fn(&T) -> Option<u32>) -> Vec<(Option<u32>, Vec<T>)> {
    let mut out: Vec<(Option<u32>, Vec<T>)> = Vec::new();
    for r in rows {
        let v = volume(r);
        match out.iter_mut().find(|(k, _)| *k == v) {
            Some((_, list)) => list.push(r.clone()),
            None => out.push((v, vec![r.clone()])),
        }
    }
    out
}

/// Chapter filter of the details tools: matches the number (`12`, `12.5`), the title or the group, case-insensitive.
pub fn chapter_matches(ch: &ChapterVm, needle: &str) -> bool {
    let n = needle.trim().to_lowercase();
    if n.is_empty() {
        return true;
    }
    chapter_label(ch.number) == n
        || ch.title.as_deref().is_some_and(|t| t.to_lowercase().contains(&n))
        || ch.group.as_deref().is_some_and(|g| g.to_lowercase().contains(&n))
}

/// Pages to render around `current` in lazy mode (`prefetch` ahead, 1 behind); every page in eager mode.
pub fn render_window(total: usize, current: usize, prefetch: u8, eager: bool) -> std::ops::Range<usize> {
    if eager || total == 0 {
        return 0..total;
    }
    let start = current.saturating_sub(1);
    let end = (current + 1 + usize::from(prefetch)).min(total);
    start.min(end)..end
}

/// Whether reading position `index` of `total` passed the 70 % mark that prefetches the next chapter
/// (`manga.reader.prefetch`).
pub fn should_prefetch_next(index: usize, total: usize) -> bool {
    total > 0 && (index + 1) * 10 >= total * 7
}

/// Pages shown together in the double-page mode: `(first, Some(second))` spreads; the first page (cover) and wide
/// pages (width > height) stand alone.
pub fn spreads(pages: &[PageVm]) -> Vec<(usize, Option<usize>)> {
    let wide = |p: &PageVm| matches!((p.width, p.height), (Some(w), Some(h)) if w > h);
    let mut out = Vec::new();
    let mut i = 0;
    while i < pages.len() {
        if i == 0 || wide(&pages[i]) || i + 1 >= pages.len() || wide(&pages[i + 1]) {
            out.push((i, None));
            i += 1;
        } else {
            out.push((i, Some(i + 1)));
            i += 2;
        }
    }
    out
}

/// Index of the spread that shows page `page`.
pub fn spread_of(spreads: &[(usize, Option<usize>)], page: usize) -> usize {
    spreads.iter().position(|(a, b)| *a == page || *b == Some(page)).unwrap_or(0)
}

/// Read fraction of a chapter from its progress (`position` is 0-based).
pub fn read_fraction(p: &ReadProgressVm) -> f64 {
    if p.completed {
        return 1.0;
    }
    match p.total {
        Some(t) if t > 0 => (f64::from(p.position + 1) / f64::from(t)).clamp(0.0, 1.0),
        _ => 0.0,
    }
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

    fn ch(ext: &str, id: &str, n: f32, lang: Option<&str>) -> ChapterVm {
        ChapterVm {
            reference: ChapterRefVm { ext_id: ext.into(), chapter_id: id.into() },
            number: n,
            volume: if n < 3.0 { Some(1) } else { None },
            title: Some(format!("Chapter {n}")),
            lang: lang.map(str::to_owned),
            group: None,
            published_unix: None,
            pages: None,
            read: 0.0,
            downloaded: false,
        }
    }

    fn providers() -> Vec<ChapterProviderVm> {
        vec![
            ChapterProviderVm {
                ext_id: "toko/comick".into(),
                ext_name: "Toko".into(),
                lang: Some("en".into()),
                chapters: vec![ch("toko", "c1", 1.0, Some("en")), ch("toko", "c2", 2.0, Some("en")), ch("toko", "c3", 3.0, Some("en"))],
                error: None,
            },
            ChapterProviderVm {
                ext_id: "@mangadex".into(),
                ext_name: "MangaDex".into(),
                lang: Some("es".into()),
                chapters: vec![ch("@mangadex", "m1", 1.0, Some("es")), ch("@mangadex", "m15", 1.5, Some("es"))],
                error: None,
            },
        ]
    }

    #[test]
    fn merge_picks_language_then_source() {
        let p = providers();
        let m = merge_chapters(&p, "es", None);
        assert_eq!(m.iter().map(|c| c.number).collect::<Vec<_>>(), vec![1.0, 1.5, 2.0, 3.0]);
        assert_eq!(m[0].chapter.reference.chapter_id, "m1");
        assert_eq!(m[0].sources, 2);
        let forced = merge_chapters(&p, "es", Some("toko/comick"));
        assert_eq!(forced[0].provider, "toko/comick");
        assert_eq!(best_source(&p, 1.0, "es", None, &["@mangadex".to_owned()]).map(|(k, _)| k).as_deref(), Some("toko/comick"));
        assert_eq!(best_source(&p, 9.0, "es", None, &[]), None);
        assert_eq!(sources_of(&p, 1.0).len(), 2);
        assert_eq!(best_provider(&p, "es", None), Some(1));
        assert_eq!(best_provider(&p, "en", None), Some(0));
        assert_eq!(neighbour_chapter(&m, 1.5, true).map(|c| c.number), Some(2.0));
        assert_eq!(neighbour_chapter(&m, 1.0, false).map(|c| c.number), None);
    }

    #[test]
    fn labels_groups_and_filters() {
        assert_eq!(chapter_label(12.0), "12");
        assert_eq!(chapter_label(12.5), "12.5");
        assert_eq!(chapter_label(0.0), "?");
        let p = providers();
        let groups = group_by_volume(&p[0].chapters, |c| c.volume);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].0, Some(1));
        assert_eq!(groups[0].1.len(), 2);
        assert!(chapter_matches(&p[0].chapters[1], "2"));
        assert!(chapter_matches(&p[0].chapters[1], "chapter 2"));
        assert!(!chapter_matches(&p[0].chapters[1], "zzz"));
    }

    #[test]
    fn reader_windows_spreads_and_prefetch() {
        assert_eq!(render_window(20, 5, 3, false), 4..9);
        assert_eq!(render_window(20, 0, 3, false), 0..4);
        assert_eq!(render_window(20, 19, 3, false), 18..20);
        assert_eq!(render_window(20, 5, 3, true), 0..20);
        assert!(!should_prefetch_next(5, 10));
        assert!(should_prefetch_next(6, 10));
        let page = |w, h| PageVm { index: 0, url: String::new(), headers: Vec::new(), width: Some(w), height: Some(h) };
        let pages = vec![page(800, 1200), page(800, 1200), page(800, 1200), page(1600, 1200), page(800, 1200)];
        let s = spreads(&pages);
        assert_eq!(s, vec![(0, None), (1, Some(2)), (3, None), (4, None)]);
        assert_eq!(spread_of(&s, 2), 1);
        let off = PageVm { url: format!("{OFFLINE_URL_PREFIX}/tmp/1.jpg"), ..page(1, 1) };
        assert!(pages_are_offline(&[off.clone()]));
        assert!(!pages_are_offline(&[off, page(1, 1)]));
        let prog = ReadProgressVm { chapter: ChapterRefVm { ext_id: "e".into(), chapter_id: "c".into() }, chapter_number: 1.0, position: 4, total: Some(10), completed: false, updated_unix: 0 };
        assert!((read_fraction(&prog) - 0.5).abs() < 1e-9);
        let card = MediaVm { id: "1".into(), title: "x".into(), cover: None, color: None, subtitle: format!("{ADULT_BADGE} - 2020"), score: None };
        assert!(is_adult_card(&card));
    }
}
