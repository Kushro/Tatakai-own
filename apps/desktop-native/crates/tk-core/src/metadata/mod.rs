//! Metadata providers and the router that falls back between them.
//!
//! Semantics follow the legacy `src/lib/metadata/router.ts`: providers are tried in user priority order, a provider
//! that fails with a transport/server error is skipped for [`COOLDOWN`] (60 s), an empty result falls through to the
//! next provider (and is returned only if nobody does better), and when *every* candidate is cooling down they are
//! retried anyway as a last resort.

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use async_trait::async_trait;

use crate::error::ProviderError;
use crate::model::{Episode, FeedKind, FeedPage, Media, MediaKind, MediaRef};

pub mod anilist;
pub mod anizip;
pub mod kitsu;
pub mod mapping;
pub mod merge;
// wave B (docs/redesign/16 section 3.5): MAL, Shikimori, Tenrai (worker b; plan 14 section 0.1 #13) and the manga-only
// MangaDex / MangaUpdates (worker l). Each file is its owner's; this file only registers them.
pub mod mal;
pub mod mangadex;
pub mod mangaupdates;
pub mod shikimori;
pub mod tenrai;

pub use anilist::AniList;
pub use anizip::AniZip;
pub use kitsu::Kitsu;
pub use mal::Mal;
pub use mangadex::MangaDex;
pub use mangaupdates::MangaUpdates;
pub use shikimori::Shikimori;
pub use tenrai::Tenrai;

/// How long a provider is skipped after a transport/server failure.
pub const COOLDOWN: Duration = Duration::from_secs(60);

/// Default provider priority.
pub fn default_priority() -> Vec<String> {
    ["anilist", "kitsu", "anizip"].iter().map(|s| (*s).to_owned()).collect()
}

#[async_trait]
pub trait MetadataProvider: Send + Sync {
    fn id(&self) -> &'static str;
    /// Kinds this provider can serve for `feed` / `search` / `media`. `episodes` ignores this.
    fn kinds(&self) -> &[MediaKind];

    async fn feed(&self, _k: MediaKind, _f: FeedKind, _page: u32) -> Result<FeedPage, ProviderError> {
        Err(ProviderError::Unsupported)
    }
    async fn search(&self, _k: MediaKind, _q: &str, _page: u32) -> Result<FeedPage, ProviderError> {
        Err(ProviderError::Unsupported)
    }
    async fn media(&self, _k: MediaKind, _r: &MediaRef) -> Result<Option<Media>, ProviderError> {
        Err(ProviderError::Unsupported)
    }
    async fn episodes(&self, _r: &MediaRef) -> Result<Vec<Episode>, ProviderError> {
        Err(ProviderError::Unsupported)
    }
    /// 0.3.1: how long this failure should take the provider out of rotation, when the provider knows better than
    /// the fixed [`COOLDOWN`] (AniList: exactly the `Retry-After` / `X-RateLimit-Reset` of its 429).
    fn cooldown_for(&self, _err: &ProviderError) -> Option<Duration> {
        None
    }
}

pub struct MetadataRouter {
    providers: Vec<Arc<dyn MetadataProvider>>,
    priority: RwLock<Vec<String>>,
    cooldown: Duration,
    start: Instant,
    /// Test hook: virtual time added to `Instant::now()`.
    skew_ms: AtomicU64,
    down_until: Mutex<HashMap<&'static str, u64>>,
}

impl MetadataRouter {
    pub fn new(providers: Vec<Arc<dyn MetadataProvider>>, priority: Vec<String>) -> Self {
        Self {
            providers,
            priority: RwLock::new(priority),
            cooldown: COOLDOWN,
            start: Instant::now(),
            skew_ms: AtomicU64::new(0),
            down_until: Mutex::new(HashMap::new()),
        }
    }

    /// Every known provider (M0 three + wave B five) with the user's priority. A provider absent from `priority`
    /// (the user removed it in Settings > Metadata, or `tk_ui::model::sanitize_priority` did not list it yet) is
    /// never asked: `ordered` ranks unknown ids last but `set_priority` callers pass the full list, so "disabled"
    /// = filtered out here by id.
    pub fn with_defaults(client: reqwest::Client, priority: Vec<String>) -> Self {
        let providers: Vec<Arc<dyn MetadataProvider>> = vec![
            Arc::new(AniList::new(client.clone())),
            Arc::new(Kitsu::new(client.clone())),
            Arc::new(AniZip::new(client.clone())),
            Arc::new(Mal::new(client.clone())),
            Arc::new(Shikimori::new(client.clone())),
            Arc::new(Tenrai::new(client.clone())),
            Arc::new(MangaDex::new(client.clone())),
            Arc::new(MangaUpdates::new(client)),
        ];
        Self::new(providers, priority)
    }

    pub fn set_priority(&self, priority: Vec<String>) {
        if let Ok(mut p) = self.priority.write() {
            *p = priority;
        }
    }

    /// The provider asked first (the user's top priority): an answer from any other one is a fallback.
    pub fn primary(&self) -> Option<String> {
        self.priority.read().ok().and_then(|p| p.first().cloned())
    }

    /// Clear all cool-downs (e.g. the user reordered sources or hit retry).
    pub fn reset_health(&self) {
        if let Ok(mut m) = self.down_until.lock() {
            m.clear();
        }
    }

    /// Provider ids currently cooling down.
    pub fn cooling_down(&self) -> Vec<&'static str> {
        let now = self.now_ms();
        self.down_until
            .lock()
            .map(|m| m.iter().filter(|(_, until)| **until > now).map(|(id, _)| *id).collect())
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub(crate) fn advance(&self, d: Duration) {
        self.skew_ms.fetch_add(d.as_millis() as u64, Ordering::SeqCst);
    }

    fn now_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64 + self.skew_ms.load(Ordering::SeqCst)
    }

    fn ordered(&self, kind: Option<MediaKind>, include_cooling: bool) -> Vec<Arc<dyn MetadataProvider>> {
        let prio = self.priority.read().map(|p| p.clone()).unwrap_or_default();
        let rank = |id: &str| prio.iter().position(|p| p == id).unwrap_or(usize::MAX);
        let now = self.now_ms();
        let down = self.down_until.lock().map(|m| m.clone()).unwrap_or_default();
        let mut v: Vec<_> = self
            .providers
            .iter()
            .filter(|p| kind.is_none_or(|k| p.kinds().contains(&k)))
            // Wave B (b): a provider missing from the priority (switched off in Settings > Metadata) is never asked.
            .filter(|p| prio.iter().any(|x| x == p.id()))
            .filter(|p| include_cooling || down.get(p.id()).copied().unwrap_or(0) <= now)
            .cloned()
            .collect();
        v.sort_by_key(|p| rank(p.id()));
        v
    }

    fn mark_failure(&self, p: &dyn MetadataProvider, err: &ProviderError) {
        let id = p.id();
        if err.trips_cooldown() {
            let d = p.cooldown_for(err).unwrap_or(self.cooldown);
            let until = self.now_ms() + d.as_millis() as u64;
            if let Ok(mut m) = self.down_until.lock() {
                m.insert(id, until);
            }
        }
    }

    fn mark_ok(&self, id: &'static str) {
        if let Ok(mut m) = self.down_until.lock() {
            m.remove(id);
        }
    }

    async fn route<T, F, Fut>(&self, kind: Option<MediaKind>, is_empty: fn(&T) -> bool, call: F) -> Result<T, ProviderError>
    where
        F: Fn(Arc<dyn MetadataProvider>) -> Fut,
        Fut: Future<Output = Result<T, ProviderError>>,
    {
        let mut candidates = self.ordered(kind, false);
        if candidates.is_empty() {
            candidates = self.ordered(kind, true);
        }
        let mut last_err: Option<ProviderError> = None;
        let mut empty: Option<T> = None;
        for p in candidates {
            let id = p.id();
            match call(p.clone()).await {
                Ok(v) => {
                    self.mark_ok(id);
                    if is_empty(&v) {
                        empty.get_or_insert(v);
                        continue;
                    }
                    return Ok(v);
                }
                Err(ProviderError::Unsupported) => {}
                Err(e) => {
                    self.mark_failure(p.as_ref(), &e);
                    // BUG-143: visible at the default level, so a 429 behind a fallback answer can be diagnosed.
                    tracing::info!(provider = id, error = %e, "metadata provider failed, falling back");
                    last_err = Some(e);
                }
            }
        }
        match (empty, last_err) {
            (Some(v), _) => Ok(v),
            (None, Some(e)) => Err(e),
            (None, None) => Err(ProviderError::Unsupported),
        }
    }

    pub async fn feed(&self, k: MediaKind, f: FeedKind, page: u32) -> Result<FeedPage, ProviderError> {
        self.route(Some(k), |p: &FeedPage| p.items.is_empty(), |p| async move { p.feed(k, f, page).await })
            .await
    }

    pub async fn search(&self, k: MediaKind, q: &str, page: u32) -> Result<FeedPage, ProviderError> {
        let q = q.to_owned();
        self.route(Some(k), |p: &FeedPage| p.items.is_empty(), |p| {
            let q = q.clone();
            async move { p.search(k, &q, page).await }
        })
        .await
    }

    pub async fn media(&self, k: MediaKind, r: &MediaRef) -> Result<Option<Media>, ProviderError> {
        let r = *r;
        self.route(Some(k), |m: &Option<Media>| m.is_none(), |p| async move { p.media(k, &r).await })
            .await
    }

    /// 0.3.2: the first provider with episodes gives the list; while some episode still lacks a title, thumbnail,
    /// date or synopsis, ani.zip (then the remaining providers, in priority order) fills it by number and adds the
    /// aired episodes the base does not list ([`merge::merge_episodes`]). A failing filler is skipped, never fatal.
    pub async fn episodes(&self, r: &MediaRef) -> Result<Vec<Episode>, ProviderError> {
        let today = time::OffsetDateTime::now_utc().date().to_string();
        self.episodes_on(r, &today).await
    }

    async fn episodes_on(&self, r: &MediaRef, today: &str) -> Result<Vec<Episode>, ProviderError> {
        let mut candidates = self.ordered(None, false);
        if candidates.is_empty() {
            candidates = self.ordered(None, true);
        }
        let mut last_err: Option<ProviderError> = None;
        let mut empty = false;
        let mut base: Option<(usize, Vec<Episode>)> = None;
        for (i, p) in candidates.iter().enumerate() {
            match p.episodes(r).await {
                Ok(v) => {
                    self.mark_ok(p.id());
                    if v.is_empty() {
                        empty = true;
                        continue;
                    }
                    base = Some((i, v));
                    break;
                }
                Err(ProviderError::Unsupported) => {}
                Err(e) => {
                    self.mark_failure(p.as_ref(), &e);
                    tracing::info!(provider = p.id(), error = %e, "metadata provider failed, falling back");
                    last_err = Some(e);
                }
            }
        }
        let Some((at, mut eps)) = base else {
            return match (empty, last_err) {
                (true, _) => Ok(Vec::new()),
                (false, Some(e)) => Err(e),
                (false, None) => Err(ProviderError::Unsupported),
            };
        };
        // Fillers: the providers after the base, ani.zip first (richest per-episode data, no AniList budget).
        let mut fillers: Vec<&Arc<dyn MetadataProvider>> = candidates[at + 1..].iter().collect();
        fillers.sort_by_key(|p| p.id() != "anizip");
        for p in fillers {
            if !merge::needs_fill(&eps) {
                break;
            }
            match p.episodes(r).await {
                Ok(extra) => {
                    self.mark_ok(p.id());
                    if !extra.is_empty() {
                        eps = merge::merge_episodes(eps, &extra, today);
                    }
                }
                Err(ProviderError::Unsupported) => {}
                Err(e) => {
                    self.mark_failure(p.as_ref(), &e);
                    tracing::info!(provider = p.id(), error = %e, "episode filler failed, keeping what we have");
                }
            }
        }
        Ok(eps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    struct Fake {
        id: &'static str,
        kinds: Vec<MediaKind>,
        calls: AtomicUsize,
        behaviour: Mutex<Result<Vec<Media>, ProviderError>>,
    }

    impl Fake {
        fn new(id: &'static str, b: Result<Vec<Media>, ProviderError>) -> Arc<Self> {
            Arc::new(Self { id, kinds: vec![MediaKind::Anime], calls: AtomicUsize::new(0), behaviour: Mutex::new(b) })
        }
        fn calls(&self) -> usize {
            self.calls.load(Ordering::SeqCst)
        }
    }

    fn media(title: &str, provider: &str) -> Media {
        Media { format: None,
            media_ref: MediaRef::anilist(1),
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
            source_provider: provider.into(),
        }
    }

    #[async_trait]
    impl MetadataProvider for Fake {
        fn id(&self) -> &'static str {
            self.id
        }
        fn kinds(&self) -> &[MediaKind] {
            &self.kinds
        }
        async fn feed(&self, _: MediaKind, _: FeedKind, page: u32) -> Result<FeedPage, ProviderError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let items = self.behaviour.lock().unwrap().clone()?;
            Ok(FeedPage { items, page, has_next: false })
        }
    }

    fn router(a: &Arc<Fake>, b: &Arc<Fake>) -> MetadataRouter {
        MetadataRouter::new(
            vec![b.clone() as Arc<dyn MetadataProvider>, a.clone() as Arc<dyn MetadataProvider>],
            vec!["a".into(), "b".into()],
        )
    }

    #[test]
    fn primary_is_the_first_priority() {
        let (a, b) = (Fake::new("a", Ok(Vec::new())), Fake::new("b", Ok(Vec::new())));
        let r = router(&a, &b);
        assert_eq!(r.primary().as_deref(), Some("a"));
        r.set_priority(vec!["b".into()]);
        assert_eq!(r.primary().as_deref(), Some("b"));
        r.set_priority(Vec::new());
        assert_eq!(r.primary(), None);
    }

    #[tokio::test]
    async fn uses_priority_order_not_registration_order() {
        let a = Fake::new("a", Ok(vec![media("from a", "a")]));
        let b = Fake::new("b", Ok(vec![media("from b", "b")]));
        let r = router(&a, &b);
        let page = r.feed(MediaKind::Anime, FeedKind::Trending, 1).await.unwrap();
        assert_eq!(page.items[0].source_provider, "a");
        assert_eq!(b.calls(), 0);
    }

    #[tokio::test]
    async fn falls_back_on_failure_and_cools_down_for_60s() {
        let a = Fake::new("a", Err(ProviderError::RateLimited { provider: "a".into() }));
        let b = Fake::new("b", Ok(vec![media("from b", "b")]));
        let r = router(&a, &b);
        let page = r.feed(MediaKind::Anime, FeedKind::Trending, 1).await.unwrap();
        assert_eq!(page.items[0].source_provider, "b");
        assert_eq!(a.calls(), 1);
        assert_eq!(r.cooling_down(), vec!["a"]);

        // still cooling down: a is skipped entirely
        r.advance(Duration::from_secs(59));
        r.feed(MediaKind::Anime, FeedKind::Trending, 1).await.unwrap();
        assert_eq!(a.calls(), 1);

        // cool-down over: a is tried again
        r.advance(Duration::from_secs(2));
        *a.behaviour.lock().unwrap() = Ok(vec![media("from a", "a")]);
        let page = r.feed(MediaKind::Anime, FeedKind::Trending, 1).await.unwrap();
        assert_eq!(page.items[0].source_provider, "a");
        assert_eq!(a.calls(), 2);
        assert!(r.cooling_down().is_empty());
    }

    #[tokio::test]
    async fn parse_errors_do_not_trip_cooldown() {
        let a = Fake::new("a", Err(ProviderError::Parse("bad".into())));
        let b = Fake::new("b", Ok(vec![media("from b", "b")]));
        let r = router(&a, &b);
        r.feed(MediaKind::Anime, FeedKind::Trending, 1).await.unwrap();
        r.feed(MediaKind::Anime, FeedKind::Trending, 1).await.unwrap();
        assert_eq!(a.calls(), 2);
    }

    #[tokio::test]
    async fn empty_result_falls_through_and_is_returned_if_nobody_has_data() {
        let a = Fake::new("a", Ok(vec![]));
        let b = Fake::new("b", Ok(vec![media("from b", "b")]));
        let r = router(&a, &b);
        let page = r.feed(MediaKind::Anime, FeedKind::Trending, 1).await.unwrap();
        assert_eq!(page.items.len(), 1);

        *b.behaviour.lock().unwrap() = Ok(vec![]);
        let page = r.feed(MediaKind::Anime, FeedKind::Trending, 1).await.unwrap();
        assert!(page.items.is_empty());
    }

    #[tokio::test]
    async fn all_cooling_down_retries_as_last_resort_and_surfaces_last_error() {
        let a = Fake::new("a", Err(ProviderError::Http(503)));
        let b = Fake::new("b", Err(ProviderError::Network("down".into())));
        let r = router(&a, &b);
        let err = r.feed(MediaKind::Anime, FeedKind::Trending, 1).await.unwrap_err();
        assert_eq!(err, ProviderError::Network("down".into()));
        assert_eq!(r.cooling_down().len(), 2);
        // second call: both are cooling, but are retried anyway
        r.feed(MediaKind::Anime, FeedKind::Trending, 1).await.unwrap_err();
        assert_eq!((a.calls(), b.calls()), (2, 2));
    }

    #[tokio::test]
    async fn kind_filter_skips_providers_without_the_kind() {
        let a = Fake::new("a", Ok(vec![media("x", "a")]));
        let b = Fake::new("b", Ok(vec![media("y", "b")]));
        let r = router(&a, &b);
        let err = r.feed(MediaKind::Manga, FeedKind::Trending, 1).await.unwrap_err();
        assert_eq!(err, ProviderError::Unsupported);
        assert_eq!(a.calls() + b.calls(), 0);
    }

    /// A provider that knows how long its rate limit lasts (AniList's `Retry-After`).
    struct Hinted;

    #[async_trait]
    impl MetadataProvider for Hinted {
        fn id(&self) -> &'static str {
            "h"
        }
        fn kinds(&self) -> &[MediaKind] {
            &[MediaKind::Anime]
        }
        async fn feed(&self, _k: MediaKind, _f: FeedKind, _p: u32) -> Result<FeedPage, ProviderError> {
            Err(ProviderError::RateLimited { provider: "h".into() })
        }
        fn cooldown_for(&self, _e: &ProviderError) -> Option<Duration> {
            Some(Duration::from_secs(7))
        }
    }

    #[tokio::test]
    async fn cools_down_for_the_wait_the_provider_reports() {
        let b = Fake::new("b", Ok(vec![media("from b", "b")]));
        let r = MetadataRouter::new(vec![Arc::new(Hinted), b.clone()], vec!["h".into(), "b".into()]);
        r.feed(MediaKind::Anime, FeedKind::Trending, 1).await.unwrap();
        assert_eq!(r.cooling_down(), vec!["h"]);
        r.advance(Duration::from_secs(6));
        assert_eq!(r.cooling_down(), vec!["h"]);
        r.advance(Duration::from_secs(2));
        assert!(r.cooling_down().is_empty());
    }

    /// Episode lists per provider id, for the merge routing tests.
    struct Eps {
        id: &'static str,
        list: Result<Vec<Episode>, ProviderError>,
        calls: AtomicUsize,
    }

    #[async_trait]
    impl MetadataProvider for Eps {
        fn id(&self) -> &'static str {
            self.id
        }
        fn kinds(&self) -> &[MediaKind] {
            &[]
        }
        async fn episodes(&self, _r: &MediaRef) -> Result<Vec<Episode>, ProviderError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.list.clone()
        }
    }

    fn ep(n: u32, title: Option<&str>, image: Option<&str>, date: Option<&str>) -> Episode {
        Episode {
            number: n,
            title: title.map(Into::into),
            image: image.map(Into::into),
            air_date: date.map(Into::into),
            is_filler: false,
            summary: title.map(|t| format!("about {t}")),
            runtime_min: None,
            rating: None,
        }
    }

    fn eps(id: &'static str, list: Result<Vec<Episode>, ProviderError>) -> Arc<Eps> {
        Arc::new(Eps { id, list, calls: AtomicUsize::new(0) })
    }

    #[tokio::test]
    async fn episodes_fill_from_anizip_before_the_next_providers() {
        let kitsu = eps("kitsu", Ok(vec![ep(1, None, None, Some("2026-10-01")), ep(2, None, None, Some("2026-10-08"))]));
        let broken = eps("tenrai", Err(ProviderError::Http(500)));
        let anizip = eps(
            "anizip",
            Ok(vec![
                ep(1, Some("One"), Some("i1"), Some("2026-10-01")),
                ep(2, Some("Two"), Some("i2"), Some("2026-10-08")),
                ep(3, Some("Three"), Some("i3"), Some("2026-10-15")),
            ]),
        );
        let r = MetadataRouter::new(
            vec![kitsu.clone(), broken.clone(), anizip.clone()],
            vec!["kitsu".into(), "tenrai".into(), "anizip".into()],
        );
        let out = r.episodes_on(&MediaRef::anilist(1), "2026-11-15").await.unwrap();
        assert_eq!(out.len(), 3);
        assert_eq!(out[1].title.as_deref(), Some("Two"));
        assert_eq!(out[1].image.as_deref(), Some("i2"));
        // ani.zip filled everything: the broken provider after it was never asked.
        assert_eq!((kitsu.calls.load(Ordering::SeqCst), anizip.calls.load(Ordering::SeqCst)), (1, 1));
        assert_eq!(broken.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn complete_base_asks_nobody_else_and_a_failing_filler_is_not_fatal() {
        let full = eps("kitsu", Ok(vec![ep(1, Some("A"), Some("i"), Some("2026-01-01"))]));
        let anizip = eps("anizip", Ok(vec![]));
        let r = MetadataRouter::new(vec![full.clone(), anizip.clone()], vec!["kitsu".into(), "anizip".into()]);
        assert_eq!(r.episodes_on(&MediaRef::anilist(1), "2026-11-15").await.unwrap().len(), 1);
        assert_eq!(anizip.calls.load(Ordering::SeqCst), 0);

        let bare = eps("kitsu", Ok(vec![ep(1, None, None, None)]));
        let down = eps("anizip", Err(ProviderError::Http(503)));
        let r = MetadataRouter::new(vec![bare, down], vec!["kitsu".into(), "anizip".into()]);
        assert_eq!(r.episodes_on(&MediaRef::anilist(1), "2026-11-15").await.unwrap().len(), 1);
        assert_eq!(r.cooling_down(), vec!["anizip"]);
    }

    #[tokio::test]
    async fn no_episodes_anywhere_keeps_the_old_answers() {
        let a = eps("kitsu", Ok(vec![]));
        let r = MetadataRouter::new(vec![a], vec!["kitsu".into()]);
        assert!(r.episodes_on(&MediaRef::anilist(1), "2026-11-15").await.unwrap().is_empty());
        let a = eps("kitsu", Err(ProviderError::Http(500)));
        let r = MetadataRouter::new(vec![a], vec!["kitsu".into()]);
        assert_eq!(r.episodes_on(&MediaRef::anilist(1), "2026-11-15").await.unwrap_err(), ProviderError::Http(500));
    }
}
