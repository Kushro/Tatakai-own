//! `sources` / `stream` / `torrent` route bodies -> [`SourceCandidate`]s for the source selector
//! (contracts/extension-host/routes.v1.json `nativeMapping.SourceCandidate`, docs/redesign/08 section 3).
//!
//! Every selector field is optional in the extension output: a missing `server` falls back to the provider name,
//! missing `quality` is `Unknown`, missing `subs` / `dubs` come from the subtitle tracks / `audioLanguage`, and a
//! missing `note` is `None`. Nothing here is ever an error; an entry that cannot be listed at all (embed page,
//! torrent without a hash) is skipped.

use serde_json::Value;
use tk_core::release::ReleaseTags;
use tk_core::sources::{INITIAL_RELIABILITY, Quality, SourceCandidate, SourceKind, TorrentInfo, WebInfo, container_label, lang_codes};

use crate::host::{headers_of, subtitles_of, web_container};
use crate::manifest::Manifest;

/// Routes [`crate::ExtensionHost::resolve_candidates`] asks one enabled, runnable extension (docs/redesign/10
/// section 6), in order. Contract stub = the wave 1 rule: `torrent` for `torrent` extensions, `stream` for the others,
/// each only if `Manifest::gated_routes` allows it. Wave 3 E: `custom` / `onlinestream` extensions whose manifest
/// lists `torrent` explicitly in `apiServer.routes` (the default route list does not count) or whose bundle exposes
/// `torrentAll` (`exposes_torrent_all`) are asked `stream` **and** `torrent` (Toko: Nyaa, SubsPlease, AnimeTosho,
/// SeaDex, nekoBT, AniLiberty).
pub fn candidate_routes(m: &Manifest, exposes_torrent_all: bool) -> Vec<&'static str> {
    let _ = exposes_torrent_all;
    let route = if m.kind == "torrent" { "torrent" } else { "stream" };
    if m.gated_routes().iter().any(|r| r == route) { vec![route] } else { Vec::new() }
}

/// Map one route body (`{ sources: SourceResultNormalized[] }`) of extension `ext_id` (display name `ext_name`).
/// Order is kept; labels repeated inside one body get the quality (or `#n`) appended so every candidate has its
/// own `source_key`.
pub fn map_candidates(ext_id: &str, ext_name: &str, body: &Value) -> Vec<SourceCandidate> {
    let Some(list) = body["sources"].as_array() else { return Vec::new() };
    let mut out: Vec<SourceCandidate> = Vec::new();
    for s in list {
        let Some(mut c) = map_one(ext_id, ext_name, s) else { continue };
        if taken(&out, c.kind, &c.label) {
            let q = c.quality.label();
            let with_q = format!("{} {q}", c.label);
            if !q.is_empty() && !taken(&out, c.kind, &with_q) {
                c.label = with_q;
            } else {
                let mut n = 2u32;
                while taken(&out, c.kind, &format!("{} #{n}", c.label)) {
                    n += 1;
                }
                c.label = format!("{} #{n}", c.label);
            }
        }
        out.push(c);
    }
    out
}

fn taken(out: &[SourceCandidate], kind: SourceKind, label: &str) -> bool {
    out.iter().any(|o| o.kind == kind && o.label == label)
}

fn map_one(ext_id: &str, ext_name: &str, s: &Value) -> Option<SourceCandidate> {
    let url = s["url"].as_str().unwrap_or("");
    let ty = s["type"].as_str().or_else(|| s["sourceType"].as_str()).unwrap_or("");
    let provider = text(&s["providerName"]).unwrap_or_else(|| ext_id.to_owned());
    let quality = match &s["quality"] {
        Value::String(q) => Quality::parse(q),
        Value::Number(n) => Quality::parse(&n.to_string()),
        _ => Quality::Unknown,
    };
    let subtitles = subtitles_of(s);
    let subs = match list_of(&s["subs"]) {
        Some(l) => lang_codes(l.iter().map(String::as_str)),
        None => lang_codes(subtitles.iter().map(|t| t.lang.as_str())),
    };
    let dubs = match list_of(&s["dubs"]) {
        Some(l) => lang_codes(l.iter().map(String::as_str)),
        None => lang_codes(s["audioLanguage"].as_str()),
    };
    let note = text(&s["note"]);
    if ty == "torrent" || s["isTorrent"].as_bool() == Some(true) {
        let magnet = Some(url).filter(|u| u.to_ascii_lowercase().starts_with("magnet:")).map(str::to_owned);
        let info_hash = text(&s["infoHash"])
            .or_else(|| text(&s["hash"]))
            .or_else(|| magnet.as_deref().and_then(btih))
            .map(|h| h.to_ascii_lowercase())?;
        let label = text(&s["group"]).or_else(|| text(&s["server"])).unwrap_or(provider);
        let torrent = TorrentInfo {
            info_hash,
            size_bytes: size_of(&s["sizeBytes"]).or_else(|| size_of(&s["fileSize"])).or_else(|| size_of(&s["size"])),
            seeders: count(&s["seeders"]),
            leechers: count(&s["leechers"]),
            magnet,
        };
        return Some(SourceCandidate {
            ext_id: ext_id.to_owned(),
            ext_name: ext_name.to_owned(),
            kind: SourceKind::Torrent,
            label,
            quality,
            container: text(&s["codec"]).or_else(|| text(&s["container"])),
            subs,
            dubs,
            reliability: INITIAL_RELIABILITY,
            note,
            web: None,
            torrent: Some(torrent),
            // wave 3 E: parse_release(name / title of the entry) + seadex_best from `seadexBest` / `isBest`
            tags: ReleaseTags::default(),
        });
    }
    if url.is_empty() {
        return None;
    }
    let container = web_container(s, url)?;
    let label = text(&s["server"]).unwrap_or(provider);
    Some(SourceCandidate {
        ext_id: ext_id.to_owned(),
        ext_name: ext_name.to_owned(),
        kind: SourceKind::Web,
        label,
        quality,
        container: Some(container_label(container).to_owned()),
        subs,
        dubs,
        reliability: INITIAL_RELIABILITY,
        note,
        web: Some(WebInfo {
            url: url.to_owned(),
            headers: headers_of(s),
            latency_ms: s["latencyMs"].as_u64().map(|v| u32::try_from(v).unwrap_or(u32::MAX)),
            subtitles,
        }),
        torrent: None,
        tags: ReleaseTags::default(),
    })
}

/// Non-empty trimmed string.
fn text(v: &Value) -> Option<String> {
    v.as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned)
}

/// `["es","en"]`, or a single `"es"` / `"es, en"`; `None` when the field is absent or of another type.
fn list_of(v: &Value) -> Option<Vec<String>> {
    match v {
        Value::Array(a) => Some(a.iter().filter_map(Value::as_str).map(str::to_owned).collect()),
        Value::String(s) => Some(s.split([',', ' ']).filter(|x| !x.is_empty()).map(str::to_owned).collect()),
        _ => None,
    }
}

fn count(v: &Value) -> u32 {
    match v {
        Value::Number(n) => n.as_u64().map_or_else(|| n.as_f64().filter(|f| *f > 0.0).map_or(0, |f| f.min(u32::MAX as f64) as u32), |v| u32::try_from(v).unwrap_or(u32::MAX)),
        Value::String(s) => s.trim().parse().unwrap_or(0),
        _ => 0,
    }
}

/// Bytes from a number or a display string (`"1.38 GB"`, `"700 MiB"`); decimal units are 1000-based.
fn size_of(v: &Value) -> Option<u64> {
    match v {
        Value::Number(n) => n.as_u64().or_else(|| n.as_f64().filter(|f| *f >= 0.0).map(|f| f as u64)),
        Value::String(s) => {
            let s = s.trim();
            let split = s.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ',')).unwrap_or(s.len());
            let num: f64 = s[..split].replace(',', ".").parse().ok()?;
            let mult: f64 = match s[split..].trim().to_ascii_lowercase().as_str() {
                "" | "b" => 1.0,
                "kb" => 1e3,
                "mb" => 1e6,
                "gb" => 1e9,
                "tb" => 1e12,
                "kib" => 1024.0,
                "mib" => 1024.0 * 1024.0,
                "gib" => 1024.0 * 1024.0 * 1024.0,
                "tib" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
                _ => return None,
            };
            Some((num * mult).round() as u64)
        }
        _ => None,
    }
}

/// `xt=urn:btih:<hash>` of a magnet link.
fn btih(magnet: &str) -> Option<String> {
    let lower = magnet.to_ascii_lowercase();
    let start = lower.find("xt=urn:btih:")? + "xt=urn:btih:".len();
    let rest = &magnet[start..];
    let end = rest.find('&').unwrap_or(rest.len());
    Some(rest[..end].to_owned()).filter(|h| !h.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tk_core::sources::source_key;

    #[test]
    fn widened_web_entries_and_missing_fields() {
        let body = json!({ "sources": [
            { "providerName": "toko", "server": "Alpha", "type": "hls", "url": "https://cdn.toko.example/a.m3u8", "quality": "1080p",
              "subs": ["ES", "en"], "dubs": ["ja"], "note": "multi-sub", "headers": { "Referer": "https://toko.example/" }, "latencyMs": 180 },
            // legacy entry: no server / subs / dubs / note -> provider name, subtitle langs, audioLanguage
            { "providerName": "beta", "type": "mp4", "url": "https://cdn.beta.example/b.mp4", "audioLanguage": "en",
              "subtitles": [{ "url": "https://cdn.beta.example/es.vtt", "language": "es-419", "label": "Español" }] },
            // nothing but a url and a type
            { "type": "hls", "url": "https://x.example/x.m3u8" },
            // not listable: embed page and empty url
            { "providerName": "embed", "type": "embed", "url": "https://embed.example/e" },
            { "providerName": "empty", "type": "hls", "url": "" }
        ]});
        let c = map_candidates("io.github.toko", "Toko", &body);
        assert_eq!(c.len(), 3);
        assert_eq!((c[0].label.as_str(), c[0].kind, c[0].quality), ("Alpha", SourceKind::Web, Quality::P1080));
        assert_eq!((c[0].subs.clone(), c[0].dubs.clone(), c[0].note.as_deref()), (vec!["es".to_owned(), "en".to_owned()], vec!["ja".to_owned()], Some("multi-sub")));
        assert_eq!(c[0].container.as_deref(), Some("HLS"));
        let web = c[0].web.as_ref().unwrap();
        assert_eq!((web.latency_ms, web.headers["Referer"].as_str()), (Some(180), "https://toko.example/"));
        assert_eq!(source_key(&c[0]), "io.github.toko|web|Alpha");
        assert_eq!((c[1].label.as_str(), c[1].quality, c[1].note.clone()), ("beta", Quality::Unknown, None));
        assert_eq!((c[1].subs.clone(), c[1].dubs.clone()), (vec!["es".to_owned()], vec!["en".to_owned()]));
        assert_eq!(c[1].to_stream().unwrap().subtitles.len(), 1);
        assert_eq!((c[2].label.as_str(), c[2].ext_name.as_str()), ("io.github.toko", "Toko"));
        assert!(c[2].subs.is_empty() && c[2].dubs.is_empty() && c[2].web.as_ref().unwrap().latency_ms.is_none());
        assert!(c.iter().all(|c| c.reliability == INITIAL_RELIABILITY && c.torrent.is_none()));
    }

    #[test]
    fn torrent_entries_carry_index_data() {
        let body = json!({ "sources": [
            { "providerName": "nyaa", "group": "SubsPlease", "type": "torrent", "quality": "1080p", "codec": "H.264",
              "url": "magnet:?xt=urn:btih:ABCDEF0123456789ABCDEF0123456789ABCDEF01&dn=x", "seeders": 812, "leechers": 41, "fileSize": "1.38 GB",
              "subs": ["en"], "dubs": ["ja"] },
            { "providerName": "nyaa", "sourceType": "torrent", "infoHash": "1111111111111111111111111111111111111111", "url": "https://nyaa.example/1.torrent",
              "seeders": "6", "sizeBytes": 1900000000u64, "note": "sin confirmar" },
            // no hash anywhere: skipped
            { "providerName": "nyaa", "type": "torrent", "url": "https://nyaa.example/2.torrent" }
        ]});
        let c = map_candidates("io.github.nyaa-own", "Nyaa", &body);
        assert_eq!(c.len(), 2);
        let t = c[0].torrent.as_ref().unwrap();
        assert_eq!((c[0].kind, c[0].label.as_str(), c[0].container.as_deref()), (SourceKind::Torrent, "SubsPlease", Some("H.264")));
        assert_eq!(t.info_hash, "abcdef0123456789abcdef0123456789abcdef01");
        assert_eq!((t.seeders, t.leechers, t.size_bytes), (812, 41, Some(1_380_000_000)));
        assert!(t.magnet.as_deref().is_some_and(|m| m.starts_with("magnet:")));
        assert!(c[0].web.is_none() && c[0].to_stream().is_none());
        let t = c[1].torrent.as_ref().unwrap();
        assert_eq!((c[1].label.as_str(), t.seeders, t.leechers, t.size_bytes, t.magnet.clone()), ("nyaa", 6, 0, Some(1_900_000_000), None));
        assert_eq!(c[1].note.as_deref(), Some("sin confirmar"));
    }

    #[test]
    fn repeated_labels_get_unique_keys() {
        let body = json!({ "sources": [
            { "providerName": "beta", "type": "mp4", "url": "https://b.example/720.mp4", "quality": "720p" },
            { "providerName": "beta", "type": "mp4", "url": "https://b.example/480.mp4", "quality": "480p" },
            { "providerName": "beta", "type": "mp4", "url": "https://b.example/x.mp4" },
            { "providerName": "beta", "type": "mp4", "url": "https://b.example/y.mp4" }
        ]});
        let labels: Vec<String> = map_candidates("e", "E", &body).into_iter().map(|c| c.label).collect();
        assert_eq!(labels, ["beta", "beta 480p", "beta #2", "beta #3"]);
        assert!(map_candidates("e", "E", &json!({})).is_empty());
    }

    #[test]
    fn sizes() {
        assert_eq!(size_of(&json!("700 MiB")), Some(734_003_200));
        assert_eq!(size_of(&json!("0,92 GB")), Some(920_000_000));
        assert_eq!(size_of(&json!("12 parsecs")), None);
        assert_eq!(size_of(&json!(5)), Some(5));
    }
}
