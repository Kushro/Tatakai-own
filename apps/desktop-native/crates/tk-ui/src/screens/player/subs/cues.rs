//! Subtitle text formats for translation (wave A, worker **d**): SRT, WebVTT and ASS / SSA into [`CueVm`]s, plain
//! text for providers and the live overlay, and SRT output for the pre-translated track. Pure and tested; the
//! player itself still renders the original tracks through libass.

use crate::backend::CueVm;

/// Parse any of SRT, WebVTT, ASS / SSA (detected from the content). Cues come back sorted by start time and
/// indexed from 0; cues without text are dropped.
pub(crate) fn parse(text: &str) -> Vec<CueVm> {
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n").replace('\r', "\n");
    let head = text.trim_start();
    let mut cues = if head.starts_with("[Script Info]") || text.contains("\nDialogue:") || head.starts_with("Dialogue:") {
        parse_ass(&text)
    } else {
        parse_srt_vtt(&text)
    };
    cues.retain(|c| !plain(&c.text).is_empty() && c.end_ms > c.start_ms);
    cues.sort_by_key(|c| (c.start_ms, c.end_ms));
    for (i, c) in cues.iter_mut().enumerate() {
        c.index = u32::try_from(i).unwrap_or(u32::MAX);
    }
    cues
}

/// `HH:MM:SS,mmm`, `HH:MM:SS.mmm`, `MM:SS.mmm` (VTT) and ASS `H:MM:SS.cc` -> milliseconds.
pub(crate) fn parse_time(s: &str) -> Option<u64> {
    let s = s.trim();
    let (hms, frac) = match s.rfind(['.', ',']) {
        Some(i) => (&s[..i], &s[i + 1..]),
        None => (s, ""),
    };
    let parts: Vec<&str> = hms.split(':').collect();
    let nums: Option<Vec<u64>> = parts.iter().map(|p| p.trim().parse::<u64>().ok()).collect();
    let nums = nums?;
    let secs = match nums.as_slice() {
        [h, m, sec] => h * 3600 + m * 60 + sec,
        [m, sec] => m * 60 + sec,
        _ => return None,
    };
    let frac_ms = if frac.is_empty() {
        0
    } else {
        let digits: String = frac.chars().take_while(char::is_ascii_digit).take(3).collect();
        if digits.is_empty() {
            return None;
        }
        // "5" = 500 ms, "50" (ASS centiseconds) = 500 ms, "500" = 500 ms.
        let n: u64 = digits.parse().ok()?;
        n * 10u64.pow(3 - u32::try_from(digits.len()).unwrap_or(3))
    };
    Some(secs * 1000 + frac_ms)
}

fn parse_srt_vtt(text: &str) -> Vec<CueVm> {
    let mut out = Vec::new();
    for block in text.split("\n\n") {
        let lines: Vec<&str> = block.lines().collect();
        let Some(pos) = lines.iter().position(|l| l.contains("-->")) else { continue };
        let timing = lines[pos];
        let Some((a, b)) = timing.split_once("-->") else { continue };
        // VTT cue settings follow the end time ("00:01.000 --> 00:02.000 align:start").
        let b = b.split_whitespace().next().unwrap_or("");
        let (Some(start), Some(end)) = (parse_time(a), parse_time(b)) else { continue };
        let body: Vec<&str> = lines[pos + 1..].iter().map(|l| l.trim_end()).collect();
        let text = body.join("\n").trim().to_owned();
        if text.is_empty() {
            continue;
        }
        out.push(CueVm { index: 0, start_ms: start, end_ms: end, text });
    }
    out
}

fn parse_ass(text: &str) -> Vec<CueVm> {
    let mut out = Vec::new();
    let mut in_events = false;
    // Default ASS v4+ event format.
    let mut fields: Vec<String> =
        ["layer", "start", "end", "style", "name", "marginl", "marginr", "marginv", "effect", "text"].iter().map(|s| (*s).to_owned()).collect();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_events = line.eq_ignore_ascii_case("[events]");
            continue;
        }
        if !in_events && !line.starts_with("Dialogue:") {
            continue;
        }
        if let Some(rest) = line.strip_prefix("Format:") {
            fields = rest.split(',').map(|f| f.trim().to_ascii_lowercase()).collect();
            continue;
        }
        let Some(rest) = line.strip_prefix("Dialogue:") else { continue };
        let n = fields.len().max(1);
        let values: Vec<&str> = rest.splitn(n, ',').collect();
        if values.len() < n {
            continue;
        }
        let at = |name: &str| fields.iter().position(|f| f == name).and_then(|i| values.get(i)).map(|v| v.trim());
        let (Some(start), Some(end)) = (at("start").and_then(parse_time), at("end").and_then(parse_time)) else { continue };
        let raw = fields.iter().position(|f| f == "text").and_then(|i| values.get(i)).copied().unwrap_or("");
        // Vector drawings carry no text.
        if raw.contains("\\p1") {
            continue;
        }
        let text = ass_text(raw);
        if text.is_empty() {
            continue;
        }
        out.push(CueVm { index: 0, start_ms: start, end_ms: end, text });
    }
    out
}

/// ASS dialogue text -> plain lines: override blocks `{...}` removed, `\N` / `\n` -> newline, `\h` -> space.
fn ass_text(raw: &str) -> String {
    let mut out = String::new();
    let mut depth = 0usize;
    for c in raw.chars() {
        match c {
            '{' => depth += 1,
            '}' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    let out = out.replace("\\N", "\n").replace("\\n", "\n").replace("\\h", " ");
    out.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n")
}

/// Text without markup: HTML-like tags (`<i>`, `<font ..>`, VTT `<c.x>` / `<00:01.000>`), ASS override blocks and
/// `\N`; whitespace collapsed per line, empty lines dropped.
pub(crate) fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    let mut depth = 0usize;
    for c in text.chars() {
        match c {
            '<' if depth == 0 => in_tag = true,
            '>' if in_tag => in_tag = false,
            '{' if !in_tag => depth += 1,
            '}' if depth > 0 => depth -= 1,
            _ if !in_tag && depth == 0 => out.push(c),
            _ => {}
        }
    }
    let out = out.replace("\\N", "\n").replace("\\n", "\n").replace("\\h", " ").replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&nbsp;", " ");
    out.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n")
}

/// Lookup key for a line of text (mpv `sub-text` and parsed cues normalise the same way).
pub(crate) fn key(text: &str) -> String {
    plain(text).to_lowercase()
}

/// `HH:MM:SS,mmm`
fn srt_time(ms: u64) -> String {
    format!("{:02}:{:02}:{:02},{:03}", ms / 3_600_000, (ms / 60_000) % 60, (ms / 1000) % 60, ms % 1000)
}

/// SRT text of `cues` (numbered from 1).
pub(crate) fn to_srt(cues: &[CueVm]) -> String {
    let mut out = String::new();
    for (i, c) in cues.iter().enumerate() {
        out.push_str(&format!("{}\n{} --> {}\n{}\n\n", i + 1, srt_time(c.start_ms), srt_time(c.end_ms), c.text.trim()));
    }
    out
}

/// Index of the cue showing at `ms` (track time), if any. `cues` sorted by start.
pub(crate) fn cue_at(cues: &[CueVm], ms: u64) -> Option<usize> {
    let upto = cues.partition_point(|c| c.start_ms <= ms);
    // Overlapping cues: the latest started one that still shows.
    (0..upto).rev().take(8).find(|&i| cues[i].end_ms > ms)
}

/// Index of the first cue that ends after `ms` (the one showing or the next one).
pub(crate) fn cue_from(cues: &[CueVm], ms: u64) -> usize {
    cues.partition_point(|c| c.end_ms <= ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRT: &str = "1\r\n00:00:01,000 --> 00:00:02,500\r\n<i>Hello</i>\r\nthere\r\n\r\n2\r\n00:00:03,000 --> 00:00:04,000\r\nBye\r\n";
    const VTT: &str = "WEBVTT\n\nNOTE a comment\n\n00:01.000 --> 00:02.000 align:start\n<c.yellow>Hi</c>\n\nid-2\n01:00:00.500 --> 01:00:01.000\nLate\n";
    const ASS: &str = "[Script Info]\nTitle: x\n\n[V4+ Styles]\nFormat: Name, Fontname\nStyle: Default,Arial\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:05.50,0:00:07.00,Default,,0,0,0,,{\\i1}Hello,\\Nworld{\\i0}\nDialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,First\nDialogue: 0,0:00:08.00,0:00:09.00,Sign,,0,0,0,,{\\p1}m 0 0 l 10 10{\\p0}\n";

    #[test]
    fn times_in_every_format() {
        assert_eq!(parse_time("00:00:01,000"), Some(1000));
        assert_eq!(parse_time("01:02:03.456"), Some(3_723_456));
        assert_eq!(parse_time("00:01.5"), Some(1500));
        assert_eq!(parse_time("0:00:05.50"), Some(5500));
        assert_eq!(parse_time("nope"), None);
    }

    #[test]
    fn srt_and_vtt_parse() {
        let c = parse(SRT);
        assert_eq!(c.len(), 2);
        assert_eq!((c[0].start_ms, c[0].end_ms, c[0].text.as_str()), (1000, 2500, "<i>Hello</i>\nthere"));
        assert_eq!(plain(&c[0].text), "Hello\nthere");
        let v = parse(VTT);
        assert_eq!(v.len(), 2);
        assert_eq!((v[0].start_ms, plain(&v[0].text).as_str()), (1000, "Hi"));
        assert_eq!(v[1].start_ms, 3_600_500);
    }

    #[test]
    fn ass_parses_sorted_with_tags_stripped_and_drawings_skipped() {
        let c = parse(ASS);
        assert_eq!(c.len(), 2);
        assert_eq!((c[0].start_ms, c[0].text.as_str(), c[0].index), (1000, "First", 0));
        assert_eq!((c[1].start_ms, c[1].end_ms, c[1].text.as_str(), c[1].index), (5500, 7000, "Hello,\nworld", 1));
    }

    #[test]
    fn srt_output_round_trips() {
        let c = parse(SRT);
        let again = parse(&to_srt(&c));
        assert_eq!(again, c);
        assert!(to_srt(&c).starts_with("1\n00:00:01,000 --> 00:00:02,500\n"));
    }

    #[test]
    fn lookup_by_time_and_key() {
        let c = parse(SRT);
        assert_eq!(cue_at(&c, 1500), Some(0));
        assert_eq!(cue_at(&c, 2700), None);
        assert_eq!(cue_at(&c, 3500), Some(1));
        assert_eq!(cue_from(&c, 2700), 1);
        assert_eq!(cue_from(&c, 9000), 2);
        assert_eq!(key("<i>Hello</i>\n  THERE "), "hello\nthere");
        assert_eq!(plain("{\\an8}Tom &amp; Jerry\\Nok"), "Tom & Jerry\nok");
    }
}
