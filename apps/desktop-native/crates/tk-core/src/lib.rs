//! tk-core: domain model, metadata router (AniList + ani.zip + Kitsu), local SQLite store (history, lists,
//! caches), secrets and `config.json` loading for the native desktop app.
//!
//! Wave A (docs/redesign/14-parity-plan.md section 0.1 #9): there is no own login any more. The Supabase client
//! is gone; identity is AniList / MAL OAuth (`lists`), the watch history stays local and syncs between the user's
//! own devices over Tatakai Link (`tk-link::history`).
//!
//! Everything here is UI-free and free of global state: paths and clocks are injected so that
//! tests never touch the real user profile.

pub mod config;
/// Debrid services as torrent sources (wave A, worker i).
pub mod debrid;
/// HLS / MP4 download engine + local library (wave B, worker h).
pub mod download;
pub mod error;
/// Client of a paired extension server (`/api/server/v1`; wave B, worker j).
pub mod extserver;
/// AniList / MyAnimeList lists: OAuth, read / write, offline queue (wave A, worker g).
pub mod lists;
/// Manga chapters per provider, best-source pick (wave B, worker l).
pub mod manga;
pub mod metadata;
pub mod model;
pub mod net;
/// Novel chapter text (wave B, worker m).
pub mod novel;
/// Local recommendation engine: taste profile, scoring, atlas projection (wave B, worker b).
pub mod recs;
pub mod release;
/// OS keyring with a file fallback for OAuth tokens, translation and debrid keys (wave A, worker f).
pub mod secrets;
pub mod sources;
pub mod store;
/// Stremio add-ons as stream sources (wave B, worker j).
pub mod stremio;
/// trace.moe scene search (wave A, worker a).
pub mod tracemoe;
/// Subtitle translation providers (wave A, worker d).
pub mod translate;

pub use config::AppConfig;
pub use error::{DebridError, DownloadError, ExtServerError, ListError, ProviderError, ReadError, SecretError, StoreError, TranslateError};
pub use secrets::Secrets;
pub use metadata::{MetadataProvider, MetadataRouter};
pub use model::*;
pub use store::LocalStore;

/// Directory name under `%APPDATA%` used by the native app (06 #6).
pub const APP_DIR_NAME: &str = "Tatakai-Next";

/// `%APPDATA%\Tatakai-Next` (Roaming). Resolved by the *app* at start-up; tests must pass tempdirs instead.
pub fn default_data_dir() -> Option<std::path::PathBuf> {
    directories::BaseDirs::new().map(|d| d.config_dir().join(APP_DIR_NAME))
}
