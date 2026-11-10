//! Media pipeline: profiles, FFmpeg argument builder (Windows D3D11 / Linux VAAPI, LSFG process chain), Anime4K
//! shader composition, capability detection, LSFG helper discovery, VAAPI render node, engine abstraction (real
//! FFmpeg vs test fake) and process lifetime (Windows Job Object, Linux `PR_SET_PDEATHSIG`).

pub mod anime4k;
pub mod args;
pub mod capabilities;
pub mod engine;
pub mod job;
pub mod lsfg;
pub mod probe;
pub mod profiles;
pub mod vaapi;
