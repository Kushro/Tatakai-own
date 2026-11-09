//! Narration engines (wave B, worker **m**; plan 14 section 0.1 #13; docs/redesign/16-parity-wave-b-contract.md
//! section 3.8; checklist `novel.tts.*`; classic `lib/novel` TTS + `desktop/services/kitten-tts.cjs`). Contract
//! skeleton: m fills the bodies, the public shape below is final.
//!
//! No TTS crate (section 3.7 of the contract): the system voice goes through an external process so no native
//! library is linked (Linux `spd-say` of speech-dispatcher, or `espeak-ng` as fallback; Windows PowerShell
//! `System.Speech.Synthesis.SpeechSynthesizer` writing a WAV with `SetOutputToWaveFile`; macOS `say -o`), the cloud
//! providers are plain `reqwest` calls (`POST /v1/audio/speech` for OpenAI-compatible, `POST /v1/text-to-speech/<voice>`
//! for ElevenLabs).
//!
//! KittenTTS runs **in process over ONNX Runtime** (`ort` crate, feature `load-dynamic`; user decision 2026-11-09,
//! no Python). Nothing is linked or downloaded at build time: `kitten_install` downloads, with progress, into
//! `<data>/tts/kitten/` (1) the ONNX Runtime shared library of a pinned release (>= 1.22, `api-22`) from
//! `github.com/microsoft/onnxruntime/releases` into `runtime/` (`libonnxruntime.so` / `onnxruntime.dll` /
//! `libonnxruntime.dylib`, ~30 MB) and (2) the model of `huggingface.co/KittenML/<repo>` into `models/<id>/`
//! (`nano-int8` ~25 MB by default, `mini` ~80 MB on request; same ids and eight voices as the classic
//! `desktop/services/kitten-tts.cjs`); then `ort::init_from(<runtime lib>)` loads it once. Text -> phonemes goes
//! through the external `espeak-ng --ipa -q` process (same engine the classic used through phonemizer / WASM);
//! without `espeak-ng` the status says so and how to install it. Model I/O (token ids, voice embedding from
//! `voices.npz` read with the `zip` crate, speed, 24 kHz float output -> WAV) follows the classic daemon
//! `desktop/services/kitten-tts-daemon.py` and the Apache-2.0 JS SDK `node_modules/@biwills/kittentts/dist/runtime-*.js`
//! / `text-preprocess-*.js`. Inference runs on `tokio::task::spawn_blocking`. Clips are WAV / MP3 files under
//! `<data>/tts/cache/<sha256>.{wav,mp3}` (row in `tts_cache`), served to the reader's `<audio>` by the app's asset
//! handler.

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
