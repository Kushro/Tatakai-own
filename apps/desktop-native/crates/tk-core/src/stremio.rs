//! Stremio add-ons (wave B, worker **j**; plan 14 D4; checklist `ext.stremio`, `ext.stremio.manage`; classic
//! `core/extensions/remote/stremio.ts`, `extensions/RemoteExtensionsPanel.tsx` "Stremio addon"). UI-free.
//!
//! * [`Manifest`] (id, version, name, description, types, resources, idPrefixes) from `manifest.json`.
//! * [`targets`]: the `(type, id)` pairs an anime add-on indexes for one episode (`kitsu:<id>:<ep>`,
//!   `mal:<id>:<ep>`, `anilist:<id>:<ep>`), filtered by [`supports_resource`] (resource `stream`, the add-on's
//!   types and `idPrefixes`).
//! * [`resource_url`]: `<base>/stream/<type>/<id>.json`.
//! * [`normalize_streams`]: a `{ streams: [...] }` answer -> the route shape tk-ext maps into
//!   `crate::sources::SourceCandidate` (`tk_ext::map_candidates`): web for `url` (`type` hls / mp4 from the path,
//!   headers from `behaviorHints.proxyHeaders.request`), torrent for `infoHash` (magnet with the `tracker:` sources,
//!   release name from `title` / `description` so `crate::release` parses group and quality, seeders from the
//!   `👤 N` marker). Rows live in `store/ext.rs::stremio_addons`.

use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub id: String,
    pub version: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub types: Vec<String>,
    /// `stream`, `catalog`, `meta`, `subtitles` (objects in newer manifests: both forms are accepted).
    #[serde(default)]
    pub resources: Vec<serde_json::Value>,
    #[serde(default)]
    pub id_prefixes: Vec<String>,
}

impl Manifest {
    /// Parse and validate (`id`, `name`, `version` non-empty; at least one resource).
    pub fn parse(text: &str) -> Result<Self, String> {
        let m: Manifest = serde_json::from_str(text).map_err(|e| format!("manifest.json: {e}"))?;
        if m.id.trim().is_empty() || m.name.trim().is_empty() || m.version.trim().is_empty() {
            return Err("manifest.json: id, name and version are required".into());
        }
        if m.resources.is_empty() {
            return Err("manifest.json: the add-on declares no resources".into());
        }
        Ok(m)
    }

    /// Resource names (`stream`, `catalog`, ...), whatever the form.
    pub fn resource_names(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for r in &self.resources {
            let name = r.as_str().or_else(|| r["name"].as_str()).unwrap_or("").to_owned();
            if !name.is_empty() && !out.contains(&name) {
                out.push(name);
            }
        }
        out
    }
}

/// Base URL of an add-on from its manifest URL (strip the trailing `/manifest.json`).
pub fn base_url(manifest_url: &str) -> String {
    manifest_url.trim().trim_end_matches("/manifest.json").trim_end_matches('/').to_owned()
}

/// `stremio://host/path/manifest.json` -> `https://host/path/manifest.json`; a URL without `manifest.json` gets it.
pub fn manifest_url(input: &str) -> Option<String> {
    let s = input.trim();
    let s = if s.len() >= 10 && s[..10].eq_ignore_ascii_case("stremio://") { format!("https://{}", &s[10..]) } else { s.to_owned() };
    let url = url::Url::parse(&s).ok()?;
    if url.scheme() != "https" && url.scheme() != "http" {
        return None;
    }
    let mut out = url.to_string();
    if !url.path().ends_with("/manifest.json") {
        out = format!("{}/manifest.json", out.trim_end_matches('/'));
    }
    Some(out)
}

/// Whether the add-on serves `resource` for `(kind, id)` (classic `supportsResource`).
pub fn supports_resource(m: &Manifest, resource: &str, kind: &str, id: &str) -> bool {
    m.resources.iter().any(|entry| {
        let (name, types, prefixes) = match entry {
            Value::String(n) => (n.as_str(), None, None),
            Value::Object(o) => (
                o.get("name").and_then(Value::as_str).unwrap_or(""),
                o.get("types").and_then(Value::as_array),
                o.get("idPrefixes").and_then(Value::as_array),
            ),
            _ => ("", None, None),
        };
        let type_ok = match types {
            Some(t) => t.iter().any(|v| v.as_str() == Some(kind)),
            None => m.types.iter().any(|t| t == kind),
        };
        let prefixes: Vec<&str> = match prefixes {
            Some(p) => p.iter().filter_map(Value::as_str).collect(),
            None => m.id_prefixes.iter().map(String::as_str).collect(),
        };
        name == resource && type_ok && (prefixes.is_empty() || prefixes.iter().any(|p| id.starts_with(p)))
    })
}

/// `<base>/<resource>/<type>/<id>.json` with the type and id percent-encoded.
pub fn resource_url(manifest_url: &str, resource: &str, kind: &str, id: &str) -> String {
    let enc = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>().replace('+', "%20");
    format!("{}/{resource}/{}/{}.json", base_url(manifest_url), enc(kind), enc(id))
}

/// Catalogue ids of one title for the episode lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TitleIds {
    pub kitsu: Option<u32>,
    pub mal: Option<u32>,
    pub anilist: Option<u32>,
}

/// `(type, id)` pairs to ask for `episode`, best first; only those the add-on serves `stream` for. `remembered` (a
/// previous match, see `ext_catalog`) goes first when still supported.
pub fn targets(m: &Manifest, ids: TitleIds, episode: u32, remembered: Option<&str>) -> Vec<(String, String)> {
    let ep = episode.max(1);
    let mut ids_in_order: Vec<String> = Vec::new();
    if let Some(k) = ids.kitsu {
        ids_in_order.push(format!("kitsu:{k}:{ep}"));
    }
    if let Some(x) = ids.mal {
        ids_in_order.push(format!("mal:{x}:{ep}"));
    }
    if let Some(a) = ids.anilist {
        ids_in_order.push(format!("anilist:{a}:{ep}"));
    }
    let mut out: Vec<(String, String)> = Vec::new();
    for kind in ["anime", "series"] {
        for id in &ids_in_order {
            if supports_resource(m, "stream", kind, id) {
                out.push((kind.to_owned(), id.clone()));
            }
        }
    }
    if let Some(key) = remembered
        && let Some(pos) = out.iter().position(|(k, id)| match_key(k, id) == key)
    {
        let t = out.remove(pos);
        out.insert(0, t);
    }
    out
}

/// What a remembered title match stores: `<type>|<id prefix>` (`series|kitsu`).
pub fn match_key(kind: &str, id: &str) -> String {
    format!("{kind}|{}", id.split(':').next().unwrap_or(""))
}

fn seeders_of(text: &str) -> Option<u32> {
    let (_, rest) = text.split_once('👤')?;
    rest.trim_start().split(|c: char| !c.is_ascii_digit()).next().and_then(|n| n.parse().ok())
}

fn first_line(v: &Value) -> Option<String> {
    v.as_str().and_then(|s| s.lines().map(str::trim).find(|l| !l.is_empty())).map(str::to_owned)
}

/// `{ streams: [...] }` -> `{ sources: [...] }` in the extension-route shape (see the module doc).
pub fn normalize_streams(addon_name: &str, body: &Value) -> Value {
    let mut out: Vec<Value> = Vec::new();
    for s in body["streams"].as_array().into_iter().flatten() {
        let text = [&s["name"], &s["title"], &s["description"]].iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(" ");
        let quality = ["2160p", "1440p", "1080p", "720p", "480p", "360p"]
            .iter()
            .find(|q| text.to_ascii_lowercase().contains(*q))
            .map(|q| (*q).to_owned())
            .or_else(|| text.to_ascii_lowercase().contains("4k").then(|| "2160p".to_owned()));
        let audio = s["audioLanguage"].as_str().or_else(|| s["language"].as_str()).map(str::to_owned);
        let subtitles: Vec<Value> = s["subtitles"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|t| {
                let u = t["url"].as_str().filter(|u| u.starts_with("https://") || u.starts_with("http://"))?;
                let lang = t["lang"].as_str().unwrap_or("und");
                Some(json!({ "url": u, "language": lang, "label": lang }))
            })
            .collect();
        let hash = s["infoHash"].as_str().filter(|h| h.len() == 40 && h.bytes().all(|c| c.is_ascii_hexdigit()));
        let label = first_line(&s["name"]).unwrap_or_else(|| addon_name.to_owned());
        if let Some(h) = hash {
            let trackers: String = s["sources"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .filter_map(|v| v.strip_prefix("tracker:"))
                .map(|t| format!("&tr={}", url::form_urlencoded::byte_serialize(t.as_bytes()).collect::<String>()))
                .collect();
            let release = first_line(&s["title"]).or_else(|| first_line(&s["description"])).or_else(|| s["behaviorHints"]["filename"].as_str().map(str::to_owned));
            out.push(json!({
                "type": "torrent",
                "url": format!("magnet:?xt=urn:btih:{}{trackers}", h.to_ascii_lowercase()),
                "infoHash": h.to_ascii_lowercase(),
                "title": release,
                "quality": quality,
                "seeders": seeders_of(&text).unwrap_or(0),
                "providerName": addon_name,
                "audioLanguage": audio,
                "note": s["fileIdx"].as_u64().map(|i| format!("file #{i}")),
            }));
            continue;
        }
        let Some(url) = s["url"].as_str().map(str::trim).filter(|u| u.starts_with("https://") || u.starts_with("http://")) else { continue };
        let path = url.split(['?', '#']).next().unwrap_or(url).to_ascii_lowercase();
        let ty = if path.ends_with(".m3u8") { "hls" } else if path.ends_with(".mpd") { "dash" } else { "mp4" };
        out.push(json!({
            "type": ty,
            "url": url,
            "quality": quality,
            "headers": s["behaviorHints"]["proxyHeaders"]["request"].as_object().cloned().unwrap_or_default(),
            "subtitles": subtitles,
            "server": label,
            "providerName": addon_name,
            "audioLanguage": audio,
        }));
    }
    json!({ "sources": out })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(resources: Value, types: &[&str], prefixes: &[&str]) -> Manifest {
        Manifest {
            id: "org.test".into(),
            version: "1.0.0".into(),
            name: "Test".into(),
            description: None,
            types: types.iter().map(|s| (*s).to_owned()).collect(),
            resources: resources.as_array().cloned().unwrap_or_default(),
            id_prefixes: prefixes.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    #[test]
    fn base_url_strips_manifest() {
        assert_eq!(super::base_url("https://a.b/c/manifest.json"), "https://a.b/c");
        assert_eq!(super::base_url("https://a.b/"), "https://a.b");
    }

    #[test]
    fn manifest_urls_are_normalized() {
        assert_eq!(manifest_url("stremio://addon.example/cfg/manifest.json").as_deref(), Some("https://addon.example/cfg/manifest.json"));
        assert_eq!(manifest_url("https://addon.example/cfg").as_deref(), Some("https://addon.example/cfg/manifest.json"));
        assert!(manifest_url("ftp://x/manifest.json").is_none());
        assert!(Manifest::parse(r#"{"id":"a","name":"A","version":"1","resources":["stream"]}"#).is_ok());
        assert!(Manifest::parse(r#"{"id":"a","name":"A","version":"1","resources":[]}"#).is_err());
    }

    #[test]
    fn resources_and_targets() {
        let m = manifest(json!(["stream"]), &["series", "anime"], &["kitsu", "tt"]);
        assert!(supports_resource(&m, "stream", "series", "kitsu:1:2"));
        assert!(!supports_resource(&m, "stream", "series", "mal:1:2"));
        assert!(!supports_resource(&m, "meta", "series", "kitsu:1:2"));
        let ids = TitleIds { kitsu: Some(7), mal: Some(9), anilist: Some(21) };
        assert_eq!(targets(&m, ids, 3, None), vec![("anime".into(), "kitsu:7:3".into()), ("series".into(), "kitsu:7:3".into())]);
        let obj = manifest(json!([{ "name": "stream", "types": ["anime"], "idPrefixes": ["mal:"] }]), &["anime"], &[]);
        assert_eq!(targets(&obj, ids, 1, None), vec![("anime".into(), "mal:9:1".into())]);
        let any = manifest(json!(["stream"]), &["series"], &[]);
        let t = targets(&any, ids, 1, Some("series|mal"));
        assert_eq!(t[0].1, "mal:9:1", "the remembered match goes first");
        assert_eq!(resource_url("https://a.b/x/manifest.json", "stream", "series", "kitsu:7:3"), "https://a.b/x/stream/series/kitsu%3A7%3A3.json");
    }

    #[test]
    fn streams_are_normalized() {
        let body = json!({ "streams": [
            { "name": "Torrentio\n1080p", "title": "[SubsPlease] Show - 03 (1080p).mkv\n👤 42 💾 1.3 GB", "infoHash": "0123456789ABCDEF0123456789abcdef01234567", "fileIdx": 2, "sources": ["tracker:udp://t.example:80", "dht:x"] },
            { "name": "Direct", "url": "https://cdn.example/v/index.m3u8?t=1", "behaviorHints": { "proxyHeaders": { "request": { "Referer": "https://x" } } } },
            { "name": "bad", "url": "magnet:?xt=urn:btih:zz" },
        ]});
        let out = normalize_streams("Torrentio", &body);
        let list = out["sources"].as_array().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0]["type"], "torrent");
        assert_eq!(list[0]["seeders"], 42);
        assert_eq!(list[0]["quality"], "1080p");
        assert!(list[0]["url"].as_str().unwrap().contains("&tr=udp%3A%2F%2Ft.example%3A80"));
        assert_eq!(list[1]["type"], "hls");
        assert_eq!(list[1]["headers"]["Referer"], "https://x");
        assert_eq!(list[1]["server"], "Direct");
    }
}
