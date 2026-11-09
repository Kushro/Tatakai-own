//! `Backend` list calls (wave A, worker **g**; docs/redesign/15 section 4): the app identity is the user's AniList
//! and / or MyAnimeList account (plan 14 section 0.1 #9, no own login).
//!
//! Model (all in `data.db`, `tk_core::store::lists`):
//! - `service = "local"` rows are the user's list. They work without any account (`lists.local-only`) and are what
//!   the Library, the details `ListButton` and the automatic progress read and write.
//! - `service = "anilist" | "mal"` rows are the *base*: the last state seen on that service. The three-way rules of
//!   `tk_core::lists::sync::decide` compare local / base / remote, so a change made on one side only is never a
//!   conflict.
//! - `list_queue` is the offline queue (`upsert | delete | favorite | unfavorite` per service). Every local write
//!   queues one op per connected service and tries to push it at once (bounded by [`PUSH_BUDGET`]); what fails for
//!   lack of network stays queued and `list_sync_now` replays it (`lists.offline-queue`).
//! - `list_conflicts` holds the titles whose status changed on both sides; the UI resolves them per entry or for
//!   all (`resolve_list_conflict`).
//!
//! OAuth: `tk_core::lists::oauth` (loopback redirect on `127.0.0.1:47410..=47430`, AniList implicit grant on the
//! first port, MAL PKCE `plain`), the browser through the `open` crate, tokens as JSON in `b.secrets`
//! (`tk_core::secrets::keys::{ANILIST_TOKEN, MAL_TOKEN}`), MAL refreshed by `lists::refresh_if_needed`.
//! Error details sent to the UI are fixed codes (`no_client_id`, `port`, `browser`, `timeout`, `denied`, `state`,
//! `invalid_callback`, `exchange`, `expired`, `not_connected`, `provider:<error>`) that Settings > Accounts
//! translates (`accounts.error.<code>`).
//!
//! The last sync time of each service lives in `<data>/lists-sync.json` (not a secret, not a setting).

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use time::OffsetDateTime;
use tk_core::ListError;
use tk_core::lists::mal::{Mal, MalEntry, MalEntryInput};
use tk_core::lists::sync::{self, Decision, EntryPatch, LOCAL, RowJson};
use tk_core::lists::{self, Service, Token, import, oauth};
use tk_core::metadata::anilist::AniList;
use tk_core::metadata::anilist::lists::{EntryInput, RemoteEntry};
use tk_core::store::lists::{FavoriteRow, ListConflictRow, ListEntryRow, ListQueueRow};
use tk_ui::backend::UiResult;
use tk_ui::backend::{
    ConflictSideVm, ExportFormatVm, ListAccountVm, ListConflictVm, ListEntryUpdateVm, ListEntryVm, ListImportVm,
    ListStatusVm, ListSyncVm, MediaVm,
};
use tk_ui::model::{ErrorKind, ListServiceVm, UiError};

use crate::backend_common::{IdKind, parse_media_id};
use crate::live::LiveBackend;
use crate::settings_store;

/// How long an interactive write waits for the push before answering (the op stays queued when it runs out).
const PUSH_BUDGET: Duration = Duration::from_secs(4);
/// A queued op that keeps failing for a non-network reason is dropped after this many attempts.
const MAX_ATTEMPTS: u32 = 5;
/// Prefix of character favourites in the `favorites` table and in `Backend::{is_favorite, set_favorite}`.
pub const CHARACTER_PREFIX: &str = "character:";
/// `<data>/lists-sync.json`: `{ "anilist": <unix>, "mal": <unix> }`.
const SYNC_FILE: &str = "lists-sync.json";

const OP_UPSERT: &str = "upsert";
const OP_DELETE: &str = "delete";
const OP_FAVORITE: &str = "favorite";
const OP_UNFAVORITE: &str = "unfavorite";

fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent("TatakaiNext/0.1")
            .timeout(Duration::from_secs(20))
            .build()
            .unwrap_or_default()
    })
}

fn anilist() -> AniList {
    AniList::new(client().clone())
}

fn mal() -> Mal {
    Mal::new(client().clone())
}

fn now() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

/// `YYYY-MM-DD` (UTC) of `at`.
fn iso_date(at: OffsetDateTime) -> String {
    let d = at.date();
    format!("{:04}-{:02}-{:02}", d.year(), u8::from(d.month()), d.day())
}

fn today() -> String {
    iso_date(OffsetDateTime::now_utc())
}

fn core_service(s: ListServiceVm) -> Service {
    match s {
        ListServiceVm::AniList => Service::AniList,
        ListServiceVm::Mal => Service::Mal,
    }
}

fn vm_service(s: Service) -> ListServiceVm {
    match s {
        Service::AniList => ListServiceVm::AniList,
        Service::Mal => ListServiceVm::Mal,
    }
}

fn label(s: Service) -> &'static str {
    vm_service(s).label()
}

/// `ListError` -> `UiError` with the detail codes Settings > Accounts translates.
fn list_err(e: ListError) -> UiError {
    match e {
        ListError::NotConnected => UiError::new(ErrorKind::Auth, "not_connected"),
        ListError::Unauthorized => UiError::new(ErrorKind::Auth, "expired"),
        ListError::OAuth(code) => UiError::new(ErrorKind::Auth, code),
        ListError::RateLimited => UiError::new(ErrorKind::RateLimited, "rate limited"),
        ListError::Network(m) => UiError::new(ErrorKind::Offline, m),
        other => UiError::new(ErrorKind::Other, other.to_string()),
    }
}

fn store_err(e: impl std::fmt::Display) -> UiError {
    UiError::new(ErrorKind::Other, e.to_string())
}

/// Public profile page of an account.
fn profile_url(s: Service, username: &str) -> String {
    match s {
        Service::AniList => format!("https://anilist.co/user/{username}"),
        Service::Mal => format!("https://myanimelist.net/profile/{username}"),
    }
}

/// Whether `progress_secs` of `duration_secs` passed `progress_at` percent (no duration: the history's own
/// `completed` flag decides).
fn counts_as_watched(progress_secs: u32, duration_secs: Option<u32>, completed: bool, progress_at: u8) -> bool {
    match duration_secs {
        Some(d) if d > 0 => u64::from(progress_secs) * 100 >= u64::from(d) * u64::from(progress_at.min(100)),
        _ => completed,
    }
}

// ----- tokens -----

fn load_token(b: &LiveBackend, s: Service) -> Result<Option<Token>, ListError> {
    let Some(json) = b.secrets.get(s.secret_key())? else {
        return Ok(None);
    };
    Ok(serde_json::from_str::<Token>(&json).ok().filter(|t| t.service == s && !t.access_token.is_empty()))
}

fn save_token(b: &LiveBackend, t: &Token) -> Result<(), ListError> {
    let json = serde_json::to_string(t).map_err(|e| ListError::Parse(e.to_string()))?;
    b.secrets.set(t.service.secret_key(), &json)?;
    Ok(())
}

/// A usable token: refreshed (and stored) when MAL's is about to expire; `Unauthorized` when it is past its expiry.
async fn token(b: &LiveBackend, s: Service) -> Result<Token, ListError> {
    let t = load_token(b, s)?.ok_or(ListError::NotConnected)?;
    let at = now();
    if let Some(fresh) = lists::refresh_if_needed(client(), &t, at).await? {
        save_token(b, &fresh)?;
        return Ok(fresh);
    }
    if lists::is_expired(&t, at) {
        return Err(ListError::Unauthorized);
    }
    Ok(t)
}

/// Services with a stored token, in `Service::ALL` order.
fn connected(b: &LiveBackend) -> Vec<Service> {
    Service::ALL.into_iter().filter(|s| matches!(load_token(b, *s), Ok(Some(_)))).collect()
}

fn write_back(b: &LiveBackend) -> bool {
    settings_store::load(&b.settings_file).lists.write_back
}

// ----- last sync -----

fn sync_file(b: &LiveBackend) -> PathBuf {
    b.data_root.join(SYNC_FILE)
}

fn read_sync_times(path: &Path) -> HashMap<String, i64> {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn last_sync(b: &LiveBackend, s: Service) -> Option<i64> {
    read_sync_times(&sync_file(b)).get(s.id()).copied()
}

fn set_last_sync(b: &LiveBackend, s: Service, at: Option<i64>) {
    let path = sync_file(b);
    let mut times = read_sync_times(&path);
    match at {
        Some(at) => times.insert(s.id().to_owned(), at),
        None => times.remove(s.id()),
    };
    if let Ok(text) = serde_json::to_string(&times)
        && let Err(e) = std::fs::write(&path, text)
    {
        tracing::warn!(error = %e, "could not write the list sync times");
    }
}

// ----- row <-> view-model -----

/// Base row of an AniList entry (`media_id` = the UI id the local list uses for this title).
fn anilist_row(e: &RemoteEntry, media_id: &str) -> ListEntryRow {
    ListEntryRow {
        service: Service::AniList.id().to_owned(),
        media_id: media_id.to_owned(),
        anilist_id: e.media.media_ref.anilist_id,
        mal_id: e.media.media_ref.mal_id,
        title: e.media.title.clone(),
        poster: e.media.cover.clone(),
        status: lists::anilist_status_to_shared(&e.status).unwrap_or("planning").to_owned(),
        progress: e.progress,
        episodes: e.media.episodes,
        score: e.score_100.filter(|s| *s > 0),
        started_at: e.started_at.clone(),
        finished_at: e.completed_at.clone(),
        repeats: e.repeats,
        notes: e.notes.clone(),
        remote_id: Some(e.id),
        updated_at: e.updated_at,
        dirty: false,
    }
}

/// Base row of a MAL entry.
fn mal_row(e: &MalEntry, media_id: &str, anilist_id: Option<u32>) -> ListEntryRow {
    ListEntryRow {
        service: Service::Mal.id().to_owned(),
        media_id: media_id.to_owned(),
        anilist_id,
        mal_id: Some(e.mal_id),
        title: e.title.clone(),
        poster: e.poster.clone(),
        status: lists::mal_status_to_shared(&e.status, e.is_rewatching).unwrap_or("planning").to_owned(),
        progress: e.progress,
        episodes: e.episodes,
        score: lists::score_from_mal(e.score),
        started_at: e.start_date.clone(),
        finished_at: e.finish_date.clone(),
        repeats: e.num_times_rewatched,
        notes: e.comments.clone(),
        remote_id: Some(u64::from(e.mal_id)),
        updated_at: tk_core::lists::mal::updated_unix(&e.updated_at),
        dirty: false,
    }
}

/// Fill what a service answer does not carry (title, poster, episode count, the other service's id) from `from`.
fn complete_from(row: &mut ListEntryRow, from: &ListEntryRow) {
    if row.title.is_empty() {
        row.title = from.title.clone();
    }
    if row.poster.is_none() {
        row.poster = from.poster.clone();
    }
    if row.episodes.is_none() {
        row.episodes = from.episodes;
    }
    if row.anilist_id.is_none() {
        row.anilist_id = from.anilist_id;
    }
    if row.mal_id.is_none() {
        row.mal_id = from.mal_id;
    }
}

fn media_vm(row: &ListEntryRow) -> MediaVm {
    let subtitle = row.episodes.map(|e| format!("{e} ep")).unwrap_or_default();
    MediaVm {
        id: row.media_id.clone(),
        title: if row.title.is_empty() { row.media_id.clone() } else { row.title.clone() },
        cover: row.poster.clone(),
        color: None,
        subtitle,
        score: None,
    }
}

fn entry_vm(row: &ListEntryRow, service: ListServiceVm, pending: bool) -> ListEntryVm {
    ListEntryVm {
        media: media_vm(row),
        service,
        status: ListStatusVm::from_id(&row.status).unwrap_or(ListStatusVm::Planning),
        progress: row.progress,
        episodes_total: row.episodes,
        score: row.score.filter(|s| *s > 0),
        started: row.started_at.clone(),
        finished: row.finished_at.clone(),
        repeats: row.repeats,
        notes: row.notes.clone(),
        updated_unix: row.updated_at,
        remote_id: row.remote_id,
        pending,
    }
}

/// The service the UI labels an entry with: `Settings::lists.primary` when connected, else the first connected one,
/// else AniList (local-only lists).
fn display_service(b: &LiveBackend, connected: &[Service]) -> ListServiceVm {
    let primary = settings_store::load(&b.settings_file).lists.primary.map(core_service);
    let s = primary
        .filter(|p| connected.contains(p))
        .or_else(|| connected.first().copied())
        .unwrap_or(Service::AniList);
    vm_service(s)
}

fn is_pending(b: &LiveBackend, row: &ListEntryRow) -> bool {
    row.dirty || Service::ALL.into_iter().any(|s| b.store.has_list_ops(s.id(), &row.media_id).unwrap_or(false))
}

fn favorite_vm(f: &FavoriteRow) -> MediaVm {
    MediaVm {
        id: f.media_id.clone(),
        title: if f.title.is_empty() { f.media_id.clone() } else { f.title.clone() },
        cover: f.poster.clone(),
        color: None,
        subtitle: String::new(),
        score: None,
    }
}

/// A new local row for `media_id` with what the app knows about the title (titles seen this session, else the
/// metadata router, best effort).
async fn blank_row(b: &LiveBackend, media_id: &str) -> UiResult<ListEntryRow> {
    let (kind, n) =
        parse_media_id(media_id).ok_or_else(|| UiError::new(ErrorKind::Other, format!("invalid id {media_id}")))?;
    let (mut anilist_id, mut mal_id) = match kind {
        IdKind::AniList => (Some(n), None),
        IdKind::Mal => (None, Some(n)),
        IdKind::Kitsu => (None, None),
    };
    let mut title = String::new();
    let mut poster = None;
    let mut episodes = None;
    if let Ok(Ok((r, m))) = tokio::time::timeout(PUSH_BUDGET, b.media(media_id)).await {
        title = m.title.clone();
        poster = m.cover.clone();
        episodes = m.episodes;
        anilist_id = anilist_id.or(r.anilist_id).or(m.media_ref.anilist_id);
        mal_id = mal_id.or(r.mal_id).or(m.media_ref.mal_id);
    } else if let Some((t, p)) = b.titles.lock().ok().and_then(|t| t.get(media_id).cloned()) {
        title = t;
        poster = p;
    }
    Ok(ListEntryRow {
        service: LOCAL.to_owned(),
        media_id: media_id.to_owned(),
        anilist_id,
        mal_id,
        title,
        poster,
        status: String::new(),
        progress: 0,
        episodes,
        score: None,
        started_at: None,
        finished_at: None,
        repeats: 0,
        notes: None,
        remote_id: None,
        updated_at: 0,
        dirty: false,
    })
}

// ----- queue -----

/// Queue a push of the current local `row` to every connected service that can address it (except `skip`).
fn queue_upsert(b: &LiveBackend, row: &ListEntryRow, skip: Option<Service>) -> UiResult<()> {
    if !write_back(b) {
        return Ok(());
    }
    for s in connected(b) {
        if Some(s) == skip || !sync::addressable(row, s) {
            continue;
        }
        queue_upsert_on(b, s, row)?;
    }
    Ok(())
}

fn queue_upsert_on(b: &LiveBackend, s: Service, row: &ListEntryRow) -> UiResult<()> {
    b.store.dequeue_list_ops_for(s.id(), &row.media_id, &[OP_UPSERT, OP_DELETE]).map_err(store_err)?;
    b.store.enqueue_list_op(s.id(), &row.media_id, OP_UPSERT, &RowJson::to_json(row), now()).map_err(store_err)?;
    Ok(())
}

/// Queue a remote delete on `s` (payload = the base row: it carries the AniList entry id / the MAL id).
fn queue_delete_on(b: &LiveBackend, s: Service, payload: &ListEntryRow) -> UiResult<()> {
    b.store.dequeue_list_ops_for(s.id(), &payload.media_id, &[OP_UPSERT, OP_DELETE]).map_err(store_err)?;
    b.store.enqueue_list_op(s.id(), &payload.media_id, OP_DELETE, &RowJson::to_json(payload), now()).map_err(store_err)?;
    Ok(())
}

#[derive(Debug, Default)]
struct Flush {
    pushed: u32,
    errors: Vec<String>,
}

/// Replay the queue (oldest first) of every connected service; `only` limits it to one title. A network, rate
/// limit or token failure stops that service (the rest stays queued); other failures count an attempt.
async fn flush(b: &LiveBackend, only: Option<&str>) -> Flush {
    let mut out = Flush::default();
    let mut touched = BTreeSet::new();
    for s in connected(b) {
        let ops = match b.store.list_queue(Some(s.id())) {
            Ok(ops) => ops,
            Err(e) => {
                out.errors.push(format!("{}: {e}", label(s)));
                continue;
            }
        };
        let ops: Vec<ListQueueRow> = ops.into_iter().filter(|o| only.is_none_or(|m| m == o.media_id)).collect();
        if ops.is_empty() {
            continue;
        }
        let tok = match token(b, s).await {
            Ok(t) => t,
            Err(e) => {
                out.errors.push(format!("{}: {}", label(s), list_err(e).detail));
                continue;
            }
        };
        for op in ops {
            match push_op(b, s, &tok, &op).await {
                Ok(()) | Err(ListError::Unsupported) => {
                    let _ = b.store.dequeue_list_op(op.id);
                    out.pushed += 1;
                    touched.insert(op.media_id.clone());
                }
                Err(e @ (ListError::Network(_) | ListError::RateLimited | ListError::Unauthorized)) => {
                    let msg = e.to_string();
                    let _ = b.store.mark_list_op_failed(op.id, &msg);
                    out.errors.push(format!("{}: {msg}", label(s)));
                    break;
                }
                Err(e) => {
                    let msg = e.to_string();
                    if op.attempts + 1 >= MAX_ATTEMPTS {
                        let _ = b.store.dequeue_list_op(op.id);
                    } else {
                        let _ = b.store.mark_list_op_failed(op.id, &msg);
                    }
                    out.errors.push(format!("{}: {msg}", label(s)));
                }
            }
        }
    }
    // A local row is clean once no service has anything queued for it.
    for media_id in touched {
        if let Ok(Some(mut row)) = b.store.list_entry(LOCAL, &media_id)
            && row.dirty
            && !Service::ALL.into_iter().any(|s| b.store.has_list_ops(s.id(), &media_id).unwrap_or(true))
        {
            row.dirty = false;
            let _ = b.store.put_list_entry(&row);
        }
    }
    out
}

/// `flush(Some(media_id))` bounded by [`PUSH_BUDGET`] (interactive writes answer fast; the queue keeps the rest).
async fn push_soon(b: &LiveBackend, media_id: &str) {
    if let Ok(f) = tokio::time::timeout(PUSH_BUDGET, flush(b, Some(media_id))).await
        && !f.errors.is_empty()
    {
        tracing::info!(errors = ?f.errors, "list write stays queued");
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct FavoritePayload {
    id: u32,
    character: bool,
}

async fn push_op(b: &LiveBackend, s: Service, tok: &Token, op: &ListQueueRow) -> Result<(), ListError> {
    match op.op.as_str() {
        OP_UPSERT => {
            // The push always sends the current local row (a newer write replaced the payload's state).
            let Some(row) = b.store.list_entry(LOCAL, &op.media_id)? else {
                return Ok(());
            };
            let mut base = match s {
                Service::AniList => {
                    let id = row.anilist_id.ok_or(ListError::Unsupported)?;
                    let input = EntryInput {
                        status: lists::shared_status_to_anilist(&row.status).map(str::to_owned),
                        progress: Some(row.progress),
                        score_100: Some(row.score.unwrap_or(0)),
                        notes: Some(row.notes.clone().unwrap_or_default()),
                    };
                    let saved = anilist().save_entry(&tok.access_token, id, &input).await?;
                    anilist_row(&saved, &row.media_id)
                }
                Service::Mal => {
                    let id = row.mal_id.ok_or(ListError::Unsupported)?;
                    let (status, rewatching) = lists::shared_status_to_mal(&row.status).unwrap_or(("plan_to_watch", false));
                    let input = MalEntryInput {
                        status: Some(status.to_owned()),
                        is_rewatching: Some(rewatching),
                        progress: Some(row.progress),
                        score: Some(lists::score_to_mal(row.score)),
                        comments: Some(row.notes.clone().unwrap_or_default()),
                    };
                    let saved = mal().update(tok, id, &input).await?;
                    mal_row(&saved, &row.media_id, row.anilist_id)
                }
            };
            complete_from(&mut base, &row);
            if base.updated_at == 0 {
                base.updated_at = now();
            }
            b.store.put_list_entry(&base)?;
            Ok(())
        }
        OP_DELETE => {
            let base = b.store.list_entry(s.id(), &op.media_id)?;
            let payload = RowJson::parse(&op.payload, s.id());
            let known = base.as_ref().or(payload.as_ref());
            match s {
                Service::AniList => {
                    if let Some(entry_id) = known.and_then(|r| r.remote_id) {
                        anilist().delete_entry(&tok.access_token, entry_id).await?;
                    }
                }
                Service::Mal => {
                    if let Some(mal_id) = known.and_then(|r| r.mal_id) {
                        mal().delete(tok, mal_id).await?;
                    }
                }
            }
            b.store.delete_list_entry(s.id(), &op.media_id)?;
            Ok(())
        }
        OP_FAVORITE | OP_UNFAVORITE => {
            // MAL's API v2 has no favourites endpoint: only AniList ops are ever queued.
            if s != Service::AniList {
                return Err(ListError::Unsupported);
            }
            let p: FavoritePayload = serde_json::from_str(&op.payload).map_err(|e| ListError::Parse(e.to_string()))?;
            anilist().set_favourite(&tok.access_token, p.id, p.character, op.op == OP_FAVORITE).await
        }
        _ => Err(ListError::Unsupported),
    }
}

// ----- local writes -----

/// Apply `patch` to the local entry (created when missing), queue it for every connected service and try to push it.
async fn write_patch(b: &LiveBackend, media_id: &str, patch: &EntryPatch) -> UiResult<ListEntryRow> {
    let mut row = match b.store.list_entry(LOCAL, media_id).map_err(store_err)? {
        Some(r) => r,
        None => blank_row(b, media_id).await?,
    };
    sync::apply_patch(&mut row, patch, &today(), now());
    b.store.put_list_entry(&row).map_err(store_err)?;
    queue_upsert(b, &row, None)?;
    push_soon(b, media_id).await;
    Ok(b.store.list_entry(LOCAL, media_id).map_err(store_err)?.unwrap_or(row))
}

fn patch_of(update: &ListEntryUpdateVm) -> EntryPatch {
    EntryPatch {
        status: update.status.map(|s| s.id().to_owned()),
        progress: update.progress,
        score: update.score,
        notes: update.notes.clone(),
    }
}

// ----- pull -----

/// The service's whole list as base rows keyed by the local media id.
async fn fetch_remote(b: &LiveBackend, s: Service, tok: &mut Token) -> Result<HashMap<String, ListEntryRow>, ListError> {
    let mut out = HashMap::new();
    match s {
        Service::AniList => {
            let user_id = match tok.user_id {
                Some(id) => id,
                None => {
                    let v = anilist().viewer(&tok.access_token).await?;
                    tok.user_id = Some(v.id);
                    tok.username = Some(v.name);
                    tok.avatar = v.avatar;
                    save_token(b, tok)?;
                    v.id
                }
            };
            for e in anilist().media_list(&tok.access_token, user_id).await? {
                if let Some(id) = sync::media_id_for(e.media.media_ref.anilist_id, e.media.media_ref.mal_id) {
                    out.insert(id.clone(), anilist_row(&e, &id));
                }
            }
        }
        Service::Mal => {
            let entries = mal().anime_list(tok).await?;
            // Local titles already known by their MAL id keep their media id.
            let mut by_mal: HashMap<u32, (String, Option<u32>)> = HashMap::new();
            for svc in [LOCAL, Service::AniList.id(), Service::Mal.id()] {
                for r in b.store.list_entries(svc, None)? {
                    if let Some(m) = r.mal_id {
                        by_mal.entry(m).or_insert((r.media_id.clone(), r.anilist_id));
                    }
                }
            }
            let unknown: Vec<u32> = entries.iter().map(|e| e.mal_id).filter(|m| !by_mal.contains_key(m)).collect();
            let mapped = if unknown.is_empty() { HashMap::new() } else { anilist().ids_for_mal(&unknown).await.unwrap_or_default() };
            for e in entries {
                let (media_id, anilist_id) = match by_mal.get(&e.mal_id) {
                    Some((id, a)) => (id.clone(), *a),
                    None => match mapped.get(&e.mal_id) {
                        Some(a) => (a.to_string(), Some(*a)),
                        None => (format!("mal:{}", e.mal_id), None),
                    },
                };
                out.insert(media_id.clone(), mal_row(&e, &media_id, anilist_id));
            }
        }
    }
    Ok(out)
}

#[derive(Debug, Default)]
struct PullReport {
    pulled: u32,
    conflicts: u32,
}

/// Three-way merge of one service into the local list (`tk_core::lists::sync::decide`).
async fn pull(b: &LiveBackend, s: Service, tok: &mut Token) -> Result<PullReport, ListError> {
    let remote = fetch_remote(b, s, tok).await?;
    let local: HashMap<String, ListEntryRow> =
        b.store.list_entries(LOCAL, None)?.into_iter().map(|r| (r.media_id.clone(), r)).collect();
    let base: HashMap<String, ListEntryRow> =
        b.store.list_entries(s.id(), None)?.into_iter().map(|r| (r.media_id.clone(), r)).collect();
    let keys: BTreeSet<&String> = local.keys().chain(base.keys()).chain(remote.keys()).collect();
    let mut report = PullReport::default();
    let at = now();
    for id in keys {
        let (l, bs, r) = (local.get(id), base.get(id), remote.get(id));
        // A local title this service cannot address (e.g. Kitsu-only, or MAL without a MAL id) is not its business.
        if r.is_none() && bs.is_none() && l.is_some_and(|l| !sync::addressable(l, s)) {
            continue;
        }
        let pending = b.store.has_list_ops(s.id(), id)? || b.store.has_list_conflict(s.id(), id)?;
        match sync::decide(l, bs, r, pending, s) {
            Decision::Nothing => {
                if !pending {
                    match r {
                        Some(r) => b.store.put_list_entry(r)?,
                        None if bs.is_some() => b.store.delete_list_entry(s.id(), id)?,
                        None => {}
                    }
                }
            }
            Decision::TakeRemote => {
                if let Some(r) = r {
                    let merged = sync::merged_local(l, r);
                    b.store.put_list_entry(&merged)?;
                    b.store.put_list_entry(r)?;
                    report.pulled += 1;
                }
            }
            Decision::PushLocal => {
                if let Some(l) = l
                    && write_back(b)
                {
                    queue_upsert_on(b, s, l).map_err(|e| ListError::Parse(e.detail))?;
                }
            }
            Decision::DeleteLocal => {
                b.store.delete_list_entry(LOCAL, id)?;
                b.store.delete_list_entry(s.id(), id)?;
                report.pulled += 1;
            }
            Decision::DeleteRemote => {
                if let Some(bs) = bs
                    && write_back(b)
                {
                    queue_delete_on(b, s, bs).map_err(|e| ListError::Parse(e.detail))?;
                }
            }
            Decision::Conflict => {
                if let (Some(l), Some(r)) = (l, r) {
                    b.store.add_list_conflict(&ListConflictRow {
                        id: 0,
                        service: s.id().to_owned(),
                        media_id: id.clone(),
                        local_json: RowJson::to_json(l),
                        remote_json: RowJson::to_json(r),
                        detected_at: at,
                    })?;
                    report.conflicts += 1;
                }
            }
        }
    }
    if s == Service::AniList {
        // Favourites made on AniList show up locally too (never removes local ones: they may still be queued).
        for m in anilist().favourite_anime(&tok.access_token).await.unwrap_or_default() {
            if let Some(id) = sync::media_id_for(m.media_ref.anilist_id, m.media_ref.mal_id)
                && !b.store.is_favorite(&id)?
            {
                b.store.set_favorite(&FavoriteRow { media_id: id, title: m.title.clone(), poster: m.cover.clone(), added_at: at })?;
            }
        }
    }
    Ok(report)
}

// ----- Backend -----

fn account_vm(b: &LiveBackend, s: Service) -> ListAccountVm {
    let tok = load_token(b, s).ok().flatten();
    ListAccountVm {
        service: vm_service(s),
        connected: tok.is_some(),
        username: tok.as_ref().and_then(|t| t.username.clone()),
        avatar: tok.as_ref().and_then(|t| t.avatar.clone()),
        profile_url: tok.as_ref().and_then(|t| t.username.as_deref()).map(|u| profile_url(s, u)),
        pending_changes: if tok.is_some() { b.store.count_list_queue(s.id()).unwrap_or(0) } else { 0 },
        last_sync_unix: tok.as_ref().and_then(|_| last_sync(b, s)),
    }
}

/// `Backend::list_accounts`.
pub async fn list_accounts(b: &LiveBackend) -> UiResult<Vec<ListAccountVm>> {
    Ok(Service::ALL.into_iter().map(|s| account_vm(b, s)).collect())
}

fn open_browser(url: &str) -> Result<(), ListError> {
    open::that_detached(url).map_err(|_| ListError::OAuth("browser".into()))
}

async fn connect(b: &LiveBackend, s: Service) -> Result<(), ListError> {
    let client_id = lists::client_id(s).ok_or_else(|| ListError::OAuth("no_client_id".into()))?;
    let state = oauth::new_state();
    let mut tok = match s {
        Service::AniList => {
            // AniList accepts one registered redirect: the first loopback port.
            let lb = oauth::bind_loopback(true).await?;
            open_browser(&oauth::anilist_authorize_url(&client_id, &state))?;
            let cb = lb.accept(&state).await?;
            oauth::anilist_token(&cb.code_or_token, now())
        }
        Service::Mal => {
            let lb = oauth::bind_loopback(false).await?;
            let verifier = oauth::pkce_verifier();
            let redirect = lb.redirect_uri.clone();
            open_browser(&oauth::mal_authorize_url(&client_id, &verifier, &state, &redirect))?;
            let cb = lb.accept(&state).await?;
            oauth::mal_exchange_code(client(), &client_id, &cb.code_or_token, &verifier, &redirect, now()).await?
        }
    };
    match s {
        Service::AniList => {
            let v = anilist().viewer(&tok.access_token).await?;
            tok.user_id = Some(v.id);
            tok.username = Some(v.name);
            tok.avatar = v.avatar;
        }
        Service::Mal => {
            let u = mal().me(&tok).await?;
            tok.user_id = Some(u.id);
            tok.username = Some(u.name);
            tok.avatar = u.picture;
        }
    }
    save_token(b, &tok)
}

/// `Backend::list_connect`: OAuth through the system browser and the loopback redirect. The first sync (import /
/// export / merge, Settings > Accounts) is the UI's next step.
pub async fn list_connect(b: &LiveBackend, service: ListServiceVm) -> UiResult<ListAccountVm> {
    let s = core_service(service);
    connect(b, s).await.map_err(list_err)?;
    Ok(account_vm(b, s))
}

/// `Backend::list_disconnect`: forget the token, the service's base rows, its queue and conflicts, and the local
/// entries that only mirrored this account (the remote list stops showing; never-synced local entries stay).
pub async fn list_disconnect(b: &LiveBackend, service: ListServiceVm) -> UiResult<()> {
    let s = core_service(service);
    let others: Vec<Service> = connected(b).into_iter().filter(|o| *o != s).collect();
    for base in b.store.list_entries(s.id(), None).map_err(store_err)? {
        let Some(local) = b.store.list_entry(LOCAL, &base.media_id).map_err(store_err)? else {
            continue;
        };
        let elsewhere = others.iter().any(|o| b.store.list_entry(o.id(), &local.media_id).ok().flatten().is_some());
        if !elsewhere && !local.dirty {
            b.store.delete_list_entry(LOCAL, &local.media_id).map_err(store_err)?;
        }
    }
    b.store.delete_list_entries(s.id()).map_err(store_err)?;
    b.store.clear_list_queue(s.id()).map_err(store_err)?;
    b.store.clear_list_conflicts(s.id()).map_err(store_err)?;
    b.secrets.delete(s.secret_key()).map_err(store_err)?;
    set_last_sync(b, s, None);
    Ok(())
}

/// `Backend::list_entry`.
pub async fn list_entry(b: &LiveBackend, media_id: &str) -> UiResult<Option<ListEntryVm>> {
    let Some(row) = b.store.list_entry(LOCAL, media_id).map_err(store_err)? else {
        return Ok(None);
    };
    let service = display_service(b, &connected(b));
    Ok(Some(entry_vm(&row, service, is_pending(b, &row))))
}

/// `Backend::set_list_entry`.
pub async fn set_list_entry(b: &LiveBackend, media_id: &str, update: &ListEntryUpdateVm) -> UiResult<ListEntryVm> {
    let row = write_patch(b, media_id, &patch_of(update)).await?;
    let service = display_service(b, &connected(b));
    Ok(entry_vm(&row, service, is_pending(b, &row)))
}

/// `Backend::remove_list_entry`.
pub async fn remove_list_entry(b: &LiveBackend, media_id: &str) -> UiResult<()> {
    let Some(local) = b.store.list_entry(LOCAL, media_id).map_err(store_err)? else {
        return Ok(());
    };
    b.store.delete_list_entry(LOCAL, media_id).map_err(store_err)?;
    b.store.delete_list_conflicts_for(None, media_id).map_err(store_err)?;
    let wb = write_back(b);
    for s in connected(b) {
        let base = b.store.list_entry(s.id(), media_id).map_err(store_err)?;
        if !wb {
            b.store.dequeue_list_ops_for(s.id(), media_id, &[OP_UPSERT, OP_DELETE]).map_err(store_err)?;
            continue;
        }
        match base {
            Some(base) => queue_delete_on(b, s, &base)?,
            // Never synced: MAL can still delete by id; an AniList entry id is unknown, so nothing is there to delete.
            None if s == Service::Mal && local.mal_id.is_some() => queue_delete_on(b, s, &local)?,
            None => {
                b.store.dequeue_list_ops_for(s.id(), media_id, &[OP_UPSERT, OP_DELETE]).map_err(store_err)?;
            }
        }
    }
    push_soon(b, media_id).await;
    Ok(())
}

/// `Backend::list_entries`.
pub async fn list_entries(b: &LiveBackend, status: Option<ListStatusVm>) -> UiResult<Vec<ListEntryVm>> {
    let rows = b.store.list_entries(LOCAL, status.map(ListStatusVm::id)).map_err(store_err)?;
    let service = display_service(b, &connected(b));
    Ok(rows.iter().map(|r| entry_vm(r, service, is_pending(b, r))).collect())
}

/// `Backend::favorites` (titles and `character:<id>` rows, newest first).
pub async fn favorites(b: &LiveBackend) -> UiResult<Vec<MediaVm>> {
    Ok(b.store.favorites().map_err(store_err)?.iter().map(favorite_vm).collect())
}

/// `Backend::is_favorite` (also `character:<id>`).
pub async fn is_favorite(b: &LiveBackend, media_id: &str) -> UiResult<bool> {
    b.store.is_favorite(media_id).map_err(store_err)
}

/// What AniList calls the favourite: `(id, character?)`; `None` = not addressable on AniList.
fn anilist_favorite_target(b: &LiveBackend, media_id: &str) -> Option<(u32, bool)> {
    if let Some(n) = media_id.strip_prefix(CHARACTER_PREFIX) {
        return n.trim().parse::<u32>().ok().filter(|n| *n > 0).map(|n| (n, true));
    }
    match parse_media_id(media_id)? {
        (IdKind::AniList, n) => Some((n, false)),
        _ => b.store.list_entry(LOCAL, media_id).ok().flatten().and_then(|r| r.anilist_id).map(|n| (n, false)),
    }
}

/// Title and poster of a new favourite.
async fn favorite_card(b: &LiveBackend, media_id: &str) -> (String, Option<String>) {
    if let Some(n) = media_id.strip_prefix(CHARACTER_PREFIX).and_then(|n| n.parse::<u32>().ok()) {
        if let Ok(Ok(Some(card))) = tokio::time::timeout(PUSH_BUDGET, anilist().character_card(n)).await {
            return card;
        }
        return (String::new(), None);
    }
    if let Some(card) = b.titles.lock().ok().and_then(|t| t.get(media_id).cloned()) {
        return card;
    }
    match tokio::time::timeout(PUSH_BUDGET, b.media(media_id)).await {
        Ok(Ok((_, m))) => (m.title.clone(), m.cover.clone()),
        _ => (String::new(), None),
    }
}

/// `Backend::set_favorite`: local first; with an AniList account, `ToggleFavourite` through the queue.
pub async fn set_favorite(b: &LiveBackend, media_id: &str, favorite: bool) -> UiResult<()> {
    if favorite {
        let (title, poster) = favorite_card(b, media_id).await;
        b.store
            .set_favorite(&FavoriteRow { media_id: media_id.to_owned(), title, poster, added_at: now() })
            .map_err(store_err)?;
    } else {
        b.store.remove_favorite(media_id).map_err(store_err)?;
    }
    let s = Service::AniList;
    if write_back(b)
        && connected(b).contains(&s)
        && let Some((id, character)) = anilist_favorite_target(b, media_id)
    {
        b.store.dequeue_list_ops_for(s.id(), media_id, &[OP_FAVORITE, OP_UNFAVORITE]).map_err(store_err)?;
        let payload = serde_json::to_string(&FavoritePayload { id, character }).map_err(store_err)?;
        let op = if favorite { OP_FAVORITE } else { OP_UNFAVORITE };
        b.store.enqueue_list_op(s.id(), media_id, op, &payload, now()).map_err(store_err)?;
        push_soon(b, media_id).await;
    }
    Ok(())
}

/// `Backend::list_sync_now`: push the queue, pull every connected service (three-way merge), push what the merge
/// queued. Per-service failures are reported in `errors` (the rest still syncs).
pub async fn list_sync_now(b: &LiveBackend) -> UiResult<ListSyncVm> {
    let services = connected(b);
    if services.is_empty() {
        return Err(UiError::new(ErrorKind::Auth, "not_connected"));
    }
    let mut out = ListSyncVm::default();
    let first = flush(b, None).await;
    out.pushed += first.pushed;
    out.errors.extend(first.errors);
    let mut any_ok = false;
    let mut last_err = None;
    for s in services {
        let mut tok = match token(b, s).await {
            Ok(t) => t,
            Err(e) => {
                let ui = list_err(e);
                out.errors.push(format!("{}: {}", label(s), ui.detail));
                last_err = Some(ui);
                continue;
            }
        };
        match pull(b, s, &mut tok).await {
            Ok(r) => {
                out.pulled += r.pulled;
                out.conflicts += r.conflicts;
                set_last_sync(b, s, Some(now()));
                any_ok = true;
            }
            Err(e) => {
                let ui = list_err(e);
                out.errors.push(format!("{}: {}", label(s), ui.detail));
                last_err = Some(ui);
            }
        }
    }
    let second = flush(b, None).await;
    out.pushed += second.pushed;
    for e in second.errors {
        if !out.errors.contains(&e) {
            out.errors.push(e);
        }
    }
    // Nothing reached any service (offline, every token refused): that is an error, not a summary.
    match (any_ok, last_err) {
        (false, Some(e)) => Err(e),
        _ => Ok(out),
    }
}

fn conflict_vm(b: &LiveBackend, c: &ListConflictRow) -> Option<ListConflictVm> {
    let s = Service::from_id(&c.service)?;
    let local = RowJson::parse(&c.local_json, LOCAL)?;
    let mut remote = RowJson::parse(&c.remote_json, s.id())?;
    complete_from(&mut remote, &local);
    let _ = b;
    Some(ListConflictVm {
        id: c.id,
        local: entry_vm(&local, vm_service(s), true),
        remote: entry_vm(&remote, vm_service(s), false),
        detected_unix: c.detected_at,
    })
}

/// `Backend::list_conflicts`.
pub async fn list_conflicts(b: &LiveBackend) -> UiResult<Vec<ListConflictVm>> {
    Ok(b.store.list_conflicts().map_err(store_err)?.iter().filter_map(|c| conflict_vm(b, c)).collect())
}

/// `Backend::resolve_list_conflict`: `Local` pushes the local entry to that service; `Remote` copies the service's
/// entry into the local list (and on to the other connected service).
pub async fn resolve_list_conflict(b: &LiveBackend, conflict_id: i64, keep: ConflictSideVm) -> UiResult<()> {
    let Some(c) = b.store.list_conflict(conflict_id).map_err(store_err)? else {
        return Ok(());
    };
    let Some(s) = Service::from_id(&c.service) else {
        b.store.delete_list_conflict(conflict_id).map_err(store_err)?;
        return Ok(());
    };
    let local = b.store.list_entry(LOCAL, &c.media_id).map_err(store_err)?;
    b.store.delete_list_conflict(conflict_id).map_err(store_err)?;
    match keep {
        ConflictSideVm::Local => match &local {
            Some(l) if write_back(b) => queue_upsert_on(b, s, l)?,
            Some(_) => {}
            None => {
                if let Some(base) = b.store.list_entry(s.id(), &c.media_id).map_err(store_err)? {
                    queue_delete_on(b, s, &base)?;
                }
            }
        },
        ConflictSideVm::Remote => {
            if let Some(mut r) = RowJson::parse(&c.remote_json, s.id()) {
                if let Some(l) = &local {
                    complete_from(&mut r, l);
                }
                let merged = sync::merged_local(local.as_ref(), &r);
                b.store.put_list_entry(&merged).map_err(store_err)?;
                b.store.put_list_entry(&r).map_err(store_err)?;
                queue_upsert(b, &merged, Some(s))?;
            }
        }
    }
    push_soon(b, &c.media_id).await;
    Ok(())
}

/// `Backend::import_list`: MAL XML export, AniList JSON (API answer or export) or a Tatakai list JSON into the local
/// list; MAL-only titles are mapped to AniList ids when AniList answers. Every imported entry is queued for the
/// connected services (pushed by the next sync); a local entry newer than the file's is kept.
pub async fn import_list(b: &LiveBackend, path: &str) -> UiResult<ListImportVm> {
    let text = tokio::fs::read_to_string(path.trim()).await.map_err(store_err)?;
    let (_format, imported) = import::parse(&text).map_err(|e| UiError::new(ErrorKind::Other, e))?;
    let mal_only: Vec<u32> = imported.entries.iter().filter(|e| e.anilist_id.is_none()).filter_map(|e| e.mal_id).collect();
    let mapped = if mal_only.is_empty() {
        HashMap::new()
    } else {
        match tokio::time::timeout(Duration::from_secs(30), anilist().ids_for_mal(&mal_only)).await {
            Ok(Ok(m)) => m,
            _ => HashMap::new(),
        }
    };
    let at = now();
    let mut out = ListImportVm { imported: 0, skipped: imported.skipped, errors: imported.errors.clone() };
    for mut e in imported.entries {
        if e.anilist_id.is_none()
            && let Some(a) = e.mal_id.and_then(|m| mapped.get(&m).copied())
        {
            e.anilist_id = Some(a);
            e.media_id = a.to_string();
        }
        let existing = b.store.list_entry(LOCAL, &e.media_id).map_err(store_err)?;
        if let Some(x) = &existing
            && e.updated_at > 0
            && x.updated_at > e.updated_at
        {
            out.skipped += 1;
            continue;
        }
        if e.updated_at == 0 {
            e.updated_at = at;
        }
        e.remote_id = None;
        let mut row = e.into_row(LOCAL, true);
        if let Some(x) = &existing {
            complete_from(&mut row, x);
        }
        b.store.put_list_entry(&row).map_err(store_err)?;
        queue_upsert(b, &row, None)?;
        out.imported += 1;
    }
    for f in imported.favorites {
        if !f.media_id.is_empty() {
            let mut row = f.into_row();
            if row.added_at == 0 {
                row.added_at = at;
            }
            b.store.set_favorite(&row).map_err(store_err)?;
        }
    }
    Ok(out)
}

/// Folder of exports: the user's Downloads folder, else the data directory.
fn export_dir(b: &LiveBackend) -> PathBuf {
    directories::UserDirs::new()
        .and_then(|d| d.download_dir().map(Path::to_path_buf))
        .unwrap_or_else(|| b.data_root.clone())
}

/// `Backend::export_list`: `<Downloads>/tatakai-lists-<date>.json | .csv` (JSON re-imports with `import_list`).
pub async fn export_list(b: &LiveBackend, format: ExportFormatVm) -> UiResult<PathBuf> {
    let entries = b.store.list_entries(LOCAL, None).map_err(store_err)?;
    let text = match format {
        ExportFormatVm::Json => {
            let favorites = b.store.favorites().map_err(store_err)?;
            import::export_json(&entries, &favorites, now())
        }
        ExportFormatVm::Csv => import::export_csv(&entries),
    };
    let dir = export_dir(b);
    tokio::fs::create_dir_all(&dir).await.map_err(store_err)?;
    let file = dir.join(format!("tatakai-lists-{}.{}", today(), format.id()));
    tokio::fs::write(&file, text).await.map_err(store_err)?;
    Ok(file)
}

/// Hook called by `LiveBackend::record_progress` after the local history write (never fails the write): when
/// `Settings::lists.auto_progress` is on and the episode passed `progress_at` percent, the list progress moves to
/// that episode (`lists.auto-progress`); a title not in the list is added as Watching (`lists.auto-add`), the last
/// episode completes it (`tk_core::lists::sync::auto_progress`).
pub async fn on_progress(b: &LiveBackend, id: &str, episode: u32, progress_secs: u32, duration_secs: Option<u32>, completed: bool) {
    let prefs = settings_store::load(&b.settings_file).lists;
    if !prefs.auto_progress || !counts_as_watched(progress_secs, duration_secs, completed, prefs.progress_at) {
        return;
    }
    let current = b.store.list_entry(LOCAL, id).ok().flatten();
    // Cheap exit before any network: already at (or past) this episode, or completed.
    if current.as_ref().is_some_and(|c| episode <= c.progress || c.status == "completed") {
        return;
    }
    let total = match &current {
        Some(c) if c.episodes.is_some() => c.episodes,
        _ => match tokio::time::timeout(PUSH_BUDGET, b.media(id)).await {
            Ok(Ok((_, m))) => m.episodes,
            _ => None,
        },
    };
    let Some(patch) = sync::auto_progress(current.as_ref().map(|c| (c.status.as_str(), c.progress)), episode, total) else {
        return;
    };
    if let Err(e) = write_patch(b, id, &patch).await {
        tracing::warn!(detail = %e.detail, "automatic list progress failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tk_core::metadata::mapping::map_anilist_media;

    #[test]
    fn watched_threshold() {
        assert!(counts_as_watched(90, Some(100), false, 90));
        assert!(!counts_as_watched(89, Some(100), false, 90));
        assert!(counts_as_watched(1300, Some(1440), false, 90));
        assert!(counts_as_watched(0, None, true, 90));
        assert!(!counts_as_watched(5000, None, false, 90));
        assert!(!counts_as_watched(5000, Some(0), false, 50));
        assert!(counts_as_watched(100, Some(100), false, 255), "progress_at is capped at 100");
    }

    #[test]
    fn iso_dates_are_zero_padded() {
        let at = OffsetDateTime::from_unix_timestamp(1_704_153_600).unwrap(); // 2024-01-02
        assert_eq!(iso_date(at), "2024-01-02");
    }

    #[test]
    fn error_codes_reach_the_ui() {
        assert_eq!(list_err(ListError::OAuth("timeout".into())).kind, ErrorKind::Auth);
        assert_eq!(list_err(ListError::OAuth("timeout".into())).detail, "timeout");
        assert_eq!(list_err(ListError::Unauthorized).detail, "expired");
        assert_eq!(list_err(ListError::NotConnected).detail, "not_connected");
        assert_eq!(list_err(ListError::Network("x".into())).kind, ErrorKind::Offline);
        assert_eq!(list_err(ListError::RateLimited).kind, ErrorKind::RateLimited);
        assert_eq!(list_err(ListError::Http(500)).kind, ErrorKind::Other);
    }

    #[test]
    fn service_mapping_and_profiles() {
        for s in Service::ALL {
            assert_eq!(core_service(vm_service(s)), s);
            assert_eq!(vm_service(s).id(), s.id());
        }
        assert_eq!(profile_url(Service::AniList, "kai"), "https://anilist.co/user/kai");
        assert_eq!(profile_url(Service::Mal, "kai"), "https://myanimelist.net/profile/kai");
    }

    fn remote() -> RemoteEntry {
        RemoteEntry {
            id: 77,
            media: map_anilist_media(&json!({
                "id": 21, "idMal": 20, "title": { "romaji": "One Piece" }, "episodes": 1100,
                "coverImage": { "large": "https://img/op.jpg" }
            })),
            status: "REPEATING".into(),
            progress: 12,
            score_100: Some(85),
            started_at: Some("2024-01-02".into()),
            completed_at: None,
            repeats: 1,
            notes: None,
            updated_at: 1_700_000_000,
        }
    }

    #[test]
    fn anilist_entries_become_base_rows() {
        let r = anilist_row(&remote(), "21");
        assert_eq!(r.service, "anilist");
        assert_eq!(r.status, "repeating");
        assert_eq!((r.anilist_id, r.mal_id), (Some(21), Some(20)));
        assert_eq!(r.remote_id, Some(77));
        assert_eq!(r.score, Some(85));
        assert_eq!(r.episodes, Some(1100));
        assert_eq!(r.title, "One Piece");
        assert!(!r.dirty);
    }

    #[test]
    fn mal_entries_become_base_rows() {
        let e = MalEntry {
            mal_id: 20,
            title: String::new(),
            poster: None,
            episodes: None,
            status: "watching".into(),
            is_rewatching: true,
            progress: 3,
            score: 8,
            start_date: None,
            finish_date: None,
            num_times_rewatched: 2,
            comments: Some("nice".into()),
            updated_at: "2024-01-02T00:00:00Z".into(),
        };
        let mut r = mal_row(&e, "21", Some(21));
        assert_eq!(r.service, "mal");
        assert_eq!(r.status, "repeating");
        assert_eq!(r.score, Some(80));
        assert_eq!(r.updated_at, 1_704_153_600);
        assert_eq!(r.repeats, 2);
        let local = anilist_row(&remote(), "21");
        complete_from(&mut r, &local);
        assert_eq!(r.title, "One Piece");
        assert_eq!(r.episodes, Some(1100));
        assert_eq!(r.poster.as_deref(), Some("https://img/op.jpg"));
    }

    #[test]
    fn entry_view_models() {
        let mut row = anilist_row(&remote(), "21");
        row.score = Some(0);
        let vm = entry_vm(&row, ListServiceVm::Mal, true);
        assert_eq!(vm.status, ListStatusVm::Repeating);
        assert_eq!(vm.score, None, "0 = unscored");
        assert_eq!(vm.media.id, "21");
        assert_eq!(vm.media.subtitle, "1100 ep");
        assert!(vm.pending);
        let fav = favorite_vm(&FavoriteRow { media_id: "character:40".into(), title: String::new(), poster: None, added_at: 1 });
        assert_eq!(fav.title, "character:40");
    }

    #[test]
    fn updates_map_to_patches() {
        let p = patch_of(&ListEntryUpdateVm { status: Some(ListStatusVm::Paused), progress: Some(4), score: Some(0), notes: None });
        assert_eq!(p.status.as_deref(), Some("paused"));
        assert_eq!(p.progress, Some(4));
        assert_eq!(p.score, Some(0));
    }
}
