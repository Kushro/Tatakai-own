//! `GET /v1/capabilities`: encoders detected with `ffmpeg -encoders`, GPU vendor, profile catalogue with
//! `measured_sustain` (stored bench results) and the fixed limits.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde::{Serialize, Serializer};

use super::profiles::{CATALOG, DEFAULT_PROFILE, Encoder, Fg, ProfileSpec, Upscale};

pub const MAX_RAM_BYTES: u64 = 512 * 1024 * 1024;
pub const SEGMENT_WAIT_SECS: u64 = 20;

/// Runs an external program and returns its stdout. Injected so detection is testable without FFmpeg.
#[async_trait]
pub trait CommandRunner: Send + Sync {
    async fn run(&self, program: &Path, args: &[&str]) -> io::Result<String>;
}

pub struct SystemRunner;

#[async_trait]
impl CommandRunner for SystemRunner {
    async fn run(&self, program: &Path, args: &[&str]) -> io::Result<String> {
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(args).stdin(std::process::Stdio::null()).kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000);
        let out = tokio::time::timeout(std::time::Duration::from_secs(15), cmd.output())
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "ffmpeg did not answer"))??;
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Detected {
    pub encoders: Vec<Encoder>,
    pub libplacebo: bool,
    pub frc_amf: bool,
}

/// Parse `ffmpeg -encoders`: lines like ` V....D h264_amf   AMD AMF H.264 Encoder (codec h264)`.
pub fn parse_encoders(out: &str) -> Vec<Encoder> {
    let names: Vec<&str> = out.lines().filter_map(|l| l.split_whitespace().nth(1)).collect();
    Encoder::PREFERENCE.into_iter().filter(|e| names.contains(&e.as_str())).collect()
}

/// Parse `ffmpeg -filters`: lines like ` ... libplacebo   V->V  Apply various GPU filters from libplacebo`.
pub fn parse_has_filter(out: &str, name: &str) -> bool {
    out.lines().any(|l| l.split_whitespace().nth(1) == Some(name))
}

pub async fn detect(ffmpeg: &Path, runner: &dyn CommandRunner) -> Detected {
    let enc = runner.run(ffmpeg, &["-hide_banner", "-encoders"]).await.unwrap_or_default();
    let fil = runner.run(ffmpeg, &["-hide_banner", "-filters"]).await.unwrap_or_default();
    Detected { encoders: parse_encoders(&enc), libplacebo: parse_has_filter(&fil, "libplacebo"), frc_amf: parse_has_filter(&fil, "frc_amf") }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Vendor {
    Amd,
    Nvidia,
    Intel,
    Other,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GpuInfo {
    pub vendor: Vendor,
    pub name: String,
    pub vulkan: bool,
}

/// Vendor guessed from the hardware encoders present when the app does not know better.
pub fn guess_gpu(d: &Detected) -> GpuInfo {
    let vendor = if d.encoders.contains(&Encoder::H264Amf) {
        Vendor::Amd
    } else if d.encoders.contains(&Encoder::H264Nvenc) {
        Vendor::Nvidia
    } else if d.encoders.contains(&Encoder::H264Qsv) {
        Vendor::Intel
    } else {
        Vendor::Unknown
    };
    GpuInfo { vendor, name: String::new(), vulkan: d.libplacebo }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sustain {
    Yes,
    No,
    Unknown,
}

impl Serialize for Sustain {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Yes => s.serialize_bool(true),
            Self::No => s.serialize_bool(false),
            Self::Unknown => s.serialize_str("unknown"),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Output {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProfileInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub upscale: &'static str,
    pub fg: Fg,
    pub output: Output,
    pub measured_sustain: Sustain,
    pub encoder: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct Limits {
    pub max_ram_bytes: u64,
    pub segment_wait_secs: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Capabilities {
    pub contract: &'static str,
    pub server_version: &'static str,
    pub instance_id: String,
    pub server_name: String,
    pub gpu: GpuInfo,
    pub encoders: Vec<&'static str>,
    pub default_profile: &'static str,
    pub profiles: Vec<ProfileInfo>,
    pub client_fg: &'static str,
    pub limits: Limits,
}

/// Everything that decides which profiles can run.
#[derive(Debug, Clone)]
pub struct Environment {
    pub detected: Detected,
    pub gpu: GpuInfo,
    pub shaders_ok: bool,
}

impl Environment {
    /// Chosen encoder: first hardware encoder present, then libopenh264.
    pub fn encoder(&self) -> Option<Encoder> {
        Encoder::PREFERENCE.into_iter().find(|e| self.detected.encoders.contains(e))
    }

    pub fn profile_available(&self, p: &ProfileSpec) -> bool {
        match (p.upscale, p.fg) {
            (Upscale::None, _) => true, // p-none is always offered (a session fails with engine_unavailable if no encoder exists)
            (Upscale::Anime4k { .. }, Fg::None) => self.detected.libplacebo && self.gpu.vulkan && self.shaders_ok && self.encoder().is_some(),
            (Upscale::Anime4k { .. }, Fg::FrcAmf) => {
                self.detected.libplacebo
                    && self.gpu.vulkan
                    && self.shaders_ok
                    && self.gpu.vendor == Vendor::Amd
                    && self.detected.encoders.contains(&Encoder::H264Amf)
                    && self.detected.frc_amf
            }
        }
    }

    /// Encoder to use for `p` (frame generation needs AMF).
    pub fn encoder_for(&self, p: &ProfileSpec) -> Option<Encoder> {
        if p.fg == Fg::FrcAmf { Some(Encoder::H264Amf) } else { self.encoder() }
    }
}

/// Stored 10-second bench verdicts (`profile id -> sustained`).
#[derive(Debug, Default, Clone)]
pub struct BenchStore {
    path: Option<PathBuf>,
    results: HashMap<String, bool>,
}

impl BenchStore {
    pub fn open(path: PathBuf) -> Self {
        let results = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        Self { path: Some(path), results }
    }

    pub fn record(&mut self, profile: &str, sustained: bool) {
        self.results.insert(profile.to_owned(), sustained);
        if let Some(p) = &self.path {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(p, serde_json::to_vec_pretty(&self.results).unwrap_or_default());
        }
    }

    pub fn get(&self, profile: &str) -> Sustain {
        self.results.get(profile).map_or(Sustain::Unknown, |b| if *b { Sustain::Yes } else { Sustain::No })
    }
}

pub fn build(env: &Environment, bench: &BenchStore, instance_id: &str, server_name: &str, client_fg_allowed: bool) -> Capabilities {
    let profiles = CATALOG
        .iter()
        .filter(|p| env.profile_available(p))
        .map(|p| ProfileInfo {
            id: p.id,
            label: p.label,
            upscale: if matches!(p.upscale, Upscale::None) { "none" } else { "anime4k" },
            fg: p.fg,
            output: Output { width: p.width, height: p.height },
            measured_sustain: if p.id == DEFAULT_PROFILE { Sustain::Yes } else { bench.get(p.id) },
            encoder: env.encoder_for(p).map_or("", Encoder::as_str),
        })
        .collect();
    Capabilities {
        contract: "v1",
        server_version: env!("CARGO_PKG_VERSION"),
        instance_id: instance_id.to_owned(),
        server_name: server_name.to_owned(),
        gpu: env.gpu.clone(),
        encoders: env.detected.encoders.iter().map(|e| e.as_str()).collect(),
        default_profile: DEFAULT_PROFILE,
        profiles,
        client_fg: if client_fg_allowed { "allowed" } else { "suppressed" },
        limits: Limits { max_ram_bytes: MAX_RAM_BYTES, segment_wait_secs: SEGMENT_WAIT_SECS },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENCODERS: &str = "Encoders:\n V..... = Video\n ------\n V....D libx264              libx264 H.264 (codec h264)\n V....D h264_amf             AMD AMF H.264 Encoder (codec h264)\n V....D h264_nvenc           NVIDIA NVENC H.264 encoder (codec h264)\n V....D libopenh264          OpenH264 H.264 / AVC (codec h264)\n A....D aac                  AAC\n";
    const FILTERS: &str = "Filters:\n T.. = Timeline support\n ... libplacebo        VV->V      Apply various GPU filters from libplacebo\n ... frc_amf           V->V       AMF Frame Rate Converter\n ... scale             V->V       Scale the input video\n";

    struct Canned;

    #[async_trait]
    impl CommandRunner for Canned {
        async fn run(&self, _p: &Path, args: &[&str]) -> io::Result<String> {
            Ok(if args.contains(&"-encoders") { ENCODERS } else { FILTERS }.to_owned())
        }
    }

    #[tokio::test]
    async fn detects_encoders_and_filters_in_preference_order() {
        let d = detect(Path::new("ffmpeg"), &Canned).await;
        assert_eq!(d.encoders, [Encoder::H264Amf, Encoder::H264Nvenc, Encoder::LibOpenH264]);
        assert!(d.libplacebo && d.frc_amf);
        assert_eq!(parse_encoders("no encoders here"), []);
        assert!(!parse_has_filter(FILTERS, "minterpolate"));
        assert_eq!(guess_gpu(&d).vendor, Vendor::Amd);
    }

    #[tokio::test]
    async fn profile_availability_and_json_shape() {
        let d = detect(Path::new("ffmpeg"), &Canned).await;
        let env = Environment { gpu: guess_gpu(&d), detected: d.clone(), shaders_ok: true };
        let mut bench = BenchStore::default();
        bench.record("p-anime4k-2160", false);
        bench.record("p-anime4k-1080", true);
        let caps = build(&env, &bench, "11111111-1111-4111-8111-111111111111", "PC", false);
        let ids: Vec<_> = caps.profiles.iter().map(|p| p.id).collect();
        assert_eq!(ids, ["p-none", "p-anime4k-1080", "p-anime4k-1440", "p-anime4k-2160", "p-anime4k-1080-fg2x"]);
        let json = serde_json::to_value(&caps).unwrap();
        assert_eq!(json["contract"], "v1");
        assert_eq!(json["default_profile"], "p-none");
        assert_eq!(json["client_fg"], "suppressed");
        assert_eq!(json["limits"], serde_json::json!({ "max_ram_bytes": 536870912u64, "segment_wait_secs": 20 }));
        assert_eq!(json["gpu"]["vendor"], "amd");
        assert_eq!(json["encoders"], serde_json::json!(["h264_amf", "h264_nvenc", "libopenh264"]));
        let by = |id: &str| json["profiles"].as_array().unwrap().iter().find(|p| p["id"] == id).unwrap().clone();
        assert_eq!(by("p-anime4k-1080")["measured_sustain"], true);
        assert_eq!(by("p-anime4k-2160")["measured_sustain"], false);
        assert_eq!(by("p-anime4k-1440")["measured_sustain"], "unknown");
        assert_eq!(by("p-none")["upscale"], "none");
        assert_eq!(by("p-anime4k-1080-fg2x")["fg"], "frc_amf");
        assert_eq!(by("p-anime4k-1080-fg2x")["encoder"], "h264_amf");
        assert_eq!(by("p-anime4k-1080")["output"], serde_json::json!({ "width": 1920, "height": 1080 }));

        // no shaders: only p-none; no AMD: no frame generation
        let env2 = Environment { shaders_ok: false, ..env.clone() };
        assert_eq!(build(&env2, &bench, "x", "PC", false).profiles.len(), 1);
        let nv = Detected { encoders: vec![Encoder::H264Nvenc], libplacebo: true, frc_amf: false };
        let env3 = Environment { gpu: guess_gpu(&nv), detected: nv, shaders_ok: true };
        let ids: Vec<_> = build(&env3, &bench, "x", "PC", false).profiles.iter().map(|p| p.id).collect();
        assert_eq!(ids, ["p-none", "p-anime4k-1080", "p-anime4k-1440", "p-anime4k-2160"]);
    }

    #[test]
    fn bench_results_persist() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("link").join("bench.json");
        let mut b = BenchStore::open(path.clone());
        assert_eq!(b.get("p-anime4k-1080"), Sustain::Unknown);
        b.record("p-anime4k-1080", true);
        assert_eq!(BenchStore::open(path).get("p-anime4k-1080"), Sustain::Yes);
    }
}
