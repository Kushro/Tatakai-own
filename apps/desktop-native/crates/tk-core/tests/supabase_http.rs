//! Supabase client against a loopback stub server (no real network, no user directories).

use std::sync::{Arc, Mutex};

use tk_core::{HistoryEntry, MediaRef, Supabase};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

type Seen = Arc<Mutex<Vec<String>>>;

/// Serves canned responses in order; records each raw request (head + body).
async fn stub(responses: Vec<(u16, &'static str)>) -> (String, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen: Seen = Arc::default();
    let seen2 = seen.clone();
    tokio::spawn(async move {
        for (status, body) in responses {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            loop {
                let n = sock.read(&mut tmp).await.unwrap();
                buf.extend_from_slice(&tmp[..n]);
                let text = String::from_utf8_lossy(&buf).to_string();
                if let Some(idx) = text.find("\r\n\r\n") {
                    let cl = text[..idx]
                        .lines()
                        .find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap()))
                        .unwrap_or(0);
                    if buf.len() >= idx + 4 + cl {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            seen2.lock().unwrap().push(String::from_utf8_lossy(&buf).to_string());
            let resp = format!("HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
            sock.write_all(resp.as_bytes()).await.unwrap();
        }
    });
    (url, seen)
}

const TOKEN: &str = r#"{"access_token":"AT1","refresh_token":"RT1","expires_in":3600,"user":{"id":"user-1"}}"#;
const TOKEN2: &str = r#"{"access_token":"AT2","refresh_token":"RT2","expires_in":3600,"user":{"id":"user-1"}}"#;

fn entry() -> HistoryEntry {
    HistoryEntry::for_playback(&MediaRef { anilist_id: Some(154587), mal_id: Some(52991), kitsu_id: None }, "Frieren", Some("https://p/x.jpg"), 3, 812, Some(1440), time::OffsetDateTime::from_unix_timestamp(1_793_135_051).unwrap()).unwrap()
}

#[tokio::test]
async fn sign_in_then_upsert_uses_conflict_key_and_merge_duplicates() {
    let (url, seen) = stub(vec![(200, TOKEN), (201, "")]).await;
    let sb = Supabase::new(&url, "ANON").unwrap();
    let s = sb.sign_in_password("a@b.c", "pw").await.unwrap();
    assert_eq!(s.user_id, "user-1");
    sb.upsert_history(&[entry()]).await.unwrap();

    let reqs = seen.lock().unwrap().clone();
    assert!(reqs[0].starts_with("POST /auth/v1/token?grant_type=password"), "{}", reqs[0]);
    assert!(reqs[0].to_lowercase().contains("apikey: anon"));
    assert!(reqs[1].starts_with("POST /rest/v1/watch_history?on_conflict=user_id,episode_id"), "{}", reqs[1]);
    let low = reqs[1].to_lowercase();
    assert!(low.contains("prefer: resolution=merge-duplicates"));
    assert!(low.contains("authorization: bearer at1"));
    assert!(reqs[1].contains(r#""episode_id":"154587?ep=3""#));
    assert!(reqs[1].contains(r#""user_id":"user-1""#));
}

#[tokio::test]
async fn a_401_refreshes_once_then_retries() {
    let (url, seen) = stub(vec![(200, TOKEN), (401, "{}"), (200, TOKEN2), (200, "[]")]).await;
    let sb = Supabase::new(&url, "ANON").unwrap();
    sb.sign_in_password("a@b.c", "pw").await.unwrap();
    let rows = sb.select_history(30).await.unwrap();
    assert!(rows.is_empty());
    let reqs = seen.lock().unwrap().clone();
    assert!(reqs[2].starts_with("POST /auth/v1/token?grant_type=refresh_token"));
    assert!(reqs[2].contains("RT1"));
    assert!(reqs[3].starts_with("GET /rest/v1/watch_history?select=*&user_id=eq.user-1&order=watched_at.desc&limit=30"), "{}", reqs[3]);
    assert!(reqs[3].to_lowercase().contains("authorization: bearer at2"));
    assert_eq!(sb.session().unwrap().access_token, "AT2");
}

#[tokio::test]
async fn bad_password_and_not_signed_in() {
    let (url, _) = stub(vec![(400, r#"{"error":"invalid_grant"}"#)]).await;
    let sb = Supabase::new(&url, "ANON").unwrap();
    assert!(matches!(sb.sign_in_password("a", "b").await, Err(tk_core::SupabaseError::InvalidCredentials)));
    assert!(matches!(sb.upsert_history(&[entry()]).await, Err(tk_core::SupabaseError::NotSignedIn)));
}
