//! Shader presets -> mpv `glsl-shaders` (docs/redesign/03 section 6).
//!
//! The Anime4K chains follow the official "GLSL_Instructions_Advanced_mpv" modes, restricted to the
//! shader files pinned in `packaging/anime4k.lock.json` (S/M/L variants; no VL, no AutoDownscalePre_x4).
//! The app resolves the directory that holds them; this module only maps presets to file names.

use std::path::{Path, PathBuf};

/// Live-switchable upscaling presets. Keyboard: Ctrl+1..Ctrl+6 in [`ShaderPreset::ALL`] order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ShaderPreset {
    #[default]
    Off,
    /// Mode A (Fast)
    Anime4kA,
    /// Mode A+A (Fast)
    Anime4kAA,
    /// Mode B (Fast)
    Anime4kB,
    /// Mode C (Fast)
    Anime4kC,
    /// AMD FidelityFX Super Resolution (mpv user shader)
    Fsr,
}

const CLAMP: &str = "Anime4K_Clamp_Highlights.glsl";
const RESTORE_M: &str = "Anime4K_Restore_CNN_M.glsl";
const RESTORE_S: &str = "Anime4K_Restore_CNN_S.glsl";
const RESTORE_SOFT_M: &str = "Anime4K_Restore_CNN_Soft_M.glsl";
const UPSCALE_M: &str = "Anime4K_Upscale_CNN_x2_M.glsl";
const UPSCALE_S: &str = "Anime4K_Upscale_CNN_x2_S.glsl";
const UPSCALE_DENOISE_M: &str = "Anime4K_Upscale_Denoise_CNN_x2_M.glsl";
const DOWNSCALE_X2: &str = "Anime4K_AutoDownscalePre_x2.glsl";
const FSR: &str = "FSR.glsl";

impl ShaderPreset {
    pub const ALL: [ShaderPreset; 6] = [
        Self::Off,
        Self::Anime4kA,
        Self::Anime4kAA,
        Self::Anime4kB,
        Self::Anime4kC,
        Self::Fsr,
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
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.id() == id)
    }

    /// Ctrl+`n` (1-based) -> preset.
    pub fn from_shortcut(n: u8) -> Option<Self> {
        Self::ALL.get(usize::from(n).checked_sub(1)?).copied()
    }

    /// Shader files, in application order, relative to the shaders directory.
    pub fn shader_files(self) -> &'static [&'static str] {
        match self {
            Self::Off => &[],
            Self::Anime4kA => &[CLAMP, RESTORE_M, UPSCALE_M, DOWNSCALE_X2, UPSCALE_S],
            Self::Anime4kAA => &[CLAMP, RESTORE_M, UPSCALE_M, RESTORE_S, DOWNSCALE_X2, UPSCALE_S],
            Self::Anime4kB => &[CLAMP, RESTORE_SOFT_M, UPSCALE_M, DOWNSCALE_X2, UPSCALE_S],
            Self::Anime4kC => &[CLAMP, UPSCALE_DENOISE_M, DOWNSCALE_X2, UPSCALE_S],
            Self::Fsr => &[FSR],
        }
    }

    /// Files of this preset that are not present in `dir`.
    pub fn missing_files(self, dir: &Path) -> Vec<&'static str> {
        self.shader_files().iter().copied().filter(|f| !dir.join(f).is_file()).collect()
    }
}

/// mpv list separator for path lists: `;` on Windows, `:` elsewhere.
pub const fn list_separator() -> char {
    if cfg!(windows) { ';' } else { ':' }
}

/// Absolute shader paths of `preset` under `dir`.
pub fn shader_paths(dir: &Path, preset: ShaderPreset) -> Vec<PathBuf> {
    preset.shader_files().iter().map(|f| dir.join(f)).collect()
}

/// The `glsl-shaders` property value: `sep`-joined absolute paths (forward-slashed so mpv never sees a
/// backslash escape), empty for [`ShaderPreset::Off`].
pub fn glsl_shaders_value(dir: &Path, preset: ShaderPreset, sep: char) -> String {
    shader_paths(dir, preset)
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect::<Vec<_>>()
        .join(&sep.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_clears_the_list() {
        assert_eq!(glsl_shaders_value(Path::new("C:/s"), ShaderPreset::Off, ';'), "");
    }

    #[test]
    fn modes_map_to_official_chains() {
        let a = glsl_shaders_value(Path::new("C:/app/shaders"), ShaderPreset::Anime4kA, ';');
        assert_eq!(
            a,
            "C:/app/shaders/Anime4K_Clamp_Highlights.glsl;C:/app/shaders/Anime4K_Restore_CNN_M.glsl;\
             C:/app/shaders/Anime4K_Upscale_CNN_x2_M.glsl;C:/app/shaders/Anime4K_AutoDownscalePre_x2.glsl;\
             C:/app/shaders/Anime4K_Upscale_CNN_x2_S.glsl"
        );
        assert_eq!(ShaderPreset::Anime4kAA.shader_files().len(), 6);
        assert!(ShaderPreset::Anime4kB.shader_files().contains(&"Anime4K_Restore_CNN_Soft_M.glsl"));
        assert!(ShaderPreset::Anime4kC.shader_files().contains(&"Anime4K_Upscale_Denoise_CNN_x2_M.glsl"));
        assert_eq!(ShaderPreset::Fsr.shader_files(), ["FSR.glsl"]);
    }

    #[test]
    fn backslashes_never_reach_mpv() {
        let v = glsl_shaders_value(Path::new("C:\\app\\shaders"), ShaderPreset::Fsr, ';');
        assert!(!v.contains('\\'), "{v}");
    }

    #[test]
    fn every_chain_starts_with_clamp_and_ends_with_an_upscale_except_fsr() {
        for p in ShaderPreset::ALL {
            let files = p.shader_files();
            match p {
                ShaderPreset::Off | ShaderPreset::Fsr => {}
                _ => {
                    assert_eq!(files[0], CLAMP, "{p:?}");
                    assert!(files.last().unwrap().contains("Upscale"), "{p:?}");
                }
            }
        }
    }

    #[test]
    fn ids_and_shortcuts_round_trip() {
        for (i, p) in ShaderPreset::ALL.into_iter().enumerate() {
            assert_eq!(ShaderPreset::from_id(p.id()), Some(p));
            assert_eq!(ShaderPreset::from_shortcut(i as u8 + 1), Some(p));
        }
        assert_eq!(ShaderPreset::from_shortcut(0), None);
        assert_eq!(ShaderPreset::from_shortcut(7), None);
        assert_eq!(ShaderPreset::from_id("nope"), None);
    }

    #[test]
    fn missing_files_lists_absent_shaders() {
        let dir = std::env::temp_dir().join("tk-player-no-such-shader-dir");
        assert_eq!(ShaderPreset::Fsr.missing_files(&dir), vec!["FSR.glsl"]);
        assert!(ShaderPreset::Off.missing_files(&dir).is_empty());
    }
}
