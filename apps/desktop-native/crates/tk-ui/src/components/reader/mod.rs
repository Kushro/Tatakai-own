//! Manga reader widgets (wave B, worker **l**; checklist `manga.reader.*`; classic `components/reader/*`:
//! `ReaderChrome`, `ReaderPageRail`, `ReaderImage`). Owned by l: it adds private files here (`page.rs`, `rail.rs`,
//! `chrome.rs`, `settings_sheet.rs`) with one `mod` line each. Only this `mod.rs` is registered by the contract.
//! Worker m (novels) does not use these: its reader is text (`screens/novel/reader.rs`).
//!
//! Shared state the widgets receive as props: `MangaReaderPrefs` (defaults from `Settings::manga.reader`, per-title
//! override from the reader page), the `Vec<PageVm>` of the chapter and the current index.
