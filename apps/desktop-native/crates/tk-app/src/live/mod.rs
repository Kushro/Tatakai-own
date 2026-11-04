//! The shipping backend: `tk_ui::Backend` over tk-core (metadata router, Supabase, local store),
//! tk-ext (extension host) and tk-link (Link server).
//!

mod browser;
mod ext;
mod link;
mod sources;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use time::OffsetDateTime;
use tk_core::sources::{INITIAL_RELIABILITY, SourceCandidate, SourceKind, parse_source_key, source_key};
use tk_core::{
    Compat, FeedKind, HistoryEntry, LocalStore, Media, MediaKind, MediaRef, MetadataRouter, ProviderError, Supabase,
    SupabaseError,
};
use tk_ui::backend::{
    AccountVm, AnimeBuildVm, Backend, ContinueVm, DetailsVm, EpisodeVm, ExtLogVm, ExtensionDetailVm, ExtensionVm, HistoryItemVm,
    HomeVm, LibraryVm, LinkCapsVm, LinkStatusVm, MediaVm, PlaybackVm, SectionVm, SourceVm, UiResult,
};
use tk_ui::model::{ErrorKind, Pairing, Settings, UiError};
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
    supabase: Option<Arc<Supabase>>,
    email: Mutex<Option<String>>,
    /// `id -> (title, poster)` of titles opened this session, for history rows.
    titles: Mutex<HashMap<String, (String, Option<String>)>>,
    ext: ext::Extensions,
    link: link::LinkService,
    /// `(media id, episode) -> (resolved at, candidates)`: `sources()` and `playback_source()` share one resolution
    /// (stream URLs expire, hence the TTL). Cleared when the set of enabled extensions changes.
    candidates: Mutex<HashMap<(String, u32), (Instant, Vec<SourceCandidate>)>>,
}

/// How long a resolved source list is reused before the extensions are asked again.
const SOURCES_TTL: Duration = Duration::from_secs(5 * 60);
/// History rows shown in the Library.
const LIBRARY_HISTORY: u32 = 200;

impl LiveBackend {
    /// `ui`: jobs for the UI thread (hidden web views of the extension browser runtime).
    pub fn new(paths: &AppPaths, cfg: &AppConfig, ui: UiThread) -> anyhow::Result<Self> {
        let settings_file = paths.ui_settings_file();
        let settings = settings_store::load(&settings_file);
        let store = Arc::new(LocalStore::open(&paths.db_file())?);
        let client = reqwest::Client::builder().user_agent("TatakaiNext/0.1").timeout(Duration::from_secs(20)).build()?;
        let router = Arc::new(MetadataRouter::with_defaults(client, settings.provider_priority.clone()));
        let supabase = cfg.supabase().and_then(|(url, key)| match Supabase::new(url, key) {
            Ok(s) => Some(Arc::new(s)),
            Err(e) => {
                tracing::warn!(error = %e, "Supabase config rejected; running without an account service");
                None
            }
        });
        let ext = ext::Extensions::new(paths.extensions_dir(), Arc::new(browser::WebviewBrowsers::new(ui)))?;
        let link = link::LinkService::new(
            paths.link_dir(),
            ext.host(),
            crate::paths::shaders_dir(paths, None),
            crate::paths::anime4k_lock(None),
        );
        Ok(Self {
            data_root: paths.root.clone(),
            settings_file,
            store,
            router,
            supabase,
            email: Mutex::new(None),
            titles: Mutex::new(HashMap::new()),
            ext,
            link,
            candidates: Mutex::new(HashMap::new()),
        })
    }

    fn remember_title(&self, id: &str, title: &str, poster: Option<&str>) {
        if let Ok(mut t) = self.titles.lock() {
            t.insert(id.to_owned(), (title.to_owned(), poster.map(str::to_owned)));
        }
    }

    async fn media(&self, id: &str) -> UiResult<(MediaRef, Media)> {
        let r = media_ref(id).ok_or_else(|| UiError::new(ErrorKind::Other, format!("invalid id {id}")))?;
        let media = self
            .router
            .media(MediaKind::Anime, &r)
            .await
            .map_err(provider_err)?
            .ok_or_else(|| UiError::new(ErrorKind::Other, format!("{id} was not found")))?;
        self.remember_title(id, &media.title, media.cover.as_deref());
        Ok((r, media))
    }

    fn progress(&self, r: &MediaRef) -> Vec<HistoryEntry> {
        HistoryEntry::anime_key(r).and_then(|k| self.store.progress_for(&k).ok()).unwrap_or_default()
    }

    fn store_err(e: impl std::fmt::Display) -> UiError {
        UiError::new(ErrorKind::Other, e.to_string())
    }

    fn continue_vms(&self, limit: usize) -> Vec<ContinueVm> {
        self.store
            .continue_watching(limit)
            .unwrap_or_default()
            .into_iter()
            .map(|e| {
                self.remember_title(&e.anime_id, &e.title, e.poster.as_deref());
                ContinueVm { media: history_media(&e), episode: e.episode, progress_secs: e.progress_sec, duration_secs: e.duration_sec }
            })
            .collect()
    }

    fn start_secs(&self, r: &MediaRef, episode: u32) -> u32 {
        self.progress(r).iter().find(|e| e.episode == episode && !e.completed).map_or(0, |e| e.progress_sec)
    }

    fn invalidate_sources(&self) {
        if let Ok(mut c) = self.candidates.lock() {
            c.clear();
        }
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
                .and_then(|c| c.get(&key).filter(|(at, _)| at.elapsed() < SOURCES_TTL).map(|(_, v)| v.clone()))
        };
        let mut list = match cached {
            Some(v) => v,
            None => {
                // `resolve_candidates` keeps server/subs/dubs/note and lists torrents (resolve_streams does not).
                let v = sources::dedupe_by_key(self.ext.resolve_candidates(media, episode).await);
                if let Ok(mut c) = self.candidates.lock() {
                    c.retain(|_, (at, _)| at.elapsed() < SOURCES_TTL);
                    c.insert(key, (Instant::now(), v.clone()));
                }
                v
            }
        };
        for c in &mut list {
            c.reliability = self.store.reliability(&c.ext_id, &c.label).unwrap_or(INITIAL_RELIABILITY);
        }
        list
    }

    /// Push dirty rows to Supabase (best effort, only when signed in).
    async fn sync_history(&self) {
        let Some(sb) = self.supabase.as_ref().filter(|s| s.session().is_some()) else { return };
        let Ok(dirty) = self.store.dirty_history() else { return };
        if dirty.is_empty() {
            return;
        }
        match sb.upsert_history(&dirty).await {
            Ok(()) => {
                let _ = self.store.mark_synced(&dirty);
            }
            Err(e) => tracing::warn!(error = %e, "history sync failed; rows stay local"),
        }
    }
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

fn to_vm(m: &Media) -> MediaVm {
    let mut parts = Vec::new();
    if let Some(y) = m.year {
        parts.push(y.to_string());
    }
    if let Some(e) = m.episodes {
        parts.push(format!("{e} ep"));
    }
    MediaVm {
        id: media_id(&m.media_ref),
        title: m.title.clone(),
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

fn supabase_err(e: SupabaseError) -> UiError {
    let kind = match &e {
        SupabaseError::InvalidCredentials | SupabaseError::NotSignedIn | SupabaseError::SessionExpired => ErrorKind::Auth,
        SupabaseError::Network(_) => ErrorKind::Offline,
        _ => ErrorKind::Other,
    };
    UiError::new(kind, e.to_string())
}

#[async_trait]
impl Backend for LiveBackend {
    async fn home(&self) -> UiResult<HomeVm> {
        let feed = |f| self.router.feed(MediaKind::Anime, f, 1);
        let (trending, airing, seasonal, popular, top) = tokio::join!(
            feed(FeedKind::Trending),
            feed(FeedKind::Airing),
            feed(FeedKind::Seasonal),
            feed(FeedKind::Popular),
            feed(FeedKind::TopRated)
        );
        let section = |key: &str, r: Result<tk_core::FeedPage, ProviderError>| SectionVm {
            title_key: key.to_owned(),
            items: r.map(|p| p.items.iter().map(to_vm).collect()).map_err(provider_err),
        };
        let continue_watching = self.continue_vms(20);
        Ok(HomeVm {
            continue_watching,
            sections: vec![
                section("feed.trending", trending),
                section("feed.airing", airing),
                section("feed.seasonal", seasonal),
                section("feed.popular", popular),
                section("feed.top_rated", top),
            ],
        })
    }

    async fn search(&self, query: &str, page: u32) -> UiResult<Vec<MediaVm>> {
        let p = self.router.search(MediaKind::Anime, query, page).await.map_err(provider_err)?;
        Ok(p.items.iter().map(to_vm).collect())
    }

    async fn details(&self, id: &str) -> UiResult<DetailsVm> {
        let (r, media) = self.media(id).await?;
        let mut rows = self.progress(&r);
        rows.sort_by(|a, b| b.watched_at.cmp(&a.watched_at));
        let (resume_episode, resume_secs) = match rows.first() {
            Some(e) if e.completed => (e.episode + 1, 0),
            Some(e) => (e.episode, e.progress_sec),
            None => (0, 0),
        };
        Ok(DetailsVm {
            media: to_vm(&media),
            banner: media.banner.clone(),
            synopsis: media.synopsis.clone(),
            genres: media.genres.clone(),
            status: media.status.clone(),
            episodes_total: media.episodes,
            resume_episode,
            resume_secs,
        })
    }

    async fn episodes(&self, id: &str) -> UiResult<Vec<EpisodeVm>> {
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
        let eps = self.router.episodes(&r).await.map_err(provider_err)?;
        Ok(eps
            .into_iter()
            .map(|e| EpisodeVm {
                number: e.number,
                title: e.title,
                image: e.image,
                air_date: e.air_date,
                is_filler: e.is_filler,
                watched: watched.get(&e.number).copied().unwrap_or(0.0),
            })
            .collect())
    }

    async fn playback(&self, id: &str, episode: u32) -> UiResult<PlaybackVm> {
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
            streams: streams.into_iter().map(sources::stream_vm).collect(),
        })
    }

    async fn record_progress(&self, id: &str, episode: u32, progress_secs: u32, duration_secs: Option<u32>) -> UiResult<()> {
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
        self.sync_history().await;
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
        self.router.set_priority(settings.provider_priority.clone());
        settings_store::save(&self.settings_file, settings).map_err(|e| UiError::new(ErrorKind::Other, e.to_string()))
    }

    async fn account(&self) -> UiResult<AccountVm> {
        let signed_in = self.supabase.as_ref().is_some_and(|s| s.session().is_some());
        Ok(AccountVm {
            // The Supabase session carries no email; keep what the user typed for display.
            email: if signed_in { self.email.lock().ok().and_then(|e| e.clone()) } else { None },
            configured: self.supabase.is_some(),
        })
    }
    async fn sign_in(&self, email: &str, password: &str) -> UiResult<AccountVm> {
        let sb = self.supabase.as_ref().ok_or_else(|| UiError::new(ErrorKind::Auth, "no account service configured"))?;
        sb.sign_in_password(email, password).await.map_err(supabase_err)?;
        if let Ok(mut e) = self.email.lock() {
            *e = Some(email.to_owned());
        }
        // Merge what the account already has, then push local rows.
        match sb.select_history(200).await {
            Ok(rows) => {
                let _ = self.store.merge_remote(&rows);
            }
            Err(e) => tracing::warn!(error = %e, "pulling remote history failed"),
        }
        self.sync_history().await;
        self.account().await
    }
    async fn sign_out(&self) -> UiResult<()> {
        if let Some(sb) = &self.supabase {
            sb.sign_out();
        }
        if let Ok(mut e) = self.email.lock() {
            *e = None;
        }
        Ok(())
    }

    async fn link_status(&self) -> UiResult<LinkStatusVm> {
        self.link.status().await
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
        let (_, media) = self.media(id).await?;
        let candidates = self.episode_candidates(id, episode, &media, false).await;
        let prefs = sources::core_prefs(&settings_store::load(&self.settings_file).sources);
        let enabled: HashMap<String, bool> =
            self.ext.installed().into_iter().map(|e| (e.id, e.enabled && e.compat != Compat::Unsupported)).collect();
        let picked = self.store.picked_source(id, episode).ok().flatten();
        Ok(sources::ranked_vms(candidates, &prefs, |ext_id| enabled.get(ext_id).copied().unwrap_or(false), picked.as_deref()))
    }

    async fn playback_source(&self, id: &str, episode: u32, source_id: &str) -> UiResult<PlaybackVm> {
        let (_, kind, _) =
            parse_source_key(source_id).ok_or_else(|| UiError::new(ErrorKind::Other, format!("invalid source id {source_id}")))?;
        if kind == SourceKind::Torrent {
            return Err(UiError::new(ErrorKind::TorrentM2, source_id));
        }
        let (r, media) = self.media(id).await?;
        let find = |list: Vec<SourceCandidate>| list.into_iter().find(|c| source_key(c) == source_id);
        // A cached list may be stale (URL expired, extension re-enabled): resolve again once before giving up.
        let candidate = match find(self.episode_candidates(id, episode, &media, false).await) {
            Some(c) => Some(c),
            None => find(self.episode_candidates(id, episode, &media, true).await),
        };
        let candidate = candidate.ok_or_else(|| UiError::new(ErrorKind::NoSource, source_id))?;
        let stream = match candidate.kind {
            SourceKind::Torrent => return Err(UiError::new(ErrorKind::TorrentM2, source_id)),
            SourceKind::Web => candidate.to_stream().ok_or_else(|| UiError::new(ErrorKind::NoSource, source_id))?,
        };
        Ok(PlaybackVm {
            title: media.title.clone(),
            poster: media.cover.clone(),
            episodes_total: media.episodes,
            start_secs: self.start_secs(&r, episode),
            streams: vec![sources::stream_vm(stream)],
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

    async fn sync_now(&self) -> UiResult<()> {
        let sb = self
            .supabase
            .as_ref()
            .filter(|s| s.session().is_some())
            .ok_or_else(|| UiError::new(ErrorKind::Auth, "not signed in"))?;
        let dirty = self.store.dirty_history().map_err(Self::store_err)?;
        if !dirty.is_empty() {
            sb.upsert_history(&dirty).await.map_err(supabase_err)?;
            self.store.mark_synced(&dirty).map_err(Self::store_err)?;
        }
        let rows = sb.select_history(LIBRARY_HISTORY).await.map_err(supabase_err)?;
        self.store.merge_remote(&rows).map_err(Self::store_err)
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
    async fn prepare_anime_build(&self) -> UiResult<std::path::PathBuf> {
        let paths = AppPaths::from_root(self.data_root.clone());
        tokio::task::spawn_blocking(move || crate::anime_build::prepare(&paths))
            .await
            .map_err(|e| UiError::new(ErrorKind::Other, e.to_string()))?
    }
    fn data_dir_display(&self) -> String {
        self.data_root.display().to_string()
    }
}
