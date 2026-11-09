//! Novel reader + TTS preferences and view-models (wave B, worker **m**; docs/redesign/16-parity-wave-b-contract.md
//! section 3.6). `NovelPrefs` is embedded in `Settings::novel` (`SettingsAction::SetNovel`). Worker m owns this
//! file; `model/mod.rs` stays frozen. Chapter / progress types come from `model/manga.rs` (worker l, shared).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::backend::MediaVm;
use crate::model::manga::{ChapterRefVm, ReadProgressVm};

pub const FONT_SIZE_MIN: u8 = 12;
pub const FONT_SIZE_MAX: u8 = 32;
pub const LINE_HEIGHT_MIN: u8 = 100;
pub const LINE_HEIGHT_MAX: u8 = 250;
pub const MAX_WIDTH_MIN: u16 = 480;
pub const MAX_WIDTH_MAX: u16 = 1400;
pub const TTS_RATE_MIN: u8 = 50;
pub const TTS_RATE_MAX: u8 = 200;

/// Reader colour theme (classic novel reader settings sheet). `id()` = `novel.theme.<id>`, `data-theme`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NovelThemeVm {
    #[default]
    System,
    Dark,
    Sepia,
    Light,
}

impl NovelThemeVm {
    pub const ALL: [NovelThemeVm; 4] = [Self::System, Self::Dark, Self::Sepia, Self::Light];

    pub fn id(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Dark => "dark",
            Self::Sepia => "sepia",
            Self::Light => "light",
        }
    }
}

/// Narration providers (classic: system voice, OpenAI, ElevenLabs; plan 14 section 0.1 #13: optional local
/// KittenTTS). `id()` = `novel.tts.provider.<id>`, `data-provider`, and the `Secrets` key suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TtsProviderVm {
    #[default]
    System,
    OpenAi,
    /// Any `/v1/audio/speech`-compatible endpoint (`TtsPrefs::endpoint`).
    OpenAiCompatible,
    ElevenLabs,
    /// Local KittenTTS (`Backend::kitten_status`).
    Kitten,
}

impl TtsProviderVm {
    pub const ALL: [TtsProviderVm; 5] = [Self::System, Self::OpenAi, Self::OpenAiCompatible, Self::ElevenLabs, Self::Kitten];

    pub fn id(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::OpenAi => "openai",
            Self::OpenAiCompatible => "openai_compatible",
            Self::ElevenLabs => "elevenlabs",
            Self::Kitten => "kitten",
        }
    }

    /// Takes an API key (`Backend::set_tts_key`).
    pub fn needs_key(self) -> bool {
        matches!(self, Self::OpenAi | Self::OpenAiCompatible | Self::ElevenLabs)
    }

    pub fn needs_endpoint(self) -> bool {
        matches!(self, Self::OpenAiCompatible)
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.id() == id)
    }
}

/// `NovelPrefs::tts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TtsPrefs {
    pub provider: TtsProviderVm,
    /// Voice id of the provider (`TtsVoiceVm::id`); empty = provider default.
    pub voice: String,
    /// Speaking rate, percent `50..=200`.
    pub rate: u8,
    /// Pitch, percent `50..=200` (system voices only).
    pub pitch: u8,
    /// `OpenAiCompatible` base URL.
    pub endpoint: Option<String>,
    /// Model name for OpenAI-style providers (`tts-1`, `eleven_multilingual_v2`).
    pub model: Option<String>,
    /// Continue with the next chapter when the current one ends.
    pub auto_advance: bool,
    /// Highlight the paragraph being read.
    pub highlight: bool,
}

impl Default for TtsPrefs {
    fn default() -> Self {
        Self {
            provider: TtsProviderVm::System,
            voice: String::new(),
            rate: 100,
            pitch: 100,
            endpoint: None,
            model: None,
            auto_advance: true,
            highlight: true,
        }
    }
}

impl TtsPrefs {
    pub fn sanitized(mut self) -> Self {
        self.voice = self.voice.trim().to_owned();
        self.rate = self.rate.clamp(TTS_RATE_MIN, TTS_RATE_MAX);
        self.pitch = self.pitch.clamp(TTS_RATE_MIN, TTS_RATE_MAX);
        self.endpoint = self
            .endpoint
            .map(|u| u.trim().trim_end_matches('/').to_owned())
            .filter(|u| u.starts_with("http://") || u.starts_with("https://"));
        self.model = self.model.map(|m| m.trim().to_owned()).filter(|m| !m.is_empty());
        self
    }
}

/// `Settings::novel`: typography and narration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NovelPrefs {
    /// Font family: `serif`, `sans`, `mono` or `custom` (then `custom_font_path`).
    pub font: String,
    /// A `.ttf` / `.otf` / `.woff2` the user picked (`novel.reader.custom-font`); loaded as an `@font-face`.
    pub custom_font_path: Option<String>,
    /// px `12..=32`.
    pub font_size: u8,
    /// percent `100..=250`.
    pub line_height: u8,
    /// Paragraph gap in `em` tenths (`8` = 0.8em), `0..=30`.
    pub paragraph_gap: u8,
    /// Column width in px `480..=1400`.
    pub max_width: u16,
    pub theme: NovelThemeVm,
    pub justify: bool,
    pub tts: TtsPrefs,
}

impl Default for NovelPrefs {
    fn default() -> Self {
        Self {
            font: "serif".to_owned(),
            custom_font_path: None,
            font_size: 18,
            line_height: 160,
            paragraph_gap: 8,
            max_width: 760,
            theme: NovelThemeVm::System,
            justify: false,
            tts: TtsPrefs::default(),
        }
    }
}

impl NovelPrefs {
    pub fn sanitized(mut self) -> Self {
        if !["serif", "sans", "mono", "custom"].contains(&self.font.as_str()) {
            self.font = "serif".to_owned();
        }
        self.custom_font_path = self.custom_font_path.map(|p| p.trim().to_owned()).filter(|p| !p.is_empty());
        if self.font == "custom" && self.custom_font_path.is_none() {
            self.font = "serif".to_owned();
        }
        self.font_size = self.font_size.clamp(FONT_SIZE_MIN, FONT_SIZE_MAX);
        self.line_height = self.line_height.clamp(LINE_HEIGHT_MIN, LINE_HEIGHT_MAX);
        self.paragraph_gap = self.paragraph_gap.min(30);
        self.max_width = self.max_width.clamp(MAX_WIDTH_MIN, MAX_WIDTH_MAX);
        self.tts = self.tts.sanitized();
        self
    }
}

/// Novel details (`novel.details`): AniList (`MANGA` type, format `NOVEL`) + the novel extensions.
#[derive(Debug, Clone, PartialEq)]
pub struct NovelDetailsVm {
    pub media: MediaVm,
    pub banner: Option<String>,
    pub synopsis: Option<String>,
    pub genres: Vec<String>,
    pub status: Option<String>,
    pub chapters_total: Option<u32>,
    pub volumes_total: Option<u32>,
    pub authors: Vec<String>,
    /// Translators / groups seen across providers (`novel.details` translator filter).
    pub translators: Vec<String>,
    pub adult: bool,
    pub resume: Option<ReadProgressVm>,
}

/// One novel chapter's text (`novelContent` route, or the offline copy).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NovelContentVm {
    pub reference: ChapterRefVm,
    pub title: String,
    /// Plain-text paragraphs (inline HTML stripped; the reader renders `<p>` per entry).
    pub paragraphs: Vec<String>,
    pub prev: Option<ChapterRefVm>,
    pub next: Option<ChapterRefVm>,
    pub word_count: u32,
    /// Served from `offline_chapters`.
    pub offline: bool,
}

/// One voice of a TTS provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TtsVoiceVm {
    pub id: String,
    pub name: String,
    /// BCP-47 (`es-MX`) when known.
    pub lang: Option<String>,
    pub gender: Option<String>,
}

/// `Backend::tts_speak` input: one paragraph (or a short run of them).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TtsRequestVm {
    pub text: String,
    pub provider: TtsProviderVm,
    /// `None` = `TtsPrefs::voice`.
    pub voice: Option<String>,
    /// Percent `50..=200`.
    pub rate: u8,
}

/// A synthesized clip: `url` is what the reader's `<audio>` element plays (served by the app's asset handler from
/// `path`), `mime` = `audio/wav` / `audio/mpeg`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TtsClipVm {
    pub path: PathBuf,
    pub url: String,
    pub mime: String,
    pub duration_ms: Option<u32>,
}

/// `Backend::tts_speak` result: either a clip to play in the UI, or `native: true` when the OS voice speaks on its
/// own (`System` provider without a file-writing engine); then `tts_stop` ends it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TtsSpeakVm {
    pub clip: Option<TtsClipVm>,
    pub native: bool,
}

/// `Backend::kitten_status`: local KittenTTS availability (plan 14 section 0.1 #13).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KittenStatusVm {
    /// The engine can run: ONNX Runtime library loaded, `espeak-ng` found and the model present.
    pub available: bool,
    /// ONNX Runtime shared library loaded from `<data>/tts/kitten/runtime` (its version, display form).
    pub runtime: Option<String>,
    /// `espeak-ng` executable used for text -> phonemes (display form); `None` = not installed.
    pub phonemizer: Option<String>,
    pub model_present: bool,
    /// `<data>/tts/kitten` (display form).
    pub model_dir: String,
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sanitized() {
        let d = NovelPrefs::default();
        assert_eq!(d.clone().sanitized(), d);
    }

    #[test]
    fn custom_font_without_path_falls_back() {
        let p = NovelPrefs { font: "custom".into(), custom_font_path: None, font_size: 99, ..NovelPrefs::default() }.sanitized();
        assert_eq!(p.font, "serif");
        assert_eq!(p.font_size, FONT_SIZE_MAX);
    }

    #[test]
    fn providers_needing_keys() {
        assert!(TtsProviderVm::OpenAi.needs_key());
        assert!(!TtsProviderVm::System.needs_key());
        assert!(!TtsProviderVm::Kitten.needs_key());
        assert_eq!(TtsProviderVm::from_id("elevenlabs"), Some(TtsProviderVm::ElevenLabs));
    }
}
