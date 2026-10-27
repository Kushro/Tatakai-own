//! FFmpeg argument vectors (no shell, ever). One generation = one FFmpeg process producing fMP4 HLS segments
//! `seg_<k>...` that it PUTs to the session's loopback sink, starting at segment `start_segment`.
//!
//! Chains per profile follow the validated shape of Enigmata-Temple `media/args.rs` (AGPL-3.0, same author):
//! decode with `d3d11va` (software fallback is FFmpeg's own), Anime4K as `libplacebo` + `custom_shader_path` (relative
//! `.hook` in the working directory), `frc_amf` only on AMD with its frame-retention compensation.
//!
//! `-protocol_whitelist http,tcp,crypto` is placed before the input AND before the output: `crypto` is mandatory for
//! `#EXT-X-KEY` AES-128 (review E6); the input is only ever the loopback fetcher.

use std::ffi::OsString;

use super::anime4k;
use super::profiles::{Encoder, Fg, ProfileSpec, SEGMENT_SECS, Upscale};

pub const PROTOCOL_WHITELIST: &str = "http,tcp,crypto";
const D3D11_DEVICE: &str = "dx";
const AMF_DEVICE: &str = "amf0";
/// `frc_amf` with `use_future_frame=0` holds back 1 input frame; 2 output frames are prepended and dropped by `trim`.
const FRC_LEAD_FRAMES: u32 = 2;
/// Upper bound of the source frame rate, only used to size `-g` (keyframes are forced every segment anyway).
const MAX_FPS: u32 = 60;

#[derive(Debug, Clone)]
pub struct PipelineParams<'a> {
    pub profile: &'a ProfileSpec,
    pub encoder: Encoder,
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
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| (*x).to_owned()).collect()
}

fn video_filter(p: &PipelineParams<'_>) -> Result<String, ArgsError> {
    let (w, h) = (p.profile.width, p.profile.height);
    match p.profile.upscale {
        Upscale::None => Ok(format!("scale=w='min(iw,{w})':h='min(ih,{h})':force_original_aspect_ratio=decrease:force_divisible_by=2,format=nv12")),
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
            Ok(f)
        }
    }
}

fn encoder_args(p: &PipelineParams<'_>) -> Vec<String> {
    let kbps = format!("{}k", p.profile.video_kbps);
    let gop = (MAX_FPS * SEGMENT_SECS).to_string();
    let keys = format!("expr:gte(t,n_forced*{SEGMENT_SECS})");
    match p.encoder {
        Encoder::H264Amf => s(&["-c:v", "h264_amf", "-usage", "transcoding", "-quality", "balanced", "-rc", "vbr_peak", "-b:v", &kbps, "-maxrate", &kbps, "-bf", "0", "-g", &gop, "-forced_idr", "1", "-force_key_frames", &keys]),
        Encoder::H264Nvenc => s(&["-c:v", "h264_nvenc", "-preset", "p4", "-rc", "vbr", "-b:v", &kbps, "-maxrate", &kbps, "-bf", "0", "-g", &gop, "-forced-idr", "1", "-force_key_frames", &keys]),
        Encoder::H264Qsv => s(&["-c:v", "h264_qsv", "-preset", "medium", "-b:v", &kbps, "-maxrate", &kbps, "-bf", "0", "-g", &gop, "-force_key_frames", &keys]),
        Encoder::LibOpenH264 => s(&["-c:v", "libopenh264", "-b:v", &kbps, "-pix_fmt", "yuv420p", "-force_key_frames", &keys]),
    }
}

/// Complete argument vector (without the program name).
pub fn build(p: &PipelineParams<'_>) -> Result<Vec<OsString>, ArgsError> {
    let mut a = s(&["-hide_banner", "-nostdin", "-loglevel", "error", "-y"]);
    let frc = p.profile.fg == Fg::FrcAmf;
    if p.encoder == Encoder::H264Amf {
        a.extend(s(&["-init_hw_device", &format!("d3d11va={D3D11_DEVICE}:{}", p.adapter), "-init_hw_device", &format!("amf={AMF_DEVICE}@{D3D11_DEVICE}")]));
        if frc {
            a.extend(s(&["-filter_hw_device", AMF_DEVICE]));
        }
    }
    a.extend(s(&["-hwaccel", "d3d11va"]));
    let ss = p.start_segment * SEGMENT_SECS;
    if p.start_segment > 0 {
        // `-ss 0` damages the first audio packet, so it is only emitted for k > 0
        a.extend(s(&["-ss", &ss.to_string()]));
    }
    a.extend(s(&["-protocol_whitelist", PROTOCOL_WHITELIST, "-i", p.input_url]));
    a.extend(s(&["-map", "0:v:0", "-map", &format!("0:a:{}?", p.audio_track)]));
    a.extend(s(&["-vf", &video_filter(p)?]));
    if p.encoder == Encoder::H264Amf {
        // the graph fixes the cadence; keep source pts (and their relation to the audio)
        a.extend(s(&["-fps_mode", "passthrough"]));
    }
    a.extend(encoder_args(p));
    a.extend(s(&["-c:a", "aac", "-b:a", "192k", "-ar", "48000", "-ac", "2"]));
    if p.start_segment > 0 {
        a.extend(s(&["-output_ts_offset", &ss.to_string()]));
    }
    a.extend(s(&[
        "-f", "hls", "-hls_time", &SEGMENT_SECS.to_string(), "-hls_segment_type", "fmp4", "-hls_flags", "independent_segments", "-hls_list_size", "6",
        "-start_number", &p.start_segment.to_string(), "-hls_fmp4_init_filename", "init.mp4",
        "-hls_segment_options", "movflags=+frag_discont:use_editlist=0:avoid_negative_ts=disabled",
        "-hls_segment_filename", &format!("{}/seg_%d.m4s", p.sink_base), "-method", "PUT",
        "-protocol_whitelist", PROTOCOL_WHITELIST, &format!("{}/live.m3u8", p.sink_base),
    ]));
    Ok(a.into_iter().map(OsString::from).collect())
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
        let p = PipelineParams { profile, encoder: enc, input_url: IN, sink_base: SINK, start_segment: start, audio_track: audio, shader_file: shader.as_deref(), adapter: 0 };
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
        let p = PipelineParams { profile, encoder: Encoder::H264Nvenc, input_url: IN, sink_base: SINK, start_segment: 0, audio_track: 0, shader_file: Some("x.hook"), adapter: 0 };
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
        for profile in &CATALOG {
            for enc in Encoder::PREFERENCE {
                let shader = match profile.upscale {
                    Upscale::Anime4k { mode, variant } => Some(anime4k::shader_file_name(mode, variant)),
                    Upscale::None => None,
                };
                let p = PipelineParams { profile, encoder: enc, input_url: IN, sink_base: SINK, start_segment: 3, audio_track: 0, shader_file: shader.as_deref(), adapter: 0 };
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
                assert_eq!(wl, [PROTOCOL_WHITELIST, PROTOCOL_WHITELIST], "{} {:?}", profile.id, enc);
                assert!(wl.iter().all(|w| w.split(',').any(|p| p == "crypto")));
                assert!(!a.iter().any(|x| x.contains("file:") || x.contains('\\') || x.contains(" ")), "no paths or shell-isms");
            }
        }
    }
}
