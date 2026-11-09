//! Real-Debrid (`https://api.real-debrid.com/rest/1.0`, `Authorization: Bearer <key>`). Ported from the classic
//! `src/core/providers/realdebrid-client.ts` + `resolveRealDebridMagnet`: `addMagnet` -> `torrents/info` ->
//! `selectFiles` -> `info.links[0]` -> `unrestrict/link`.

use serde_json::Value;

use super::{Account, POLL_ATTEMPTS, POLL_INTERVAL, RemoteFile, Resolved, Service, base_name, json_of, number, pick_file, rfc3339_unix, text};
use crate::error::DebridError;

const BASE: &str = "https://api.real-debrid.com/rest/1.0";
/// Hashes probed by adding them (the account list answers for the rest).
const MAX_PROBES: usize = 3;
/// Statuses after which a torrent will never become ready.
const FAILED: [&str; 4] = ["magnet_error", "error", "virus", "dead"];

pub(super) async fn account(client: &reqwest::Client, key: &str) -> Result<Account, DebridError> {
    let v = json_of(client.get(format!("{BASE}/user")).bearer_auth(key).send().await?).await?;
    parse_account(&v)
}

fn parse_account(v: &Value) -> Result<Account, DebridError> {
    if v.get("id").is_none() && v.get("username").is_none() {
        return Err(DebridError::Parse("Real-Debrid /user without an account".into()));
    }
    Ok(Account {
        service: Service::RealDebrid,
        username: text(&v["username"]).or_else(|| text(&v["email"])),
        premium_until: rfc3339_unix(&v["expiration"]).filter(|_| v["type"].as_str() != Some("free")),
    })
}

/// Hashes the account holds as `downloaded` (from `GET /torrents`).
fn ready_hashes(list: &Value) -> Vec<String> {
    list.as_array()
        .map(|items| {
            items
                .iter()
                .filter(|t| t["status"].as_str() == Some("downloaded"))
                .filter_map(|t| t["hash"].as_str().map(str::to_ascii_lowercase))
                .collect()
        })
        .unwrap_or_default()
}

pub(super) async fn check_cached(client: &reqwest::Client, key: &str, hashes: &[String]) -> Result<Vec<String>, DebridError> {
    let list = json_of(client.get(format!("{BASE}/torrents")).query(&[("limit", "100")]).bearer_auth(key).send().await?).await?;
    let ready = ready_hashes(&list);
    let mut cached: Vec<String> = hashes.iter().filter(|h| ready.contains(h)).cloned().collect();
    for hash in hashes.iter().filter(|h| !ready.contains(h)).take(MAX_PROBES) {
        match probe(client, key, hash).await {
            Ok(true) => cached.push(hash.clone()),
            Ok(false) => {}
            Err(DebridError::Unauthorized) => return Err(DebridError::Unauthorized),
            Err(e) => tracing::debug!(%hash, error = %e, "Real-Debrid cache probe failed"),
        }
    }
    Ok(cached)
}

/// Add the hash, select every file and see whether it is `downloaded` right away. Not cached: removed again.
async fn probe(client: &reqwest::Client, key: &str, hash: &str) -> Result<bool, DebridError> {
    let id = add_magnet(client, key, &format!("magnet:?xt=urn:btih:{hash}")).await?;
    let mut ready = false;
    for _ in 0..3 {
        let info = info(client, key, &id).await?;
        match info["status"].as_str().unwrap_or_default() {
            "waiting_files_selection" => select_files(client, key, &id, "all").await?,
            "downloaded" => {
                ready = true;
                break;
            }
            "magnet_conversion" | "queued" => {}
            _ => break,
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
    if !ready {
        let _ = client.delete(format!("{BASE}/torrents/delete/{id}")).bearer_auth(key).send().await;
    }
    Ok(ready)
}

async fn add_magnet(client: &reqwest::Client, key: &str, magnet: &str) -> Result<String, DebridError> {
    let v = json_of(client.post(format!("{BASE}/torrents/addMagnet")).bearer_auth(key).form(&[("magnet", magnet)]).send().await?).await?;
    text(&v["id"]).ok_or_else(|| DebridError::Parse("Real-Debrid did not return a torrent id".into()))
}

async fn info(client: &reqwest::Client, key: &str, id: &str) -> Result<Value, DebridError> {
    let v = json_of(client.get(format!("{BASE}/torrents/info/{id}")).bearer_auth(key).send().await?).await?;
    if let Some(status) = v["status"].as_str()
        && FAILED.contains(&status)
    {
        return Err(DebridError::Parse(format!("Real-Debrid could not prepare this torrent ({status})")));
    }
    Ok(v)
}

async fn select_files(client: &reqwest::Client, key: &str, id: &str, files: &str) -> Result<(), DebridError> {
    json_of(client.post(format!("{BASE}/torrents/selectFiles/{id}")).bearer_auth(key).form(&[("files", files)]).send().await?).await?;
    Ok(())
}

/// `info.files` -> remote files (`id` = Real-Debrid file id, paths without the leading `/`).
fn files_of(info: &Value) -> Vec<RemoteFile> {
    info["files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter_map(|f| {
                    Some(RemoteFile {
                        id: text(&f["id"])?,
                        path: f["path"].as_str()?.trim_start_matches('/').to_owned(),
                        size: number(&f["bytes"]).unwrap_or(0),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The link of `file` in `info.links`: links follow the order of the selected files.
fn link_for(info: &Value, file_id: &str) -> Option<String> {
    let files = info["files"].as_array()?;
    let selected: Vec<String> =
        files.iter().filter(|f| number(&f["selected"]) == Some(1)).filter_map(|f| text(&f["id"])).collect();
    let pos = selected.iter().position(|id| id == file_id)?;
    info["links"].as_array()?.get(pos).and_then(|l| l.as_str()).map(str::to_owned)
}

pub(super) async fn resolve(client: &reqwest::Client, key: &str, magnet: &str, episode: u32, titles: &[String]) -> Result<Resolved, DebridError> {
    let id = add_magnet(client, key, magnet).await?;
    let mut current = Value::Null;
    for _ in 0..POLL_ATTEMPTS {
        current = info(client, key, &id).await?;
        let has_files = current["files"].as_array().is_some_and(|f| !f.is_empty());
        if has_files && current["status"].as_str() != Some("magnet_conversion") {
            break;
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
    let files = files_of(&current);
    let file = pick_file(&files, episode, titles).cloned().ok_or(DebridError::NoFile)?;
    if current["status"].as_str() == Some("waiting_files_selection") || link_for(&current, &file.id).is_none() {
        select_files(client, key, &id, &file.id).await?;
    }
    for attempt in 0..POLL_ATTEMPTS {
        let state = info(client, key, &id).await?;
        if state["status"].as_str() == Some("downloaded")
            && let Some(link) = link_for(&state, &file.id)
        {
            let v = json_of(client.post(format!("{BASE}/unrestrict/link")).bearer_auth(key).form(&[("link", link.as_str())]).send().await?).await?;
            let url = text(&v["download"]).ok_or_else(|| DebridError::Parse("Real-Debrid did not return a download link".into()))?;
            return Ok(Resolved {
                url,
                file_name: text(&v["filename"]).unwrap_or_else(|| base_name(&file.path)),
                size_bytes: number(&v["filesize"]).or(Some(file.size)).filter(|s| *s > 0),
            });
        }
        if attempt >= 2 && matches!(state["status"].as_str(), Some("downloading" | "queued")) {
            // Not cached: Real-Debrid started downloading it.
            return Err(DebridError::NotCached);
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
    Err(DebridError::NotCached)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn account_and_ready_hashes() {
        let a = parse_account(&json!({"id": 1, "username": "ana", "type": "premium", "expiration": "2030-01-01T00:00:00.000Z"})).unwrap();
        assert_eq!(a.username.as_deref(), Some("ana"));
        assert_eq!(a.premium_until, Some(1_893_456_000));
        let free = parse_account(&json!({"id": 1, "username": "b", "type": "free", "expiration": "2030-01-01T00:00:00.000Z"})).unwrap();
        assert_eq!(free.premium_until, None);
        assert!(parse_account(&json!({"error": "bad_token"})).is_err());
        let list = json!([{"hash": "ABC", "status": "downloaded"}, {"hash": "def", "status": "downloading"}]);
        assert_eq!(ready_hashes(&list), vec!["abc".to_owned()]);
    }

    #[test]
    fn files_and_links_follow_the_selection() {
        let info = json!({
            "status": "downloaded",
            "files": [
                {"id": 1, "path": "/Show/Show - 01.mkv", "bytes": 100, "selected": 0},
                {"id": 2, "path": "/Show/Show - 02.mkv", "bytes": 200, "selected": 1},
                {"id": 3, "path": "/Show/Show - 03.mkv", "bytes": 300, "selected": 1}
            ],
            "links": ["https://real-debrid.com/d/two", "https://real-debrid.com/d/three"]
        });
        let files = files_of(&info);
        assert_eq!(files[0].path, "Show/Show - 01.mkv");
        assert_eq!(link_for(&info, "3").as_deref(), Some("https://real-debrid.com/d/three"));
        assert_eq!(link_for(&info, "1"), None);
    }
}
