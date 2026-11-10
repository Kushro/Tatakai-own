//! Video enhancement preferences beyond the shader preset (`Settings::video`; docs/redesign/18-video-enhancements-inventory.md):
//! Anime4K chain size, debanding, image adjustments and frame generation. Serde default so older files load.

use serde::{Deserialize, Serialize};

/// Bounds of the image adjustments (mpv `brightness` / `contrast` / `saturation` / `gamma`, -100..=100, 0 = neutral).
pub const ADJUST_MIN: i8 = -100;
pub const ADJUST_MAX: i8 = 100;

/// Frame generation (Enigmata's LSFG): only through the external mpv with the Vulkan renderer, where the
/// lsfg-vk Vulkan layer (Linux) can hook it. `Off` everywhere else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FrameGenVm {
    #[default]
    Off,
    /// 2x frames.
    X2,
    /// 3x frames.
    X3,
    /// 4x frames.
    X4,
}

impl FrameGenVm {
    pub const ALL: [FrameGenVm; 4] = [Self::Off, Self::X2, Self::X3, Self::X4];

    pub fn id(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::X2 => "x2",
            Self::X3 => "x3",
            Self::X4 => "x4",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.id() == id)
    }

    /// lsfg-vk multiplier (`LSFG_MULTIPLIER`); 1 = off.
    pub fn multiplier(self) -> u8 {
        match self {
            Self::Off => 1,
            Self::X2 => 2,
            Self::X3 => 3,
            Self::X4 => 4,
        }
    }
}

/// `Settings::video`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoPrefs {
    /// Anime4K presets use the VL networks (high-end GPUs) instead of M/S.
    pub anime4k_hq: bool,
    /// mpv `deband` with Enigmata's values (iterations 2, threshold 40, range 20, grain 6).
    pub deband: bool,
    pub brightness: i8,
    pub contrast: i8,
    pub saturation: i8,
    pub gamma: i8,
    /// Frame generation through the external mpv + lsfg-vk (Linux).
    pub frame_gen: FrameGenVm,
    /// Path of the user's `Lossless.dll` (Lossless Scaling) that lsfg-vk needs; `None` = lsfg-vk's own lookup.
    pub lsfg_dll: Option<String>,
}

impl VideoPrefs {
    pub fn sanitized(mut self) -> Self {
        for v in [&mut self.brightness, &mut self.contrast, &mut self.saturation, &mut self.gamma] {
            *v = (*v).clamp(ADJUST_MIN, ADJUST_MAX);
        }
        self.lsfg_dll = self.lsfg_dll.map(|p| p.trim().to_owned()).filter(|p| !p.is_empty());
        self
    }

    /// Any image adjustment away from neutral.
    pub fn adjusted(&self) -> bool {
        self.brightness != 0 || self.contrast != 0 || self.saturation != 0 || self.gamma != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_neutral_and_sanitized() {
        let d = VideoPrefs::default();
        assert_eq!(d.clone().sanitized(), d);
        assert!(!d.adjusted() && !d.anime4k_hq && d.frame_gen == FrameGenVm::Off);
    }

    #[test]
    fn adjustments_are_clamped_and_blank_dll_dropped() {
        let v = VideoPrefs { brightness: 127, gamma: -128, lsfg_dll: Some("  ".into()), ..VideoPrefs::default() }.sanitized();
        assert_eq!((v.brightness, v.gamma, v.lsfg_dll), (100, -100, None));
    }

    #[test]
    fn frame_gen_ids_round_trip() {
        for m in FrameGenVm::ALL {
            assert_eq!(FrameGenVm::from_id(m.id()), Some(m));
        }
        assert_eq!(FrameGenVm::X3.multiplier(), 3);
        let json = serde_json::to_string(&VideoPrefs { frame_gen: FrameGenVm::X2, ..VideoPrefs::default() }).unwrap();
        assert!(json.contains("\"frame_gen\":\"x2\""), "{json}");
    }
}
