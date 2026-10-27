//! Media pipeline: profiles, FFmpeg argument builder, Anime4K shader composition, capability detection,
//! engine abstraction (real FFmpeg vs test fake) and the Windows Job Object.

pub mod anime4k;
pub mod args;
pub mod capabilities;
pub mod engine;
pub mod job;
pub mod profiles;
