//! Home, discover, airing calendar, recommendations and metadata-provider health (wave B, worker **b**;
//! docs/redesign/16-parity-wave-b-contract.md section 4; checklist `home.*`, `details.track-show`,
//! `settings.metadata.extra-providers`, `settings.metadata.provider-status`, `metadata.providers.*`).
//!
//! * `home`: the wave-0 rows (trending feeds the hero) plus "Upcoming" (AniList `NOT_YET_RELEASED` with the expected
//!   start date) and "Most favourite" (`FAVOURITES_DESC`), each behind its `Settings::home` toggle. The other Home
//!   blocks have their own calls so they load and fail independently: `latest_episodes`, `top10` (AniList
//!   `mediaTrends` since now - 1 / 7 / 30 days), `because_you_watched`, `genres` / `genre_feed`, `followed_updates`
//!   (episodes of the Watching list aired since the previous Home visit, `home_visits`), `torrent_activity`.
//! * Calendar: `airing_schedule` (AniList `airingSchedules`, Tenrai `/schedules` as fallback), `tracked_shows` /
//!   `set_tracked` (`tracked_shows` table), `reminders_due` (tracked episodes airing within
//!   `Settings::home.reminder_minutes`, each announced once through `airing_reminders`).
//! * Recommendations: a local taste profile (`tk_core::recs`) from the history, the list mirror and "Like" feedback,
//!   candidates from AniList `discover` (top genres, seen / dismissed titles excluded), scored with reasons; the
//!   Atlas projects seen titles + recommendations to 2-D.
//! * Provider health: the router's cool-downs plus a light probe per provider (cached [`PROBE_TTL`]); MAL without a
//!   client id reports `NO_CLIENT_ID`. [`external_facts`] / [`merge_facts`] are what Details uses to fill score /
//!   rank / synopsis / alternative titles from MAL, Shikimori and Tenrai (wired into `live/details.rs` by the
//!   integrator, see the commit message).
//!
//! 18+ titles follow `Settings::home.mature_content` (`HomePrefs::adult_in_rows`): only `Show` lets them into the
//! mixed rows, the calendar and the recommendations.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use tk_core::metadata::anilist::airing::{AiringRow, DatedMedia};
use tk_core::metadata::anilist::search::{SearchSort, SearchVars};
use tk_core::metadata::mal::ExternalFacts;
use tk_core::metadata::tenrai::Broadcast;
use tk_core::metadata::{
    AniList, AniZip, COOLDOWN, Kitsu, Mal, MangaDex, MangaUpdates, MetadataProvider, Shikimori, Tenrai,
};
use tk_core::recs::{self, Candidate, Seen, TasteProfile};
use tk_core::store::home::TrackedShowRow;
use tk_core::{FeedKind, HistoryEntry, Media, MediaKind, MediaRef, ProviderError};
use tk_ui::backend::{
    AiringSlotVm, AtlasPointVm, BecauseVm, DetailsExtraVm, HomeVm, LatestEpisodeVm, MediaVm,
    ProviderHealthVm, RecommendationVm, RecsFeedbackVm, SearchPageVm, SectionVm, TasteProfileVm, TopRangeVm,
    TorrentActivityVm, TorrentStateVm, UiResult,
};
use tk_ui::model::home::{HomePrefs, NO_CLIENT_ID};
use tk_ui::model::{ErrorKind, PROVIDERS, UiError};

use crate::live::{LiveBackend, history_media, media_ref, provider_err, to_vm};
use crate::settings_store;

/// "Latest episodes" look back this far.
const LATEST_WINDOW_SECS: i64 = 3 * 86_400;
/// "Followed updates" window when there is no previous visit, and its minimum width.
const FOLLOWED_DEFAULT_SECS: i64 = 7 * 86_400;
const FOLLOWED_MIN_SECS: i64 = 86_400;
const FOLLOWED_MAX_SECS: i64 = 30 * 86_400;
/// A Home load this long after the last one starts a new visit (the previous visit is then "since").
const VISIT_GAP_SECS: i64 = 6 * 3600;
/// `LocalStore` setting holding the start of the previous Home visit.
const PREV_VISIT_KEY: &str = "home.prev_visit";
const HOME_SECTION: &str = "home";
/// Airing pages read for one calendar week (50 rows each).
const WEEK_PAGES: u32 = 8;
const TENRAI_PAGES: u32 = 6;
/// Reminders also catch an episode that aired this long ago (the poller runs once a minute).
const REMINDER_GRACE_SECS: i64 = 15 * 60;
/// "Because you watched": seeds and titles per seed.
const BECAUSE_SEEDS: usize = 3;
const BECAUSE_ITEMS: usize = 12;
/// History / list titles the taste profile reads.
const TASTE_HISTORY: u32 = 400;
const TASTE_MAX_TITLES: usize = 150;
const TASTE_TTL: Duration = Duration::from_secs(10 * 60);
/// "Skip" hides a recommendation for this long; "Not interested" forever.
const SKIP_SECS: i64 = 14 * 86_400;
/// Votes needed before the accuracy is shown.
const ACCURACY_MIN_VOTES: u32 = 5;
/// Seen titles drawn on the Atlas next to the recommendations.
const ATLAS_SEEN: usize = 60;
const GENRES_TTL: Duration = Duration::from_secs(24 * 3600);
/// Provider probe results are reused this long.
const PROBE_TTL: Duration = Duration::from_secs(5 * 60);
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
/// MAL id probed for provider health (One Piece: present on every MAL-keyed catalogue).
const PROBE_MAL_ID: u32 = 21;
/// AniList genre that is 18+ by definition.
const ADULT_GENRE: &str = "Hentai";
/// List services whose mirror rows count for Home / recommendations.
const LIST_SERVICES: [&str; 3] = ["anilist", "mal", "local"];

// ---------------------------------------------------------------------------------------------
// Clients
// ---------------------------------------------------------------------------------------------

fn client() -> &'static reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent("TatakaiNext/0.1")
            .timeout(Duration::from_secs(20))
            .build()
            .unwrap_or_default()
    })
}

/// AniList for the calls the metadata router does not offer (airing, trends, genres, recommendations).
fn anilist() -> &'static AniList {
    static P: OnceLock<AniList> = OnceLock::new();
    P.get_or_init(|| AniList::new(client().clone()))
}

fn mal() -> &'static Mal {
    static P: OnceLock<Mal> = OnceLock::new();
    P.get_or_init(|| Mal::new(client().clone()))
}

fn shikimori() -> &'static Shikimori {
    static P: OnceLock<Shikimori> = OnceLock::new();
    P.get_or_init(|| Shikimori::new(client().clone()))
}

fn tenrai() -> &'static Tenrai {
    static P: OnceLock<Tenrai> = OnceLock::new();
    P.get_or_init(|| Tenrai::new(client().clone()))
}

fn now_unix() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

fn prefs(b: &LiveBackend) -> HomePrefs {
    settings_store::load(&b.settings_file).home
}

fn store_err(e: impl std::fmt::Display) -> UiError {
    UiError::new(ErrorKind::Other, e.to_string())
}

// ---------------------------------------------------------------------------------------------
// Pure mapping (tested)
// ---------------------------------------------------------------------------------------------

/// Airing rows -> latest-episode cards: 18+ dropped unless allowed, one card per title (the first row wins, so
/// pass newest-first rows).
fn latest_vms(rows: &[AiringRow], adult: bool) -> Vec<LatestEpisodeVm> {
    let mut seen = HashSet::new();
    rows.iter()
        .filter(|r| adult || !r.adult)
        .filter_map(|r| {
            let media = to_vm(&r.media);
            (!media.id.is_empty() && seen.insert(media.id.clone())).then_some(LatestEpisodeVm {
                media,
                episode: r.episode,
                aired_unix: r.at_unix,
            })
        })
        .collect()
}

/// A dated title as a card whose subtitle is the expected start date (`YYYY-MM-DD`, partial dates trimmed).
fn dated_vm(d: &DatedMedia) -> MediaVm {
    let mut vm = to_vm(&d.media);
    if let Some(date) = &d.start_date {
        vm.subtitle = date.clone();
    }
    vm
}

fn dated_vms(items: &[DatedMedia], adult: bool) -> Vec<MediaVm> {
    items.iter().filter(|d| adult || !d.adult).map(dated_vm).collect()
}

/// `since` of "followed updates": the previous visit, at least [`FOLLOWED_MIN_SECS`] and at most
/// [`FOLLOWED_MAX_SECS`] back.
fn followed_since(prev_visit: Option<i64>, now: i64) -> i64 {
    prev_visit
        .unwrap_or(now - FOLLOWED_DEFAULT_SECS)
        .clamp(now - FOLLOWED_MAX_SECS, now - FOLLOWED_MIN_SECS)
}

/// Visit bookkeeping: `(new last visit, new previous visit)` given the stored ones. A load within
/// [`VISIT_GAP_SECS`] of the last one belongs to the same visit (nothing changes).
fn next_visit(last: Option<i64>, prev: Option<i64>, now: i64) -> (i64, Option<i64>) {
    match last {
        Some(l) if now - l < VISIT_GAP_SECS => (l, prev),
        Some(l) => (now, Some(l)),
        None => (now, None),
    }
}

/// Day of the week of a unix instant in a fixed offset (0 = Monday).
fn weekday_at(unix: i64, offset_secs: i64) -> u8 {
    // 1970-01-01 was a Thursday (index 3).
    ((((unix + offset_secs).div_euclid(86_400)) + 3).rem_euclid(7)) as u8
}

/// JST (UTC+9) weekly slot `weekday hh:mm` inside `[week_start, week_start + 7 days)`, as unix seconds.
fn jst_slot_in_week(week_start: i64, weekday: u8, hour: u8, minute: u8) -> Option<i64> {
    const JST: i64 = 9 * 3600;
    let first_midnight = (week_start + JST).div_euclid(86_400) * 86_400 - JST;
    (0..9)
        .map(|k| first_midnight + k * 86_400)
        .filter(|day| weekday_at(*day, JST) == weekday)
        .map(|day| day + i64::from(hour) * 3600 + i64::from(minute) * 60)
        .find(|t| (week_start..week_start + 7 * 86_400).contains(t))
}

/// Tenrai broadcasts -> calendar slots of the week (episode unknown = 0).
fn broadcast_slots(rows: &[Broadcast], week_start: i64) -> Vec<(Media, i64)> {
    rows.iter()
        .filter(|b| b.timezone == "Asia/Tokyo")
        .filter_map(|b| {
            Some((
                b.media.clone(),
                jst_slot_in_week(week_start, b.weekday, b.hour, b.minute)?,
            ))
        })
        .collect()
}

/// Is `vm` one of the tracked shows (by UI id, or by AniList id for rows tracked under another id)?
fn is_tracked_vm(vm: &MediaVm, tracked: &[TrackedShowRow]) -> bool {
    tracked
        .iter()
        .any(|t| t.media_id == vm.id || t.anilist_id.is_some_and(|a| a.to_string() == vm.id))
}

fn tracked_vm(r: &TrackedShowRow) -> MediaVm {
    MediaVm {
        id: r.media_id.clone(),
        title: r.title.clone(),
        cover: r.poster.clone(),
        color: None,
        subtitle: String::new(),
        score: None,
    }
}

/// Calendar slots from airing rows (18+ dropped unless allowed), sorted by time; `tracked_only` keeps My shows.
fn slots_from_rows(rows: &[AiringRow], tracked: &[TrackedShowRow], adult: bool) -> Vec<AiringSlotVm> {
    let mut v: Vec<AiringSlotVm> = rows
        .iter()
        .filter(|r| adult || !r.adult)
        .map(|r| {
            let media = to_vm(&r.media);
            let tracked = is_tracked_vm(&media, tracked);
            AiringSlotVm {
                media,
                episode: r.episode,
                at_unix: r.at_unix,
                tracked,
            }
        })
        .filter(|s| !s.media.id.is_empty())
        .collect();
    v.sort_by(|a, b| {
        a.at_unix
            .cmp(&b.at_unix)
            .then_with(|| a.media.title.cmp(&b.media.title))
    });
    v.dedup_by(|a, b| a.media.id == b.media.id && a.episode == b.episode);
    v
}

/// Genres for the cloud: AniList order, the 18+ genre only when 18+ titles may be shown where asked.
fn visible_genres(all: &[String], adult_where_asked: bool) -> Vec<String> {
    all.iter()
        .filter(|g| adult_where_asked || g.as_str() != ADULT_GENRE)
        .cloned()
        .collect()
}

/// Top `n` trending titles, 18+ excluded unless allowed (`adult_ids` = AniList ids flagged 18+).
fn top_n(items: &[Media], adult_ids: &HashSet<u32>, adult: bool, n: usize) -> Vec<MediaVm> {
    items
        .iter()
        .filter(|m| adult || !m.media_ref.anilist_id.is_some_and(|id| adult_ids.contains(&id)))
        .take(n)
        .map(to_vm)
        .collect()
}

/// Seconds of the Top 10 range.
fn range_secs(r: TopRangeVm) -> i64 {
    match r {
        TopRangeVm::Today => 86_400,
        TopRangeVm::Week => 7 * 86_400,
        TopRangeVm::Month => 30 * 86_400,
    }
}

/// Ids hidden from recommendations by feedback: "Not interested" always, "Skip" for [`SKIP_SECS`].
fn dismissed(rows: &[(String, String, i64)], now: i64) -> HashSet<String> {
    rows.iter()
        .filter(|(_, f, at)| {
            f == RecsFeedbackVm::NotInterested.id()
                || (f == RecsFeedbackVm::Skip.id() && now - at < SKIP_SECS)
        })
        .map(|(id, _, _)| id.clone())
        .collect()
}

/// Fill what `base` lacks from the external catalogues, in priority order: score, rank, popularity, members,
/// synopsis, episodes; alternative titles and screenshots are merged without duplicates. Returns the providers
/// that contributed at least one field (for `data-facts-source`).
pub(super) fn merge_facts(base: &mut ExternalFacts, others: &[ExternalFacts]) -> Vec<&'static str> {
    let mut used = Vec::new();
    for o in others {
        let mut took = false;
        let mut fill_u32 = |slot: &mut Option<u32>, v: Option<u32>| {
            if slot.is_none() && v.is_some() {
                *slot = v;
                took = true;
            }
        };
        fill_u32(&mut base.score, o.score);
        fill_u32(&mut base.rank, o.rank);
        fill_u32(&mut base.popularity_rank, o.popularity_rank);
        fill_u32(&mut base.members, o.members);
        fill_u32(&mut base.episodes, o.episodes);
        if base.synopsis.is_none() && o.synopsis.is_some() {
            base.synopsis = o.synopsis.clone();
            took = true;
        }
        for t in &o.titles {
            if !base.titles.contains(t) {
                base.titles.push(t.clone());
                took = true;
            }
        }
        for s in &o.screenshots {
            if !base.screenshots.contains(s) {
                base.screenshots.push(s.clone());
                took = true;
            }
        }
        if took && !used.contains(&o.provider) {
            used.push(o.provider);
        }
    }
    used
}

/// Status of a probe result for the health map: `Ok` = answered, `Err(text)` = failed, `None` = this provider cannot
/// be probed (capability not implemented).
fn probe_outcome(r: Result<(), ProviderError>, id: &str) -> Option<Result<(), String>> {
    match r {
        Ok(()) => Some(Ok(())),
        Err(ProviderError::Unsupported) if id == "mal" => Some(Err(NO_CLIENT_ID.to_owned())),
        Err(ProviderError::Unsupported) => None,
        Err(e) => Some(Err(e.to_string())),
    }
}

// ---------------------------------------------------------------------------------------------
// Home
// ---------------------------------------------------------------------------------------------

/// `Backend::home`: the wave-0 rows plus Upcoming and Most favourite.
pub async fn home(b: &LiveBackend) -> UiResult<HomeVm> {
    let p = prefs(b);
    let adult = p.adult_in_rows();
    let feed = |f| b.router.feed(MediaKind::Anime, f, 1);
    let upcoming = async {
        if p.show_upcoming {
            Some(anilist().upcoming(1, adult).await)
        } else {
            None
        }
    };
    let favourite = async {
        if p.show_most_favorite {
            Some(anilist().most_favourite(1, adult).await)
        } else {
            None
        }
    };
    let (trending, airing, seasonal, popular, top, upcoming, favourite) = tokio::join!(
        feed(FeedKind::Trending),
        feed(FeedKind::Airing),
        feed(FeedKind::Seasonal),
        feed(FeedKind::Popular),
        feed(FeedKind::TopRated),
        upcoming,
        favourite
    );
    let section = |key: &str, r: Result<tk_core::FeedPage, ProviderError>| SectionVm {
        title_key: key.to_owned(),
        items: r
            .map(|p| p.items.iter().map(to_vm).collect())
            .map_err(provider_err),
    };
    let dated = |key: &str, r: Result<(Vec<DatedMedia>, bool), ProviderError>| SectionVm {
        title_key: key.to_owned(),
        items: r.map(|(items, _)| dated_vms(&items, adult)).map_err(provider_err),
    };
    let continue_watching = b.continue_vms(20);
    let mut sections = vec![
        section("feed.trending", trending),
        section("feed.airing", airing),
        section("feed.seasonal", seasonal),
        section("feed.popular", popular),
        section("feed.top_rated", top),
    ];
    if let Some(r) = upcoming {
        sections.push(dated("home.row.upcoming", r));
    }
    if let Some(r) = favourite {
        sections.push(dated("home.row.most_favorite", r));
    }
    // Wave B (j, `ext.custom-providers.home-rows`): one `ext:<extension name>` row per extension with watch providers.
    sections.extend(super::ext_catalog::home_rows(b).await);
    Ok(HomeVm {
        continue_watching,
        sections,
    })
}

pub async fn latest_episodes(b: &LiveBackend, page: u32) -> UiResult<Vec<LatestEpisodeVm>> {
    let adult = prefs(b).adult_in_rows();
    let now = now_unix();
    let p = anilist()
        .airing_schedules(now - LATEST_WINDOW_SECS, now, None, true, page.max(1))
        .await
        .map_err(provider_err)?;
    Ok(latest_vms(&p.rows, adult))
}

pub async fn top10(b: &LiveBackend, range: TopRangeVm) -> UiResult<Vec<MediaVm>> {
    let adult = prefs(b).adult_in_rows();
    let p = anilist()
        .trending_since(now_unix() - range_secs(range), 1, adult)
        .await
        .map_err(provider_err)?;
    Ok(top_n(&p.page.items, &p.adult_ids, adult, 10))
}

pub async fn because_you_watched(b: &LiveBackend) -> UiResult<Vec<BecauseVm>> {
    let adult = prefs(b).adult_in_rows();
    let history = b.store.history(100).map_err(store_err)?;
    let mut seeds: Vec<(u32, &HistoryEntry)> = Vec::new();
    for e in &history {
        if let Some(id) = e.anilist_id.filter(|i| *i > 0)
            && !seeds.iter().any(|(s, _)| *s == id)
        {
            seeds.push((id, e));
        }
        if seeds.len() >= BECAUSE_SEEDS {
            break;
        }
    }
    let mut out = Vec::new();
    let mut last_err = None;
    for (id, e) in seeds {
        match anilist().recommendations_of(id).await {
            Ok(list) => {
                let items: Vec<MediaVm> = list
                    .iter()
                    .filter(|(_, a)| adult || !*a)
                    .map(|(m, _)| to_vm(m))
                    .take(BECAUSE_ITEMS)
                    .collect();
                if !items.is_empty() {
                    out.push(BecauseVm {
                        seed: history_media(e),
                        items,
                    });
                }
            }
            Err(err) => last_err = Some(err),
        }
    }
    match (out.is_empty(), last_err) {
        (true, Some(e)) => Err(provider_err(e)),
        _ => Ok(out),
    }
}

pub async fn genres(b: &LiveBackend) -> UiResult<Vec<String>> {
    static CACHE: Mutex<Option<(Instant, Vec<String>)>> = Mutex::new(None);
    let where_asked = prefs(b).adult_where_asked();
    let cached = CACHE.lock().ok().and_then(|c| {
        c.as_ref()
            .filter(|(at, _)| at.elapsed() < GENRES_TTL)
            .map(|(_, g)| g.clone())
    });
    let all = match cached {
        Some(g) => g,
        None => {
            let g = anilist().genres().await.map_err(provider_err)?;
            if let Ok(mut c) = CACHE.lock() {
                *c = Some((Instant::now(), g.clone()));
            }
            g
        }
    };
    Ok(visible_genres(&all, where_asked))
}

pub async fn genre_feed(b: &LiveBackend, genre: &str, page: u32) -> UiResult<SearchPageVm> {
    let p = prefs(b);
    let adult = if genre == ADULT_GENRE {
        p.adult_where_asked()
    } else {
        p.adult_in_rows()
    };
    if genre == ADULT_GENRE && !adult {
        return Ok(SearchPageVm {
            items: Vec::new(),
            page: page.max(1),
            has_next: false,
            total: Some(0),
        });
    }
    let vars = SearchVars {
        kind: Some(MediaKind::Anime),
        genres: vec![genre.to_owned()],
        sort: SearchSort::Popularity,
        adult,
        page: page.max(1),
        per_page: 24,
        ..SearchVars::default()
    };
    let r = anilist()
        .search_filtered_page(&vars)
        .await
        .map_err(provider_err)?;
    Ok(SearchPageVm {
        items: top_n(&r.page.items, &r.adult_ids, adult, usize::MAX),
        page: r.page.page.max(1),
        has_next: r.page.has_next,
        total: r.total,
    })
}

/// AniList ids of the Watching (and rewatching) rows of every list mirror.
fn watching_ids(b: &LiveBackend) -> Vec<u32> {
    let mut ids = Vec::new();
    for s in LIST_SERVICES {
        for st in ["watching", "repeating"] {
            for row in b.store.list_entries(s, Some(st)).unwrap_or_default() {
                if let Some(id) = row.anilist_id.filter(|i| *i > 0)
                    && !ids.contains(&id)
                {
                    ids.push(id);
                }
            }
        }
    }
    ids
}

/// Start of the previous Home visit, updating the bookkeeping (see [`next_visit`]).
fn previous_visit(b: &LiveBackend, now: i64) -> Option<i64> {
    let last = b.store.last_home_visit(HOME_SECTION).ok().flatten();
    let prev = b.store.get_setting::<i64>(PREV_VISIT_KEY).ok().flatten();
    let (new_last, new_prev) = next_visit(last, prev, now);
    if Some(new_last) != last
        && let Err(e) = b.store.home_visited(HOME_SECTION, new_last)
    {
        tracing::warn!(%e, "recording the Home visit failed");
    }
    if new_prev != prev
        && let Some(p) = new_prev
        && let Err(e) = b.store.set_setting(PREV_VISIT_KEY, &p)
    {
        tracing::warn!(%e, "recording the previous Home visit failed");
    }
    new_prev
}

pub async fn followed_updates(b: &LiveBackend) -> UiResult<Vec<LatestEpisodeVm>> {
    let now = now_unix();
    let since = followed_since(previous_visit(b, now), now);
    let ids = watching_ids(b);
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut rows = Vec::new();
    for chunk in ids.chunks(50) {
        let p = anilist()
            .airing_schedules(since, now, Some(chunk), true, 1)
            .await
            .map_err(provider_err)?;
        rows.extend(p.rows);
    }
    rows.sort_by(|a, b| b.at_unix.cmp(&a.at_unix));
    // Titles of the user's own list: 18+ is not filtered here (they chose them).
    Ok(latest_vms(&rows, true))
}

pub async fn torrent_activity(b: &LiveBackend) -> UiResult<TorrentActivityVm> {
    // Never start the engine for the banner: a closed client has no activity.
    if b.torrent.running().is_none() {
        return Ok(TorrentActivityVm::default());
    }
    let items: Vec<_> = b
        .torrent
        .library()
        .await?
        .into_iter()
        .filter(|i| {
            matches!(
                i.state,
                TorrentStateVm::Downloading | TorrentStateVm::Initializing
            )
        })
        .collect();
    let (mut down, mut up) = (0_u64, 0_u64);
    for i in &items {
        if let Ok(s) = b.torrent.stats(&i.id).await {
            down += s.down_bps;
            up += s.up_bps;
        }
    }
    Ok(TorrentActivityVm {
        active: u32::try_from(items.len()).unwrap_or(u32::MAX),
        down_bps: down,
        up_bps: up,
        items,
    })
}

// ---------------------------------------------------------------------------------------------
// Calendar
// ---------------------------------------------------------------------------------------------

fn tracked_rows(b: &LiveBackend) -> Vec<TrackedShowRow> {
    b.store.tracked_shows().unwrap_or_default()
}

pub async fn airing_schedule(b: &LiveBackend, week_start_unix: i64) -> UiResult<Vec<AiringSlotVm>> {
    let adult = prefs(b).adult_in_rows();
    let tracked = tracked_rows(b);
    let end = week_start_unix + 7 * 86_400 - 1;
    let mut rows = Vec::new();
    let mut failure = None;
    for page in 1..=WEEK_PAGES {
        match anilist()
            .airing_schedules(week_start_unix, end, None, false, page)
            .await
        {
            Ok(p) => {
                rows.extend(p.rows);
                if !p.has_next {
                    break;
                }
            }
            Err(e) => {
                failure = Some(e);
                break;
            }
        }
    }
    if rows.is_empty()
        && let Some(e) = failure
    {
        // AniList is down: Tenrai's weekly broadcast table (MAL ids, JST times) as the fallback.
        tracing::debug!(error = %e, "AniList airing failed, trying Tenrai schedules");
        let mut slots = Vec::new();
        for page in 1..=TENRAI_PAGES {
            match tenrai().schedules(None, page).await {
                Ok((list, next)) => {
                    slots.extend(broadcast_slots(&list, week_start_unix));
                    if !next {
                        break;
                    }
                }
                Err(_) if !slots.is_empty() => break,
                Err(_) => return Err(provider_err(e)),
            }
        }
        let rows: Vec<AiringRow> = slots
            .into_iter()
            .map(|(media, at_unix)| AiringRow {
                media,
                episode: 0,
                at_unix,
                adult: false,
                popularity: 0,
            })
            .collect();
        return Ok(slots_from_rows(&rows, &tracked, adult));
    }
    Ok(slots_from_rows(&rows, &tracked, adult))
}

pub async fn tracked_shows(b: &LiveBackend) -> UiResult<Vec<MediaVm>> {
    Ok(b.store
        .tracked_shows()
        .map_err(store_err)?
        .iter()
        .map(tracked_vm)
        .collect())
}

pub async fn is_tracked(b: &LiveBackend, media_id: &str) -> UiResult<bool> {
    b.store.is_tracked(media_id).map_err(store_err)
}

pub async fn set_tracked(b: &LiveBackend, media_id: &str, tracked: bool) -> UiResult<()> {
    if !tracked {
        return b.store.remove_tracked(media_id).map_err(store_err);
    }
    let known = b.titles.lock().ok().and_then(|t| t.get(media_id).cloned());
    let (title, poster) = match known {
        Some(t) => t,
        None => {
            let (_, m) = b.media(media_id).await?;
            (m.title.clone(), m.cover.clone())
        }
    };
    let row = TrackedShowRow {
        media_id: media_id.to_owned(),
        title,
        poster,
        anilist_id: media_ref(media_id).and_then(|r| r.anilist_id),
        added_at: now_unix(),
    };
    b.store.set_tracked(&row).map_err(store_err)
}

pub async fn reminders_due(b: &LiveBackend) -> UiResult<Vec<AiringSlotVm>> {
    let p = prefs(b);
    if !p.airing_reminders {
        return Ok(Vec::new());
    }
    let tracked = tracked_rows(b);
    let ids: Vec<u32> = tracked
        .iter()
        .filter_map(|t| t.anilist_id)
        .filter(|i| *i > 0)
        .collect();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let now = now_unix();
    let to = now + i64::from(p.reminder_minutes) * 60;
    let mut due = Vec::new();
    for chunk in ids.chunks(50) {
        let page = anilist()
            .airing_schedules(now - REMINDER_GRACE_SECS, to, Some(chunk), false, 1)
            .await
            .map_err(provider_err)?;
        // The user tracked these titles themselves: 18+ is not filtered.
        for slot in slots_from_rows(&page.rows, &tracked, true) {
            if b.store
                .reminder_announced(&slot.media.id, slot.episode, now)
                .map_err(store_err)?
            {
                due.push(slot);
            }
        }
    }
    Ok(due)
}

// ---------------------------------------------------------------------------------------------
// Recommendations
// ---------------------------------------------------------------------------------------------

/// The viewer's taste, rebuilt at most every [`TASTE_TTL`] (or after a "Like").
struct Taste {
    profile: TasteProfile,
    /// AniList ids of seen titles + Planning (never recommended).
    excluded: HashSet<u32>,
    /// Seen titles with their genres (Atlas).
    seen: Vec<(MediaVm, Vec<String>)>,
    built: Instant,
}

static TASTE: Mutex<Option<Arc<Taste>>> = Mutex::new(None);

fn invalidate_taste() {
    if let Ok(mut t) = TASTE.lock() {
        *t = None;
    }
}

async fn taste(b: &LiveBackend) -> UiResult<Arc<Taste>> {
    if let Some(t) = TASTE
        .lock()
        .ok()
        .and_then(|t| t.as_ref().filter(|t| t.built.elapsed() < TASTE_TTL).cloned())
    {
        return Ok(t);
    }
    // id -> (weight, card)
    let mut weights: HashMap<u32, (f32, MediaVm)> = HashMap::new();
    let mut excluded: HashSet<u32> = HashSet::new();
    let bump = |id: u32, w: f32, vm: MediaVm, weights: &mut HashMap<u32, (f32, MediaVm)>| {
        let e = weights.entry(id).or_insert((0.0, vm));
        e.0 = e.0.max(w);
    };
    for e in b.store.history(TASTE_HISTORY).map_err(store_err)? {
        if let Some(id) = e.anilist_id.filter(|i| *i > 0) {
            bump(
                id,
                recs::seen_weight(None, e.completed, false),
                history_media(&e),
                &mut weights,
            );
        }
    }
    for s in LIST_SERVICES {
        for row in b.store.list_entries(s, None).unwrap_or_default() {
            let Some(id) = row.anilist_id.filter(|i| *i > 0) else {
                continue;
            };
            if row.status == "planning" {
                excluded.insert(id);
                continue;
            }
            let vm = MediaVm {
                id: row.media_id.clone(),
                title: row.title.clone(),
                cover: row.poster.clone(),
                color: None,
                subtitle: String::new(),
                score: None,
            };
            bump(
                id,
                recs::seen_weight(row.score, row.status == "completed", row.status == "dropped"),
                vm,
                &mut weights,
            );
        }
    }
    for f in b.store.recs_feedback().unwrap_or_default() {
        if f.feedback == RecsFeedbackVm::Like.id()
            && let Ok(id) = f.media_id.parse::<u32>()
        {
            let vm = MediaVm {
                id: f.media_id.clone(),
                title: String::new(),
                cover: None,
                color: None,
                subtitle: String::new(),
                score: None,
            };
            bump(id, 1.0, vm, &mut weights);
        }
    }
    let mut ids: Vec<(u32, f32)> = weights.iter().map(|(id, (w, _))| (*id, *w)).collect();
    ids.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });
    ids.truncate(TASTE_MAX_TITLES);
    let mut features: Vec<Candidate> = Vec::new();
    let all_ids: Vec<u32> = ids.iter().map(|(id, _)| *id).collect();
    for chunk in all_ids.chunks(50) {
        features.extend(anilist().features(chunk).await.map_err(provider_err)?);
    }
    let by_id: HashMap<u32, &Candidate> = features.iter().map(|c| (c.anilist_id, c)).collect();
    let mut seen_rows = Vec::new();
    let mut seen_cards = Vec::new();
    for (id, w) in &ids {
        let Some(c) = by_id.get(id) else { continue };
        seen_rows.push(Seen {
            anilist_id: *id,
            genres: c.genres.clone(),
            studios: c.studios.clone(),
            year: c.year,
            tags: c.tags.clone(),
            weight: *w,
        });
        if let Some((_, vm)) = weights.get(id)
            && !vm.title.is_empty()
        {
            seen_cards.push((vm.clone(), c.genres.clone()));
        }
    }
    excluded.extend(weights.keys().copied());
    let t = Arc::new(Taste {
        profile: TasteProfile::build(&seen_rows),
        excluded,
        seen: seen_cards,
        built: Instant::now(),
    });
    if let Ok(mut slot) = TASTE.lock() {
        *slot = Some(t.clone());
    }
    Ok(t)
}

fn feedback_rows(b: &LiveBackend) -> Vec<(String, String, i64)> {
    b.store
        .recs_feedback()
        .unwrap_or_default()
        .into_iter()
        .map(|r| (r.media_id, r.feedback, r.created_at))
        .collect()
}

/// One page of scored recommendations with the genres of each (for the Atlas).
async fn recommend(b: &LiveBackend, page: u32) -> UiResult<Vec<(RecommendationVm, Vec<String>)>> {
    let adult = prefs(b).adult_in_rows();
    let taste = taste(b).await?;
    if taste.profile.is_empty() {
        return Ok(Vec::new());
    }
    let hidden = dismissed(&feedback_rows(b), now_unix());
    let mut excluded = taste.excluded.clone();
    excluded.extend(hidden.iter().filter_map(|id| id.parse::<u32>().ok()));
    let mut exclude_list: Vec<u32> = excluded.iter().copied().collect();
    exclude_list.sort_unstable();
    let found = anilist()
        .discover(&taste.profile.top_genres(3), &exclude_list, page.max(1), adult)
        .await
        .map_err(provider_err)?;
    let found: Vec<(Media, Candidate, bool)> = found.into_iter().filter(|(_, _, a)| adult || !*a).collect();
    let cands: Vec<Candidate> = found.iter().map(|(_, c, _)| c.clone()).collect();
    Ok(recs::rank(&taste.profile, &cands, &excluded)
        .into_iter()
        .map(|r| {
            let (media, c, _) = &found[r.index];
            let vm = RecommendationVm {
                media: to_vm(media),
                score: r.score,
                reasons: r.reasons.iter().map(recs::Reason::token).collect(),
                atlas: None,
            };
            (vm, c.genres.clone())
        })
        .filter(|(vm, _)| !hidden.contains(&vm.media.id))
        .collect())
}

pub async fn recommendations(b: &LiveBackend, page: u32) -> UiResult<Vec<RecommendationVm>> {
    let mut list = recommend(b, page).await?;
    // Atlas position of each recommendation among themselves (the Atlas tab adds the seen titles).
    let points = recs::atlas(&list.iter().map(|(_, g)| g.clone()).collect::<Vec<_>>());
    for ((vm, _), p) in list.iter_mut().zip(points) {
        vm.atlas = Some((p.x, p.y));
    }
    Ok(list.into_iter().map(|(vm, _)| vm).collect())
}

pub async fn recs_feedback(b: &LiveBackend, media_id: &str, feedback: RecsFeedbackVm) -> UiResult<()> {
    b.store
        .set_recs_feedback(media_id, feedback.id(), now_unix())
        .map_err(store_err)?;
    if feedback == RecsFeedbackVm::Like {
        invalidate_taste();
    }
    Ok(())
}

pub async fn taste_profile(b: &LiveBackend) -> UiResult<TasteProfileVm> {
    let t = taste(b).await?;
    let rows = b.store.recs_feedback().unwrap_or_default();
    let likes = rows
        .iter()
        .filter(|r| r.feedback == RecsFeedbackVm::Like.id())
        .count() as u32;
    let dislikes = rows
        .iter()
        .filter(|r| r.feedback == RecsFeedbackVm::NotInterested.id())
        .count() as u32;
    Ok(TasteProfileVm {
        genres: t.profile.genres.clone(),
        studios: t.profile.studios.clone(),
        eras: t.profile.eras.clone(),
        themes: t.profile.tags.clone(),
        sample_size: t.profile.sample_size,
        accuracy: recs::accuracy(likes, dislikes, ACCURACY_MIN_VOTES),
    })
}

pub async fn atlas_points(b: &LiveBackend) -> UiResult<Vec<AtlasPointVm>> {
    let t = taste(b).await?;
    let mut items: Vec<(MediaVm, Vec<String>)> = t.seen.iter().take(ATLAS_SEEN).cloned().collect();
    items.extend(recommend(b, 1).await?.into_iter().map(|(r, g)| (r.media, g)));
    let points = recs::atlas(&items.iter().map(|(_, g)| g.clone()).collect::<Vec<_>>());
    Ok(items
        .into_iter()
        .zip(points)
        .map(|((media, _), p)| AtlasPointVm {
            media,
            x: p.x,
            y: p.y,
            cluster: p.cluster,
        })
        .collect())
}

// ---------------------------------------------------------------------------------------------
// External facts (Details) and provider health
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
struct Health {
    last_ok_unix: Option<i64>,
    last_error: Option<String>,
    probed: Option<Instant>,
}

static HEALTH: Mutex<Option<HashMap<&'static str, Health>>> = Mutex::new(None);
/// When each provider was first seen cooling down (the router does not expose the remaining time).
static COOLING_SINCE: Mutex<Option<HashMap<&'static str, Instant>>> = Mutex::new(None);

fn static_id(id: &str) -> Option<&'static str> {
    PROVIDERS.iter().copied().find(|p| *p == id)
}

fn record(id: &str, r: &Result<(), String>, probed: bool) {
    let Some(id) = static_id(id) else { return };
    if let Ok(mut m) = HEALTH.lock() {
        let h = m.get_or_insert_with(HashMap::new).entry(id).or_default();
        match r {
            Ok(()) => {
                h.last_ok_unix = Some(now_unix());
                h.last_error = None;
            }
            Err(e) => h.last_error = Some(e.clone()),
        }
        if probed {
            h.probed = Some(Instant::now());
        }
    }
}

fn record_result<T>(id: &str, r: &Result<T, ProviderError>) {
    let outcome = match r {
        Ok(_) => Some(Ok(())),
        Err(e) => probe_outcome(Err(e.clone()), id),
    };
    if let Some(o) = outcome {
        record(id, &o, false);
    }
}

/// MAL, Shikimori and Tenrai facts of a MAL id, in the user's priority order (disabled providers skipped).
/// External facts are reused this long per MAL id (Details is opened back and forth).
const FACTS_TTL: Duration = Duration::from_secs(30 * 60);
const FACTS_CACHE_MAX: usize = 128;

type FactsCache = HashMap<u32, (Instant, Vec<ExternalFacts>)>;
static FACTS: Mutex<Option<FactsCache>> = Mutex::new(None);

/// MAL, Shikimori and Tenrai facts of a MAL id, in the user's priority order (disabled or cooling providers skipped),
/// cached [`FACTS_TTL`].
pub(super) async fn external_facts(b: &LiveBackend, mal_id: u32) -> Vec<ExternalFacts> {
    if let Some(hit) = FACTS.lock().ok().and_then(|c| {
        c.as_ref()
            .and_then(|m| m.get(&mal_id))
            .filter(|(at, _)| at.elapsed() < FACTS_TTL)
            .map(|(_, v)| v.clone())
    }) {
        return hit;
    }
    let s = settings_store::load(&b.settings_file);
    let order = s.home.enabled_providers(&s.provider_priority);
    let cooling = b.router.cooling_down();
    let mut out = Vec::new();
    for id in order
        .iter()
        .filter(|id| matches!(id.as_str(), "mal" | "shikimori" | "tenrai"))
    {
        if cooling.iter().any(|c| *c == id.as_str()) {
            continue;
        }
        let r = match id.as_str() {
            "mal" => mal().facts(mal_id).await,
            "shikimori" => shikimori().facts(mal_id).await,
            _ => tenrai().facts(mal_id).await,
        };
        record_result(id, &r);
        if let Ok(Some(f)) = r {
            out.push(f);
        }
    }
    if let Ok(mut c) = FACTS.lock() {
        let m = c.get_or_insert_with(HashMap::new);
        if m.len() >= FACTS_CACHE_MAX {
            m.retain(|_, (at, _)| at.elapsed() < FACTS_TTL);
        }
        if m.len() < FACTS_CACHE_MAX {
            m.insert(mal_id, (Instant::now(), out.clone()));
        }
    }
    out
}

/// Fill the AniList facts of Details from the external catalogues (`metadata.providers.*`): mean score when AniList
/// has none, alternative titles merged. Returns the merged facts (rank / popularity / members / screenshots for the
/// fields `MediaFactsVm` gains at integration) and the providers that contributed (`data-facts-source`).
pub(super) async fn enrich_extra(
    b: &LiveBackend,
    vm: &mut DetailsExtraVm,
) -> (ExternalFacts, Vec<&'static str>) {
    let mut base = ExternalFacts {
        provider: "anilist",
        score: vm.facts.mean_score,
        titles: vm.facts.synonyms.clone(),
        ..ExternalFacts::default()
    };
    let Some(mal_id) = vm.facts.mal_id.filter(|i| *i > 0) else {
        return (base, Vec::new());
    };
    let others = external_facts(b, mal_id).await;
    let used = merge_facts(&mut base, &others);
    vm.facts.mean_score = base.score;
    vm.facts.synonyms = base.titles.clone();
    (base, used)
}

/// Synopsis of a MAL id from the external catalogues, for titles AniList has none for.
pub(super) async fn external_synopsis(b: &LiveBackend, mal_id: Option<u32>) -> Option<String> {
    let id = mal_id.filter(|i| *i > 0)?;
    external_facts(b, id).await.into_iter().find_map(|f| f.synopsis)
}

async fn probe(id: &'static str) -> (&'static str, Option<Result<(), String>>) {
    let r = tokio::time::timeout(PROBE_TIMEOUT, async {
        match id {
            "anilist" => anilist().genres().await.map(|_| ()),
            "kitsu" => Kitsu::new(client().clone())
                .feed(MediaKind::Anime, FeedKind::Trending, 1)
                .await
                .map(|_| ()),
            "anizip" => AniZip::new(client().clone())
                .episodes(&MediaRef::anilist(PROBE_MAL_ID))
                .await
                .map(|_| ()),
            "mal" => mal().facts(PROBE_MAL_ID).await.map(|_| ()),
            "shikimori" => shikimori().facts(PROBE_MAL_ID).await.map(|_| ()),
            "tenrai" => tenrai().facts(PROBE_MAL_ID).await.map(|_| ()),
            "mangadex" => MangaDex::new(client().clone())
                .feed(MediaKind::Manga, FeedKind::Trending, 1)
                .await
                .map(|_| ()),
            "mangaupdates" => MangaUpdates::new(client().clone())
                .search("one piece")
                .await
                .map(|_| ()),
            _ => Err(ProviderError::Unsupported),
        }
    })
    .await
    .unwrap_or_else(|_| Err(ProviderError::Network("timed out".into())));
    (id, probe_outcome(r, id))
}

fn needs_probe(id: &str) -> bool {
    HEALTH
        .lock()
        .ok()
        .and_then(|m| m.as_ref().and_then(|m| m.get(id)).and_then(|h| h.probed))
        .is_none_or(|at| at.elapsed() >= PROBE_TTL)
}

fn kinds_of(id: &str) -> Vec<String> {
    match id {
        "anilist" | "mal" => vec!["anime".into(), "manga".into()],
        "mangadex" | "mangaupdates" => vec!["manga".into()],
        "anizip" => vec!["episodes".into()],
        _ => vec!["anime".into()],
    }
}

pub async fn provider_health(b: &LiveBackend) -> UiResult<Vec<ProviderHealthVm>> {
    let s = settings_store::load(&b.settings_file);
    let enabled = s.home.enabled_providers(&s.provider_priority);
    let cooling = b.router.cooling_down();
    // Cool-down clock: first time seen cooling -> now; providers back in rotation are forgotten.
    let since: HashMap<&'static str, Instant> = match COOLING_SINCE.lock() {
        Ok(mut m) => {
            let m = m.get_or_insert_with(HashMap::new);
            m.retain(|id, _| cooling.contains(id));
            for id in &cooling {
                m.entry(*id).or_insert_with(Instant::now);
            }
            m.clone()
        }
        Err(_) => HashMap::new(),
    };
    // Probe the enabled, not cooling providers whose last probe is stale (concurrently).
    let want =
        |id: &'static str| enabled.iter().any(|e| e == id) && !cooling.contains(&id) && needs_probe(id);
    let want = &want;
    let maybe = |id: &'static str| async move { if want(id) { Some(probe(id).await) } else { None } };
    let results = tokio::join!(
        maybe("anilist"),
        maybe("kitsu"),
        maybe("anizip"),
        maybe("mal"),
        maybe("shikimori"),
        maybe("tenrai"),
        maybe("mangadex"),
        maybe("mangaupdates")
    );
    for (id, outcome) in [
        results.0, results.1, results.2, results.3, results.4, results.5, results.6, results.7,
    ]
    .into_iter()
    .flatten()
    {
        match outcome {
            Some(o) => record(id, &o, true),
            None => record(id, &Ok(()), true),
        }
    }
    let health = HEALTH.lock().ok().and_then(|m| m.clone()).unwrap_or_default();
    Ok(s.provider_priority
        .iter()
        .filter_map(|id| static_id(id))
        .map(|id| {
            let h = health.get(id).cloned().unwrap_or_default();
            let is_cooling = cooling.contains(&id);
            ProviderHealthVm {
                id: id.to_owned(),
                enabled: enabled.iter().any(|e| e == id),
                cooling_down: is_cooling,
                cooling_secs: is_cooling
                    .then(|| {
                        since
                            .get(id)
                            .map_or(COOLDOWN, |at| COOLDOWN.saturating_sub(at.elapsed()))
                    })
                    .map(|d| u32::try_from(d.as_secs()).unwrap_or(u32::MAX)),
                kinds: kinds_of(id),
                last_ok_unix: h.last_ok_unix,
                last_error: h.last_error,
                discontinued: false,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn media(id: u32, title: &str) -> Media {
        Media {
            media_ref: MediaRef::anilist(id),
            kind: MediaKind::Anime,
            title: title.into(),
            titles: Default::default(),
            cover: None,
            banner: None,
            color: None,
            synopsis: None,
            genres: vec![],
            episodes: None,
            status: None,
            season: None,
            year: None,
            score: None,
            source_provider: "anilist".into(),
        }
    }

    fn row(id: u32, ep: u32, at: i64, adult: bool) -> AiringRow {
        AiringRow {
            media: media(id, &format!("T{id}")),
            episode: ep,
            at_unix: at,
            adult,
            popularity: 0,
        }
    }

    #[test]
    fn latest_keeps_one_card_per_title_and_drops_adult() {
        let rows = vec![
            row(1, 6, 300, false),
            row(1, 5, 200, false),
            row(2, 1, 100, true),
            row(3, 2, 50, false),
        ];
        let v = latest_vms(&rows, false);
        assert_eq!(
            v.iter()
                .map(|l| (l.media.id.as_str(), l.episode))
                .collect::<Vec<_>>(),
            [("1", 6), ("3", 2)]
        );
        assert_eq!(latest_vms(&rows, true).len(), 3);
    }

    #[test]
    fn followed_window_is_bounded() {
        let now = 100 * 86_400;
        assert_eq!(followed_since(None, now), now - FOLLOWED_DEFAULT_SECS);
        assert_eq!(followed_since(Some(now - 60), now), now - FOLLOWED_MIN_SECS);
        assert_eq!(followed_since(Some(0), now), now - FOLLOWED_MAX_SECS);
        assert_eq!(followed_since(Some(now - 3 * 86_400), now), now - 3 * 86_400);
    }

    #[test]
    fn visits_roll_after_a_gap() {
        assert_eq!(next_visit(None, None, 1000), (1000, None));
        assert_eq!(next_visit(Some(1000), None, 1000 + 60), (1000, None));
        let later = 1000 + VISIT_GAP_SECS + 1;
        assert_eq!(next_visit(Some(1000), None, later), (later, Some(1000)));
        assert_eq!(
            next_visit(Some(later), Some(1000), later + 10),
            (later, Some(1000))
        );
    }

    #[test]
    fn weekdays_and_jst_slots() {
        // 1970-01-01 00:00 UTC = Thursday.
        assert_eq!(weekday_at(0, 0), 3);
        // 2026-11-09 is a Monday; 00:00 UTC.
        let monday = 1_794_182_400;
        assert_eq!(weekday_at(monday, 0), 0);
        // Sunday 23:15 JST = Sunday 14:15 UTC: inside a week starting Monday 00:00 UTC.
        let t = jst_slot_in_week(monday, 6, 23, 15).unwrap();
        assert_eq!(t, monday + 6 * 86_400 + 14 * 3600 + 15 * 60);
        // Monday 01:00 JST of this week is Sunday 16:00 UTC, before the start: the slot is the next one, Sunday 16:00
        // UTC at the end of this week.
        let t = jst_slot_in_week(monday, 0, 1, 0).unwrap();
        assert_eq!(t, monday + 6 * 86_400 + 16 * 3600);
        assert!((monday..monday + 7 * 86_400).contains(&t));
    }

    #[test]
    fn calendar_slots_are_sorted_tracked_and_deduped() {
        let tracked = vec![TrackedShowRow {
            media_id: "2".into(),
            title: "T2".into(),
            poster: None,
            anilist_id: Some(2),
            added_at: 0,
        }];
        let rows = vec![
            row(2, 3, 500, false),
            row(1, 1, 100, false),
            row(1, 1, 100, false),
            row(9, 1, 50, true),
        ];
        let s = slots_from_rows(&rows, &tracked, false);
        assert_eq!(
            s.iter()
                .map(|x| (x.media.id.as_str(), x.tracked))
                .collect::<Vec<_>>(),
            [("1", false), ("2", true)]
        );
    }

    #[test]
    fn genre_cloud_hides_the_adult_genre() {
        let all = vec!["Action".to_owned(), ADULT_GENRE.to_owned(), "Drama".to_owned()];
        assert_eq!(visible_genres(&all, false), ["Action", "Drama"]);
        assert_eq!(visible_genres(&all, true).len(), 3);
    }

    #[test]
    fn top_n_filters_flagged_adult_titles() {
        let items = vec![media(1, "a"), media(2, "b"), media(3, "c")];
        let flagged: HashSet<u32> = [2].into_iter().collect();
        assert_eq!(
            top_n(&items, &flagged, false, 10)
                .iter()
                .map(|m| m.id.as_str())
                .collect::<Vec<_>>(),
            ["1", "3"]
        );
        assert_eq!(top_n(&items, &flagged, true, 2).len(), 2);
        assert_eq!(range_secs(TopRangeVm::Week), 7 * 86_400);
    }

    #[test]
    fn feedback_dismisses_not_interested_forever_and_skip_for_a_while() {
        let now = 100 * 86_400;
        let rows = vec![
            ("1".to_owned(), "not_interested".to_owned(), 0),
            ("2".to_owned(), "skip".to_owned(), now - 86_400),
            ("3".to_owned(), "skip".to_owned(), now - SKIP_SECS - 1),
            ("4".to_owned(), "like".to_owned(), now),
        ];
        let d = dismissed(&rows, now);
        assert!(d.contains("1") && d.contains("2"));
        assert!(!d.contains("3") && !d.contains("4"));
    }

    #[test]
    fn facts_merge_fills_gaps_in_order() {
        let mut base = ExternalFacts {
            provider: "anilist",
            score: Some(80),
            ..ExternalFacts::default()
        };
        let mal = ExternalFacts {
            provider: "mal",
            score: Some(87),
            rank: Some(62),
            synopsis: Some("Pirates.".into()),
            titles: vec!["OP".into()],
            ..ExternalFacts::default()
        };
        let shiki = ExternalFacts {
            provider: "shikimori",
            score: Some(90),
            titles: vec!["OP".into(), "Ван-Пис".into()],
            screenshots: vec!["s.jpg".into()],
            ..ExternalFacts::default()
        };
        let tenrai = ExternalFacts {
            provider: "tenrai",
            rank: Some(1),
            ..ExternalFacts::default()
        };
        let used = merge_facts(&mut base, &[mal, shiki, tenrai]);
        assert_eq!(used, ["mal", "shikimori"]);
        assert_eq!(
            (base.score, base.rank, base.synopsis.as_deref()),
            (Some(80), Some(62), Some("Pirates."))
        );
        assert_eq!(base.titles, ["OP", "Ван-Пис"]);
        assert_eq!(base.screenshots, ["s.jpg"]);
    }

    #[test]
    fn probe_outcomes() {
        assert_eq!(probe_outcome(Ok(()), "kitsu"), Some(Ok(())));
        assert_eq!(
            probe_outcome(Err(ProviderError::Unsupported), "mal"),
            Some(Err(NO_CLIENT_ID.to_owned()))
        );
        assert_eq!(probe_outcome(Err(ProviderError::Unsupported), "mangadex"), None);
        assert!(matches!(
            probe_outcome(Err(ProviderError::Http(503)), "tenrai"),
            Some(Err(_))
        ));
    }

    #[test]
    fn broadcasts_outside_jst_are_ignored() {
        let monday = 1_794_182_400;
        let b = |tz: &str| Broadcast {
            media: media(5, "x"),
            weekday: 2,
            hour: 12,
            minute: 0,
            timezone: tz.into(),
        };
        assert_eq!(broadcast_slots(&[b("Asia/Tokyo"), b("UTC")], monday).len(), 1);
    }
}
