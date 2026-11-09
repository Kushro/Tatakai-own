//! Narration engines (wave B, worker **m**; plan 14 section 0.1 #13; docs/redesign/16-parity-wave-b-contract.md
//! section 3.8; checklist `novel.tts.*`; classic `lib/novel` TTS + `desktop/services/kitten-tts.cjs`). Contract
//! skeleton: m fills the bodies, the public shape below is final.
//!
//! No TTS crate (section 3.7 of the contract): the system voice goes through an external process so no native
//! library is linked (Linux `spd-say` of speech-dispatcher, or `espeak-ng` as fallback; Windows PowerShell
//! `System.Speech.Synthesis.SpeechSynthesizer` writing a WAV with `SetOutputToWaveFile`; macOS `say -o`), the cloud
//! providers are plain `reqwest` calls (`POST /v1/audio/speech` for OpenAI-compatible, `POST /v1/text-to-speech/<voice>`
//! for ElevenLabs), and KittenTTS is a local Python daemon (`python3 -m kittentts` style script m ships under
//! `<data>/tts/kitten/`, model downloaded on demand with progress) spoken to over stdin / stdout JSON lines. Clips are
//! WAV / MP3 files under `<data>/tts/cache/<sha256>.{wav,mp3}` (row in `tts_cache`), served to the reader's `<audio>`
//! by the app's asset handler.

use tk_ui::backend::{KittenStatusVm, TtsRequestVm, TtsSpeakVm, TtsVoiceVm, UiResult};
use tk_ui::model::{TtsProviderVm, UiError};

use crate::paths::AppPaths;

/// Where the engines keep their files: `<data>/tts/cache`, `<data>/tts/kitten`.
pub const TTS_DIR: &str = "tts";

pub struct Tts {
    #[allow(dead_code)]
    paths: AppPaths,
    #[allow(dead_code)]
    client: reqwest::Client,
}

impl Tts {
    pub fn new(paths: AppPaths, client: reqwest::Client) -> Self {
        Self { paths, client }
    }

    /// `<data>/tts`.
    pub fn dir(&self) -> std::path::PathBuf {
        self.paths.root.join(TTS_DIR)
    }

    pub async fn voices(&self, _provider: TtsProviderVm, _api_key: Option<&str>) -> UiResult<Vec<TtsVoiceVm>> {
        Err(UiError::not_wired("tts_voices"))
    }
    /// `api_key` is read by the caller from `Secrets` (`keys::tts(provider.id())`); never stored here.
    pub async fn speak(&self, _req: &TtsRequestVm, _api_key: Option<&str>, _endpoint: Option<&str>, _model: Option<&str>) -> UiResult<TtsSpeakVm> {
        Err(UiError::not_wired("tts_speak"))
    }
    pub async fn stop(&self) -> UiResult<()> {
        Err(UiError::not_wired("tts_stop"))
    }
    pub async fn kitten_status(&self) -> UiResult<KittenStatusVm> {
        Err(UiError::not_wired("kitten_status"))
    }
    pub async fn kitten_install(&self) -> UiResult<KittenStatusVm> {
        Err(UiError::not_wired("kitten_install"))
    }
    pub async fn kitten_remove(&self) -> UiResult<()> {
        Err(UiError::not_wired("kitten_remove"))
    }
}
