//! trace.moe scene search (wave A, worker **a**; docs/redesign/15 section 4). `GET https://api.trace.moe/search?url=`
//! or `POST https://api.trace.moe/search` with the raw image body (`Content-Type: image/*`). Results carry the
//! AniList id only (`anilist` is a number without `anilistInfo`, an object with it: both are read); tk-app maps the
//! ids to titles through AniList.

use serde_json::Value;

use crate::error::ProviderError;

pub const ENDPOINT: &str = "https://api.trace.moe/search";
/// trace.moe rejects larger uploads (HTTP 413).
pub const MAX_IMAGE_BYTES: usize = 25 * 1024 * 1024;

/// What to search with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageQuery {
    Url(String),
    Bytes { data: Vec<u8>, mime: String },
}

/// One result (`result[]` of the response).
#[derive(Debug, Clone, PartialEq)]
pub struct Match {
    pub anilist_id: u32,
    pub episode: Option<u32>,
    pub from_secs: f64,
    pub to_secs: f64,
    /// 0.0..=1.0
    pub similarity: f32,
    pub image: Option<String>,
    pub video: Option<String>,
}

pub struct TraceMoe {
    client: reqwest::Client,
    endpoint: String,
}

/// `episode` is a number, a numeric string, an array (`[1, 2]`: the first) or null.
fn episode(v: &Value) -> Option<u32> {
    match v {
        Value::Number(n) => n.as_f64().filter(|f| f.is_finite() && *f >= 0.0).map(|f| f as u32),
        Value::String(s) => s.trim().parse::<f64>().ok().filter(|f| f.is_finite() && *f >= 0.0).map(|f| f as u32),
        Value::Array(a) => a.first().and_then(episode),
        _ => None,
    }
}

fn anilist_id(v: &Value) -> Option<u32> {
    let id = if v.is_object() { &v["id"] } else { v };
    id.as_u64().and_then(|n| u32::try_from(n).ok()).filter(|n| *n != 0)
}

fn url_field(v: &Value) -> Option<String> {
    v.as_str().map(str::trim).filter(|s| s.starts_with("https://") || s.starts_with("http://")).map(str::to_owned)
}

/// Parse a trace.moe response body: best match first, entries without an AniList id dropped.
pub fn parse_response(doc: &Value) -> Result<Vec<Match>, ProviderError> {
    if let Some(err) = doc["error"].as_str().map(str::trim).filter(|e| !e.is_empty()) {
        return Err(ProviderError::Parse(err.to_owned()));
    }
    let Some(list) = doc["result"].as_array() else {
        return Err(ProviderError::Parse("trace.moe: missing result".into()));
    };
    let mut out: Vec<Match> = list
        .iter()
        .filter_map(|r| {
            Some(Match {
                anilist_id: anilist_id(&r["anilist"])?,
                episode: episode(&r["episode"]),
                from_secs: r["from"].as_f64().unwrap_or(0.0).max(0.0),
                to_secs: r["to"].as_f64().unwrap_or(0.0).max(0.0),
                similarity: (r["similarity"].as_f64().unwrap_or(0.0) as f32).clamp(0.0, 1.0),
                image: url_field(&r["image"]),
                video: url_field(&r["video"]),
            })
        })
        .collect();
    out.sort_by(|a, b| b.similarity.total_cmp(&a.similarity));
    Ok(out)
}

/// The request URL: `endpoint?cutBorders=1[&url=<image url>]`.
pub fn request_url(endpoint: &str, q: &ImageQuery, cut_borders: bool) -> Result<url::Url, ProviderError> {
    let mut u = url::Url::parse(endpoint).map_err(|e| ProviderError::Parse(format!("trace.moe endpoint: {e}")))?;
    {
        let mut pairs = u.query_pairs_mut();
        if cut_borders {
            pairs.append_pair("cutBorders", "1");
        }
        if let ImageQuery::Url(image) = q {
            pairs.append_pair("url", image.trim());
        }
    }
    if u.query() == Some("") {
        u.set_query(None);
    }
    Ok(u)
}

impl TraceMoe {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client, endpoint: ENDPOINT.to_owned() }
    }

    /// Tests.
    pub fn with_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = endpoint.into();
        self
    }

    /// Best match first.
    pub async fn search(&self, q: &ImageQuery, cut_borders: bool) -> Result<Vec<Match>, ProviderError> {
        let url = request_url(&self.endpoint, q, cut_borders)?;
        let req = match q {
            ImageQuery::Url(image) => {
                let image = image.trim();
                if !(image.starts_with("https://") || image.starts_with("http://")) {
                    return Err(ProviderError::Parse("trace.moe: the image URL must start with http(s)://".into()));
                }
                self.client.get(url)
            }
            ImageQuery::Bytes { data, mime } => {
                if data.is_empty() {
                    return Err(ProviderError::Parse("trace.moe: empty image".into()));
                }
                if data.len() > MAX_IMAGE_BYTES {
                    return Err(ProviderError::Http(413));
                }
                let mime = if mime.starts_with("image/") || mime.starts_with("video/") { mime.as_str() } else { "image/jpeg" };
                self.client.post(url).header("Content-Type", mime).body(data.clone())
            }
        };
        let resp = req.header("Accept", "application/json").send().await?;
        let status = resp.status().as_u16();
        if status == 429 || status == 402 {
            return Err(ProviderError::RateLimited { provider: "trace.moe".into() });
        }
        let body: Value = resp.json().await.map_err(|e| ProviderError::Parse(e.without_url().to_string()))?;
        if !(200..300).contains(&status) {
            // trace.moe explains bad requests (invalid image, unreachable URL) in `error`.
            if let Some(err) = body["error"].as_str().filter(|e| !e.trim().is_empty()) {
                return Err(ProviderError::Parse(err.to_owned()));
            }
            return Err(ProviderError::Http(status));
        }
        parse_response(&body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_matches_best_first() {
        let doc = json!({ "frameCount": 1, "error": "", "result": [
            { "anilist": 21, "filename": "a.mp4", "episode": "12", "from": 63.5, "to": 65.1, "similarity": 0.81,
              "image": "https://media.trace.moe/i.jpg", "video": "https://media.trace.moe/v.mp4" },
            { "anilist": { "id": 1535, "title": { "romaji": "Death Note" } }, "episode": [3, 4], "from": 1.0, "to": 2.0, "similarity": 0.97 },
            { "anilist": null, "episode": 1, "similarity": 0.99 },
            { "anilist": 5, "episode": null, "from": -1.0, "to": 3.0, "similarity": 1.4, "image": "javascript:x" }
        ] });
        let m = parse_response(&doc).unwrap();
        assert_eq!(m.len(), 3);
        assert_eq!(m[0].anilist_id, 5);
        assert_eq!(m[0].similarity, 1.0);
        assert_eq!(m[0].from_secs, 0.0);
        assert_eq!(m[0].image, None);
        assert_eq!(m[1].anilist_id, 1535);
        assert_eq!(m[1].episode, Some(3));
        assert_eq!(m[2].episode, Some(12));
        assert_eq!(m[2].video.as_deref(), Some("https://media.trace.moe/v.mp4"));
    }

    #[test]
    fn errors_are_reported() {
        assert!(parse_response(&json!({ "error": "Failed to fetch image" })).is_err());
        assert!(parse_response(&json!({})).is_err());
        assert!(parse_response(&json!({ "result": [] })).unwrap().is_empty());
    }

    #[test]
    fn request_url_carries_the_image_url() {
        let u = request_url(ENDPOINT, &ImageQuery::Url(" https://x.test/a b.png ".into()), true).unwrap();
        assert_eq!(u.as_str(), "https://api.trace.moe/search?cutBorders=1&url=https%3A%2F%2Fx.test%2Fa+b.png");
        let u = request_url(ENDPOINT, &ImageQuery::Bytes { data: vec![1], mime: "image/png".into() }, false).unwrap();
        assert_eq!(u.as_str(), ENDPOINT);
    }
}
