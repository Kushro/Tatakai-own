//! TorBox (`https://api.torbox.app/v1/api`, `Authorization: Bearer <key>`, envelope `{ success, error, detail,
//! data }`). Ported from the classic `src/core/providers/torbox-client.ts` + `resolveTorboxMagnet`:
//! `torrents/checkcached` for the picker, `torrents/createtorrent` (multipart) -> `torrents/mylist?id=` ->
//! `torrents/requestdl` for playback.

use serde_json::Value;

use super::{Account, POLL_ATTEMPTS, POLL_INTERVAL, RemoteFile, Resolved, Service, base_name, json_of, magnet_info_hash, number, pick_file, rfc3339_unix, text};
use crate::error::DebridError;

const BASE: &str = "https://api.torbox.app/v1/api";
/// TorBox accepts this many hashes per cache check.
const CHECK_BATCH: usize = 50;

/// The `data` of a TorBox envelope, or the error it carries.
fn data(v: Value) -> Result<Value, DebridError> {
    if v["success"].as_bool() == Some(false) {
        let detail = text(&v["detail"]).or_else(|| text(&v["error"])).unwrap_or_else(|| "request failed".into());
        let lower = detail.to_ascii_lowercase();
        if lower.contains("auth") || lower.contains("token") || lower.contains("api key") {
            return Err(DebridError::Unauthorized);
        }
        return Err(DebridError::Parse(detail));
    }
    Ok(match v.get("data") {
        Some(d) => d.clone(),
        None => v,
    })
}

pub(super) async fn account(client: &reqwest::Client, key: &str) -> Result<Account, DebridError> {
    let v = data(json_of(client.get(format!("{BASE}/user/me")).query(&[("settings", "false")]).bearer_auth(key).send().await?).await?)?;
    parse_account(&v)
}

fn parse_account(v: &Value) -> Result<Account, DebridError> {
    if !v.is_object() {
        return Err(DebridError::Parse("TorBox /user/me without an account".into()));
    }
    let paid = number(&v["plan"]).is_some_and(|p| p > 0);
    Ok(Account {
        service: Service::TorBox,
        username: text(&v["email"]).or_else(|| text(&v["username"])),
        premium_until: rfc3339_unix(&v["premium_expires_at"]).filter(|_| paid),
    })
}

/// Hashes present in a `checkcached?format=object` answer (`{ "<hash>": {...} }`; a list form is accepted too).
fn cached_of(d: &Value) -> Vec<String> {
    match d {
        Value::Object(map) => map.keys().map(|k| k.to_ascii_lowercase()).collect(),
        Value::Array(items) => items.iter().filter_map(|i| i["hash"].as_str().map(str::to_ascii_lowercase)).collect(),
        _ => Vec::new(),
    }
}

pub(super) async fn check_cached(client: &reqwest::Client, key: &str, hashes: &[String]) -> Result<Vec<String>, DebridError> {
    let mut out = Vec::new();
    for chunk in hashes.chunks(CHECK_BATCH) {
        let joined = chunk.join(",");
        let v = json_of(
            client
                .get(format!("{BASE}/torrents/checkcached"))
                .query(&[("hash", joined.as_str()), ("format", "object"), ("list_files", "false")])
                .bearer_auth(key)
                .send()
                .await?,
        )
        .await?;
        out.extend(cached_of(&data(v)?));
    }
    Ok(out)
}

/// `multipart/form-data` body with text fields only (no `multipart` feature in reqwest here).
fn multipart(fields: &[(&str, &str)]) -> (String, String) {
    let boundary = format!("----TatakaiDebrid{}", uuid::Uuid::new_v4().simple());
    let mut body = String::new();
    for (name, value) in fields {
        body.push_str(&format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"));
    }
    body.push_str(&format!("--{boundary}--\r\n"));
    (body, format!("multipart/form-data; boundary={boundary}"))
}

/// The torrent with `id` from a `mylist` answer (object or list; never another torrent of the list).
fn torrent_with_id(d: &Value, id: &str) -> Option<Value> {
    match d {
        Value::Array(items) => items.iter().find(|t| text(&t["id"]).as_deref() == Some(id)).cloned(),
        Value::Object(_) if text(&d["id"]).as_deref() == Some(id) => Some(d.clone()),
        _ => None,
    }
}

fn files_of(t: &Value) -> Vec<RemoteFile> {
    t["files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter_map(|f| {
                    let path = text(&f["name"]).or_else(|| text(&f["short_name"])).or_else(|| text(&f["absolute_path"]))?;
                    Some(RemoteFile { id: text(&f["id"])?, path, size: number(&f["size"]).unwrap_or(0) })
                })
                .collect()
        })
        .unwrap_or_default()
}

async fn existing(client: &reqwest::Client, key: &str, hash: &str) -> Option<String> {
    let v = json_of(client.get(format!("{BASE}/torrents/mylist")).query(&[("bypass_cache", "true")]).bearer_auth(key).send().await.ok()?)
        .await
        .ok()?;
    let d = data(v).ok()?;
    d.as_array()?
        .iter()
        .find(|t| t["hash"].as_str().is_some_and(|h| h.eq_ignore_ascii_case(hash)) && t["download_present"].as_bool() == Some(true))
        .and_then(|t| text(&t["id"]))
}

pub(super) async fn resolve(client: &reqwest::Client, key: &str, magnet: &str, episode: u32, titles: &[String]) -> Result<Resolved, DebridError> {
    // Reuse a torrent already in the account (no duplicate per play).
    let reused = match magnet_info_hash(magnet) {
        Some(hash) => existing(client, key, &hash).await,
        None => None,
    };
    let id = match reused {
        Some(id) => id,
        None => {
            let (body, content_type) = multipart(&[("magnet", magnet)]);
            let v = data(
                json_of(
                    client
                        .post(format!("{BASE}/torrents/createtorrent"))
                        .bearer_auth(key)
                        .header(reqwest::header::CONTENT_TYPE, content_type)
                        .body(body)
                        .send()
                        .await?,
                )
                .await?,
            )?;
            text(&v["torrent_id"]).or_else(|| text(&v["id"])).ok_or_else(|| DebridError::Parse("TorBox did not return a torrent id".into()))?
        }
    };
    let mut ready = None;
    for _ in 0..POLL_ATTEMPTS {
        let v = json_of(
            client
                .get(format!("{BASE}/torrents/mylist"))
                .query(&[("id", id.as_str()), ("bypass_cache", "true")])
                .bearer_auth(key)
                .send()
                .await?,
        )
        .await?;
        if let Some(t) = torrent_with_id(&data(v)?, &id) {
            if let Some(err) = text(&t["error"]) {
                return Err(DebridError::Parse(format!("TorBox could not prepare this torrent ({err})")));
            }
            if t["download_present"].as_bool() == Some(true) && t["files"].as_array().is_some_and(|f| !f.is_empty()) {
                ready = Some(t);
                break;
            }
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
    let torrent = ready.ok_or(DebridError::NotCached)?;
    let files = files_of(&torrent);
    let file = pick_file(&files, episode, titles).cloned().ok_or(DebridError::NoFile)?;
    // TorBox wants the token in this endpoint's query.
    let v = json_of(
        client
            .get(format!("{BASE}/torrents/requestdl"))
            .query(&[("token", key), ("torrent_id", id.as_str()), ("file_id", file.id.as_str()), ("redirect", "false")])
            .send()
            .await?,
    )
    .await?;
    let d = data(v)?;
    let url = text(&d).or_else(|| text(&d["url"])).ok_or_else(|| DebridError::Parse("TorBox did not return a download link".into()))?;
    Ok(Resolved { url, file_name: base_name(&file.path), size_bytes: Some(file.size).filter(|s| *s > 0) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn envelope_account_and_cache() {
        assert!(matches!(data(json!({"success": false, "detail": "Invalid API token"})), Err(DebridError::Unauthorized)));
        let d = data(json!({"success": true, "data": {"email": "a@b.c", "plan": 2, "premium_expires_at": "2030-01-01T00:00:00Z"}})).unwrap();
        let a = parse_account(&d).unwrap();
        assert_eq!((a.username.as_deref(), a.premium_until), (Some("a@b.c"), Some(1_893_456_000)));
        assert_eq!(cached_of(&json!({"ABC": {"name": "x"}})), vec!["abc".to_owned()]);
        assert_eq!(cached_of(&json!([{"hash": "def"}])), vec!["def".to_owned()]);
        assert!(cached_of(&Value::Null).is_empty());
    }

    #[test]
    fn mylist_never_takes_another_torrent() {
        let list = json!([{"id": 5, "files": []}, {"id": 7, "files": [{"id": 0, "name": "Show/Show - 01.mkv", "size": 10}]}]);
        let t = torrent_with_id(&list, "7").unwrap();
        assert_eq!(files_of(&t)[0].path, "Show/Show - 01.mkv");
        assert!(torrent_with_id(&list, "9").is_none());
        assert!(torrent_with_id(&json!({"id": 1}), "2").is_none());
    }

    #[test]
    fn multipart_body_shape() {
        let (body, ct) = multipart(&[("magnet", "magnet:?xt=urn:btih:abc")]);
        let boundary = ct.split("boundary=").nth(1).unwrap();
        assert!(body.starts_with(&format!("--{boundary}\r\n")));
        assert!(body.contains("name=\"magnet\"\r\n\r\nmagnet:?xt=urn:btih:abc\r\n"));
        assert!(body.ends_with(&format!("--{boundary}--\r\n")));
    }
}
