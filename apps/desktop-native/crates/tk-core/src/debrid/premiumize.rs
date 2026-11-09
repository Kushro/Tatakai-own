//! Premiumize (`https://www.premiumize.me/api`, `apikey` parameter, envelope `{ status: "success" | "error",
//! message }`). Ported from the classic `src/core/providers/cloud-debrid-client.ts` (`resolvePremiumize`):
//! `cache/check` for the picker, `transfer/directdl` for a cached magnet (direct links per file, no transfer).

use serde_json::Value;

use super::{Account, RemoteFile, Resolved, Service, base_name, json_of, number, pick_file, text};
use crate::error::DebridError;

const BASE: &str = "https://www.premiumize.me/api";
/// Items per `cache/check` request.
const CHECK_BATCH: usize = 100;

fn ok(v: Value) -> Result<Value, DebridError> {
    if v["status"].as_str() == Some("success") {
        return Ok(v);
    }
    let message = text(&v["message"]).unwrap_or_else(|| "request failed".into());
    let lower = message.to_ascii_lowercase();
    if lower.contains("api key") || lower.contains("apikey") || lower.contains("not logged in") || lower.contains("auth") {
        return Err(DebridError::Unauthorized);
    }
    Err(DebridError::Parse(message))
}

pub(super) async fn account(client: &reqwest::Client, key: &str) -> Result<Account, DebridError> {
    let v = ok(json_of(client.get(format!("{BASE}/account/info")).query(&[("apikey", key)]).send().await?).await?)?;
    Ok(parse_account(&v))
}

fn parse_account(v: &Value) -> Account {
    Account {
        service: Service::Premiumize,
        username: text(&v["customer_id"]),
        premium_until: number(&v["premium_until"]).filter(|u| *u > 0).and_then(|u| i64::try_from(u).ok()),
    }
}

/// `response[i]` is the answer for `hashes[i]`.
fn cached_of(v: &Value, hashes: &[String]) -> Vec<String> {
    let Some(answers) = v["response"].as_array() else { return Vec::new() };
    hashes.iter().zip(answers).filter(|(_, a)| a.as_bool() == Some(true)).map(|(h, _)| h.clone()).collect()
}

pub(super) async fn check_cached(client: &reqwest::Client, key: &str, hashes: &[String]) -> Result<Vec<String>, DebridError> {
    let mut out = Vec::new();
    for chunk in hashes.chunks(CHECK_BATCH) {
        let mut form: Vec<(&str, &str)> = vec![("apikey", key)];
        form.extend(chunk.iter().map(|h| ("items[]", h.as_str())));
        let v = ok(json_of(client.post(format!("{BASE}/cache/check")).form(&form).send().await?).await?)?;
        out.extend(cached_of(&v, chunk));
    }
    Ok(out)
}

fn files_of(v: &Value) -> Vec<RemoteFile> {
    v["content"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|f| {
                    let link = text(&f["link"]).or_else(|| text(&f["stream_link"]))?;
                    Some(RemoteFile { id: link, path: text(&f["path"])?, size: number(&f["size"]).unwrap_or(0) })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(super) async fn resolve(client: &reqwest::Client, key: &str, magnet: &str, episode: u32, titles: &[String]) -> Result<Resolved, DebridError> {
    let v = json_of(client.post(format!("{BASE}/transfer/directdl")).form(&[("apikey", key), ("src", magnet)]).send().await?).await?;
    // Not cached: Premiumize answers with an error (it would need a transfer first).
    let v = ok(v).map_err(|e| match e {
        DebridError::Unauthorized => DebridError::Unauthorized,
        _ => DebridError::NotCached,
    })?;
    let files = files_of(&v);
    if files.is_empty() {
        return Err(DebridError::NotCached);
    }
    let file = pick_file(&files, episode, titles).cloned().ok_or(DebridError::NoFile)?;
    Ok(Resolved { url: file.id.clone(), file_name: base_name(&file.path), size_bytes: Some(file.size).filter(|s| *s > 0) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn envelope_account_cache_and_files() {
        assert!(matches!(ok(json!({"status": "error", "message": "Not logged in."})), Err(DebridError::Unauthorized)));
        let a = parse_account(&json!({"status": "success", "customer_id": 1234, "premium_until": 1893456000}));
        assert_eq!((a.username.as_deref(), a.premium_until), (Some("1234"), Some(1_893_456_000)));
        let hashes = vec!["a".to_owned(), "b".to_owned(), "c".to_owned()];
        assert_eq!(cached_of(&json!({"response": [true, false, true]}), &hashes), vec!["a".to_owned(), "c".to_owned()]);
        let files = files_of(&json!({"content": [{"path": "Show/Show - 01.mkv", "size": 10, "link": "https://p.me/1"}, {"path": "x"}]}));
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].id, "https://p.me/1");
    }
}
