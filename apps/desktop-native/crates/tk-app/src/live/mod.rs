//! The shipping backend: `tk_ui::Backend` over tk-core (metadata router, local store, secrets, AniList / MAL
//! lists), tk-ext (extension host) and tk-link (Link server, incl. the watch-history sync with the phone).
//! No own account (plan 14 section 0.1 #9): the Supabase client was removed in wave A.
//!

mod browser;
mod ext;
mod link;
mod sources;
// wave 3 (docs/redesign/10 section 4): engine wiring (A), playback path (B), settings + policies (D)
mod torrent;
mod torrent_play;
mod torrent_settings;
// wave A (docs/redesign/15 section 2): one file per owner. a search, c details, d subs, e presence, f system +
// history_sync, g lists, i debrid + torrent_manual. This file only delegates; nobody edits it after the contract.
mod debrid;
mod details;
mod history_sync;
mod lists;
mod presence;
mod search;
mod subs;
mod system;
mod torrent_manual;
// wave B (docs/redesign/16 section 2): b home (incl. the former `home()` body), h downloads, j ext_server +
// ext_catalog (live/ext.rs is j's too), k server + sniffer (+ link.rs, browser.rs), l manga, m novel, n social.
mod downloads;
mod ext_catalog;
mod ext_server;
mod hls;
mod home;
mod manga;
mod novel;
mod server;
mod sniffer;
mod social;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use time::OffsetDateTime;
use tk_core::sources::{INITIAL_RELIABILITY, SourceCandidate, SourceKind, parse_source_key, source_key};
use tk_core::{Compat, HistoryEntry, LocalStore, Media, MediaKind, MediaRef, MetadataRouter, ProviderError};
use tk_ui::backend::{
    ActivityFeedVm, ActivityReplyVm, ActivityVm, AiringSlotVm, AnimeBuildVm, AtlasPointVm, AutoDownloadRunVm, Backend,
    BecauseVm, ChapterProviderVm, ChapterRefVm, CharacterVm, ConflictSideVm, ContinueVm, CueVm, CustomProviderVm,
    DebridAccountVm, DebridServiceVm, DetailsExtraVm, DetailsVm, DownloadRequestVm, DownloadStorageVm, DownloadVm,
    EpisodeVm, ExportFormatVm, ExtCatalogItemVm, ExtLogVm, ExtPrefFieldVm, ExtRepoVm, ExtServerPairVm, ExtServerVm,
    ExtSourceVm, ExtUpdateVm, ExtensionDetailVm, ExtensionVm, FlareSolverrVm, HistoryItemVm, HomeVm, ImageMatchVm,
    ImageQueryVm, KittenStatusVm, LatestEpisodeVm, LibraryScanVm, LibraryVm, LinkCapsVm, LinkStatusVm, ListAccountVm,
    ListConflictVm, ListEntryUpdateVm, ListEntryVm, ListImportVm, ListStatusVm, ListSyncVm, LocalItemVm, MangaDetailsVm,
    MediaVm, NotificationPageVm, NovelContentVm, NovelDetailsVm, PageVm, PlaybackVm, PresenceVm, ProfileVm,
    ProviderHealthVm, ProviderItemVm, ProxyStatusVm, ReadHomeVm, ReadProgressVm, RecommendationVm, RecsFeedbackVm,
    RemoteCommandVm, ReviewVm, SearchPageVm, SearchQueryVm, SectionVm, SendOutcomeVm, SendToDeviceVm, ServerDeviceVm, ServerLogPageVm,
    ServerPairingVm, ServerStatusVm, SkipMarkVm, SniffRequestVm, SniffResultVm, SourceVm, StorageUsageVm,
    StremioAddonVm, SystemCapsVm, TasteProfileVm, TopRangeVm, TorrentActivityVm, TorrentFileVm, TorrentLibraryItemVm,
    TorrentPeerVm, TorrentPlaybackVm, TorrentPreviewVm, TorrentSessionVm, TorrentSettingsVm, TorrentStatsVm,
    TorrentTrackerVm, TranslateRequestVm, TranslationTestVm, TtsRequestVm, TtsSpeakVm, TtsVoiceVm, UiResult,
    UpdateCheckVm, UserPageVm, WatchOrderEntryVm,
};
use tk_ui::model::{ErrorKind, ListServiceVm, Pairing, ReadKindVm, Settings, TranslationProviderVm, TtsProviderVm, UiError};
use tk_ui::ui_thread::UiThread;

use crate::backend_common::{IdKind, parse_media_id};
use crate::config::AppConfig;
use crate::paths::AppPaths;
use crate::settings_store;

pub struct LiveBackend {
    /// Data directory root (`AppPaths::root`): Settings texts, mpv-anime-build extraction.
    data_root: std::path::PathBuf,
    settings_file: std::path::PathBuf,
    store: Arc<LocalStore>,
    router: Arc<MetadataRouter>,
    /// `id -> (title, poster)` of titles opened this session, for history rows.
    titles: Mutex<HashMap<String, (String, Option<String>)>>,
    ext: ext::Extensions,
    link: link::LinkService,
    /// `(media id, episode) -> (expires at, candidates)`: `sources()` and `playback_source()` share one resolution
    /// (stream URLs expire, hence the TTL). Cleared when the set of enabled extensions changes.
    candidates: Mutex<HashMap<(String, u32), (Instant, Vec<SourceCandidate>)>>,
    /// BUG-134: the torrent candidates every listing returned, kept after the list itself expires (a magnet does not
    /// go stale like a stream URL), so playing or sending a torrent picked in Details does not ask every extension
    /// again. Cleared with `candidates`.
    torrent_memo: Mutex<TorrentMemo>,
    /// BUG-134: the last ranked list `sources()` answered per `(media id, episode)`, whatever its age: the player
    /// starts a source confirmed in Details from it at once and refreshes the list behind (`Backend::known_sources`).
    /// Cleared with `candidates`.
    last_sources: Mutex<LastSources>,
    /// When the extensions' outbound route (proxy / FlareSolverr) last changed (BUG-102): an empty listing right
    /// after is asked again once before "No sources found".
    net_changed: Mutex<Option<Instant>>,
    /// Torrent engine, started on demand (wave 3 A, `live/torrent.rs`).
    torrent: torrent::TorrentService,
    /// OS keyring / fallback file (wave A, `tk_core::Secrets`, f): AniList / MAL tokens (g), translation keys (d),
    /// debrid keys (i).
    secrets: tk_core::Secrets,
    // --- wave B services (docs/redesign/16 section 3.8): one per owner, constructed in `new`, bodies in their files.
    /// HLS / MP4 download engine + local library (worker h, `live/downloads.rs`).
    downloads: downloads::Downloads,
    /// Native extension server (`tk-server`) + proxy / FlareSolverr (worker k, `live/server.rs`).
    server: server::ExtServer,
    /// Embed sniffer over the hidden web views (worker k, `live/sniffer.rs`).
    sniffer: sniffer::Sniffer,
    /// Narration engines (worker m, `crate::tts`).
    tts: crate::tts::Tts,
    /// Remote commands from paired phones (`tk-link` `/v1/control`), consumed by `wait_remote_command` (worker k).
    remote: tokio::sync::Mutex<tokio::sync::mpsc::UnboundedReceiver<RemoteCommandVm>>,
}

/// How long a resolved source list is reused before the extensions are asked again.
const SOURCES_TTL: Duration = Duration::from_secs(5 * 60);
/// BUG-102: a list some extension route or provider failed (or timed out) on is reused only this long, so the
/// sources it missed (often the torrents) come back on the next ask instead of staying hidden for `SOURCES_TTL`.
const PARTIAL_SOURCES_TTL: Duration = Duration::from_secs(15);
/// BUG-102: how long after an outbound-route change an empty source listing is retried once, and the pause before.
const NET_CHANGE_WINDOW: Duration = Duration::from_secs(90);
const NET_CHANGE_RETRY_PAUSE: Duration = Duration::from_secs(2);

/// How many `(media id, episode)` lists `TorrentMemo` keeps (the oldest is dropped first).
const TORRENT_MEMO_KEYS: usize = 64;
/// BUG-134: how long past its TTL an expired list still serves `playback_source` a web candidate picked from it (a
/// direct stream URL usually lives far longer than `SOURCES_TTL`; a dead one fails over like any broken source).
const STALE_WEB_GRACE: Duration = Duration::from_secs(15 * 60);

/// BUG-134: the last ranked `SourceVm` list of each `(media id, episode)`, never expired, bounded to
/// `TORRENT_MEMO_KEYS` lists (the least recently written goes first).
#[derive(Default)]
pub(crate) struct LastSources {
    lists: HashMap<(String, u32), Vec<SourceVm>>,
    order: std::collections::VecDeque<(String, u32)>,
}

impl LastSources {
    pub(crate) fn remember(&mut self, key: (String, u32), list: &[SourceVm]) {
        if list.is_empty() {
            return;
        }
        self.lists.insert(key.clone(), list.to_vec());
        self.order.retain(|k| *k != key);
        self.order.push_back(key);
        while self.order.len() > TORRENT_MEMO_KEYS {
            if let Some(old) = self.order.pop_front() {
                self.lists.remove(&old);
            }
        }
    }

    pub(crate) fn get(&self, id: &str, episode: u32) -> Option<Vec<SourceVm>> {
        self.lists.get(&(id.to_owned(), episode)).cloned()
    }

    fn clear(&mut self) {
        self.lists.clear();
        self.order.clear();
    }
}

/// BUG-134: torrent candidates by `(media id, episode)`, upserted by source key from every listing (a partial listing
/// that missed a torrent does not forget it) and never expired; bounded to `TORRENT_MEMO_KEYS` lists.
#[derive(Default)]
pub(crate) struct TorrentMemo {
    lists: HashMap<(String, u32), Vec<SourceCandidate>>,
    /// Keys from the least to the most recently written.
    order: std::collections::VecDeque<(String, u32)>,
}

impl TorrentMemo {
    /// Keep the torrent candidates (with their torrent info) of a listing of `key`.
    pub(crate) fn remember(&mut self, key: (String, u32), list: &[SourceCandidate]) {
        let torrents: Vec<&SourceCandidate> =
            list.iter().filter(|c| c.kind == SourceKind::Torrent && c.torrent.is_some()).collect();
        if torrents.is_empty() {
            return;
        }
        let slot = self.lists.entry(key.clone()).or_default();
        for c in torrents {
            let k = source_key(c);
            match slot.iter_mut().find(|o| source_key(o) == k) {
                Some(o) => *o = c.clone(),
                None => slot.push(c.clone()),
            }
        }
        self.order.retain(|k| *k != key);
        self.order.push_back(key);
        while self.order.len() > TORRENT_MEMO_KEYS {
            if let Some(old) = self.order.pop_front() {
                self.lists.remove(&old);
            }
        }
    }

    /// The torrent candidate `source` of `id`: of that episode, or of any episode without one.
    pub(crate) fn find(&self, id: &str, episode: Option<u32>, source: &str) -> Option<SourceCandidate> {
        self.lists
            .iter()
            .filter(|((mid, ep), _)| mid.as_str() == id && episode.is_none_or(|e| e == *ep))
            .flat_map(|(_, v)| v.iter())
            .find(|c| source_key(c) == source)
            .cloned()
    }

    fn clear(&mut self) {
        self.lists.clear();
        self.order.clear();
    }
}

/// How long a resolved source list of `len` candidates is reused (`None`: not kept). Pure.
fn candidates_ttl(len: usize, partial: bool) -> Option<Duration> {
    match (len, partial) {
        (0, _) => None,
        (_, true) => Some(PARTIAL_SOURCES_TTL),
        _ => Some(SOURCES_TTL),
    }
}

/// The metadata provider whose answers are complete; any other one answering for an AniList / MAL id is a fallback.
const PRIMARY_METADATA: &str = "anilist";

/// BUG-143: titles + poster of the last complete (AniList) answer for one series, kept in the metadata cache.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
struct KnownMedia {
    title: String,
    titles: std::collections::BTreeMap<String, String>,
    cover: Option<String>,
}

impl KnownMedia {
    fn of(m: &Media) -> Self {
        Self { title: m.title.clone(), titles: m.titles.clone(), cover: m.cover.clone() }
    }
}

/// Cache key of `KnownMedia` (`media.known.a<id>` / `media.known.m<id>`); Kitsu-only refs have none.
fn known_key(r: &MediaRef) -> Option<String> {
    match (r.anilist_id, r.mal_id) {
        (Some(a), _) => Some(format!("media.known.a{a}")),
        (None, Some(m)) => Some(format!("media.known.m{m}")),
        _ => None,
    }
}

/// BUG-143: completes a fallback answer. With a complete entry its title leads (extensions search with the first
/// title: Toko finds 25 OG4 torrents with "Overgeared" first, 13 with "Temppal" first), its titles win on the same
/// key, the fallback only adds new keys / spellings, and its poster stays (the history row is written from it).
/// Without one, the history's title (`(title, poster)`) leads when the fallback lacks an English title, and its
/// poster stays. Pure.
fn complete_fallback(m: &mut Media, known: Option<&KnownMedia>, history: Option<(String, Option<String>)>) {
    let has = |titles: &std::collections::BTreeMap<String, String>, v: &str| {
        titles.values().any(|x| x.trim().eq_ignore_ascii_case(v.trim()))
    };
    if let Some(k) = known {
        let mut titles = k.titles.clone();
        for (key, v) in std::mem::take(&mut m.titles) {
            if !titles.contains_key(&key) && !v.trim().is_empty() && !has(&titles, &v) {
                titles.insert(key, v);
            }
        }
        m.titles = titles;
        if !k.title.trim().is_empty() {
            m.title = k.title.clone();
        }
        if k.cover.is_some() {
            m.cover = k.cover.clone();
        }
        return;
    }
    let Some((title, poster)) = history else { return };
    if poster.is_some() {
        m.cover = poster;
    }
    if title.trim().is_empty() || m.titles.contains_key("english") {
        return;
    }
    // BUG-144: only the display keys count; Kitsu's `alt_N` ("OVERGEARED") is not shown, so it must not keep the
    // history title out of `english` (the English display would fall to Kitsu's romaji, "Temppal…").
    if !is_display_title(&m.titles, &title) {
        m.titles.insert("english".to_owned(), title.clone());
    }
    m.title = title;
}

/// Whether `v` is one of the shown titles (`english` / `romaji` / `native`, ignoring case). Pure.
fn is_display_title(titles: &std::collections::BTreeMap<String, String>, v: &str) -> bool {
    ["english", "romaji", "native"]
        .iter()
        .filter_map(|k| titles.get(*k))
        .any(|x| x.trim().eq_ignore_ascii_case(v.trim()))
}

/// BUG-144: a `KnownMedia` from the raw AniList objects Details already cached for the series (`anilist.chain.<key>`:
/// the card, with titles and poster; `anilist.extra.<key>`: titles), for a fallback answer without a stored complete
/// answer, so AniList's romaji ("Tempal: Item no Chikara") reaches the extensions instead of Kitsu's. A missing
/// English title / poster is taken from the history row (`(title, poster)`). `None` without AniList titles. Pure.
fn known_from_anilist_raw(
    chain: Option<&serde_json::Value>,
    extra: Option<&serde_json::Value>,
    history: Option<(String, Option<String>)>,
) -> Option<KnownMedia> {
    use tk_core::metadata::anilist::details::{media_titles, pick_title};
    let mut titles = std::collections::BTreeMap::new();
    let mut cover = None;
    if let Some(c) = chain {
        let card = tk_core::metadata::mapping::map_anilist_media(c);
        titles = card.titles;
        cover = card.cover;
    }
    for (k, v) in extra.map(media_titles).unwrap_or_default() {
        titles.entry(k).or_insert(v);
    }
    if titles.is_empty() {
        return None;
    }
    if let Some((title, poster)) = history {
        if !title.trim().is_empty() && !titles.contains_key("english") && !is_display_title(&titles, &title) {
            titles.insert("english".to_owned(), title);
        }
        if cover.is_none() {
            cover = poster;
        }
    }
    let title = pick_title(&titles, "english")?;
    Some(KnownMedia { title, titles, cover })
}

/// Whether a list that expires (or expired) at `until` may still serve a picked web candidate at `now`. Pure.
fn within_stale_grace(until: Instant, now: Instant) -> bool {
    now < until + STALE_WEB_GRACE
}

/// Whether the outbound route of the extensions differs between two saved settings. Pure.
fn route_changed(old: &Settings, new: &Settings) -> bool {
    let (a, b) = (old.server.clone().sanitized(), new.server.clone().sanitized());
    a.outbound_proxy != b.outbound_proxy || a.flaresolverr_url != b.flaresolverr_url
}

/// Whether an empty listing at `now` follows a route change closely enough to be asked again. Pure.
fn retry_after_route_change(changed_at: Option<Instant>, now: Instant) -> bool {
    changed_at.is_some_and(|at| now.saturating_duration_since(at) < NET_CHANGE_WINDOW)
}
/// History rows shown in the Library.
const LIBRARY_HISTORY: u32 = 200;

impl LiveBackend {
    /// `ui`: jobs for the UI thread (hidden web views of the extension browser runtime). `config.json` carries
    /// nothing the backend needs yet (the provider order comes from `Settings`); kept for future machine options.
    pub fn new(paths: &AppPaths, _cfg: &AppConfig, ui: UiThread) -> anyhow::Result<Self> {
        let settings_file = paths.ui_settings_file();
        let settings = settings_store::load(&settings_file);
        set_title_language(&settings.title_language);
        let store = Arc::new(LocalStore::open(&paths.db_file())?);
        let client = reqwest::Client::builder().user_agent("TatakaiNext/0.1").timeout(Duration::from_secs(20)).build()?;
        let router = Arc::new(MetadataRouter::with_defaults(client.clone(), settings.home.enabled_providers(&settings.provider_priority)));
        let browsers = Arc::new(browser::WebviewBrowsers::new(ui));
        let ext = ext::Extensions::new(paths.extensions_dir(), browsers.clone())?;
        // Wave B (k): remote commands from paired phones (`tk-link` `/v1/control`) flow through this channel.
        let (remote_tx, remote_rx) = tokio::sync::mpsc::unbounded_channel();
        // Wave A (f): the Link server also serves / merges the watch history of paired phones (`history_sync`).
        let link = link::LinkService::new(
            paths.link_dir(),
            ext.host(),
            store.clone(),
            crate::paths::shaders_dir(paths, None),
            // the lock pins the installer's `<exe>/shaders`; `<data>/shaders` is filled by `shaders::fetch` from the
            // SHA-256-checked mpv-anime-build zip, whose copies of two passes differ byte-wise from the lock
            crate::paths::anime4k_lock(None).filter(|_| !crate::paths::shaders_dir(paths, None).starts_with(&paths.root)),
            link::link_lsfg_dll(settings.video.lsfg_dll.as_deref()),
            remote_tx,
        );
        Ok(Self {
            data_root: paths.root.clone(),
            settings_file,
            store: store.clone(),
            router,
            titles: Mutex::new(HashMap::new()),
            ext,
            link,
            candidates: Mutex::new(HashMap::new()),
            torrent_memo: Mutex::new(TorrentMemo::default()),
            last_sources: Mutex::new(LastSources::default()),
            net_changed: Mutex::new(None),
            torrent: torrent::TorrentService::new(paths.clone()),
            secrets: tk_core::Secrets::open(&paths.root),
            downloads: downloads::Downloads::new(paths.clone(), store.clone(), client.clone()),
            server: server::ExtServer::new(paths.clone(), client.clone()),
            sniffer: sniffer::Sniffer::new(browsers),
            tts: crate::tts::Tts::new(paths.clone(), client),
            remote: tokio::sync::Mutex::new(remote_rx),
        })
    }

    /// The data directory layout (`AppPaths`) of this backend.
    fn paths(&self) -> AppPaths {
        AppPaths::from_root(self.data_root.clone())
    }

    /// BUG-009: hand the Link host a weak reference to this backend (torrent engine). Call once after `Arc::new`.
    pub fn bind_self(self: &Arc<Self>) {
        link::bind_backend(self);
    }

    /// Wave A: start-up jobs (`live/system.rs::on_start`, worker f: update notice). tk-app's `main.rs` spawns it
    /// through `Services::on_start` once the UI runtime is up; a stub until f lands.
    pub async fn on_start(&self) {
        self.link.start_if_paired().await;
        system::on_start(self).await;
        // Wave B start-up jobs, each a no-op until its owner lands: h resumes the download queue, verifies completed
        // rows and schedules the auto-downloader; k starts `tk-server` when `Settings::server.enabled`; j checks
        // extension updates (`Settings::ext.auto_check_updates`) and consumes a pending `tatakai://` server link.
        downloads::on_start(self).await;
        server::on_start(self).await;
        ext_server::on_start(self).await;
    }

    fn remember_title(&self, id: &str, title: &str, poster: Option<&str>) {
        if let Ok(mut t) = self.titles.lock() {
            t.insert(id.to_owned(), (title.to_owned(), poster.map(str::to_owned)));
        }
    }

    /// Whether `m` came from a fallback provider while the primary one (AniList) did not answer (rate limit, cool-down,
    /// outage) for a title the primary one knows. Kitsu-only ids and a user priority without AniList first are not.
    fn is_fallback(&self, r: &MediaRef, m: &Media) -> bool {
        self.router.primary().as_deref() == Some(PRIMARY_METADATA)
            && m.source_provider != PRIMARY_METADATA
            && (r.anilist_id.is_some() || r.mal_id.is_some())
    }

    /// BUG-002 / BUG-143: a fallback provider (Kitsu while AniList is rate-limited) can lack the English title and
    /// spell the romaji differently ("Temppal: Item no Chikara" for Overgeared): extensions then search with the wrong
    /// title first (Toko: 0 torrents) and the history row would take Kitsu's title and poster. The last complete
    /// AniList answer for that series (`media.known.*`, metadata cache lifetime) leads the titles and keeps its
    /// poster; without one, the playback history's title and poster do. A fallback answer is never stored itself.
    pub(crate) fn fill_known_title(&self, m: &mut Media) {
        let r = m.media_ref;
        self.fill_known_for(&r, m);
    }

    fn fill_known_for(&self, r: &MediaRef, m: &mut Media) {
        if !self.is_fallback(r, m) {
            return;
        }
        // BUG-144: titles do not go stale like the rest of an answer: an expired AniList row (the metadata lifetime
        // passed) still beats the fallback's titles, so these reads ignore the expiry.
        let any_age = |key: &str| self.store.cache_get_at::<serde_json::Value>(key, i64::MIN).ok().flatten();
        let history = || {
            self.progress(r).into_iter().find(|e| !e.title.trim().is_empty()).map(|e| (e.title, e.poster))
        };
        let mut known = known_key(r).and_then(|k| any_age(&k)).and_then(|v| serde_json::from_value::<KnownMedia>(v).ok());
        let mut from = "anilist cache";
        if known.is_none()
            && let Some(key) = details::ref_key(r)
        {
            let chain = any_age(&format!("anilist.chain.{key}"));
            let extra = any_age(&format!("anilist.extra.{key}"));
            known = known_from_anilist_raw(chain.as_ref(), extra.as_ref(), history());
            from = "anilist details cache";
        }
        let history = if known.is_none() {
            from = "history";
            history()
        } else {
            None
        };
        complete_fallback(m, known.as_ref(), history);
        tracing::warn!(
            provider = %m.source_provider,
            cooling_down = ?self.router.cooling_down(),
            titles_from = from,
            title = %m.title,
            "metadata fallback: AniList did not answer, completing {} with the known titles",
            m.source_provider
        );
    }

    /// BUG-143: a complete AniList answer is remembered (titles + poster) for the fallback answers that may follow.
    fn remember_known(&self, r: &MediaRef, m: &Media) {
        if m.source_provider != PRIMARY_METADATA {
            return;
        }
        let Some(key) = known_key(r) else { return };
        let k = KnownMedia::of(m);
        if self.store.cache_get::<KnownMedia>(&key).ok().flatten().as_ref() == Some(&k) {
            return;
        }
        if let Err(e) = self.store.cache_put(&key, &k, system::metadata_ttl_secs(self)) {
            tracing::warn!(%e, key, "writing the known titles failed");
        }
    }

    async fn media(&self, id: &str) -> UiResult<(MediaRef, Media)> {
        let r = media_ref(id).ok_or_else(|| UiError::new(ErrorKind::Other, format!("invalid id {id}")))?;
        let mut media = self
            .router
            .media(MediaKind::Anime, &r)
            .await
            .map_err(provider_err)?
            .ok_or_else(|| UiError::new(ErrorKind::Other, format!("{id} was not found")))?;
        self.remember_known(&r, &media);
        self.fill_known_for(&r, &mut media);
        self.remember_title(id, &media.title, media.cover.as_deref());
        Ok((r, media))
    }

    fn progress(&self, r: &MediaRef) -> Vec<HistoryEntry> {
        HistoryEntry::anime_key(r).and_then(|k| self.store.progress_for(&k).ok()).unwrap_or_default()
    }

    fn store_err(e: impl std::fmt::Display) -> UiError {
        UiError::new(ErrorKind::Other, e.to_string())
    }

    /// "Continue watching": the playback history merged with the list mirror (AniList-first: a series at 2/12 on
    /// AniList shows episode 3 even if it was never played here; `tk_core::lists::progress::continue_rows`).
    fn continue_vms(&self, limit: usize) -> Vec<ContinueVm> {
        let history = self.store.history(500).unwrap_or_default();
        let mut list = self.store.list_entries(tk_core::lists::sync::LOCAL, Some("watching")).unwrap_or_default();
        list.extend(self.store.list_entries(tk_core::lists::sync::LOCAL, Some("repeating")).unwrap_or_default());
        tk_core::lists::progress::continue_rows(history, &list, limit)
            .into_iter()
            .map(|e| {
                self.remember_title(&e.anime_id, &e.title, e.poster.as_deref());
                ContinueVm {
                    media: MediaVm { id: e.anime_id, title: e.title, cover: e.poster, color: None, subtitle: String::new(), score: None },
                    episode: e.episode,
                    progress_secs: e.progress_sec,
                    duration_secs: e.duration_sec,
                }
            })
            .collect()
    }

    /// The list mirror row of a title (AniList-first progress), if any.
    fn list_row(&self, r: &MediaRef) -> Option<tk_core::store::lists::ListEntryRow> {
        HistoryEntry::anime_key(r).and_then(|k| self.store.list_entry(tk_core::lists::sync::LOCAL, &k).ok().flatten())
    }

    fn start_secs(&self, r: &MediaRef, episode: u32) -> u32 {
        self.progress(r).iter().find(|e| e.episode == episode && !e.completed).map_or(0, |e| e.progress_sec)
    }

    fn invalidate_sources(&self) {
        if let Ok(mut c) = self.candidates.lock() {
            c.clear();
        }
        if let Ok(mut m) = self.torrent_memo.lock() {
            m.clear();
        }
        if let Ok(mut l) = self.last_sources.lock() {
            l.clear();
        }
    }

    /// BUG-134: the web candidate `source` of a cached list of `(id, episode)` that expired less than
    /// `STALE_WEB_GRACE` ago (a fresh one is found by `episode_candidates` itself). No network.
    fn recent_web_candidate(&self, id: &str, episode: u32, source: &str) -> Option<SourceCandidate> {
        let now = Instant::now();
        let c = self.candidates.lock().ok()?;
        let (until, list) = c.get(&(id.to_owned(), episode))?;
        if !within_stale_grace(*until, now) {
            return None;
        }
        list.iter().find(|c| c.kind == SourceKind::Web && source_key(c) == source).cloned()
    }

    /// BUG-134: a torrent candidate some listing of `id` returned, even if that list expired (see `TorrentMemo`).
    fn remembered_torrent(&self, id: &str, episode: Option<u32>, source: &str) -> Option<SourceCandidate> {
        self.torrent_memo.lock().ok().and_then(|m| m.find(id, episode, source))
    }

    /// Candidates of an episode: from the cache unless stale or `fresh`, else resolved through the extensions.
    /// Reliability is always re-read from the store (it changes with every `report_source_result`).
    async fn episode_candidates(&self, id: &str, episode: u32, media: &Media, fresh: bool) -> Vec<SourceCandidate> {
        let key = (id.to_owned(), episode);
        let cached = if fresh {
            None
        } else {
            self.candidates
                .lock()
                .ok()
                .and_then(|c| c.get(&key).filter(|(until, _)| Instant::now() < *until).map(|(_, v)| v.clone()))
        };
        let mut list = match cached {
            Some(v) => v,
            None => {
                // `resolve_candidates` keeps server/subs/dubs/note and lists torrents (resolve_streams does not).
                // Wave B (j): paired extension servers and Stremio add-ons contribute candidates too.
                // `fresh` also skips the extensions' own route caches (BUG-102: they kept an empty list 10-30 min).
                let listing = self.ext.resolve_candidates(media, episode, fresh).await;
                let mut v = listing.candidates;
                v.extend(ext_catalog::extra_candidates(self, media, episode).await);
                let v = sources::dedupe_by_key(v);
                if let Ok(mut m) = self.torrent_memo.lock() {
                    m.remember(key.clone(), &v);
                }
                // An empty listing (sites unreachable, proxy down: BUG-082) is not kept, so "Retry" asks again; a
                // partial one only briefly. BUG-143: so is one asked with a fallback provider's titles (AniList
                // rate-limited), so the next open asks again with AniList's.
                let fallback = media_ref(id).is_some_and(|r| self.is_fallback(&r, media));
                if let Some(ttl) = candidates_ttl(v.len(), listing.partial || fallback)
                    && let Ok(mut c) = self.candidates.lock()
                {
                    let now = Instant::now();
                    // Expired lists stay a while for `recent_web_candidate` (BUG-134); lookups here skip them.
                    c.retain(|_, (until, _)| within_stale_grace(*until, now));
                    c.insert(key, (now + ttl, v.clone()));
                }
                v
            }
        };
        for c in &mut list {
            c.reliability = self.store.reliability(&c.ext_id, &c.label).unwrap_or(INITIAL_RELIABILITY);
        }
        list
    }
}

impl LiveBackend {
    /// `Backend::sources` of a catalog title: every extension's candidates, ranked with the current prefs.
    async fn ranked_sources(&self, id: &str, episode: u32) -> UiResult<Vec<SourceVm>> {
        let (_, media) = self.media(id).await?;
        let settings = settings_store::load(&self.settings_file);
        let prefs = sources::core_prefs(&settings.sources);
        let enabled: HashMap<String, bool> =
            self.ext.installed().into_iter().map(|e| (e.id, e.enabled && e.compat != Compat::Unsupported)).collect();
        let picked = self.store.picked_source(id, episode).ok().flatten();
        let rank = |mut candidates: Vec<SourceCandidate>| {
            // Wave B (k, `sources.embed-sniffer.timeout-setting`): with the sniffer off, embed pages are not offered.
            if !settings.server.sniffer_enabled {
                candidates.retain(|c| !c.web.as_ref().is_some_and(|w| w.embed));
            }
            sources::ranked_vms(candidates, &prefs, |ext_id| enabled.get(ext_id).copied().unwrap_or(ext_catalog::is_remote_ext_id(ext_id)), picked.as_deref())
        };
        let mut ranked = rank(self.episode_candidates(id, episode, &media, false).await);
        let has_sources_ext = self.ext.host().has_source_extensions();
        let changed_at = self.net_changed.lock().ok().and_then(|g| *g);
        if ranked.is_empty() && has_sources_ext && retry_after_route_change(changed_at, Instant::now()) {
            // BUG-102: the first listing after the proxy changed can come back empty (extension work still running
            // on the old route); ask once more before saying "No sources found".
            tracing::info!("empty source listing right after an outbound-route change; asking the extensions again");
            if let Ok(mut g) = self.net_changed.lock() {
                *g = None;
            }
            tokio::time::sleep(NET_CHANGE_RETRY_PAUSE).await;
            ranked = rank(self.episode_candidates(id, episode, &media, true).await);
        }
        if ranked.is_empty() && has_sources_ext {
            return Err(self.why_no_sources(&settings).await);
        }
        // Wave A, worker i: `SourceVm::debrid` from the connected debrid services (best effort). Wave B
        // (`sources.debrid.episode-context`, k): the episode context travels with the call instead of being guessed.
        Ok(debrid::mark_cached(self, id, episode, ranked).await)
    }

    /// BUG-082: installed source extensions listed nothing. With an outbound proxy that does not answer, that is the
    /// reason (every extension fetch goes through it); otherwise they found nothing. Never "nothing installed".
    async fn why_no_sources(&self, settings: &Settings) -> UiError {
        if let Some(proxy) = settings.server.outbound_proxy.as_deref().filter(|p| !p.trim().is_empty()) {
            match tk_server::upstream::probe_proxy(proxy.trim(), NO_SOURCES_PROXY_PROBE).await {
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(error = %e, "no extension listed a source: the outbound proxy does not answer");
                    return UiError::new(ErrorKind::Offline, tk_ui::model::SOURCES_PROXY_DOWN);
                }
            }
        }
        tracing::warn!("no extension listed a source for this episode");
        UiError::new(ErrorKind::NoSource, tk_ui::model::SOURCES_NONE_FOUND)
    }
}

/// How long the outbound-proxy check of [`LiveBackend::why_no_sources`] waits.
const NO_SOURCES_PROXY_PROBE: Duration = Duration::from_secs(6);

/// Episodes past the season's `total` (AniList) are dropped unless they already aired (`air_date` before `today`,
/// both `YYYY-MM-DD`): AniList's count can lag behind a season that is still airing, never the other way round.
fn cap_to_season(eps: Vec<tk_core::Episode>, total: Option<u32>, today: &str) -> Vec<tk_core::Episode> {
    let Some(total) = total.filter(|t| *t > 0) else { return eps };
    eps.into_iter()
        .filter(|e| e.number <= total || e.air_date.as_deref().is_some_and(|d| d.len() >= 10 && &d[..10] < today))
        .collect()
}

fn media_ref(id: &str) -> Option<MediaRef> {
    let (kind, n) = parse_media_id(id)?;
    Some(match kind {
        IdKind::AniList => MediaRef::anilist(n),
        IdKind::Mal => MediaRef { mal_id: Some(n), ..MediaRef::default() },
        IdKind::Kitsu => MediaRef { kitsu_id: Some(n), ..MediaRef::default() },
    })
}

fn media_id(r: &MediaRef) -> String {
    match (r.anilist_id, r.mal_id, r.kitsu_id) {
        (Some(a), _, _) => a.to_string(),
        (_, Some(m), _) => format!("mal:{m}"),
        (_, _, Some(k)) => format!("kitsu:{k}"),
        _ => String::new(),
    }
}

/// `Settings::title_language` (Settings > Appearance, wave A worker c), mirrored here so every card mapper picks the
/// same title without reading the settings file per row. Set at start and on every `save_settings`.
static TITLE_LANGUAGE: std::sync::RwLock<String> = std::sync::RwLock::new(String::new());

fn set_title_language(lang: &str) {
    if let Ok(mut l) = TITLE_LANGUAGE.write() {
        *l = lang.to_owned();
    }
}

fn to_vm(m: &Media) -> MediaVm {
    let title = match TITLE_LANGUAGE.read() {
        Ok(l) if !l.is_empty() => details::display_title(m, &l),
        _ => m.title.clone(),
    };
    let mut parts = Vec::new();
    if let Some(y) = m.year {
        parts.push(y.to_string());
    }
    if let Some(e) = m.episodes {
        parts.push(format!("{e} ep"));
    }
    MediaVm {
        id: media_id(&m.media_ref),
        title,
        cover: m.cover.clone(),
        color: m.color.clone(),
        subtitle: parts.join(" - "),
        score: m.score,
    }
}

/// Poster card of a history row (the store keeps title + poster; the id is the UI id).
fn history_media(e: &HistoryEntry) -> MediaVm {
    MediaVm { id: e.anime_id.clone(), title: e.title.clone(), cover: e.poster.clone(), color: None, subtitle: String::new(), score: None }
}

fn provider_err(e: ProviderError) -> UiError {
    let kind = match &e {
        ProviderError::RateLimited { .. } => ErrorKind::RateLimited,
        ProviderError::Network(_) => ErrorKind::Offline,
        _ => ErrorKind::Other,
    };
    UiError::new(kind, e.to_string())
}

#[async_trait]
impl Backend for LiveBackend {
    // Wave B: the Home body moved to `live/home.rs` (worker b) so b extends it without editing this file.
    async fn home(&self) -> UiResult<HomeVm> {
        home::home(self).await
    }

    async fn search(&self, query: &str, page: u32) -> UiResult<Vec<MediaVm>> {
        let mut p = self.router.search(MediaKind::Anime, query, page).await.map_err(provider_err)?;
        p.items.iter_mut().for_each(|m| self.fill_known_title(m));
        Ok(p.items.iter().map(to_vm).collect())
    }

    async fn details(&self, id: &str) -> UiResult<DetailsVm> {
        // Wave B (j, `ext.custom-providers.info`): provider titles (`ext:<ext_id>:<key>`) come from the extension.
        if ext_catalog::is_provider_id(id) {
            return ext_catalog::details(self, id).await;
        }
        let (r, media) = self.media(id).await?;
        // History position, or the list progress when it is further (AniList-first).
        let (resume_episode, resume_secs) = tk_core::lists::progress::resume(&self.progress(&r), self.list_row(&r).as_ref());
        Ok(DetailsVm {
            media: to_vm(&media),
            banner: media.banner.clone(),
            // Wave B (b, `metadata.providers.mal`): MAL's synopsis fills the gap when the router has none.
            synopsis: match media.synopsis.clone() {
                Some(s) => Some(s),
                None => home::external_synopsis(self, media.media_ref.mal_id).await,
            },
            genres: media.genres.clone(),
            status: media.status.clone(),
            episodes_total: media.episodes,
            resume_episode,
            resume_secs,
        })
    }

    async fn episodes(&self, id: &str) -> UiResult<Vec<EpisodeVm>> {
        if ext_catalog::is_provider_id(id) {
            return ext_catalog::episodes(self, id).await;
        }
        let r = media_ref(id).ok_or_else(|| UiError::new(ErrorKind::Other, format!("invalid id {id}")))?;
        let watched: HashMap<u32, f64> = self
            .progress(&r)
            .iter()
            .map(|e| {
                let f = if e.completed {
                    1.0
                } else {
                    e.duration_sec.filter(|d| *d > 0).map_or(0.0, |d| (f64::from(e.progress_sec) / f64::from(d)).clamp(0.0, 1.0))
                };
                (e.episode, f)
            })
            .collect();
        let list_progress = self.list_row(&r).map_or(0, |l| l.progress);
        let eps = self.router.episodes(&r).await.map_err(provider_err)?;
        // BUG-031: an episode source (TVDB through ani.zip) can list a continuing series' future episodes (Overgeared:
        // 22 for a 12-episode season); AniList's count wins for the ones that have not aired.
        let total = self.media(id).await.ok().and_then(|(_, m)| m.episodes);
        let today = OffsetDateTime::now_utc().date().to_string();
        let eps = cap_to_season(eps, total, &today);
        Ok(eps
            .into_iter()
            .map(|e| EpisodeVm {
                number: e.number,
                title: e.title,
                image: e.image,
                air_date: e.air_date,
                is_filler: e.is_filler,
                watched: tk_core::lists::progress::watched_fraction(e.number, watched.get(&e.number).copied().unwrap_or(0.0), list_progress),
                summary: e.summary,
                runtime_min: e.runtime_min,
                rating: e.rating,
            })
            .collect())
    }

    async fn set_episode_watched(&self, id: &str, episode: u32, watched: bool) -> UiResult<()> {
        lists::set_episode_watched(self, id, episode, watched).await
    }

    async fn playback(&self, id: &str, episode: u32) -> UiResult<PlaybackVm> {
        // Wave B (h, `downloads.local-library.play-offline`): a completed download / local file plays without network.
        if let Some(local) = downloads::local_playback(self, id, episode).await? {
            return Ok(local);
        }
        if ext_catalog::is_provider_id(id) {
            return ext_catalog::playback(self, id, episode).await;
        }
        let (r, media) = self.media(id).await?;
        let streams = self.ext.resolve(&media, episode).await;
        if streams.is_empty() {
            return Err(UiError::new(ErrorKind::NoSource, ""));
        }
        Ok(PlaybackVm {
            title: media.title.clone(),
            poster: media.cover.clone(),
            episodes_total: media.episodes,
            start_secs: self.start_secs(&r, episode),
            // Wave B (k, `server.cors-proxy-local`): web streams go through the loopback media proxy when it is on.
            streams: {
                let mut out = Vec::new();
                for s in streams {
                    out.push(server::local_stream(self, sources::stream_vm(s)).await);
                }
                out
            },
        })
    }

    async fn record_progress(&self, id: &str, episode: u32, progress_secs: u32, duration_secs: Option<u32>) -> UiResult<()> {
        // Wave B (k, `link.remote-control`): feeds `GET /v1/player` of paired phones.
        link::note_progress(self, id, episode, progress_secs, duration_secs);
        // Wave B (j, `ext.custom-providers.watch` / `.continue`): provider titles keep their own history rows.
        if ext_catalog::is_provider_id(id) {
            return ext_catalog::record_progress(self, id, episode, progress_secs, duration_secs).await;
        }
        let r = media_ref(id).ok_or_else(|| UiError::new(ErrorKind::Other, format!("invalid id {id}")))?;
        let (title, poster) = self.titles.lock().ok().and_then(|t| t.get(id).cloned()).unwrap_or_default();
        let Some(entry) = HistoryEntry::for_playback(
            &r,
            &title,
            poster.as_deref(),
            episode,
            progress_secs,
            duration_secs,
            OffsetDateTime::now_utc(),
        ) else {
            // Kitsu-only references are never written (E9).
            return Ok(());
        };
        self.store.record_history(&entry).map_err(|e| UiError::new(ErrorKind::Other, e.to_string()))?;
        if entry.completed {
            // The manual pick expires with the episode (docs/redesign/08 section 4).
            if let Err(e) = self.store.clear_picked_source(id, episode) {
                tracing::warn!(error = %e, "could not clear the picked source");
            }
        }
        // Wave A, worker g: automatic list progress (never fails the local write). The row itself stays local:
        // paired phones pull it over Tatakai Link (`GET /v1/history`), there is no cloud push any more.
        lists::on_progress(self, id, episode, progress_secs, duration_secs, entry.completed).await;
        Ok(())
    }

    async fn extensions(&self) -> UiResult<Vec<ExtensionVm>> {
        self.ext.list().await
    }
    async fn install_extension(&self, path: &str) -> UiResult<ExtensionVm> {
        let r = self.ext.install(path).await;
        self.invalidate_sources();
        r
    }
    async fn set_extension_enabled(&self, id: &str, enabled: bool) -> UiResult<()> {
        let r = self.ext.set_enabled(id, enabled).await;
        self.invalidate_sources();
        r
    }
    async fn remove_extension(&self, id: &str) -> UiResult<()> {
        let r = self.ext.remove(id).await;
        self.invalidate_sources();
        r
    }

    async fn load_settings(&self) -> UiResult<Settings> {
        Ok(settings_store::load(&self.settings_file))
    }
    async fn save_settings(&self, settings: &Settings) -> UiResult<()> {
        // Wave B (b, `settings.metadata.extra-providers`): switched-off providers are not routed.
        self.router.set_priority(settings.home.enabled_providers(&settings.provider_priority));
        set_title_language(&settings.title_language);
        let route_moved = route_changed(&settings_store::load(&self.settings_file), settings);
        settings_store::save(&self.settings_file, settings).map_err(|e| UiError::new(ErrorKind::Other, e.to_string()))?;
        // Wave B: services that keep a copy of their prefs (no-ops until their owners land).
        downloads::on_settings(self, settings).await;
        server::on_settings(self, settings).await;
        if route_moved {
            // BUG-102: lists resolved through the old route are dropped (ours, and the route caches inside each
            // extension: their runtimes restart), and the next empty one is asked again.
            self.invalidate_sources();
            self.ext.host().restart_workers();
            if let Ok(mut g) = self.net_changed.lock() {
                *g = Some(Instant::now());
            }
        }
        Ok(())
    }

    async fn link_status(&self) -> UiResult<LinkStatusVm> {
        self.link.status().await
    }
    async fn link_peek(&self) -> UiResult<LinkStatusVm> {
        Ok(self.link.peek().await)
    }
    async fn link_open_pairing(&self) -> UiResult<Pairing> {
        self.link.open_pairing().await
    }
    async fn link_revoke(&self, device_id: &str) -> UiResult<()> {
        self.link.revoke(device_id).await
    }
    async fn link_allow_private_network(&self) -> UiResult<()> {
        crate::backend_common::allow_private_network(self.link.port().await).await
    }

    // --- wave 1 (docs/redesign/08) ---

    async fn sources(&self, id: &str, episode: u32) -> UiResult<Vec<SourceVm>> {
        let listed = if ext_catalog::is_provider_id(id) {
            ext_catalog::sources(self, id, episode).await
        } else {
            self.ranked_sources(id, episode).await
        };
        if let Ok(list) = &listed
            && let Ok(mut l) = self.last_sources.lock()
        {
            l.remember((id.to_owned(), episode), list);
        }
        listed
    }

    fn known_sources(&self, id: &str, episode: u32) -> Option<Vec<SourceVm>> {
        self.last_sources.lock().ok().and_then(|l| l.get(id, episode))
    }

    fn picked_source_key(&self, id: &str, episode: u32) -> Option<String> {
        self.store.picked_source(id, episode).ok().flatten()
    }

    async fn mpv_hls_master(&self, url: &str, headers: &[(String, String)]) -> Option<String> {
        if !hls::applies(url, headers) {
            return None;
        }
        let prefs = settings_store::load(&self.settings_file).sources;
        let dir = self.data_root.join("cache").join("mpv-hls");
        hls::filtered_master(&dir, url, &prefs.dub_lang, &prefs.sub_lang).await
    }

    async fn playback_source(&self, id: &str, episode: u32, source_id: &str) -> UiResult<PlaybackVm> {
        // Wave B (j, `ext.custom-providers.watch`): provider titles resolve through their extension.
        if ext_catalog::is_provider_id(id) {
            return ext_catalog::playback_source(self, id, episode, source_id).await;
        }
        let (_, kind, _) =
            parse_source_key(source_id).ok_or_else(|| UiError::new(ErrorKind::Other, format!("invalid source id {source_id}")))?;
        if kind == SourceKind::Torrent {
            return Err(UiError::new(ErrorKind::TorrentM2, source_id));
        }
        let (r, media) = self.media(id).await?;
        let find = |list: Vec<SourceCandidate>| list.into_iter().find(|c| source_key(c) == source_id);
        // BUG-134: a web source picked from a list that just expired plays from that list instead of waiting for
        // every extension again (the player refreshes its list behind). A cached list may be stale (URL expired,
        // extension re-enabled): resolve again once before giving up.
        let candidate = match self.recent_web_candidate(id, episode, source_id) {
            Some(c) => Some(c),
            None => match find(self.episode_candidates(id, episode, &media, false).await) {
                Some(c) => Some(c),
                None => find(self.episode_candidates(id, episode, &media, true).await),
            },
        };
        let candidate = candidate.ok_or_else(|| UiError::new(ErrorKind::NoSource, source_id))?;
        // Wave B (k, `sources.embed-sniffer`): an embed page is resolved to its media URL in the hidden web view first.
        if candidate.web.as_ref().is_some_and(|w| w.embed) {
            return sniffer::playback_embed(self, &r, &media, episode, &candidate).await;
        }
        let stream = match candidate.kind {
            SourceKind::Torrent => return Err(UiError::new(ErrorKind::TorrentM2, source_id)),
            SourceKind::Web => candidate.to_stream().ok_or_else(|| UiError::new(ErrorKind::NoSource, source_id))?,
        };
        Ok(PlaybackVm {
            title: media.title.clone(),
            poster: media.cover.clone(),
            episodes_total: media.episodes,
            start_secs: self.start_secs(&r, episode),
            streams: vec![server::local_stream(self, sources::stream_vm(stream)).await],
        })
    }

    async fn report_source_result(&self, source_id: &str, ok: bool) -> UiResult<()> {
        let (ext_id, _, label) =
            parse_source_key(source_id).ok_or_else(|| UiError::new(ErrorKind::Other, format!("invalid source id {source_id}")))?;
        self.store.record_source_result(&ext_id, &label, ok).map(|_| ()).map_err(Self::store_err)
    }

    async fn set_picked_source(&self, id: &str, episode: u32, source_id: Option<&str>) -> UiResult<()> {
        let r = match source_id {
            Some(key) => self.store.set_picked_source(id, episode, key),
            None => self.store.clear_picked_source(id, episode),
        };
        r.map_err(Self::store_err)
    }

    async fn library(&self) -> UiResult<LibraryVm> {
        let history = self
            .store
            .history(LIBRARY_HISTORY)
            .map_err(Self::store_err)?
            .into_iter()
            .map(|e| HistoryItemVm { media: history_media(&e), episode: e.episode, watched_unix: e.watched_at.unix_timestamp() })
            .collect();
        Ok(LibraryVm { continue_watching: self.continue_vms(40), history })
    }

    async fn extension_logs(&self, id: &str) -> UiResult<Vec<ExtLogVm>> {
        self.ext.logs(id).await
    }
    async fn extension_detail(&self, id: &str) -> UiResult<ExtensionDetailVm> {
        self.ext.detail(id).await
    }

    async fn link_restart(&self, port: u16) -> UiResult<LinkStatusVm> {
        self.link.restart(port).await
    }
    async fn link_rename_device(&self, device_id: &str, name: &str) -> UiResult<()> {
        self.link.rename(device_id, name).await
    }
    async fn link_capabilities(&self) -> UiResult<LinkCapsVm> {
        self.link.capabilities().await
    }

    // --- wave 2 (docs/redesign/09): bodies live in `crate::anime_build` (B). ---

    async fn anime_build_status(&self) -> UiResult<AnimeBuildVm> {
        Ok(crate::anime_build::status(&AppPaths::from_root(self.data_root.clone())))
    }
    async fn fetch_shaders(&self) -> UiResult<u32> {
        crate::shaders::fetch(&crate::paths::shaders_dir(&AppPaths::from_root(self.data_root.clone()), None)).await
    }
    async fn prepare_anime_build(&self) -> UiResult<std::path::PathBuf> {
        let paths = AppPaths::from_root(self.data_root.clone());
        tokio::task::spawn_blocking(move || crate::anime_build::prepare(&paths))
            .await
            .map_err(|e| UiError::new(ErrorKind::Other, e.to_string()))?
    }
    fn data_dir_display(&self) -> String {
        self.data_root.display().to_string()
    }

    // --- wave 3 (docs/redesign/10 section 4): bodies in live/torrent.rs (A), live/torrent_play.rs (B),
    // live/torrent_settings.rs (D). Nobody edits these delegations. ---

    async fn torrent_play(&self, source: &str, id: &str, episode: u32) -> UiResult<TorrentPlaybackVm> {
        torrent_play::play(self, source, id, episode).await
    }
    async fn torrent_playback_ended(&self, torrent_id: &str, completed: bool) -> UiResult<()> {
        torrent_settings::playback_ended(self, torrent_id, completed).await
    }
    async fn torrent_add_download(&self, source: &str, id: &str, episode: Option<u32>) -> UiResult<String> {
        torrent_settings::add_download(self, source, id, episode).await
    }
    async fn torrent_stats(&self, torrent_id: &str) -> UiResult<TorrentStatsVm> {
        self.torrent.stats(torrent_id).await
    }
    async fn torrent_files(&self, torrent_id: &str) -> UiResult<Vec<TorrentFileVm>> {
        self.torrent.files(torrent_id).await
    }
    async fn torrent_peers(&self, torrent_id: &str) -> UiResult<Vec<TorrentPeerVm>> {
        self.torrent.peers(torrent_id).await
    }
    async fn torrent_trackers(&self, torrent_id: &str) -> UiResult<Vec<TorrentTrackerVm>> {
        self.torrent.trackers(torrent_id).await
    }
    async fn torrent_library(&self) -> UiResult<Vec<TorrentLibraryItemVm>> {
        self.torrent.library().await
    }
    async fn torrent_remove(&self, torrent_id: &str, delete_files: bool) -> UiResult<()> {
        self.torrent.remove(torrent_id, delete_files).await
    }
    async fn torrent_pause(&self, torrent_id: &str) -> UiResult<()> {
        self.torrent.pause(torrent_id).await
    }
    async fn torrent_resume(&self, torrent_id: &str) -> UiResult<()> {
        self.torrent.resume(torrent_id).await
    }
    async fn torrent_set_persist(&self, torrent_id: &str, persist: bool) -> UiResult<()> {
        self.torrent.set_persist(torrent_id, persist).await
    }
    async fn torrent_open_folder(&self, torrent_id: &str) -> UiResult<()> {
        self.torrent.open_folder(torrent_id).await
    }
    async fn torrent_settings(&self) -> UiResult<TorrentSettingsVm> {
        torrent_settings::settings(self).await
    }
    async fn save_torrent_settings(&self, settings: &TorrentSettingsVm) -> UiResult<()> {
        torrent_settings::save_settings(self, settings).await
    }
    async fn background_downloads_ack(&self) -> UiResult<()> {
        torrent_settings::background_ack(self).await
    }
    async fn background_downloads_enabled(&self) -> UiResult<bool> {
        torrent_settings::background_enabled(self).await
    }
    async fn seek_marks(&self, torrent_id: &str, file_index: usize) -> UiResult<Vec<(f64, f64)>> {
        self.torrent.seek_marks(torrent_id, file_index).await
    }
    async fn skip_marks(&self, id: &str, episode: u32) -> UiResult<Vec<SkipMarkVm>> {
        torrent_play::skip_marks(self, id, episode).await
    }

    // --- wave A (docs/redesign/15-parity-wave-a-contract.md section 3): one delegation per method, bodies in
    // live/<area>.rs. Nobody edits these delegations. ---

    // a: live/search.rs
    async fn search_filtered(&self, q: &SearchQueryVm) -> UiResult<SearchPageVm> {
        search::search_filtered(self, q).await
    }
    async fn search_in_extension(&self, ext_id: &str, query: &str, page: u32) -> UiResult<Vec<MediaVm>> {
        search::search_in_extension(self, ext_id, query, page).await
    }
    async fn feed(&self, feed: &str, page: u32) -> UiResult<SearchPageVm> {
        search::feed(self, feed, page).await
    }
    async fn search_by_studio(&self, studio: &str, page: u32) -> UiResult<SearchPageVm> {
        search::search_by_studio(self, studio, page).await
    }
    async fn search_by_image(&self, image: ImageQueryVm) -> UiResult<Vec<ImageMatchVm>> {
        search::search_by_image(self, image).await
    }
    async fn character(&self, character_id: &str) -> UiResult<CharacterVm> {
        search::character(self, character_id).await
    }

    // c: live/details.rs
    async fn details_extra(&self, id: &str) -> UiResult<DetailsExtraVm> {
        details::details_extra(self, id).await
    }
    async fn watch_order(&self, id: &str) -> UiResult<Vec<WatchOrderEntryVm>> {
        details::watch_order(self, id).await
    }

    // d: live/subs.rs
    async fn fetch_subtitle(&self, url: &str, headers: &[(String, String)]) -> UiResult<String> {
        subs::fetch_subtitle(self, url, headers).await
    }
    async fn translate_cues(&self, req: &TranslateRequestVm) -> UiResult<Vec<CueVm>> {
        subs::translate_cues(self, req).await
    }
    async fn translation_test(&self) -> UiResult<TranslationTestVm> {
        subs::translation_test(self).await
    }
    async fn screenshot_path(&self, title: &str, episode: u32) -> UiResult<std::path::PathBuf> {
        subs::screenshot_path(self, title, episode).await
    }
    async fn set_translation_key(&self, provider: TranslationProviderVm, key: Option<&str>) -> UiResult<()> {
        subs::set_translation_key(self, provider, key).await
    }
    async fn translation_key_present(&self, provider: TranslationProviderVm) -> UiResult<bool> {
        subs::translation_key_present(self, provider).await
    }

    // e: live/presence.rs
    async fn presence(&self, state: &PresenceVm) -> UiResult<()> {
        // Wave B (k, `link.remote-control`): the PC player state for `GET /v1/player` and remote seek.
        link::note_presence(self, state);
        presence::presence(self, state).await
    }

    // f: live/system.rs
    async fn reset_app(&self, keep_downloads: bool) -> UiResult<()> {
        system::reset_app(self, keep_downloads).await
    }
    async fn storage_usage(&self) -> UiResult<StorageUsageVm> {
        system::storage_usage(self).await
    }
    async fn metadata_cache_usage(&self) -> UiResult<(u64, u64)> {
        system::metadata_cache(self).await
    }
    async fn clear_metadata_cache(&self) -> UiResult<u64> {
        system::clear_metadata_cache(self).await
    }
    async fn clear_watch_history(&self) -> UiResult<u32> {
        system::clear_watch_history(self).await
    }
    async fn history_last_sync_unix(&self) -> UiResult<Option<i64>> {
        system::history_last_sync(self).await
    }
    async fn clear_cache(&self) -> UiResult<u64> {
        system::clear_cache(self).await
    }
    async fn open_data_folder(&self) -> UiResult<()> {
        system::open_data_folder(self).await
    }
    async fn open_logs(&self) -> UiResult<()> {
        system::open_logs(self).await
    }
    async fn export_history(&self, format: ExportFormatVm) -> UiResult<std::path::PathBuf> {
        system::export_history(self, format).await
    }
    async fn licenses_text(&self) -> UiResult<String> {
        system::licenses_text(self).await
    }
    async fn check_updates(&self) -> UiResult<UpdateCheckVm> {
        system::check_updates(self).await
    }
    fn system_caps(&self) -> SystemCapsVm {
        system::system_caps(self)
    }
    async fn autostart_enabled(&self) -> UiResult<bool> {
        system::autostart_enabled(self).await
    }
    async fn set_autostart(&self, enabled: bool) -> UiResult<()> {
        system::set_autostart(self, enabled).await
    }
    async fn open_url(&self, url: &str) -> UiResult<()> {
        system::open_url(self, url).await
    }

    // g: live/lists.rs
    async fn list_accounts(&self) -> UiResult<Vec<ListAccountVm>> {
        lists::list_accounts(self).await
    }
    async fn list_connect(&self, service: ListServiceVm) -> UiResult<ListAccountVm> {
        lists::list_connect(self, service).await
    }
    async fn list_disconnect(&self, service: ListServiceVm) -> UiResult<()> {
        lists::list_disconnect(self, service).await
    }
    async fn list_entry(&self, media_id: &str) -> UiResult<Option<ListEntryVm>> {
        lists::list_entry(self, media_id).await
    }
    async fn set_list_entry(&self, media_id: &str, update: &ListEntryUpdateVm) -> UiResult<ListEntryVm> {
        lists::set_list_entry(self, media_id, update).await
    }
    async fn remove_list_entry(&self, media_id: &str) -> UiResult<()> {
        lists::remove_list_entry(self, media_id).await
    }
    async fn list_entries(&self, status: Option<ListStatusVm>) -> UiResult<Vec<ListEntryVm>> {
        lists::list_entries(self, status).await
    }
    async fn favorites(&self) -> UiResult<Vec<MediaVm>> {
        lists::favorites(self).await
    }
    async fn is_favorite(&self, media_id: &str) -> UiResult<bool> {
        lists::is_favorite(self, media_id).await
    }
    async fn set_favorite(&self, media_id: &str, favorite: bool) -> UiResult<()> {
        lists::set_favorite(self, media_id, favorite).await
    }
    async fn list_sync_now(&self) -> UiResult<ListSyncVm> {
        lists::list_sync_now(self).await
    }
    async fn list_conflicts(&self) -> UiResult<Vec<ListConflictVm>> {
        lists::list_conflicts(self).await
    }
    async fn resolve_list_conflict(&self, conflict_id: i64, keep: ConflictSideVm) -> UiResult<()> {
        lists::resolve_list_conflict(self, conflict_id, keep).await
    }
    async fn import_list(&self, path: &str) -> UiResult<ListImportVm> {
        lists::import_list(self, path).await
    }
    async fn export_list(&self, format: ExportFormatVm) -> UiResult<std::path::PathBuf> {
        lists::export_list(self, format).await
    }

    // i: live/torrent_manual.rs + live/debrid.rs
    async fn torrent_preview(&self, source: &str) -> UiResult<TorrentPreviewVm> {
        torrent_manual::torrent_preview(self, source).await
    }
    async fn torrent_add_manual(&self, source: &str, media_id: Option<&str>, alignment: &[(usize, u32)]) -> UiResult<String> {
        torrent_manual::torrent_add_manual(self, source, media_id, alignment).await
    }
    async fn torrent_preload_next(&self, id: &str, episode: u32) -> UiResult<()> {
        torrent_manual::torrent_preload_next(self, id, episode).await
    }
    async fn take_pending_magnet(&self) -> UiResult<Option<String>> {
        torrent_manual::take_pending_magnet(self).await
    }
    async fn magnet_handler_registered(&self) -> UiResult<bool> {
        torrent_manual::magnet_handler_registered(self).await
    }
    async fn set_magnet_handler(&self, enabled: bool) -> UiResult<()> {
        torrent_manual::set_magnet_handler(self, enabled).await
    }
    async fn torrent_session_history(&self, limit: u32) -> UiResult<Vec<TorrentSessionVm>> {
        torrent_manual::torrent_session_history(self, limit).await
    }
    async fn torrent_clear_session_history(&self) -> UiResult<u32> {
        torrent_manual::torrent_clear_session_history(self).await
    }
    async fn torrent_reannounce(&self, torrent_id: &str) -> UiResult<()> {
        torrent_manual::torrent_reannounce(self, torrent_id).await
    }
    async fn debrid_accounts(&self) -> UiResult<Vec<DebridAccountVm>> {
        debrid::debrid_accounts(self).await
    }
    async fn set_debrid_key(&self, service: DebridServiceVm, key: Option<&str>) -> UiResult<DebridAccountVm> {
        debrid::set_debrid_key(self, service, key).await
    }
    async fn set_debrid_order(&self, order: &[DebridServiceVm]) -> UiResult<Vec<DebridServiceVm>> {
        debrid::set_debrid_order(self, order).await
    }
    async fn debrid_play(&self, source: &str, id: &str, episode: u32) -> UiResult<PlaybackVm> {
        debrid::debrid_play(self, source, id, episode).await
    }

    // --- wave B (docs/redesign/16-parity-wave-b-contract.md section 3): one delegation per method, bodies in
    // live/<area>.rs. Nobody edits these delegations. ---

    // b: live/home.rs
    async fn latest_episodes(&self, page: u32) -> UiResult<Vec<LatestEpisodeVm>> {
        home::latest_episodes(self, page).await
    }
    async fn top10(&self, range: TopRangeVm) -> UiResult<Vec<MediaVm>> {
        home::top10(self, range).await
    }
    async fn because_you_watched(&self) -> UiResult<Vec<BecauseVm>> {
        home::because_you_watched(self).await
    }
    async fn genres(&self) -> UiResult<Vec<String>> {
        home::genres(self).await
    }
    async fn genre_feed(&self, genre: &str, page: u32) -> UiResult<SearchPageVm> {
        home::genre_feed(self, genre, page).await
    }
    async fn followed_updates(&self) -> UiResult<Vec<LatestEpisodeVm>> {
        home::followed_updates(self).await
    }
    async fn airing_schedule(&self, week_start_unix: i64) -> UiResult<Vec<AiringSlotVm>> {
        home::airing_schedule(self, week_start_unix).await
    }
    async fn tracked_shows(&self) -> UiResult<Vec<MediaVm>> {
        home::tracked_shows(self).await
    }
    async fn is_tracked(&self, media_id: &str) -> UiResult<bool> {
        home::is_tracked(self, media_id).await
    }
    async fn set_tracked(&self, media_id: &str, tracked: bool) -> UiResult<()> {
        home::set_tracked(self, media_id, tracked).await
    }
    async fn reminders_due(&self) -> UiResult<Vec<AiringSlotVm>> {
        home::reminders_due(self).await
    }
    async fn recommendations(&self, page: u32) -> UiResult<Vec<RecommendationVm>> {
        home::recommendations(self, page).await
    }
    async fn recs_feedback(&self, media_id: &str, feedback: RecsFeedbackVm) -> UiResult<()> {
        home::recs_feedback(self, media_id, feedback).await
    }
    async fn taste_profile(&self) -> UiResult<TasteProfileVm> {
        home::taste_profile(self).await
    }
    async fn atlas_points(&self) -> UiResult<Vec<AtlasPointVm>> {
        home::atlas_points(self).await
    }
    async fn torrent_activity(&self) -> UiResult<TorrentActivityVm> {
        home::torrent_activity(self).await
    }
    async fn provider_health(&self) -> UiResult<Vec<ProviderHealthVm>> {
        home::provider_health(self).await
    }

    // h: live/downloads.rs
    async fn downloads(&self) -> UiResult<Vec<DownloadVm>> {
        downloads::downloads(self).await
    }
    async fn download_enqueue(&self, media_id: &str, episode: u32, req: &DownloadRequestVm) -> UiResult<i64> {
        downloads::enqueue(self, media_id, episode, req).await
    }
    async fn download_enqueue_many(&self, media_id: &str, episodes: &[u32], req: &DownloadRequestVm) -> UiResult<Vec<i64>> {
        downloads::enqueue_many(self, media_id, episodes, req).await
    }
    async fn download_pause(&self, id: i64) -> UiResult<()> {
        downloads::pause(self, id).await
    }
    async fn download_resume(&self, id: i64) -> UiResult<()> {
        downloads::resume(self, id).await
    }
    async fn download_cancel(&self, id: i64, delete_file: bool) -> UiResult<()> {
        downloads::cancel(self, id, delete_file).await
    }
    async fn download_remove(&self, id: i64, delete_file: bool) -> UiResult<()> {
        downloads::remove(self, id, delete_file).await
    }
    async fn download_clear_history(&self) -> UiResult<u32> {
        downloads::clear_history(self).await
    }
    async fn download_open_folder(&self, id: i64) -> UiResult<()> {
        downloads::open_folder(self, id).await
    }
    async fn download_storage(&self) -> UiResult<DownloadStorageVm> {
        downloads::storage(self).await
    }
    async fn local_playback(&self, media_id: &str, episode: u32) -> UiResult<Option<PlaybackVm>> {
        downloads::local_playback(self, media_id, episode).await
    }
    async fn local_library(&self) -> UiResult<Vec<LocalItemVm>> {
        downloads::local_library(self).await
    }
    async fn library_scan(&self, dir: Option<&str>) -> UiResult<LibraryScanVm> {
        downloads::library_scan(self, dir).await
    }
    async fn library_import(&self, paths: &[String]) -> UiResult<LibraryScanVm> {
        downloads::library_import(self, paths).await
    }
    async fn local_match(&self, item_id: i64, media_id: &str, episode: u32) -> UiResult<()> {
        downloads::local_match(self, item_id, media_id, episode).await
    }
    async fn local_remove(&self, item_id: i64, delete_file: bool) -> UiResult<()> {
        downloads::local_remove(self, item_id, delete_file).await
    }
    async fn auto_download_check_now(&self) -> UiResult<AutoDownloadRunVm> {
        downloads::auto_check_now(self).await
    }
    async fn auto_download_last_run(&self) -> UiResult<Option<AutoDownloadRunVm>> {
        downloads::auto_last_run(self).await
    }
    async fn download_verify(&self, id: i64) -> UiResult<DownloadVm> {
        downloads::verify(self, id).await
    }

    // j: live/ext_server.rs (servers, deep link) + live/ext_catalog.rs (Stremio, updates, prefs, repos, providers)
    async fn ext_servers(&self) -> UiResult<Vec<ExtServerVm>> {
        ext_server::servers(self).await
    }
    async fn ext_server_pair(&self, req: &ExtServerPairVm) -> UiResult<ExtServerVm> {
        ext_server::pair(self, req).await
    }
    async fn ext_server_set_enabled(&self, id: &str, enabled: bool) -> UiResult<()> {
        let r = ext_server::set_enabled(self, id, enabled).await;
        self.invalidate_sources();
        r
    }
    async fn ext_server_remove(&self, id: &str) -> UiResult<()> {
        let r = ext_server::remove(self, id).await;
        self.invalidate_sources();
        r
    }
    async fn ext_server_test(&self, id: &str) -> UiResult<ExtServerVm> {
        ext_server::test(self, id).await
    }
    async fn ext_server_sources(&self, id: &str) -> UiResult<Vec<ExtSourceVm>> {
        ext_server::sources(self, id).await
    }
    async fn take_pending_server_link(&self) -> UiResult<Option<String>> {
        ext_server::take_pending_link(self).await
    }
    async fn stremio_addons(&self) -> UiResult<Vec<StremioAddonVm>> {
        ext_catalog::stremio_addons(self).await
    }
    async fn stremio_add(&self, manifest_url: &str) -> UiResult<StremioAddonVm> {
        let r = ext_catalog::stremio_add(self, manifest_url).await;
        self.invalidate_sources();
        r
    }
    async fn stremio_remove(&self, id: &str) -> UiResult<()> {
        let r = ext_catalog::stremio_remove(self, id).await;
        self.invalidate_sources();
        r
    }
    async fn stremio_set_enabled(&self, id: &str, enabled: bool) -> UiResult<()> {
        let r = ext_catalog::stremio_set_enabled(self, id, enabled).await;
        self.invalidate_sources();
        r
    }
    async fn ext_updates_check(&self) -> UiResult<Vec<ExtUpdateVm>> {
        ext_catalog::updates_check(self).await
    }
    async fn ext_update(&self, id: &str) -> UiResult<ExtensionVm> {
        let r = ext_catalog::update(self, id).await;
        self.invalidate_sources();
        r
    }
    async fn ext_update_all(&self) -> UiResult<u32> {
        let r = ext_catalog::update_all(self).await;
        self.invalidate_sources();
        r
    }
    async fn set_all_extensions_enabled(&self, enabled: bool) -> UiResult<u32> {
        let r = ext_catalog::set_all_enabled(self, enabled).await;
        self.invalidate_sources();
        r
    }
    async fn install_extension_url(&self, url: &str) -> UiResult<ExtensionVm> {
        let r = ext_catalog::install_url(self, url).await;
        self.invalidate_sources();
        r
    }
    async fn ext_preferences(&self, id: &str) -> UiResult<Vec<ExtPrefFieldVm>> {
        ext_catalog::preferences(self, id).await
    }
    async fn set_ext_preference(&self, id: &str, key: &str, value: &str) -> UiResult<()> {
        ext_catalog::set_preference(self, id, key, value).await
    }
    async fn ext_repos(&self) -> UiResult<Vec<ExtRepoVm>> {
        ext_catalog::repos(self).await
    }
    async fn ext_repo_add(&self, url: &str) -> UiResult<ExtRepoVm> {
        ext_catalog::repo_add(self, url).await
    }
    async fn ext_repo_remove(&self, id: &str) -> UiResult<()> {
        ext_catalog::repo_remove(self, id).await
    }
    async fn ext_repo_catalog(&self, id: &str, refresh: bool) -> UiResult<Vec<ExtCatalogItemVm>> {
        ext_catalog::repo_catalog(self, id, refresh).await
    }
    async fn custom_providers(&self) -> UiResult<Vec<CustomProviderVm>> {
        ext_catalog::custom_providers(self).await
    }
    async fn provider_home(&self, ext_id: &str) -> UiResult<Vec<SectionVm>> {
        ext_catalog::provider_home(self, ext_id).await
    }
    async fn provider_feed(&self, ext_id: &str, row: &str, page: u32) -> UiResult<SearchPageVm> {
        ext_catalog::provider_feed(self, ext_id, row, page).await
    }
    async fn provider_item(&self, ext_id: &str, item_key: &str) -> UiResult<ProviderItemVm> {
        ext_catalog::provider_item(self, ext_id, item_key).await
    }
    async fn provider_bookmarks(&self) -> UiResult<Vec<MediaVm>> {
        ext_catalog::provider_bookmarks(self).await
    }
    async fn set_provider_bookmark(&self, ext_id: &str, item_key: &str, bookmarked: bool) -> UiResult<()> {
        ext_catalog::set_provider_bookmark(self, ext_id, item_key, bookmarked).await
    }

    // k: live/server.rs (tk-server, proxy, FlareSolverr), live/sniffer.rs (embed sniffer), live/link.rs (Link extras)
    async fn server_status(&self) -> UiResult<ServerStatusVm> {
        server::status(self).await
    }
    async fn server_start(&self) -> UiResult<ServerStatusVm> {
        server::start(self).await
    }
    async fn server_stop(&self) -> UiResult<()> {
        server::stop(self).await
    }
    async fn server_open_pairing(&self) -> UiResult<ServerPairingVm> {
        server::open_pairing(self).await
    }
    async fn server_devices(&self) -> UiResult<Vec<ServerDeviceVm>> {
        server::devices(self).await
    }
    async fn server_revoke(&self, device_id: &str) -> UiResult<()> {
        server::revoke(self, device_id).await
    }
    async fn server_set_viewer_password(&self, password: Option<&str>) -> UiResult<()> {
        server::set_viewer_password(self, password).await
    }
    async fn server_logs(&self, after: u64) -> UiResult<ServerLogPageVm> {
        server::logs(self, after).await
    }
    async fn server_proxy_status(&self) -> UiResult<ProxyStatusVm> {
        server::proxy_status(self).await
    }
    async fn server_clear_cache(&self) -> UiResult<()> {
        server::clear_cache(self).await
    }
    async fn flaresolverr_test(&self, url: Option<&str>) -> UiResult<FlareSolverrVm> {
        server::flaresolverr_test(self, url).await
    }
    async fn sniff_embed(&self, req: &SniffRequestVm) -> UiResult<SniffResultVm> {
        sniffer::sniff(self, req).await
    }
    async fn send_to_device(&self, req: &SendToDeviceVm) -> UiResult<SendOutcomeVm> {
        link::send_to_device(self, req).await
    }
    async fn wait_remote_command(&self) -> UiResult<RemoteCommandVm> {
        link::wait_remote_command(self).await
    }

    // l: live/manga.rs (the `read_*` calls serve novels too)
    async fn manga_home(&self) -> UiResult<ReadHomeVm> {
        manga::home(self).await
    }
    async fn manga_followed_updates(&self) -> UiResult<Vec<MediaVm>> {
        manga::followed_updates(self).await
    }
    async fn manga_search(&self, q: &SearchQueryVm) -> UiResult<SearchPageVm> {
        manga::search(self, q).await
    }
    async fn manga_details(&self, id: &str) -> UiResult<MangaDetailsVm> {
        manga::details(self, id).await
    }
    async fn manga_chapters(&self, id: &str, refresh: bool) -> UiResult<Vec<ChapterProviderVm>> {
        manga::chapters(self, id, refresh).await
    }
    async fn manga_pages(&self, chapter: &ChapterRefVm) -> UiResult<Vec<PageVm>> {
        manga::pages(self, chapter).await
    }
    async fn read_progress(&self, kind: ReadKindVm, media_id: &str) -> UiResult<Vec<ReadProgressVm>> {
        manga::read_progress(self, kind, media_id).await
    }
    async fn set_read_progress(&self, kind: ReadKindVm, media_id: &str, progress: &ReadProgressVm) -> UiResult<()> {
        manga::set_read_progress(self, kind, media_id, progress).await
    }
    async fn read_list_entry(&self, kind: ReadKindVm, media_id: &str) -> UiResult<Option<ListEntryVm>> {
        manga::read_list_entry(self, kind, media_id).await
    }
    async fn set_read_list_entry(&self, kind: ReadKindVm, media_id: &str, update: &ListEntryUpdateVm) -> UiResult<ListEntryVm> {
        manga::set_read_list_entry(self, kind, media_id, update).await
    }
    async fn remove_read_list_entry(&self, kind: ReadKindVm, media_id: &str) -> UiResult<()> {
        manga::remove_read_list_entry(self, kind, media_id).await
    }
    async fn read_list_entries(&self, kind: ReadKindVm, status: Option<ListStatusVm>) -> UiResult<Vec<ListEntryVm>> {
        manga::read_list_entries(self, kind, status).await
    }
    async fn manga_download_chapters(&self, id: &str, chapters: &[ChapterRefVm]) -> UiResult<u32> {
        manga::download_chapters(self, id, chapters).await
    }
    async fn read_remove_offline(&self, kind: ReadKindVm, chapter: &ChapterRefVm) -> UiResult<()> {
        manga::remove_offline(self, kind, chapter).await
    }
    async fn read_clear_cache(&self, kind: ReadKindVm, media_id: &str) -> UiResult<()> {
        manga::clear_cache(self, kind, media_id).await
    }

    // m: live/novel.rs (+ crate::tts)
    async fn novel_home(&self) -> UiResult<ReadHomeVm> {
        novel::home(self).await
    }
    async fn novel_search(&self, q: &SearchQueryVm) -> UiResult<SearchPageVm> {
        novel::search(self, q).await
    }
    async fn novel_details(&self, id: &str) -> UiResult<NovelDetailsVm> {
        novel::details(self, id).await
    }
    async fn novel_chapters(&self, id: &str, translator: Option<&str>, refresh: bool) -> UiResult<Vec<ChapterProviderVm>> {
        novel::chapters(self, id, translator, refresh).await
    }
    async fn novel_content(&self, chapter: &ChapterRefVm) -> UiResult<NovelContentVm> {
        novel::content(self, chapter).await
    }
    async fn novel_check_sources(&self, id: &str) -> UiResult<Vec<(String, u32)>> {
        novel::check_sources(self, id).await
    }
    async fn novel_download_chapters(&self, id: &str, chapters: &[ChapterRefVm]) -> UiResult<u32> {
        novel::download_chapters(self, id, chapters).await
    }
    async fn tts_voices(&self, provider: TtsProviderVm) -> UiResult<Vec<TtsVoiceVm>> {
        novel::tts_voices(self, provider).await
    }
    async fn tts_speak(&self, req: &TtsRequestVm) -> UiResult<TtsSpeakVm> {
        novel::tts_speak(self, req).await
    }
    async fn tts_stop(&self) -> UiResult<()> {
        novel::tts_stop(self).await
    }
    async fn set_tts_key(&self, provider: TtsProviderVm, key: Option<&str>) -> UiResult<()> {
        novel::set_tts_key(self, provider, key).await
    }
    async fn tts_key_present(&self, provider: TtsProviderVm) -> UiResult<bool> {
        novel::tts_key_present(self, provider).await
    }
    async fn kitten_status(&self) -> UiResult<KittenStatusVm> {
        novel::kitten_status(self).await
    }
    async fn kitten_install(&self) -> UiResult<KittenStatusVm> {
        novel::kitten_install(self).await
    }
    async fn kitten_remove(&self) -> UiResult<()> {
        novel::kitten_remove(self).await
    }

    // n: live/social.rs
    async fn anilist_profile(&self, name: Option<&str>) -> UiResult<ProfileVm> {
        social::profile(self, name).await
    }
    async fn anilist_follow(&self, user_id: u64, follow: bool) -> UiResult<()> {
        social::follow(self, user_id, follow).await
    }
    async fn anilist_followers(&self, user_id: u64, page: u32) -> UiResult<UserPageVm> {
        social::followers(self, user_id, page).await
    }
    async fn anilist_following(&self, user_id: u64, page: u32) -> UiResult<UserPageVm> {
        social::following(self, user_id, page).await
    }
    async fn anilist_activity(&self, feed: &ActivityFeedVm, page: u32) -> UiResult<Vec<ActivityVm>> {
        social::activity(self, feed, page).await
    }
    async fn anilist_activity_replies(&self, activity_id: u64) -> UiResult<Vec<ActivityReplyVm>> {
        social::activity_replies(self, activity_id).await
    }
    async fn anilist_like_activity(&self, activity_id: u64, like: bool) -> UiResult<()> {
        social::like_activity(self, activity_id, like).await
    }
    async fn anilist_reply(&self, activity_id: u64, text: &str) -> UiResult<ActivityReplyVm> {
        social::reply(self, activity_id, text).await
    }
    async fn anilist_post_status(&self, text: &str) -> UiResult<ActivityVm> {
        social::post_status(self, text).await
    }
    async fn anilist_delete_activity(&self, activity_id: u64) -> UiResult<()> {
        social::delete_activity(self, activity_id).await
    }
    async fn anilist_notifications(&self, page: u32, unread_only: bool) -> UiResult<NotificationPageVm> {
        social::notifications(self, page, unread_only).await
    }
    async fn anilist_unread_count(&self) -> UiResult<u32> {
        social::unread_count(self).await
    }
    async fn anilist_mark_notifications_read(&self) -> UiResult<()> {
        social::mark_notifications_read(self).await
    }
    async fn anilist_reviews(&self, media_id: &str, page: u32) -> UiResult<Vec<ReviewVm>> {
        social::reviews(self, media_id, page).await
    }
    async fn anilist_review(&self, review_id: u64) -> UiResult<ReviewVm> {
        social::review(self, review_id).await
    }
}

#[cfg(test)]
mod season_cap_tests {
    use super::cap_to_season;

    fn ep(number: u32, air: Option<&str>) -> tk_core::Episode {
        tk_core::Episode { number, title: None, image: None, air_date: air.map(str::to_owned), is_filler: false, ..Default::default() }
    }

    #[test]
    fn future_episodes_past_the_season_are_dropped() {
        // BUG-031: Overgeared, 12 on AniList, 22 listed (8-22 not aired).
        let eps: Vec<_> = (1..=22).map(|n| ep(n, Some(if n <= 7 { "2026-10-01" } else { "2027-01-01" }))).collect();
        let out = cap_to_season(eps.clone(), Some(12), "2026-11-14");
        assert_eq!(out.len(), 12);
        // An aired episode past a lagging AniList count stays; no count keeps everything.
        let mut aired = eps[..13].to_vec();
        aired[12].air_date = Some("2026-11-01T00:00:00Z".into());
        assert_eq!(cap_to_season(aired, Some(12), "2026-11-14").len(), 13);
        assert_eq!(cap_to_season(eps.clone(), None, "2026-11-14").len(), 22);
        assert_eq!(cap_to_season(eps, Some(0), "2026-11-14").len(), 22);
    }
}

#[cfg(test)]
mod route_change_tests {
    use super::*;

    fn cand(kind: SourceKind, label: &str, hash: &str) -> SourceCandidate {
        SourceCandidate {
            ext_id: "toko".into(),
            ext_name: "Toko".into(),
            kind,
            label: label.into(),
            quality: Default::default(),
            container: None,
            subs: Vec::new(),
            dubs: Vec::new(),
            reliability: INITIAL_RELIABILITY,
            note: None,
            web: None,
            torrent: (kind == SourceKind::Torrent).then(|| tk_core::sources::TorrentInfo {
                info_hash: hash.repeat(20),
                size_bytes: None,
                seeders: None,
                leechers: 0,
                magnet: None,
            }),
            tags: Default::default(),
        }
    }

    #[test]
    fn torrent_candidates_outlive_their_list() {
        // BUG-134: a torrent picked in Details is found by its key without asking the extensions again, even after
        // a later (partial) listing that missed it; web rows are not kept (their URLs expire).
        let mut memo = TorrentMemo::default();
        let toons = cand(SourceKind::Torrent, "ToonsHub", "2b");
        let web = cand(SourceKind::Web, "Alpha", "");
        memo.remember(("anilist:1".into(), 3), &[toons.clone(), web.clone()]);
        memo.remember(("anilist:1".into(), 3), &[cand(SourceKind::Torrent, "Erai", "cd")]);
        assert_eq!(memo.find("anilist:1", Some(3), &source_key(&toons)), Some(toons.clone()));
        assert!(memo.find("anilist:1", Some(3), "toko|torrent|Erai").is_some());
        assert_eq!(memo.find("anilist:1", Some(3), &source_key(&web)), None);
        assert_eq!(memo.find("anilist:1", Some(4), &source_key(&toons)), None);
        assert_eq!(memo.find("anilist:2", Some(3), &source_key(&toons)), None);
        // Whole-torrent downloads (no episode) look in any episode of the title.
        assert_eq!(memo.find("anilist:1", None, &source_key(&toons)), Some(toons.clone()));
        // A newer listing replaces the row (same key, new hash).
        let renewed = cand(SourceKind::Torrent, "ToonsHub", "ef");
        memo.remember(("anilist:1".into(), 3), &[renewed.clone()]);
        assert_eq!(memo.find("anilist:1", Some(3), &source_key(&toons)), Some(renewed));
        // Bounded: the oldest list goes first.
        for ep in 100..100 + TORRENT_MEMO_KEYS as u32 {
            memo.remember(("anilist:9".into(), ep), &[toons.clone()]);
        }
        assert_eq!(memo.find("anilist:1", Some(3), &source_key(&toons)), None);
        assert!(memo.find("anilist:9", Some(100), &source_key(&toons)).is_some());
        memo.clear();
        assert_eq!(memo.find("anilist:9", None, &source_key(&toons)), None);
    }

    #[test]
    fn the_last_ranked_list_outlives_its_ttl() {
        // BUG-134: the player starts a pick from the last list `sources()` answered, however old; bounded.
        let row = |label: &str| SourceVm {
            id: format!("toko|web|{label}"),
            ext_id: "toko".into(),
            ext_name: "Toko".into(),
            kind: tk_ui::backend::SourceKindVm::Web,
            label: label.into(),
            quality: "1080p".into(),
            container: None,
            subs: Vec::new(),
            dubs: Vec::new(),
            reliability: INITIAL_RELIABILITY,
            note: None,
            latency_ms: None,
            size_bytes: None,
            seeders: None,
            leechers: None,
            info_hash: None,
            ext_enabled: true,
            score: 0.0,
            recommended: false,
            picked: false,
            tags: Default::default(),
            debrid: None,
            embed: false,
        };
        let mut last = LastSources::default();
        last.remember(("anilist:1".into(), 3), &[row("Alpha"), row("Beta")]);
        last.remember(("anilist:1".into(), 3), &[]);
        assert_eq!(last.get("anilist:1", 3).map(|l| l.len()), Some(2));
        last.remember(("anilist:1".into(), 3), &[row("Gamma")]);
        assert_eq!(last.get("anilist:1", 3).unwrap()[0].label, "Gamma");
        assert!(last.get("anilist:1", 4).is_none());
        for ep in 100..100 + TORRENT_MEMO_KEYS as u32 {
            last.remember(("anilist:9".into(), ep), &[row("Alpha")]);
        }
        assert!(last.get("anilist:1", 3).is_none());
        last.clear();
        assert!(last.get("anilist:9", 100).is_none());
    }

    #[test]
    fn an_expired_list_serves_a_picked_web_source_only_for_a_while() {
        // BUG-134: a web row picked from a list that just expired plays without a new listing; much later, it does not.
        let until = Instant::now();
        assert!(within_stale_grace(until, until));
        assert!(within_stale_grace(until, until + STALE_WEB_GRACE - Duration::from_secs(1)));
        assert!(!within_stale_grace(until, until + STALE_WEB_GRACE));
        assert!(SOURCES_TTL + STALE_WEB_GRACE <= Duration::from_secs(30 * 60));
    }

    #[test]
    fn empty_lists_are_not_kept_and_partial_ones_only_briefly() {
        // BUG-102: an empty list is asked again; one some provider failed on comes back after a few seconds.
        assert_eq!(candidates_ttl(0, false), None);
        assert_eq!(candidates_ttl(0, true), None);
        assert_eq!(candidates_ttl(15, true), Some(PARTIAL_SOURCES_TTL));
        assert_eq!(candidates_ttl(15, false), Some(SOURCES_TTL));
        assert!(PARTIAL_SOURCES_TTL <= Duration::from_secs(30));
    }

    #[test]
    fn a_proxy_change_retries_the_next_empty_listing_once() {
        // BUG-102: clearing (or changing) the outbound proxy marks the route as changed; other edits do not.
        let mut old = Settings::default();
        old.server.outbound_proxy = Some("http://127.0.0.1:9".into());
        let mut new = old.clone();
        assert!(!route_changed(&old, &new));
        new.server.name = "PC".into();
        assert!(!route_changed(&old, &new));
        new.server.outbound_proxy = None;
        assert!(route_changed(&old, &new));
        let mut fs = old.clone();
        fs.server.flaresolverr_url = Some("http://127.0.0.1:8191".into());
        assert!(route_changed(&old, &fs));

        let now = Instant::now();
        assert!(!retry_after_route_change(None, now));
        assert!(retry_after_route_change(Some(now), now + Duration::from_secs(20)));
        assert!(!retry_after_route_change(Some(now), now + NET_CHANGE_WINDOW));
    }
}

#[cfg(test)]
mod fallback_tests {
    use super::*;
    use std::collections::BTreeMap;

    fn titles(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
    }

    /// Overgeared as Kitsu answers it while AniList is rate-limited (r12, BUG-143).
    fn kitsu_og() -> Media {
        Media {
            media_ref: MediaRef { anilist_id: None, mal_id: None, kitsu_id: Some(50743) },
            kind: MediaKind::Anime,
            title: "Temppal: Item no Chikara".into(),
            titles: titles(&[("romaji", "Temppal: Item no Chikara"), ("native", "テムパル～アイテムの力～")]),
            cover: Some("https://media.kitsu.app/anime/50743/poster.jpg".into()),
            banner: None,
            color: None,
            synopsis: None,
            genres: Vec::new(),
            episodes: Some(12),
            status: None,
            season: None,
            year: None,
            score: None,
            source_provider: "kitsu".into(),
        }
    }

    fn anilist_og() -> KnownMedia {
        KnownMedia {
            title: "Overgeared".into(),
            titles: titles(&[
                ("english", "Overgeared"),
                ("romaji", "Tempal: Item no Chikara"),
                ("native", "テムパル～アイテムの力～"),
            ]),
            cover: Some("https://s4.anilist.co/file/anilistcdn/media/anime/cover/large/bx212888.jpg".into()),
        }
    }

    /// The `titles` param the extension host sends (`tk_ext::host::stream_params`: title first, then the map).
    fn ext_titles(m: &Media) -> Vec<String> {
        let mut out = vec![m.title.clone()];
        for t in m.titles.values() {
            if !out.contains(t) {
                out.push(t.clone());
            }
        }
        out
    }

    #[test]
    fn a_known_anilist_entry_leads_the_titles_and_keeps_its_poster() {
        let mut m = kitsu_og();
        complete_fallback(&mut m, Some(&anilist_og()), None);
        assert_eq!(ext_titles(&m), ["Overgeared", "テムパル～アイテムの力～", "Tempal: Item no Chikara"]);
        assert_eq!(m.cover.as_deref(), anilist_og().cover.as_deref());
        // Still a fallback answer (not stored as known, short sources lifetime).
        assert_eq!(m.source_provider, "kitsu");
    }

    #[test]
    fn the_fallback_only_adds_new_keys() {
        let mut m = kitsu_og();
        m.titles.insert("alt_1".into(), "Overgeared".into());
        m.titles.insert("alt_2".into(), "Tempal".into());
        complete_fallback(&mut m, Some(&anilist_og()), None);
        assert_eq!(m.titles.get("romaji").map(String::as_str), Some("Tempal: Item no Chikara"));
        assert_eq!(m.titles.get("alt_2").map(String::as_str), Some("Tempal"));
        assert!(!m.titles.contains_key("alt_1"), "same title as the English one");
    }

    #[test]
    fn without_a_known_entry_the_history_title_and_poster_hold() {
        let mut m = kitsu_og();
        complete_fallback(&mut m, None, Some(("Overgeared".into(), Some("https://anilist/poster.jpg".into()))));
        assert_eq!(ext_titles(&m)[0], "Overgeared");
        assert!(ext_titles(&m).contains(&"Temppal: Item no Chikara".to_owned()));
        assert_eq!(m.cover.as_deref(), Some("https://anilist/poster.jpg"));
    }

    #[test]
    fn a_fallback_with_an_english_title_keeps_it() {
        let mut m = kitsu_og();
        m.titles.insert("english".into(), "Overgeared".into());
        m.title = "Overgeared".into();
        complete_fallback(&mut m, None, Some(("Otro".into(), None)));
        assert_eq!(m.title, "Overgeared");
        assert_eq!(m.cover.as_deref(), kitsu_og().cover.as_deref(), "no history poster: Kitsu's stays");
    }

    #[test]
    fn nothing_known_leaves_the_fallback_as_is() {
        let mut m = kitsu_og();
        complete_fallback(&mut m, None, None);
        assert_eq!(m, kitsu_og());
    }

    /// Overgeared as `map_kitsu_media` maps it (the `Media` of `tk-core` `mapping.rs`'s BUG-083 test): Kitsu's only
    /// English spelling is `alt_1 = "OVERGEARED"`.
    fn kitsu_og_mapped() -> Media {
        let res = serde_json::json!({
            "id": "50743",
            "attributes": {
                "canonicalTitle": "Temppal: Item no Chikara",
                "titles": { "en_jp": "Temppal: Item no Chikara", "ja_jp": "テムパル～アイテムの力～" },
                "abbreviatedTitles": ["OVERGEARED", "템빨", "TEMPPAL: ITEM NO CHIKARA", "OG", "AoT", "TEMPAL"]
            }
        });
        let m = tk_core::metadata::mapping::map_kitsu_media(&serde_json::json!({}), &res);
        assert_eq!(m.titles.get("alt_1").map(String::as_str), Some("OVERGEARED"));
        m
    }

    /// The raw `anilist.chain.a212888` card Details caches (`CHAIN_QUERY`, `...mediaCard`).
    fn anilist_og_chain() -> serde_json::Value {
        serde_json::json!({
            "id": 212888, "idMal": 59145, "type": "ANIME", "format": "ONA",
            "title": { "romaji": "Tempal: Item no Chikara", "english": "Overgeared", "native": "テムパル～アイテムの力～" },
            "coverImage": { "extraLarge": "https://s4.anilist.co/file/anilistcdn/media/anime/cover/large/bx212888.jpg" },
            "relations": { "edges": [] }
        })
    }

    fn history_og() -> Option<(String, Option<String>)> {
        Some(("Overgeared".into(), Some("https://anilist/poster.jpg".into())))
    }

    #[test]
    fn bug144_the_history_title_is_english_despite_a_kitsu_alt_spelling() {
        let mut m = kitsu_og_mapped();
        complete_fallback(&mut m, None, history_og());
        assert_eq!(super::details::display_title(&m, "english"), "Overgeared");
        assert_eq!(m.titles.get("english").map(String::as_str), Some("Overgeared"));
        assert_eq!(ext_titles(&m)[0], "Overgeared");
        // the romaji preference still shows the romaji
        assert_eq!(super::details::display_title(&m, "romaji"), "Temppal: Item no Chikara");
    }

    #[test]
    fn bug144_a_history_title_equal_to_the_romaji_is_not_called_english() {
        let mut m = kitsu_og_mapped();
        complete_fallback(&mut m, None, Some(("temppal: item no chikara".into(), None)));
        assert!(!m.titles.contains_key("english"));
    }

    #[test]
    fn bug144_cached_anilist_titles_reach_the_extensions() {
        let known = known_from_anilist_raw(Some(&anilist_og_chain()), None, history_og()).expect("AniList titles");
        assert_eq!(known.cover.as_deref(), anilist_og().cover.as_deref(), "the AniList card's poster");
        let mut m = kitsu_og_mapped();
        complete_fallback(&mut m, Some(&known), None);
        let t = ext_titles(&m);
        assert_eq!(t[0], "Overgeared");
        assert!(t.contains(&"Tempal: Item no Chikara".to_owned()), "{t:?}");
        assert!(!t.contains(&"Temppal: Item no Chikara".to_owned()), "AniList's romaji wins on the same key: {t:?}");
        assert_eq!(super::details::display_title(&m, "english"), "Overgeared");
        assert_eq!(m.cover.as_deref(), anilist_og().cover.as_deref());
    }

    #[test]
    fn bug144_details_extra_titles_complete_with_the_history() {
        // `anilist.extra` has titles but no poster; an entry without English takes the history's.
        let extra = serde_json::json!({ "id": 212888, "title": { "romaji": "Tempal: Item no Chikara", "native": "テムパル～アイテムの力～" } });
        let known = known_from_anilist_raw(None, Some(&extra), history_og()).expect("AniList titles");
        assert_eq!(known.title, "Overgeared");
        assert_eq!(known.titles.get("romaji").map(String::as_str), Some("Tempal: Item no Chikara"));
        assert_eq!(known.cover.as_deref(), Some("https://anilist/poster.jpg"));
        assert_eq!(known_from_anilist_raw(None, None, history_og()), None);
        assert_eq!(known_from_anilist_raw(None, Some(&serde_json::json!({})), history_og()), None);
    }

    #[test]
    fn known_keys_follow_the_ref() {
        assert_eq!(known_key(&MediaRef::anilist(212888)).as_deref(), Some("media.known.a212888"));
        assert_eq!(known_key(&MediaRef { mal_id: Some(7), ..MediaRef::default() }).as_deref(), Some("media.known.m7"));
        assert_eq!(known_key(&MediaRef { kitsu_id: Some(4), ..MediaRef::default() }), None);
    }

    #[test]
    fn a_fallback_listing_is_kept_briefly() {
        assert_eq!(candidates_ttl(30, true), Some(PARTIAL_SOURCES_TTL));
    }
}
