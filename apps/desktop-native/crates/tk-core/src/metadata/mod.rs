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

pub use anilist::AniList;
pub use anizip::AniZip;
pub use kitsu::Kitsu;

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

    /// The three M0 providers with the default priority.
    pub fn with_defaults(client: reqwest::Client, priority: Vec<String>) -> Self {
        let providers: Vec<Arc<dyn MetadataProvider>> = vec![
            Arc::new(AniList::new(client.clone())),
            Arc::new(Kitsu::new(client.clone())),
            Arc::new(AniZip::new(client)),
        ];
        Self::new(providers, priority)
    }

    pub fn set_priority(&self, priority: Vec<String>) {
        if let Ok(mut p) = self.priority.write() {
            *p = priority;
        }
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
            .filter(|p| include_cooling || down.get(p.id()).copied().unwrap_or(0) <= now)
            .cloned()
            .collect();
        v.sort_by_key(|p| rank(p.id()));
        v
    }

    fn mark_failure(&self, id: &'static str, err: &ProviderError) {
        if err.trips_cooldown() {
            let until = self.now_ms() + self.cooldown.as_millis() as u64;
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
            match call(p).await {
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
                    self.mark_failure(id, &e);
                    tracing::debug!(provider = id, error = %e, "metadata provider failed, falling back");
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

    pub async fn episodes(&self, r: &MediaRef) -> Result<Vec<Episode>, ProviderError> {
        let r = *r;
        self.route(None, |e: &Vec<Episode>| e.is_empty(), |p| async move { p.episodes(&r).await })
            .await
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
        Media {
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
}
