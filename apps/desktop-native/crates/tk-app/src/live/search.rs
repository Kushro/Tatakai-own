//! `Backend` search / feed / character calls (wave A, worker **a**; docs/redesign/15-parity-wave-a-contract.md
//! section 4).
//!
//! - `search_filtered`: AniList's real filters (`tk_core::metadata::anilist::search`), reached directly (the
//!   router has no generic call for them). When AniList fails and the query is a plain text search, the metadata
//!   router answers it instead (Kitsu / others, no filters).
//! - `search_in_extension`: the extension's custom watch sources (`custom/search` route, ids `ext:<ext>:<source>:<item>`)
//!   plus, for extensions that resolve streams by title (`stream` / `torrent` / `sources` routes), the metadata
//!   results of the query that the extension can actually play (probed with episode 1, concurrently).
//! - `feed`: the feed ids of `tk_ui::screens::browse::FeedId` (home rows, trending windows, genre / tag catalogues,
//!   character search, AniList's tag collection).
//! - `search_by_studio`: AniList `Studio(search:)`.
//! - `search_by_image`: trace.moe (`tk_core::tracemoe`), titles looked up on AniList by id.
//! - `character`: AniList `Character(id:)`, kept in memory for a while (pages are opened back and forth).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tk_core::metadata::AniList;
use tk_core::metadata::anilist::search::{Character, CharacterPage, MediaTag, SearchResultPage, SearchSort, SearchVars};
use tk_core::tracemoe::{ImageQuery, Match, TraceMoe};
use tk_core::{FeedKind, FeedPage, Media, MediaKind, ProviderError};
use tk_ext::{ExtensionHost, Manifest};
use tk_ui::backend::{
    CharacterAppearanceVm, CharacterVm, ImageMatchVm, ImageQueryVm, MediaVm, PersonVm, SearchPageVm, SearchQueryVm,
    SearchSortVm, UiResult,
};
use tk_ui::model::{ErrorKind, UiError};
use tk_ui::screens::browse::{FeedId, TAG_ADULT, TAG_SPOILER};

use super::{LiveBackend, provider_err, to_vm};

/// Results per page of the filtered search and the genre / tag feeds.
const PER_PAGE: u32 = 30;
/// Metadata results probed per page of a search inside a title-resolving extension.
const PROBE_PER_PAGE: usize = 10;
/// One probe of an extension (`stream` route) never waits longer than this.
const PROBE_TIMEOUT: Duration = Duration::from_secs(25);
/// Character pages kept in memory.
const CHARACTER_TTL: Duration = Duration::from_secs(30 * 60);
const CHARACTER_CACHE_MAX: usize = 64;

fn http_client(timeout: Duration) -> reqwest::Client {
    reqwest::Client::builder().user_agent("TatakaiNext/0.1").timeout(timeout).build().unwrap_or_default()
}

/// AniList for the calls the metadata router does not offer (filters, studios, characters, tags, trends).
fn anilist() -> &'static AniList {
    static CLIENT: OnceLock<AniList> = OnceLock::new();
    CLIENT.get_or_init(|| AniList::new(http_client(Duration::from_secs(20))))
}

/// trace.moe; uploads of a few MiB need a longer timeout than metadata calls.
fn tracemoe() -> &'static TraceMoe {
    static CLIENT: OnceLock<TraceMoe> = OnceLock::new();
    CLIENT.get_or_init(|| TraceMoe::new(http_client(Duration::from_secs(60))))
}

fn now_unix() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

// ---------------------------------------------------------------------------------------------
// Pure mapping (tested)
// ---------------------------------------------------------------------------------------------

fn sort_of(s: SearchSortVm) -> SearchSort {
    match s {
        SearchSortVm::Relevance => SearchSort::Relevance,
        SearchSortVm::Popularity => SearchSort::Popularity,
        SearchSortVm::Score => SearchSort::Score,
        SearchSortVm::Trending => SearchSort::Trending,
        SearchSortVm::Newest => SearchSort::Newest,
        SearchSortVm::Title => SearchSort::Title,
    }
}

/// `SearchQueryVm::status` (`releasing | finished | upcoming | cancelled | hiatus`) -> AniList `MediaStatus`.
fn status_of(s: &str) -> Option<String> {
    let v = match s.trim().to_ascii_lowercase().as_str() {
        "releasing" | "airing" => "RELEASING",
        "finished" | "completed" => "FINISHED",
        "upcoming" | "not_yet_released" => "NOT_YET_RELEASED",
        "cancelled" | "canceled" => "CANCELLED",
        "hiatus" => "HIATUS",
        _ => return None,
    };
    Some(v.to_owned())
}

fn non_empty(s: &Option<String>) -> Option<String> {
    s.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned)
}

/// The AniList variables of a UI query (anime only).
pub(crate) fn vars_of(q: &SearchQueryVm) -> SearchVars {
    SearchVars {
        kind: Some(MediaKind::Anime),
        query: q.query.clone(),
        genres: q.genres.clone(),
        tags: q.tags.clone(),
        format: non_empty(&q.format).map(|f| f.to_ascii_uppercase()),
        status: q.status.as_deref().and_then(status_of),
        season: non_empty(&q.season).map(|s| s.to_ascii_uppercase()),
        season_year: q.year,
        year_from: q.year_from,
        year_to: q.year_to,
        episodes_min: q.episodes_min,
        episodes_max: q.episodes_max,
        score_min: q.score_min,
        score_max: q.score_max,
        sort: sort_of(q.sort),
        adult: q.adult,
        page: q.page.max(1),
        per_page: PER_PAGE,
    }
}

fn page_vm(p: &FeedPage, total: Option<u32>) -> SearchPageVm {
    SearchPageVm { items: p.items.iter().map(to_vm).collect(), page: p.page.max(1), has_next: p.has_next, total }
}

fn result_vm(p: SearchResultPage) -> SearchPageVm {
    page_vm(&p.page, p.total)
}

fn home_feed(f: &str) -> Option<FeedKind> {
    Some(match f {
        "trending" => FeedKind::Trending,
        "airing" => FeedKind::Airing,
        "seasonal" => FeedKind::Seasonal,
        "popular" => FeedKind::Popular,
        "top_rated" => FeedKind::TopRated,
        "upcoming" => FeedKind::Upcoming,
        "recently_completed" => FeedKind::RecentlyCompleted,
        _ => return None,
    })
}

/// Characters as cards (see `tk_ui::screens::browse` docs): id = AniList character id, cover = portrait.
fn characters_vm(p: CharacterPage) -> SearchPageVm {
    SearchPageVm {
        items: p
            .items
            .into_iter()
            .map(|c| MediaVm {
                id: c.id.to_string(),
                title: c.name,
                cover: c.image,
                color: None,
                subtitle: c.native_name.unwrap_or_default(),
                score: None,
            })
            .collect(),
        page: p.page.max(1),
        has_next: p.has_next,
        total: p.total,
    }
}

/// AniList tags as feed items (see `tk_ui::screens::browse` docs): `score` carries the spoiler / 18+ bits.
fn tags_vm(tags: Vec<MediaTag>) -> SearchPageVm {
    let total = u32::try_from(tags.len()).ok();
    SearchPageVm {
        items: tags
            .into_iter()
            .map(|t| {
                let flags = if t.spoiler { TAG_SPOILER } else { 0 } | if t.adult { TAG_ADULT } else { 0 };
                MediaVm { id: t.name.clone(), title: t.name, cover: None, color: None, subtitle: t.category, score: Some(flags) }
            })
            .collect(),
        page: 1,
        has_next: false,
        total,
    }
}

/// UI id of a custom-source result: `ext:<extension id>:<source id>:<item id>` (the details page splits it with
/// `splitn(4, ':')`; extension ids never contain `:`).
pub(crate) fn ext_item_id(ext_id: &str, source_id: &str, item_id: &str) -> String {
    format!("ext:{ext_id}:{source_id}:{item_id}")
}

/// Map a `custom/search` answer (`{ results: CustomMediaCard[] }`, or the array itself) to cards. Explicit (18+)
/// entries are dropped, like the classic default.
pub(crate) fn map_custom_results(ext_id: &str, source_id: &str, body: &Value) -> Vec<MediaVm> {
    let list = if body.is_array() { body } else { &body["results"] };
    let text = |v: &Value| v.as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
    list.as_array()
        .into_iter()
        .flatten()
        .filter(|c| c["isAdult"].as_bool() != Some(true) && c["nsfw"].as_bool() != Some(true))
        .filter_map(|c| {
            let id = text(&c["id"]).or_else(|| c["id"].as_i64().map(|n| n.to_string()))?;
            let title = text(&c["title"])?;
            Some(MediaVm {
                id: ext_item_id(ext_id, source_id, &id),
                title,
                cover: text(&c["image"]).filter(|u| u.starts_with("https://") || u.starts_with("http://")),
                color: None,
                subtitle: text(&c["subtitle"]).unwrap_or_default(),
                score: None,
            })
        })
        .collect()
}

/// Which source route of a title-resolving extension proves that it can play a title.
pub(crate) fn probe_route(m: &Manifest) -> Option<&'static str> {
    let routes = m.gated_routes();
    let has = |r: &str| routes.iter().any(|x| x == r);
    if m.kind != "torrent" && has("stream") {
        Some("stream")
    } else if has("torrent") {
        Some("torrent")
    } else if has("sources") {
        Some("sources")
    } else {
        None
    }
}

/// Route parameters of a probe (same shape as the host's own `stream` resolution).
fn probe_params(m: &Media) -> Value {
    let mut titles: Vec<String> = vec![m.title.clone()];
    for t in m.titles.values() {
        if !titles.contains(t) {
            titles.push(t.clone());
        }
    }
    let mut p = json!({ "titles": titles, "episode": 1 });
    if let Some(id) = m.media_ref.anilist_id {
        p["anilistId"] = json!(id);
    }
    p
}

/// `(year, month, day)` -> display text through the current UI language (`tk_ui::screens::character`).
fn birthday(b: (Option<i32>, Option<u32>, Option<u32>)) -> Option<String> {
    tk_ui::screens::character::birthday_text(b.0, b.1, b.2)
}

fn person_vm(p: tk_core::metadata::anilist::search::Person) -> PersonVm {
    PersonVm { id: p.id.to_string(), name: p.name, image: p.image, role: p.role }
}

pub(crate) fn character_vm(c: Character) -> CharacterVm {
    CharacterVm {
        id: c.id.to_string(),
        name: c.name,
        native_name: c.native_name,
        alternative_names: c.alternative_names,
        image: c.image,
        description: c.description,
        gender: c.gender,
        age: c.age,
        birthday: birthday(c.birthday),
        blood_type: c.blood_type,
        favourites: c.favourites,
        appearances: c
            .appearances
            .into_iter()
            .map(|a| CharacterAppearanceVm {
                media: to_vm(&a.media),
                role: a.role,
                voice_actors: a.voice_actors.into_iter().map(person_vm).collect(),
            })
            .collect(),
    }
}

/// trace.moe hits with their titles; ids AniList does not return still show (with a placeholder title).
fn image_matches(matches: Vec<Match>, titles: &[Media]) -> Vec<ImageMatchVm> {
    matches
        .into_iter()
        .map(|m| {
            let media = titles
                .iter()
                .find(|t| t.media_ref.anilist_id == Some(m.anilist_id))
                .map(to_vm)
                .unwrap_or_else(|| MediaVm {
                    id: m.anilist_id.to_string(),
                    title: format!("AniList #{}", m.anilist_id),
                    cover: None,
                    color: None,
                    subtitle: String::new(),
                    score: None,
                });
            ImageMatchVm {
                media,
                episode: m.episode,
                from_secs: m.from_secs,
                to_secs: m.to_secs,
                similarity: m.similarity,
                preview_image: m.image,
                preview_video: m.video,
            }
        })
        .collect()
}

fn ext_err(e: impl std::fmt::Display) -> UiError {
    UiError::new(ErrorKind::Extension, e.to_string())
}

// ---------------------------------------------------------------------------------------------
// Backend calls
// ---------------------------------------------------------------------------------------------

/// `Backend::search_filtered`.
pub async fn search_filtered(b: &LiveBackend, q: &SearchQueryVm) -> UiResult<SearchPageVm> {
    let vars = vars_of(q);
    match anilist().search_filtered_page(&vars).await {
        Ok(p) => Ok(result_vm(p)),
        Err(e) if vars.is_plain() && !vars.query.trim().is_empty() => {
            tracing::warn!(error = %e, "AniList search failed; falling back to the metadata router");
            let p = b.router.search(MediaKind::Anime, &vars.query, vars.page).await.map_err(provider_err)?;
            Ok(page_vm(&p, None))
        }
        Err(e) => Err(provider_err(e)),
    }
}

/// `Backend::search_in_extension`.
pub async fn search_in_extension(b: &LiveBackend, ext_id: &str, query: &str, page: u32) -> UiResult<Vec<MediaVm>> {
    let query = query.split_whitespace().collect::<Vec<_>>().join(" ");
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let host = b.ext.host();
    let info = host.get(ext_id).ok_or_else(|| ext_err(format!("{ext_id} is not installed")))?;
    if !info.enabled {
        return Err(ext_err(format!("{ext_id} is disabled")));
    }
    let manifest = read_manifest(&host, ext_id).await?;
    let page = page.max(1);
    let mut out: Vec<MediaVm> = Vec::new();
    let mut failures: Vec<String> = Vec::new();

    // 1. Custom watch sources: the extension's own catalogue.
    for source in manifest.custom_sources.iter().filter(|s| s.kind == "watch") {
        let params = json!({ "sourceId": source.id, "query": query, "page": page });
        match host.call(ext_id, "custom/search", params).await {
            Ok(body) => {
                for m in map_custom_results(ext_id, &source.id, &body) {
                    if !out.iter().any(|x| x.id == m.id) {
                        out.push(m);
                    }
                }
            }
            Err(e) => {
                tracing::warn!(extension = %ext_id, source = %source.id, error = %e, "custom search failed");
                failures.push(e.to_string());
            }
        }
    }

    // 2. Title-resolving extensions: metadata results the extension can play.
    if let Some(route) = probe_route(&manifest) {
        match b.router.search(MediaKind::Anime, &query, page).await {
            Ok(p) => {
                for m in probe(&host, ext_id, route, p.items).await {
                    let vm = to_vm(&m);
                    if !vm.id.is_empty() && !out.iter().any(|x| x.id == vm.id) {
                        out.push(vm);
                    }
                }
            }
            Err(e) => failures.push(e.to_string()),
        }
    } else if manifest.custom_sources.iter().all(|s| s.kind != "watch") {
        return Err(ext_err(format!("{} cannot search anime", manifest.name)));
    }

    if out.is_empty() && !failures.is_empty() && page == 1 {
        return Err(ext_err(failures.join("; ")));
    }
    Ok(out)
}

async fn read_manifest(host: &ExtensionHost, ext_id: &str) -> UiResult<Manifest> {
    if !tk_ext::manifest::valid_id(ext_id) {
        return Err(ext_err(format!("invalid extension id {ext_id}")));
    }
    let path = host.root().join(ext_id).join("manifest.json");
    let text = tokio::fs::read_to_string(&path).await.map_err(ext_err)?;
    Manifest::parse(&text).map_err(ext_err)
}

/// The titles of `items` (first [`PROBE_PER_PAGE`]) for which `route` returns at least one source, in order.
async fn probe(host: &Arc<ExtensionHost>, ext_id: &str, route: &'static str, items: Vec<Media>) -> Vec<Media> {
    let items: Vec<Media> = items.into_iter().take(PROBE_PER_PAGE).collect();
    let mut set = tokio::task::JoinSet::new();
    for (i, m) in items.iter().enumerate() {
        let host = host.clone();
        let ext_id = ext_id.to_owned();
        let params = probe_params(m);
        set.spawn(async move {
            let ok = match tokio::time::timeout(PROBE_TIMEOUT, host.call(&ext_id, route, params)).await {
                Ok(Ok(v)) => v["sources"].as_array().is_some_and(|a| !a.is_empty()),
                Ok(Err(e)) => {
                    tracing::debug!(extension = %ext_id, error = %e, "probe failed");
                    false
                }
                Err(_) => false,
            };
            (i, ok)
        });
    }
    let mut playable = vec![false; items.len()];
    while let Some(r) = set.join_next().await {
        if let Ok((i, true)) = r {
            playable[i] = true;
        }
    }
    items.into_iter().zip(playable).filter(|(_, ok)| *ok).map(|(m, _)| m).collect()
}

/// `Backend::feed` (feed ids of `tk_ui::screens::browse::FeedId`).
pub async fn feed(b: &LiveBackend, feed: &str, page: u32) -> UiResult<SearchPageVm> {
    let page = page.max(1);
    let id = FeedId::parse(feed).ok_or_else(|| UiError::new(ErrorKind::Other, format!("unknown feed {feed}")))?;
    let catalogue = |genres: Vec<String>, tags: Vec<String>, sort: SearchSort| SearchVars {
        kind: Some(MediaKind::Anime),
        genres,
        tags,
        sort,
        page,
        per_page: PER_PAGE,
        ..SearchVars::default()
    };
    match id {
        FeedId::Home(f) => {
            let kind = home_feed(&f).ok_or_else(|| UiError::new(ErrorKind::Other, format!("unknown feed {f}")))?;
            let p = b.router.feed(MediaKind::Anime, kind, page).await.map_err(provider_err)?;
            Ok(page_vm(&p, None))
        }
        FeedId::Trending(w) => {
            let r = match w.seconds() {
                Some(secs) => anilist().trending_since(now_unix() - secs, page, false).await,
                None => anilist().search_filtered_page(&catalogue(Vec::new(), Vec::new(), SearchSort::Popularity)).await,
            };
            r.map(result_vm).map_err(provider_err)
        }
        FeedId::Genre(g) => anilist()
            .search_filtered_page(&catalogue(vec![g], Vec::new(), SearchSort::Popularity))
            .await
            .map(result_vm)
            .map_err(provider_err),
        FeedId::Tag(t) => anilist()
            .search_filtered_page(&catalogue(Vec::new(), vec![t], SearchSort::Popularity))
            .await
            .map(result_vm)
            .map_err(provider_err),
        FeedId::Characters(q) => anilist().search_characters(&q, page).await.map(characters_vm).map_err(provider_err),
        FeedId::Tags => {
            if page > 1 {
                return Ok(SearchPageVm { items: Vec::new(), page, has_next: false, total: None });
            }
            anilist().tag_collection().await.map(tags_vm).map_err(provider_err)
        }
    }
}

/// `Backend::search_by_studio`.
pub async fn search_by_studio(_b: &LiveBackend, studio: &str, page: u32) -> UiResult<SearchPageVm> {
    anilist().studio_page(studio, page.max(1), false).await.map(result_vm).map_err(provider_err)
}

/// `Backend::search_by_image` (trace.moe, then AniList for titles).
pub async fn search_by_image(_b: &LiveBackend, image: ImageQueryVm) -> UiResult<Vec<ImageMatchVm>> {
    let q = match image {
        ImageQueryVm::Url(u) => ImageQuery::Url(u),
        ImageQueryVm::Bytes { data, mime } => ImageQuery::Bytes { data, mime },
    };
    let matches = tracemoe().search(&q, true).await.map_err(|e| match e {
        ProviderError::Parse(msg) => UiError::new(ErrorKind::Other, msg),
        other => provider_err(other),
    })?;
    let mut ids: Vec<u32> = Vec::new();
    for m in &matches {
        if !ids.contains(&m.anilist_id) {
            ids.push(m.anilist_id);
        }
    }
    // Titles are a nicety: a failed lookup still shows the scenes.
    let titles = match anilist().media_by_ids(&ids).await {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(error = %e, "AniList lookup of trace.moe ids failed");
            Vec::new()
        }
    };
    Ok(image_matches(matches, &titles))
}

type CharacterCache = Mutex<HashMap<u32, (Instant, Character)>>;

fn character_cache() -> &'static CharacterCache {
    static CACHE: OnceLock<CharacterCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// `Backend::character`.
pub async fn character(_b: &LiveBackend, character_id: &str) -> UiResult<CharacterVm> {
    let id: u32 = character_id
        .trim()
        .parse()
        .ok()
        .filter(|n| *n != 0)
        .ok_or_else(|| UiError::new(ErrorKind::Other, format!("invalid character id {character_id}")))?;
    let cached = character_cache()
        .lock()
        .ok()
        .and_then(|c| c.get(&id).filter(|(at, _)| at.elapsed() < CHARACTER_TTL).map(|(_, v)| v.clone()));
    let c = match cached {
        Some(c) => c,
        None => {
            let c = anilist()
                .character(id)
                .await
                .map_err(provider_err)?
                .ok_or_else(|| UiError::new(ErrorKind::Other, format!("character {id} was not found")))?;
            if let Ok(mut cache) = character_cache().lock() {
                cache.retain(|_, (at, _)| at.elapsed() < CHARACTER_TTL);
                if cache.len() >= CHARACTER_CACHE_MAX {
                    cache.clear();
                }
                cache.insert(id, (Instant::now(), c.clone()));
            }
            c
        }
    };
    Ok(character_vm(c))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tk_core::MediaRef;
    use tk_ui::screens::browse::TrendWindow;

    fn media(id: u32) -> Media {
        Media {
            media_ref: MediaRef::anilist(id),
            kind: MediaKind::Anime,
            title: format!("T{id}"),
            titles: [("romaji".to_owned(), format!("R{id}")), ("english".to_owned(), format!("T{id}"))].into_iter().collect(),
            cover: None,
            banner: None,
            color: None,
            synopsis: None,
            genres: Vec::new(),
            episodes: Some(12),
            status: None,
            season: None,
            year: Some(2020),
            score: Some(80),
            source_provider: "anilist".into(),
        }
    }

    #[test]
    fn query_maps_to_anilist_vars() {
        let q = SearchQueryVm {
            query: "frieren".into(),
            genres: vec!["Action".into()],
            format: Some("tv".into()),
            status: Some("upcoming".into()),
            season: Some("winter".into()),
            year: Some(2025),
            sort: SearchSortVm::Score,
            adult: true,
            page: 0,
            ..SearchQueryVm::default()
        };
        let v = vars_of(&q);
        assert_eq!(v.kind, Some(MediaKind::Anime));
        assert_eq!(v.format.as_deref(), Some("TV"));
        assert_eq!(v.status.as_deref(), Some("NOT_YET_RELEASED"));
        assert_eq!(v.season.as_deref(), Some("WINTER"));
        assert_eq!(v.season_year, Some(2025));
        assert_eq!(v.sort, SearchSort::Score);
        assert_eq!(v.page, 1);
        assert_eq!(v.per_page, PER_PAGE);
        assert!(v.adult);
        assert!(!v.is_plain());
        assert!(vars_of(&SearchQueryVm { query: "x".into(), ..SearchQueryVm::default() }).is_plain());
        assert_eq!(status_of("nope"), None);
        assert_eq!(status_of("Finished").as_deref(), Some("FINISHED"));
        let vars = AniList::search_vars(&v);
        assert_eq!(vars["status_in"], json!(["NOT_YET_RELEASED"]));
    }

    #[test]
    fn home_feeds_map_to_kinds() {
        for f in tk_ui::screens::browse::HOME_FEEDS {
            assert!(home_feed(f).is_some(), "{f}");
        }
        assert_eq!(home_feed("x"), None);
        assert_eq!(TrendWindow::Day.seconds(), Some(86_400));
    }

    #[test]
    fn custom_results_are_mapped_and_explicit_ones_dropped() {
        let body = json!({ "namespace": "x", "results": [
            { "id": "abc", "title": "Show", "image": "https://i/x.jpg", "subtitle": "Ep 3" },
            { "id": 42, "title": "Numeric", "image": "javascript:alert(1)" },
            { "id": "n", "title": "Adult", "isAdult": true },
            { "id": "", "title": "No id" },
            { "id": "t" }
        ] });
        let v = map_custom_results("ext.one", "src", &body);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].id, "ext:ext.one:src:abc");
        assert_eq!(v[0].cover.as_deref(), Some("https://i/x.jpg"));
        assert_eq!(v[0].subtitle, "Ep 3");
        assert_eq!(v[1].id, "ext:ext.one:src:42");
        assert_eq!(v[1].cover, None);
        let bare = json!([{ "id": "a", "title": "A" }]);
        assert_eq!(map_custom_results("e", "s", &bare).len(), 1);
        assert!(map_custom_results("e", "s", &json!({})).is_empty());
    }

    fn manifest(kind: &str, routes: Option<&[&str]>) -> Manifest {
        let mut m = json!({ "id": "x.y", "name": "X", "version": "1", "type": kind, "description": "", "permissions": [] });
        if let Some(r) = routes {
            m["apiServer"] = json!({ "routes": r });
        }
        Manifest::parse(&m.to_string()).unwrap()
    }

    #[test]
    fn probe_route_follows_the_manifest() {
        assert_eq!(probe_route(&manifest("onlinestream", None)), Some("stream"));
        assert_eq!(probe_route(&manifest("torrent", None)), Some("torrent"));
        assert_eq!(probe_route(&manifest("custom", Some(&["custom/search", "custom/info"]))), None);
        assert_eq!(probe_route(&manifest("onlinestream", Some(&["sources"]))), Some("sources"));
        let p = probe_params(&media(21));
        assert_eq!(p["anilistId"], 21);
        assert_eq!(p["episode"], 1);
        assert_eq!(p["titles"][0], "T21");
        assert_eq!(p["titles"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn tags_and_characters_become_cards() {
        let tags = vec![
            MediaTag { name: "Twist".into(), category: "Theme".into(), spoiler: true, adult: false },
            MediaTag { name: "Nudity".into(), category: "Sexual".into(), spoiler: false, adult: true },
        ];
        let p = tags_vm(tags);
        assert_eq!(p.items[0].score, Some(TAG_SPOILER));
        assert_eq!(p.items[1].score, Some(TAG_ADULT));
        assert_eq!(p.items[1].subtitle, "Sexual");
        assert!(!p.has_next);
        let c = CharacterPage {
            items: vec![tk_core::metadata::anilist::search::CharacterHit {
                id: 40,
                name: "Luffy".into(),
                native_name: Some("ルフィ".into()),
                image: Some("l.png".into()),
                favourites: None,
            }],
            page: 1,
            has_next: true,
            total: Some(1),
        };
        let p = characters_vm(c);
        assert_eq!(p.items[0].id, "40");
        assert_eq!(p.items[0].subtitle, "ルフィ");
        assert!(p.has_next);
    }

    #[test]
    fn image_matches_keep_order_and_fill_titles() {
        let m = |id: u32, sim: f32| Match { anilist_id: id, episode: Some(3), from_secs: 61.0, to_secs: 63.0, similarity: sim, image: None, video: None };
        let v = image_matches(vec![m(21, 0.95), m(99, 0.9)], &[media(21)]);
        assert_eq!(v[0].media.title, "T21");
        assert_eq!(v[0].media.id, "21");
        assert_eq!(v[1].media.id, "99");
        assert_eq!(v[1].media.title, "AniList #99");
        assert_eq!(v[0].episode, Some(3));
    }
}
