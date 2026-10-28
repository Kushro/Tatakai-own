//! tk-ext: rquickjs host for `.kai` extensions (contracts/extension-host).
//!
//! * install: ZIP -> validated manifest + bundle, static compat analysis (`full | partial | unsupported`)
//! * run: one QuickJS runtime per extension (64 MiB, 15 s per call), shims per `globals.v1.md`
//! * network: `fetch` gated by `network:domain:*` and the shared SSRF-safe client of `tk_core::net`
//! * API: [`ExtensionHost::call`] (logical routes), [`ExtensionHost::resolve_streams`] (playback)

pub mod compat;
pub mod error;
pub mod hashing;
pub mod host;
pub mod manifest;
pub mod package;
mod runtime;

pub use compat::{CompatReport, analyze_bundle};
pub use error::{ExtError, InstallError};
pub use host::{ExtensionHost, map_sources};
pub use manifest::Manifest;
