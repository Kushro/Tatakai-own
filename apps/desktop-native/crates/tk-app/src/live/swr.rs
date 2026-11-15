//! 0.3.1: stale-while-revalidate over the SQLite cache (`LocalStore::cache_*`), for AniList answers that are fine a
//! little old (Home rows, Top 10, latest episodes, the calendar).
//!
//! A value is stored with the time it was fetched and a long row lifetime ([`KEEP_SECS`]); [`freshness`] decides:
//! fresh -> answered as is; stale -> answered at once and refreshed in the background (one refresh per key at a
//! time, the next visit reads the new value); missing -> fetched and stored.

use std::collections::HashSet;
use std::future::Future;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use serde::de::DeserializeOwned;
use tk_core::LocalStore;
use tk_ui::backend::UiResult;

/// How long a stored answer is kept at all (a stale answer is still better than an empty Home offline).
pub const KEEP_SECS: i64 = 7 * 86_400;
/// Home rows / Top 10 / latest episodes are refreshed when older than this.
pub const HOME_FRESH_SECS: i64 = 15 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    Fresh,
    Stale,
}

/// `fetched_at` is fresh for `fresh_secs` (a clock that went backwards counts as fresh).
pub fn freshness(fetched_at: i64, now: i64, fresh_secs: i64) -> Freshness {
    if now - fetched_at < fresh_secs { Freshness::Fresh } else { Freshness::Stale }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Stamped<T> {
    pub fetched_at: i64,
    pub value: T,
}

fn now_unix() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

pub fn read<T: DeserializeOwned>(store: &LocalStore, key: &str) -> Option<Stamped<T>> {
    match store.cache_get::<Stamped<T>>(key) {
        Ok(v) => v,
        Err(e) => {
            tracing::debug!(%e, key, "reading a cached answer failed");
            None
        }
    }
}

pub fn write<T: Serialize>(store: &LocalStore, key: &str, value: &T) {
    let s = Stamped { fetched_at: now_unix(), value };
    if let Err(e) = store.cache_put(key, &s, KEEP_SECS) {
        tracing::warn!(%e, key, "writing a cached answer failed");
    }
}

/// Keys with a background refresh running.
fn refreshing() -> &'static Mutex<HashSet<String>> {
    static R: std::sync::OnceLock<Mutex<HashSet<String>>> = std::sync::OnceLock::new();
    R.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Run `refresh` in the background unless one is running for `key` (no runtime: nothing happens).
pub fn spawn_refresh<Fut>(key: String, refresh: Fut)
where
    Fut: Future<Output = ()> + Send + 'static,
{
    if tokio::runtime::Handle::try_current().is_err() {
        return;
    }
    if !refreshing().lock().map(|mut s| s.insert(key.clone())).unwrap_or(false) {
        return;
    }
    tokio::spawn(async move {
        tk_core::metadata::anilist::gateway::background(refresh).await;
        if let Ok(mut s) = refreshing().lock() {
            s.remove(&key);
        }
    });
}

/// Stale-while-revalidate: the stored value at once (refreshed behind when stale), else `fetch` (stored on success).
pub async fn get<T, F, Fut>(store: &Arc<LocalStore>, key: &str, fresh_secs: i64, fetch: F) -> UiResult<T>
where
    T: Serialize + DeserializeOwned + Send + 'static,
    F: Fn() -> Fut + Send + 'static,
    Fut: Future<Output = UiResult<T>> + Send + 'static,
{
    if let Some(s) = read::<T>(store, key) {
        if freshness(s.fetched_at, now_unix(), fresh_secs) == Freshness::Stale {
            let store = store.clone();
            let owned = key.to_owned();
            spawn_refresh(key.to_owned(), async move {
                match fetch().await {
                    Ok(v) => write(&store, &owned, &v),
                    Err(e) => tracing::debug!(detail = %e.detail, key = owned, "background refresh failed"),
                }
            });
        }
        return Ok(s.value);
    }
    let v = fetch().await?;
    write(store, key, &v);
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freshness_window() {
        assert_eq!(freshness(1000, 1000, HOME_FRESH_SECS), Freshness::Fresh);
        assert_eq!(freshness(1000, 1000 + HOME_FRESH_SECS - 1, HOME_FRESH_SECS), Freshness::Fresh);
        assert_eq!(freshness(1000, 1000 + HOME_FRESH_SECS, HOME_FRESH_SECS), Freshness::Stale);
        assert_eq!(freshness(1000, 10, HOME_FRESH_SECS), Freshness::Fresh);
    }

    fn store() -> Arc<LocalStore> {
        Arc::new(LocalStore::open_in_memory().unwrap())
    }

    #[tokio::test]
    async fn fresh_value_is_served_without_fetching_and_stale_one_is_refreshed_behind() {
        let s = store();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let fetch = |n: u32, calls: Arc<std::sync::atomic::AtomicUsize>| {
            move || {
                let calls = calls.clone();
                async move {
                    calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Ok::<u32, tk_ui::model::UiError>(n)
                }
            }
        };
        // Miss: fetched and stored.
        assert_eq!(get(&s, "k", 900, fetch(1, calls.clone())).await.unwrap(), 1);
        // Fresh: the stored value, no fetch.
        assert_eq!(get(&s, "k", 900, fetch(2, calls.clone())).await.unwrap(), 1);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        // Stale (backdated): still the stored value at once, refreshed in the background.
        s.cache_put("k", &Stamped { fetched_at: now_unix() - 1000, value: 1u32 }, KEEP_SECS).unwrap();
        assert_eq!(get(&s, "k", 900, fetch(3, calls.clone())).await.unwrap(), 1);
        for _ in 0..50 {
            if read::<u32>(&s, "k").is_some_and(|v| v.value == 3) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(read::<u32>(&s, "k").unwrap().value, 3);
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    }
}
