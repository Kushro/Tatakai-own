//! tk-ext: rquickjs host for `.kai` extensions (contracts/extension-host).
//!
//! * install: ZIP -> validated manifest + bundle, static compat analysis (`full | partial | unsupported`)
//! * run: one QuickJS runtime per extension (256 MiB, 15 s per call), shims per `globals.v1.md`
//! * network: `fetch` gated by `network:domain:*` and the shared SSRF-safe client of `tk_core::net`
//! * API: [`ExtensionHost::call`] (logical routes), [`ExtensionHost::resolve_streams`] (playback),
//!   [`ExtensionHost::resolve_candidates`] (source selector: web + torrent entries, unranked)

pub mod browser;
pub mod candidates;
pub mod compat;
pub mod error;
pub mod hashing;
pub mod host;
pub mod manifest;
pub mod package;
mod runtime;

pub use browser::{BrowserError, BrowserId, BrowserRuntime, browser_domains, browser_url_allowed};
pub use candidates::{candidate_routes, map_candidates};
pub use compat::{
    CompatReport, DEFERRED_PERMISSIONS, analyze_bundle, deferred_permissions, exposes_torrent_all, wants_torrent_session,
};
pub use error::{ExtError, InstallError};
pub use host::{ExtensionHost, map_sources};
pub use manifest::Manifest;
