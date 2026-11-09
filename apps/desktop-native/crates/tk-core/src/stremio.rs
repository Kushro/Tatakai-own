//! Stremio add-ons (wave B, worker **j**; plan 14 D4; checklist `ext.stremio`, `ext.stremio.manage`; classic
//! `extensions/RemoteExtensionsPanel.tsx` "Stremio addon"). UI-free. j implements: `Manifest` (id, version, name,
//! description, types, resources, idPrefixes) from `manifest.json`; `streams(manifest_url, type, id) ->
//! Vec<StremioStream>` from `<base>/stream/<type>/<id>.json` where `id` is `kitsu:<id>:<ep>` or `mal:<id>:<ep>` (what
//! anime add-ons index); mapping to `crate::sources::SourceCandidate` (web for `url`, torrent for `infoHash` + optional
//! `fileIdx`, `name` / `title` -> label + quality through `crate::release`). Rows in `store/ext.rs::stremio_addons`.

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
    /// `stream`, `catalog`, `meta`, `subtitles` (objects in newer manifests: j accepts both forms).
    #[serde(default)]
    pub resources: Vec<serde_json::Value>,
    #[serde(default)]
    pub id_prefixes: Vec<String>,
}

/// Base URL of an add-on from its manifest URL (strip the trailing `/manifest.json`).
pub fn base_url(manifest_url: &str) -> String {
    manifest_url.trim().trim_end_matches("/manifest.json").trim_end_matches('/').to_owned()
}

#[cfg(test)]
mod tests {
    #[test]
    fn base_url_strips_manifest() {
        assert_eq!(super::base_url("https://a.b/c/manifest.json"), "https://a.b/c");
        assert_eq!(super::base_url("https://a.b/"), "https://a.b");
    }
}
