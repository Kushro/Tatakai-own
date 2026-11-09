//! Narration engines (wave B, worker **m**; plan 14 section 0.1 #13; docs/redesign/16-parity-wave-b-contract.md
//! section 0.12; checklist `novel.tts.*`; classic `lib/novel` TTS + `desktop/services/kitten-tts.cjs`).
//!
//! No TTS crate (section 3.7 of the contract): the system voice goes through an external process so no native
//! library is linked ([`system`]: Linux `espeak-ng -w` writing a WAV, or `spd-say` of speech-dispatcher speaking on
//! its own; Windows PowerShell `System.Speech.Synthesis.SpeechSynthesizer` writing a WAV with `SetOutputToWaveFile`;
//! macOS `say -o`), the cloud providers are plain `reqwest` calls ([`cloud`]: `POST /v1/audio/speech` for OpenAI and
//! OpenAI-compatible servers, `POST /v1/text-to-speech/<voice>` for ElevenLabs).
//!
//! KittenTTS runs **in process over ONNX Runtime** ([`kitten`]; `ort` crate, feature `load-dynamic`; user decision
//! 2026-11-09, no Python). Nothing is linked or downloaded at build time: `kitten_install` downloads, with progress
//! (`tk_ui::model::novel::set_kitten_progress`), into `<data>/tts/kitten/` the ONNX Runtime shared library of a
//! pinned release and the model of `huggingface.co/KittenML/<repo>`; then `ort::init_from(<runtime lib>)` loads it
//! once. Text -> phonemes goes through the external `espeak-ng --ipa -q` process ([`text`] holds the front end).
//!
//! Clips are WAV / MP3 files under `<data>/tts/cache/<key>.{wav,mp3}` (`key` = [`Tts::clip_key`]); the caller
//! (`live/novel.rs`) records them in the `tts_cache` table and the reader's `<audio>` fetches them through the UI
//! asset handler (`tk_ui::model::novel::clip_url`). Child modules live in `src/tts/` (declared here: `main.rs` is
//! closed and `#[path]` is not used).

mod cloud;
mod kitten;
mod system;
mod text;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};

use sha2::{Digest, Sha256};
use tk_ui::backend::{KittenStatusVm, TtsClipVm, TtsRequestVm, TtsSpeakVm, TtsVoiceVm, UiResult};
use tk_ui::model::novel::clip_url;
use tk_ui::model::{ErrorKind, TtsProviderVm, UiError};

use crate::paths::AppPaths;

/// Where the engines keep their files: `<data>/tts/cache`, `<data>/tts/kitten`.
pub const TTS_DIR: &str = "tts";
/// Clip cache folder under [`TTS_DIR`].
pub const CACHE_DIR: &str = "cache";
/// Longest text one request synthesizes (a paragraph; the reader never sends more).
pub const MAX_TEXT: usize = 4000;

pub struct Tts {
    paths: AppPaths,
    client: reqwest::Client,
    /// Pitch percent for the system voices (`TtsPrefs::pitch`), set by `live/novel.rs` before each request.
    pitch: AtomicU8,
    /// Ends native speech (`spd-say`) on `stop`.
    stop: tokio::sync::Notify,
    kitten: kitten::Kitten,
}

pub(crate) fn err(e: impl std::fmt::Display) -> UiError {
    UiError::new(ErrorKind::Other, e.to_string())
}

pub(crate) fn net_err(e: reqwest::Error) -> UiError {
    let kind = if e.is_connect() || e.is_timeout() {
        ErrorKind::Offline
    } else {
        ErrorKind::Other
    };
    UiError::new(kind, e.without_url().to_string())
}

impl Tts {
    pub fn new(paths: AppPaths, client: reqwest::Client) -> Self {
        let kitten = kitten::Kitten::new(paths.root.join(TTS_DIR).join(kitten::KITTEN_DIR));
        Self {
            paths,
            client,
            pitch: AtomicU8::new(100),
            stop: tokio::sync::Notify::new(),
            kitten,
        }
    }

    /// `<data>/tts`.
    pub fn dir(&self) -> std::path::PathBuf {
        self.paths.root.join(TTS_DIR)
    }

    /// `<data>/tts/cache`.
    pub fn cache_dir(&self) -> PathBuf {
        self.dir().join(CACHE_DIR)
    }

    /// The HTTP client of the app (shared with `live/novel.rs` for the AniList novel queries).
    pub fn client(&self) -> reqwest::Client {
        self.client.clone()
    }

    /// System voice pitch percent (`50..=200`).
    pub fn set_pitch(&self, pitch: u8) {
        self.pitch.store(pitch.clamp(50, 200), Ordering::Relaxed);
    }

    /// Cache key of a request: sha256 of everything that changes the audio.
    pub fn clip_key(&self, req: &TtsRequestVm, model: Option<&str>, endpoint: Option<&str>) -> String {
        clip_key(req, self.pitch.load(Ordering::Relaxed), model, endpoint)
    }

    pub async fn voices(&self, provider: TtsProviderVm, api_key: Option<&str>) -> UiResult<Vec<TtsVoiceVm>> {
        match provider {
            TtsProviderVm::System => system::voices().await,
            TtsProviderVm::OpenAi => Ok(cloud::openai_voices()),
            TtsProviderVm::OpenAiCompatible => {
                Ok(cloud::compatible_voices(&self.client, None, api_key).await)
            }
            TtsProviderVm::ElevenLabs => {
                let key = api_key.filter(|k| !k.trim().is_empty()).ok_or_else(missing_key)?;
                cloud::elevenlabs_voices(&self.client, key).await
            }
            TtsProviderVm::Kitten => Ok(kitten::voices()),
        }
    }

    /// Voices of an OpenAI-compatible server at `endpoint` (its `/audio/voices` when it has one).
    pub async fn compatible_voices(&self, endpoint: &str, api_key: Option<&str>) -> Vec<TtsVoiceVm> {
        cloud::compatible_voices(&self.client, Some(endpoint), api_key).await
    }

    /// `api_key` is read by the caller from `Secrets` (`keys::tts(provider.id())`); never stored here.
    pub async fn speak(
        &self,
        req: &TtsRequestVm,
        api_key: Option<&str>,
        endpoint: Option<&str>,
        model: Option<&str>,
    ) -> UiResult<TtsSpeakVm> {
        let text: String = req.text.trim().chars().take(MAX_TEXT).collect();
        if text.is_empty() {
            return Err(UiError::new(ErrorKind::Other, "nothing to read"));
        }
        let req = TtsRequestVm { text, ..req.clone() };
        let key = self.clip_key(&req, model, endpoint);
        let dir = self.cache_dir();
        tokio::fs::create_dir_all(&dir).await.map_err(err)?;
        let rate = req.rate.clamp(50, 200);
        let voice = req.voice.as_deref().map(str::trim).filter(|v| !v.is_empty());
        match req.provider {
            TtsProviderVm::System => {
                let pitch = self.pitch.load(Ordering::Relaxed);
                let target = dir.join(format!("{key}.wav"));
                match system::speak(&req.text, voice, rate, pitch, &target, &self.stop).await? {
                    system::Spoken::File => clip_of(&target, "audio/wav").await.map(|c| TtsSpeakVm {
                        clip: Some(c),
                        native: false,
                    }),
                    system::Spoken::Native => Ok(TtsSpeakVm {
                        clip: None,
                        native: true,
                    }),
                }
            }
            TtsProviderVm::OpenAi | TtsProviderVm::OpenAiCompatible => {
                let compatible = req.provider == TtsProviderVm::OpenAiCompatible;
                let base = if compatible {
                    endpoint
                        .filter(|e| !e.trim().is_empty())
                        .ok_or_else(|| err("the OpenAI-compatible server URL is empty"))?
                } else {
                    cloud::OPENAI_BASE
                };
                let key_opt = api_key.filter(|k| !k.trim().is_empty());
                if !compatible && key_opt.is_none() {
                    return Err(missing_key());
                }
                let bytes =
                    cloud::openai_speech(&self.client, base, key_opt, &req.text, voice, rate, model).await?;
                let (ext, mime) = audio_kind(&bytes);
                let target = dir.join(format!("{key}.{ext}"));
                write_clip(&target, &bytes).await?;
                clip_of(&target, mime).await.map(|c| TtsSpeakVm {
                    clip: Some(c),
                    native: false,
                })
            }
            TtsProviderVm::ElevenLabs => {
                let key_s = api_key.filter(|k| !k.trim().is_empty()).ok_or_else(missing_key)?;
                let bytes =
                    cloud::elevenlabs_speech(&self.client, key_s, &req.text, voice, rate, model).await?;
                let target = dir.join(format!("{key}.mp3"));
                write_clip(&target, &bytes).await?;
                clip_of(&target, "audio/mpeg").await.map(|c| TtsSpeakVm {
                    clip: Some(c),
                    native: false,
                })
            }
            TtsProviderVm::Kitten => {
                let model_id = model
                    .unwrap_or(tk_ui::model::novel::KITTEN_DEFAULT_MODEL)
                    .to_owned();
                let target = dir.join(format!("{key}.wav"));
                self.kitten
                    .speak(&model_id, &req.text, voice, rate, &target)
                    .await?;
                clip_of(&target, "audio/wav").await.map(|c| TtsSpeakVm {
                    clip: Some(c),
                    native: false,
                })
            }
        }
    }

    /// Ends native speech; clips are stopped by the UI's `<audio>`.
    pub async fn stop(&self) -> UiResult<()> {
        self.stop.notify_waiters();
        Ok(())
    }

    pub async fn kitten_status(&self) -> UiResult<KittenStatusVm> {
        Ok(self.kitten.status(&self.current_kitten_model()).await)
    }

    /// Model the install / status uses (`Settings::novel.tts`); set by `live/novel.rs` before the call.
    fn current_kitten_model(&self) -> String {
        self.kitten.selected_model()
    }

    /// The KittenTTS model id `kitten_status` / `kitten_install` work on.
    pub fn select_kitten_model(&self, id: &str) {
        self.kitten.select_model(id);
    }

    pub async fn kitten_install(&self) -> UiResult<KittenStatusVm> {
        let model = self.current_kitten_model();
        self.kitten.install(&self.client, &model).await?;
        Ok(self.kitten.status(&model).await)
    }

    pub async fn kitten_remove(&self) -> UiResult<()> {
        self.kitten.remove().await
    }
}

fn missing_key() -> UiError {
    UiError::new(ErrorKind::Auth, "no API key saved for this narration provider")
}

/// sha256(provider | voice | rate | pitch | model | endpoint | text) as hex.
pub fn clip_key(req: &TtsRequestVm, pitch: u8, model: Option<&str>, endpoint: Option<&str>) -> String {
    let mut h = Sha256::new();
    for part in [
        req.provider.id(),
        req.voice.as_deref().unwrap_or(""),
        &req.rate.to_string(),
        &pitch.to_string(),
        model.unwrap_or(""),
        endpoint.unwrap_or(""),
        req.text.trim(),
    ] {
        h.update(part.as_bytes());
        h.update([0x1f]);
    }
    hex_of(&h.finalize())
}

fn hex_of(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// `(extension, mime)` of an audio body by its magic bytes (`RIFF` = WAV, else MP3).
pub fn audio_kind(bytes: &[u8]) -> (&'static str, &'static str) {
    if bytes.starts_with(b"RIFF") {
        ("wav", "audio/wav")
    } else {
        ("mp3", "audio/mpeg")
    }
}

async fn write_clip(path: &Path, bytes: &[u8]) -> UiResult<()> {
    if bytes.is_empty() {
        return Err(err("the narration service returned no audio"));
    }
    let tmp = path.with_extension("part");
    tokio::fs::write(&tmp, bytes).await.map_err(err)?;
    tokio::fs::rename(&tmp, path).await.map_err(err)
}

/// Clip VM of a file the engines wrote (duration read from the WAV header).
async fn clip_of(path: &Path, mime: &str) -> UiResult<TtsClipVm> {
    let bytes = tokio::fs::read(path).await.map_err(err)?;
    if bytes.is_empty() {
        return Err(err("the narration engine produced an empty file"));
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(TtsClipVm {
        path: path.to_path_buf(),
        url: clip_url(&name),
        mime: mime.to_owned(),
        duration_ms: if mime == "audio/wav" {
            wav_duration_ms(&bytes)
        } else {
            None
        },
    })
}

/// Duration of a PCM WAV from its `fmt ` byte rate and `data` size.
pub fn wav_duration_ms(bytes: &[u8]) -> Option<u32> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let mut i = 12;
    let mut byte_rate: Option<u32> = None;
    while i + 8 <= bytes.len() {
        let id = &bytes[i..i + 4];
        let size = u32::from_le_bytes([bytes[i + 4], bytes[i + 5], bytes[i + 6], bytes[i + 7]]) as usize;
        let body = i + 8;
        if id == b"fmt " && body + 12 <= bytes.len() {
            byte_rate = Some(u32::from_le_bytes([
                bytes[body + 8],
                bytes[body + 9],
                bytes[body + 10],
                bytes[body + 11],
            ]));
        }
        if id == b"data" {
            let rate = byte_rate.filter(|r| *r > 0)?;
            // A streamed WAV (espeak-ng to a pipe) may carry 0 / 0xFFFFFFFF: use what is there.
            let data = if size == 0 || size == u32::MAX as usize {
                bytes.len().saturating_sub(body)
            } else {
                size
            };
            return Some(((data as u64 * 1000) / u64::from(rate)) as u32);
        }
        i = body + size + (size & 1);
    }
    None
}

/// 16-bit PCM mono WAV of `samples` (`-1.0..=1.0`).
pub fn wav_bytes(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + samples.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// First executable called `name` on `PATH` (plus the usual install folders GUI apps do not inherit).
pub fn which(name: &str) -> Option<PathBuf> {
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT".into())
            .split(';')
            .map(str::to_ascii_lowercase)
            .collect()
    } else {
        vec![String::new()]
    };
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    if cfg!(target_os = "macos") {
        dirs.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from));
    } else if cfg!(windows) {
        for var in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(pf) = std::env::var_os(var) {
                dirs.push(PathBuf::from(pf).join("eSpeak NG"));
            }
        }
    } else {
        dirs.extend(["/usr/bin", "/usr/local/bin"].map(PathBuf::from));
    }
    for d in dirs {
        for ext in &exts {
            let p = d.join(format!("{name}{ext}"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// A `std::process::Command` that never flashes a console window on Windows.
pub(crate) fn quiet_command(program: &Path) -> std::process::Command {
    #[allow(unused_mut)]
    let mut cmd = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(text: &str) -> TtsRequestVm {
        TtsRequestVm {
            text: text.into(),
            provider: TtsProviderVm::System,
            voice: None,
            rate: 100,
        }
    }

    #[test]
    fn keys_change_with_every_input() {
        let a = clip_key(&req("hola"), 100, None, None);
        assert_eq!(a.len(), 64);
        assert_eq!(a, clip_key(&req(" hola "), 100, None, None));
        assert_ne!(a, clip_key(&req("hola!"), 100, None, None));
        assert_ne!(a, clip_key(&req("hola"), 120, None, None));
        assert_ne!(a, clip_key(&req("hola"), 100, Some("tts-1"), None));
        let mut r = req("hola");
        r.provider = TtsProviderVm::OpenAi;
        assert_ne!(a, clip_key(&r, 100, None, None));
    }

    #[test]
    fn wav_round_trip() {
        let samples = vec![0.0_f32; 24_000];
        let wav = wav_bytes(&samples, 24_000);
        assert_eq!(wav.len(), 44 + 48_000);
        assert_eq!(wav_duration_ms(&wav), Some(1000));
        assert_eq!(audio_kind(&wav), ("wav", "audio/wav"));
        assert_eq!(audio_kind(b"ID3...."), ("mp3", "audio/mpeg"));
        assert_eq!(wav_duration_ms(b"nope"), None);
    }
}
