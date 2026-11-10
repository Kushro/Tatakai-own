//! `GET /v1/capabilities`: encoders detected with `ffmpeg -encoders` (filtered and ordered per platform), GPU vendor,
//! profile catalogue with `measured_sustain` (stored bench results), LSFG availability and the fixed limits.
//!
//! Platform-aware detection: Windows keeps D3D11 + AMF / NVENC / QSV / libopenh264; Linux uses the VAAPI render node
//! ([`super::vaapi`], `-hwaccel vaapi` when `ffmpeg -hwaccels` lists it, `h264_vaapi` when listed and the node is
//! not NVIDIA), then NVENC, libx264, libopenh264.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde::{Serialize, Serializer};

use super::args::Backend;
use super::lsfg::{LsfgCapability, LsfgStatus};
use super::profiles::{CATALOG, DEFAULT_PROFILE, Encoder, Fg, LsfgMode, Platform, ProfileSpec, Upscale};
use super::vaapi::RenderNode;

pub const MAX_RAM_BYTES: u64 = 512 * 1024 * 1024;
pub const SEGMENT_WAIT_SECS: u64 = 15;

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
    /// Encoders of this platform's preference list that FFmpeg has, in that order.
    pub encoders: Vec<Encoder>,
    pub libplacebo: bool,
    pub frc_amf: bool,
    /// `ffmpeg -hwaccels` lists `vaapi`.
    pub hwaccel_vaapi: bool,
}

/// Parse `ffmpeg -encoders` for this platform: lines like ` V....D h264_amf   AMD AMF H.264 Encoder (codec h264)`.
pub fn parse_encoders(out: &str) -> Vec<Encoder> {
    parse_encoders_for(Platform::current(), out)
}

/// Linux builds of FFmpeg usually list `h264_amf` / `h264_qsv` too; only the platform's own list is kept.
pub fn parse_encoders_for(platform: Platform, out: &str) -> Vec<Encoder> {
    let names: Vec<&str> = out.lines().filter_map(|l| l.split_whitespace().nth(1)).collect();
    Encoder::preference(platform).into_iter().filter(|e| names.contains(&e.as_str())).collect()
}

/// Parse `ffmpeg -hwaccels` (`Hardware acceleration methods:` then one name per line).
pub fn parse_has_hwaccel(out: &str, name: &str) -> bool {
    out.lines().skip_while(|l| !l.contains("acceleration methods")).skip(1).any(|l| l.trim() == name)
}

/// Parse `ffmpeg -filters`: lines like ` ... libplacebo   V->V  Apply various GPU filters from libplacebo`.
pub fn parse_has_filter(out: &str, name: &str) -> bool {
    out.lines().any(|l| l.split_whitespace().nth(1) == Some(name))
}

pub async fn detect(ffmpeg: &Path, runner: &dyn CommandRunner) -> Detected {
    detect_for(Platform::current(), ffmpeg, runner).await
}

pub async fn detect_for(platform: Platform, ffmpeg: &Path, runner: &dyn CommandRunner) -> Detected {
    let enc = runner.run(ffmpeg, &["-hide_banner", "-encoders"]).await.unwrap_or_default();
    let fil = runner.run(ffmpeg, &["-hide_banner", "-filters"]).await.unwrap_or_default();
    let hw = if platform == Platform::Linux { runner.run(ffmpeg, &["-hide_banner", "-hwaccels"]).await.unwrap_or_default() } else { String::new() };
    Detected {
        encoders: parse_encoders_for(platform, &enc),
        libplacebo: parse_has_filter(&fil, "libplacebo"),
        frc_amf: parse_has_filter(&fil, "frc_amf"),
        hwaccel_vaapi: parse_has_hwaccel(&hw, "vaapi"),
    }
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

/// [`guess_gpu`], except that on Linux with `h264_vaapi` the render node's PCI vendor is the GPU's.
pub fn guess_gpu_with(d: &Detected, node: Option<&RenderNode>) -> GpuInfo {
    let mut g = guess_gpu(d);
    if let Some(n) = node.filter(|n| n.usable() && n.vendor != Vendor::Unknown && d.encoders.contains(&Encoder::H264Vaapi)) {
        g.vendor = n.vendor;
    }
    g
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
    /// LSFG profiles with a fixed multiplier (additive).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fg_multiplier: Option<u8>,
    /// LSFG profiles whose multiplier is chosen per source to approach this rate (additive).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fg_target_fps: Option<u32>,
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
    /// LSFG availability (additive): `available`, `experimental`, `multipliers`, `auto_target_fps`, `reason`.
    pub lsfg: LsfgCapability,
}

/// Everything that decides which profiles can run.
#[derive(Debug, Clone)]
pub struct Environment {
    pub detected: Detected,
    pub gpu: GpuInfo,
    pub shaders_ok: bool,
    pub platform: Platform,
    /// Linux VAAPI render node (`None` on Windows or without `/dev/dri`).
    pub render_node: Option<RenderNode>,
    pub lsfg: LsfgStatus,
    /// The `L` (or `VL`) Anime4K networks of the 1440p / 2160p profiles are present (`shaders_ok` covers only `M`).
    pub anime4k_large_ok: bool,
}

impl Environment {
    /// Environment of this platform without a render node and with LSFG disabled.
    pub fn new(detected: Detected, gpu: GpuInfo, shaders_ok: bool) -> Self {
        Self { detected, gpu, shaders_ok, platform: Platform::current(), render_node: None, lsfg: LsfgStatus::default(), anime4k_large_ok: shaders_ok }
    }

    fn vaapi_node(&self) -> Option<&RenderNode> {
        self.render_node.as_ref().filter(|n| self.platform == Platform::Linux && n.usable())
    }

    /// Encoders that can run here, in preference order: `h264_vaapi` needs a usable render node.
    pub fn usable_encoders(&self) -> Vec<Encoder> {
        self.detected.encoders.iter().copied().filter(|e| *e != Encoder::H264Vaapi || self.vaapi_node().is_some()).collect()
    }

    /// Chosen encoder: first usable one of the platform's preference list.
    pub fn encoder(&self) -> Option<Encoder> {
        self.usable_encoders().first().copied()
    }

    /// Hardware path of the graphs: D3D11 on Windows; VAAPI on Linux when the node is usable and FFmpeg can decode or
    /// encode with it; software otherwise.
    pub fn backend(&self) -> Backend {
        match self.platform {
            Platform::Windows => Backend::D3d11,
            Platform::Linux => match self.vaapi_node() {
                Some(n) if self.detected.hwaccel_vaapi || self.detected.encoders.contains(&Encoder::H264Vaapi) => {
                    Backend::Vaapi { device: n.device.clone(), decode: self.detected.hwaccel_vaapi }
                }
                _ => Backend::Software,
            },
        }
    }

    fn anime4k_ok(&self) -> bool {
        self.detected.libplacebo && self.gpu.vulkan && self.shaders_ok
    }

    pub fn profile_available(&self, p: &ProfileSpec) -> bool {
        if matches!(p.upscale, Upscale::Anime4k { variant: crate::media::anime4k::Anime4kVariant::L, .. }) && !self.anime4k_large_ok {
            return false;
        }
        match (p.upscale, p.fg) {
            (Upscale::None, Fg::None) => true, // p-none is always offered (a session fails with engine_unavailable if no encoder exists)
            (Upscale::None, Fg::FrcAmf) => false,
            (upscale, Fg::Lsfg(_)) => {
                self.lsfg.is_available() && self.encoder().is_some() && (matches!(upscale, Upscale::None) || self.anime4k_ok())
            }
            (Upscale::Anime4k { .. }, Fg::None) => self.anime4k_ok() && self.encoder().is_some(),
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
            fg_multiplier: match p.fg {
                Fg::Lsfg(LsfgMode::Fixed(m)) => Some(m),
                _ => None,
            },
            fg_target_fps: match p.fg {
                Fg::Lsfg(LsfgMode::Auto { target_fps }) => Some(target_fps),
                _ => None,
            },
        })
        .collect();
    Capabilities {
        contract: "v1",
        server_version: env!("CARGO_PKG_VERSION"),
        instance_id: instance_id.to_owned(),
        server_name: server_name.to_owned(),
        gpu: env.gpu.clone(),
        encoders: env.usable_encoders().into_iter().map(Encoder::as_str).collect(),
        default_profile: DEFAULT_PROFILE,
        profiles,
        client_fg: if client_fg_allowed { "allowed" } else { "suppressed" },
        limits: Limits { max_ram_bytes: MAX_RAM_BYTES, segment_wait_secs: SEGMENT_WAIT_SECS },
        lsfg: env.lsfg.capability(),
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
        let d = detect_for(Platform::Windows, Path::new("ffmpeg"), &Canned).await;
        assert_eq!(d.encoders, [Encoder::H264Amf, Encoder::H264Nvenc, Encoder::LibOpenH264]);
        assert!(d.libplacebo && d.frc_amf);
        assert_eq!(parse_encoders("no encoders here"), []);
        assert!(!parse_has_filter(FILTERS, "minterpolate"));
        assert_eq!(guess_gpu(&d).vendor, Vendor::Amd);
        // Linux keeps its own list: no AMF / QSV, libx264 before libopenh264
        let l = detect_for(Platform::Linux, Path::new("ffmpeg"), &Canned).await;
        assert_eq!(l.encoders, [Encoder::H264Nvenc, Encoder::LibX264, Encoder::LibOpenH264]);
    }

    #[tokio::test]
    async fn profile_availability_and_json_shape() {
        let d = detect_for(Platform::Windows, Path::new("ffmpeg"), &Canned).await;
        let env = Environment { platform: Platform::Windows, ..Environment::new(d.clone(), guess_gpu(&d), true) };
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
        assert_eq!(json["limits"], serde_json::json!({ "max_ram_bytes": 536870912u64, "segment_wait_secs": 15 }));
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
        let nv = Detected { encoders: vec![Encoder::H264Nvenc], libplacebo: true, ..Detected::default() };
        let env3 = Environment { platform: Platform::Windows, ..Environment::new(nv.clone(), guess_gpu(&nv), true) };
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

    const LINUX_ENCODERS: &str = " V....D libx264              libx264\n V....D h264_amf             AMD AMF\n V....D h264_nvenc           NVENC\n V....D h264_vaapi           H.264/AVC (VAAPI)\n V....D libopenh264          OpenH264\n";
    const HWACCELS: &str = "Hardware acceleration methods:\nvdpau\ncuda\nvaapi\nqsv\ndrm\nvulkan\namf\n\n";

    fn amd_node() -> RenderNode {
        RenderNode { device: "/dev/dri/renderD128".into(), vendor: Vendor::Amd }
    }

    fn linux_env(node: Option<RenderNode>, hwaccels: &str) -> Environment {
        let d = Detected {
            encoders: parse_encoders_for(Platform::Linux, LINUX_ENCODERS),
            libplacebo: true,
            frc_amf: true,
            hwaccel_vaapi: parse_has_hwaccel(hwaccels, "vaapi"),
        };
        let gpu = guess_gpu_with(&d, node.as_ref());
        Environment { platform: Platform::Linux, render_node: node, ..Environment::new(d, gpu, true) }
    }

    #[test]
    fn linux_picks_vaapi_on_an_amd_render_node() {
        assert!(parse_has_hwaccel(HWACCELS, "vaapi") && !parse_has_hwaccel(HWACCELS, "d3d11va"));
        assert!(!parse_has_hwaccel("vaapi\n", "vaapi"), "only lines after the header count");
        let env = linux_env(Some(amd_node()), HWACCELS);
        assert_eq!(env.usable_encoders(), [Encoder::H264Vaapi, Encoder::H264Nvenc, Encoder::LibX264, Encoder::LibOpenH264]);
        assert_eq!(env.encoder(), Some(Encoder::H264Vaapi));
        assert_eq!(env.backend(), Backend::Vaapi { device: "/dev/dri/renderD128".into(), decode: true });
        assert_eq!(env.gpu.vendor, Vendor::Amd, "the node vendor wins over the NVENC guess");
        let caps = build(&env, &BenchStore::default(), "x", "PC", false);
        assert_eq!(caps.encoders[0], "h264_vaapi");
        assert!(!caps.encoders.contains(&"h264_amf"));
        // frc_amf is never offered on Linux, even if FFmpeg lists the filter
        assert!(!caps.profiles.iter().any(|p| p.id == "p-anime4k-1080-fg2x"));
        assert!(caps.profiles.iter().any(|p| p.id == "p-anime4k-1080"));

        // no render node: no VAAPI at all, software decode, libx264 is not chosen before NVENC
        let none = linux_env(None, HWACCELS);
        assert_eq!(none.encoder(), Some(Encoder::H264Nvenc));
        assert_eq!(none.backend(), Backend::Software);
        // an NVIDIA node is not used for VAAPI
        let nv = linux_env(Some(RenderNode { device: "/dev/dri/renderD128".into(), vendor: Vendor::Nvidia }), HWACCELS);
        assert_eq!(nv.encoder(), Some(Encoder::H264Nvenc));
        assert_eq!(nv.backend(), Backend::Software);
        // FFmpeg without the vaapi hwaccel still encodes with h264_vaapi
        let enc_only = linux_env(Some(amd_node()), "");
        assert_eq!(enc_only.backend(), Backend::Vaapi { device: "/dev/dri/renderD128".into(), decode: false });
    }

    #[test]
    fn lsfg_profiles_follow_the_probe_and_carry_their_multiplier() {
        use crate::media::lsfg::{LsfgPaths, ProbeInfo};
        let mut env = linux_env(Some(amd_node()), HWACCELS);
        let caps = build(&env, &BenchStore::default(), "x", "PC", false);
        assert!(!caps.profiles.iter().any(|p| p.fg == Fg::Lsfg(LsfgMode::Fixed(2))));
        let json = serde_json::to_value(&caps).unwrap();
        assert_eq!(json["lsfg"]["available"], false);
        assert_eq!(json["lsfg"]["reason"], "disabled");

        env.lsfg = LsfgStatus::available(
            LsfgPaths { helper: "/opt/t/temple-lsfg".into(), dll: "/s/Lossless.dll".into() },
            ProbeInfo { device: "AMD Radeon RX 9070 XT (RADV GFX1201)".into(), fp16: true },
        );
        let caps = build(&env, &BenchStore::default(), "x", "PC", false);
        let ids: Vec<_> = caps.profiles.iter().map(|p| p.id).collect();
        for id in ["p-lsfg-1080-x2", "p-lsfg-1080-x5", "p-lsfg-1080-auto120", "p-anime4k-lsfg-1080-x3", "p-anime4k-lsfg-1080-auto120"] {
            assert!(ids.contains(&id), "{id}");
        }
        let json = serde_json::to_value(&caps).unwrap();
        assert_eq!(json["lsfg"], serde_json::json!({ "available": true, "experimental": true, "multipliers": [2, 3, 4, 5], "auto_target_fps": 120 }));
        let by = |id: &str| json["profiles"].as_array().unwrap().iter().find(|p| p["id"] == id).unwrap().clone();
        assert_eq!(by("p-lsfg-1080-x3")["fg"], "lsfg");
        assert_eq!(by("p-lsfg-1080-x3")["fg_multiplier"], 3);
        assert!(by("p-lsfg-1080-x3").get("fg_target_fps").is_none());
        assert_eq!(by("p-anime4k-lsfg-1080-auto120")["fg_target_fps"], 120);
        assert_eq!(by("p-anime4k-lsfg-1080-auto120")["upscale"], "anime4k");
        assert_eq!(by("p-lsfg-1080-x2")["encoder"], "h264_vaapi");
        assert!(by("p-none").get("fg_multiplier").is_none(), "M0 profiles keep their shape");
        // Anime4K + LSFG needs the shaders; plain LSFG does not
        env.shaders_ok = false;
        let ids: Vec<_> = build(&env, &BenchStore::default(), "x", "PC", false).profiles.iter().map(|p| p.id).collect();
        assert!(ids.contains(&"p-lsfg-1080-x4") && !ids.contains(&"p-anime4k-lsfg-1080-x4"));
    }
}
