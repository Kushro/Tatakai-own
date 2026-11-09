//! Manga chapters across providers (wave B, worker **l**; docs/redesign/16-parity-wave-b-contract.md section 3.5;
//! checklist `manga.chapters.providers`, `manga.chapters.auto-best`, `manga.metadata.*`). UI-free. l owns this
//! directory and adds private files. Final public shape: `ChapterList { ext_id, lang, chapters: Vec<Chapter> }`,
//! `merge(lists) -> Vec<Chapter>` (dedupe by number, keep the preferred language / source first),
//! `pick_best(lists, preferred_lang, preferred_source, reliability) -> Option<(ext_id, chapter_id)>`.
//! Metadata providers: `crate::metadata::{mangadex, mangaupdates}` (manga-only `MetadataProvider`s registered by
//! `MetadataRouter::with_defaults`) and `crate::metadata::anilist::manga` (AniList `MANGA` queries incl. `NOVEL`
//! format for worker m). Rows: `store/reading.rs`.

/// One chapter of one provider (the extension's own `chapter_id` is opaque).
#[derive(Debug, Clone, PartialEq)]
pub struct Chapter {
    pub ext_id: String,
    pub chapter_id: String,
    pub number: f32,
    pub volume: Option<u32>,
    pub title: Option<String>,
    pub lang: Option<String>,
    pub group: Option<String>,
    pub published_unix: Option<i64>,
    pub pages: Option<u32>,
}
