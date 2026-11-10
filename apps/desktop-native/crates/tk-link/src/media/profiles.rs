//! Processing profiles of Link v1 and encoders.
//!
//! * `p-none`, `p-anime4k-{1080,1440,2160}`: one FFmpeg (libplacebo + Anime4K on Windows and Linux).
//! * `p-anime4k-1080-fg2x`: `frc_amf` frame generation (AMD AMF, Windows only).
//! * `p-lsfg-1080-x{2..5}`, `p-lsfg-1080-auto120` and `p-anime4k-lsfg-1080-*`: LSFG frame generation (experimental)
//!   through the `temple-lsfg` helper between two FFmpeg (see [`super::args::build_lsfg`]).
//!
//! The LSFG catalog (multipliers 2..5, auto mode aiming at 120 fps, bitrates) and [`auto_lsfg_multiplier`] are
//! ported from Enigmata-Temple `crates/temple-core/src/media/profiles.rs` (`lsfg_specs`, `l-lsfg-1080x2..x5`,
//! `sl-anime4k-lsfg-1080x2..x5`, `*-1080-auto120`; AGPL-3.0, same author).

use serde::{Serialize, Serializer};

use super::anime4k::{Anime4kMode, Anime4kVariant};

pub const SEGMENT_SECS: u32 = 2;

/// Platform family: decides the hardware path (D3D11 + AMF/NVENC/QSV on Windows, VAAPI on Linux).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Platform {
    Windows,
    /// Linux (and any other non-Windows system).
    Linux,
}

impl Platform {
    pub const fn current() -> Self {
        if cfg!(windows) { Self::Windows } else { Self::Linux }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Encoder {
    H264Amf,
    H264Nvenc,
    H264Qsv,
    /// Linux VAAPI (Mesa radeonsi / iHD); needs a usable render node.
    H264Vaapi,
    /// Software, preferred over libopenh264 when FFmpeg has it (Linux builds usually do).
    LibX264,
    LibOpenH264,
}

impl Encoder {
    /// Windows: hardware first, software last (unchanged since M0).
    pub const WINDOWS_PREFERENCE: [Self; 4] = [Self::H264Amf, Self::H264Nvenc, Self::H264Qsv, Self::LibOpenH264];
    /// Linux: VAAPI, then NVENC (proprietary NVIDIA driver), then libx264, then libopenh264.
    pub const LINUX_PREFERENCE: [Self; 4] = [Self::H264Vaapi, Self::H264Nvenc, Self::LibX264, Self::LibOpenH264];
    /// Preference order of this platform.
    pub const PREFERENCE: [Self; 4] = if cfg!(windows) { Self::WINDOWS_PREFERENCE } else { Self::LINUX_PREFERENCE };

    pub const fn preference(platform: Platform) -> [Self; 4] {
        match platform {
            Platform::Windows => Self::WINDOWS_PREFERENCE,
            Platform::Linux => Self::LINUX_PREFERENCE,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::H264Amf => "h264_amf",
            Self::H264Nvenc => "h264_nvenc",
            Self::H264Qsv => "h264_qsv",
            Self::H264Vaapi => "h264_vaapi",
            Self::LibX264 => "libx264",
            Self::LibOpenH264 => "libopenh264",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Upscale {
    None,
    Anime4k { mode: Anime4kMode, variant: Anime4kVariant },
}

/// LSFG multiplier: fixed, or chosen per session to bring the output close to `target_fps`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LsfgMode {
    Fixed(u8),
    Auto { target_fps: u32 },
}

impl LsfgMode {
    /// Multiplier for a source of `source_fps` (only the auto mode looks at it).
    pub fn multiplier(self, source_fps: Option<f64>) -> u8 {
        match self {
            Self::Fixed(m) => m,
            Self::Auto { target_fps } => auto_lsfg_multiplier(target_fps, source_fps),
        }
    }
}

/// Frame generation of a profile. Serialized as `"none"`, `"frc_amf"` or `"lsfg"` (the multiplier travels in
/// `fg_multiplier` / `fg_target_fps`, additive fields of the profile).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fg {
    None,
    FrcAmf,
    Lsfg(LsfgMode),
}

impl Fg {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::FrcAmf => "frc_amf",
            Self::Lsfg(_) => "lsfg",
        }
    }

    pub fn lsfg(self) -> Option<LsfgMode> {
        match self {
            Self::Lsfg(m) => Some(m),
            _ => None,
        }
    }
}

impl Serialize for Fg {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
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

/// LSFG multipliers the helper accepts.
pub const LSFG_MULTIPLIERS: [u8; 4] = [2, 3, 4, 5];
/// Target of the auto profiles: the 120 Hz panel of the phone or tablet.
pub const LSFG_AUTO_TARGET_FPS: u32 = 120;
/// Auto multiplier when the source rate is unknown or unusable: the lightest one.
pub const LSFG_AUTO_FALLBACK_MULTIPLIER: u8 = 2;

/// Auto LSFG multiplier bringing `source_fps` close to `target_fps`: `target / source` rounded to the nearest integer
/// (an exact tie rounds down: less work and the output does not exceed the target), clamped to 2..=5. With 120 fps:
/// 23.976 and 25 -> x5, 29.97 and 30 -> x4, 50 and 59.94 -> x2. Without a usable rate,
/// [`LSFG_AUTO_FALLBACK_MULTIPLIER`]. Ported from Enigmata-Temple `profiles.rs` (`auto_lsfg_multiplier`).
pub fn auto_lsfg_multiplier(target_fps: u32, source_fps: Option<f64>) -> u8 {
    let Some(source_fps) = source_fps.filter(|fps| fps.is_finite() && *fps > 0.0) else {
        return LSFG_AUTO_FALLBACK_MULTIPLIER;
    };
    let (min, max) = (LSFG_MULTIPLIERS[0], LSFG_MULTIPLIERS[LSFG_MULTIPLIERS.len() - 1]);
    let nearest = (f64::from(target_fps) / source_fps - 0.5).ceil();
    if nearest <= f64::from(min) {
        min
    } else if nearest >= f64::from(max) {
        max
    } else {
        nearest as u8
    }
}

const A4K_1080: Upscale = Upscale::Anime4k { mode: Anime4kMode::A, variant: Anime4kVariant::M };

const fn lsfg(id: &'static str, label: &'static str, upscale: Upscale, mode: LsfgMode, video_kbps: u32) -> ProfileSpec {
    ProfileSpec { id, label, upscale, fg: Fg::Lsfg(mode), width: 1920, height: 1080, video_kbps }
}

/// Temple's LSFG bitrates at 1080p: x2 8 Mb/s, x3 10, x4 11, x5 12, auto (nominal x4) 11.
pub const CATALOG: [ProfileSpec; 15] = [
    ProfileSpec { id: P_NONE, label: "No enhancement", upscale: Upscale::None, fg: Fg::None, width: 1920, height: 1080, video_kbps: 6000 },
    ProfileSpec { id: "p-anime4k-1080", label: "Anime4K to 1080p", upscale: A4K_1080, fg: Fg::None, width: 1920, height: 1080, video_kbps: 8000 },
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
        upscale: A4K_1080,
        fg: Fg::FrcAmf,
        width: 1920,
        height: 1080,
        video_kbps: 10000,
    },
    lsfg("p-lsfg-1080-x2", "LSFG frame generation 2x, 1080p (experimental)", Upscale::None, LsfgMode::Fixed(2), 8000),
    lsfg("p-lsfg-1080-x3", "LSFG frame generation 3x, 1080p (experimental)", Upscale::None, LsfgMode::Fixed(3), 10_000),
    lsfg("p-lsfg-1080-x4", "LSFG frame generation 4x, 1080p (experimental)", Upscale::None, LsfgMode::Fixed(4), 11_000),
    lsfg("p-lsfg-1080-x5", "LSFG frame generation 5x, 1080p (experimental)", Upscale::None, LsfgMode::Fixed(5), 12_000),
    lsfg("p-lsfg-1080-auto120", "LSFG frame generation to 120 fps, 1080p (experimental)", Upscale::None, LsfgMode::Auto { target_fps: LSFG_AUTO_TARGET_FPS }, 11_000),
    lsfg("p-anime4k-lsfg-1080-x2", "Anime4K 1080p + LSFG 2x (experimental)", A4K_1080, LsfgMode::Fixed(2), 8000),
    lsfg("p-anime4k-lsfg-1080-x3", "Anime4K 1080p + LSFG 3x (experimental)", A4K_1080, LsfgMode::Fixed(3), 10_000),
    lsfg("p-anime4k-lsfg-1080-x4", "Anime4K 1080p + LSFG 4x (experimental)", A4K_1080, LsfgMode::Fixed(4), 11_000),
    lsfg("p-anime4k-lsfg-1080-x5", "Anime4K 1080p + LSFG 5x (experimental)", A4K_1080, LsfgMode::Fixed(5), 12_000),
    lsfg(
        "p-anime4k-lsfg-1080-auto120",
        "Anime4K 1080p + LSFG to 120 fps (experimental)",
        A4K_1080,
        LsfgMode::Auto { target_fps: LSFG_AUTO_TARGET_FPS },
        11_000,
    ),
];

pub fn find(id: &str) -> Option<&'static ProfileSpec> {
    CATALOG.iter().find(|p| p.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_ids_are_unique_stable_and_link_style() {
        let ids: Vec<_> = CATALOG.iter().map(|p| p.id).collect();
        // M0 ids keep their place
        assert_eq!(ids[..5], ["p-none", "p-anime4k-1080", "p-anime4k-1440", "p-anime4k-2160", "p-anime4k-1080-fg2x"]);
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len());
        assert!(ids.iter().all(|id| id.starts_with("p-")));
        let lsfg: Vec<_> = CATALOG.iter().filter(|p| p.fg.lsfg().is_some()).collect();
        assert_eq!(lsfg.len(), 10);
        for p in &lsfg {
            assert_eq!((p.width, p.height), (1920, 1080), "{}", p.id);
            assert!(p.label.contains("LSFG"));
        }
        assert_eq!(find("p-lsfg-1080-x3").unwrap().fg, Fg::Lsfg(LsfgMode::Fixed(3)));
        assert_eq!(find("p-anime4k-lsfg-1080-auto120").unwrap().upscale, A4K_1080);
        assert_eq!(find("p-lsfg-1080-auto120").unwrap().fg, Fg::Lsfg(LsfgMode::Auto { target_fps: 120 }));
        assert_eq!(find("p-lsfg-1080-x5").unwrap().video_kbps, 12_000);
        assert!(find("l-lsfg-1080x2").is_none(), "Temple ids are not Link ids");
    }

    #[test]
    fn auto_multiplier_aims_at_the_target() {
        let m = |fps: f64| auto_lsfg_multiplier(120, Some(fps));
        assert_eq!([m(23.976), m(24.0), m(25.0), m(29.97), m(30.0), m(50.0), m(59.94), m(60.0), m(120.0)], [5, 5, 5, 4, 4, 2, 2, 2, 2]);
        // 48 fps: 2.5 exactly -> rounds down
        assert_eq!(m(48.0), 2);
        assert_eq!(m(40.0), 3);
        assert_eq!(auto_lsfg_multiplier(60, Some(24.0)), 2);
        for bad in [None, Some(0.0), Some(f64::INFINITY), Some(f64::NAN), Some(-3.0)] {
            assert_eq!(auto_lsfg_multiplier(120, bad), LSFG_AUTO_FALLBACK_MULTIPLIER);
        }
        assert_eq!(LsfgMode::Fixed(3).multiplier(Some(60.0)), 3);
        assert_eq!(LsfgMode::Auto { target_fps: 120 }.multiplier(Some(30.0)), 4);
    }

    #[test]
    fn platform_preferences() {
        assert_eq!(Encoder::preference(Platform::Windows), [Encoder::H264Amf, Encoder::H264Nvenc, Encoder::H264Qsv, Encoder::LibOpenH264]);
        assert_eq!(Encoder::preference(Platform::Linux)[0], Encoder::H264Vaapi);
        assert_eq!(Encoder::PREFERENCE, Encoder::preference(Platform::current()));
        assert_eq!(serde_json::to_value(Fg::Lsfg(LsfgMode::Fixed(2))).unwrap(), "lsfg");
        assert_eq!(serde_json::to_value(Fg::FrcAmf).unwrap(), "frc_amf");
        assert_eq!(serde_json::to_value(Fg::None).unwrap(), "none");
    }
}
