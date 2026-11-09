//! tk-core: domain model, metadata router (AniList + ani.zip + Kitsu), Supabase REST client,
//! local SQLite store and `config.json` loading for the native desktop app.
//!
//! Everything here is UI-free and free of global state: paths and clocks are injected so that
//! tests never touch the real user profile.

pub mod config;
/// Debrid services as torrent sources (wave A, worker i).
pub mod debrid;
pub mod error;
/// AniList / MyAnimeList lists: OAuth, read / write, offline queue (wave A, worker g).
pub mod lists;
pub mod metadata;
pub mod model;
pub mod net;
pub mod release;
/// OS keyring with a file fallback for tokens and the Supabase session (wave A, worker f).
pub mod secrets;
pub mod sources;
pub mod store;
pub mod supabase;
/// trace.moe scene search (wave A, worker a).
pub mod tracemoe;
/// Subtitle translation providers (wave A, worker d).
pub mod translate;

pub use config::AppConfig;
pub use error::{DebridError, ListError, ProviderError, SecretError, StoreError, SupabaseError, TranslateError};
pub use secrets::Secrets;
pub use metadata::{MetadataProvider, MetadataRouter};
pub use model::*;
pub use store::LocalStore;
pub use supabase::{Session, Supabase};

/// Directory name under `%APPDATA%` used by the native app (06 #6).
pub const APP_DIR_NAME: &str = "Tatakai-Next";

/// `%APPDATA%\Tatakai-Next` (Roaming). Resolved by the *app* at start-up; tests must pass tempdirs instead.
pub fn default_data_dir() -> Option<std::path::PathBuf> {
    directories::BaseDirs::new().map(|d| d.config_dir().join(APP_DIR_NAME))
}
