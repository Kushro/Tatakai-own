//! Screens routed by `app::Route`.

pub mod browse;
/// Airing calendar page (wave B, worker b).
pub mod calendar;
pub mod character;
pub mod details;
/// Downloads page (wave B, worker h).
pub mod downloads;
pub mod extensions;
pub mod gallery;
pub mod home;
pub mod image_search;
pub mod library;
pub mod link;
/// Manga home / details / reader (wave B, worker l).
pub mod manga;
/// Novel home / details / reader + TTS (wave B, worker m).
pub mod novel;
pub mod player;
/// Custom provider page of an extension (wave B, worker j).
pub mod provider;
/// "For you" recommendations + Anime Atlas (wave B, worker b).
pub mod recs;
pub mod search;
/// Native extension server page (wave B, worker k).
pub mod server;
pub mod settings;
/// First-run wizard (wave A, worker f).
pub mod setup;
/// AniList social pages (wave B, worker n).
pub mod social;
pub mod studio;
pub mod torrents;
