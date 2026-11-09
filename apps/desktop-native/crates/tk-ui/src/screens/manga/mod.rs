//! Manga (wave B, worker **l**; checklist `manga.*`; classic `pages/manga/*`): [`home`] (`/manga`), [`details`]
//! (`/manga/:id`) and [`reader`] (`/manga/:id/read/:ext_id/:chapter_id`, bare route: no rail). l owns every file here
//! and may add private ones with one `mod` line. Data: `Backend::manga_*`, `read_*` (shared with novels, m consumes
//! them read-only).

pub mod details;
pub mod home;
pub mod reader;
