//! FFmpeg argument vectors (no shell, ever). One generation = one FFmpeg process producing fMP4 HLS segments
//! `seg_<k>...` that it PUTs to the session's loopback sink, starting at segment `start_segment` — or, for LSFG
//! profiles, a chain of three processes (FFmpeg -> `temple-lsfg` -> FFmpeg, [`build_lsfg`]) whose last one PUTs.
//!
//! Chains per profile follow the validated shape of Enigmata-Temple `crates/temple-core/src/media/args.rs`
//! (AGPL-3.0, same author):
//!
//! * **Windows** ([`Backend::D3d11`]): decode with `d3d11va` (software fallback is FFmpeg's own), Anime4K as
//!   `libplacebo` + `custom_shader_path` (relative `.hook` in the working directory), `frc_amf` only on AMD with its
//!   frame-retention compensation; encoders AMF / NVENC / QSV / libopenh264.
//! * **Linux** ([`Backend::Vaapi`], ported from Temple's `GpuBackend::Vaapi`): `-init_hw_device
//!   vaapi=va:/dev/dri/renderD*`, decode with `-hwaccel vaapi -hwaccel_device va` **without**
//!   `-hwaccel_output_format` (frames arrive in system memory; 10-bit or codecs the GPU cannot decode fall back to
//!   software without changing the graph), the same libplacebo / Anime4K filters, then `format=nv12,hwupload` and
//!   `h264_vaapi` (`-rc_mode VBR`, `-filter_hw_device va`). Without a usable render node ([`Backend::Software`])
//!   nothing hardware is requested and libx264 / libopenh264 encode.
//!
//! `-protocol_whitelist http,tcp,crypto` is placed before the input AND before the output: `crypto` is mandatory for
//! `#EXT-X-KEY` AES-128 (review E6); the input is only ever the loopback fetcher.

use std::ffi::OsString;
use std::path::Path;

use super::anime4k;
use super::probe::FrameRate;
use super::profiles::{Encoder, Fg, ProfileSpec, SEGMENT_SECS, Upscale};
use super::vaapi::DEVICE_NAME as VAAPI_DEVICE;

pub const PROTOCOL_WHITELIST: &str = "http,tcp,crypto";
const D3D11_DEVICE: &str = "dx";
const AMF_DEVICE: &str = "amf0";
/// `frc_amf` with `use_future_frame=0` holds back 1 input frame; 2 output frames are prepended and dropped by `trim`.
const FRC_LEAD_FRAMES: u32 = 2;
/// Upper bound of the source frame rate, only used to size `-g` (keyframes are forced every segment anyway).
const MAX_FPS: u32 = 60;
/// Macroblocks per second (MaxMBPS) of H.264 level 4.2: 1080p at 64 fps. Above it a level is declared (LSFG).
const LEVEL_4_2_MBPS: u64 = 522_240;

/// Hardware path of the graph (see the module docs). Chosen per platform by `Environment::backend`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Backend {
    /// Windows: `-hwaccel d3d11va` (+ D3D11/AMF devices for `h264_amf`).
    D3d11,
    /// Linux: VAAPI on the render node `device` (`/dev/dri/renderD128`). `decode` = FFmpeg lists the `vaapi`
    /// hwaccel; the device is also initialised for `h264_vaapi`.
    Vaapi { device: String, decode: bool },
    /// No hardware decode; only software (or NVENC) encoders.
    Software,
}

#[derive(Debug, Clone)]
pub struct PipelineParams<'a> {
    pub profile: &'a ProfileSpec,
    pub encoder: Encoder,
    pub backend: &'a Backend,
    /// `http://127.0.0.1:<port>/u/<token>/...` from the SSRF fetcher (never an external URL).
    pub input_url: &'a str,
    /// `http://127.0.0.1:<port>/sink/<token>/<generation>`: where FFmpeg PUTs init/segments.
    pub sink_base: &'a str,
    pub start_segment: u32,
    pub audio_track: u32,
    /// Relative `.hook` name for Anime4K profiles.
    pub shader_file: Option<&'a str>,
    /// D3D11 adapter index used for decode/AMF.
    pub adapter: u32,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ArgsError {
    #[error("profile needs a shader file")]
    MissingShader,
    #[error("profile needs encoder h264_amf")]
    NeedsAmf,
    #[error("encoder does not run on this backend")]
    EncoderBackend,
    #[error("not an LSFG profile")]
    NotLsfg,
    #[error("LSFG profiles run as three processes (build_lsfg)")]
    IsLsfg,
    #[error("the LSFG library path must be absolute")]
    DllNotAbsolute,
    #[error("the LSFG output rate overflows")]
    RateOverflow,
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| (*x).to_owned()).collect()
}

fn vaapi_device_arg(device: &str) -> String {
    format!("vaapi={VAAPI_DEVICE}:{device}")
}

/// The encoder must match the backend: AMF only over D3D11, VAAPI only over a VAAPI node.
fn check_encoder(p: &PipelineParams<'_>) -> Result<(), ArgsError> {
    match (p.encoder, p.backend) {
        (Encoder::H264Amf, Backend::D3d11) | (Encoder::H264Vaapi, Backend::Vaapi { .. }) => Ok(()),
        (Encoder::H264Amf | Encoder::H264Vaapi, _) => Err(ArgsError::EncoderBackend),
        _ => Ok(()),
    }
}

/// Decode options (hwaccel) of the input, after the device init.
fn push_decode(a: &mut Vec<String>, backend: &Backend) {
    match backend {
        Backend::D3d11 => a.extend(s(&["-hwaccel", "d3d11va"])),
        Backend::Vaapi { decode: true, .. } => a.extend(s(&["-hwaccel", "vaapi", "-hwaccel_device", VAAPI_DEVICE])),
        Backend::Vaapi { decode: false, .. } | Backend::Software => {}
    }
}

/// `-init_hw_device` set of the single-process graph and of the LSFG encode process (`decoding` = this process also
/// decodes the source).
fn push_devices(a: &mut Vec<String>, p: &PipelineParams<'_>, decoding: bool, frc: bool) {
    match p.backend {
        Backend::D3d11 => {
            if p.encoder == Encoder::H264Amf {
                a.extend(s(&["-init_hw_device", &format!("d3d11va={D3D11_DEVICE}:{}", p.adapter), "-init_hw_device", &format!("amf={AMF_DEVICE}@{D3D11_DEVICE}")]));
                if frc {
                    a.extend(s(&["-filter_hw_device", AMF_DEVICE]));
                }
            }
        }
        Backend::Vaapi { device, decode } => {
            let encode = p.encoder == Encoder::H264Vaapi;
            if encode || (decoding && *decode) {
                a.extend(s(&["-init_hw_device", &vaapi_device_arg(device)]));
            }
            if encode {
                // `hwupload` before `h264_vaapi` needs the filter device
                a.extend(s(&["-filter_hw_device", VAAPI_DEVICE]));
            }
        }
        Backend::Software => {}
    }
}

fn video_filter(p: &PipelineParams<'_>) -> Result<String, ArgsError> {
    let (w, h) = (p.profile.width, p.profile.height);
    let mut f = match p.profile.upscale {
        Upscale::None => format!("scale=w='min(iw,{w})':h='min(ih,{h})':force_original_aspect_ratio=decrease:force_divisible_by=2,format=nv12"),
        Upscale::Anime4k { .. } => {
            let shader = p.shader_file.ok_or(ArgsError::MissingShader)?;
            let mut f = format!("libplacebo=w={w}:h={h}:force_original_aspect_ratio=decrease:force_divisible_by=2:format=nv12:custom_shader_path={shader}");
            if p.profile.fg == Fg::FrcAmf {
                if p.encoder != Encoder::H264Amf {
                    return Err(ArgsError::NeedsAmf);
                }
                let lead = FRC_LEAD_FRAMES;
                f.push_str(&format!(
                    ",tpad=stop=1:stop_mode=clone,hwupload,frc_amf=profile=high:use_future_frame=0:fallback_mode=dup,trim=start_frame={lead},settb=AVTB,setpts=STARTPTS+(N-{lead})/(FRAME_RATE*TB)"
                ));
            }
            f
        }
    };
    if p.encoder == Encoder::H264Vaapi {
        f.push_str(",hwupload");
    }
    Ok(f)
}

/// `-level` for an output of `width`x`height` at `fps`: none up to 4.2 (1080p60 as always), then the lowest level
/// whose MaxMBPS fits (5.1 up to 1080p120, 5.2 up to 1080p254, 6.x above). Ported from Temple `h264_level`.
pub fn h264_level(width: u32, height: u32, fps: f64) -> Option<&'static str> {
    let macroblocks = u64::from(width.div_ceil(16)) * u64::from(height.div_ceil(16));
    let per_second = (macroblocks as f64 * fps).ceil() as u64;
    if per_second <= LEVEL_4_2_MBPS {
        None
    } else if per_second <= 983_040 {
        Some("5.1")
    } else if per_second <= 2_073_600 {
        Some("5.2")
    } else if per_second <= 4_177_920 {
        Some("6.0")
    } else if per_second <= 8_355_840 {
        Some("6.1")
    } else {
        Some("6.2")
    }
}

fn encoder_args(encoder: Encoder, video_kbps: u32, gop: u32, level: Option<&str>) -> Vec<String> {
    let kbps = format!("{video_kbps}k");
    let gop = gop.to_string();
    let keys = format!("expr:gte(t,n_forced*{SEGMENT_SECS})");
    let lvl: Vec<String> = match (encoder, level) {
        (Encoder::LibOpenH264, _) | (_, None) => Vec::new(),
        // `h264_vaapi` takes the numeric level_idc (`5.1` -> `51`)
        (Encoder::H264Vaapi, Some(l)) => s(&["-level", &l.replace('.', "")]),
        (_, Some(l)) => s(&["-level", l]),
    };
    let mut a = match encoder {
        Encoder::H264Amf => s(&["-c:v", "h264_amf", "-usage", "transcoding", "-quality", "balanced", "-rc", "vbr_peak", "-b:v", &kbps, "-maxrate", &kbps, "-bf", "0"]),
        Encoder::H264Nvenc => s(&["-c:v", "h264_nvenc", "-preset", "p4", "-rc", "vbr", "-b:v", &kbps, "-maxrate", &kbps, "-bf", "0"]),
        Encoder::H264Qsv => s(&["-c:v", "h264_qsv", "-preset", "medium", "-b:v", &kbps, "-maxrate", &kbps, "-bf", "0"]),
        Encoder::H264Vaapi => s(&["-c:v", "h264_vaapi", "-rc_mode", "VBR", "-b:v", &kbps, "-maxrate", &kbps, "-bf", "0"]),
        Encoder::LibX264 => {
            let buf = format!("{}k", video_kbps * 2);
            s(&["-c:v", "libx264", "-preset", "veryfast", "-b:v", &kbps, "-maxrate", &kbps, "-bufsize", &buf, "-pix_fmt", "yuv420p", "-bf", "0"])
        }
        Encoder::LibOpenH264 => return s(&["-c:v", "libopenh264", "-b:v", &kbps, "-pix_fmt", "yuv420p", "-force_key_frames", &keys]),
    };
    a.extend(lvl);
    a.extend(s(&["-g", &gop]));
    match encoder {
        Encoder::H264Amf => a.extend(s(&["-forced_idr", "1"])),
        Encoder::H264Nvenc => a.extend(s(&["-forced-idr", "1"])),
        _ => {}
    }
    a.extend(s(&["-force_key_frames", &keys]));
    a
}

/// Audio encode, `-output_ts_offset` and the HLS PUT output (shared by both builders).
fn push_tail(a: &mut Vec<String>, p: &PipelineParams<'_>) {
    a.extend(s(&["-c:a", "aac", "-b:a", "192k", "-ar", "48000", "-ac", "2"]));
    if p.start_segment > 0 {
        a.extend(s(&["-output_ts_offset", &(p.start_segment * SEGMENT_SECS).to_string()]));
    }
    a.extend(s(&[
        "-f", "hls", "-hls_time", &SEGMENT_SECS.to_string(), "-hls_segment_type", "fmp4", "-hls_flags", "independent_segments", "-hls_list_size", "6",
        "-start_number", &p.start_segment.to_string(), "-hls_fmp4_init_filename", "init.mp4",
        "-hls_segment_options", "movflags=+frag_discont:use_editlist=0:avoid_negative_ts=disabled",
        "-hls_segment_filename", &format!("{}/seg_%d.m4s", p.sink_base), "-method", "PUT",
        "-protocol_whitelist", PROTOCOL_WHITELIST, &format!("{}/live.m3u8", p.sink_base),
    ]));
}

/// `-ss k*S` (input option) for a generation starting at k > 0: `-ss 0` damages the first audio packet.
fn push_seek(a: &mut Vec<String>, p: &PipelineParams<'_>) {
    if p.start_segment > 0 {
        a.extend(s(&["-ss", &(p.start_segment * SEGMENT_SECS).to_string()]));
    }
}

const HEAD: [&str; 5] = ["-hide_banner", "-nostdin", "-loglevel", "error", "-y"];

/// Complete argument vector (without the program name) of a single-process profile.
pub fn build(p: &PipelineParams<'_>) -> Result<Vec<OsString>, ArgsError> {
    if p.profile.fg.lsfg().is_some() {
        return Err(ArgsError::IsLsfg);
    }
    check_encoder(p)?;
    let mut a = s(&HEAD);
    push_devices(&mut a, p, true, p.profile.fg == Fg::FrcAmf);
    push_decode(&mut a, p.backend);
    push_seek(&mut a, p);
    a.extend(s(&["-protocol_whitelist", PROTOCOL_WHITELIST, "-i", p.input_url]));
    a.extend(s(&["-map", "0:v:0", "-map", &format!("0:a:{}?", p.audio_track)]));
    a.extend(s(&["-vf", &video_filter(p)?]));
    if p.encoder == Encoder::H264Amf {
        // the graph fixes the cadence; keep source pts (and their relation to the audio)
        a.extend(s(&["-fps_mode", "passthrough"]));
    }
    a.extend(encoder_args(p.encoder, p.profile.video_kbps, MAX_FPS * SEGMENT_SECS, None));
    push_tail(&mut a, p);
    Ok(a.into_iter().map(OsString::from).collect())
}

/// The three processes of an LSFG generation (ported from Temple `LsfgArgs` / `lsfg_parts`): FFmpeg A decodes,
/// scales (Anime4K included) and writes raw RGBA to `pipe:1`; `temple-lsfg` inserts `multiplier - 1` frames between
/// each pair; FFmpeg B reads raw RGBA from `pipe:0` at `source rate x multiplier`, the audio of the original (same
/// `-ss`) and encodes to the same HLS PUT output as [`build`]. The pipes are connected by the engine.
#[derive(Clone, PartialEq, Eq)]
pub struct LsfgArgs {
    pub decode: Vec<OsString>,
    pub helper: Vec<OsString>,
    pub encode: Vec<OsString>,
    pub multiplier: u8,
    pub output_rate: FrameRate,
}

impl std::fmt::Debug for LsfgArgs {
    /// No URLs (they carry tokens) and no library path.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LsfgArgs").field("multiplier", &self.multiplier).field("output_rate", &self.output_rate).finish_non_exhaustive()
    }
}

/// Raw frames must be exactly `W`x`H`: without upscale the picture is fitted and centred with bars (Temple
/// `fit_filter`, `contain` without a scaling step); with Anime4K it is first padded to the output aspect ratio and
/// libplacebo scales the whole frame (Temple `contain` with a scaling step).
fn lsfg_scale(p: &PipelineParams<'_>) -> Result<String, ArgsError> {
    let (w, h) = (p.profile.width, p.profile.height);
    match p.profile.upscale {
        Upscale::None => Ok(format!("scale=w={w}:h={h}:force_original_aspect_ratio=decrease:force_divisible_by=2,pad={w}:{h}:(ow-iw)/2:(oh-ih)/2")),
        Upscale::Anime4k { .. } => {
            let shader = p.shader_file.ok_or(ArgsError::MissingShader)?;
            Ok(format!(
                "pad=w='2*ceil(max(iw,ih*{w}/{h})/2)':h='2*ceil(max(ih,iw*{h}/{w})/2)':x='(ow-iw)/2':y='(oh-ih)/2',\
                 libplacebo=w={w}:h={h}:format=nv12:custom_shader_path={shader}"
            ))
        }
    }
}

/// Arguments of the three LSFG processes. `source_rate` is the probed rate of the source (or the fallback): the
/// decode process forces it with `fps=<rate>:start_time=0`, so the encode process can trust `-framerate`. `dll` is
/// the absolute path of `Lossless.dll`; it only goes to the helper.
pub fn build_lsfg(p: &PipelineParams<'_>, source_rate: FrameRate, dll: &Path) -> Result<LsfgArgs, ArgsError> {
    let mode = p.profile.fg.lsfg().ok_or(ArgsError::NotLsfg)?;
    if !dll.is_absolute() {
        return Err(ArgsError::DllNotAbsolute);
    }
    check_encoder(p)?;
    let multiplier = mode.multiplier(Some(source_rate.fps()));
    let out_rate = source_rate.times(u32::from(multiplier)).ok_or(ArgsError::RateOverflow)?;
    let (w, h) = (p.profile.width, p.profile.height);

    // FFmpeg A: decode + scale -> raw RGBA
    let mut d = s(&HEAD);
    match p.backend {
        Backend::D3d11 => d.extend(s(&["-hwaccel", "d3d11va"])),
        Backend::Vaapi { device, decode: true } => {
            d.extend(s(&["-init_hw_device", &vaapi_device_arg(device)]));
            push_decode(&mut d, p.backend);
        }
        Backend::Vaapi { decode: false, .. } | Backend::Software => {}
    }
    push_seek(&mut d, p);
    d.extend(s(&["-protocol_whitelist", PROTOCOL_WHITELIST, "-i", p.input_url, "-map", "0:v:0"]));
    let chain = format!("fps={}:start_time=0,{},format=rgba", source_rate.ffmpeg(), lsfg_scale(p)?);
    d.extend(s(&["-vf", &chain, "-fps_mode", "passthrough", "-f", "rawvideo", "-pix_fmt", "rgba", "pipe:1"]));

    // temple-lsfg
    let mut helper: Vec<OsString> = vec!["--dll".into(), dll.as_os_str().to_owned()];
    helper.extend(["--width", &w.to_string(), "--height", &h.to_string(), "--multiplier", &multiplier.to_string()].map(OsString::from));

    // FFmpeg B: raw RGBA + original audio -> encoder -> HLS PUT
    let mut e = s(&HEAD);
    push_devices(&mut e, p, false, false);
    e.extend(s(&["-f", "rawvideo", "-pix_fmt", "rgba", "-s", &format!("{w}x{h}"), "-framerate", &out_rate.ffmpeg(), "-i", "pipe:0"]));
    push_seek(&mut e, p);
    e.extend(s(&["-protocol_whitelist", PROTOCOL_WHITELIST, "-i", p.input_url]));
    e.extend(s(&["-map", "0:v:0", "-map", &format!("1:a:{}?", p.audio_track)]));
    let vf = if p.encoder == Encoder::H264Vaapi { "format=nv12,hwupload" } else { "format=nv12" };
    e.extend(s(&["-vf", vf, "-fps_mode", "passthrough"]));
    let gop = (out_rate.fps().ceil() as u32).max(1) * SEGMENT_SECS;
    e.extend(encoder_args(p.encoder, p.profile.video_kbps, gop, h264_level(w, h, out_rate.fps())));
    push_tail(&mut e, p);

    let os = |v: Vec<String>| v.into_iter().map(OsString::from).collect();
    Ok(LsfgArgs { decode: os(d), helper, encode: os(e), multiplier, output_rate: out_rate })
}

/// Anime4K `.hook` for a profile, written into `workdir`; `None` for profiles without upscale.
pub fn prepare_shader(profile: &ProfileSpec, shaders: Option<&std::path::Path>, workdir: &std::path::Path) -> std::io::Result<Option<String>> {
    match (profile.upscale, shaders) {
        (Upscale::None, _) => Ok(None),
        (Upscale::Anime4k { mode, variant }, Some(dir)) => anime4k::write_shader(dir, workdir, mode, variant).map(Some),
        (Upscale::Anime4k { .. }, None) => Err(std::io::Error::new(std::io::ErrorKind::NotFound, "Anime4K shaders are not installed")),
    }
}

#[cfg(test)]
mod tests {
    use super::super::profiles::{CATALOG, find};
    use super::*;

    const IN: &str = "http://127.0.0.1:50001/u/tok/p/0/AAAA";
    const SINK: &str = "http://127.0.0.1:50002/sink/tok/1";

    fn args(id: &str, enc: Encoder, start: u32, audio: u32) -> Vec<String> {
        let profile = find(id).unwrap();
        let shader = match profile.upscale {
            Upscale::Anime4k { mode, variant } => Some(anime4k::shader_file_name(mode, variant)),
            Upscale::None => None,
        };
        let p = PipelineParams { profile, encoder: enc, backend: &Backend::D3d11, input_url: IN, sink_base: SINK, start_segment: start, audio_track: audio, shader_file: shader.as_deref(), adapter: 0 };
        build(&p).unwrap().into_iter().map(|o| o.into_string().unwrap()).collect()
    }

    fn tail() -> Vec<&'static str> {
        vec![
            "-c:a", "aac", "-b:a", "192k", "-ar", "48000", "-ac", "2", "-f", "hls", "-hls_time", "2", "-hls_segment_type", "fmp4", "-hls_flags",
            "independent_segments", "-hls_list_size", "6", "-start_number", "0", "-hls_fmp4_init_filename", "init.mp4", "-hls_segment_options",
            "movflags=+frag_discont:use_editlist=0:avoid_negative_ts=disabled", "-hls_segment_filename", "http://127.0.0.1:50002/sink/tok/1/seg_%d.m4s",
            "-method", "PUT", "-protocol_whitelist", "http,tcp,crypto", "http://127.0.0.1:50002/sink/tok/1/live.m3u8",
        ]
    }

    #[test]
    fn golden_p_none_libopenh264() {
        let mut want: Vec<&str> = vec![
            "-hide_banner", "-nostdin", "-loglevel", "error", "-y", "-hwaccel", "d3d11va", "-protocol_whitelist", "http,tcp,crypto", "-i", IN, "-map", "0:v:0",
            "-map", "0:a:0?", "-vf", "scale=w='min(iw,1920)':h='min(ih,1080)':force_original_aspect_ratio=decrease:force_divisible_by=2,format=nv12",
            "-c:v", "libopenh264", "-b:v", "6000k", "-pix_fmt", "yuv420p", "-force_key_frames", "expr:gte(t,n_forced*2)",
        ];
        want.extend(tail());
        assert_eq!(args("p-none", Encoder::LibOpenH264, 0, 0), want);
    }

    #[test]
    fn golden_p_none_nvenc_and_qsv() {
        let nv = args("p-none", Encoder::H264Nvenc, 0, 0);
        let idx = nv.iter().position(|a| a == "-c:v").unwrap();
        assert_eq!(
            nv[idx..idx + 17],
            ["-c:v", "h264_nvenc", "-preset", "p4", "-rc", "vbr", "-b:v", "6000k", "-maxrate", "6000k", "-bf", "0", "-g", "120", "-forced-idr", "1", "-force_key_frames"]
        );
        let qsv = args("p-none", Encoder::H264Qsv, 0, 0);
        let idx = qsv.iter().position(|a| a == "-c:v").unwrap();
        assert_eq!(qsv[idx..idx + 12], ["-c:v", "h264_qsv", "-preset", "medium", "-b:v", "6000k", "-maxrate", "6000k", "-bf", "0", "-g", "120"]);
        assert!(!nv.contains(&"-init_hw_device".to_owned()), "AMF device only for AMF");
    }

    #[test]
    fn golden_anime4k_1080_amf() {
        let mut want: Vec<&str> = vec![
            "-hide_banner", "-nostdin", "-loglevel", "error", "-y", "-init_hw_device", "d3d11va=dx:0", "-init_hw_device", "amf=amf0@dx", "-hwaccel", "d3d11va",
            "-protocol_whitelist", "http,tcp,crypto", "-i", IN, "-map", "0:v:0", "-map", "0:a:0?", "-vf",
            "libplacebo=w=1920:h=1080:force_original_aspect_ratio=decrease:force_divisible_by=2:format=nv12:custom_shader_path=anime4k-v4.0-a-m.hook",
            "-fps_mode", "passthrough", "-c:v", "h264_amf", "-usage", "transcoding", "-quality", "balanced", "-rc", "vbr_peak", "-b:v", "8000k", "-maxrate", "8000k",
            "-bf", "0", "-g", "120", "-forced_idr", "1", "-force_key_frames", "expr:gte(t,n_forced*2)",
        ];
        want.extend(tail());
        assert_eq!(args("p-anime4k-1080", Encoder::H264Amf, 0, 0), want);
    }

    #[test]
    fn golden_anime4k_1440_and_2160_differ_only_in_size_variant_and_bitrate() {
        let a = args("p-anime4k-1440", Encoder::H264Nvenc, 0, 0);
        assert!(a.contains(&"libplacebo=w=2560:h=1440:force_original_aspect_ratio=decrease:force_divisible_by=2:format=nv12:custom_shader_path=anime4k-v4.0-a-l.hook".to_owned()));
        assert!(a.contains(&"14000k".to_owned()));
        let b = args("p-anime4k-2160", Encoder::H264Nvenc, 0, 0);
        assert!(b.contains(&"libplacebo=w=3840:h=2160:force_original_aspect_ratio=decrease:force_divisible_by=2:format=nv12:custom_shader_path=anime4k-v4.0-a-l.hook".to_owned()));
        assert!(b.contains(&"28000k".to_owned()));
    }

    #[test]
    fn golden_fg2x_amf_filter_chain_and_hw_device() {
        let a = args("p-anime4k-1080-fg2x", Encoder::H264Amf, 0, 0);
        let vf = &a[a.iter().position(|x| x == "-vf").unwrap() + 1];
        assert_eq!(
            vf,
            "libplacebo=w=1920:h=1080:force_original_aspect_ratio=decrease:force_divisible_by=2:format=nv12:custom_shader_path=anime4k-v4.0-a-m.hook,\
             tpad=stop=1:stop_mode=clone,hwupload,frc_amf=profile=high:use_future_frame=0:fallback_mode=dup,trim=start_frame=2,settb=AVTB,\
             setpts=STARTPTS+(N-2)/(FRAME_RATE*TB)"
        );
        let i = a.iter().position(|x| x == "-filter_hw_device").unwrap();
        assert_eq!(a[i + 1], "amf0");
        assert!(a.contains(&"10000k".to_owned()));
        // frame generation is AMD-only
        let profile = find("p-anime4k-1080-fg2x").unwrap();
        let p = PipelineParams { profile, encoder: Encoder::H264Nvenc, backend: &Backend::D3d11, input_url: IN, sink_base: SINK, start_segment: 0, audio_track: 0, shader_file: Some("x.hook"), adapter: 0 };
        assert_eq!(build(&p).unwrap_err(), ArgsError::NeedsAmf);
    }

    #[test]
    fn seek_generation_uses_ss_start_number_and_ts_offset_only_when_k_is_positive() {
        let a = args("p-none", Encoder::LibOpenH264, 30, 1);
        let i = a.iter().position(|x| x == "-ss").unwrap();
        assert_eq!(a[i + 1], "60");
        assert!(i < a.iter().position(|x| x == "-i").unwrap(), "-ss is an input option");
        let o = a.iter().position(|x| x == "-output_ts_offset").unwrap();
        assert_eq!(a[o + 1], "60");
        let n = a.iter().position(|x| x == "-start_number").unwrap();
        assert_eq!(a[n + 1], "30");
        assert!(a.contains(&"0:a:1?".to_owned()));
        let zero = args("p-none", Encoder::LibOpenH264, 0, 0);
        assert!(!zero.contains(&"-ss".to_owned()) && !zero.contains(&"-output_ts_offset".to_owned()));
    }

    #[test]
    fn invariants_for_every_profile_and_encoder() {
        let vaapi = Backend::Vaapi { device: "/dev/dri/renderD128".into(), decode: true };
        let backends = [Backend::D3d11, vaapi, Backend::Software];
        let encoders = [Encoder::H264Amf, Encoder::H264Nvenc, Encoder::H264Qsv, Encoder::H264Vaapi, Encoder::LibX264, Encoder::LibOpenH264];
        for profile in CATALOG.iter().filter(|p| p.fg.lsfg().is_none()) {
            for (backend, enc) in backends.iter().flat_map(|b| encoders.iter().map(move |e| (b, *e))) {
                let shader = match profile.upscale {
                    Upscale::Anime4k { mode, variant } => Some(anime4k::shader_file_name(mode, variant)),
                    Upscale::None => None,
                };
                let p = PipelineParams { profile, encoder: enc, backend, input_url: IN, sink_base: SINK, start_segment: 3, audio_track: 0, shader_file: shader.as_deref(), adapter: 0 };
                let Ok(v) = build(&p) else { continue };
                let a: Vec<String> = v.into_iter().map(|o| o.into_string().unwrap()).collect();
                // the only input is the loopback fetcher; FFmpeg never sees an external URL
                let inputs: Vec<_> = a.iter().enumerate().filter(|(_, x)| *x == "-i").map(|(i, _)| &a[i + 1]).collect();
                assert_eq!(inputs, [IN]);
                // every URL-like argument is loopback
                for x in a.iter().filter(|x| x.contains("://")) {
                    assert!(x.starts_with("http://127.0.0.1:"), "{x}");
                }
                // whitelist before the input and before the output
                let wl: Vec<_> = a.iter().enumerate().filter(|(_, x)| *x == "-protocol_whitelist").map(|(i, _)| a[i + 1].as_str()).collect();
                assert_eq!(wl, [PROTOCOL_WHITELIST, PROTOCOL_WHITELIST], "{} {:?} {:?}", profile.id, enc, backend);
                assert!(wl.iter().all(|w| w.split(',').any(|p| p == "crypto")));
                assert!(!a.iter().any(|x| x.contains("file:") || x.contains('\\') || x.contains(" ")), "no paths or shell-isms");
                if a.contains(&"h264_vaapi".to_owned()) {
                    assert!(a.windows(2).any(|w| w[0] == "-filter_hw_device" && w[1] == "va"));
                    assert!(a[a.iter().position(|x| x == "-vf").unwrap() + 1].ends_with(",hwupload"));
                }
            }
        }
    }

    // ---- Linux (VAAPI / software) and LSFG -------------------------------------------------------------------------

    fn va() -> Backend {
        Backend::Vaapi { device: "/dev/dri/renderD128".into(), decode: true }
    }

    fn with(id: &str, enc: Encoder, backend: &Backend, start: u32, audio: u32) -> Vec<String> {
        let profile = find(id).unwrap();
        let shader = match profile.upscale {
            Upscale::Anime4k { mode, variant } => Some(anime4k::shader_file_name(mode, variant)),
            Upscale::None => None,
        };
        let p = PipelineParams { profile, encoder: enc, backend, input_url: IN, sink_base: SINK, start_segment: start, audio_track: audio, shader_file: shader.as_deref(), adapter: 0 };
        build(&p).unwrap().into_iter().map(|o| o.into_string().unwrap()).collect()
    }

    fn strs(v: &[OsString]) -> Vec<String> {
        v.iter().map(|o| o.to_string_lossy().into_owned()).collect()
    }

    fn lsfg(id: &str, enc: Encoder, backend: &Backend, rate: FrameRate, start: u32, audio: u32) -> Result<LsfgArgs, ArgsError> {
        let profile = find(id).unwrap();
        let shader = match profile.upscale {
            Upscale::Anime4k { mode, variant } => Some(anime4k::shader_file_name(mode, variant)),
            Upscale::None => None,
        };
        let p = PipelineParams { profile, encoder: enc, backend, input_url: IN, sink_base: SINK, start_segment: start, audio_track: audio, shader_file: shader.as_deref(), adapter: 0 };
        build_lsfg(&p, rate, dll())
    }

    fn dll() -> &'static Path {
        if cfg!(windows) { Path::new(r"C:\Steam\steamapps\common\Lossless Scaling\Lossless.dll") } else { Path::new("/home/u/.local/share/Steam/steamapps/common/Lossless Scaling/Lossless.dll") }
    }

    const R24: FrameRate = FrameRate { num: 24_000, den: 1001 };

    #[test]
    fn golden_p_none_vaapi() {
        let mut want: Vec<&str> = vec![
            "-hide_banner", "-nostdin", "-loglevel", "error", "-y", "-init_hw_device", "vaapi=va:/dev/dri/renderD128", "-filter_hw_device", "va", "-hwaccel",
            "vaapi", "-hwaccel_device", "va", "-protocol_whitelist", "http,tcp,crypto", "-i", IN, "-map", "0:v:0", "-map", "0:a:0?", "-vf",
            "scale=w='min(iw,1920)':h='min(ih,1080)':force_original_aspect_ratio=decrease:force_divisible_by=2,format=nv12,hwupload", "-c:v", "h264_vaapi",
            "-rc_mode", "VBR", "-b:v", "6000k", "-maxrate", "6000k", "-bf", "0", "-g", "120", "-force_key_frames", "expr:gte(t,n_forced*2)",
        ];
        want.extend(tail());
        assert_eq!(with("p-none", Encoder::H264Vaapi, &va(), 0, 0), want);
    }

    #[test]
    fn golden_anime4k_1080_vaapi_uses_libplacebo_then_hwupload() {
        let a = with("p-anime4k-1080", Encoder::H264Vaapi, &va(), 0, 0);
        let vf = &a[a.iter().position(|x| x == "-vf").unwrap() + 1];
        assert_eq!(vf, "libplacebo=w=1920:h=1080:force_original_aspect_ratio=decrease:force_divisible_by=2:format=nv12:custom_shader_path=anime4k-v4.0-a-m.hook,hwupload");
        assert!(!a.contains(&"d3d11va".to_owned()) && !a.contains(&"-fps_mode".to_owned()));
        assert!(a.contains(&"8000k".to_owned()));
    }

    #[test]
    fn linux_software_fallbacks_ask_for_no_hardware() {
        let x = with("p-none", Encoder::LibX264, &Backend::Software, 0, 0);
        assert!(!x.iter().any(|a| a.starts_with("-hwaccel") || a == "-init_hw_device"), "{x:?}");
        let i = x.iter().position(|a| a == "-c:v").unwrap();
        assert_eq!(
            x[i..i + 20],
            ["-c:v", "libx264", "-preset", "veryfast", "-b:v", "6000k", "-maxrate", "6000k", "-bufsize", "12000k", "-pix_fmt", "yuv420p", "-bf", "0", "-g", "120", "-force_key_frames", "expr:gte(t,n_forced*2)", "-c:a", "aac"]
        );
        // VAAPI decode with a software encoder: device + hwaccel, no filter device, no hwupload
        let o = with("p-none", Encoder::LibOpenH264, &va(), 0, 0);
        assert!(o.windows(2).any(|w| w[0] == "-init_hw_device" && w[1] == "vaapi=va:/dev/dri/renderD128"));
        assert!(o.windows(2).any(|w| w[0] == "-hwaccel" && w[1] == "vaapi"));
        assert!(!o.contains(&"-filter_hw_device".to_owned()));
        assert!(!o.iter().any(|a| a.contains("hwupload")));
        // a render node FFmpeg cannot decode on: only the encoder device
        let enc_only = Backend::Vaapi { device: "/dev/dri/renderD129".into(), decode: false };
        let v = with("p-none", Encoder::H264Vaapi, &enc_only, 0, 0);
        assert!(v.contains(&"vaapi=va:/dev/dri/renderD129".to_owned()) && !v.contains(&"-hwaccel".to_owned()));
        let n = with("p-none", Encoder::LibOpenH264, &enc_only, 0, 0);
        assert!(!n.contains(&"-init_hw_device".to_owned()));
    }

    #[test]
    fn encoders_must_match_their_backend() {
        let profile = find("p-none").unwrap();
        let vaapi = va();
        let mk = |encoder, backend| PipelineParams { profile, encoder, backend, input_url: IN, sink_base: SINK, start_segment: 0, audio_track: 0, shader_file: None, adapter: 0 };
        assert_eq!(build(&mk(Encoder::H264Vaapi, &Backend::D3d11)).unwrap_err(), ArgsError::EncoderBackend);
        assert_eq!(build(&mk(Encoder::H264Amf, &vaapi)).unwrap_err(), ArgsError::EncoderBackend);
        assert_eq!(build(&mk(Encoder::H264Vaapi, &Backend::Software)).unwrap_err(), ArgsError::EncoderBackend);
        let l = PipelineParams { profile: find("p-lsfg-1080-x2").unwrap(), ..mk(Encoder::LibX264, &Backend::Software) };
        assert_eq!(build(&l).unwrap_err(), ArgsError::IsLsfg);
        assert_eq!(build_lsfg(&mk(Encoder::LibX264, &Backend::Software), R24, dll()).unwrap_err(), ArgsError::NotLsfg);
        assert_eq!(build_lsfg(&l, R24, Path::new("Lossless.dll")).unwrap_err(), ArgsError::DllNotAbsolute);
        // frc_amf stays AMD/Windows only
        let fg = PipelineParams { profile: find("p-anime4k-1080-fg2x").unwrap(), shader_file: Some("x.hook"), ..mk(Encoder::H264Vaapi, &vaapi) };
        assert_eq!(build(&fg).unwrap_err(), ArgsError::NeedsAmf);
    }

    #[test]
    fn golden_lsfg_x2_vaapi_three_processes() {
        let a = lsfg("p-lsfg-1080-x2", Encoder::H264Vaapi, &va(), R24, 0, 0).unwrap();
        assert_eq!((a.multiplier, a.output_rate), (2, FrameRate { num: 48_000, den: 1001 }));
        assert_eq!(
            strs(&a.decode),
            [
                "-hide_banner", "-nostdin", "-loglevel", "error", "-y", "-init_hw_device", "vaapi=va:/dev/dri/renderD128", "-hwaccel", "vaapi", "-hwaccel_device",
                "va", "-protocol_whitelist", "http,tcp,crypto", "-i", IN, "-map", "0:v:0", "-vf",
                "fps=24000/1001:start_time=0,scale=w=1920:h=1080:force_original_aspect_ratio=decrease:force_divisible_by=2,pad=1920:1080:(ow-iw)/2:(oh-ih)/2,format=rgba",
                "-fps_mode", "passthrough", "-f", "rawvideo", "-pix_fmt", "rgba", "pipe:1",
            ]
        );
        let mut helper = vec!["--dll".to_owned(), dll().to_string_lossy().into_owned()];
        helper.extend(["--width", "1920", "--height", "1080", "--multiplier", "2"].map(String::from));
        assert_eq!(strs(&a.helper), helper);
        let mut want: Vec<&str> = vec![
            "-hide_banner", "-nostdin", "-loglevel", "error", "-y", "-init_hw_device", "vaapi=va:/dev/dri/renderD128", "-filter_hw_device", "va", "-f",
            "rawvideo", "-pix_fmt", "rgba", "-s", "1920x1080", "-framerate", "48000/1001", "-i", "pipe:0", "-protocol_whitelist", "http,tcp,crypto", "-i", IN,
            "-map", "0:v:0", "-map", "1:a:0?", "-vf", "format=nv12,hwupload", "-fps_mode", "passthrough", "-c:v", "h264_vaapi", "-rc_mode", "VBR", "-b:v",
            "8000k", "-maxrate", "8000k", "-bf", "0", "-g", "96", "-force_key_frames", "expr:gte(t,n_forced*2)",
        ];
        want.extend(tail());
        assert_eq!(strs(&a.encode), want);
        assert!(!format!("{a:?}").contains("127.0.0.1") && !format!("{a:?}").contains("Lossless"));
    }

    #[test]
    fn lsfg_anime4k_auto120_picks_the_multiplier_and_level_from_the_rate() {
        let thirty = FrameRate { num: 30, den: 1 };
        let a = lsfg("p-anime4k-lsfg-1080-auto120", Encoder::H264Vaapi, &va(), thirty, 0, 0).unwrap();
        assert_eq!((a.multiplier, a.output_rate.ffmpeg()), (4, "120/1".to_owned()));
        let d = strs(&a.decode);
        let vf = &d[d.iter().position(|x| x == "-vf").unwrap() + 1];
        assert_eq!(
            vf,
            "fps=30/1:start_time=0,pad=w='2*ceil(max(iw,ih*1920/1080)/2)':h='2*ceil(max(ih,iw*1080/1920)/2)':x='(ow-iw)/2':y='(oh-ih)/2',\
             libplacebo=w=1920:h=1080:format=nv12:custom_shader_path=anime4k-v4.0-a-m.hook,format=rgba"
        );
        let e = strs(&a.encode);
        assert!(e.windows(2).any(|w| w[0] == "-level" && w[1] == "51"), "1080p120 needs level 5.1 ({e:?})");
        assert!(e.windows(2).any(|w| w[0] == "-g" && w[1] == "240"));
        assert!(e.contains(&"11000k".to_owned()));
        // the same profile at 23.976 fps goes x5
        assert_eq!(lsfg("p-anime4k-lsfg-1080-auto120", Encoder::H264Vaapi, &va(), R24, 0, 0).unwrap().multiplier, 5);
        // fixed x5 at 60 fps: 300 fps output, level 6.0, `-level 6.0` for AMF on Windows
        let sixty = FrameRate { num: 60, den: 1 };
        let w = lsfg("p-lsfg-1080-x5", Encoder::H264Amf, &Backend::D3d11, sixty, 0, 0).unwrap();
        let e = strs(&w.encode);
        assert!(e.windows(2).any(|w| w[0] == "-level" && w[1] == "6.0"));
        assert_eq!(e[5..9], ["-init_hw_device", "d3d11va=dx:0", "-init_hw_device", "amf=amf0@dx"]);
        assert!(e.windows(2).any(|w| w[0] == "-forced_idr" && w[1] == "1"));
        assert_eq!(strs(&w.decode)[5..7], ["-hwaccel", "d3d11va"]);
    }

    #[test]
    fn lsfg_seek_audio_track_and_loopback_invariants() {
        let encoders = [Encoder::H264Vaapi, Encoder::LibX264, Encoder::LibOpenH264, Encoder::H264Amf, Encoder::H264Nvenc, Encoder::H264Qsv];
        let backends = [va(), Backend::Software, Backend::D3d11];
        for profile in CATALOG.iter().filter(|p| p.fg.lsfg().is_some()) {
            for backend in &backends {
                for enc in encoders {
                    let Ok(a) = lsfg(profile.id, enc, backend, R24, 30, 2) else { continue };
                    let (d, e) = (strs(&a.decode), strs(&a.encode));
                    for v in [&d, &e] {
                        let ss = v.iter().position(|x| x == "-ss").unwrap();
                        assert_eq!(v[ss + 1], "60");
                        // -ss belongs to the loopback input, not to the raw pipe
                        assert_eq!(v[v[ss..].iter().position(|x| x == "-i").unwrap() + ss + 1], IN);
                        for x in v.iter().filter(|x| x.contains("://")) {
                            assert!(x.starts_with("http://127.0.0.1:"), "{x}");
                        }
                    }
                    let inputs: Vec<_> = e.iter().enumerate().filter(|(_, x)| *x == "-i").map(|(i, _)| e[i + 1].as_str()).collect();
                    assert_eq!(inputs, ["pipe:0", IN], "{}", profile.id);
                    let wl = e.iter().filter(|x| *x == "-protocol_whitelist").count();
                    assert_eq!(wl, 2, "before the loopback input and before the output");
                    assert!(e.contains(&"1:a:2?".to_owned()));
                    assert!(e.windows(2).any(|w| w[0] == "-output_ts_offset" && w[1] == "60"));
                    assert!(e.windows(2).any(|w| w[0] == "-start_number" && w[1] == "30"));
                    assert_eq!(d.last().unwrap(), "pipe:1");
                    assert!(!d.iter().any(|x| x.contains(":a:")), "the decode process maps no audio");
                }
            }
        }
    }

    #[test]
    fn h264_levels() {
        assert_eq!(h264_level(1920, 1080, 60.0), None);
        assert_eq!(h264_level(1920, 1080, 120.0), Some("5.1"));
        assert_eq!(h264_level(1920, 1080, 150.0), Some("5.2"));
        assert_eq!(h264_level(1920, 1080, 300.0), Some("6.0"));
    }
}
