//! Local KittenTTS over ONNX Runtime, in process (`novel.tts.kitten`; plan 14 section 0.1 #13; user decision
//! 2026-11-09: no Python). `ort` 2.0.0-rc.15 with `load-dynamic` + `api-22`: nothing is linked or downloaded at build
//! time. [`Kitten::install`] downloads, on request and with progress, into `<data>/tts/kitten/`:
//!
//! - `runtime/`: the ONNX Runtime shared library of the pinned release [`ORT_VERSION`] from
//!   `github.com/microsoft/onnxruntime/releases` (`.zip` on Windows read with the `zip` crate, `.tgz` elsewhere
//!   unpacked with the system `tar`), plus `VERSION`;
//! - `models/<id>/`: `config.json`, the `.onnx` model and `voices.npz` of `huggingface.co/KittenML/<repo>` (ids,
//!   repos and the eight voices as in the classic `desktop/services/kitten-tts.cjs`).
//!
//! Inference follows the classic daemon (`desktop/services/kitten-tts-daemon.py`) and the Apache-2.0 JS SDK
//! `@biwills/kittentts` (`runtime-*.js`: inputs `input_ids` int64 `[1, n]`, `style` float32 `[1, cols]` = the voice
//! matrix row `min(len(text), rows - 1)`, `speed` float32 `[1]` x the config's `speed_priors`; output 24 kHz float
//! audio minus its last 5000 samples). Text -> IPA with the external `espeak-ng --ipa -q -v en-us`.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ort::environment::Environment;
use ort::session::Session;
use ort::value::Tensor;
use serde_json::Value;
use tk_ui::backend::{KittenStatusVm, TtsVoiceVm, UiResult};
use tk_ui::model::novel::{
    KITTEN_DEFAULT_MODEL, KITTEN_VOICES, KittenProgressVm, kitten_model_info, set_kitten_progress,
};
use tk_ui::model::{ErrorKind, UiError};
use tokio::io::AsyncWriteExt;

use super::text::{self, Piece};
use super::{err, net_err, wav_bytes, which};

pub const KITTEN_DIR: &str = "kitten";
/// Pinned ONNX Runtime release (>= 1.22 for `api-22`).
pub const ORT_VERSION: &str = "1.22.0";
const DEFAULT_VOICE: &str = "Luna";
const DEFAULT_SAMPLE_RATE: u32 = 24_000;
/// Samples the model pads at the end of every chunk (`TRIM_SAMPLES` of the SDK).
const TRIM_SAMPLES: usize = 5_000;
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(1800);

/// File name of the runtime library on this platform.
pub fn runtime_lib_name() -> &'static str {
    if cfg!(windows) {
        "onnxruntime.dll"
    } else if cfg!(target_os = "macos") {
        "libonnxruntime.dylib"
    } else {
        "libonnxruntime.so"
    }
}

/// Release asset of [`ORT_VERSION`] for this platform; `None` where Microsoft ships none.
pub fn runtime_asset() -> Option<String> {
    let arch = std::env::consts::ARCH;
    let name = match (std::env::consts::OS, arch) {
        ("windows", "x86_64") => "win-x64",
        ("windows", "aarch64") => "win-arm64",
        ("linux", "x86_64") => "linux-x64",
        ("linux", "aarch64") => "linux-aarch64",
        ("macos", "aarch64") => "osx-arm64",
        ("macos", "x86_64") => "osx-x86_64",
        _ => return None,
    };
    let ext = if cfg!(windows) { "zip" } else { "tgz" };
    Some(format!("onnxruntime-{name}-{ORT_VERSION}.{ext}"))
}

pub fn runtime_url(asset: &str) -> String {
    format!("https://github.com/microsoft/onnxruntime/releases/download/v{ORT_VERSION}/{asset}")
}

/// `huggingface.co/<repo>/resolve/main/<file>`.
pub fn hf_url(repo: &str, file: &str) -> String {
    format!("https://huggingface.co/{repo}/resolve/main/{file}")
}

/// A file name a model config may name: plain, no traversal.
fn safe_file(name: &str) -> bool {
    !name.is_empty()
        && !name.contains("..")
        && !name.starts_with('/')
        && !name.contains('\\')
        && !name.contains(':')
}

/// `(model_file, voices)` of a KittenTTS `config.json`.
pub fn config_files(config: &Value) -> Result<(String, String), String> {
    let model = config["model_file"]
        .as_str()
        .map(str::trim)
        .filter(|s| safe_file(s));
    let voices = config["voices"]
        .as_str()
        .map(str::trim)
        .filter(|s| safe_file(s))
        .unwrap_or("voices.npz");
    if let Some(t) = config["type"].as_str()
        && t != "ONNX1"
        && t != "ONNX2"
    {
        return Err(format!("unsupported KittenTTS model type '{t}'"));
    }
    let model = model.ok_or("the model config names no model file")?;
    Ok((model.to_owned(), voices.to_owned()))
}

/// One voice matrix of `voices.npz` (`rows x cols` float32, row-major).
#[derive(Debug, Clone, PartialEq)]
pub struct VoiceMatrix {
    pub rows: usize,
    pub cols: usize,
    pub data: Vec<f32>,
}

impl VoiceMatrix {
    /// Style row for a text of `len` characters (`selectStyleRow`).
    pub fn style_for(&self, len: usize) -> Vec<f32> {
        let row = len.min(self.rows.saturating_sub(1));
        self.data[row * self.cols..(row + 1) * self.cols].to_vec()
    }
}

/// A `.npy` file (version 1..3, little-endian float32, C order, 2-D).
pub fn parse_npy(bytes: &[u8]) -> Result<VoiceMatrix, String> {
    if bytes.len() < 10 || &bytes[1..6] != b"NUMPY" {
        return Err("invalid npy header".into());
    }
    let major = bytes[6];
    let (header_len, offset) = if major == 1 {
        (usize::from(u16::from_le_bytes([bytes[8], bytes[9]])), 10)
    } else {
        if bytes.len() < 12 {
            return Err("truncated npy header".into());
        }
        (
            u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize,
            12,
        )
    };
    let header = std::str::from_utf8(
        bytes
            .get(offset..offset + header_len)
            .ok_or("truncated npy header")?,
    )
    .map_err(|_| "npy header is not text")?;
    if !header.contains("'descr': '<f4'") {
        return Err(format!("unsupported npy dtype: {header}"));
    }
    if !header.contains("'fortran_order': False") {
        return Err("fortran-order npy arrays are not supported".into());
    }
    let shape = header
        .split("'shape':")
        .nth(1)
        .and_then(|s| s.split('(').nth(1))
        .and_then(|s| s.split(')').next())
        .ok_or("missing npy shape")?;
    let dims: Vec<usize> = shape
        .split(',')
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .filter_map(|d| d.parse().ok())
        .collect();
    let (rows, cols) = match dims.as_slice() {
        [r, c] => (*r, *c),
        [c] => (1, *c),
        _ => return Err(format!("expected a 2-D voice matrix, got {shape}")),
    };
    let data_bytes = &bytes[offset + header_len..];
    if data_bytes.len() < rows * cols * 4 || cols == 0 || rows == 0 {
        return Err("npy data is shorter than its shape".into());
    }
    let data = data_bytes[..rows * cols * 4]
        .chunks_exact(4)
        .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .collect();
    Ok(VoiceMatrix { rows, cols, data })
}

/// Every `<name>.npy` of a `voices.npz`.
pub fn parse_npz(bytes: &[u8]) -> Result<HashMap<String, VoiceMatrix>, String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut out = HashMap::new();
    for i in 0..archive.len() {
        let mut f = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = f.name().to_owned();
        let Some(stem) = name.strip_suffix(".npy") else {
            continue;
        };
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        out.insert(stem.to_owned(), parse_npy(&buf)?);
    }
    if out.is_empty() {
        return Err("voices.npz holds no voices".into());
    }
    Ok(out)
}

fn gender_of(voice: &str) -> &'static str {
    match voice {
        "Bella" | "Luna" | "Rosie" | "Kiki" => "female",
        _ => "male",
    }
}

pub fn voices() -> Vec<TtsVoiceVm> {
    KITTEN_VOICES
        .iter()
        .map(|v| TtsVoiceVm {
            id: (*v).to_owned(),
            name: (*v).to_owned(),
            lang: Some("en-US".into()),
            gender: Some(gender_of(v).into()),
        })
        .collect()
}

struct Engine {
    model: String,
    session: Session,
    voices: HashMap<String, VoiceMatrix>,
    aliases: HashMap<String, String>,
    priors: HashMap<String, f32>,
    sample_rate: u32,
}

pub struct Kitten {
    dir: PathBuf,
    selected: Mutex<String>,
    env: Arc<Mutex<Option<Environment>>>,
    engine: Arc<Mutex<Option<Engine>>>,
    last_error: Arc<Mutex<Option<String>>>,
    installing: AtomicBool,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn part_path(dest: &Path) -> PathBuf {
    let mut p = dest.as_os_str().to_owned();
    p.push(".part");
    PathBuf::from(p)
}

impl Kitten {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            selected: Mutex::new(KITTEN_DEFAULT_MODEL.to_owned()),
            env: Arc::new(Mutex::new(None)),
            engine: Arc::new(Mutex::new(None)),
            last_error: Arc::new(Mutex::new(None)),
            installing: AtomicBool::new(false),
        }
    }

    pub fn select_model(&self, id: &str) {
        if kitten_model_info(id).is_some() {
            *lock(&self.selected) = id.to_owned();
        }
    }

    pub fn selected_model(&self) -> String {
        lock(&self.selected).clone()
    }

    fn runtime_dir(&self) -> PathBuf {
        self.dir.join("runtime")
    }

    fn runtime_lib(&self) -> PathBuf {
        self.runtime_dir().join(runtime_lib_name())
    }

    fn model_dir(&self, id: &str) -> PathBuf {
        self.dir.join("models").join(id)
    }

    /// `(config, model file, voices file)` when the model is fully on disk.
    fn model_files(&self, id: &str) -> Option<(Value, PathBuf, PathBuf)> {
        let dir = self.model_dir(id);
        let config: Value = serde_json::from_slice(&std::fs::read(dir.join("config.json")).ok()?).ok()?;
        let (model, voices) = config_files(&config).ok()?;
        let (m, v) = (dir.join(model), dir.join(voices));
        (m.is_file() && v.is_file()).then_some((config, m, v))
    }

    pub async fn status(&self, model: &str) -> KittenStatusVm {
        let runtime = self.runtime_lib().is_file().then(|| {
            let v = std::fs::read_to_string(self.runtime_dir().join("VERSION"))
                .unwrap_or_else(|_| ORT_VERSION.to_owned());
            format!("ONNX Runtime {}", v.trim())
        });
        let phonemizer = which("espeak-ng").map(|p| p.display().to_string());
        let model_present = self.model_files(model).is_some();
        let error = if self.installing.load(Ordering::Relaxed) {
            None
        } else {
            lock(&self.last_error).clone()
        };
        KittenStatusVm {
            available: runtime.is_some() && phonemizer.is_some() && model_present && error.is_none(),
            runtime,
            phonemizer,
            model_present,
            model_dir: self.dir.display().to_string(),
            error,
        }
    }

    pub async fn install(&self, client: &reqwest::Client, model: &str) -> UiResult<()> {
        if self.installing.swap(true, Ordering::SeqCst) {
            return Err(UiError::new(
                ErrorKind::Other,
                "KittenTTS is already being installed",
            ));
        }
        let res = self.install_inner(client, model).await;
        set_kitten_progress(None);
        self.installing.store(false, Ordering::SeqCst);
        match &res {
            Ok(()) => {
                *lock(&self.last_error) = None;
                // A different model may have been loaded before: load the new one on the next request.
                *lock(&self.engine) = None;
            }
            Err(e) => *lock(&self.last_error) = Some(e.detail.clone()),
        }
        res
    }

    async fn install_inner(&self, client: &reqwest::Client, model: &str) -> UiResult<()> {
        let (repo, _) =
            kitten_model_info(model).ok_or_else(|| err(format!("unknown KittenTTS model {model}")))?;
        if !self.runtime_lib().is_file() {
            let asset = runtime_asset().ok_or_else(|| err("ONNX Runtime has no build for this system"))?;
            let rt = self.runtime_dir();
            tokio::fs::create_dir_all(&rt).await.map_err(err)?;
            let archive = rt.join(&asset);
            download(client, &runtime_url(&asset), &archive, "runtime").await?;
            set_kitten_progress(Some(KittenProgressVm {
                stage: "extract".into(),
                done: 0,
                total: None,
            }));
            let lib = self.runtime_lib();
            let res = tokio::task::spawn_blocking(move || extract_runtime(&archive, &lib))
                .await
                .map_err(err)?;
            res.map_err(err)?;
            tokio::fs::write(rt.join("VERSION"), ORT_VERSION)
                .await
                .map_err(err)?;
        }
        if self.model_files(model).is_some() {
            return Ok(());
        }
        let dir = self.model_dir(model);
        tokio::fs::create_dir_all(&dir).await.map_err(err)?;
        let config_path = dir.join("config.json");
        download(client, &hf_url(repo, "config.json"), &config_path, "config").await?;
        let config: Value = serde_json::from_slice(&tokio::fs::read(&config_path).await.map_err(err)?)
            .map_err(|e| err(format!("config.json: {e}")))?;
        let (model_file, voices_file) = config_files(&config).map_err(err)?;
        download(
            client,
            &hf_url(repo, &model_file),
            &dir.join(&model_file),
            "model",
        )
        .await?;
        download(
            client,
            &hf_url(repo, &voices_file),
            &dir.join(&voices_file),
            "voices",
        )
        .await?;
        Ok(())
    }

    pub async fn remove(&self) -> UiResult<()> {
        *lock(&self.engine) = None;
        *lock(&self.last_error) = None;
        let models = self.dir.join("models");
        if models.exists() {
            tokio::fs::remove_dir_all(&models).await.map_err(err)?;
        }
        // The library stays mapped while the process runs; on Windows its file cannot be deleted until restart.
        let rt = self.runtime_dir();
        if rt.exists()
            && let Err(e) = tokio::fs::remove_dir_all(&rt).await
        {
            tracing::warn!(error = %e, "could not remove the ONNX Runtime folder (still loaded?)");
        }
        Ok(())
    }

    /// Synthesize `text` into a WAV at `target`.
    pub async fn speak(
        &self,
        model: &str,
        text_in: &str,
        voice: Option<&str>,
        rate: u8,
        target: &Path,
    ) -> UiResult<()> {
        let espeak = which("espeak-ng").ok_or_else(|| {
            UiError::new(
                ErrorKind::Other,
                "espeak-ng is not installed (KittenTTS needs it for phonemes)",
            )
        })?;
        let Some((config, model_path, voices_path)) = self.model_files(model) else {
            return Err(UiError::new(
                ErrorKind::Other,
                "the KittenTTS model is not installed",
            ));
        };
        let lib = self.runtime_lib();
        if !lib.is_file() {
            return Err(UiError::new(ErrorKind::Other, "ONNX Runtime is not installed"));
        }
        let (env, engine, last_error) = (self.env.clone(), self.engine.clone(), self.last_error.clone());
        let model_id = model.to_owned();
        let text_s = text_in.to_owned();
        let voice = voice
            .filter(|v| !v.is_empty())
            .unwrap_or(DEFAULT_VOICE)
            .to_owned();
        let speed = f32::from(rate.clamp(50, 200)) / 100.0;
        let res = tokio::task::spawn_blocking(move || -> Result<(Vec<f32>, u32), String> {
            ensure_engine(&env, &engine, &lib, &model_id, &config, &model_path, &voices_path)?;
            let mut g = lock(&engine);
            let e = g.as_mut().ok_or("KittenTTS engine is not loaded")?;
            let samples = synth(e, &espeak, &text_s, &voice, speed)?;
            Ok((samples, e.sample_rate))
        })
        .await
        .map_err(err)?;
        let (samples, sample_rate) = match res {
            Ok(v) => v,
            Err(e) => {
                *lock(&last_error) = Some(e.clone());
                return Err(err(e));
            }
        };
        if samples.is_empty() {
            return Err(err("KittenTTS produced no audio for this text"));
        }
        *lock(&self.last_error) = None;
        let wav = wav_bytes(&samples, sample_rate);
        let tmp = part_path(target);
        tokio::fs::write(&tmp, &wav).await.map_err(err)?;
        tokio::fs::rename(&tmp, target).await.map_err(err)
    }
}

/// Download `url` to `dest` (through `<dest>.part`), reporting progress as `stage`.
async fn download(client: &reqwest::Client, url: &str, dest: &Path, stage: &str) -> UiResult<()> {
    let mut resp = client
        .get(url)
        .timeout(DOWNLOAD_TIMEOUT)
        .send()
        .await
        .map_err(net_err)?;
    if !resp.status().is_success() {
        return Err(err(format!("HTTP {} downloading {url}", resp.status().as_u16())));
    }
    let total = resp.content_length();
    let part = part_path(dest);
    let mut file = tokio::fs::File::create(&part).await.map_err(err)?;
    let mut done: u64 = 0;
    let mut reported: u64 = 0;
    set_kitten_progress(Some(KittenProgressVm {
        stage: stage.to_owned(),
        done: 0,
        total,
    }));
    while let Some(chunk) = resp.chunk().await.map_err(net_err)? {
        file.write_all(&chunk).await.map_err(err)?;
        done += chunk.len() as u64;
        if done - reported >= 256 * 1024 {
            reported = done;
            set_kitten_progress(Some(KittenProgressVm {
                stage: stage.to_owned(),
                done,
                total,
            }));
        }
    }
    file.flush().await.map_err(err)?;
    drop(file);
    if let Some(t) = total
        && done < t
    {
        let _ = tokio::fs::remove_file(&part).await;
        return Err(err(format!(
            "download of {url} ended early ({done} of {t} bytes)"
        )));
    }
    tokio::fs::rename(&part, dest).await.map_err(err)
}

/// Every regular file under `dir`.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for entry in rd.flatten() {
            let p = entry.path();
            match std::fs::symlink_metadata(&p) {
                Ok(m) if m.is_dir() => stack.push(p),
                Ok(m) if m.is_file() => out.push(p),
                _ => {}
            }
        }
    }
    out
}

/// Whether a file of the runtime archive is the main ONNX Runtime library (not the providers shim).
pub fn is_runtime_lib(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    if n.contains("providers") {
        return false;
    }
    if cfg!(windows) {
        n == "onnxruntime.dll"
    } else {
        n.starts_with("libonnxruntime") && (n.contains(".so") || n.ends_with(".dylib"))
    }
}

/// Copy the runtime library out of the release archive to `lib`, then delete the archive.
fn extract_runtime(archive: &Path, lib: &Path) -> Result<(), String> {
    let res = if archive.extension().is_some_and(|e| e == "zip") {
        extract_zip(archive, lib)
    } else {
        extract_tgz(archive, lib)
    };
    let _ = std::fs::remove_file(archive);
    res
}

fn extract_zip(archive: &Path, lib: &Path) -> Result<(), String> {
    let file = std::fs::File::open(archive).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().rsplit('/').next().unwrap_or_default().to_owned();
        if entry.is_file() && is_runtime_lib(&name) {
            let tmp = part_path(lib);
            let mut out = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
            out.flush().map_err(|e| e.to_string())?;
            drop(out);
            return std::fs::rename(&tmp, lib).map_err(|e| e.to_string());
        }
    }
    Err("the ONNX Runtime archive holds no runtime library".into())
}

fn extract_tgz(archive: &Path, lib: &Path) -> Result<(), String> {
    let tar = which("tar").ok_or("the system 'tar' tool is needed to unpack ONNX Runtime")?;
    let work = lib.with_file_name("unpack");
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let status = super::quiet_command(&tar)
        .arg("-xzf")
        .arg(archive)
        .arg("-C")
        .arg(&work)
        .status()
        .map_err(|e| e.to_string())?;
    let res = if status.success() {
        let best = files_under(&work)
            .into_iter()
            .filter(|p| {
                p.file_name()
                    .is_some_and(|n| is_runtime_lib(&n.to_string_lossy()))
            })
            .max_by_key(|p| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0));
        match best {
            Some(src) => std::fs::copy(&src, lib).map(|_| ()).map_err(|e| e.to_string()),
            None => Err("the ONNX Runtime archive holds no runtime library".into()),
        }
    } else {
        Err(format!("tar failed ({status})"))
    };
    let _ = std::fs::remove_dir_all(&work);
    res
}

/// Load the runtime (once per process) and the model (again when `model_id` changed).
fn ensure_engine(
    env: &Mutex<Option<Environment>>,
    engine: &Mutex<Option<Engine>>,
    lib: &Path,
    model_id: &str,
    config: &Value,
    model_path: &Path,
    voices_path: &Path,
) -> Result<(), String> {
    if lock(engine).as_ref().is_some_and(|e| e.model == model_id) {
        return Ok(());
    }
    let environment = {
        let mut g = lock(env);
        match g.as_ref() {
            Some(e) => e.clone(),
            None => {
                let e = ort::init_from(lib)
                    .map_err(|e| format!("ONNX Runtime could not be loaded: {e}"))?
                    .with_name("tatakai-kitten")
                    .build()
                    .map_err(|e| format!("ONNX Runtime environment: {e}"))?;
                *g = Some(e.clone());
                e
            }
        }
    };
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
        .clamp(1, 4);
    let builder = Session::builder(&environment).map_err(|e| e.to_string())?;
    let mut builder = builder.with_intra_threads(threads).map_err(|e| e.to_string())?;
    let session = builder
        .commit_from_file(model_path)
        .map_err(|e| format!("KittenTTS model: {e}"))?;
    let voices = parse_npz(&std::fs::read(voices_path).map_err(|e| e.to_string())?)?;
    let aliases = config["voice_aliases"]
        .as_object()
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned())))
                .collect()
        })
        .unwrap_or_default();
    let priors = config["speed_priors"]
        .as_object()
        .map(|o| {
            o.iter()
                .filter_map(|(k, v)| v.as_f64().map(|s| (k.clone(), s as f32)))
                .collect()
        })
        .unwrap_or_default();
    let sample_rate = config["sample_rate"]
        .as_u64()
        .and_then(|r| u32::try_from(r).ok())
        .filter(|r| *r > 0)
        .unwrap_or(DEFAULT_SAMPLE_RATE);
    *lock(engine) = Some(Engine {
        model: model_id.to_owned(),
        session,
        voices,
        aliases,
        priors,
        sample_rate,
    });
    Ok(())
}

/// Voice matrix name of a display voice: the config alias (`Luna` -> `expr-voice-...`) or the name itself.
fn resolve_voice<'a>(
    aliases: &'a HashMap<String, String>,
    voices: &HashMap<String, VoiceMatrix>,
    voice: &'a str,
) -> Option<String> {
    let name = aliases.get(voice).map(String::as_str).unwrap_or(voice);
    if voices.contains_key(name) {
        return Some(name.to_owned());
    }
    // Unknown voice: the default one, else any.
    let fallback = aliases
        .get(DEFAULT_VOICE)
        .map(String::as_str)
        .unwrap_or(DEFAULT_VOICE);
    if voices.contains_key(fallback) {
        return Some(fallback.to_owned());
    }
    let mut names: Vec<&String> = voices.keys().collect();
    names.sort();
    names.first().map(|s| (*s).clone())
}

/// IPA of one chunk with its punctuation kept (`phonemizeWithPreservedPunctuation`).
fn phonemize(espeak: &Path, chunk: &str) -> Result<String, String> {
    let mut out = String::new();
    for piece in text::split_phrases(chunk) {
        match piece {
            Piece::Literal(l) => out.push_str(&l),
            Piece::Phrase(p) => {
                let mut child = super::quiet_command(espeak)
                    .args(["-q", "--ipa", "-v", "en-us", "--stdin"])
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .map_err(|e| format!("espeak-ng: {e}"))?;
                if let Some(mut stdin) = child.stdin.take() {
                    stdin.write_all(p.as_bytes()).map_err(|e| e.to_string())?;
                }
                let o = child.wait_with_output().map_err(|e| e.to_string())?;
                if !o.status.success() {
                    return Err(format!("espeak-ng failed ({})", o.status));
                }
                out.push_str(&text::clean_ipa(&String::from_utf8_lossy(&o.stdout)));
            }
        }
    }
    Ok(out)
}

fn synth(e: &mut Engine, espeak: &Path, text_in: &str, voice: &str, speed: f32) -> Result<Vec<f32>, String> {
    let clean = text::preprocess(text_in);
    let chunks = text::chunk_text(&clean, text::MAX_CHUNK);
    let name = resolve_voice(&e.aliases, &e.voices, voice).ok_or("the KittenTTS model has no voices")?;
    let effective = speed * e.priors.get(&name).copied().unwrap_or(1.0);
    let mut audio: Vec<f32> = Vec::new();
    for chunk in chunks {
        let phonemes = phonemize(espeak, &chunk)?;
        let ids = text::input_ids(&phonemes);
        if ids.len() <= 3 {
            continue;
        }
        let matrix = e.voices.get(&name).ok_or("missing voice matrix")?;
        let style = matrix.style_for(chunk.chars().count());
        let cols = style.len();
        let n = ids.len();
        let ids_t = Tensor::from_array(([1usize, n], ids)).map_err(|e| e.to_string())?;
        let style_t = Tensor::from_array(([1usize, cols], style)).map_err(|e| e.to_string())?;
        let speed_t = Tensor::from_array(([1usize], vec![effective])).map_err(|e| e.to_string())?;
        let outputs = e
            .session
            .run(ort::inputs!["input_ids" => ids_t, "style" => style_t, "speed" => speed_t])
            .map_err(|e| format!("KittenTTS inference: {e}"))?;
        let (_, data) = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| e.to_string())?;
        let keep = data.len().saturating_sub(TRIM_SAMPLES);
        audio.extend_from_slice(&data[..keep]);
    }
    Ok(audio)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn npy(rows: usize, cols: usize) -> Vec<u8> {
        let mut header = format!("{{'descr': '<f4', 'fortran_order': False, 'shape': ({rows}, {cols}), }}");
        while (10 + header.len() + 1) % 64 != 0 {
            header.push(' ');
        }
        header.push('\n');
        let mut out = b"\x93NUMPY\x01\x00".to_vec();
        out.extend_from_slice(&(header.len() as u16).to_le_bytes());
        out.extend_from_slice(header.as_bytes());
        for i in 0..rows * cols {
            out.extend_from_slice(&(i as f32).to_le_bytes());
        }
        out
    }

    #[test]
    fn reads_voice_matrices() {
        let m = parse_npy(&npy(3, 2)).unwrap();
        assert_eq!((m.rows, m.cols), (3, 2));
        assert_eq!(m.style_for(0), vec![0.0, 1.0]);
        assert_eq!(m.style_for(99), vec![4.0, 5.0]);
        assert!(parse_npy(b"nope").is_err());
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut w = zip::ZipWriter::new(&mut buf);
            w.start_file("expr-voice-2-f.npy", zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(&npy(2, 4)).unwrap();
            w.finish().unwrap();
        }
        let voices = parse_npz(buf.get_ref()).unwrap();
        assert_eq!(voices["expr-voice-2-f"].cols, 4);
        let aliases: HashMap<String, String> = [("Bella".to_owned(), "expr-voice-2-f".to_owned())].into();
        assert_eq!(
            resolve_voice(&aliases, &voices, "Bella").as_deref(),
            Some("expr-voice-2-f")
        );
        assert_eq!(
            resolve_voice(&aliases, &voices, "Nobody").as_deref(),
            Some("expr-voice-2-f")
        );
    }

    #[test]
    fn config_and_assets() {
        assert_eq!(
            config_files(
                &json!({ "type": "ONNX1", "model_file": "kitten_tts_nano_v0_8.onnx", "voices": "voices.npz" })
            )
            .unwrap(),
            ("kitten_tts_nano_v0_8.onnx".to_owned(), "voices.npz".to_owned())
        );
        assert!(config_files(&json!({ "model_file": "../x.onnx" })).is_err());
        assert!(config_files(&json!({ "type": "KITTEN2", "model_file": "a.onnx" })).is_err());
        assert_eq!(
            hf_url("KittenML/kitten-tts-mini-0.8", "config.json"),
            "https://huggingface.co/KittenML/kitten-tts-mini-0.8/resolve/main/config.json"
        );
        if let Some(a) = runtime_asset() {
            assert!(a.contains(ORT_VERSION));
            assert!(
                runtime_url(&a)
                    .starts_with("https://github.com/microsoft/onnxruntime/releases/download/v1.22.0/")
            );
        }
        if cfg!(target_os = "linux") {
            assert!(is_runtime_lib("libonnxruntime.so.1.22.0"));
            assert!(!is_runtime_lib("libonnxruntime_providers_shared.so"));
        }
        assert_eq!(voices().len(), 8);
        assert_eq!(voices()[0].gender.as_deref(), Some("female"));
    }
}
