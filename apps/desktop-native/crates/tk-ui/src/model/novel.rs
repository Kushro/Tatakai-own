//! Novel reader + TTS preferences and view-models (wave B, worker **m**; docs/redesign/16-parity-wave-b-contract.md
//! section 3.6). `NovelPrefs` is embedded in `Settings::novel` (`SettingsAction::SetNovel`). Worker m owns this
//! file; `model/mod.rs` stays frozen. Chapter / progress types come from `model/manga.rs` (worker l, shared).

use std::path::PathBuf;
use std::sync::Mutex;

use base64::Engine;
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
pub const PARAGRAPH_GAP_MAX: u8 = 30;

/// Built-in font choices of the reader (`NovelPrefs::font`); `custom` = the picked file, `family:<name>` = a font
/// installed on the system (`novel.reader.typography`: sans, Georgia, Lora, dyslexia-friendly, installed).
pub const READER_FONTS: [&str; 7] = ["serif", "sans", "mono", "georgia", "lora", "dyslexic", "custom"];
/// Prefix of an installed system font family in `NovelPrefs::font`.
pub const FAMILY_PREFIX: &str = "family:";

/// KittenTTS models offered for download: `(id, Hugging Face repo, approximate MB)`; same ids as the classic
/// `desktop/services/kitten-tts.cjs` (`kitten-2` left out for size, plan 14 section 0.1 #13).
pub const KITTEN_MODELS: [(&str, &str, u32); 2] = [
    ("nano-int8", "KittenML/kitten-tts-nano-0.8-int8", 25),
    ("mini", "KittenML/kitten-tts-mini-0.8", 80),
];
/// Default KittenTTS model.
pub const KITTEN_DEFAULT_MODEL: &str = "nano-int8";
/// Approximate size of the ONNX Runtime library KittenTTS downloads with the model.
pub const KITTEN_RUNTIME_MB: u32 = 30;
/// The eight voices of the legacy KittenTTS ONNX models (classic `LEGACY_VOICES`).
pub const KITTEN_VOICES: [&str; 8] = ["Bella", "Jasper", "Luna", "Bruno", "Rosie", "Hugo", "Kiki", "Leo"];

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
    pub const ALL: [TtsProviderVm; 5] = [
        Self::System,
        Self::OpenAi,
        Self::OpenAiCompatible,
        Self::ElevenLabs,
        Self::Kitten,
    ];

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

/// Reading flow of the novel reader (`novel.reader.flow`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NovelFlowPrefs {
    /// Append the next chapter when the end is reached (continuous scroll).
    pub continuous: bool,
    /// Reading progress bar at the top of the reader.
    pub show_progress: bool,
    /// Percent read in the reader bar.
    pub show_percent: bool,
    /// Chapter number in the reader bar.
    pub show_chapter: bool,
    /// Keep the screen on while the reader is open (Screen Wake Lock of the web view).
    pub keep_awake: bool,
}

impl Default for NovelFlowPrefs {
    fn default() -> Self {
        Self {
            continuous: true,
            show_progress: true,
            show_percent: true,
            show_chapter: true,
            keep_awake: false,
        }
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
    /// Reading flow (added by worker m with `serde(default)`; older files load the defaults).
    pub flow: NovelFlowPrefs,
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
            flow: NovelFlowPrefs::default(),
        }
    }
}

impl NovelPrefs {
    pub fn sanitized(mut self) -> Self {
        self.font = self.font.trim().to_owned();
        let family_ok = self.font.strip_prefix(FAMILY_PREFIX).is_some_and(|f| {
            let f = f.trim();
            !f.is_empty() && f.len() <= 120 && !f.contains(['"', '\\', ';', '{', '}', '<', '>'])
        });
        if !family_ok && !READER_FONTS.contains(&self.font.as_str()) {
            self.font = "serif".to_owned();
        }
        self.custom_font_path = self
            .custom_font_path
            .map(|p| p.trim().to_owned())
            .filter(|p| !p.is_empty());
        if self.font == "custom" && self.custom_font_path.is_none() {
            self.font = "serif".to_owned();
        }
        self.font_size = self.font_size.clamp(FONT_SIZE_MIN, FONT_SIZE_MAX);
        self.line_height = self.line_height.clamp(LINE_HEIGHT_MIN, LINE_HEIGHT_MAX);
        self.paragraph_gap = self.paragraph_gap.min(PARAGRAPH_GAP_MAX);
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

impl NovelPrefs {
    /// CSS `font-family` of the reader for [`NovelPrefs::font`]. `custom` uses the `@font-face` the reader declares
    /// as `TkNovelCustom`; installed families are quoted. Only generic fallbacks, never a web font URL.
    pub fn font_stack(&self) -> String {
        if let Some(family) = self.font.strip_prefix(FAMILY_PREFIX) {
            return format!("\"{}\", var(--tk-font-text), serif", family.trim());
        }
        match self.font.as_str() {
            "sans" => "var(--tk-font-text), system-ui, sans-serif".to_owned(),
            "mono" => "var(--tk-font-mono), ui-monospace, monospace".to_owned(),
            "georgia" => "Georgia, \"Times New Roman\", serif".to_owned(),
            "lora" => "Lora, Georgia, serif".to_owned(),
            "dyslexic" => "OpenDyslexic, \"Atkinson Hyperlegible\", \"Comic Sans MS\", sans-serif".to_owned(),
            "custom" => "TkNovelCustom, Georgia, serif".to_owned(),
            _ => "Georgia, \"Noto Serif\", \"Times New Roman\", serif".to_owned(),
        }
    }
}

impl TtsPrefs {
    /// KittenTTS model id of these prefs: `model` holds it while the provider is `Kitten` (the UI clears `model`
    /// when the provider changes), else the default.
    pub fn kitten_model(&self) -> &'static str {
        let want = if self.provider == TtsProviderVm::Kitten {
            self.model.as_deref()
        } else {
            None
        };
        KITTEN_MODELS
            .iter()
            .map(|(id, _, _)| *id)
            .find(|id| Some(*id) == want)
            .unwrap_or(KITTEN_DEFAULT_MODEL)
    }
}

/// Name of the web view asset handler the novel screens register (`/tk-novel/clip/<file>` narration clips,
/// `/tk-novel/font/<file>` the custom reader font).
pub const ASSET_HANDLER: &str = "tk-novel";

/// URL of a narration clip file (by file name) for the reader's audio element.
pub fn clip_url(file_name: &str) -> String {
    format!("/{ASSET_HANDLER}/clip/{file_name}")
}

/// `(repo, approximate MB)` of a KittenTTS model id.
pub fn kitten_model_info(id: &str) -> Option<(&'static str, u32)> {
    KITTEN_MODELS
        .iter()
        .find(|(m, _, _)| *m == id)
        .map(|(_, repo, mb)| (*repo, *mb))
}

/// Progress of a running `Backend::kitten_install` (download of the ONNX Runtime library and the model). tk-app
/// writes it with [`set_kitten_progress`] from its download loop; the settings row polls [`kitten_progress`] while
/// the install call is pending (same process, so a plain static is enough).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KittenProgressVm {
    /// `runtime` | `model` | `voices` | `config` | `extract`.
    pub stage: String,
    pub done: u64,
    pub total: Option<u64>,
}

impl KittenProgressVm {
    /// 0..=100 when the total is known.
    pub fn percent(&self) -> Option<u8> {
        self.total
            .filter(|t| *t > 0)
            .map(|t| ((self.done.min(t) * 100) / t) as u8)
    }
}

static KITTEN_PROGRESS: Mutex<Option<KittenProgressVm>> = Mutex::new(None);

pub fn set_kitten_progress(p: Option<KittenProgressVm>) {
    if let Ok(mut g) = KITTEN_PROGRESS.lock() {
        *g = p;
    }
}

pub fn kitten_progress() -> Option<KittenProgressVm> {
    KITTEN_PROGRESS.lock().ok().and_then(|g| g.clone())
}

/// Chapter number carried by a novel `ChapterRefVm::chapter_id`. The id is tk-core's `novel::ChapterKey`
/// (URL-safe base64 of a JSON object whose `n` field is the number); `None` for any other id.
pub fn chapter_number_of(chapter_id: &str) -> Option<f32> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(chapter_id.trim().as_bytes())
        .ok()?;
    let v: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    v.get("n")
        .and_then(serde_json::Value::as_f64)
        .map(|n| n as f32)
        .filter(|n| n.is_finite())
}

/// Display form of a chapter number (`12`, `12.5`).
pub fn chapter_label(number: f32) -> String {
    if number.fract().abs() < f32::EPSILON {
        format!("{}", number as i64)
    } else {
        format!("{number:.1}")
    }
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
        let p = NovelPrefs {
            font: "custom".into(),
            custom_font_path: None,
            font_size: 99,
            ..NovelPrefs::default()
        }
        .sanitized();
        assert_eq!(p.font, "serif");
        assert_eq!(p.font_size, FONT_SIZE_MAX);
    }

    #[test]
    fn providers_needing_keys() {
        assert!(TtsProviderVm::OpenAi.needs_key());
        assert!(!TtsProviderVm::System.needs_key());
        assert!(!TtsProviderVm::Kitten.needs_key());
        assert_eq!(
            TtsProviderVm::from_id("elevenlabs"),
            Some(TtsProviderVm::ElevenLabs)
        );
    }

    #[test]
    fn fonts_and_families() {
        for f in READER_FONTS.iter().filter(|f| **f != "custom") {
            let p = NovelPrefs {
                font: (*f).into(),
                ..NovelPrefs::default()
            }
            .sanitized();
            assert_eq!(p.font, *f);
            assert!(!p.font_stack().is_empty());
        }
        let p = NovelPrefs {
            font: "family:Noto Serif JP".into(),
            ..NovelPrefs::default()
        }
        .sanitized();
        assert_eq!(p.font, "family:Noto Serif JP");
        assert!(p.font_stack().starts_with("\"Noto Serif JP\""));
        let bad = NovelPrefs {
            font: "family:x;}body{".into(),
            ..NovelPrefs::default()
        }
        .sanitized();
        assert_eq!(bad.font, "serif");
        let custom = NovelPrefs {
            font: "custom".into(),
            custom_font_path: Some("/f.ttf".into()),
            ..NovelPrefs::default()
        };
        assert_eq!(custom.sanitized().font_stack(), "TkNovelCustom, Georgia, serif");
    }

    #[test]
    fn kitten_model_follows_the_provider() {
        let mut t = TtsPrefs {
            provider: TtsProviderVm::Kitten,
            model: Some("mini".into()),
            ..TtsPrefs::default()
        };
        assert_eq!(t.kitten_model(), "mini");
        t.provider = TtsProviderVm::OpenAi;
        assert_eq!(t.kitten_model(), KITTEN_DEFAULT_MODEL);
        assert_eq!(kitten_model_info("mini").map(|i| i.1), Some(80));
        assert_eq!(kitten_model_info("kitten-2"), None);
    }

    #[test]
    fn chapter_numbers_from_ids() {
        // {"m":"1","p":"a","k":"a:1","n":12.5}
        let json = br#"{"m":"1","p":"a","k":"a:1","n":12.5}"#;
        let id = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json);
        assert_eq!(chapter_number_of(&id), Some(12.5));
        assert_eq!(chapter_number_of("plain-id"), None);
        assert_eq!(chapter_label(3.0), "3");
        assert_eq!(chapter_label(3.5), "3.5");
    }

    #[test]
    fn progress_percent() {
        set_kitten_progress(Some(KittenProgressVm {
            stage: "model".into(),
            done: 50,
            total: Some(200),
        }));
        assert_eq!(kitten_progress().and_then(|p| p.percent()), Some(25));
        set_kitten_progress(None);
        assert_eq!(kitten_progress(), None);
        assert_eq!(
            KittenProgressVm {
                stage: "x".into(),
                done: 1,
                total: None
            }
            .percent(),
            None
        );
    }
    // ---- F6 (parity wave B) ----

    #[test]
    fn typography_is_clamped_both_ways() {
        let lo = NovelPrefs { font_size: 1, line_height: 10, paragraph_gap: 0, max_width: 10, ..NovelPrefs::default() }.sanitized();
        assert_eq!((lo.font_size, lo.line_height, lo.paragraph_gap, lo.max_width), (FONT_SIZE_MIN, LINE_HEIGHT_MIN, 0, MAX_WIDTH_MIN));
        let hi = NovelPrefs { font_size: 255, line_height: 255, paragraph_gap: 255, max_width: u16::MAX, ..NovelPrefs::default() }.sanitized();
        assert_eq!((hi.font_size, hi.line_height, hi.paragraph_gap, hi.max_width), (FONT_SIZE_MAX, LINE_HEIGHT_MAX, PARAGRAPH_GAP_MAX, MAX_WIDTH_MAX));
    }

    #[test]
    fn unknown_fonts_and_bad_families_fall_back_to_serif() {
        let long = format!("family:{}", "f".repeat(121));
        for bad in ["comic", "", "family:", "family:   ", "family:a\"b", "family:<x>", long.as_str()] {
            let p = NovelPrefs { font: bad.to_owned(), ..NovelPrefs::default() }.sanitized();
            assert_eq!(p.font, "serif", "{bad:?}");
        }
        let p = NovelPrefs { font: "  sans  ".into(), ..NovelPrefs::default() }.sanitized();
        assert_eq!(p.font, "sans");
        // a blank custom path counts as none
        let p = NovelPrefs { font: "custom".into(), custom_font_path: Some("   ".into()), ..NovelPrefs::default() }.sanitized();
        assert_eq!(p.font, "serif");
        assert_eq!(p.custom_font_path, None);
        let p = NovelPrefs { font: "custom".into(), custom_font_path: Some(" /f.otf ".into()), ..NovelPrefs::default() }.sanitized();
        assert_eq!(p.font, "custom");
        assert_eq!(p.custom_font_path.as_deref(), Some("/f.otf"));
    }

    #[test]
    fn tts_prefs_are_sanitized_inside_novel_prefs() {
        let tts = TtsPrefs {
            voice: "  af_bella ".into(),
            rate: 10,
            pitch: 250,
            endpoint: Some(" https://tts.example/v1/ ".into()),
            model: Some("   ".into()),
            ..TtsPrefs::default()
        };
        let p = NovelPrefs { tts, ..NovelPrefs::default() }.sanitized();
        assert_eq!(p.tts.voice, "af_bella");
        assert_eq!(p.tts.rate, TTS_RATE_MIN);
        assert_eq!(p.tts.pitch, TTS_RATE_MAX);
        assert_eq!(p.tts.endpoint.as_deref(), Some("https://tts.example/v1"));
        assert_eq!(p.tts.model, None);
        let t = TtsPrefs { endpoint: Some("tts.example".into()), model: Some(" tts-1 ".into()), ..TtsPrefs::default() }.sanitized();
        assert_eq!(t.endpoint, None);
        assert_eq!(t.model.as_deref(), Some("tts-1"));
        assert_eq!(TtsPrefs::default().sanitized(), TtsPrefs::default());
    }

    #[test]
    fn older_files_without_flow_load_the_defaults() {
        let p: NovelPrefs = serde_json::from_str(r#"{"font":"sans","font_size":20}"#).unwrap();
        assert_eq!(p.flow, NovelFlowPrefs::default());
        assert_eq!(p.tts, TtsPrefs::default());
        assert_eq!(p.font_size, 20);
    }
}
