//! AllDebrid (`https://api.alldebrid.com/v4`, `Authorization: Bearer <key>`, envelope `{ status: "success" |
//! "error", data, error: { code, message } }`). Ported from the classic `src/core/providers/cloud-debrid-client.ts`
//! (`resolveAllDebrid`): `magnet/upload` -> `magnet/status` (v4.1, `statusCode` 4 = ready) -> `magnet/files` ->
//! `link/unlock`.

use serde_json::Value;

use super::{Account, POLL_ATTEMPTS, POLL_INTERVAL, RemoteFile, Resolved, Service, base_name, json_of, number, pick_file, text};
use crate::error::DebridError;

const BASE: &str = "https://api.alldebrid.com/v4";
const BASE_41: &str = "https://api.alldebrid.com/v4.1";
/// AllDebrid asks API clients to identify themselves.
const AGENT: &str = "tatakai";
/// `magnet/status` code of a ready magnet; codes above it are errors.
const READY: u64 = 4;
/// Hashes probed by uploading them (the account list answers for the rest).
const MAX_PROBES: usize = 3;
/// Folder depth / file count limits of the `magnet/files` tree walk.
const MAX_DEPTH: usize = 12;
const MAX_FILES: usize = 2000;

/// The `data` of an AllDebrid envelope, or the error it carries.
fn data(v: Value) -> Result<Value, DebridError> {
    if v["status"].as_str() == Some("success") {
        return Ok(v["data"].clone());
    }
    let code = v["error"]["code"].as_str().unwrap_or_default();
    if code.starts_with("AUTH_") {
        return Err(DebridError::Unauthorized);
    }
    let message = text(&v["error"]["message"]).unwrap_or_else(|| "request failed".into());
    Err(DebridError::Parse(message))
}

async fn get(client: &reqwest::Client, key: &str, base: &str, path: &str, query: &[(&str, &str)]) -> Result<Value, DebridError> {
    let resp = client.get(format!("{base}{path}")).bearer_auth(key).query(&[("agent", AGENT)]).query(query).send().await?;
    data(json_of(resp).await?)
}

async fn post(client: &reqwest::Client, key: &str, base: &str, path: &str, form: &[(&str, &str)]) -> Result<Value, DebridError> {
    let resp = client.post(format!("{base}{path}")).bearer_auth(key).query(&[("agent", AGENT)]).form(form).send().await?;
    data(json_of(resp).await?)
}

pub(super) async fn account(client: &reqwest::Client, key: &str) -> Result<Account, DebridError> {
    parse_account(&get(client, key, BASE, "/user", &[]).await?)
}

fn parse_account(d: &Value) -> Result<Account, DebridError> {
    let user = &d["user"];
    if !user.is_object() {
        return Err(DebridError::Parse("AllDebrid /user without an account".into()));
    }
    Ok(Account {
        service: Service::AllDebrid,
        username: text(&user["username"]).or_else(|| text(&user["email"])),
        premium_until: number(&user["premiumUntil"])
            .filter(|u| *u > 0 && user["isPremium"].as_bool() != Some(false))
            .and_then(|u| i64::try_from(u).ok()),
    })
}

/// Hashes of ready magnets in a `magnet/status` answer.
fn ready_hashes(d: &Value) -> Vec<String> {
    let list = match &d["magnets"] {
        Value::Array(items) => items.clone(),
        Value::Object(_) => vec![d["magnets"].clone()],
        _ => Vec::new(),
    };
    list.iter()
        .filter(|m| number(&m["statusCode"]) == Some(READY))
        .filter_map(|m| m["hash"].as_str().map(str::to_ascii_lowercase))
        .collect()
}

pub(super) async fn check_cached(client: &reqwest::Client, key: &str, hashes: &[String]) -> Result<Vec<String>, DebridError> {
    let ready = ready_hashes(&post(client, key, BASE_41, "/magnet/status", &[]).await?);
    let mut cached: Vec<String> = hashes.iter().filter(|h| ready.contains(h)).cloned().collect();
    for hash in hashes.iter().filter(|h| !ready.contains(h)).take(MAX_PROBES) {
        let upload = match post(client, key, BASE, "/magnet/upload", &[("magnets[]", hash.as_str())]).await {
            Ok(u) => u,
            Err(DebridError::Unauthorized) => return Err(DebridError::Unauthorized),
            Err(e) => {
                tracing::debug!(%hash, error = %e, "AllDebrid cache probe failed");
                continue;
            }
        };
        let m = &upload["magnets"][0];
        if m["ready"].as_bool() == Some(true) {
            cached.push(hash.clone());
        } else if let Some(id) = text(&m["id"]) {
            // Not cached: do not leave a download running in the account.
            let _ = post(client, key, BASE, "/magnet/delete", &[("id", id.as_str())]).await;
        }
    }
    Ok(cached)
}

/// Flatten a `magnet/files` tree (`{ n, s, l }` files, `{ n, e: [...] }` folders).
fn walk(nodes: &Value, prefix: &str, depth: usize, out: &mut Vec<RemoteFile>) {
    let Some(items) = nodes.as_array() else { return };
    if depth > MAX_DEPTH {
        return;
    }
    for node in items {
        if out.len() >= MAX_FILES {
            return;
        }
        let name = node["n"].as_str().unwrap_or_default();
        if node.get("e").is_some() {
            walk(&node["e"], &format!("{prefix}{name}/"), depth + 1, out);
        } else if let Some(link) = text(&node["l"]) {
            out.push(RemoteFile { id: link, path: format!("{prefix}{name}"), size: number(&node["s"]).unwrap_or(0) });
        }
    }
}

fn status_of(d: &Value, id: &str) -> Option<Value> {
    match &d["magnets"] {
        Value::Array(items) => items.iter().find(|m| text(&m["id"]).as_deref() == Some(id)).cloned(),
        Value::Object(_) => Some(d["magnets"].clone()),
        _ => None,
    }
}

pub(super) async fn resolve(client: &reqwest::Client, key: &str, magnet: &str, episode: u32, titles: &[String]) -> Result<Resolved, DebridError> {
    let upload = post(client, key, BASE, "/magnet/upload", &[("magnets[]", magnet)]).await?;
    let first = &upload["magnets"][0];
    if let Some(err) = text(&first["error"]["message"]) {
        return Err(DebridError::Parse(err));
    }
    let id = text(&first["id"]).ok_or_else(|| DebridError::Parse("AllDebrid did not accept the magnet".into()))?;
    let mut ready = first["ready"].as_bool() == Some(true);
    for attempt in 0..POLL_ATTEMPTS {
        if ready {
            break;
        }
        let state = post(client, key, BASE_41, "/magnet/status", &[("id", id.as_str())]).await?;
        let code = status_of(&state, &id).and_then(|m| number(&m["statusCode"]));
        match code {
            Some(READY) => ready = true,
            Some(c) if c > READY => return Err(DebridError::Parse("AllDebrid could not prepare this torrent".into())),
            // 1 = downloading on AllDebrid: not cached.
            Some(1) if attempt >= 2 => return Err(DebridError::NotCached),
            _ => tokio::time::sleep(POLL_INTERVAL).await,
        }
    }
    if !ready {
        return Err(DebridError::NotCached);
    }
    let listing = post(client, key, BASE, "/magnet/files", &[("id[]", id.as_str())]).await?;
    let mut files = Vec::new();
    if let Some(m) = status_of(&listing, &id) {
        walk(&m["files"], "", 0, &mut files);
    }
    let file = pick_file(&files, episode, titles).cloned().ok_or(DebridError::NoFile)?;
    let unlocked = post(client, key, BASE, "/link/unlock", &[("link", file.id.as_str())]).await?;
    let url = text(&unlocked["link"]).ok_or_else(|| DebridError::Parse("AllDebrid did not return a playable link".into()))?;
    Ok(Resolved {
        url,
        file_name: text(&unlocked["filename"]).unwrap_or_else(|| base_name(&file.path)),
        size_bytes: number(&unlocked["filesize"]).or(Some(file.size)).filter(|s| *s > 0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn envelope_and_account() {
        assert!(matches!(data(json!({"status": "error", "error": {"code": "AUTH_BAD_APIKEY", "message": "x"}})), Err(DebridError::Unauthorized)));
        assert!(matches!(data(json!({"status": "error", "error": {"code": "MAGNET_INVALID", "message": "bad"}})), Err(DebridError::Parse(m)) if m == "bad"));
        let d = data(json!({"status": "success", "data": {"user": {"username": "ana", "isPremium": true, "premiumUntil": 1893456000}}})).unwrap();
        let a = parse_account(&d).unwrap();
        assert_eq!((a.username.as_deref(), a.premium_until), (Some("ana"), Some(1_893_456_000)));
        assert!(parse_account(&json!({})).is_err());
    }

    #[test]
    fn ready_hashes_and_file_tree() {
        let d = json!({"magnets": [{"id": 1, "hash": "ABC", "statusCode": 4}, {"id": 2, "hash": "def", "statusCode": 1}]});
        assert_eq!(ready_hashes(&d), vec!["abc".to_owned()]);
        let tree = json!([
            {"n": "Show", "e": [
                {"n": "Show - 01.mkv", "s": 100, "l": "https://alldebrid.com/f/1"},
                {"n": "Extras", "e": [{"n": "NCOP.mkv", "s": 5, "l": "https://alldebrid.com/f/2"}]}
            ]},
            {"n": "info.nfo", "s": 1, "l": "https://alldebrid.com/f/3"}
        ]);
        let mut files = Vec::new();
        walk(&tree, "", 0, &mut files);
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["Show/Show - 01.mkv", "Show/Extras/NCOP.mkv", "info.nfo"]);
        assert_eq!(status_of(&json!({"magnets": {"id": 9, "statusCode": 4}}), "9").and_then(|m| number(&m["statusCode"])), Some(4));
    }
}
