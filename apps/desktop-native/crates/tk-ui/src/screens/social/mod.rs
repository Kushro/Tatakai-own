//! AniList social (wave B, worker **n**; plan 14 section 0.1 #10: only what AniList offers; checklist `social.*`,
//! `details.reviews`): [`profile`] (`/profile` = the viewer, `/user/:name` = a public profile), [`notifications`]
//! (`/notifications`) and [`activity`] (`/activity`). All under `NavDest::Library`; the Library page links to them (n
//! asks the integrator for that one row). n owns every file here plus `components/reviews.rs` (the Details mount).
//! Without an AniList account (`Backend::list_accounts`), every page shows the "connect AniList" `EmptyState` with a
//! link to Settings > Accounts (`data-social="disconnected"`).

pub mod activity;
pub mod notifications;
pub mod profile;
