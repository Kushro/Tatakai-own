//! `sources` / `stream` / `torrent` route bodies -> [`SourceCandidate`]s for the source selector
//! (contracts/extension-host/routes.v1.json `nativeMapping.SourceCandidate`, docs/redesign/08 section 3).
//!
//! Every selector field is optional in the extension output: a missing `server` falls back to the provider name,
//! missing `quality` is `Unknown`, missing `subs` / `dubs` come from the subtitle tracks / `audioLanguage`, and a
//! missing `note` is `None`. Nothing here is ever an error; an entry that cannot be listed at all (embed page,
//! torrent without a hash) is skipped.
//!
//! Torrent entries (wave 3, docs/redesign/10 section 6) also accept the field names Toko's torrent providers emit
//! (`torrentTitle`, `magnetLink`, `releaseGroup`, `fileSize`, `isBest`): the release name is parsed with
//! `tk_core::release::parse_release` into `SourceCandidate::tags`; `seadex_best` comes from `seadexBest` / `isBest`
//! / `best`, or, when the entry says nothing, from the provider being SeaDex (Toko drops `isBest` and SeaDex only
//! lists recommended releases). Base32 info-hashes (32 chars) are converted to the 40-char hex form.

use serde_json::Value;
use tk_core::release::{ReleaseTags, parse_release};
use tk_core::sources::{INITIAL_RELIABILITY, Quality, SourceCandidate, SourceKind, TorrentInfo, WebInfo, container_label, lang_codes};

use crate::host::{headers_of, subtitles_of, web_container};
use crate::manifest::Manifest;

/// Routes [`crate::ExtensionHost::resolve_candidates`] asks one enabled, runnable extension (docs/redesign/10
/// section 6), in order, each only if `Manifest::gated_routes` allows it: `torrent` for `torrent` extensions; `stream`
/// for `custom` / `onlinestream` ones, plus `torrent` when their manifest lists it explicitly in `apiServer.routes`
/// (the default route list does not count) or their bundle exposes `torrentAll` (`exposes_torrent_all`). Toko lists
/// it, so its torrent providers (Nyaa, SubsPlease, AnimeTosho, SeaDex, nekoBT, AniLiberty) reach the picker.
pub fn candidate_routes(m: &Manifest, exposes_torrent_all: bool) -> Vec<&'static str> {
    let gated = m.gated_routes();
    let allowed = |route: &str| gated.iter().any(|r| r == route);
    if m.kind == "torrent" {
        return if allowed("torrent") { vec!["torrent"] } else { Vec::new() };
    }
    // only an explicit `apiServer.routes` list counts: the default list names `torrent` for every extension
    let lists_torrent = m.api_server.as_ref().and_then(|a| a.routes.as_ref()).is_some_and(|r| r.iter().any(|x| x == "torrent"));
    let mut out = Vec::new();
    if allowed("stream") {
        out.push("stream");
    }
    if (lists_torrent || exposes_torrent_all) && allowed("torrent") {
        out.push("torrent");
    }
    out
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
        let is_magnet = |u: &String| u.to_ascii_lowercase().starts_with("magnet:");
        let magnet = Some(url.to_owned())
            .filter(is_magnet)
            .or_else(|| text(&s["magnetLink"]).filter(is_magnet))
            .or_else(|| text(&s["magnet"]).filter(is_magnet));
        let info_hash = text(&s["infoHash"])
            .or_else(|| text(&s["hash"]))
            .or_else(|| magnet.as_deref().and_then(btih))
            .and_then(|h| hex_info_hash(&h))?;
        // The first name that says something (a group or a resolution) wins: SubsPlease's `torrentTitle` is the bare
        // "Title - 06" while its magnet `dn` is the full release name with group and resolution.
        let names: Vec<String> = [text(&s["torrentTitle"]), text(&s["title"]), text(&s["name"]), text(&s["releaseName"]), text(&s["fileName"])]
            .into_iter()
            .flatten()
            .chain(magnet.as_deref().and_then(magnet_name))
            .collect();
        let mut tags = names
            .iter()
            .map(|n| parse_release(n))
            .find(|t| t.group.is_some() || t.resolution != Quality::Unknown)
            .or_else(|| names.first().map(|n| parse_release(n)))
            .unwrap_or_default();
        if tags.resolution == Quality::Unknown {
            tags.resolution = quality;
        }
        if s["dualAudio"].as_bool() == Some(true) {
            tags.dual_audio = true;
        }
        if s["isBatch"].as_bool() == Some(true) || s["batch"].as_bool() == Some(true) {
            tags.batch = true;
        }
        let explicit_best = ["seadexBest", "isBest", "best"].iter().find_map(|k| s[*k].as_bool());
        let from_seadex =
            [&s["providerName"], &s["source"], &s["providerKey"]].iter().any(|v| v.as_str().is_some_and(|p| p.eq_ignore_ascii_case("seadex")));
        tags.seadex_best = explicit_best.unwrap_or(from_seadex);
        let quality = if quality == Quality::Unknown { tags.resolution } else { quality };
        let label = text(&s["group"])
            .or_else(|| text(&s["releaseGroup"]))
            .or_else(|| tags.group.clone())
            .or_else(|| text(&s["server"]))
            .unwrap_or(provider);
        let torrent = TorrentInfo {
            info_hash,
            size_bytes: size_of(&s["sizeBytes"])
                .or_else(|| size_of(&s["fileSize"]))
                .or_else(|| size_of(&s["size"]))
                .or_else(|| magnet.as_deref().and_then(magnet_length)),
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
            tags,
        });
    }
    if url.is_empty() {
        return None;
    }
    // Wave B (worker k, plan 14 section 0.1 #13): an embed page is listed as a web candidate flagged `embed`; the app
    // sniffs its media URL in a hidden web view before playing (`tk-app/src/live/sniffer.rs`).
    let kind_text = s["type"].as_str().or_else(|| s["sourceType"].as_str()).unwrap_or("");
    let embed = kind_text.eq_ignore_ascii_case("embed") && url.starts_with("https://");
    let container = if embed { None } else { Some(web_container(s, url)?) };
    let label = text(&s["server"]).unwrap_or(provider);
    Some(SourceCandidate {
        ext_id: ext_id.to_owned(),
        ext_name: ext_name.to_owned(),
        kind: SourceKind::Web,
        label,
        quality,
        container: container.map(|c| container_label(c).to_owned()).or_else(|| embed.then(|| "Embed".to_owned())),
        subs,
        dubs,
        reliability: INITIAL_RELIABILITY,
        note,
        web: Some(WebInfo {
            url: url.to_owned(),
            headers: headers_of(s),
            latency_ms: s["latencyMs"].as_u64().map(|v| u32::try_from(v).unwrap_or(u32::MAX)),
            subtitles,
            embed,
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

/// `dn=<name>` of a magnet link, percent-decoded (`+` is a space).
/// `xl=` (exact length in bytes) of a magnet link.
fn magnet_length(magnet: &str) -> Option<u64> {
    let query = magnet.split_once('?')?.1;
    query.split('&').find_map(|kv| kv.strip_prefix("xl=")).and_then(|v| v.parse().ok()).filter(|n| *n > 0)
}

fn magnet_name(magnet: &str) -> Option<String> {
    let query = magnet.split_once('?')?.1;
    let raw = query.split('&').find_map(|kv| kv.strip_prefix("dn="))?;
    let bytes = raw.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => match std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|x| u8::from_str_radix(x, 16).ok()) {
                Some(b) => {
                    out.push(b);
                    i += 2;
                }
                None => out.push(b'%'),
            },
            b => out.push(b),
        }
        i += 1;
    }
    Some(String::from_utf8_lossy(&out).trim().to_owned()).filter(|n| !n.is_empty())
}

/// A v1 info-hash as 40 lowercase hex chars: hex is kept, base32 (32 chars, RFC 4648) is converted; anything else is
/// `None` (the entry is skipped).
fn hex_info_hash(h: &str) -> Option<String> {
    let h = h.trim();
    if h.len() == 40 && h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Some(h.to_ascii_lowercase());
    }
    if h.len() != 32 {
        return None;
    }
    let mut bits: u64 = 0;
    let mut nbits = 0u32;
    let mut out = String::with_capacity(40);
    for c in h.bytes() {
        let v = match c.to_ascii_uppercase() {
            c @ b'A'..=b'Z' => c - b'A',
            c @ b'2'..=b'7' => c - b'2' + 26,
            _ => return None,
        };
        bits = (bits << 5) | u64::from(v);
        nbits += 5;
        while nbits >= 8 {
            nbits -= 8;
            out.push_str(&format!("{:02x}", (bits >> nbits) & 0xff));
        }
        bits &= (1u64 << nbits) - 1;
    }
    Some(out).filter(|o| o.len() == 40)
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
            // wave B: an https embed page is listed, flagged `embed`; an empty url is not listable
            { "providerName": "embed", "type": "embed", "url": "https://embed.example/e" },
            { "providerName": "empty", "type": "hls", "url": "" }
        ]});
        let c = map_candidates("io.github.toko", "Toko", &body);
        assert_eq!(c.len(), 4);
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
        assert!(c[..3].iter().all(|c| !c.web.as_ref().unwrap().embed));
        assert_eq!((c[3].label.as_str(), c[3].container.as_deref()), ("embed", Some("Embed")));
        assert!(c[3].web.as_ref().unwrap().embed);
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

    /// The shape Toko's torrent route returns (host.js `normalize` over `torrentAll` chunks of the nyaa / seadex /
    /// animetosho providers): no `group` / `infoHash`, but `torrentTitle`, `magnetLink`, `releaseGroup`, `fileSize`.
    #[test]
    fn toko_torrent_entries_get_release_tags() {
        let body = json!({ "sources": [
            { "source": "nyaa", "providerName": "nyaa", "providerKey": "nyaa", "server": "", "type": "torrent", "sourceType": "torrent",
              "isTorrent": true, "quality": "1080p", "audioLanguage": "ja", "headers": {}, "subtitles": [],
              "url": "magnet:?xt=urn:btih:ABCDEF0123456789ABCDEF0123456789ABCDEF01&dn=%5BSubsPlease%5D%20Show%20-%2005%20(1080p)",
              "magnetLink": "magnet:?xt=urn:btih:ABCDEF0123456789ABCDEF0123456789ABCDEF01&dn=%5BSubsPlease%5D%20Show%20-%2005%20(1080p)",
              "torrentTitle": "[SubsPlease] Show - 05 (1080p) [A1B2C3D4].mkv", "releaseGroup": "SubsPlease",
              "seeders": 812, "leechers": 41, "peers": 853, "fileSize": "1.4 GiB", "matchScore": 90, "fileFormat": "mkv" },
            // .torrent url, magnet only in magnetLink, base32 hash, no releaseGroup: label = parsed group
            { "source": "animetosho", "providerName": "animetosho", "server": "", "type": "torrent", "sourceType": "torrent", "quality": "auto",
              "url": "https://animetosho.example/1.torrent",
              "magnetLink": "magnet:?xt=urn:btih:VPG6AARWIEJXHW2N3YAEIQQZRSRGQEAB&dn=x",
              "torrentTitle": "[Judas] Show S01 (BD 1080p HEVC x265 10bit Dual-Audio Multi-Subs) [Batch]", "seeders": 30 },
            // SeaDex: `isBest` dropped by the provider -> best from the provider name; explicit false wins
            { "source": "seadex", "providerName": "seadex", "server": "", "type": "torrent", "sourceType": "torrent", "quality": "1080p",
              "url": "magnet:?xt=urn:btih:2222222222222222222222222222222222222222&dn=a",
              "torrentTitle": "[Okay-Subs] Show - S01E01 (BD 1080p) [ABCD].mkv", "releaseGroup": "Okay-Subs", "seeders": 5 },
            { "source": "seadex", "providerName": "seadex", "server": "", "type": "torrent", "sourceType": "torrent", "isBest": false,
              "url": "magnet:?xt=urn:btih:3333333333333333333333333333333333333333&dn=b", "torrentTitle": "[Alt] Show (WEB 720p)" },
            // nekoBT-style explicit flag on another provider
            { "providerName": "nekobt", "type": "torrent", "seadexBest": true, "infoHash": "4444444444444444444444444444444444444444",
              "url": "https://nekobt.example/t/4", "torrentTitle": "[Group] Show - 01 [WEB 1080p AV1]" }
        ]});
        let c = map_candidates("tatakai.extension.toko", "Toko", &body);
        assert_eq!(c.len(), 5);
        assert!(c.iter().all(|c| c.kind == SourceKind::Torrent && c.web.is_none()));
        // nyaa
        let t = c[0].torrent.as_ref().unwrap();
        assert_eq!((c[0].label.as_str(), c[0].quality), ("SubsPlease", Quality::P1080));
        assert_eq!(t.info_hash, "abcdef0123456789abcdef0123456789abcdef01");
        assert_eq!((t.seeders, t.leechers, t.size_bytes), (812, 41, Some(1_503_238_554)));
        assert_eq!(c[0].tags.episodes, Some((5, 5)));
        assert_eq!(c[0].tags.group.as_deref(), Some("SubsPlease"));
        assert!(!c[0].tags.batch && !c[0].tags.seadex_best);
        // animetosho batch
        let t = c[1].torrent.as_ref().unwrap();
        assert_eq!(c[1].label, "Judas");
        assert_eq!(t.info_hash.len(), 40);
        assert_eq!(t.info_hash, "abcde00236411373db4dde004442198ca2681001");
        assert!(t.magnet.as_deref().is_some_and(|m| m.starts_with("magnet:")));
        assert_eq!(c[1].quality, Quality::P1080); // "auto" from the provider, 1080p from the name
        let g = &c[1].tags;
        assert!(g.batch && g.dual_audio && g.multi_sub && g.bit_depth == Some(10) && g.season == Some(1));
        assert_eq!((g.source, g.codec), (Some(tk_core::release::ReleaseSource::Bd), Some(tk_core::release::VideoCodec::Hevc)));
        // seadex
        assert!(c[2].tags.seadex_best && !c[3].tags.seadex_best);
        assert_eq!(c[2].label, "Okay-Subs");
        assert_eq!(c[3].label, "Alt");
        assert_eq!(c[3].quality, Quality::P720);
        assert!(c[4].tags.seadex_best);
        assert_eq!(c[4].tags.codec, Some(tk_core::release::VideoCodec::Av1));
    }

    #[test]
    fn hashes_and_magnet_names() {
        assert_eq!(hex_info_hash("ABCDEF0123456789ABCDEF0123456789ABCDEF01").as_deref(), Some("abcdef0123456789abcdef0123456789abcdef01"));
        assert_eq!(hex_info_hash("VPG6AARWIEJXHW2N3YAEIQQZRSRGQEAB").as_deref(), Some("abcde00236411373db4dde004442198ca2681001"));
        assert_eq!(hex_info_hash("xyz"), None);
        assert_eq!(hex_info_hash(&"1".repeat(32)), None); // '1' is not base32
        assert_eq!(magnet_name("magnet:?xt=urn:btih:aa&dn=%5BG%5D+Show+-+01&tr=x").as_deref(), Some("[G] Show - 01"));
        assert_eq!(magnet_name("magnet:?xt=urn:btih:aa"), None);
        assert_eq!(magnet_name("magnet:?dn=50%").as_deref(), Some("50%"));
    }

    #[test]
    fn custom_extensions_with_torrent_providers_ask_both_routes() {
        let base = r#"{"id":"x.toko","name":"Toko","version":"1.0.0","type":"custom","description":"d","permissions":[]}"#;
        let m = Manifest::parse(base).unwrap();
        // default route list: stream only, unless the bundle exposes torrentAll
        assert_eq!(candidate_routes(&m, false), ["stream"]);
        assert_eq!(candidate_routes(&m, true), ["stream", "torrent"]);
        // Toko lists torrent explicitly
        let toko = base.replace(r#""permissions":[]"#, r#""permissions":[],"apiServer":{"routes":["sources","stream","torrent"]}"#);
        assert_eq!(candidate_routes(&Manifest::parse(&toko).unwrap(), false), ["stream", "torrent"]);
        // an explicit list without torrent wins over torrentAll
        let no_torrent = base.replace(r#""permissions":[]"#, r#""permissions":[],"apiServer":{"routes":["sources","stream"]}"#);
        assert_eq!(candidate_routes(&Manifest::parse(&no_torrent).unwrap(), true), ["stream"]);
        // onlinestream behaves the same; torrent extensions only ask torrent
        let online = toko.replace(r#""type":"custom""#, r#""type":"onlinestream""#);
        assert_eq!(candidate_routes(&Manifest::parse(&online).unwrap(), false), ["stream", "torrent"]);
        let torrent = base.replace(r#""type":"custom""#, r#""type":"torrent""#);
        assert_eq!(candidate_routes(&Manifest::parse(&torrent).unwrap(), true), ["torrent"]);
        let gated = torrent.replace(r#""permissions":[]"#, r#""permissions":[],"apiServer":{"routes":["sources"]}"#);
        assert!(candidate_routes(&Manifest::parse(&gated).unwrap(), false).is_empty());
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

    #[test]
    fn a_bare_torrent_title_falls_back_to_the_magnet_release_name_and_length() {
        // SubsPlease through Toko: bare torrentTitle, the group / resolution / size live in the magnet
        let body = json!({ "sources": [{
            "source": "subplease", "sourceType": "torrent", "quality": "unknown", "torrentTitle": "Magical Explorer - 06",
            "url": "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=%5BSubsPlease%5D%20Magical%20Explorer%20-%2006%20%281080p%29%20%5BF86EEED0%5D.mkv&xl=1395864371"
        }] });
        let c = map_candidates("toko", "Toko", &body);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].label, "SubsPlease");
        assert_eq!(c[0].quality, Quality::P1080);
        assert_eq!(c[0].torrent.as_ref().unwrap().size_bytes, Some(1_395_864_371));
    }
}