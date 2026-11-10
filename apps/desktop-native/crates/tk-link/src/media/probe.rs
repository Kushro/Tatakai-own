//! Source frame rate, needed by the LSFG chain (the second FFmpeg reads raw frames at `source rate x multiplier`).
//!
//! `FrameRate` is ported from Enigmata-Temple `crates/temple-core/src/media/probe.rs` (AGPL-3.0, same author). Link
//! only asks `ffprobe` for the first video stream's `r_frame_rate` / `avg_frame_rate`, reading the loopback fetcher
//! URL with the same `-protocol_whitelist` as FFmpeg.

use serde::Deserialize;

/// Highest source rate accepted from `ffprobe`.
pub const MAX_SOURCE_FPS: f64 = 240.0;

/// Rate assumed when the probe fails: the decode process forces its cadence with `fps=<rate>`, so audio and video
/// stay in sync with any value; 23.976 is the common rate of anime.
pub const FALLBACK_RATE: FrameRate = FrameRate { num: 24_000, den: 1001 };

/// Source frame rate as an exact fraction (`24000/1001`), as `ffprobe` prints it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameRate {
    pub num: u32,
    pub den: u32,
}

impl FrameRate {
    /// `n/d` (or `n`) from `ffprobe`: both positive and the rate between 1 and 240 fps; otherwise `None` (`0/0` is
    /// what `ffprobe` prints when it does not know).
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let (num, den) = match text.split_once('/') {
            Some((num, den)) => (num.trim().parse::<u32>().ok()?, den.trim().parse::<u32>().ok()?),
            None => (text.parse::<u32>().ok()?, 1),
        };
        let rate = Self { num, den };
        (num > 0 && den > 0 && (1.0..=MAX_SOURCE_FPS).contains(&rate.fps())).then_some(rate)
    }

    pub fn fps(self) -> f64 {
        f64::from(self.num) / f64::from(self.den)
    }

    /// The rate times `factor`, same denominator (`24000/1001` x 5 = `120000/1001`). `None` on overflow.
    pub fn times(self, factor: u32) -> Option<Self> {
        Some(Self { num: self.num.checked_mul(factor)?, den: self.den })
    }

    /// `num/den`, the form of `-framerate` and of the `fps` filter.
    pub fn ffmpeg(self) -> String {
        format!("{}/{}", self.num, self.den)
    }
}

/// `ffprobe` arguments (without the program) that print the first video stream's rates as JSON.
pub fn ffprobe_args(input_url: &str) -> Vec<String> {
    [
        "-v",
        "error",
        "-protocol_whitelist",
        super::args::PROTOCOL_WHITELIST,
        "-select_streams",
        "v:0",
        "-show_entries",
        "stream=r_frame_rate,avg_frame_rate",
        "-print_format",
        "json",
        "-i",
        input_url,
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect()
}

/// The rate of the first video stream in `ffprobe -print_format json` output (`r_frame_rate` first, then
/// `avg_frame_rate`, as Temple does).
pub fn parse_ffprobe_rate(stdout: &str) -> Option<FrameRate> {
    #[derive(Deserialize)]
    struct Stream {
        r_frame_rate: Option<String>,
        avg_frame_rate: Option<String>,
    }
    #[derive(Deserialize)]
    struct Out {
        #[serde(default)]
        streams: Vec<Stream>,
    }
    let out: Out = serde_json::from_str(stdout).ok()?;
    let s = out.streams.into_iter().next()?;
    [s.r_frame_rate, s.avg_frame_rate].into_iter().flatten().find_map(|t| FrameRate::parse(&t))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates_parse_and_multiply() {
        assert_eq!(FrameRate::parse("24000/1001"), Some(FrameRate { num: 24_000, den: 1001 }));
        assert_eq!(FrameRate::parse(" 30 "), Some(FrameRate { num: 30, den: 1 }));
        for bad in ["0/0", "1000/1", "abc", "", "-1/2", "1/0"] {
            assert_eq!(FrameRate::parse(bad), None, "{bad}");
        }
        let r = FrameRate::parse("24000/1001").unwrap();
        assert_eq!(r.times(5).unwrap().ffmpeg(), "120000/1001");
        assert!((r.fps() - 23.976).abs() < 0.001);
        assert_eq!(FrameRate { num: u32::MAX, den: 1 }.times(2), None);
    }

    #[test]
    fn ffprobe_json_prefers_r_frame_rate_and_falls_back() {
        let j = r#"{"programs":[],"streams":[{"r_frame_rate":"24000/1001","avg_frame_rate":"0/0"}]}"#;
        assert_eq!(parse_ffprobe_rate(j), Some(FrameRate { num: 24_000, den: 1001 }));
        let j = r#"{"streams":[{"r_frame_rate":"0/0","avg_frame_rate":"30/1"}]}"#;
        assert_eq!(parse_ffprobe_rate(j), Some(FrameRate { num: 30, den: 1 }));
        assert_eq!(parse_ffprobe_rate(r#"{"streams":[]}"#), None);
        assert_eq!(parse_ffprobe_rate("not json"), None);
        let a = ffprobe_args("http://127.0.0.1:1/u/t");
        assert_eq!(a.last().unwrap(), "http://127.0.0.1:1/u/t");
        assert!(a.windows(2).any(|w| w[0] == "-protocol_whitelist" && w[1] == "http,tcp,crypto"));
    }
}
