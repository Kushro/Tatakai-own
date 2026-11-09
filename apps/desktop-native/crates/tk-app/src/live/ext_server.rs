//! Extension-server client (wave B, worker **j**; docs/redesign/16-parity-wave-b-contract.md section 4; checklist
//! `ext.server-client.*`, `ext.ecosystems.via-server`, `ext.deep-link.server`; classic
//! `core/extensions/server/client.ts`, `ExtensionServersSettings.tsx`).
//! HTTP client in `tk_core::extserver` (`/api/server/v1`: `health`, `auth/pair`, `auth/login`, `auth/logout`,
//! `sources`, `invoke`, `resolve` NDJSON); servers in `LocalStore` (`tk-core/src/store/ext.rs::ext_servers`), tokens in
//! `b.secrets` under `tk_core::secrets::keys::ext_server(id)`. Remote candidates reach the ranking through
//! `ext_catalog::extra_candidates` -> [`remote_candidates`]: `ext_id = "server:<id>"`, `ext_name` = the server's name,
//! so the selector shows where each source comes from. Aniyomi / Mihon / CloudStream / Sora / Mangayomi / Seanime
//! sources installed on the server arrive the same way (`ext.ecosystems.via-server`): Next is their client only.
//!
//! Offline handling (`ext.server-client.fallback`): a server that does not answer is marked offline (session state +
//! `last_error`) and never blocks the local extensions; with `Settings::ext.server_fallback_local` a quick health
//! probe runs first so an unreachable PC costs seconds, not the whole resolution timeout.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde_json::json;
use tk_core::extserver::{self, CLIENT_LABEL, ExtServerClient, PROBE_TIMEOUT, SourceDescriptor};
use tk_core::secrets::keys;
use tk_core::sources::SourceCandidate;
use tk_core::store::ext::ExtServerRow;
use tk_core::{ExtServerError, Media};
use tk_ui::backend::{ExtServerPairVm, ExtServerVm, ExtSourceVm, UiResult};
use tk_ui::model::{ErrorKind, UiError};

use crate::live::LiveBackend;

/// Session state of a server: reachable or not, enabled sources it offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Live {
    online: bool,
    sources: u32,
}

fn session() -> &'static Mutex<HashMap<String, Live>> {
    static S: OnceLock<Mutex<HashMap<String, Live>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(HashMap::new()))
}

fn set_live(id: &str, online: bool, sources: Option<u32>) {
    if let Ok(mut m) = session().lock() {
        let prev = m.get(id).map_or(0, |l| l.sources);
        m.insert(id.to_owned(), Live { online, sources: sources.unwrap_or(prev) });
    }
}

fn live_of(id: &str) -> Option<Live> {
    session().lock().ok().and_then(|m| m.get(id).copied())
}

/// Plain client for LAN servers (the host guard of `tk_core::extserver` decides what may be contacted).
fn http() -> reqwest::Client {
    static C: OnceLock<reqwest::Client> = OnceLock::new();
    C.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent("TatakaiNext/0.1")
            .connect_timeout(Duration::from_secs(5))
            .build()
            .unwrap_or_default()
    })
    .clone()
}

fn now_unix() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

/// `ExtServerError` -> `UiError`. Refusals keep their stable code as `detail` (the UI localizes
/// `extserver.error.<code>`); network failures are `Offline`.
pub(crate) fn server_err(e: ExtServerError) -> UiError {
    match e {
        ExtServerError::Network(m) => UiError::new(ErrorKind::Offline, m),
        ExtServerError::Refused(code) => UiError::new(ErrorKind::Auth, code),
        ExtServerError::NotPaired(m) => UiError::new(ErrorKind::Auth, format!("authentication_required: {m}")),
        ExtServerError::Http(s) => UiError::new(ErrorKind::Extension, format!("http {s}")),
        ExtServerError::Parse(m) => UiError::new(ErrorKind::Extension, m),
    }
}

fn store_err(e: impl std::fmt::Display) -> UiError {
    UiError::new(ErrorKind::Other, e.to_string())
}

fn not_found(id: &str) -> UiError {
    UiError::new(ErrorKind::Other, format!("extension server {id} not found"))
}

fn vm(row: &ExtServerRow) -> ExtServerVm {
    let live = live_of(&row.id);
    ExtServerVm {
        id: row.id.clone(),
        name: row.name.clone(),
        url: row.url.clone(),
        enabled: row.enabled,
        online: live.map(|l| l.online),
        sources: live.map_or(0, |l| l.sources),
        role: row.role.clone(),
        last_error: row.last_error.clone(),
        last_ok_unix: row.last_ok_at,
    }
}

fn source_vm(d: SourceDescriptor) -> ExtSourceVm {
    ExtSourceVm {
        id: d.id,
        name: if d.name.is_empty() { d.extension_id.clone() } else { d.name },
        ecosystem: if d.ecosystem.is_empty() { "tatakai".to_owned() } else { d.ecosystem },
        kind: d.kind,
        languages: d.languages,
        enabled: d.enabled,
        available: d.available,
        reason: d.reason.filter(|r| !r.is_empty()),
    }
}

fn token_of(b: &LiveBackend, id: &str) -> Option<String> {
    b.secrets.get(&keys::ext_server(id)).ok().flatten().filter(|t| !t.is_empty())
}

fn client_for(b: &LiveBackend, row: &ExtServerRow) -> Result<ExtServerClient, UiError> {
    ExtServerClient::new(http(), &row.url, token_of(b, &row.id)).map_err(server_err)
}

/// Contact the server once: health + sources. Records the outcome (store + session) and returns the source count.
async fn probe(b: &LiveBackend, row: &ExtServerRow) -> Result<u32, UiError> {
    let client = client_for(b, row)?;
    let r = async {
        client.health_within(PROBE_TIMEOUT).await?;
        client.sources().await
    }
    .await;
    match r {
        Ok(list) => {
            let n = list.iter().filter(|s| s.enabled && s.available).count() as u32;
            set_live(&row.id, true, Some(n));
            let _ = b.store.set_ext_server_result(&row.id, Some(now_unix()), None);
            Ok(n)
        }
        Err(e) => {
            let e = server_err(e);
            set_live(&row.id, false, None);
            let _ = b.store.set_ext_server_result(&row.id, None, Some(&e.detail));
            Err(e)
        }
    }
}

/// Start-up (`LiveBackend::on_start`): register the `tatakai:` scheme for this user (best effort), push the stored
/// extension preferences into the host, and probe the enabled servers so the list shows their state.
pub async fn on_start(b: &LiveBackend) {
    let _ = tokio::task::spawn_blocking(|| {
        if let Err(e) = crate::deeplink::ensure_registered() {
            tracing::debug!(error = %e, "tatakai: scheme not registered");
        }
    })
    .await;
    super::ext_catalog::push_all_preferences(b);
    let rows: Vec<ExtServerRow> = b.store.ext_servers().unwrap_or_default().into_iter().filter(|r| r.enabled).collect();
    let probes = rows.iter().map(|r| probe(b, r));
    for (row, r) in rows.iter().zip(join_all(probes).await) {
        if let Err(e) = r {
            tracing::info!(server = %row.name, detail = %e.detail, "extension server offline at start-up");
        }
    }
}

/// Await every future (sequential polling order, concurrent progress) without an extra dependency.
async fn join_all<F: std::future::Future>(futs: impl IntoIterator<Item = F>) -> Vec<F::Output> {
    let futs: Vec<F> = futs.into_iter().collect();
    let mut out: Vec<Option<F::Output>> = futs.iter().map(|_| None).collect();
    let mut pinned: Vec<std::pin::Pin<Box<F>>> = futs.into_iter().map(Box::pin).collect();
    std::future::poll_fn(|cx| {
        let mut pending = false;
        for (i, f) in pinned.iter_mut().enumerate() {
            if out[i].is_some() {
                continue;
            }
            match f.as_mut().poll(cx) {
                std::task::Poll::Ready(v) => out[i] = Some(v),
                std::task::Poll::Pending => pending = true,
            }
        }
        if pending { std::task::Poll::Pending } else { std::task::Poll::Ready(()) }
    })
    .await;
    out.into_iter().flatten().collect()
}

pub(crate) use join_all as join_all_futures;

pub async fn servers(b: &LiveBackend) -> UiResult<Vec<ExtServerVm>> {
    Ok(b.store.ext_servers().map_err(store_err)?.iter().map(vm).collect())
}

/// Pair from a deep link / address + code / password (classic `connectServer`): `/health` first (protocol 1, set up),
/// then `/auth/pair` with a code or `/auth/login` with the viewer password (empty when the server allows it).
pub async fn pair(b: &LiveBackend, req: &ExtServerPairVm) -> UiResult<ExtServerVm> {
    let link = extserver::parse_server_link(&req.url).map_err(server_err)?;
    let code = req.code.as_deref().map(str::trim).filter(|c| !c.is_empty()).map(str::to_owned).or(link.code);
    let password = req.password.clone().or(link.password).unwrap_or_default();
    let probe_client = ExtServerClient::new(http(), &link.server, None).map_err(server_err)?;
    let health = probe_client.health().await.map_err(server_err)?;
    if health.setup_required {
        return Err(server_err(ExtServerError::Refused("setup_required".into())));
    }
    let token = match &code {
        Some(c) => probe_client.pair(c, CLIENT_LABEL).await,
        None if health.pairing_required && password.is_empty() => Err(ExtServerError::Refused("pairing_required".into())),
        None => probe_client.login_viewer(&password, CLIENT_LABEL).await,
    }
    .map_err(server_err)?;

    let existing = b
        .store
        .ext_servers()
        .map_err(store_err)?
        .into_iter()
        .find(|r| r.url == link.server && (health.server_id.is_none() || r.server_id == health.server_id));
    let id = existing.as_ref().map_or_else(extserver::new_id, |r| r.id.clone());
    let name = req
        .name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_owned)
        .or_else(|| existing.as_ref().map(|r| r.name.clone()))
        .or_else(|| Some(health.name.trim().to_owned()).filter(|n| !n.is_empty()))
        .unwrap_or_else(|| link.server.clone());
    b.secrets.set(&keys::ext_server(&id), &token.token).map_err(store_err)?;
    let row = ExtServerRow {
        id: id.clone(),
        name,
        url: link.server.clone(),
        role: token.role.clone(),
        enabled: true,
        server_id: health.server_id.clone(),
        last_ok_at: Some(now_unix()),
        last_error: None,
        added_at: existing.as_ref().map_or_else(now_unix, |r| r.added_at),
    };
    b.store.upsert_ext_server(&row).map_err(store_err)?;
    set_live(&id, true, None);
    // Best effort: the source count of the list.
    let _ = probe(b, &row).await;
    let row = b.store.ext_server(&id).map_err(store_err)?.unwrap_or(row);
    Ok(vm(&row))
}

pub async fn set_enabled(b: &LiveBackend, id: &str, enabled: bool) -> UiResult<()> {
    if !b.store.set_ext_server_enabled(id, enabled).map_err(store_err)? {
        return Err(not_found(id));
    }
    Ok(())
}

/// Forget the server: `POST /auth/logout` when it answers (revokes the token on the PC), then token and row.
pub async fn remove(b: &LiveBackend, id: &str) -> UiResult<()> {
    let row = b.store.ext_server(id).map_err(store_err)?.ok_or_else(|| not_found(id))?;
    if let Ok(client) = client_for(b, &row)
        && let Err(e) = client.logout().await
    {
        tracing::debug!(server = %row.name, error = %e, "logout failed (removing locally anyway)");
    }
    let _ = b.secrets.delete(&keys::ext_server(id));
    b.store.delete_ext_server(id).map_err(store_err)?;
    if let Ok(mut m) = session().lock() {
        m.remove(id);
    }
    Ok(())
}

/// Health + sources: an unreachable server answers `Ok` with `online = Some(false)` and its `last_error`.
pub async fn test(b: &LiveBackend, id: &str) -> UiResult<ExtServerVm> {
    let row = b.store.ext_server(id).map_err(store_err)?.ok_or_else(|| not_found(id))?;
    if let Err(e) = probe(b, &row).await {
        tracing::info!(server = %row.name, detail = %e.detail, "extension server test failed");
    }
    let row = b.store.ext_server(id).map_err(store_err)?.unwrap_or(row);
    Ok(vm(&row))
}

pub async fn sources(b: &LiveBackend, id: &str) -> UiResult<Vec<ExtSourceVm>> {
    let row = b.store.ext_server(id).map_err(store_err)?.ok_or_else(|| not_found(id))?;
    let client = client_for(b, &row)?;
    match client.sources().await {
        Ok(list) => {
            let n = list.iter().filter(|s| s.enabled && s.available).count() as u32;
            set_live(id, true, Some(n));
            let _ = b.store.set_ext_server_result(id, Some(now_unix()), None);
            Ok(list.into_iter().map(source_vm).collect())
        }
        Err(e) => {
            let e = server_err(e);
            set_live(id, false, None);
            let _ = b.store.set_ext_server_result(id, None, Some(&e.detail));
            Err(e)
        }
    }
}

/// `Backend::take_pending_server_link`: implemented (pure hand-over of `crate::deeplink::take`, the
/// `tatakai://extensionserver/connect?...` link `main.rs` kept from the command line or a second instance).
pub async fn take_pending_link(_b: &LiveBackend) -> UiResult<Option<String>> {
    Ok(crate::deeplink::take())
}

/// Resolution arguments of `/resolve` (classic `resolveRemoteSources` options).
fn resolve_args(media: &Media, episode: u32) -> serde_json::Value {
    let mut args = json!({ "title": media.title, "episode": episode.max(1), "dub": false });
    if let Some(id) = media.media_ref.anilist_id {
        args["anilistId"] = json!(id);
    }
    args
}

/// Candidates of every enabled, paired server for one episode (`ext.server-client.sources`). Failures mark the
/// server offline and yield nothing; local extensions are never waited on by this.
pub async fn remote_candidates(b: &LiveBackend, media: &Media, episode: u32) -> Vec<SourceCandidate> {
    let rows: Vec<ExtServerRow> = b.store.ext_servers().unwrap_or_default().into_iter().filter(|r| r.enabled).collect();
    if rows.is_empty() {
        return Vec::new();
    }
    let quick_probe = crate::settings_store::load(&b.settings_file).ext.server_fallback_local;
    let args = resolve_args(media, episode);
    let work = rows.iter().map(|row| {
        let args = args.clone();
        async move {
            let client = match client_for(b, row) {
                Ok(c) => c,
                Err(_) => return Vec::new(),
            };
            // A server already known offline this session gets a quick probe instead of the full resolve timeout.
            if quick_probe
                && live_of(&row.id).is_some_and(|l| !l.online)
                && let Err(e) = client.health_within(PROBE_TIMEOUT).await
            {
                let e = server_err(e);
                let _ = b.store.set_ext_server_result(&row.id, None, Some(&e.detail));
                return Vec::new();
            }
            match client.resolve(args).await {
                Ok(snapshot) => {
                    set_live(&row.id, true, None);
                    let _ = b.store.set_ext_server_result(&row.id, Some(now_unix()), None);
                    let body = extserver::normalize_snapshot(&row.url, &snapshot);
                    tk_ext::map_candidates(&format!("server:{}", row.id), &row.name, &body)
                }
                Err(e) => {
                    let e = server_err(e);
                    tracing::info!(server = %row.name, detail = %e.detail, "extension server unavailable; local extensions only");
                    set_live(&row.id, false, None);
                    let _ = b.store.set_ext_server_result(&row.id, None, Some(&e.detail));
                    Vec::new()
                }
            }
        }
    });
    join_all(work).await.into_iter().flatten().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refusals_keep_their_code() {
        let e = server_err(ExtServerError::Refused("pairing_expired".into()));
        assert_eq!((e.kind, e.detail.as_str()), (ErrorKind::Auth, "pairing_expired"));
        assert_eq!(server_err(ExtServerError::Network("x".into())).kind, ErrorKind::Offline);
        assert_eq!(server_err(ExtServerError::Http(502)).kind, ErrorKind::Extension);
    }

    #[test]
    fn session_state_keeps_the_last_source_count() {
        set_live("t-1", true, Some(4));
        set_live("t-1", false, None);
        assert_eq!(live_of("t-1"), Some(Live { online: false, sources: 4 }));
    }

    #[tokio::test]
    async fn join_all_keeps_the_order() {
        let futs = (0..4u64).map(|i| async move {
            tokio::time::sleep(Duration::from_millis(40 - i * 10)).await;
            i
        });
        assert_eq!(join_all(futs).await, vec![0, 1, 2, 3]);
    }

    #[test]
    fn resolve_args_carry_the_anilist_id() {
        let media = Media {
            media_ref: tk_core::MediaRef::anilist(21),
            kind: tk_core::MediaKind::Anime,
            title: "One Piece".into(),
            titles: Default::default(),
            cover: None,
            banner: None,
            color: None,
            synopsis: None,
            genres: Vec::new(),
            episodes: None,
            status: None,
            season: None,
            year: None,
            score: None,
            source_provider: "anilist".into(),
        };
        let a = resolve_args(&media, 0);
        assert_eq!((a["anilistId"].as_u64(), a["episode"].as_u64()), (Some(21), Some(1)));
    }
}
