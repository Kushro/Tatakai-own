//! Shader presets -> mpv `glsl-shaders` (docs/redesign/03 section 6).
//!
//! The Anime4K chains follow the official "GLSL_Instructions_Advanced_mpv" modes, restricted to the
//! shader files pinned in `packaging/anime4k.lock.json` (S/M/L variants; no VL, no AutoDownscalePre_x4).
//! The app resolves the directory that holds them; this module only maps presets to file names.

use std::path::{Path, PathBuf};

/// Live-switchable video enhancement presets. Keyboard: Ctrl+1..Ctrl+9 for the first nine of
/// [`ShaderPreset::ALL`] (the original six keep their keys).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ShaderPreset {
    #[default]
    Off,
    /// Anime4K mode A: restore + upscale (most anime).
    Anime4kA,
    /// Anime4K mode A+A: A with a second restore (blurrier sources).
    Anime4kAA,
    /// Anime4K mode B: soft restore (sources with ringing / aliasing).
    Anime4kB,
    /// Anime4K mode C: denoise upscale (sharp, noisy sources).
    Anime4kC,
    /// AMD FidelityFX Super Resolution 1.0 (mpv user shader).
    Fsr,
    /// Anime4K mode B+B.
    Anime4kBB,
    /// Anime4K mode C+A.
    Anime4kCA,
    /// "Fidelity" (Enigmata): FSRCNNX line-art luma upscale + KrigBilateral chroma + SSimDownscaler.
    Fidelity,
    /// ArtCNN Ani4Kv2 (neural upscaler, fragment version).
    ArtCnn,
    /// NVIDIA Image Scaling (spatial upscale + sharpen).
    NvScaler,
    /// AMD FidelityFX Contrast Adaptive Sharpening (no upscale).
    Cas,
}

/// Anime4K chain size (official "GLSL_Instructions_Advanced_mpv"): `Fast` = M/S shaders for most GPUs, `High` =
/// VL shaders for high-end GPUs (Enigmata's HQ tier). Presets that are not Anime4K ignore it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Anime4kQuality {
    #[default]
    Fast,
    High,
}

const CLAMP: &str = "Anime4K_Clamp_Highlights.glsl";
const DOWN_X2: &str = "Anime4K_AutoDownscalePre_x2.glsl";
const DOWN_X4: &str = "Anime4K_AutoDownscalePre_x4.glsl";
const FSR: &str = "FSR.glsl";

impl ShaderPreset {
    pub const ALL: [ShaderPreset; 12] = [
        Self::Off,
        Self::Anime4kA,
        Self::Anime4kAA,
        Self::Anime4kB,
        Self::Anime4kC,
        Self::Fsr,
        Self::Anime4kBB,
        Self::Anime4kCA,
        Self::Fidelity,
        Self::ArtCnn,
        Self::NvScaler,
        Self::Cas,
    ];

    /// Stable id used in settings files and i18n keys (`preset.<id>`).
    pub fn id(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Anime4kA => "anime4k-a",
            Self::Anime4kAA => "anime4k-aa",
            Self::Anime4kB => "anime4k-b",
            Self::Anime4kC => "anime4k-c",
            Self::Fsr => "fsr",
            Self::Anime4kBB => "anime4k-bb",
            Self::Anime4kCA => "anime4k-ca",
            Self::Fidelity => "fidelity",
            Self::ArtCnn => "artcnn",
            Self::NvScaler => "nvscaler",
            Self::Cas => "cas",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.id() == id)
    }

    /// Ctrl+`n` (1-based, 1..=9) -> preset.
    pub fn from_shortcut(n: u8) -> Option<Self> {
        if n > 9 {
            return None;
        }
        Self::ALL.get(usize::from(n).checked_sub(1)?).copied()
    }

    /// Whether [`Anime4kQuality`] changes this preset's chain.
    pub fn is_anime4k(self) -> bool {
        matches!(self, Self::Anime4kA | Self::Anime4kAA | Self::Anime4kB | Self::Anime4kC | Self::Anime4kBB | Self::Anime4kCA)
    }

    /// Shader files, in application order, relative to the shaders directory.
    pub fn shader_files(self, quality: Anime4kQuality) -> Vec<String> {
        // Big (first) and small (second) networks of the chain: VL/M on High, M/S on Fast.
        let (big, small) = match quality {
            Anime4kQuality::Fast => ("M", "S"),
            Anime4kQuality::High => ("VL", "M"),
        };
        let restore = |v: &str| format!("Anime4K_Restore_CNN_{v}.glsl");
        let soft = |v: &str| format!("Anime4K_Restore_CNN_Soft_{v}.glsl");
        let up = |v: &str| format!("Anime4K_Upscale_CNN_x2_{v}.glsl");
        let denoise = |v: &str| format!("Anime4K_Upscale_Denoise_CNN_x2_{v}.glsl");
        let s = |x: &str| x.to_owned();
        match self {
            Self::Off => Vec::new(),
            Self::Anime4kA => vec![s(CLAMP), restore(big), up(big), s(DOWN_X2), s(DOWN_X4), up(small)],
            Self::Anime4kAA => vec![s(CLAMP), restore(big), up(big), restore(small), s(DOWN_X2), s(DOWN_X4), up(small)],
            Self::Anime4kB => vec![s(CLAMP), soft(big), up(big), s(DOWN_X2), s(DOWN_X4), up(small)],
            Self::Anime4kC => vec![s(CLAMP), denoise(big), s(DOWN_X2), s(DOWN_X4), up(small)],
            Self::Anime4kBB => vec![s(CLAMP), soft(big), up(big), s(DOWN_X2), s(DOWN_X4), soft(small), up(small)],
            Self::Anime4kCA => vec![s(CLAMP), denoise(big), s(DOWN_X2), s(DOWN_X4), restore(small), up(small)],
            Self::Fsr => vec![s(FSR)],
            Self::Fidelity => vec![s("FSRCNNX_x2_8-0-4-1_LineArt.glsl"), s("KrigBilateral.glsl"), s("SSimDownscaler.glsl")],
            Self::ArtCnn => vec![s("Ani4Kv2_ArtCNN_C4F32_i2.glsl")],
            Self::NvScaler => vec![s("NVScaler.glsl")],
            Self::Cas => vec![s("CAS.glsl")],
        }
    }

    /// Files of this preset that are not present in `dir`.
    pub fn missing_files(self, dir: &Path, quality: Anime4kQuality) -> Vec<String> {
        self.shader_files(quality).into_iter().filter(|f| !dir.join(f).is_file()).collect()
    }
}

/// Every shader file any preset can ask for (both qualities), for the on-demand install.
pub fn all_preset_files() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for p in ShaderPreset::ALL {
        for q in [Anime4kQuality::Fast, Anime4kQuality::High] {
            for f in p.shader_files(q) {
                if !out.contains(&f) {
                    out.push(f);
                }
            }
        }
    }
    out
}

/// mpv list separator for path lists: `;` on Windows, `:` elsewhere.
pub const fn list_separator() -> char {
    if cfg!(windows) { ';' } else { ':' }
}

/// Absolute shader paths of `preset` under `dir`.
pub fn shader_paths(dir: &Path, preset: ShaderPreset, quality: Anime4kQuality) -> Vec<PathBuf> {
    preset.shader_files(quality).iter().map(|f| dir.join(f)).collect()
}

/// The `glsl-shaders` property value: `sep`-joined absolute paths (forward-slashed so mpv never sees a
/// backslash escape), empty for [`ShaderPreset::Off`].
pub fn glsl_shaders_value(dir: &Path, preset: ShaderPreset, quality: Anime4kQuality, sep: char) -> String {
    shader_paths(dir, preset, quality)
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect::<Vec<_>>()
        .join(&sep.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    const F: Anime4kQuality = Anime4kQuality::Fast;
    const H: Anime4kQuality = Anime4kQuality::High;

    #[test]
    fn off_clears_the_list() {
        assert_eq!(glsl_shaders_value(Path::new("C:/s"), ShaderPreset::Off, F, ';'), "");
    }

    #[test]
    fn modes_map_to_official_chains() {
        let a = glsl_shaders_value(Path::new("C:/app/shaders"), ShaderPreset::Anime4kA, F, ';');
        assert_eq!(
            a,
            "C:/app/shaders/Anime4K_Clamp_Highlights.glsl;C:/app/shaders/Anime4K_Restore_CNN_M.glsl;\
             C:/app/shaders/Anime4K_Upscale_CNN_x2_M.glsl;C:/app/shaders/Anime4K_AutoDownscalePre_x2.glsl;\
             C:/app/shaders/Anime4K_AutoDownscalePre_x4.glsl;C:/app/shaders/Anime4K_Upscale_CNN_x2_S.glsl"
        );
        assert_eq!(ShaderPreset::Anime4kAA.shader_files(F).len(), 7);
        assert!(ShaderPreset::Anime4kB.shader_files(F).contains(&"Anime4K_Restore_CNN_Soft_M.glsl".to_owned()));
        assert!(ShaderPreset::Anime4kC.shader_files(F).contains(&"Anime4K_Upscale_Denoise_CNN_x2_M.glsl".to_owned()));
        assert_eq!(ShaderPreset::Fsr.shader_files(F), ["FSR.glsl"]);
    }

    #[test]
    fn high_quality_uses_the_vl_networks_and_m_second_pass() {
        let a = ShaderPreset::Anime4kA.shader_files(H);
        assert_eq!(a[1], "Anime4K_Restore_CNN_VL.glsl");
        assert_eq!(a.last().unwrap(), "Anime4K_Upscale_CNN_x2_M.glsl");
        assert_eq!(ShaderPreset::Cas.shader_files(H), ShaderPreset::Cas.shader_files(F), "non-Anime4K ignores quality");
        assert!(!ShaderPreset::Fidelity.is_anime4k() && ShaderPreset::Anime4kCA.is_anime4k());
    }

    #[test]
    fn backslashes_never_reach_mpv() {
        let v = glsl_shaders_value(Path::new("C:\\app\\shaders"), ShaderPreset::Fsr, F, ';');
        assert!(!v.contains('\\'), "{v}");
    }

    #[test]
    fn every_anime4k_chain_starts_with_clamp_and_ends_with_an_upscale() {
        for p in ShaderPreset::ALL.into_iter().filter(|p| p.is_anime4k()) {
            for q in [F, H] {
                let files = p.shader_files(q);
                assert_eq!(files[0], CLAMP, "{p:?}");
                assert!(files.last().unwrap().contains("Upscale"), "{p:?}");
            }
        }
    }

    #[test]
    fn ids_and_shortcuts_round_trip() {
        for p in ShaderPreset::ALL {
            assert_eq!(ShaderPreset::from_id(p.id()), Some(p));
        }
        for (i, p) in ShaderPreset::ALL.into_iter().take(9).enumerate() {
            assert_eq!(ShaderPreset::from_shortcut(i as u8 + 1), Some(p));
        }
        // The original six keep their keys.
        assert_eq!(ShaderPreset::from_shortcut(6), Some(ShaderPreset::Fsr));
        assert_eq!(ShaderPreset::from_shortcut(0), None);
        assert_eq!(ShaderPreset::from_shortcut(10), None);
        assert_eq!(ShaderPreset::from_id("nope"), None);
    }

    #[test]
    fn missing_files_lists_absent_shaders() {
        let dir = std::env::temp_dir().join("tk-player-no-such-shader-dir");
        assert_eq!(ShaderPreset::Fsr.missing_files(&dir, F), vec!["FSR.glsl".to_owned()]);
        assert!(ShaderPreset::Off.missing_files(&dir, F).is_empty());
    }

    #[test]
    fn install_list_has_every_preset_file_once() {
        let all = all_preset_files();
        assert!(all.contains(&"Anime4K_Upscale_CNN_x2_VL.glsl".to_owned()) && all.contains(&"CAS.glsl".to_owned()));
        let mut d = all.clone();
        d.dedup();
        assert_eq!(d.len(), all.len());
    }
}
