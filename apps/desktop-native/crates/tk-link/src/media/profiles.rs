//! Processing profiles of Link v1 (`p-none`, `p-anime4k-{1080,1440,2160}`, `p-anime4k-1080-fg2x`) and encoders.

use serde::Serialize;

use super::anime4k::{Anime4kMode, Anime4kVariant};

pub const SEGMENT_SECS: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Encoder {
    H264Amf,
    H264Nvenc,
    H264Qsv,
    LibOpenH264,
}

impl Encoder {
    /// Preference order: hardware first, software last.
    pub const PREFERENCE: [Self; 4] = [Self::H264Amf, Self::H264Nvenc, Self::H264Qsv, Self::LibOpenH264];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::H264Amf => "h264_amf",
            Self::H264Nvenc => "h264_nvenc",
            Self::H264Qsv => "h264_qsv",
            Self::LibOpenH264 => "libopenh264",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Upscale {
    None,
    Anime4k { mode: Anime4kMode, variant: Anime4kVariant },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Fg {
    None,
    FrcAmf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProfileSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub upscale: Upscale,
    pub fg: Fg,
    pub width: u32,
    pub height: u32,
    pub video_kbps: u32,
}

pub const P_NONE: &str = "p-none";
pub const DEFAULT_PROFILE: &str = P_NONE;

pub const CATALOG: [ProfileSpec; 5] = [
    ProfileSpec { id: P_NONE, label: "No enhancement", upscale: Upscale::None, fg: Fg::None, width: 1920, height: 1080, video_kbps: 6000 },
    ProfileSpec {
        id: "p-anime4k-1080",
        label: "Anime4K to 1080p",
        upscale: Upscale::Anime4k { mode: Anime4kMode::A, variant: Anime4kVariant::M },
        fg: Fg::None,
        width: 1920,
        height: 1080,
        video_kbps: 8000,
    },
    ProfileSpec {
        id: "p-anime4k-1440",
        label: "Anime4K to 1440p",
        upscale: Upscale::Anime4k { mode: Anime4kMode::A, variant: Anime4kVariant::L },
        fg: Fg::None,
        width: 2560,
        height: 1440,
        video_kbps: 14000,
    },
    ProfileSpec {
        id: "p-anime4k-2160",
        label: "Anime4K to 2160p",
        upscale: Upscale::Anime4k { mode: Anime4kMode::A, variant: Anime4kVariant::L },
        fg: Fg::None,
        width: 3840,
        height: 2160,
        video_kbps: 28000,
    },
    ProfileSpec {
        id: "p-anime4k-1080-fg2x",
        label: "Anime4K 1080p + frame generation 2x (AMD)",
        upscale: Upscale::Anime4k { mode: Anime4kMode::A, variant: Anime4kVariant::M },
        fg: Fg::FrcAmf,
        width: 1920,
        height: 1080,
        video_kbps: 10000,
    },
];

pub fn find(id: &str) -> Option<&'static ProfileSpec> {
    CATALOG.iter().find(|p| p.id == id)
}
