//! Subtitle preferences (wave A, worker **d**; docs/redesign/15-parity-wave-a-contract.md section 3).
//! Embedded in `Settings::subs` and persisted with it in `ui-settings.json`. Every field has a serde default so
//! older files load; `sanitized()` repairs out-of-range values. Worker d owns this file: new fields, bounds
//! and tests go here, never in `model/mod.rs`.

use serde::{Deserialize, Serialize};

/// `TranslateRequestVm::cache_key` of an empty request that asks `Backend::translate_cues` to empty the translation
/// cache (Settings > Subtitles "Clear cache") instead of translating; the answer is one cue whose `index` is the
/// number of rows removed. tk-app `live/subs.rs` honours it (no extra `Backend` method in the frozen trait).
pub const CLEAR_CACHE_KEY: &str = "\u{0}clear-translation-cache";

/// Target languages offered for translation (ISO 639-1; labels `subs.lang.<code>`).
pub const TRANSLATE_LANGS: [&str; 12] = ["es", "en", "pt", "fr", "de", "it", "ja", "ko", "zh", "ru", "ar", "pl"];

/// Languages offered for the secondary subtitle (labels `subs.lang.<code>`).
pub const SECONDARY_LANGS: [&str; 6] = ["en", "es", "pt", "ja", "fr", "de"];

/// Subtitle rendering style, mapped by the player to mpv `sub-*` options (`tk_player::Player::set_sub_style`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SubtitleStyleVm {
    /// Percent of mpv's default size (`sub-scale` = size / 100); 50..=200.
    pub size: u8,
    /// Font family (`sub-font`); empty = mpv default.
    pub font: String,
    /// Text colour `#RRGGBB` (`sub-color`); user data, not a stylesheet literal.
    pub color: String,
    /// Background box opacity 0..=100 (`sub-back-color` alpha); 0 = no box.
    pub bg_opacity: u8,
    /// Outline width in px 0..=5 (`sub-border-size`).
    pub outline: u8,
    /// Outline colour `#RRGGBB` (`sub-border-color`); user data.
    pub outline_color: String,
    /// Vertical position 0..=100 from the bottom (`sub-pos`), 100 = default bottom margin.
    pub position: u8,
    pub bold: bool,
}

impl Default for SubtitleStyleVm {
    fn default() -> Self {
        Self { size: 100, font: String::new(), color: "#FFFFFF".to_owned(), bg_opacity: 0, outline: 2, outline_color: "#000000".to_owned(), position: 100, bold: false }
    }
}

impl SubtitleStyleVm {
    pub fn sanitized(mut self) -> Self {
        self.size = self.size.clamp(50, 200);
        self.font = self.font.trim().to_owned();
        if !is_hex_color(&self.color) {
            self.color = Self::default().color;
        }
        if !is_hex_color(&self.outline_color) {
            self.outline_color = Self::default().outline_color;
        }
        self.color = self.color.to_ascii_uppercase();
        self.outline_color = self.outline_color.to_ascii_uppercase();
        self.bg_opacity = self.bg_opacity.min(100);
        self.outline = self.outline.min(5);
        self.position = self.position.min(100);
        self
    }
}

/// `#RRGGBB` (case-insensitive).
pub fn is_hex_color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

/// Subtitle translation engines (classic `TranslationPanel`). Persisted as `snake_case`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranslationProviderVm {
    /// Translation off.
    #[default]
    Off,
    Google,
    DeepL,
    DeepLx,
    Azure,
    /// Any OpenAI-compatible chat endpoint (`endpoint` + `model`).
    OpenAi,
}

impl TranslationProviderVm {
    pub const ALL: [TranslationProviderVm; 6] = [Self::Off, Self::Google, Self::DeepL, Self::DeepLx, Self::Azure, Self::OpenAi];

    /// i18n suffix (`translate.provider.<id>`) and `data-provider` value.
    pub fn id(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Google => "google",
            Self::DeepL => "deepl",
            Self::DeepLx => "deeplx",
            Self::Azure => "azure",
            Self::OpenAi => "openai",
        }
    }

    pub fn needs_key(self) -> bool {
        matches!(self, Self::DeepL | Self::Azure | Self::OpenAi)
    }

    pub fn needs_endpoint(self) -> bool {
        matches!(self, Self::DeepLx | Self::OpenAi)
    }
}

/// Subtitle translation settings (Settings > Subtitles > Translation).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TranslationPrefs {
    pub provider: TranslationProviderVm,
    /// DeepLX / OpenAI-compatible base URL; empty = provider default.
    pub endpoint: String,
    // No `api_key` here (plan 14 decision, 2026-11-09): provider keys live in `tk_core::Secrets` under
    // `keys::translation(provider.id())`, written through `Backend::set_translation_key`. A legacy `api_key` field in
    // an old `ui-settings.json` is ignored by serde.
    /// OpenAI-compatible model name.
    pub model: String,
    /// Azure region.
    pub region: String,
    /// ISO 639-1 target language.
    pub target_lang: String,
    /// Translate the visible cue as it shows (needs the cue text: external SRT/VTT/ASS tracks).
    pub live: bool,
    /// Translate the whole track before playback and load it as a secondary subtitle.
    pub pretranslate: bool,
}

impl Default for TranslationPrefs {
    fn default() -> Self {
        Self {
            provider: TranslationProviderVm::Off,
            endpoint: String::new(),
            model: String::new(),
            region: String::new(),
            target_lang: "es".to_owned(),
            live: false,
            pretranslate: false,
        }
    }
}

impl TranslationPrefs {
    pub fn sanitized(mut self) -> Self {
        for f in [&mut self.endpoint, &mut self.model, &mut self.region] {
            *f = f.trim().to_owned();
        }
        self.target_lang = self.target_lang.trim().to_ascii_lowercase();
        if self.target_lang.len() != 2 || !self.target_lang.chars().all(|c| c.is_ascii_lowercase()) {
            self.target_lang = Self::default().target_lang;
        }
        self
    }
}

/// `Settings::subs`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SubtitlePrefs {
    pub style: SubtitleStyleVm,
    /// Secondary subtitle language (shown on top, mpv `secondary-sid`); `None` = off.
    pub secondary_lang: Option<String>,
    pub translation: TranslationPrefs,
}

impl SubtitlePrefs {
    pub fn sanitized(mut self) -> Self {
        self.style = self.style.sanitized();
        self.secondary_lang = self
            .secondary_lang
            .map(|l| l.trim().to_ascii_lowercase())
            .filter(|l| l.len() == 2 && l.chars().all(|c| c.is_ascii_lowercase()));
        self.translation = self.translation.sanitized();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_survive_sanitize_and_bounds_are_clamped() {
        let d = SubtitlePrefs::default();
        assert_eq!(d.clone().sanitized(), d);
        let bad = SubtitlePrefs {
            style: SubtitleStyleVm { size: 10, color: "red".into(), bg_opacity: 200, outline: 9, position: 150, ..Default::default() },
            secondary_lang: Some(" EN ".into()),
            translation: TranslationPrefs { target_lang: "ESP".into(), ..Default::default() },
        }
        .sanitized();
        assert_eq!((bad.style.size, bad.style.bg_opacity, bad.style.outline, bad.style.position), (50, 100, 5, 100));
        assert_eq!(bad.style.color, "#FFFFFF");
        assert_eq!(bad.style.outline_color, "#000000");
        let lower = SubtitleStyleVm { color: "#ffcc00".into(), outline_color: "x".into(), ..Default::default() }.sanitized();
        assert_eq!((lower.color.as_str(), lower.outline_color.as_str()), ("#FFCC00", "#000000"));
        assert_eq!(bad.secondary_lang.as_deref(), Some("en"));
        assert_eq!(bad.translation.target_lang, "es");
    }

    #[test]
    fn provider_ids_round_trip_through_serde() {
        for p in TranslationProviderVm::ALL {
            let json = serde_json::to_string(&p).unwrap();
            assert_eq!(json, format!("\"{}\"", p.id()));
            assert_eq!(serde_json::from_str::<TranslationProviderVm>(&json).unwrap(), p);
        }
        assert!(is_hex_color("#a1B2c3") && !is_hex_color("#a1B2c") && !is_hex_color("a1B2c3f"));
        assert!(TRANSLATE_LANGS.iter().chain(SECONDARY_LANGS.iter()).all(|l| l.len() == 2));
    }

    /// A settings file written before `outline_color` existed still loads.
    #[test]
    fn older_style_json_gets_the_new_defaults() {
        let s: SubtitleStyleVm = serde_json::from_str(r##"{"size":120,"color":"#00FF00"}"##).unwrap();
        assert_eq!((s.size, s.outline_color.as_str(), s.outline), (120, "#000000", 2));
    }
}
