//! `metadata::anilist::details` mapper against a sample AniList `Media` answer (wave A, worker c).

use std::path::PathBuf;

use serde_json::Value;
use tk_core::MediaKind;
use tk_core::metadata::anilist::details::{format_fuzzy_date, map_media_extra, media_ids, media_titles, pick_title};

fn media() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/anilist-media-extra.json");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).unwrap();
    doc["data"]["Media"].clone()
}

#[test]
fn facts_are_mapped() {
    let x = map_media_extra(&media());
    assert_eq!(x.format.as_deref(), Some("TV"));
    assert_eq!(x.source.as_deref(), Some("MANGA"));
    assert_eq!(x.season.as_deref(), Some("FALL"));
    assert_eq!(x.season_year, Some(2023));
    assert_eq!(x.duration_min, Some(24));
    assert_eq!(format_fuzzy_date(x.start_date).as_deref(), Some("2023-09-29"));
    assert_eq!(format_fuzzy_date(x.end_date).as_deref(), Some("2024-03"));
    assert_eq!(x.studios, vec!["MADHOUSE".to_owned()]);
    assert_eq!(x.producers, vec!["Aniplex".to_owned(), "Dentsu".to_owned()]);
    assert_eq!(x.country.as_deref(), Some("JP"));
    assert_eq!((x.mean_score, x.popularity, x.favourites), (Some(90), Some(400_000), Some(50_000)));
    assert_eq!(x.synonyms, vec!["Frieren at the Funeral".to_owned()]);
    assert_eq!(x.tags, vec![("Elf".to_owned(), 96), ("Travel".to_owned(), 100)]);
    assert!(!x.adult);
    assert_eq!(
        x.trailer,
        Some(("youtube".into(), "qgQ1Ss1m0hY".into(), Some("https://i.ytimg.com/vi/qgQ1Ss1m0hY/hqdefault.jpg".into())))
    );
    assert_eq!(x.next_airing, None);
    assert_eq!(x.external_links.len(), 1);
    assert_eq!(x.external_links[0].0, "Crunchyroll");
}

#[test]
fn people_relations_and_recommendations_are_mapped() {
    let x = map_media_extra(&media());
    assert_eq!(x.relations.len(), 2);
    assert_eq!(x.relations[0].relation, "SOURCE");
    assert_eq!(x.relations[0].media.kind, MediaKind::Manga);
    assert_eq!(x.relations[1].media.kind, MediaKind::Anime);
    assert_eq!(x.characters.len(), 2);
    assert_eq!(x.characters[0].name, "Frieren");
    assert_eq!(x.characters[0].role, "MAIN");
    assert_eq!(x.characters[0].voice_actors[0].name, "Atsumi Tanezaki");
    assert_eq!(x.characters[0].voice_actors[0].role.as_deref(), Some("Japanese"));
    assert_eq!(x.characters[1].image, None);
    assert_eq!(x.staff.len(), 1);
    assert_eq!(x.staff[0].role.as_deref(), Some("Director"));
    assert_eq!(x.recommendations.len(), 1);
    assert_eq!(x.recommendations[0].media_ref.anilist_id, Some(21));
}

#[test]
fn ids_and_titles_are_read() {
    let m = media();
    let r = media_ids(&m);
    assert_eq!((r.anilist_id, r.mal_id), (Some(154_587), Some(52_991)));
    let t = media_titles(&m);
    assert_eq!(pick_title(&t, "native").as_deref(), Some("葬送のフリーレン"));
    assert_eq!(pick_title(&t, "english").as_deref(), Some("Frieren: Beyond Journey's End"));
}
