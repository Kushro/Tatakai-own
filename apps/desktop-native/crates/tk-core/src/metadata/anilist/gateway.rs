//! The one door to AniList (0.3.1): every GraphQL request of the app (metadata provider, airing, search, lists,
//! social, novels, manga) goes through one process-wide [`Gateway`].
//!
//! * **Limiter** ([`Limiter`], pure and clock-injected): at most [`MAX_IN_FLIGHT`] requests at once and a per-minute
//!   budget. The budget starts at a conservative [`DEFAULT_LIMIT`] and is learned from `X-RateLimit-Limit` /
//!   `X-RateLimit-Remaining`; when the remaining requests run low it waits until `X-RateLimit-Reset` (or until the
//!   oldest request of the last minute leaves the window). Never more than AniList allows: no evasion of any kind.
//! * **429**: `Retry-After` (or `X-RateLimit-Reset`) blocks the gateway for exactly that long. A short wait
//!   (<= [`SHORT_WAIT`]) is waited out and the request retried once; a long one returns
//!   [`GatewayError::RateLimited`] with the wait, so the router cools AniList down for that long and falls back.
//! * **Priorities**: interactive requests (details, search, player, list writes) go before background ones (Home
//!   rows, recommendations, calendar prefetch, social polling), which also leave [`INTERACTIVE_RESERVE`] requests of
//!   the budget to them. The default is interactive; a call site marks a future as background with [`background`]
//!   (a tokio task-local, so nothing in between has to pass it on).
//! * **De-duplication**: identical in-flight queries (same endpoint, query, variables and token) share one HTTP call.
//!   Mutations are never shared.
//! * **Diagnostics**: [`Gateway::stats`] (total, last minute, 429s, current wait) and one `debug` line per request.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use reqwest::header::HeaderMap;
use serde_json::{Value, json};
use tokio::sync::{Notify, OnceCell};

use crate::error::ProviderError;

/// Per-minute budget assumed until AniList's headers say otherwise (it has long run "degraded" at ~30/min).
pub const DEFAULT_LIMIT: u32 = 30;
/// Requests in flight at once.
pub const MAX_IN_FLIGHT: u32 = 3;
/// A 429 asking to wait at most this long is waited out and retried once, transparently.
pub const SHORT_WAIT: Duration = Duration::from_secs(10);
/// 429 without `Retry-After` / `X-RateLimit-Reset`.
pub const DEFAULT_RETRY: Duration = Duration::from_secs(60);
/// Requests of the budget background calls leave to interactive ones.
pub const INTERACTIVE_RESERVE: u32 = 3;
/// The budget is a sliding minute.
const WINDOW_MS: u64 = 60_000;
/// Requests kept in hand (headers lag behind requests already in flight).
const SLACK: u32 = 1;
/// Longest single sleep while waiting for a free slot (a release also wakes the waiters).
const SLOT_POLL_MS: u64 = 250;

// ---------------------------------------------------------------------------------------------
// Priority
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Priority {
    /// The user is waiting for it: details, search, player, list writes.
    #[default]
    Interactive,
    /// Prefetch and polling: Home rows, recommendations, calendar, social polling.
    Background,
}

impl Priority {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Interactive => "interactive",
            Self::Background => "background",
        }
    }
}

tokio::task_local! {
    static PRIORITY: Priority;
}

/// Run `f` with every AniList request inside it marked as `p`.
pub async fn with_priority<F: Future>(p: Priority, f: F) -> F::Output {
    PRIORITY.scope(p, f).await
}

/// Run `f` with every AniList request inside it marked [`Priority::Background`].
pub async fn background<F: Future>(f: F) -> F::Output {
    with_priority(Priority::Background, f).await
}

/// The priority of the calling task ([`Priority::Interactive`] unless inside [`background`]).
pub fn current_priority() -> Priority {
    PRIORITY.try_with(|p| *p).unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------
// Headers and retry policy (pure)
// ---------------------------------------------------------------------------------------------

/// The rate-limit headers of one answer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RateHeaders {
    pub limit: Option<u32>,
    pub remaining: Option<u32>,
    /// `X-RateLimit-Reset`: unix seconds.
    pub reset_unix: Option<i64>,
    /// `Retry-After` in seconds (the HTTP-date form is not used by AniList and is ignored).
    pub retry_after_secs: Option<u64>,
}

pub fn parse_headers(h: &HeaderMap) -> RateHeaders {
    fn num<T: std::str::FromStr>(h: &HeaderMap, name: &str) -> Option<T> {
        h.get(name)?.to_str().ok()?.trim().parse().ok()
    }
    RateHeaders {
        limit: num(h, "x-ratelimit-limit"),
        remaining: num(h, "x-ratelimit-remaining"),
        reset_unix: num(h, "x-ratelimit-reset"),
        retry_after_secs: num(h, "retry-after"),
    }
}

/// How long a 429 asks to wait: `Retry-After`, else until `X-RateLimit-Reset`, else [`DEFAULT_RETRY`]; at least 1 s.
pub fn retry_wait(h: &RateHeaders, now_ms: u64) -> Duration {
    let ms = if let Some(s) = h.retry_after_secs {
        s.saturating_mul(1000)
    } else if let Some(r) = h.reset_unix {
        (r.max(0) as u64).saturating_mul(1000).saturating_sub(now_ms)
    } else {
        DEFAULT_RETRY.as_millis() as u64
    };
    Duration::from_millis(ms.max(1000))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryPolicy {
    /// Wait this long, then send the request again.
    Retry(Duration),
    /// Give up: the caller (router) cools AniList down for this long.
    GiveUp(Duration),
}

/// After a 429 on attempt `attempt` (0 = first): a short wait is retried once, anything else gives up.
pub fn on_rate_limited(wait: Duration, attempt: u32) -> RetryPolicy {
    if attempt == 0 && wait <= SHORT_WAIT {
        RetryPolicy::Retry(wait)
    } else {
        RetryPolicy::GiveUp(wait)
    }
}

/// Whether a GraphQL body reports rate limiting (AniList also does it with a 200 / 400 and `errors[].status`).
pub fn body_rate_limited(body: &Value) -> bool {
    body["errors"].as_array().is_some_and(|e| e.iter().any(|x| x["status"] == 429))
}

// ---------------------------------------------------------------------------------------------
// Limiter (pure, clock-injected: every method takes `now_ms`, unix milliseconds)
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitWhy {
    /// A 429 blocked the gateway.
    Blocked,
    /// [`MAX_IN_FLIGHT`] requests are already out.
    Slot,
    /// Background yields to an interactive request that is waiting.
    Yield,
    /// The per-minute budget is (nearly) spent.
    Budget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Go,
    Wait { ms: u64, why: WaitWhy },
}

#[derive(Debug, Clone)]
pub struct Limiter {
    limit: u32,
    learned: bool,
    /// `X-RateLimit-Remaining` of the last answer, minus the requests sent since.
    remaining: Option<u32>,
    remaining_at: u64,
    reset_at: Option<u64>,
    /// Send times of the last minute.
    sent: VecDeque<u64>,
    in_flight: u32,
    cap: u32,
    blocked_until: u64,
    interactive_waiting: u32,
}

impl Default for Limiter {
    fn default() -> Self {
        Self::new(DEFAULT_LIMIT, MAX_IN_FLIGHT)
    }
}

impl Limiter {
    pub fn new(limit: u32, cap: u32) -> Self {
        Self {
            limit: limit.max(1),
            learned: false,
            remaining: None,
            remaining_at: 0,
            reset_at: None,
            sent: VecDeque::new(),
            in_flight: 0,
            cap: cap.max(1),
            blocked_until: 0,
            interactive_waiting: 0,
        }
    }

    fn prune(&mut self, now: u64) {
        while self.sent.front().is_some_and(|t| t + WINDOW_MS <= now) {
            self.sent.pop_front();
        }
        // A header older than the window (or past its reset) says nothing about the current minute.
        if self.remaining.is_some() && (self.remaining_at + WINDOW_MS <= now || self.reset_at.is_some_and(|r| r <= now)) {
            self.remaining = None;
            self.reset_at = None;
        }
    }

    /// Requests that may still go this minute.
    fn available(&self) -> u32 {
        let window = self.limit.saturating_sub(self.sent.len() as u32);
        self.remaining.map_or(window, |r| r.min(window))
    }

    /// When `need` more requests become available.
    fn budget_free_at(&self, now: u64, need: u32) -> u64 {
        let window_free = self.limit.saturating_sub(self.sent.len() as u32);
        let header_low = self.remaining.is_some_and(|r| r < need);
        if header_low {
            // Remaining is low: AniList's reset if it gave one, else when that header goes stale.
            return self.reset_at.unwrap_or(self.remaining_at + WINDOW_MS).max(now + 1);
        }
        if window_free >= need {
            return now + 1;
        }
        let k = (need - window_free) as usize;
        self.sent.get(k - 1).map_or(now + 1, |t| t + WINDOW_MS).max(now + 1)
    }

    /// What a request of priority `p` should do now. Prunes the window first.
    pub fn decide(&mut self, now: u64, p: Priority) -> Decision {
        self.prune(now);
        if self.blocked_until > now {
            return Decision::Wait { ms: self.blocked_until - now, why: WaitWhy::Blocked };
        }
        if self.in_flight >= self.cap {
            return Decision::Wait { ms: SLOT_POLL_MS, why: WaitWhy::Slot };
        }
        if p == Priority::Background && self.interactive_waiting > 0 {
            return Decision::Wait { ms: SLOT_POLL_MS, why: WaitWhy::Yield };
        }
        let reserve = SLACK + if p == Priority::Background { INTERACTIVE_RESERVE } else { 0 };
        let need = reserve + 1;
        if self.available() < need {
            let ms = self.budget_free_at(now, need) - now;
            return Decision::Wait { ms, why: WaitWhy::Budget };
        }
        Decision::Go
    }

    pub fn on_send(&mut self, now: u64) {
        self.in_flight += 1;
        self.sent.push_back(now);
        if let Some(r) = self.remaining.as_mut() {
            *r = r.saturating_sub(1);
        }
    }

    pub fn on_done(&mut self) {
        self.in_flight = self.in_flight.saturating_sub(1);
    }

    pub fn on_headers(&mut self, now: u64, h: &RateHeaders) {
        if let Some(l) = h.limit.filter(|l| *l > 0) {
            self.limit = l;
            self.learned = true;
        }
        if let Some(r) = h.remaining {
            self.remaining = Some(r);
            self.remaining_at = now;
        }
        if let Some(r) = h.reset_unix {
            self.reset_at = Some((r.max(0) as u64).saturating_mul(1000));
        }
    }

    /// A 429: nothing goes out for `wait` (the block replaces whatever remaining count the headers had; after it the
    /// next answer's headers say where the budget stands).
    pub fn on_429(&mut self, now: u64, wait: Duration) {
        self.blocked_until = self.blocked_until.max(now + wait.as_millis() as u64);
        self.remaining = None;
        self.reset_at = None;
    }

    pub fn interactive_waiting(&mut self, delta: i32) {
        self.interactive_waiting = (self.interactive_waiting as i64 + delta as i64).max(0) as u32;
    }

    /// Requests sent during the last minute (after pruning at `now`).
    pub fn last_minute(&mut self, now: u64) -> u32 {
        self.prune(now);
        self.sent.len() as u32
    }

    pub fn blocked_for(&self, now: u64) -> Duration {
        Duration::from_millis(self.blocked_until.saturating_sub(now))
    }

    pub fn limit(&self) -> (u32, bool) {
        (self.limit, self.learned)
    }

    pub fn remaining(&self) -> Option<u32> {
        self.remaining
    }
}

// ---------------------------------------------------------------------------------------------
// Gateway
// ---------------------------------------------------------------------------------------------

/// One AniList answer: HTTP status and the JSON body (a GraphQL error body included).
#[derive(Debug, Clone, PartialEq)]
pub struct Reply {
    pub status: u16,
    pub body: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GatewayError {
    #[error("anilist rate limited for {}s", retry_after.as_secs())]
    RateLimited { retry_after: Duration },
    #[error("network error: {0}")]
    Network(String),
    #[error("unexpected payload: {0}")]
    Parse(String),
}

impl From<GatewayError> for ProviderError {
    fn from(e: GatewayError) -> Self {
        match e {
            GatewayError::RateLimited { .. } => ProviderError::RateLimited { provider: "anilist".into() },
            GatewayError::Network(m) => ProviderError::Network(m),
            GatewayError::Parse(m) => ProviderError::Parse(m),
        }
    }
}

/// Diagnostics (Settings > System).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GatewayStats {
    /// HTTP requests sent since start.
    pub total: u64,
    /// HTTP requests sent during the last minute.
    pub last_minute: u32,
    /// 429 answers since start.
    pub rate_limited: u64,
    /// Calls answered by an identical request already in flight.
    pub deduped: u64,
    /// Seconds an interactive request would wait now (429 block or spent budget).
    pub wait_secs: u64,
    /// The per-minute budget and whether it came from AniList's headers.
    pub limit: u32,
    pub limit_learned: bool,
    pub remaining: Option<u32>,
}

type Shared = Arc<OnceCell<Result<Reply, GatewayError>>>;

pub struct Gateway {
    limiter: Mutex<Limiter>,
    notify: Notify,
    inflight: Mutex<HashMap<u64, Shared>>,
    total: AtomicU64,
    rate_limited: AtomicU64,
    deduped: AtomicU64,
}

impl Default for Gateway {
    fn default() -> Self {
        Self::new()
    }
}

/// The process-wide gateway every production [`super::AniList`] / novel client shares.
pub fn global() -> Arc<Gateway> {
    static G: OnceLock<Arc<Gateway>> = OnceLock::new();
    G.get_or_init(|| Arc::new(Gateway::new())).clone()
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Dedupe key: endpoint, query, variables and (hashed) token.
pub fn request_key(endpoint: &str, query: &str, variables: &Value, token: Option<&str>) -> u64 {
    let mut h = DefaultHasher::new();
    endpoint.hash(&mut h);
    query.hash(&mut h);
    variables.to_string().hash(&mut h);
    token.hash(&mut h);
    h.finish()
}

fn is_mutation(query: &str) -> bool {
    query.trim_start().starts_with("mutation")
}

/// The first field selected (`Page`, `Media`, an alias...) for the log line.
pub fn operation_name(query: &str) -> &str {
    let rest = query.split_once('{').map_or("", |(_, r)| r).trim_start();
    let end = rest.find(|c: char| !(c.is_alphanumeric() || c == '_')).unwrap_or(rest.len());
    let op = &rest[..end];
    if op == "Page" {
        // 0.3.1 QA: every list query is a `Page`; name the field inside it so the log tells them apart.
        for (field, name) in PAGE_FIELDS.iter().zip(PAGE_OPS) {
            if query.contains(&format!(" {field}(")) || query.contains(&format!(" {field} {{")) {
                return name;
            }
        }
    }
    op
}

/// The request variables for the debug log, cut to 160 characters (0.3.1 QA: tells which screen asked).
fn vars_summary(v: &Value) -> String {
    let s = v.to_string();
    match s.char_indices().nth(160) {
        Some((i, _)) => format!("{}…", &s[..i]),
        None => s,
    }
}

/// Fields of AniList's `Page` the app asks for, most specific first (`media` last: other fields contain media
/// sub-selections).
const PAGE_FIELDS: [&str; 9] =
    ["airingSchedules", "airingTrends", "recommendations", "mediaList", "characters", "staff", "reviews", "activities", "media"];
const PAGE_OPS: [&str; 9] = [
    "Page.airingSchedules",
    "Page.airingTrends",
    "Page.recommendations",
    "Page.mediaList",
    "Page.characters",
    "Page.staff",
    "Page.reviews",
    "Page.activities",
    "Page.media",
];

struct Permit<'a>(&'a Gateway);

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        if let Ok(mut l) = self.0.limiter.lock() {
            l.on_done();
        }
        self.0.notify.notify_waiters();
    }
}

struct WaitingGuard<'a>(&'a Gateway);

impl Drop for WaitingGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut l) = self.0.limiter.lock() {
            l.interactive_waiting(-1);
        }
        self.0.notify.notify_waiters();
    }
}

impl Gateway {
    pub fn new() -> Self {
        Self::with_limiter(Limiter::default())
    }

    pub fn with_limiter(l: Limiter) -> Self {
        Self {
            limiter: Mutex::new(l),
            notify: Notify::new(),
            inflight: Mutex::new(HashMap::new()),
            total: AtomicU64::new(0),
            rate_limited: AtomicU64::new(0),
            deduped: AtomicU64::new(0),
        }
    }

    pub fn stats(&self) -> GatewayStats {
        let now = now_ms();
        let (last_minute, wait_ms, limit, learned, remaining) = match self.limiter.lock() {
            Ok(mut l) => {
                let mut probe = l.clone();
                let wait = match probe.decide(now, Priority::Interactive) {
                    Decision::Wait { ms, why: WaitWhy::Blocked | WaitWhy::Budget } => ms,
                    _ => 0,
                };
                let (limit, learned) = l.limit();
                (l.last_minute(now), wait, limit, learned, l.remaining())
            }
            Err(_) => (0, 0, DEFAULT_LIMIT, false, None),
        };
        GatewayStats {
            total: self.total.load(Ordering::Relaxed),
            last_minute,
            rate_limited: self.rate_limited.load(Ordering::Relaxed),
            deduped: self.deduped.load(Ordering::Relaxed),
            wait_secs: wait_ms.div_ceil(1000),
            limit,
            limit_learned: learned,
            remaining,
        }
    }

    /// How long a 429 still blocks this gateway (the router's cool-down for AniList).
    pub fn blocked_for(&self) -> Duration {
        self.limiter.lock().map(|l| l.blocked_for(now_ms())).unwrap_or_default()
    }

    /// A slot under the limiter. A 429 block longer than [`SHORT_WAIT`] is not waited out: the caller falls back.
    async fn acquire(&self, p: Priority) -> Result<Permit<'_>, GatewayError> {
        let _waiting = (p == Priority::Interactive).then(|| {
            if let Ok(mut l) = self.limiter.lock() {
                l.interactive_waiting(1);
            }
            WaitingGuard(self)
        });
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let decision = {
                let mut l = self.limiter.lock().map_err(|_| GatewayError::Network("limiter poisoned".into()))?;
                let now = now_ms();
                let d = l.decide(now, p);
                if d == Decision::Go {
                    l.on_send(now);
                }
                d
            };
            match decision {
                Decision::Go => return Ok(Permit(self)),
                Decision::Wait { ms, why: WaitWhy::Blocked } if Duration::from_millis(ms) > SHORT_WAIT => {
                    return Err(GatewayError::RateLimited { retry_after: Duration::from_millis(ms) });
                }
                Decision::Wait { ms, why } => {
                    if why == WaitWhy::Budget {
                        tracing::debug!(target: "tk_core::anilist", wait_ms = ms, priority = p.as_str(), "anilist budget spent, waiting");
                    }
                    tokio::select! {
                        () = tokio::time::sleep(Duration::from_millis(ms)) => {}
                        () = &mut notified => {}
                    }
                }
            }
        }
    }

    /// POST one GraphQL request (`token` = bearer). Identical in-flight queries share one HTTP call.
    pub async fn execute(
        &self,
        client: &reqwest::Client,
        endpoint: &str,
        query: &str,
        variables: Value,
        token: Option<&str>,
    ) -> Result<Reply, GatewayError> {
        if is_mutation(query) {
            return self.run(client, endpoint, query, &variables, token).await;
        }
        let key = request_key(endpoint, query, &variables, token);
        let cell: Shared = {
            let mut m = self.inflight.lock().map_err(|_| GatewayError::Network("gateway poisoned".into()))?;
            m.entry(key).or_default().clone()
        };
        let mut ran = false;
        let out = cell
            .get_or_init(|| {
                ran = true;
                self.run(client, endpoint, query, &variables, token)
            })
            .await
            .clone();
        if ran {
            if let Ok(mut m) = self.inflight.lock()
                && m.get(&key).is_some_and(|c| Arc::ptr_eq(c, &cell))
            {
                m.remove(&key);
            }
        } else {
            self.deduped.fetch_add(1, Ordering::Relaxed);
            tracing::debug!(target: "tk_core::anilist", op = operation_name(query), "anilist request shared with one in flight");
        }
        out
    }

    async fn run(
        &self,
        client: &reqwest::Client,
        endpoint: &str,
        query: &str,
        variables: &Value,
        token: Option<&str>,
    ) -> Result<Reply, GatewayError> {
        let p = current_priority();
        let op = operation_name(query);
        let mut attempt = 0;
        loop {
            let permit = self.acquire(p).await?;
            let started = Instant::now();
            let mut req = client
                .post(endpoint)
                .header("Accept", "application/json")
                .json(&json!({ "query": query, "variables": variables }));
            if let Some(t) = token {
                req = req.bearer_auth(t);
            }
            self.total.fetch_add(1, Ordering::Relaxed);
            let resp = req.send().await.map_err(|e| GatewayError::Network(e.without_url().to_string()))?;
            let status = resp.status().as_u16();
            let headers = parse_headers(resp.headers());
            let body = resp.json::<Value>().await;
            let now = now_ms();
            if let Ok(mut l) = self.limiter.lock() {
                l.on_headers(now, &headers);
            }
            tracing::debug!(
                target: "tk_core::anilist",
                method = "POST",
                op,
                priority = p.as_str(),
                status,
                remaining = ?headers.remaining,
                limit = ?headers.limit,
                ms = started.elapsed().as_millis() as u64,
                vars = %vars_summary(variables),
                "anilist request"
            );
            let limited = status == 429 || body.as_ref().is_ok_and(body_rate_limited);
            if limited {
                self.rate_limited.fetch_add(1, Ordering::Relaxed);
                let wait = retry_wait(&headers, now);
                if let Ok(mut l) = self.limiter.lock() {
                    l.on_429(now, wait);
                }
                drop(permit);
                match on_rate_limited(wait, attempt) {
                    RetryPolicy::Retry(w) => {
                        tracing::info!(target: "tk_core::anilist", wait_s = w.as_secs(), op, "anilist 429, retrying once after the wait");
                        attempt += 1;
                        continue;
                    }
                    RetryPolicy::GiveUp(w) => {
                        tracing::info!(target: "tk_core::anilist", wait_s = w.as_secs(), op, "anilist 429, cooling down");
                        return Err(GatewayError::RateLimited { retry_after: w });
                    }
                }
            }
            drop(permit);
            let body = body.map_err(|e| GatewayError::Parse(e.without_url().to_string()))?;
            return Ok(Reply { status, body });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::HeaderValue;

    const T0: u64 = 1_800_000_000_000;

    #[test]
    fn parses_rate_headers() {
        let mut h = HeaderMap::new();
        h.insert("X-RateLimit-Limit", HeaderValue::from_static("90"));
        h.insert("X-RateLimit-Remaining", HeaderValue::from_static(" 59 "));
        h.insert("X-RateLimit-Reset", HeaderValue::from_static("1800000030"));
        h.insert("Retry-After", HeaderValue::from_static("30"));
        assert_eq!(
            parse_headers(&h),
            RateHeaders { limit: Some(90), remaining: Some(59), reset_unix: Some(1_800_000_030), retry_after_secs: Some(30) }
        );
        let mut bad = HeaderMap::new();
        bad.insert("Retry-After", HeaderValue::from_static("Wed, 21 Oct 2026 07:28:00 GMT"));
        assert_eq!(parse_headers(&bad), RateHeaders::default());
    }

    #[test]
    fn retry_wait_prefers_retry_after_then_reset() {
        let h = RateHeaders { retry_after_secs: Some(7), reset_unix: Some(1_800_000_050), ..Default::default() };
        assert_eq!(retry_wait(&h, T0), Duration::from_secs(7));
        let h = RateHeaders { reset_unix: Some(1_800_000_050), ..Default::default() };
        assert_eq!(retry_wait(&h, T0), Duration::from_secs(50));
        assert_eq!(retry_wait(&RateHeaders::default(), T0), DEFAULT_RETRY);
        // A reset in the past still waits a second.
        let h = RateHeaders { reset_unix: Some(1), ..Default::default() };
        assert_eq!(retry_wait(&h, T0), Duration::from_secs(1));
    }

    #[test]
    fn short_429_is_retried_once_long_one_gives_up() {
        assert_eq!(on_rate_limited(Duration::from_secs(3), 0), RetryPolicy::Retry(Duration::from_secs(3)));
        assert_eq!(on_rate_limited(SHORT_WAIT, 0), RetryPolicy::Retry(SHORT_WAIT));
        assert_eq!(on_rate_limited(Duration::from_secs(3), 1), RetryPolicy::GiveUp(Duration::from_secs(3)));
        assert_eq!(on_rate_limited(Duration::from_secs(45), 0), RetryPolicy::GiveUp(Duration::from_secs(45)));
    }

    #[test]
    fn graphql_body_rate_limit() {
        assert!(body_rate_limited(&json!({ "errors": [{ "message": "Too Many Requests.", "status": 429 }] })));
        assert!(!body_rate_limited(&json!({ "errors": [{ "status": 404 }] })));
        assert!(!body_rate_limited(&json!({ "data": {} })));
    }

    #[test]
    fn concurrency_cap() {
        let mut l = Limiter::new(90, 3);
        for _ in 0..3 {
            assert_eq!(l.decide(T0, Priority::Interactive), Decision::Go);
            l.on_send(T0);
        }
        assert!(matches!(l.decide(T0, Priority::Interactive), Decision::Wait { why: WaitWhy::Slot, .. }));
        l.on_done();
        assert_eq!(l.decide(T0, Priority::Interactive), Decision::Go);
    }

    #[test]
    fn conservative_budget_until_headers_then_learned() {
        let mut l = Limiter::new(DEFAULT_LIMIT, 100);
        // 29 interactive requests (one kept as slack) go in the first second...
        for i in 0..29 {
            assert_eq!(l.decide(T0 + i, Priority::Interactive), Decision::Go, "request {i}");
            l.on_send(T0 + i);
            l.on_done();
        }
        // ...the 30th waits until the first leaves the minute.
        assert_eq!(l.decide(T0 + 100, Priority::Interactive), Decision::Wait { ms: 60_000 - 100, why: WaitWhy::Budget });
        assert_eq!(l.decide(T0 + 60_000, Priority::Interactive), Decision::Go);
        // AniList says 90: the window widens.
        l.on_headers(T0 + 60_000, &RateHeaders { limit: Some(90), remaining: Some(80), ..Default::default() });
        assert_eq!(l.limit(), (90, true));
        assert_eq!(l.decide(T0 + 60_001, Priority::Interactive), Decision::Go);
    }

    #[test]
    fn low_remaining_waits_for_reset() {
        let mut l = Limiter::new(90, 100);
        l.on_headers(T0, &RateHeaders { limit: Some(90), remaining: Some(1), reset_unix: Some(1_800_000_020), ..Default::default() });
        assert_eq!(l.decide(T0 + 1000, Priority::Interactive), Decision::Wait { ms: 19_000, why: WaitWhy::Budget });
        // Past the reset the header is forgotten.
        assert_eq!(l.decide(T0 + 20_000, Priority::Interactive), Decision::Go);
        // Without a reset: until the header goes stale (one minute).
        l.on_headers(T0 + 20_000, &RateHeaders { remaining: Some(0), ..Default::default() });
        assert_eq!(l.decide(T0 + 30_000, Priority::Interactive), Decision::Wait { ms: 50_000, why: WaitWhy::Budget });
    }

    #[test]
    fn background_leaves_a_reserve_and_yields_to_interactive() {
        let mut l = Limiter::new(90, 100);
        l.on_headers(T0, &RateHeaders { limit: Some(90), remaining: Some(4), ..Default::default() });
        assert!(matches!(l.decide(T0, Priority::Background), Decision::Wait { why: WaitWhy::Budget, .. }));
        assert_eq!(l.decide(T0, Priority::Interactive), Decision::Go);
        l.on_headers(T0, &RateHeaders { remaining: Some(50), ..Default::default() });
        assert_eq!(l.decide(T0, Priority::Background), Decision::Go);
        l.interactive_waiting(1);
        assert!(matches!(l.decide(T0, Priority::Background), Decision::Wait { why: WaitWhy::Yield, .. }));
        assert_eq!(l.decide(T0, Priority::Interactive), Decision::Go);
        l.interactive_waiting(-1);
        assert_eq!(l.decide(T0, Priority::Background), Decision::Go);
    }

    #[test]
    fn a_429_blocks_for_the_asked_wait() {
        let mut l = Limiter::new(90, 3);
        l.on_429(T0, Duration::from_secs(30));
        assert_eq!(l.decide(T0 + 5_000, Priority::Interactive), Decision::Wait { ms: 25_000, why: WaitWhy::Blocked });
        assert_eq!(l.blocked_for(T0 + 10_000), Duration::from_secs(20));
        assert_eq!(l.decide(T0 + 30_000, Priority::Interactive), Decision::Go);
    }

    #[test]
    fn sends_consume_the_header_remaining() {
        let mut l = Limiter::new(90, 100);
        l.on_headers(T0, &RateHeaders { remaining: Some(3), ..Default::default() });
        l.on_send(T0);
        l.on_done();
        assert_eq!(l.remaining(), Some(2));
        assert_eq!(l.decide(T0, Priority::Interactive), Decision::Go);
        l.on_send(T0);
        l.on_done();
        assert!(matches!(l.decide(T0, Priority::Interactive), Decision::Wait { why: WaitWhy::Budget, .. }));
    }

    #[test]
    fn keys_and_names() {
        let a = request_key("e", "query { Page { id } }", &json!({ "page": 1 }), None);
        assert_eq!(a, request_key("e", "query { Page { id } }", &json!({ "page": 1 }), None));
        assert_ne!(a, request_key("e", "query { Page { id } }", &json!({ "page": 2 }), None));
        assert_ne!(a, request_key("e", "query { Page { id } }", &json!({ "page": 1 }), Some("t")));
        assert_eq!(operation_name("query ($id: Int) { Media(id: $id) { id } }"), "Media");
        assert_eq!(operation_name("query { trending: Page { id } }"), "trending");
        assert_eq!(
            operation_name("query ($p: Int) { Page(page: $p, perPage: 50) { pageInfo { hasNextPage } airingSchedules(airingAt_greater: 1) { media { id } } } }"),
            "Page.airingSchedules"
        );
        assert_eq!(operation_name("query { Page(page: 1) { pageInfo { total } media(id_in: [1]) { id } } }"), "Page.media");
        assert!(is_mutation("  mutation ($id: Int) { ToggleFollow(userId: $id) { id } }"));
        assert!(!is_mutation("query { Viewer { id } }"));
    }

    #[tokio::test]
    async fn priority_is_task_local() {
        assert_eq!(current_priority(), Priority::Interactive);
        assert_eq!(background(async { current_priority() }).await, Priority::Background);
        let both = background(async { tokio::join!(async { current_priority() }, async { current_priority() }) }).await;
        assert_eq!(both, (Priority::Background, Priority::Background));
    }

    // ---- mock server -------------------------------------------------------------------------------------------

    use std::sync::atomic::AtomicUsize;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A tiny HTTP/1.1 server answering each request with the next canned `(status, headers, body)` (the last one
    /// repeats); counts the requests and delays each answer by `delay`.
    pub(crate) async fn mock(answers: Vec<(u16, Vec<(&'static str, String)>, String)>, delay: Duration) -> (String, Arc<AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let hits = Arc::new(AtomicUsize::new(0));
        let h = hits.clone();
        let answers = Arc::new(answers);
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = listener.accept().await else { return };
                let h = h.clone();
                let answers = answers.clone();
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 4096];
                    // Read headers + the Content-Length body.
                    loop {
                        let n = s.read(&mut chunk).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                        let text = String::from_utf8_lossy(&buf).to_string();
                        if let Some(end) = text.find("\r\n\r\n") {
                            let len = text[..end]
                                .lines()
                                .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap_or(0)))
                                .unwrap_or(0);
                            if buf.len() >= end + 4 + len {
                                break;
                            }
                        }
                    }
                    let i = h.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(delay).await;
                    let (status, headers, body) = &answers[i.min(answers.len() - 1)];
                    let mut out = format!("HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n", body.len());
                    for (k, v) in headers {
                        out.push_str(&format!("{k}: {v}\r\n"));
                    }
                    out.push_str("\r\n");
                    out.push_str(body);
                    let _ = s.write_all(out.as_bytes()).await;
                    let _ = s.shutdown().await;
                });
            }
        });
        (format!("http://{addr}/"), hits)
    }

    fn ok_body() -> String {
        json!({ "data": { "Media": { "id": 1 } } }).to_string()
    }

    #[tokio::test]
    async fn short_retry_after_is_waited_out_and_retried_once() {
        let (url, hits) = mock(
            vec![
                (429, vec![("Retry-After", "1".into())], json!({ "errors": [{ "status": 429 }] }).to_string()),
                (200, vec![("X-RateLimit-Limit", "90".into()), ("X-RateLimit-Remaining", "88".into())], ok_body()),
            ],
            Duration::ZERO,
        )
        .await;
        let g = Gateway::new();
        let started = Instant::now();
        let r = g.execute(&reqwest::Client::new(), &url, "query { Media { id } }", json!({}), None).await.unwrap();
        assert!(started.elapsed() >= Duration::from_millis(900), "waited {:?}", started.elapsed());
        assert_eq!(r.status, 200);
        assert_eq!(r.body["data"]["Media"]["id"], 1);
        assert_eq!(hits.load(Ordering::SeqCst), 2);
        let s = g.stats();
        assert_eq!((s.total, s.rate_limited, s.limit, s.limit_learned, s.remaining), (2, 1, 90, true, Some(88)));
    }

    #[tokio::test]
    async fn long_retry_after_returns_the_wait_and_blocks_later_calls() {
        let (url, hits) = mock(vec![(429, vec![("Retry-After", "45".into())], "{}".into())], Duration::ZERO).await;
        let g = Gateway::new();
        let c = reqwest::Client::new();
        let e = g.execute(&c, &url, "query { Media { id } }", json!({}), None).await.unwrap_err();
        assert_eq!(e, GatewayError::RateLimited { retry_after: Duration::from_secs(45) });
        assert!(g.blocked_for() > Duration::from_secs(40));
        // The next call does not even reach AniList: it fails fast with the remaining wait.
        let e = g.execute(&c, &url, "query { Page { id } }", json!({}), None).await.unwrap_err();
        assert!(matches!(e, GatewayError::RateLimited { retry_after } if retry_after > Duration::from_secs(40)));
        assert_eq!(hits.load(Ordering::SeqCst), 1);
        assert!(g.stats().wait_secs > 40);
    }

    #[tokio::test]
    async fn identical_requests_in_flight_share_one_call() {
        let (url, hits) = mock(vec![(200, vec![], ok_body())], Duration::from_millis(150)).await;
        let g = Gateway::new();
        let c = reqwest::Client::new();
        let q = "query ($id: Int) { Media(id: $id) { id } }";
        let (a, b, other) = tokio::join!(
            g.execute(&c, &url, q, json!({ "id": 1 }), None),
            g.execute(&c, &url, q, json!({ "id": 1 }), None),
            g.execute(&c, &url, q, json!({ "id": 2 }), None),
        );
        assert_eq!(a.unwrap(), b.unwrap());
        assert!(other.is_ok());
        assert_eq!(hits.load(Ordering::SeqCst), 2);
        assert_eq!(g.stats().deduped, 1);
        // Once settled, the same query goes out again.
        g.execute(&c, &url, q, json!({ "id": 1 }), None).await.unwrap();
        assert_eq!(hits.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn mutations_are_never_shared() {
        let (url, hits) = mock(vec![(200, vec![], ok_body())], Duration::from_millis(100)).await;
        let g = Gateway::new();
        let c = reqwest::Client::new();
        let q = "mutation ($id: Int) { ToggleLike(id: $id) { id } }";
        let (a, b) = tokio::join!(
            g.execute(&c, &url, q, json!({ "id": 1 }), Some("t")),
            g.execute(&c, &url, q, json!({ "id": 1 }), Some("t")),
        );
        assert!(a.is_ok() && b.is_ok());
        assert_eq!(hits.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn concurrency_is_capped_on_the_wire() {
        let (url, hits) = mock(vec![(200, vec![], ok_body())], Duration::from_millis(200)).await;
        let g = Arc::new(Gateway::with_limiter(Limiter::new(90, 2)));
        let c = reqwest::Client::new();
        let started = Instant::now();
        let mut tasks = Vec::new();
        for i in 0..4 {
            let (g, c, url) = (g.clone(), c.clone(), url.clone());
            tasks.push(tokio::spawn(async move { g.execute(&c, &url, "query { Media { id } }", json!({ "i": i }), None).await }));
        }
        for t in tasks {
            t.await.unwrap().unwrap();
        }
        // Four 200 ms answers two at a time: at least two rounds.
        assert!(started.elapsed() >= Duration::from_millis(380), "{:?}", started.elapsed());
        assert_eq!(hits.load(Ordering::SeqCst), 4);
    }
}
