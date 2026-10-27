//! tk-core: domain model, metadata router (AniList + ani.zip + Kitsu), Supabase REST client,
//! local SQLite store and `config.json` loading for the native desktop app.
//!
//! Everything here is UI-free and free of global state: paths and clocks are injected so that
//! tests never touch the real user profile.

pub mod config;
pub mod error;
pub mod metadata;
pub mod model;
pub mod net;
pub mod store;
pub mod supabase;

pub use config::AppConfig;
pub use error::{ProviderError, StoreError, SupabaseError};
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
