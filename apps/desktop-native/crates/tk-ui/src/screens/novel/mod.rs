//! Novels (wave B, worker **m**; checklist `novel.*`; classic `pages/novel/*`): [`home`] (`/novels`), [`details`]
//! (`/novels/:id`) and [`reader`] (`/novels/:id/read/:ext_id/:chapter_id`, bare route). m owns every file here:
//! [`controls`] (typography / flow controls shared by the reader sheet and Settings > Novel reader) and [`assets`]
//! (the `tk-novel` asset handler serving narration clips and the custom font, plus the audio / wake-lock JS).
//! Data: `Backend::novel_*`, `tts_*`, `kitten_*`, and the shared `read_*` calls of worker l (read-only use of
//! `model/manga.rs` types).

pub mod assets;
pub mod controls;
pub mod details;
pub mod home;
pub mod reader;
