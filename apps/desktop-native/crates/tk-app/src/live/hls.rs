//! BUG-059 on the PC: FFmpeg's HLS demuxer (libmpv, embedded and external mpv) opens every variant and every
//! `EXT-X-MEDIA` rendition of a master playlist and reads a segment of each before the first frame. anizone through an
//! extension-server proxy (the PC's own local proxy for header-bound sources, or another PC's server) lists 15 audio
//! renditions + 3 variants, and one rendition without segments yet kept mpv at "0:00 / --:--" for minutes. Handing mpv
//! a master with only the variant it would play and the renditions in the preferred languages avoids waiting for the
//! others. Same rules as Android's `HlsMasterFilter` (e24b268b).
//!
//! Only for sources without HTTP headers (proxy URLs carry their own): mpv plays the filtered copy from a local file,
//! and nested requests from a local master may not carry the source's Referer / User-Agent.

use std::time::Duration;

/// Larger "masters" are not playlists worth rewriting.
const MAX_MASTER_BYTES: usize = 512 * 1024;
const FETCH_TIMEOUT: Duration = Duration::from_secs(8);

/// ISO 639-1 of a `LANGUAGE` tag or a rendition `NAME` (`es-419`, `spa`, `Spanish (Latin America)`, `Español` -> `es`).
pub(crate) fn lang_of(tag_or_name: &str) -> Option<String> {
    let s = tag_or_name.trim().to_lowercase();
    if s.is_empty() {
        return None;
    }
    let s: String = s
        .chars()
        .map(|c| match c {
            'á' | 'à' => 'a',
            'é' => 'e',
            'í' => 'i',
            'ó' => 'o',
            'ú' => 'u',
            'ñ' => 'n',
            'ç' => 'c',
            other => other,
        })
        .collect();
    const ISO3: [(&str, &str); 22] = [
        ("spa", "es"), ("esl", "es"), ("eng", "en"), ("jpn", "ja"), ("jap", "ja"), ("por", "pt"), ("fre", "fr"), ("fra", "fr"),
        ("ger", "de"), ("deu", "de"), ("ita", "it"), ("rus", "ru"), ("kor", "ko"), ("chi", "zh"), ("zho", "zh"), ("ara", "ar"),
        ("hin", "hi"), ("ind", "id"), ("vie", "vi"), ("tha", "th"), ("tur", "tr"), ("pol", "pl"),
    ];
    const NAMES: [(&str, &str); 26] = [
        ("spanish", "es"), ("espanol", "es"), ("castellano", "es"), ("latino", "es"), ("english", "en"), ("ingles", "en"),
        ("japanese", "ja"), ("japones", "ja"), ("日本語", "ja"), ("portuguese", "pt"), ("portugues", "pt"), ("french", "fr"),
        ("francais", "fr"), ("german", "de"), ("deutsch", "de"), ("italian", "it"), ("italiano", "it"), ("russian", "ru"),
        ("korean", "ko"), ("chinese", "zh"), ("arabic", "ar"), ("hindi", "hi"), ("indonesian", "id"), ("thai", "th"),
        ("vietnamese", "vi"), ("turkish", "tr"),
    ];
    let words: Vec<&str> = s.split(|c: char| matches!(c, '-' | '_' | ' ' | '(' | ')' | '[' | ']' | '.' | ',' | '/' | ':')).filter(|w| !w.is_empty()).collect();
    let head = *words.first()?;
    if matches!(head, "und" | "unk" | "mul" | "zxx") {
        return None;
    }
    if head.len() == 2 && head.chars().all(|c| c.is_ascii_lowercase()) {
        let fixed = match head {
            "jp" => "ja",
            "cn" => "zh",
            "kr" => "ko",
            "br" => "pt",
            "sp" => "es",
            h => h,
        };
        return Some(fixed.to_owned());
    }
    if let Some((_, c)) = ISO3.iter().find(|(k, _)| *k == head) {
        return Some((*c).to_owned());
    }
    words.iter().find_map(|w| NAMES.iter().find(|(k, _)| k == w)).map(|(_, c)| (*c).to_owned())
}

fn lang_matches(language: Option<&str>, name: Option<&str>, pref: &str) -> bool {
    let pref = match lang_of(pref) {
        Some(p) => p,
        None => return false,
    };
    [language, name].into_iter().flatten().any(|v| lang_of(v).as_deref() == Some(pref.as_str()))
}

/// `KEY=value` / `KEY="value"` pairs of an attribute list.
fn attrs(list: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = list;
    while !rest.is_empty() {
        let Some(eq) = rest.find('=') else { break };
        let key = rest[..eq].trim().trim_start_matches(',').trim().to_owned();
        rest = &rest[eq + 1..];
        let value;
        if let Some(r) = rest.strip_prefix('"') {
            let end = r.find('"').unwrap_or(r.len());
            value = r[..end].to_owned();
            rest = r.get(end + 1..).unwrap_or("");
        } else {
            let end = rest.find(',').unwrap_or(rest.len());
            value = rest[..end].to_owned();
            rest = &rest[end..];
        }
        rest = rest.trim_start_matches(',');
        out.push((key, value));
    }
    out
}

fn attr<'a>(a: &'a [(String, String)], key: &str) -> Option<&'a str> {
    a.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

/// `rel` resolved against `base` (RFC 3986 for the cases playlists use: absolute, `//host`, `/path`, relative with
/// `./` / `../`).
pub(crate) fn resolve(base: &str, rel: &str) -> String {
    let rel = rel.trim();
    if rel.starts_with("http://") || rel.starts_with("https://") {
        return rel.to_owned();
    }
    let Some(scheme_end) = base.find("://") else { return rel.to_owned() };
    let scheme = &base[..scheme_end];
    if let Some(r) = rel.strip_prefix("//") {
        return format!("{scheme}://{r}");
    }
    let after = &base[scheme_end + 3..];
    let host_end = after.find('/').unwrap_or(after.len());
    let origin = &base[..scheme_end + 3 + host_end];
    let base_path = after[host_end..].split(['?', '#']).next().unwrap_or("");
    let joined = if rel.starts_with('/') {
        rel.to_owned()
    } else {
        let dir = &base_path[..base_path.rfind('/').map_or(0, |i| i + 1)];
        format!("{}{rel}", if dir.is_empty() { "/" } else { dir })
    };
    // Normalize `.` and `..` in the path (not in the query).
    let (path, query) = match joined.find('?') {
        Some(i) => (&joined[..i], &joined[i..]),
        None => (joined.as_str(), ""),
    };
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/').skip(1) {
        match seg {
            "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    format!("{origin}/{}{query}", parts.join("/"))
}

/// The filtered master, or `None` when `text` is not a master playlist or nothing would be dropped (then the original
/// plays). Keeps one variant (the highest `BANDWIDTH`, what mpv picks) and, per audio / subtitle group that variant
/// uses, the renditions in `audio_lang` / `sub_lang` (matched on `LANGUAGE` or `NAME`); a group with no match keeps its
/// `DEFAULT=YES` rendition, else its first. Renditions without `URI` (muxed) and closed captions stay. Every URI
/// becomes absolute against `base_url`.
pub(crate) fn filter_master(text: &str, base_url: &str, audio_lang: &str, sub_lang: &str) -> Option<String> {
    if !text.trim_start().starts_with("#EXTM3U") {
        return None;
    }
    let lines: Vec<&str> = text.lines().collect();
    if !lines.iter().any(|l| l.starts_with("#EXT-X-STREAM-INF")) {
        return None;
    }
    struct Variant<'a> {
        tag: &'a str,
        uri: &'a str,
        bandwidth: u64,
    }
    let mut variants: Vec<Variant> = Vec::new();
    let mut media: Vec<(Vec<(String, String)>, &str)> = Vec::new();
    let mut others: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();
        if line.starts_with("#EXT-X-STREAM-INF") {
            let mut j = i + 1;
            while j < lines.len() && (lines[j].trim().is_empty() || lines[j].trim().starts_with('#')) {
                j += 1;
            }
            if j < lines.len() {
                let a = attrs(line.split_once(':').map_or("", |x| x.1));
                let bandwidth = attr(&a, "BANDWIDTH").and_then(|b| b.parse().ok()).unwrap_or(0);
                variants.push(Variant { tag: line, uri: lines[j].trim(), bandwidth });
            }
            i = j + 1;
            continue;
        }
        if line.starts_with("#EXT-X-MEDIA:") {
            media.push((attrs(&line["#EXT-X-MEDIA:".len()..]), line));
        } else if line.starts_with("#EXT-X-I-FRAME-STREAM-INF") {
            // Not played; FFmpeg skips them anyway.
        } else if !line.is_empty() && line != "#EXTM3U" {
            others.push(line);
        }
        i += 1;
    }
    // Highest bandwidth; the first one on ties (Kotlin's maxBy).
    let chosen = variants.iter().fold(None::<&Variant>, |best, v| match best {
        Some(b) if b.bandwidth >= v.bandwidth => Some(b),
        _ => Some(v),
    })?;
    let ca = attrs(chosen.tag.split_once(':').map_or("", |x| x.1));
    let used: Vec<&str> = ["AUDIO", "SUBTITLES", "VIDEO", "CLOSED-CAPTIONS"].iter().filter_map(|k| attr(&ca, k)).collect();

    let mut keep = vec![false; media.len()];
    let mut groups: Vec<(String, String)> = Vec::new();
    for (a, _) in &media {
        let key = (attr(a, "TYPE").unwrap_or("").to_owned(), attr(a, "GROUP-ID").unwrap_or("").to_owned());
        if !groups.contains(&key) {
            groups.push(key);
        }
    }
    for (ty, group) in &groups {
        if !used.contains(&group.as_str()) {
            continue;
        }
        let idx: Vec<usize> = (0..media.len())
            .filter(|&k| attr(&media[k].0, "TYPE").unwrap_or("") == ty && attr(&media[k].0, "GROUP-ID").unwrap_or("") == group)
            .collect();
        let with_uri: Vec<usize> = idx.iter().copied().filter(|&k| attr(&media[k].0, "URI").is_some()).collect();
        for &k in idx.iter().filter(|&&k| attr(&media[k].0, "URI").is_none()) {
            keep[k] = true;
        }
        if ty == "CLOSED-CAPTIONS" || with_uri.is_empty() {
            continue;
        }
        if ty == "VIDEO" {
            for &k in &with_uri {
                keep[k] = true;
            }
            continue;
        }
        let pref = match ty.as_str() {
            "AUDIO" => audio_lang,
            "SUBTITLES" => sub_lang,
            _ => "",
        };
        let matched: Vec<usize> =
            with_uri.iter().copied().filter(|&k| lang_matches(attr(&media[k].0, "LANGUAGE"), attr(&media[k].0, "NAME"), pref)).collect();
        let picked = if matched.is_empty() {
            let default: Vec<usize> = with_uri
                .iter()
                .copied()
                .filter(|&k| attr(&media[k].0, "DEFAULT").is_some_and(|d| d.eq_ignore_ascii_case("YES")))
                .take(1)
                .collect();
            if default.is_empty() { vec![with_uri[0]] } else { default }
        } else {
            matched
        };
        for k in picked {
            keep[k] = true;
        }
    }
    if variants.len() == 1 && keep.iter().all(|k| *k) {
        return None;
    }
    let abs_uri_attr = |line: &str| -> String {
        match line.find("URI=\"") {
            Some(s) => {
                let start = s + 5;
                let end = line[start..].find('"').map_or(line.len(), |e| start + e);
                format!("{}{}{}", &line[..start], resolve(base_url, &line[start..end]), &line[end..])
            }
            None => line.to_owned(),
        }
    };
    let mut out = String::from("#EXTM3U\n");
    for l in others {
        out.push_str(&abs_uri_attr(l));
        out.push('\n');
    }
    for (k, (_, line)) in media.iter().enumerate() {
        if keep[k] {
            out.push_str(&abs_uri_attr(line));
            out.push('\n');
        }
    }
    out.push_str(chosen.tag);
    out.push('\n');
    out.push_str(&resolve(base_url, chosen.uri));
    out.push('\n');
    Some(out)
}

/// Whether the stream is worth checking: an HLS URL (proxy URLs keep the file name) without headers.
pub(crate) fn applies(url: &str, headers: &[(String, String)]) -> bool {
    let path = url.split(['?', '#']).next().unwrap_or("").to_ascii_lowercase();
    headers.is_empty() && (url.starts_with("http://") || url.starts_with("https://")) && path.ends_with(".m3u8")
}

/// Download the master at `url` and, when [`filter_master`] drops something, write the filtered copy into `dir` (the
/// previous copies are removed: one load at a time) and return its path. `None` = play `url` as it is (not a master,
/// nothing to drop, or any failure).
pub(crate) async fn filtered_master(dir: &std::path::Path, url: &str, audio_lang: &str, sub_lang: &str) -> Option<String> {
    let client = reqwest::Client::builder().timeout(FETCH_TIMEOUT).build().ok()?;
    let resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() || resp.content_length().is_some_and(|n| n as usize > MAX_MASTER_BYTES) {
        return None;
    }
    let final_url = resp.url().to_string();
    let body = resp.bytes().await.ok()?;
    if body.len() > MAX_MASTER_BYTES {
        return None;
    }
    let text = String::from_utf8_lossy(&body);
    let filtered = filter_master(&text, &final_url, audio_lang, sub_lang)?;
    let count = |t: &str| t.lines().filter(|l| l.starts_with("#EXT-X-MEDIA")).count();
    std::fs::create_dir_all(dir).ok()?;
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let _ = std::fs::remove_file(e.path());
        }
    }
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let path = dir.join(format!("master-{nanos}.m3u8"));
    std::fs::write(&path, filtered.as_bytes()).ok()?;
    tracing::info!(before = count(&text), after = count(&filtered), "HLS master filtered for libmpv (BUG-059)");
    Some(path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const MASTER: &str = "#EXTM3U\n#EXT-X-INDEPENDENT-SEGMENTS\n\
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aud\",NAME=\"Japanese\",LANGUAGE=\"ja\",DEFAULT=YES,URI=\"audio/ja.m3u8\"\n\
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aud\",NAME=\"English\",LANGUAGE=\"en\",URI=\"audio/en.m3u8\"\n\
#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"aud\",NAME=\"Español (América Latina)\",LANGUAGE=\"es-419\",URI=\"audio/es.m3u8\"\n\
#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"subs\",NAME=\"English\",LANGUAGE=\"en\",URI=\"subs/en.m3u8\"\n\
#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"subs\",NAME=\"Spanish\",LANGUAGE=\"spa\",URI=\"/abs/subs/es.m3u8\"\n\
#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360,AUDIO=\"aud\",SUBTITLES=\"subs\"\nv/360.m3u8\n\
#EXT-X-STREAM-INF:BANDWIDTH=5000000,RESOLUTION=1920x1080,AUDIO=\"aud\",SUBTITLES=\"subs\"\nv/1080.m3u8\n";

    #[test]
    fn keeps_the_top_variant_and_the_preferred_languages() {
        let out = filter_master(MASTER, "http://127.0.0.1:9000/v1/media/t/master.m3u8?x=1", "ja", "es").unwrap();
        assert!(out.contains("URI=\"http://127.0.0.1:9000/v1/media/t/audio/ja.m3u8\""), "{out}");
        assert!(!out.contains("audio/en.m3u8") && !out.contains("audio/es.m3u8"), "{out}");
        assert!(out.contains("URI=\"http://127.0.0.1:9000/abs/subs/es.m3u8\"") && !out.contains("subs/en.m3u8"), "{out}");
        assert!(out.contains("http://127.0.0.1:9000/v1/media/t/v/1080.m3u8") && !out.contains("360.m3u8"), "{out}");
        assert!(out.contains("#EXT-X-INDEPENDENT-SEGMENTS"));
    }

    #[test]
    fn falls_back_to_the_default_rendition_and_leaves_simple_playlists_alone() {
        let out = filter_master(MASTER, "https://cdn.test/a/master.m3u8", "ko", "ko").unwrap();
        assert!(out.contains("audio/ja.m3u8") && out.contains("subs/en.m3u8"), "{out}");
        assert_eq!(filter_master("#EXTM3U\n#EXTINF:4,\nseg1.ts\n", "https://cdn.test/a.m3u8", "ja", "es"), None);
        let single = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nv.m3u8\n";
        assert_eq!(filter_master(single, "https://cdn.test/a.m3u8", "ja", "es"), None);
    }

    #[test]
    fn resolves_languages_urls_and_scope() {
        assert_eq!(lang_of("es-419").as_deref(), Some("es"));
        assert_eq!(lang_of("Español (América Latina)").as_deref(), Some("es"));
        assert_eq!(lang_of("jpn").as_deref(), Some("ja"));
        assert_eq!(lang_of("und"), None);
        assert_eq!(resolve("https://a.test/x/y/m.m3u8?t=1", "../z.m3u8"), "https://a.test/x/z.m3u8");
        assert_eq!(resolve("https://a.test/x/m.m3u8", "//b.test/q.m3u8"), "https://b.test/q.m3u8");
        assert!(applies("http://127.0.0.1:1/v1/media/t/master.m3u8", &[]));
        assert!(!applies("https://cdn.test/master.m3u8", &[("Referer".into(), "x".into())]));
        assert!(!applies("http://127.0.0.1:1/stream/0/ep.mkv", &[]));
    }
}
