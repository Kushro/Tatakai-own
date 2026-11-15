//! Link server glue over tk-link (`start(LinkConfig, Arc<dyn LinkHost>)` -> `LinkHandle`).
//!
//! The server starts the first time the Link screen asks for status (so the OS firewall prompt never
//! appears at first launch - review E6) and runs until the process exits or the user restarts it on
//! another port (`restart`). A failed start (port taken: os error 10048 / 98) is not an `Err` of
//! `status` / `restart`: it is reported in `LinkStatusVm::error` so the screen can offer the fixes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tk_core::LocalStore;
use tk_ext::ExtensionHost;
use tk_link::control::{PhoneEvent, PlayerState};
use tk_link::discovery::Announcer;
use tk_link::media::capabilities::Sustain;
use tk_link::sessions::{CreateRequest, SourceRequest};
use tk_link::devices::HistoryMode;
use tk_link::{Device, LinkConfig, LinkError, LinkHandle, LinkHost, Vendor};
use tk_ui::backend::{Anime4kNoteVm, Backend, DeviceVm, HistoryModeVm, LinkCapsVm, LinkStatusVm, PresenceVm, RemoteCommandVm, SendOutcomeVm, SendProgress, SendToDeviceVm, UiResult};
use tk_ui::model::{ErrorKind, Pairing, UiError};

use super::LiveBackend;

/// A player state older than this is treated as "no player" (the player reports every 15 s and on every pause
/// change, `screens/player/presence.rs`).
const PLAYER_STALE: Duration = Duration::from_secs(45);

/// What the PC's player shows, as last reported (`note_presence` / `note_progress`), for `GET /v1/player`.
#[derive(Default)]
struct PlayerTrack {
    state: Option<(Instant, PlayerState)>,
}

type SharedPlayer = Arc<Mutex<PlayerTrack>>;

/// `GET /v1/player` answer from the tracked state (`None` when stale or closed). Pure apart from the clock.
fn current_player(track: &SharedPlayer) -> Option<PlayerState> {
    let g = track.lock().ok()?;
    g.state.as_ref().filter(|(at, _)| at.elapsed() < PLAYER_STALE).map(|(_, s)| s.clone())
}

/// Merge a presence report into the tracked state (`Watching` keeps the media id of the last progress row of
/// the same title / episode; anything else closes the player). Pure.
fn merge_presence(prev: Option<&PlayerState>, state: &PresenceVm) -> Option<PlayerState> {
    match state {
        PresenceVm::Watching { title, episode, position_secs, duration_secs, paused, .. } => {
            let media_id = prev.filter(|p| p.episode == Some(*episode)).and_then(|p| p.media_id.clone());
            Some(PlayerState {
                playing: !*paused,
                title: Some(title.clone()).filter(|t| !t.is_empty()),
                episode: Some(*episode),
                position_secs: *position_secs,
                duration_secs: *duration_secs,
                media_id,
            })
        }
        _ => None,
    }
}

/// Whether a transport command changes nothing given the player state (`play` while playing, `pause` while
/// paused): it is accepted without reaching the player, whose only primitive is "toggle". Pure.
fn redundant(cmd: &tk_link::control::ControlCommand, st: Option<&PlayerState>) -> bool {
    use tk_link::control::ControlCommand as C;
    match (cmd, st) {
        (C::Play, Some(s)) => s.playing,
        (C::Pause, Some(s)) => !s.playing,
        _ => false,
    }
}

/// What the Link server may ask of the app. M0: no library folder; FFmpeg only if `fetch-deps.ps1` ran.
/// Extension permissions come from the installed extension, so `url` sources can only reach its declared domains.
/// Wave A (worker f): the watch history for `GET` / `POST /v1/history` (`super::history_sync`).
struct AppLinkHost {
    ext: Arc<ExtensionHost>,
    store: Arc<LocalStore>,
    /// Wave B (worker k): commands from `POST /v1/control` for `LiveBackend::wait_remote_command`.
    remote: tokio::sync::mpsc::UnboundedSender<RemoteCommandVm>,
    /// Wave B (worker k): the PC player as last reported, for `GET /v1/player`.
    player: SharedPlayer,
    /// `ui-settings.json`, read for the extension server's port and switch (BUG-023 trusted origins).
    settings_file: PathBuf,
    /// BUG-009: the backend, for the torrent engine (`torrent_play`); bound once the backend is in its `Arc`.
    backend: BackendSlot,
}

/// BUG-131: one Link reader of a torrent, given back on drop unless [`ReaderHold::keep`] hands it to the session.
struct ReaderHold(Option<String>);

impl ReaderHold {
    fn new(id: &str) -> Self {
        super::torrent_settings::link_reader_added(id);
        Self(Some(id.to_owned()))
    }
    fn keep(mut self) {
        self.0 = None;
    }
}

impl Drop for ReaderHold {
    fn drop(&mut self) {
        if let Some(id) = self.0.take() {
            super::torrent_settings::link_reader_ended(&id);
        }
    }
}

/// Late-bound backend of the Link host ([`bind_backend`]).
type BackendSlot = Arc<std::sync::OnceLock<std::sync::Weak<LiveBackend>>>;

/// BUG-009: let the Link host reach the backend (torrent engine). Called once by `main.rs` after `Arc::new`.
pub fn bind_backend(b: &Arc<LiveBackend>) {
    let _ = b.link.backend.set(Arc::downgrade(b));
}

/// Path prefix of the extension server's media proxy (`tk_server::API_PREFIX` + `/media/`).
const MEDIA_PROXY_PREFIX: &str = "/api/server/v1/media/";

/// BUG-023: the media proxy of this PC's own extension server on each of `ips` (+ loopback) and `port`. Pure.
fn own_media_origins(ips: &[std::net::Ipv4Addr], port: u16) -> Vec<tk_core::net::TrustedOrigin> {
    let mut all: Vec<std::net::Ipv4Addr> = ips.iter().copied().filter(|ip| !ip.is_unspecified()).collect();
    all.push(std::net::Ipv4Addr::LOCALHOST);
    all.sort_unstable();
    all.dedup();
    all.into_iter()
        .map(|ip| tk_core::net::TrustedOrigin::new(std::net::SocketAddr::new(ip.into(), port), MEDIA_PROXY_PREFIX))
        .collect()
}

impl LinkHost for AppLinkHost {
    fn extension_permissions(&self, id: &str) -> Option<Vec<String>> {
        self.ext.get(id).map(|e| e.permissions)
    }
    fn history_since(&self, since_unix: i64, limit: usize) -> Vec<tk_core::HistoryEntry> {
        super::history_sync::since(&self.store, since_unix, limit)
    }
    fn history_merge(&self, rows: Vec<tk_core::HistoryEntry>) -> usize {
        super::history_sync::merge(&self.store, rows)
    }
    fn history_merge_from(&self, device_id: &str, rows: Vec<tk_core::HistoryEntry>, authoritative: bool) -> usize {
        super::history_sync::merge_from(&self.store, device_id, rows, authoritative)
    }
    fn history_clear(&self) -> bool {
        super::history_sync::clear(&self.store)
    }
    fn library(&self) -> Vec<tk_link::LibraryItem> {
        Vec::new()
    }
    fn library_path(&self, _id: &str) -> Option<PathBuf> {
        None
    }
    fn ffmpeg_path(&self) -> Option<PathBuf> {
        // the bundled build first (`<exe>/ffmpeg/bin`); on Linux the distribution's FFmpeg (VAAPI, libplacebo)
        crate::paths::ffmpeg_path(None).or_else(|| if cfg!(windows) { None } else { find_in_path("ffmpeg") })
    }
    fn control(&self, cmd: tk_link::control::ControlCommand) -> bool {
        let st = current_player(&self.player);
        if redundant(&cmd, st.as_ref()) {
            return true;
        }
        self.remote.send(remote_vm(cmd)).is_ok()
    }
    fn player_state(&self) -> Option<PlayerState> {
        current_player(&self.player)
    }
    fn open_torrent(&self, req: tk_link::TorrentRequest) -> tk_link::HostFuture<'_, Result<tk_link::OpenedTorrent, tk_link::api::ErrorCode>> {
        let backend = self.backend.get().and_then(std::sync::Weak::upgrade);
        let forced = torrent_failure_forced(std::env::var(FAIL_TORRENT_ENV).ok().as_deref());
        Box::pin(async move {
            if forced {
                // QA (r3 BLOCKED case): exercise the "sent another source instead" notice without a torrent without peers
                tracing::warn!(media = %req.media_id, episode = req.episode, "Link torrent source refused on purpose ({FAIL_TORRENT_ENV})");
                return Err(tk_link::api::ErrorCode::SourceUnavailable);
            }
            let b = backend.ok_or(tk_link::api::ErrorCode::SourceUnavailable)?;
            let played = super::torrent_play::play(&b, &req.uri, &req.media_id, req.episode).await.map_err(|e| {
                tracing::info!(kind = ?e.kind, detail = %e.detail, "Link torrent source could not be opened");
                match e.kind {
                    ErrorKind::NoSource => tk_link::api::ErrorCode::LibraryItemNotFound,
                    _ => tk_link::api::ErrorCode::SourceUnavailable,
                }
            })?;
            let url = played.playback.streams.first().map(|s| s.url.clone()).ok_or(tk_link::api::ErrorCode::SourceUnavailable)?;
            tracing::info!(torrent = %played.torrent_id, file = played.file_index, "Link session reads a torrent");
            super::torrent_settings::link_reader_added(&played.torrent_id);
            Ok(tk_link::OpenedTorrent { url, torrent_id: played.torrent_id })
        })
    }
    fn reuse_torrent(&self, torrent_id: &str) -> tk_link::HostFuture<'_, bool> {
        let backend = self.backend.get().and_then(std::sync::Weak::upgrade);
        let id = torrent_id.to_owned();
        Box::pin(async move {
            let Some(engine) = backend.and_then(|b| b.torrent.running()) else { return false };
            let Some(entry) = engine.list().into_iter().find(|e| e.id.eq_ignore_ascii_case(&id)) else { return false };
            // counted before the resume, so an after-watch policy that is waiting out its grace period skips the pause;
            // given back if the resume fails or the renew gives up meanwhile (the future is dropped). The resume is a
            // no-op for a torrent that transfers (the usual case: the replaced session still reads it).
            let reader = ReaderHold::new(&entry.id);
            if let Err(e) = engine.resume(&entry.id).await {
                tracing::info!(torrent = %id, error = %e, "the torrent of a renewed Link session could not be resumed");
                return false;
            }
            reader.keep();
            true
        })
    }
    fn torrent_session_ended(&self, torrent_id: &str) {
        // BUG-131: a renew closes the replaced session after the new one holds the torrent: only the last reader's end
        // runs the after-watch policy (it pauses or removes the torrent)
        if !super::torrent_settings::link_reader_ended(torrent_id) {
            tracing::info!(torrent = %torrent_id, "a Link session of the torrent ended; another one still reads it");
            return;
        }
        let Some(b) = self.backend.get().and_then(std::sync::Weak::upgrade) else { return };
        let id = torrent_id.to_owned();
        tokio::spawn(async move {
            if let Err(e) = super::torrent_settings::playback_ended(&b, &id, false).await {
                tracing::debug!(detail = %e.detail, "torrent after-watch policy after a Link session failed");
            }
        });
    }
    fn trusted_media_origins(&self) -> Vec<tk_core::net::TrustedOrigin> {
        let prefs = crate::settings_store::load(&self.settings_file).server;
        if !prefs.enabled || !prefs.proxy_enabled {
            return Vec::new();
        }
        let mut ips = tk_link::detect_lan_ips();
        ips.extend(tk_server::lan_ips());
        own_media_origins(&ips, prefs.port)
    }
}

/// The player reported its presence (`Backend::presence`; integrator line in `LiveBackend::presence`): track it for
/// `GET /v1/player` and tell the remote-seek mapping where the player is (`screens/player/remote.rs`).
pub fn note_presence(b: &LiveBackend, state: &PresenceVm) {
    if let PresenceVm::Watching { position_secs, .. } = state {
        tk_ui::screens::player::remote::note_position(*position_secs);
    }
    if let Ok(mut g) = b.link.player.lock() {
        let prev = g.state.as_ref().map(|(_, s)| s.clone());
        g.state = merge_presence(prev.as_ref(), state).map(|s| (Instant::now(), s));
    }
}

/// The player saved progress (`Backend::record_progress`; integrator line in `LiveBackend::record_progress`): the
/// media id and position of what is playing.
pub fn note_progress(b: &LiveBackend, id: &str, episode: u32, progress_secs: u32, duration_secs: Option<u32>) {
    tk_ui::screens::player::remote::note_position(progress_secs);
    if let Ok(mut g) = b.link.player.lock() {
        let mut st = g.state.as_ref().map(|(_, s)| s.clone()).unwrap_or(PlayerState {
            playing: true,
            title: None,
            episode: Some(episode),
            position_secs: progress_secs,
            duration_secs,
            media_id: None,
        });
        st.media_id = Some(id.to_owned());
        st.episode = Some(episode);
        st.position_secs = progress_secs;
        if duration_secs.is_some() {
            st.duration_secs = duration_secs;
        }
        g.state = Some((Instant::now(), st));
    }
}

/// Wire command (`tk-link::control`) -> UI command. Pure; tested below.
fn remote_vm(cmd: tk_link::control::ControlCommand) -> RemoteCommandVm {
    use tk_link::control::ControlCommand as C;
    match cmd {
        C::Play => RemoteCommandVm::Play,
        C::Pause => RemoteCommandVm::Pause,
        C::Toggle => RemoteCommandVm::TogglePause,
        C::Next => RemoteCommandVm::NextEpisode,
        C::Prev => RemoteCommandVm::PrevEpisode,
        C::Seek { seconds } => RemoteCommandVm::Seek(seconds),
        C::Open { media_id, episode, position_secs } => RemoteCommandVm::Open { media_id, episode, position_secs },
    }
}

/// The Link session source for a resolved stream: only public http(s) URLs travel (the phone's session fetches
/// them through the SSRF-safe loopback fetcher of the PC). Pure.
fn session_source(url: &str, headers: &[(String, String)]) -> Option<SourceRequest> {
    let u = reqwest::Url::parse(url).ok()?;
    if !matches!(u.scheme(), "http" | "https") {
        return None;
    }
    let host = u.host_str()?;
    if host == "localhost" || host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback()) {
        return None;
    }
    Some(SourceRequest::Url {
        url: url.to_owned(),
        headers: headers.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        extension_id: None,
        container: None,
    })
}

fn link_api_err(e: tk_link::api::ApiError) -> UiError {
    let kind = match e.0 {
        tk_link::api::ErrorCode::DeviceNotAuthorized => ErrorKind::Auth,
        tk_link::api::ErrorCode::SourceUnavailable | tk_link::api::ErrorCode::UrlSourceDenied => ErrorKind::NoSource,
        _ => ErrorKind::Other,
    };
    UiError::new(kind, format!("{}: {}", e.0.as_str(), e.0.message()))
}

/// QA hook: with `TATAKAI_LINK_FAIL_TORRENT=1` in the PC's environment every torrent source of a Link session is
/// refused (`503 source_unavailable`, as when its metadata never arrives), so both "Enviar a dispositivo" from the PC and
/// "Enviar" from the phone fall back to a web stream and say so. Off when unset, empty or `0`.
pub const FAIL_TORRENT_ENV: &str = "TATAKAI_LINK_FAIL_TORRENT";

/// Whether [`FAIL_TORRENT_ENV`] asks to refuse torrent sources. Pure.
pub fn torrent_failure_forced(value: Option<&str>) -> bool {
    value.map(str::trim).is_some_and(|v| !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false"))
}

/// `Backend::send_to_device` (wave B, worker **k**; checklist `link.send-to-device`): resolve the episode on the PC
/// (`Backend::playback`, the same sources the PC player would use), start a Link session owned by the phone at
/// `position_secs` (`tk-link::sessions`, the path the phone itself uses with `POST /v1/sessions`) and queue the
/// `handoff` event its `GET /v1/events` long-poll delivers ("La PC te envió <título>" -> "Reproducir aquí").
///
/// BUG-107: the steps go to `req.progress` ("Preparando la fuente…" -> "Iniciando la sesión…") and a "Cancelar" while
/// the source is being resolved stops it before anything is created. The chosen torrent is tried before the web
/// streams are resolved (that resolution, every extension, was the 25 s the PC showed nothing even when the torrent
/// went); they are only resolved when the torrent is refused or there is none.
pub async fn send_to_device(b: &LiveBackend, req: &SendToDeviceVm) -> UiResult<SendOutcomeVm> {
    let progress = &req.progress;
    let h = b.link.ensure().await?;
    let device = h
        .devices()
        .into_iter()
        .find(|d| d.id == req.device_id && d.is_active(now_unix()))
        .ok_or_else(|| UiError::new(ErrorKind::Auth, format!("device {} is not paired", req.device_id)))?;
    // BUG-009 / BUG-115: the episode's chosen source goes first (a torrent streamed by this PC's engine, or the chosen
    // web stream resolved on its own); with no choice, the top-ranked source when it is a torrent. The web streams of
    // every extension follow, and only those say "sent another source".
    let picked = b.store.picked_source(&req.media_id, req.episode).ok().flatten();
    let first = match picked {
        Some(p) => first_source(Some(&p), None),
        None => first_source(None, unless_cancelled(progress, recommended_source(b, &req.media_id, req.episode)).await?.as_deref()),
    };
    let mut last: Option<UiError> = None;
    let mut tried_url: Option<String> = None;
    let torrent_first = matches!(first, FirstSource::Torrent(_));
    // a chosen source was tried before the web streams: any of them is "another source"
    let fallback = !matches!(first, FirstSource::Ranked);
    match first {
        FirstSource::Torrent(uri) => {
            let title = unless_cancelled(progress, b.media(&req.media_id)).await?.map(|(_, m)| m.title).unwrap_or_default();
            let source = SourceRequest::Torrent { uri, media_id: req.media_id.clone(), episode: req.episode };
            match start_and_hand_off(&h, &device.id, req, source, &title).await? {
                Ok(()) => return Ok(send_outcome(0, true, "")),
                Err(e) => last = Some(e),
            }
            tracing::info!("send to device: the torrent could not be opened, sending a web stream instead");
            progress.back_to_preparing();
        }
        FirstSource::Web(key) => {
            let resolved =
                unless_cancelled(progress, <LiveBackend as Backend>::playback_source(b, &req.media_id, req.episode, &key)).await?;
            match resolved {
                Ok(p) => {
                    let chosen = p.streams.iter().find_map(|s| session_source(&s.url, &s.headers).map(|src| (src, s.url.clone())));
                    if let Some((source, url)) = chosen {
                        tracing::info!(source = %key, "send to device: sending the chosen web source");
                        match start_and_hand_off(&h, &device.id, req, source, &p.title).await? {
                            Ok(()) => return Ok(send_outcome(0, false, "")),
                            Err(e) => last = Some(e),
                        }
                        tried_url = Some(url);
                        progress.back_to_preparing();
                    } else {
                        last = Some(UiError::new(ErrorKind::NoSource, format!("{key}: no stream the phone can open")));
                    }
                }
                Err(e) => last = Some(e),
            }
            tracing::info!(source = %key, "send to device: the chosen web source could not be sent, trying the others");
        }
        FirstSource::Ranked => {}
    }
    let playback = match unless_cancelled(progress, <LiveBackend as Backend>::playback(b, &req.media_id, req.episode)).await? {
        Ok(p) => p,
        Err(e) if fallback => {
            tracing::debug!(detail = %e.detail, "send to device: no other web stream after the chosen source");
            return Err(last.unwrap_or(e));
        }
        Err(e) => return Err(e),
    };
    // names for the "sent another source" notice (plan §9.1 / §11.4): the chosen source is never named
    let offset = usize::from(fallback);
    let streams = playback
        .streams
        .iter()
        .filter(|s| tried_url.as_deref() != Some(s.url.as_str()))
        .filter_map(|s| session_source(&s.url, &s.headers).map(|src| (src, stream_name(&s.provider, s.quality.as_deref()))));
    for (i, (source, name)) in streams.enumerate() {
        match start_and_hand_off(&h, &device.id, req, source, &playback.title).await? {
            Ok(()) => return Ok(send_outcome(i + offset, torrent_first, &name)),
            Err(e) => last = Some(e),
        }
        progress.back_to_preparing();
    }
    Err(last.unwrap_or_else(|| UiError::new(ErrorKind::NoSource, "no stream of this episode can be sent to the phone")))
}

/// BUG-107: the error of a send the user cancelled (the UI already knows and shows nothing).
fn send_cancelled() -> UiError {
    UiError::new(ErrorKind::Other, "send to device: cancelled")
}

/// BUG-107: `fut`, unless the user presses "Cancelar" first (then `Err(cancelled)` and `fut` is dropped: these steps
/// only read, nothing is left half-made).
async fn unless_cancelled<T>(progress: &SendProgress, fut: impl std::future::Future<Output = T>) -> UiResult<T> {
    if progress.is_cancelled() {
        return Err(send_cancelled());
    }
    tokio::select! {
        biased;
        () = progress.cancelled() => Err(send_cancelled()),
        out = fut => Ok(out),
    }
}

/// One attempt of [`send_to_device`]: start the session for `device_id` with `source` and queue the handoff. The outer
/// `Err` ends the send (cancelled, device revoked); the inner one is this source refused (try the next).
async fn start_and_hand_off(
    h: &LinkHandle,
    device_id: &str,
    req: &SendToDeviceVm,
    source: SourceRequest,
    title: &str,
) -> UiResult<Result<(), UiError>> {
    if !req.progress.begin_session() {
        return Err(send_cancelled());
    }
    let create = CreateRequest { source, profile: None, audio_track: 0, start_secs: f64::from(req.position_secs) };
    match h.create_session_for(device_id, create).await {
        Ok(created) => {
            let event = PhoneEvent::Handoff {
                session_id: created.session_id,
                media_id: Some(req.media_id.clone()),
                episode: Some(req.episode),
                position_secs: req.position_secs,
                title: Some(title.to_owned()).filter(|t| !t.is_empty()),
            };
            if !h.push_event(device_id, event) {
                return Err(UiError::new(ErrorKind::Auth, format!("device {device_id} is not paired")));
            }
            tracing::info!(device = %device_id, media = %req.media_id, episode = req.episode, "sent to device");
            Ok(Ok(()))
        }
        Err(e) => {
            tracing::debug!(error = ?e.0, "send to device: session refused, trying the next stream");
            Ok(Err(link_api_err(e)))
        }
    }
}

/// A web stream as the user knows it: "anizone · 1080p" (provider, then quality). Pure.
fn stream_name(provider: &str, quality: Option<&str>) -> String {
    let provider = provider.trim();
    match quality.map(str::trim).filter(|q| !q.is_empty()) {
        Some(q) if !provider.is_empty() => format!("{provider} · {q}"),
        Some(q) => q.to_owned(),
        None => provider.to_owned(),
    }
}

/// Plan §9.1 / §11.4: what to tell the user after the source at `index` was sent. The first one is the chosen source
/// (the torrent when `torrent_first`, else the chosen or top web stream); any later one went "instead". Pure.
fn send_outcome(index: usize, torrent_first: bool, name: &str) -> SendOutcomeVm {
    if index == 0 {
        return SendOutcomeVm::default();
    }
    SendOutcomeVm { sent_instead: Some(name.trim().to_owned()), torrent_failed: torrent_first }
}

/// What [`send_to_device`] tries before the web streams of every extension.
#[derive(Debug, Clone, PartialEq, Eq)]
enum FirstSource {
    /// A torrent source id, streamed by the PC's engine.
    Torrent(String),
    /// The web source the user chose for the episode, resolved on its own.
    Web(String),
    /// No choice and the top-ranked source is a web stream: the resolved web streams, best first.
    Ranked,
}

/// BUG-009 / BUG-115: the episode's chosen source (`picked`, torrent or web) wins; with no choice, the top-ranked
/// source (`recommended`) when it is a torrent (the order the PC player uses). Pure.
fn first_source(picked: Option<&str>, recommended: Option<&str>) -> FirstSource {
    match picked.map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) if is_torrent_source_id(p) => FirstSource::Torrent(p.to_owned()),
        Some(p) => FirstSource::Web(p.to_owned()),
        None => match recommended.filter(|r| is_torrent_source_id(r)) {
            Some(r) => FirstSource::Torrent(r.to_owned()),
            None => FirstSource::Ranked,
        },
    }
}

/// The id of the top-ranked source of `media_id` / `episode` (only asked when the user chose none).
async fn recommended_source(b: &LiveBackend, media_id: &str, episode: u32) -> Option<String> {
    let ranked = <LiveBackend as Backend>::sources(b, media_id, episode).await.ok()?;
    ranked.into_iter().next().map(|s| s.id)
}

/// `<ext>|torrent|<label>` (`tk_core::sources::source_key`). Pure.
fn is_torrent_source_id(id: &str) -> bool {
    tk_core::sources::parse_source_key(id).is_some_and(|(_, kind, _)| kind == tk_core::sources::SourceKind::Torrent)
}

/// `Settings::server.link_announce` changed (or start-up): announce Link over mDNS or stop (`link.discovery`).
pub async fn set_announce(b: &LiveBackend, on: bool) {
    b.link.announce.store(on, Ordering::Relaxed);
    let st = b.link.state.lock().await;
    let handle = st.handle.clone();
    drop(st);
    match (on, handle) {
        (true, Some(h)) => b.link.start_announcer(&h),
        _ => b.link.stop_announcer(),
    }
}

/// `Backend::wait_remote_command` (wave B, worker **k**; `link.remote-control`, `link.handoff`): the next command a
/// paired phone posted to `/v1/control`. Implemented: it awaits the channel `AppLinkHost::control` feeds. The route
/// handler itself is k's (`tk-link/src/server.rs`); until k adds it, nothing arrives and the shell simply waits.
pub async fn wait_remote_command(b: &LiveBackend) -> UiResult<RemoteCommandVm> {
    let mut rx = b.remote.lock().await;
    rx.recv().await.ok_or_else(|| UiError::new(ErrorKind::Other, "remote command channel closed"))
}

/// The running server (if any) and the port it uses or will try next.
struct LinkState {
    handle: Option<Arc<LinkHandle>>,
    port: u16,
}

pub struct LinkService {
    dir: PathBuf,
    ext: Arc<ExtensionHost>,
    store: Arc<LocalStore>,
    anime4k_dir: PathBuf,
    anime4k_lock: Option<PathBuf>,
    /// `Lossless.dll` for the LSFG profiles ([`link_lsfg_dll`]); `None` = Steam lookup by tk-link.
    lsfg_dll: Option<PathBuf>,
    state: tokio::sync::Mutex<LinkState>,
    /// Wave B (k): handed to every `AppLinkHost` so `/v1/control` reaches `LiveBackend::wait_remote_command`.
    remote: tokio::sync::mpsc::UnboundedSender<RemoteCommandVm>,
    /// Wave B (k): the player state shared with every `AppLinkHost` (`GET /v1/player`).
    player: SharedPlayer,
    /// Wave B (k, `link.discovery`): announce over mDNS while the server runs (`Settings::server.link_announce`).
    announce: AtomicBool,
    announcer: Mutex<Option<Announcer>>,
    /// BUG-009: shared with every `AppLinkHost` ([`bind_backend`]).
    backend: BackendSlot,
}

/// `name` in one of the absolute `PATH` entries.
fn find_in_path(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?).filter(|d| d.is_absolute()).map(|d| d.join(name)).find(|p| p.is_file())
}

/// The `Lossless.dll` for Link's LSFG helper from the player's "Lossless.dll" setting. temple-lsfg reads the game's
/// `Lossless.dll` (not the `lsfg-vk.dll` pack lsfg-vk 2.x wants): a setting that names another file is used only when
/// a `Lossless.dll` sits next to it; otherwise Link searches Steam itself.
pub fn link_lsfg_dll(setting: Option<&str>) -> Option<PathBuf> {
    let p = PathBuf::from(setting.map(str::trim).filter(|s| !s.is_empty())?);
    if p.file_name().is_some_and(|n| n.eq_ignore_ascii_case("Lossless.dll")) {
        return Some(p);
    }
    let sibling = p.with_file_name("Lossless.dll");
    sibling.is_file().then_some(sibling)
}

/// The chosen port lives next to the Link identity (`<link>/port`); missing or invalid = the default.
const PORT_FILE: &str = "port";

fn load_port(dir: &Path) -> u16 {
    std::fs::read_to_string(dir.join(PORT_FILE))
        .ok()
        .and_then(|s| s.trim().parse::<u16>().ok())
        .filter(|p| *p != 0)
        .unwrap_or(tk_link::DEFAULT_PORT)
}

fn save_port(dir: &Path, port: u16) {
    let write = std::fs::create_dir_all(dir).and_then(|()| std::fs::write(dir.join(PORT_FILE), format!("{port}\n")));
    if let Err(e) = write {
        tracing::warn!(error = %e, "could not persist the Link port");
    }
}

fn server_name() -> String {
    std::env::var("COMPUTERNAME").ok().filter(|n| !n.trim().is_empty()).map_or_else(|| "Tatakai".to_owned(), |n| format!("Tatakai ({n})"))
}

fn now_unix() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

fn err(e: impl std::fmt::Display) -> UiError {
    UiError::new(ErrorKind::Other, e.to_string())
}

fn is_addr_in_use(e: &std::io::Error) -> bool {
    // WSAEADDRINUSE (10048) / EADDRINUSE (98 on Linux); std maps both to AddrInUse, the codes are a fallback.
    e.kind() == std::io::ErrorKind::AddrInUse || matches!(e.raw_os_error(), Some(10048 | 98))
}

/// Start failure -> `UiError`. A taken port is `AddrInUse` with the OS message as detail.
fn start_err(e: &LinkError, port: u16) -> UiError {
    match e {
        LinkError::Io(io) if is_addr_in_use(io) => UiError::new(ErrorKind::AddrInUse, format!("io: {io} · bind :{port}")),
        _ => err(e),
    }
}

/// The active paired devices of `<dir>/devices.json` with the server down: none online, no error. `counts` = history
/// rows per origin device.
fn paired_vm(dir: &Path, port: u16, counts: &HashMap<String, u64>) -> LinkStatusVm {
    let devices = tk_link::devices::DeviceRegistry::open(dir.join("devices.json"))
        .map(|r| r.list())
        .unwrap_or_default()
        .into_iter()
        .filter(|d| d.is_active(now_unix()))
        .map(|d| device_vm(d, &[], counts, false))
        .collect();
    LinkStatusVm { running: false, port, devices, error: None }
}

fn device_vm(d: Device, online: &[String], counts: &HashMap<String, u64>, resyncing: bool) -> DeviceVm {
    // Wave B (k, `link.last-seen`): tk-link refreshes both on every authenticated request of the phone.
    let online = online.iter().any(|id| *id == d.id);
    let contributed = counts.get(&d.id).copied().unwrap_or(0);
    DeviceVm {
        history_mode: mode_vm(d.history_mode),
        contributed,
        resyncing,
        id: d.id,
        name: d.name,
        platform: d.platform,
        last_seen_unix: d.last_seen_at,
        online,
    }
}

/// tk-link history mode -> view model. Pure.
fn mode_vm(m: HistoryMode) -> HistoryModeVm {
    match m {
        HistoryMode::Full => HistoryModeVm::Full,
        HistoryMode::ReceiveOnly => HistoryModeVm::ReceiveOnly,
        HistoryMode::Paused => HistoryModeVm::Paused,
    }
}

/// View model -> tk-link history mode. Pure.
fn mode_link(m: HistoryModeVm) -> HistoryMode {
    match m {
        HistoryModeVm::Full => HistoryMode::Full,
        HistoryModeVm::ReceiveOnly => HistoryMode::ReceiveOnly,
        HistoryModeVm::Paused => HistoryMode::Paused,
    }
}

/// `7A3F 91C2 0E5D B8A4`: the first 16 hex digits of the CA fingerprint, upper case, in groups of 4.
fn display_fingerprint(hex: &str) -> String {
    let digits: Vec<char> = hex.chars().filter(char::is_ascii_hexdigit).take(16).map(|c| c.to_ascii_uppercase()).collect();
    digits.chunks(4).map(|c| c.iter().collect::<String>()).collect::<Vec<_>>().join(" ")
}

fn vendor_label(v: Vendor) -> Option<&'static str> {
    match v {
        Vendor::Amd => Some("AMD"),
        Vendor::Nvidia => Some("NVIDIA"),
        Vendor::Intel => Some("Intel"),
        Vendor::Other | Vendor::Unknown => None,
    }
}

/// Stop a shared handle. The handle is only cloned for the duration of a call, so it is normally unique by now;
/// if not, stop it in place and give the listener tasks a moment to release the port.
async fn stop_handle(mut h: Arc<LinkHandle>) {
    for _ in 0..20 {
        match Arc::try_unwrap(h) {
            Ok(owned) => {
                owned.shutdown().await;
                return;
            }
            Err(shared) => {
                h = shared;
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        }
    }
    h.stop();
    tokio::time::sleep(Duration::from_millis(150)).await;
}

impl LinkService {
    pub fn new(
        dir: PathBuf,
        ext: Arc<ExtensionHost>,
        store: Arc<LocalStore>,
        anime4k_dir: PathBuf,
        anime4k_lock: Option<PathBuf>,
        lsfg_dll: Option<PathBuf>,
        remote: tokio::sync::mpsc::UnboundedSender<RemoteCommandVm>,
    ) -> Self {
        let port = load_port(&dir);
        Self {
            dir,
            ext,
            store,
            anime4k_dir,
            anime4k_lock,
            lsfg_dll,
            state: tokio::sync::Mutex::new(LinkState { handle: None, port }),
            remote,
            player: SharedPlayer::default(),
            announce: AtomicBool::new(true),
            announcer: Mutex::new(None),
            backend: BackendSlot::default(),
        }
    }

    /// (Re)register the mDNS service for `h` (best effort: a failure only means phones must scan the QR).
    fn start_announcer(&self, h: &LinkHandle) {
        let ips: Vec<std::net::IpAddr> = h.local_addrs().iter().map(std::net::SocketAddr::ip).collect();
        let next = match Announcer::start_on(&h.server_name(), &h.instance_id(), h.port(), &h.ca_fingerprint(), &ips) {
            Ok(a) => Some(a),
            Err(e) => {
                tracing::warn!(error = %e, "mDNS announcement failed");
                None
            }
        };
        if let Ok(mut g) = self.announcer.lock() {
            *g = next; // the previous one (if any) unregisters on drop
        }
    }

    fn stop_announcer(&self) {
        if let Ok(mut g) = self.announcer.lock() {
            g.take();
        }
    }

    /// Start on `st.port`.
    async fn start_locked(&self, st: &mut LinkState) -> Result<Arc<LinkHandle>, UiError> {
        let mut cfg = LinkConfig::new(self.dir.clone(), server_name());
        cfg.port = st.port;
        cfg.anime4k_dir = Some(self.anime4k_dir.clone());
        cfg.anime4k_lock = self.anime4k_lock.clone();
        // LSFG (experimental): temple-lsfg via TEMPLE_LSFG or next to the executable; probed once per server start
        cfg.lsfg = tk_link::LsfgSetup::Detect { dll: self.lsfg_dll.clone() };
        let host = Arc::new(AppLinkHost {
            ext: self.ext.clone(),
            store: self.store.clone(),
            remote: self.remote.clone(),
            player: self.player.clone(),
            // `<data>/link` -> `<data>/ui-settings.json` (`AppPaths::ui_settings_file`)
            backend: self.backend.clone(),
            settings_file: self.dir.parent().map_or_else(|| self.dir.join("ui-settings.json"), |root| root.join("ui-settings.json")),
        });
        match tk_link::start(cfg, host).await {
            Ok(h) => {
                let h = Arc::new(h);
                st.handle = Some(h.clone());
                if self.announce.load(Ordering::Relaxed) {
                    self.start_announcer(&h);
                }
                Ok(h)
            }
            Err(e) => {
                tracing::warn!(port = st.port, error = %e, "Tatakai Link did not start");
                Err(start_err(&e, st.port))
            }
        }
    }

    async fn ensure(&self) -> Result<Arc<LinkHandle>, UiError> {
        let mut st = self.state.lock().await;
        if let Some(h) = st.handle.as_ref() {
            return Ok(h.clone());
        }
        self.start_locked(&mut st).await
    }

    fn running_vm(&self, h: &LinkHandle) -> LinkStatusVm {
        let online = h.online_device_ids();
        let counts = super::history_sync::origin_counts(&self.store);
        LinkStatusVm {
            running: true,
            port: h.port(),
            devices: h
                .devices()
                .into_iter()
                .filter(|d| d.is_active(now_unix()))
                .map(|d| {
                    let resyncing = h.resync_running(&d.id);
                    device_vm(d, &online, &counts, resyncing)
                })
                .collect(),
            error: None,
        }
    }

    /// Server down: the paired devices still come from `devices.json`, none of them online.
    fn stopped_vm(&self, port: u16, error: UiError) -> LinkStatusVm {
        LinkStatusVm { error: Some(error), ..paired_vm(&self.dir, port, &super::history_sync::origin_counts(&self.store)) }
    }

    /// BUG-130: the status without starting the server (no firewall prompt): the running server's devices, or the
    /// paired ones from `devices.json` (none online) while it is down. Waits for a start in progress (the state lock),
    /// so right after launch it answers with the server `start_if_paired` brought up.
    pub async fn peek(&self) -> LinkStatusVm {
        let st = self.state.lock().await;
        match st.handle.as_ref() {
            Some(h) => self.running_vm(h),
            None => paired_vm(&self.dir, st.port, &super::history_sync::origin_counts(&self.store)),
        }
    }

    /// App start-up: with at least one paired phone the server starts right away, so the phone reaches this PC without
    /// the user opening the Link screen first. With none it stays lazy (no listening port until pairing).
    pub async fn start_if_paired(&self) {
        let paired = tk_link::devices::DeviceRegistry::open(self.dir.join("devices.json"))
            .map(|r| r.list().into_iter().any(|d| d.is_active(now_unix())))
            .unwrap_or(false);
        if paired && let Err(e) = self.ensure().await {
            tracing::warn!(detail = %e.detail, "Tatakai Link could not start");
        }
    }

    /// Starts the server if it is not running (each call retries a failed start).
    pub async fn status(&self) -> Result<LinkStatusVm, UiError> {
        let mut st = self.state.lock().await;
        if let Some(h) = st.handle.clone() {
            return Ok(self.running_vm(&h));
        }
        match self.start_locked(&mut st).await {
            Ok(h) => Ok(self.running_vm(&h)),
            Err(e) => Ok(self.stopped_vm(st.port, e)),
        }
    }

    /// Stop the server (if running) and start it on `port`. The port is persisted only when the start works.
    pub async fn restart(&self, port: u16) -> Result<LinkStatusVm, UiError> {
        if port == 0 {
            return Err(err("port 0 is not a valid Link port"));
        }
        let mut st = self.state.lock().await;
        if let Some(h) = st.handle.take() {
            self.stop_announcer();
            stop_handle(h).await;
        }
        st.port = port;
        match self.start_locked(&mut st).await {
            Ok(h) => {
                save_port(&self.dir, h.port());
                Ok(self.running_vm(&h))
            }
            Err(e) => Ok(self.stopped_vm(port, e)),
        }
    }

    /// The port the server uses (or will try), for the firewall rule.
    pub async fn port(&self) -> u16 {
        let st = self.state.lock().await;
        st.handle.as_ref().map_or(st.port, |h| h.port())
    }

    pub async fn open_pairing(&self) -> Result<Pairing, UiError> {
        let p = self.ensure().await?.open_pairing_window();
        Ok(Pairing { uri: p.uri(), code: p.code, expires_unix: p.expires_at.unix_timestamp() })
    }

    pub async fn revoke(&self, id: &str) -> Result<(), UiError> {
        self.ensure().await?.revoke(id);
        Ok(())
    }

    /// Persisted in tk-link `devices.json` (the name is sanitized there).
    pub async fn rename(&self, id: &str, name: &str) -> Result<(), UiError> {
        if self.ensure().await?.rename_device(id, name) {
            Ok(())
        } else {
            Err(err(format!("device {id} could not be renamed (unknown, revoked or empty name)")))
        }
    }

    /// 2026-11 history per device: persisted in tk-link `devices.json`.
    pub async fn set_history_mode(&self, id: &str, mode: HistoryModeVm) -> Result<(), UiError> {
        if self.ensure().await?.set_history_mode(id, mode_link(mode)) {
            Ok(())
        } else {
            Err(err(format!("device {id} is unknown or revoked")))
        }
    }

    /// "Use as source": the device pushes everything (winning for the window), then the others replace theirs.
    pub async fn use_as_source(&self, id: &str) -> Result<(), UiError> {
        if self.ensure().await?.start_resync_from(id) {
            Ok(())
        } else {
            Err(err(format!("device {id} cannot be the history source (unknown, revoked or not in full mode)")))
        }
    }

    /// Delete the PC history rows whose origin is `id` (no server needed: the rows are in the store).
    pub async fn delete_contributed(&self, id: &str) -> Result<u64, UiError> {
        let store = self.store.clone();
        let id = id.to_owned();
        let n = tokio::task::spawn_blocking(move || store.delete_history_by_origin(&id)).await.map_err(err)?.map_err(err)?;
        Ok(n as u64)
    }

    /// What `GET /v1/capabilities` answers, read from the running server (no network round trip).
    pub async fn capabilities(&self) -> Result<LinkCapsVm, UiError> {
        let h = self.ensure().await?;
        let caps = h.capabilities();
        let gpu = Some(caps.gpu.name.trim().to_owned())
            .filter(|n| !n.is_empty())
            .or_else(|| vendor_label(caps.gpu.vendor).map(str::to_owned));
        Ok(LinkCapsVm {
            gpu,
            // tk-link only knows whether Vulkan works through libplacebo, not the API version.
            vulkan: caps.gpu.vulkan.then(|| "libplacebo".to_owned()),
            encoder: caps.encoders.first().map(|e| (*e).to_owned()),
            upscale: caps.profiles.iter().any(|p| p.upscale == "anime4k").then(|| "Anime4K (libplacebo)".to_owned()),
            profiles: caps
                .profiles
                .iter()
                .map(|p| {
                    let measured = match p.measured_sustain {
                        Sustain::Yes => Some(true),
                        Sustain::No => Some(false),
                        Sustain::Unknown => None,
                    };
                    (p.id.to_owned(), measured)
                })
                .collect(),
            fingerprint: display_fingerprint(&h.ca_fingerprint()),
            lan_ip: h.local_addrs().iter().map(std::net::SocketAddr::ip).find(|ip| !ip.is_loopback()).map(|ip| ip.to_string()),
            anime4k_note: anime4k_note(caps.gpu.vulkan, &self.anime4k_dir, &caps.profiles),
            lsfg_reason: (!caps.lsfg.available).then(|| caps.lsfg.reason.clone().unwrap_or_else(|| "probe_failed".to_owned())),
        })
    }
}

/// Why the Anime4K profiles are missing or partial (BUG-048 / BUG-010), from what the PC has (`vulkan`, the shader
/// files) and what tk-link offers.
fn anime4k_note(vulkan: bool, dir: &std::path::Path, profiles: &[tk_link::media::capabilities::ProfileInfo]) -> Option<Anime4kNoteVm> {
    use tk_link::media::anime4k::{Anime4kMode, Anime4kVariant, chain_present, shaders_present};
    if !vulkan {
        return Some(Anime4kNoteVm::NoVulkan);
    }
    let offered = |h: &str| profiles.iter().any(|p| p.upscale == "anime4k" && p.id.contains(h));
    if !shaders_present(dir) || !profiles.iter().any(|p| p.upscale == "anime4k") {
        return Some(Anime4kNoteVm::ShadersMissing);
    }
    if !chain_present(dir, Anime4kMode::A, Anime4kVariant::L) && !offered("2160") {
        return Some(Anime4kNoteVm::LargeMissing);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_cancel_drops_the_slow_resolution() {
        // BUG-107: "Cancelar" while the extensions are still resolving ends the send right away, and nothing after it
        let p = SendProgress::new();
        let slow = unless_cancelled(&p, async {
            tokio::time::sleep(Duration::from_secs(60)).await;
            1
        });
        let cancel = async {
            tokio::task::yield_now().await;
            assert!(p.cancel());
        };
        let (out, ()) = tokio::time::timeout(Duration::from_secs(2), async { tokio::join!(slow, cancel) }).await.expect("not waited out");
        assert!(out.is_err());
        // a cancelled send does not even start the next step, nor a session
        assert!(unless_cancelled(&p, async { 2 }).await.is_err());
        assert!(!p.begin_session());
        // not cancelled: the step's value comes through
        let q = SendProgress::new();
        assert_eq!(unless_cancelled(&q, async { 3 }).await.ok(), Some(3));
    }

    #[test]
    fn the_paired_devices_are_read_without_the_server() {
        // BUG-130: Details lists the paired phones right after launch, without starting Link
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(paired_vm(dir.path(), 47321, &HashMap::new()), LinkStatusVm { running: false, port: 47321, devices: Vec::new(), error: None });
        let reg = tk_link::devices::DeviceRegistry::open(dir.path().join("devices.json")).unwrap();
        let now = now_unix();
        let dev = |id: &str, revoked_at: Option<i64>| Device {
            id: id.into(),
            name: format!("phone {id}"),
            fingerprint: format!("fp-{id}"),
            created_at: now - 10,
            not_after: now + 3600,
            revoked_at,
            platform: "android".into(),
            last_seen_at: Some(now - 5),
            history_mode: HistoryMode::ReceiveOnly,
        };
        reg.add(dev("a", None)).unwrap();
        reg.add(dev("b", Some(now - 1))).unwrap();
        drop(reg);
        let vm = paired_vm(dir.path(), 47321, &HashMap::from([("a".to_owned(), 7)]));
        assert!(!vm.running && vm.error.is_none());
        assert_eq!(vm.devices.len(), 1);
        assert_eq!(vm.devices[0].id, "a");
        assert!(!vm.devices[0].online);
        assert_eq!(vm.devices[0].last_seen_unix, Some(now - 5));
        // 2026-11 history per device: the mode and the contributed rows travel to the row
        assert_eq!((vm.devices[0].history_mode, vm.devices[0].contributed, vm.devices[0].resyncing), (HistoryModeVm::ReceiveOnly, 7, false));
        for m in [HistoryModeVm::Full, HistoryModeVm::ReceiveOnly, HistoryModeVm::Paused] {
            assert_eq!(mode_vm(mode_link(m)), m);
        }
    }

    #[test]
    fn the_qa_hook_refuses_torrents_only_when_asked() {
        assert!(!torrent_failure_forced(None));
        for off in ["", " ", "0", "false"] {
            assert!(!torrent_failure_forced(Some(off)), "{off:?}");
        }
        for on in ["1", "yes", "true"] {
            assert!(torrent_failure_forced(Some(on)), "{on:?}");
        }
    }

    #[test]
    fn the_lsfg_dll_setting_must_be_the_games_lossless_dll() {
        assert_eq!(link_lsfg_dll(None), None);
        assert_eq!(link_lsfg_dll(Some("  ")), None);
        assert_eq!(link_lsfg_dll(Some("/s/Lossless Scaling/Lossless.dll")), Some(PathBuf::from("/s/Lossless Scaling/Lossless.dll")));
        let dir = tempfile::tempdir().unwrap();
        let pack = dir.path().join("lsfg-vk.dll");
        assert_eq!(link_lsfg_dll(Some(pack.to_str().unwrap())), None, "no Lossless.dll next to the lsfg-vk pack");
        std::fs::write(dir.path().join("Lossless.dll"), b"x").unwrap();
        assert_eq!(link_lsfg_dll(Some(pack.to_str().unwrap())), Some(dir.path().join("Lossless.dll")));
    }

    #[test]
    fn control_commands_map_to_ui_commands() {
        use tk_link::control::ControlCommand as C;
        assert_eq!(remote_vm(C::Toggle), RemoteCommandVm::TogglePause);
        assert_eq!(remote_vm(C::Seek { seconds: 90 }), RemoteCommandVm::Seek(90));
        assert_eq!(
            remote_vm(C::Open { media_id: "21".into(), episode: 3, position_secs: 120 }),
            RemoteCommandVm::Open { media_id: "21".into(), episode: 3, position_secs: 120 }
        );
    }

    #[test]
    fn player_tracking_and_redundant_commands() {
        use tk_link::control::ControlCommand as C;
        let watching = PresenceVm::Watching {
            title: "One Piece".into(),
            episode: 3,
            position_secs: 90,
            duration_secs: Some(1440),
            paused: true,
            poster: None,
        };
        let prev = PlayerState { playing: true, title: None, episode: Some(3), position_secs: 80, duration_secs: None, media_id: Some("21".into()) };
        let st = merge_presence(Some(&prev), &watching).unwrap();
        assert!(!st.playing);
        assert_eq!(st.media_id.as_deref(), Some("21"), "same episode keeps the media id");
        assert_eq!(st.position_secs, 90);
        let other_ep = PlayerState { episode: Some(2), ..prev.clone() };
        assert_eq!(merge_presence(Some(&other_ep), &watching).unwrap().media_id, None);
        assert_eq!(merge_presence(Some(&prev), &PresenceVm::Browsing), None, "leaving the player closes it");
        assert!(redundant(&C::Pause, Some(&st)));
        assert!(!redundant(&C::Play, Some(&st)));
        assert!(!redundant(&C::Toggle, Some(&st)));
        let track = SharedPlayer::default();
        assert_eq!(current_player(&track), None);
        track.lock().unwrap().state = Some((Instant::now(), st.clone()));
        assert_eq!(current_player(&track), Some(st));
    }

    #[test]
    fn only_public_streams_become_session_sources() {
        let h = vec![("Referer".to_owned(), "https://site/".to_owned())];
        match session_source("https://cdn.example/a.m3u8", &h) {
            Some(SourceRequest::Url { url, headers, .. }) => {
                assert_eq!(url, "https://cdn.example/a.m3u8");
                assert_eq!(headers.get("Referer").map(String::as_str), Some("https://site/"));
            }
            _ => panic!("expected a url source"),
        }
        assert!(session_source("http://127.0.0.1:8099/api/server/v1/media/x/a.m3u8", &[]).is_none());
        assert!(session_source("file:///home/a.mkv", &[]).is_none());
        assert!(session_source("not a url", &[]).is_none());
    }

    #[test]
    fn own_media_origins_cover_the_lan_ips_and_loopback_only_on_the_server_port() {
        let o = own_media_origins(&["192.168.1.43".parse().unwrap(), "192.168.1.43".parse().unwrap()], 8099);
        assert_eq!(o.len(), 2);
        let ok = reqwest::Url::parse("http://192.168.1.43:8099/api/server/v1/media/abc/index.m3u8").unwrap();
        assert!(o.iter().any(|t| t.matches(&ok)));
        let lo = reqwest::Url::parse("http://127.0.0.1:8099/api/server/v1/media/abc/x.ts").unwrap();
        assert!(o.iter().any(|t| t.matches(&lo)));
        for bad in ["http://192.168.1.43:8098/api/server/v1/media/abc/x.ts", "http://192.168.1.43:8099/api/server/v1/devices", "http://192.168.1.44:8099/api/server/v1/media/a/x.ts"] {
            assert!(!o.iter().any(|t| t.matches(&reqwest::Url::parse(bad).unwrap())), "{bad}");
        }
    }

    #[test]
    fn the_chosen_source_goes_first_even_when_the_recommended_one_is_a_torrent() {
        // BUG-115: chosen web + recommended torrent -> the chosen web source, never the torrent
        assert_eq!(first_source(Some("toko|web|anizone"), Some("toko|torrent|ToonsHub")), FirstSource::Web("toko|web|anizone".into()));
        assert_eq!(first_source(Some("toko|web|anizone"), None), FirstSource::Web("toko|web|anizone".into()));
        // a chosen torrent
        assert_eq!(first_source(Some("toko|torrent|Erai"), Some("toko|torrent|ToonsHub")), FirstSource::Torrent("toko|torrent|Erai".into()));
        assert_eq!(first_source(Some("toko|torrent|Erai"), Some("toko|web|anizone")), FirstSource::Torrent("toko|torrent|Erai".into()));
        // no choice: the recommended one when it is a torrent, else the ranked web streams
        assert_eq!(first_source(None, Some("toko|torrent|ToonsHub")), FirstSource::Torrent("toko|torrent|ToonsHub".into()));
        assert_eq!(first_source(Some(" "), Some("toko|torrent|ToonsHub")), FirstSource::Torrent("toko|torrent|ToonsHub".into()));
        assert_eq!(first_source(None, Some("toko|web|anizone")), FirstSource::Ranked);
        assert_eq!(first_source(None, None), FirstSource::Ranked);
    }

    #[test]
    fn torrent_source_ids_are_recognised() {
        assert!(is_torrent_source_id("toko|torrent|ToonsHub"));
        assert!(is_torrent_source_id("manual|torrent|2b5d833d"));
        assert!(!is_torrent_source_id("toko|web|anizone"));
        assert!(!is_torrent_source_id("magnet:?xt=urn:btih:2b5d"));
    }

    #[test]
    fn fingerprint_display_form() {
        assert_eq!(display_fingerprint("7a3f91c20e5db8a4ffff"), "7A3F 91C2 0E5D B8A4");
        assert_eq!(display_fingerprint("ab"), "AB");
    }

    #[test]
    fn port_file_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(load_port(tmp.path()), tk_link::DEFAULT_PORT);
        save_port(tmp.path(), 47331);
        assert_eq!(load_port(tmp.path()), 47331);
        std::fs::write(tmp.path().join(PORT_FILE), "nope").unwrap();
        assert_eq!(load_port(tmp.path()), tk_link::DEFAULT_PORT);
    }

    #[test]
    fn addr_in_use_maps_to_its_kind() {
        let e = LinkError::Io(std::io::Error::from(std::io::ErrorKind::AddrInUse));
        assert_eq!(start_err(&e, 47321).kind, ErrorKind::AddrInUse);
        assert_eq!(start_err(&LinkError::NoAddress, 47321).kind, ErrorKind::Other);
    }

    #[test]
    fn send_outcome_names_the_source_sent_instead() {
        // plan §9.1 / §11.4: the chosen source went: nothing to say
        assert_eq!(send_outcome(0, true, ""), SendOutcomeVm::default());
        assert_eq!(send_outcome(0, false, "anizone · 1080p"), SendOutcomeVm::default());
        // the torrent could not be opened: the first web stream went
        assert_eq!(send_outcome(1, true, "anizone · 1080p"), SendOutcomeVm { sent_instead: Some("anizone · 1080p".into()), torrent_failed: true });
        // the chosen web stream was refused: another one went
        assert_eq!(send_outcome(2, false, "kwik"), SendOutcomeVm { sent_instead: Some("kwik".into()), torrent_failed: false });
        assert_eq!(stream_name("anizone", Some("1080p")), "anizone · 1080p");
        assert_eq!(stream_name("anizone", Some(" ")), "anizone");
        assert_eq!(stream_name("", Some("720p")), "720p");
    }
}
