//! `Backend` details extras (wave A, worker **c**; docs/redesign/15 section 4).
//!
//! `details_extra` asks AniList directly (`tk_core::metadata::anilist::details`; the router has no generic call
//! for it) with its own `reqwest::Client`, caches the raw `Media` object in `b.store` for the metadata cache lifetime set in Settings > Data and maps it to
//! `DetailsExtraVm`. `watch_order` walks the PREQUEL / SEQUEL chain (at most `MAX_CHAIN_HOPS` each way) with the
//! light chain query, each node cached the same way, and lays it out with `watch_order_steps` (side stories / recaps after
//! their parent). Kitsu-only ids have no AniList data: both calls answer empty.

use std::collections::HashSet;
use std::sync::OnceLock;
use std::time::Duration;

use serde_json::Value;
use tk_core::metadata::AniList;
use tk_core::metadata::anilist::details::{
    ChainNode, MAX_CHAIN_HOPS, MediaExtra, chain_next, format_fuzzy_date, map_chain_node, map_media_extra, media_ids,
    media_titles, pick_title, watch_order_steps,
};
use tk_core::metadata::anilist::search::Person;
use tk_core::{Media, MediaKind, MediaRef};
use tk_ui::backend::{
    AiringVm, CharacterRoleVm, DetailsExtraVm, MediaFactsVm, PersonVm, RelatedVm, TrailerVm, UiResult,
    WatchOrderEntryVm,
};
use tk_ui::model::{ErrorKind, UiError};

use super::{LiveBackend, media_ref, provider_err, to_vm};

fn anilist() -> &'static AniList {
    static CLIENT: OnceLock<AniList> = OnceLock::new();
    CLIENT.get_or_init(|| {
        let client = reqwest::Client::builder()
            .user_agent("TatakaiNext/0.1")
            .timeout(Duration::from_secs(20))
            .build()
            .unwrap_or_default();
        AniList::new(client)
    })
}

/// Stable cache key part of a ref: `a<id>` or `m<id>`.
fn ref_key(r: &MediaRef) -> Option<String> {
    match (r.anilist_id, r.mal_id) {
        (Some(a), _) => Some(format!("a{a}")),
        (None, Some(m)) => Some(format!("m{m}")),
        _ => None,
    }
}

/// `ext:<extension id>:<key>`: a title found inside an extension without AniList / MAL mapping.
fn is_extension_id(id: &str) -> bool {
    id.starts_with("ext:")
}

fn not_found(id: &str) -> UiError {
    UiError::new(ErrorKind::Other, format!("{id} was not found on AniList"))
}

fn person_vm(p: &Person) -> PersonVm {
    PersonVm { id: p.id.to_string(), name: p.name.clone(), image: p.image.clone(), role: p.role.clone() }
}

/// Title of a card in the preferred language (`english` | `romaji` | `native`; checklist
/// `details.title-language`), falling back english -> romaji -> native -> the mapped title. tk-app's card mapper
/// (`live/mod.rs::to_vm`) calls it once `Settings` carries the preference (integration note of worker c).
pub fn display_title(m: &Media, preferred: &str) -> String {
    pick_title(&m.titles, preferred).unwrap_or_else(|| m.title.clone())
}

/// Alternative titles: the two titles not shown (english-first, as the card mapper picks) + AniList synonyms,
/// without duplicates.
fn alternative_titles(raw: &Value, synonyms: &[String]) -> Vec<String> {
    let titles = media_titles(raw);
    let shown = pick_title(&titles, "english");
    let mut out: Vec<String> = Vec::new();
    for t in ["romaji", "english", "native"].iter().filter_map(|k| titles.get(*k)).chain(synonyms.iter()) {
        if Some(t) != shown.as_ref() && !out.contains(t) {
            out.push(t.clone());
        }
    }
    out
}

/// `MediaExtra` + ids of the raw object -> view model.
pub fn extra_vm(raw: &Value, x: MediaExtra, kitsu_id: Option<u32>) -> DetailsExtraVm {
    let ids = media_ids(raw);
    let synonyms = alternative_titles(raw, &x.synonyms);
    let facts = MediaFactsVm {
        format: x.format,
        source: x.source,
        season: x.season.map(|s| s.to_ascii_lowercase()),
        season_year: x.season_year,
        duration_min: x.duration_min,
        start_date: format_fuzzy_date(x.start_date),
        end_date: format_fuzzy_date(x.end_date),
        studios: x.studios,
        producers: x.producers,
        country: x.country,
        mean_score: x.mean_score,
        popularity: x.popularity,
        favourites: x.favourites,
        synonyms,
        tags: x.tags,
        anilist_id: ids.anilist_id,
        mal_id: ids.mal_id,
        kitsu_id,
        adult: x.adult,
        facts_sources: Vec::new(),
        rank: None,
        members: None,
        screenshots: Vec::new(),
    };
    DetailsExtraVm {
        facts,
        relations: x
            .relations
            .iter()
            .map(|r| RelatedVm {
                media: to_vm(&r.media),
                relation: r.relation.clone(),
                kind: if r.media.kind == MediaKind::Manga { "manga" } else { "anime" }.to_owned(),
            })
            .collect(),
        characters: x
            .characters
            .iter()
            .map(|c| CharacterRoleVm {
                id: c.id.to_string(),
                name: c.name.clone(),
                image: c.image.clone(),
                role: c.role.clone(),
                voice_actors: c.voice_actors.iter().map(person_vm).collect(),
            })
            .collect(),
        staff: x.staff.iter().map(person_vm).collect(),
        recommendations: x.recommendations.iter().map(to_vm).collect(),
        trailer: x.trailer.map(|(site, id, thumbnail)| TrailerVm { site, id, thumbnail }),
        next_airing: x.next_airing.map(|(episode, at_unix)| AiringVm { episode, at_unix }),
        external_links: x.external_links,
    }
}

/// Raw object through the metadata cache (lifetime from Settings > Data, default 24 h); `fetch` runs on a miss. Cache failures only log.
async fn cached<F, Fut>(b: &LiveBackend, key: &str, fetch: F) -> UiResult<Option<Value>>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<Option<Value>, tk_core::ProviderError>>,
{
    match b.store.cache_get::<Value>(key) {
        Ok(Some(v)) => return Ok(Some(v)),
        Ok(None) => {}
        Err(e) => tracing::warn!(%e, key, "reading the AniList cache failed"),
    }
    let fresh = fetch().await.map_err(provider_err)?;
    if let Some(v) = &fresh
        && let Err(e) = b.store.cache_put(key, v, super::system::metadata_ttl_secs(b))
    {
        tracing::warn!(%e, key, "writing the AniList cache failed");
    }
    Ok(fresh)
}

/// `Backend::details_extra`.
pub async fn details_extra(b: &LiveBackend, id: &str) -> UiResult<DetailsExtraVm> {
    if is_extension_id(id) {
        // Extension-only title (`ext:<id>:<key>`, Search "in extension"): AniList knows nothing about it.
        return Ok(DetailsExtraVm::default());
    }
    let r = media_ref(id).ok_or_else(|| UiError::new(ErrorKind::Other, format!("invalid id {id}")))?;
    let Some(key) = ref_key(&r) else {
        // Kitsu-only title: AniList cannot be asked by that id.
        let mut vm = DetailsExtraVm::default();
        vm.facts.kitsu_id = r.kitsu_id;
        return Ok(vm);
    };
    let raw = cached(b, &format!("anilist.extra.{key}"), || anilist().media_extra_raw(&r))
        .await?
        .ok_or_else(|| not_found(id))?;
    let extra = map_media_extra(&raw);
    let mut vm = extra_vm(&raw, extra, r.kitsu_id);
    // Wave B (b, `metadata.providers.mal` / `.shikimori` / `.tenrai`): merge the external catalogues' facts.
    let (x, used) = super::home::enrich_extra(b, &mut vm).await;
    vm.facts.facts_sources = used.iter().map(|s| (*s).to_owned()).collect();
    vm.facts.rank = x.rank;
    vm.facts.members = x.members;
    vm.facts.screenshots = x.screenshots;
    Ok(vm)
}

async fn chain_node(b: &LiveBackend, r: &MediaRef) -> UiResult<Option<ChainNode>> {
    let Some(key) = ref_key(r) else { return Ok(None) };
    let raw = cached(b, &format!("anilist.chain.{key}"), || anilist().chain_node_raw(r)).await?;
    Ok(raw.as_ref().and_then(map_chain_node))
}

/// Walks `relation` from `start` while titles are new, at most `MAX_CHAIN_HOPS`. A failing hop ends the walk
/// (the part already known is still shown).
async fn walk(b: &LiveBackend, start: &ChainNode, relation: &str, seen: &mut HashSet<u32>) -> Vec<ChainNode> {
    let mut out: Vec<ChainNode> = Vec::new();
    for _ in 0..MAX_CHAIN_HOPS {
        let from = out.last().unwrap_or(start);
        let Some(next) = chain_next(from, relation, seen) else { break };
        seen.insert(next);
        match chain_node(b, &MediaRef::anilist(next)).await {
            Ok(Some(n)) => out.push(n),
            Ok(None) => break,
            Err(e) => {
                tracing::warn!(detail = %e.detail, next, relation, "walking the season chain stopped");
                break;
            }
        }
    }
    out
}

/// `Backend::watch_order`.
pub async fn watch_order(b: &LiveBackend, id: &str) -> UiResult<Vec<WatchOrderEntryVm>> {
    if is_extension_id(id) {
        return Ok(Vec::new());
    }
    let r = media_ref(id).ok_or_else(|| UiError::new(ErrorKind::Other, format!("invalid id {id}")))?;
    if ref_key(&r).is_none() {
        return Ok(Vec::new());
    }
    let current = chain_node(b, &r).await?.ok_or_else(|| not_found(id))?;
    let mut seen: HashSet<u32> = current.id().into_iter().collect();
    let mut back = walk(b, &current, "PREQUEL", &mut seen).await;
    let forward = walk(b, &current, "SEQUEL", &mut seen).await;
    back.reverse();
    let current_index = back.len();
    let mut chain = back;
    chain.push(current);
    chain.extend(forward);
    Ok(watch_order_steps(&chain, current_index)
        .into_iter()
        .enumerate()
        .map(|(i, s)| WatchOrderEntryVm {
            media: to_vm(&s.media),
            position: u32::try_from(i + 1).unwrap_or(u32::MAX),
            relation: s.relation,
            current: s.current,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn alternative_titles_skip_the_shown_one() {
        let raw = json!({ "title": { "romaji": "Sousou no Frieren", "english": "Frieren", "native": "葬送のフリーレン" } });
        let alt = alternative_titles(&raw, &["Frieren".to_owned(), "Frieren at the Funeral".to_owned()]);
        assert_eq!(alt, vec!["Sousou no Frieren", "葬送のフリーレン", "Frieren at the Funeral"]);
    }

    #[test]
    fn extra_vm_maps_ids_seasons_and_relation_kinds() {
        let raw = json!({ "id": 5, "idMal": 7, "season": "FALL", "title": { "romaji": "X" },
            "relations": { "edges": [ { "relationType": "SOURCE", "node": { "id": 9, "type": "MANGA", "title": { "romaji": "M" } } } ] },
            "trailer": { "site": "YouTube", "id": "abc" }, "nextAiringEpisode": { "episode": 3, "airingAt": 1_800_000_000 } });
        let vm = extra_vm(&raw, map_media_extra(&raw), Some(11));
        assert_eq!((vm.facts.anilist_id, vm.facts.mal_id, vm.facts.kitsu_id), (Some(5), Some(7), Some(11)));
        assert_eq!(vm.facts.season.as_deref(), Some("fall"));
        assert_eq!(vm.relations[0].kind, "manga");
        assert_eq!(vm.relations[0].media.id, "9");
        assert_eq!(vm.trailer.unwrap().url(), "https://www.youtube.com/watch?v=abc");
        assert_eq!(vm.next_airing, Some(AiringVm { episode: 3, at_unix: 1_800_000_000 }));
    }

    #[test]
    fn display_title_follows_the_preference() {
        let raw = json!({ "id": 1, "title": { "romaji": "Shingeki no Kyojin", "english": "Attack on Titan", "native": "進撃の巨人" } });
        let m = tk_core::metadata::mapping::map_anilist_media(&raw);
        assert_eq!(display_title(&m, "native"), "進撃の巨人");
        assert_eq!(display_title(&m, "romaji"), "Shingeki no Kyojin");
        assert_eq!(display_title(&m, "english"), "Attack on Titan");
        let bare = tk_core::metadata::mapping::map_anilist_media(&json!({ "id": 2, "title": { "romaji": "R" } }));
        assert_eq!(display_title(&bare, "native"), "R");
    }

    #[test]
    fn extension_ids_are_recognised() {
        assert!(is_extension_id("ext:demo:abc"));
        assert!(!is_extension_id("21"));
        assert!(!is_extension_id("mal:21"));
    }

    #[test]
    fn cache_keys_prefer_anilist() {
        assert_eq!(ref_key(&MediaRef::anilist(21)).as_deref(), Some("a21"));
        assert_eq!(ref_key(&MediaRef { mal_id: Some(4), ..MediaRef::default() }).as_deref(), Some("m4"));
        assert_eq!(ref_key(&MediaRef { kitsu_id: Some(4), ..MediaRef::default() }), None);
    }
}
