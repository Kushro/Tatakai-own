//! Ranks the 11 example sources of the prototype (`contracts/fixtures/sources/sources-for.json`, from
//! `data.js::sourcesFor` with seed 0) and checks the order the prototype `sources.js::score()` produces.
//! Expected orders were computed by hand from that formula; the scores are in the comments.

use std::path::PathBuf;

use serde_json::Value;
use tk_core::sources::{Quality, SourceCandidate, SourceKind, SourceMode, SourcePrefs, rank, score};

fn candidates() -> Vec<SourceCandidate> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../../contracts/fixtures/sources/sources-for.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let v: Value = serde_json::from_str(&text).unwrap();
    let list: Vec<SourceCandidate> = serde_json::from_value(v["candidates"].clone()).unwrap();
    assert_eq!(list.len(), 11);
    list
}

fn order(prefs: &SourcePrefs, enabled: impl Fn(&str) -> bool) -> Vec<String> {
    rank(candidates(), prefs, enabled).into_iter().map(|(c, _)| c.label).collect()
}

fn all_on(_: &str) -> bool {
    true
}

// default prefs (any kind, >= 720p, sub es, dub ja):
// Erai-raws 91.6, Varyg 84.56, SubsPlease 83.5, Alpha 80.7, Beta 76.9, Judas 69.94, Streamwish 69.9,
// Zulu 66.6, Ok.ru 53.8, Anon-uploader 21.62, Fembed 8.5
const DEFAULT_ORDER: [&str; 11] =
    ["Erai-raws", "Varyg", "SubsPlease", "Alpha", "Beta", "Judas", "Streamwish", "Zulu", "Ok.ru", "Anon-uploader", "Fembed"];

#[test]
fn fixture_matches_the_prototype_shape() {
    let list = candidates();
    assert_eq!(list.iter().filter(|c| c.kind == SourceKind::Web).count(), 6);
    for c in &list {
        match c.kind {
            SourceKind::Web => assert!(c.web.is_some() && c.torrent.is_none(), "{}", c.label),
            SourceKind::Torrent => assert!(c.torrent.is_some() && c.web.is_none(), "{}", c.label),
        }
    }
    assert_eq!(list[4].quality, Quality::P480);
    assert_eq!(list[9].container.as_deref(), Some("HEVC 10-bit"));
}

#[test]
fn default_prefs_order_and_scores() {
    let prefs = SourcePrefs::default();
    assert_eq!(order(&prefs, all_on), DEFAULT_ORDER);
    let ranked = rank(candidates(), &prefs, all_on);
    let by = |label: &str| ranked.iter().find(|(c, _)| c.label == label).map(|(_, s)| *s).unwrap();
    for (label, want) in [
        ("Erai-raws", 91.6),
        ("Varyg", 84.56),
        ("SubsPlease", 83.5),
        ("Alpha", 80.7),
        ("Judas", 69.94),
        ("Streamwish", 69.9),
        ("Anon-uploader", 21.62),
        ("Fembed", 8.5),
    ] {
        assert!((by(label) - want).abs() < 1e-3, "{label}: {} != {want}", by(label));
    }
}

#[test]
fn manual_mode_does_not_change_the_ranking() {
    let prefs = SourcePrefs { mode: SourceMode::Manual, ..SourcePrefs::default() };
    assert_eq!(order(&prefs, all_on), DEFAULT_ORDER);
}

#[test]
fn prefer_torrent() {
    // torrents +20, web -20: Erai-raws 111.6, Varyg 104.56, SubsPlease 103.5, Judas 89.94, Alpha 60.7, Beta 56.9,
    // Streamwish 49.9, Zulu 46.6, Anon-uploader 41.62, Ok.ru 33.8, Fembed -11.5
    let prefs = SourcePrefs { prefer: Some(SourceKind::Torrent), ..SourcePrefs::default() };
    assert_eq!(
        order(&prefs, all_on),
        ["Erai-raws", "Varyg", "SubsPlease", "Judas", "Alpha", "Beta", "Streamwish", "Zulu", "Anon-uploader", "Ok.ru", "Fembed"]
    );
}

#[test]
fn prefer_web() {
    // web +20, torrents -20: Alpha 100.7, Beta 96.9, Streamwish 89.9, Zulu 86.6, Ok.ru 73.8, Erai-raws 71.6,
    // Varyg 64.56, SubsPlease 63.5, Judas 49.94, Fembed 28.5, Anon-uploader 1.62
    let prefs = SourcePrefs { prefer: Some(SourceKind::Web), ..SourcePrefs::default() };
    assert_eq!(
        order(&prefs, all_on),
        ["Alpha", "Beta", "Streamwish", "Zulu", "Ok.ru", "Erai-raws", "Varyg", "SubsPlease", "Judas", "Fembed", "Anon-uploader"]
    );
}

#[test]
fn disabled_extensions_sink_to_the_bottom() {
    let prefs = SourcePrefs::default();
    // Nyaa off: every torrent gets -1000 and keeps its relative order below the web sources
    assert_eq!(
        order(&prefs, |id| id != "io.github.nyaa-own"),
        ["Alpha", "Beta", "Streamwish", "Zulu", "Ok.ru", "Fembed", "Erai-raws", "Varyg", "SubsPlease", "Judas", "Anon-uploader"]
    );
    // Toko off: Alpha -919.3, Beta -923.1, Zulu -933.4 go last, even below Fembed (8.5)
    assert_eq!(
        order(&prefs, |id| id != "io.github.toko"),
        ["Erai-raws", "Varyg", "SubsPlease", "Judas", "Streamwish", "Ok.ru", "Anon-uploader", "Fembed", "Alpha", "Beta", "Zulu"]
    );
    let alpha = candidates().into_iter().find(|c| c.label == "Alpha").unwrap();
    assert!((score(&alpha, &prefs, false) - (80.7 - 1000.0)).abs() < 1e-2);
}
