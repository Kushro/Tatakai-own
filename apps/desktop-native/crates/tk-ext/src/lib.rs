//! tk-ext: rquickjs host for `.kai` extensions (contracts/extension-host).
//!
//! * install: ZIP -> validated manifest + bundle, static compat analysis (`full | partial | unsupported`)
//! * run: one QuickJS runtime per extension (64 MiB, 15 s per call), shims per `globals.v1.md`
//! * network: `fetch` gated by `network:domain:*` and the shared SSRF-safe client of `tk_core::net`
//! * API: [`ExtensionHost::call`] (logical routes), [`ExtensionHost::resolve_streams`] (playback),
//!   [`ExtensionHost::resolve_candidates`] (source selector: web + torrent entries, unranked)

pub mod candidates;
pub mod compat;
pub mod error;
pub mod hashing;
pub mod host;
pub mod manifest;
pub mod package;
mod runtime;

pub use candidates::map_candidates;
pub use compat::{CompatReport, DEFERRED_PERMISSIONS, analyze_bundle, deferred_permissions, wants_torrent_session};
pub use error::{ExtError, InstallError};
pub use host::{ExtensionHost, map_sources};
pub use manifest::Manifest;
