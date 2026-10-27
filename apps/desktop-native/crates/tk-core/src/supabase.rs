//! Minimal Supabase client (GoTrue password auth + PostgREST `watch_history`).
//! Contract: `contracts/supabase/m0-tables.md`. The session is sent ONLY to the configured Supabase URL.

use std::sync::RwLock;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::error::SupabaseError;
use crate::model::HistoryEntry;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Session {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix seconds at which the access token expires.
    pub expires_at: i64,
    pub user_id: String,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session").field("user_id", &self.user_id).field("expires_at", &self.expires_at).finish_non_exhaustive()
    }
}

pub struct Supabase {
    http: reqwest::Client,
    base: url::Url,
    anon_key: String,
    session: RwLock<Option<Session>>,
}

impl Supabase {
    /// `url` must be https (or http to a loopback host, for tests).
    pub fn new(url: &str, anon_key: &str) -> Result<Self, SupabaseError> {
        Self::with_client(reqwest::Client::new(), url, anon_key)
    }

    pub fn with_client(http: reqwest::Client, url: &str, anon_key: &str) -> Result<Self, SupabaseError> {
        let mut base = url::Url::parse(url.trim_end_matches('/')).map_err(|e| SupabaseError::BadUrl(e.to_string()))?;
        let loopback = matches!(base.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
        if base.scheme() != "https" && !(base.scheme() == "http" && loopback) {
            return Err(SupabaseError::BadUrl("https required".into()));
        }
        if !base.path().ends_with('/') {
            let p = format!("{}/", base.path());
            base.set_path(&p);
        }
        Ok(Self { http, base, anon_key: anon_key.to_owned(), session: RwLock::new(None) })
    }

    pub fn session(&self) -> Option<Session> {
        self.session.read().ok().and_then(|s| s.clone())
    }

    pub fn set_session(&self, s: Option<Session>) {
        if let Ok(mut g) = self.session.write() {
            *g = s;
        }
    }

    pub fn sign_out(&self) {
        self.set_session(None);
    }

    fn endpoint(&self, path: &str) -> url::Url {
        // `base` always ends with '/', so join cannot escape the Supabase origin for relative paths.
        self.base.join(path).unwrap_or_else(|_| self.base.clone())
    }

    fn parse_session(body: &Value, now: i64) -> Result<Session, SupabaseError> {
        let bad = |what: &str| SupabaseError::Parse(format!("missing {what}"));
        let expires_at = body["expires_at"]
            .as_i64()
            .or_else(|| body["expires_in"].as_i64().map(|s| now + s))
            .ok_or_else(|| bad("expires"))?;
        Ok(Session {
            access_token: body["access_token"].as_str().ok_or_else(|| bad("access_token"))?.to_owned(),
            refresh_token: body["refresh_token"].as_str().ok_or_else(|| bad("refresh_token"))?.to_owned(),
            expires_at,
            user_id: body["user"]["id"].as_str().ok_or_else(|| bad("user.id"))?.to_owned(),
        })
    }

    async fn token_request(&self, grant: &str, body: Value) -> Result<Session, SupabaseError> {
        let mut url = self.endpoint("auth/v1/token");
        url.set_query(Some(&format!("grant_type={grant}")));
        let resp = self.http.post(url).header("apikey", &self.anon_key).json(&body).send().await?;
        match resp.status().as_u16() {
            400 | 401 | 422 => Err(if grant == "password" { SupabaseError::InvalidCredentials } else { SupabaseError::SessionExpired }),
            s if !(200..300).contains(&s) => Err(SupabaseError::Http(s)),
            _ => {
                let v: Value = resp.json().await.map_err(|e| SupabaseError::Parse(e.without_url().to_string()))?;
                Self::parse_session(&v, OffsetDateTime::now_utc().unix_timestamp())
            }
        }
    }

    pub async fn sign_in_password(&self, email: &str, password: &str) -> Result<Session, SupabaseError> {
        let s = self.token_request("password", json!({ "email": email, "password": password })).await?;
        self.set_session(Some(s.clone()));
        Ok(s)
    }

    pub async fn refresh(&self) -> Result<Session, SupabaseError> {
        let cur = self.session().ok_or(SupabaseError::NotSignedIn)?;
        match self.token_request("refresh_token", json!({ "refresh_token": cur.refresh_token })).await {
            Ok(s) => {
                self.set_session(Some(s.clone()));
                Ok(s)
            }
            Err(SupabaseError::SessionExpired) => {
                self.set_session(None);
                Err(SupabaseError::SessionExpired)
            }
            Err(e) => Err(e),
        }
    }

    /// Refresh when the token expires within `leeway_secs`.
    async fn fresh_session(&self, leeway_secs: i64) -> Result<Session, SupabaseError> {
        let cur = self.session().ok_or(SupabaseError::NotSignedIn)?;
        if cur.expires_at - OffsetDateTime::now_utc().unix_timestamp() <= leeway_secs {
            return self.refresh().await;
        }
        Ok(cur)
    }

    /// JSON body of one `watch_history` row (E9): `mal_id`/`anilist_id` only when known.
    pub fn history_row(user_id: &str, e: &HistoryEntry) -> Value {
        let mut row = json!({
            "user_id": user_id,
            "anime_id": e.anime_id,
            "anime_name": e.title,
            "anime_poster": e.poster.clone().unwrap_or_default(),
            "episode_id": e.episode_id,
            "episode_number": e.episode,
            "progress_seconds": e.progress_sec,
            "duration_seconds": e.duration_sec,
            "completed": e.completed,
            "watched_at": e.watched_at.to_offset(time::UtcOffset::UTC).format(&Rfc3339).unwrap_or_default(),
        });
        if let Some(a) = e.anilist_id {
            row["anilist_id"] = a.into();
        }
        if let Some(m) = e.mal_id {
            row["mal_id"] = m.into();
        }
        row
    }

    pub fn entry_from_row(r: &Value) -> Option<HistoryEntry> {
        let watched_at = OffsetDateTime::parse(r["watched_at"].as_str()?, &Rfc3339).ok()?;
        let n = |k: &str| r[k].as_u64().and_then(|v| u32::try_from(v).ok());
        Some(HistoryEntry {
            anime_id: r["anime_id"].as_str()?.to_owned(),
            title: r["anime_name"].as_str().unwrap_or("Unknown Anime").to_owned(),
            poster: r["anime_poster"].as_str().filter(|s| !s.is_empty()).map(str::to_owned),
            episode_id: r["episode_id"].as_str()?.to_owned(),
            episode: n("episode_number")?,
            progress_sec: n("progress_seconds").unwrap_or(0),
            duration_sec: n("duration_seconds").filter(|d| *d > 0),
            completed: r["completed"].as_bool().unwrap_or(false),
            watched_at,
            anilist_id: n("anilist_id"),
            mal_id: n("mal_id"),
        })
    }

    async fn rest(&self, build: impl Fn(&str) -> reqwest::RequestBuilder) -> Result<reqwest::Response, SupabaseError> {
        let mut sess = self.fresh_session(30).await?;
        for attempt in 0..2 {
            let resp = build(&sess.access_token)
                .header("apikey", &self.anon_key)
                .bearer_auth(&sess.access_token)
                .send()
                .await?;
            if resp.status().as_u16() == 401 && attempt == 0 {
                sess = self.refresh().await?;
                continue;
            }
            return match resp.status().as_u16() {
                401 => Err(SupabaseError::SessionExpired),
                s if !(200..300).contains(&s) => Err(SupabaseError::Http(s)),
                _ => Ok(resp),
            };
        }
        Err(SupabaseError::SessionExpired)
    }

    /// `POST /rest/v1/watch_history?on_conflict=user_id,episode_id` with `Prefer: resolution=merge-duplicates`.
    pub async fn upsert_history(&self, entries: &[HistoryEntry]) -> Result<(), SupabaseError> {
        if entries.is_empty() {
            return Ok(());
        }
        let uid = self.session().ok_or(SupabaseError::NotSignedIn)?.user_id;
        let rows: Vec<Value> = entries.iter().map(|e| Self::history_row(&uid, e)).collect();
        let mut url = self.endpoint("rest/v1/watch_history");
        url.set_query(Some("on_conflict=user_id,episode_id"));
        self.rest(|_| {
            self.http
                .post(url.clone())
                .header("Prefer", "resolution=merge-duplicates,return=minimal")
                .json(&rows)
        })
        .await?;
        Ok(())
    }

    /// `GET /rest/v1/watch_history?select=*&user_id=eq.<uid>&order=watched_at.desc&limit=N`
    pub async fn select_history(&self, limit: u32) -> Result<Vec<HistoryEntry>, SupabaseError> {
        let uid = self.session().ok_or(SupabaseError::NotSignedIn)?.user_id;
        let mut url = self.endpoint("rest/v1/watch_history");
        url.query_pairs_mut()
            .append_pair("select", "*")
            .append_pair("user_id", &format!("eq.{uid}"))
            .append_pair("order", "watched_at.desc")
            .append_pair("limit", &limit.to_string());
        let resp = self.rest(|_| self.http.get(url.clone())).await?;
        let rows: Vec<Value> = resp.json().await.map_err(|e| SupabaseError::Parse(e.without_url().to_string()))?;
        Ok(rows.iter().filter_map(Self::entry_from_row).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::MediaRef;

    fn entry() -> HistoryEntry {
        HistoryEntry::for_playback(
            &MediaRef { anilist_id: Some(154587), mal_id: None, kitsu_id: Some(9) },
            "Frieren",
            None,
            3,
            812,
            Some(1440),
            OffsetDateTime::from_unix_timestamp(1_793_135_051).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn row_follows_e9_key_format_and_omits_unknown_ids() {
        let row = Supabase::history_row("u1", &entry());
        assert_eq!(row["anime_id"], "154587");
        assert_eq!(row["episode_id"], "154587?ep=3");
        assert_eq!(row["anilist_id"], 154587);
        assert!(row.get("mal_id").is_none());
        assert_eq!(row["anime_poster"], "");
        assert_eq!(row["completed"], false);
        assert_eq!(row["watched_at"], "2026-10-27T21:04:11Z");
    }

    #[test]
    fn mal_only_uses_mal_prefix_and_ninety_percent_completes() {
        let e = HistoryEntry::for_playback(
            &MediaRef { anilist_id: None, mal_id: Some(52991), kitsu_id: None },
            "x",
            Some("p"),
            1,
            90,
            Some(100),
            OffsetDateTime::UNIX_EPOCH,
        )
        .unwrap();
        assert_eq!(e.anime_id, "mal:52991");
        assert_eq!(e.episode_id, "mal:52991?ep=1");
        assert!(e.completed);
        assert!(HistoryEntry::for_playback(&MediaRef { anilist_id: None, mal_id: None, kitsu_id: Some(1) }, "x", None, 1, 0, None, OffsetDateTime::UNIX_EPOCH).is_none());
    }

    #[test]
    fn rejects_plain_http_to_remote_hosts() {
        assert!(Supabase::new("http://example.com", "k").is_err());
        assert!(Supabase::new("http://127.0.0.1:1", "k").is_ok());
        assert!(Supabase::new("https://x.supabase.co", "k").is_ok());
    }

    #[test]
    fn round_trips_a_row() {
        let e = entry();
        let back = Supabase::entry_from_row(&Supabase::history_row("u1", &e)).unwrap();
        assert_eq!(back.episode_id, e.episode_id);
        assert_eq!(back.progress_sec, 812);
        assert_eq!(back.anilist_id, Some(154587));
    }
}
