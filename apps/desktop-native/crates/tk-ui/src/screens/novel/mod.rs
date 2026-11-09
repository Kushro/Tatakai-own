//! Novels (wave B, worker **m**; checklist `novel.*`; classic `pages/novel/*`): [`home`] (`/novels`), [`details`]
//! (`/novels/:id`) and [`reader`] (`/novels/:id/read/:ext_id/:chapter_id`, bare route). m owns every file here and
//! may add private ones (`tts_bar.rs`, `settings_sheet.rs`) with one `mod` line. Data: `Backend::novel_*`, `tts_*`,
//! `kitten_*`, and the shared `read_*` calls of worker l (read-only use of `model/manga.rs` types).

pub mod details;
pub mod home;
pub mod reader;
