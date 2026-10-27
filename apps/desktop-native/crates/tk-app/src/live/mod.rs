//! The shipping backend: `tk_ui::Backend` over tk-core (metadata router, Supabase, local store),
//! tk-ext (extension host) and tk-link (Link server).
//!
//! * `mod.rs` is written against tk-core as committed by B5 (`m3e/desktop-core` cc9228e).
//! * `ext.rs` / `link.rs` are written against the 04 section 3.3 contract only (tk-ext and tk-link were
//!   placeholders when this was written); every guessed signature is marked `CONTRACT-GUESS`.

mod ext;
mod link;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use time::OffsetDateTime;
use tk_core::{
    FeedKind, HistoryEntry, LocalStore, Media, MediaKind, MediaRef, MetadataRouter, ProviderError, Supabase,
    SupabaseError,
};
use tk_ui::backend::{
    AccountVm, Backend, ContinueVm, DetailsVm, EpisodeVm, ExtensionVm, HomeVm, LinkStatusVm, MediaVm, PlaybackVm,
    SectionVm, StreamVm, SubtitleVm, UiResult,
};
use tk_ui::model::{ErrorKind, Pairing, Settings, UiError};

use crate::backend_common::{IdKind, parse_media_id};
use crate::config::AppConfig;
use crate::paths::AppPaths;
use crate::settings_store;

pub struct LiveBackend {
    settings_file: std::path::PathBuf,
    store: Arc<LocalStore>,
    router: Arc<MetadataRouter>,
    supabase: Option<Arc<Supabase>>,
    email: Mutex<Option<String>>,
    /// `id -> (title, poster)` of titles opened this session, for history rows.
    titles: Mutex<HashMap<String, (String, Option<String>)>>,
    ext: ext::Extensions,
    link: link::LinkService,
}

impl LiveBackend {
    pub fn new(paths: &AppPaths, cfg: &AppConfig) -> anyhow::Result<Self> {
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
        Ok(Self {
            settings_file,
            store,
            router,
            supabase,
            email: Mutex::new(None),
            titles: Mutex::new(HashMap::new()),
            ext: ext::Extensions::new(paths.extensions_dir())?,
            link: link::LinkService::new(paths.link_dir()),
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
        let continue_watching = self
            .store
            .continue_watching(20)
            .unwrap_or_default()
            .into_iter()
            .map(|e| {
                let media = MediaVm {
                    id: e.anime_id.clone(),
                    title: e.title.clone(),
                    cover: e.poster.clone(),
                    color: None,
                    subtitle: String::new(),
                    score: None,
                };
                self.remember_title(&e.anime_id, &e.title, e.poster.as_deref());
                ContinueVm { media, episode: e.episode, progress_secs: e.progress_sec, duration_secs: e.duration_sec }
            })
            .collect();
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
        let start_secs = self
            .progress(&r)
            .iter()
            .find(|e| e.episode == episode && !e.completed)
            .map_or(0, |e| e.progress_sec);
        Ok(PlaybackVm {
            title: media.title.clone(),
            poster: media.cover.clone(),
            episodes_total: media.episodes,
            start_secs,
            streams: streams
                .into_iter()
                .map(|s| StreamVm {
                    url: s.url,
                    headers: s.headers.into_iter().collect(),
                    provider: s.provider,
                    quality: s.quality,
                    subtitles: s
                        .subtitles
                        .into_iter()
                        .map(|t| SubtitleVm { url: t.url, lang: t.lang, label: t.label })
                        .collect(),
                })
                .collect(),
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
        self.sync_history().await;
        Ok(())
    }

    async fn extensions(&self) -> UiResult<Vec<ExtensionVm>> {
        self.ext.list().await
    }
    async fn install_extension(&self, path: &str) -> UiResult<ExtensionVm> {
        self.ext.install(path).await
    }
    async fn set_extension_enabled(&self, id: &str, enabled: bool) -> UiResult<()> {
        self.ext.set_enabled(id, enabled).await
    }
    async fn remove_extension(&self, id: &str) -> UiResult<()> {
        self.ext.remove(id).await
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
        crate::backend_common::allow_private_network().await
    }
}
