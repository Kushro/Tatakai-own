//! Cloud narration (`novel.tts.openai`, `novel.tts.openai-compatible`, `novel.tts.elevenlabs`; classic
//! `TtsIntegrationSettings.tsx`): plain `reqwest` calls with the user's own key. OpenAI and any server speaking its
//! `POST <base>/audio/speech` (local Kokoro / Piper / openedai-speech servers), ElevenLabs
//! `POST /v1/text-to-speech/<voice_id>`.

use std::time::Duration;

use serde_json::{Value, json};
use tk_ui::backend::{TtsVoiceVm, UiResult};
use tk_ui::model::{ErrorKind, UiError};

use super::{err, net_err};

pub const OPENAI_BASE: &str = "https://api.openai.com/v1";
const ELEVEN_BASE: &str = "https://api.elevenlabs.io/v1";
/// OpenAI's built-in voices (also what most compatible servers accept).
pub const OPENAI_VOICES: [&str; 11] = [
    "alloy", "ash", "ballad", "coral", "echo", "fable", "nova", "onyx", "sage", "shimmer", "verse",
];
/// ElevenLabs "Rachel", their documented default voice.
pub const ELEVEN_DEFAULT_VOICE: &str = "21m00Tcm4TlvDq8ikWAM";
const SYNTH_TIMEOUT: Duration = Duration::from_secs(90);

pub fn openai_voices() -> Vec<TtsVoiceVm> {
    OPENAI_VOICES
        .iter()
        .map(|v| TtsVoiceVm {
            id: (*v).to_owned(),
            name: capitalize(v),
            lang: None,
            gender: None,
        })
        .collect()
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

/// `<base>/audio/speech` for an OpenAI-style base URL (`.../v1`); a base that already names the route is kept.
pub fn speech_url(base: &str) -> String {
    let b = base.trim().trim_end_matches('/');
    if b.ends_with("/audio/speech") {
        b.to_owned()
    } else {
        format!("{b}/audio/speech")
    }
}

/// OpenAI `speed` (0.25..=4) of a rate percent.
pub fn openai_speed(rate: u8) -> f32 {
    (f32::from(rate) / 100.0).clamp(0.25, 4.0)
}

/// ElevenLabs `voice_settings.speed` (0.7..=1.2) of a rate percent.
pub fn eleven_speed(rate: u8) -> f32 {
    (f32::from(rate) / 100.0).clamp(0.7, 1.2)
}

/// Body of an OpenAI-style speech request.
pub fn openai_body(text: &str, voice: Option<&str>, rate: u8, model: Option<&str>) -> Value {
    json!({
        "model": model.filter(|m| !m.trim().is_empty()).unwrap_or("tts-1"),
        "input": text,
        "voice": voice.unwrap_or("alloy"),
        "response_format": "mp3",
        "speed": openai_speed(rate),
    })
}

async fn fail(resp: reqwest::Response) -> UiError {
    let status = resp.status().as_u16();
    let body = resp.text().await.unwrap_or_default();
    let detail: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    let msg = detail["error"]["message"]
        .as_str()
        .or_else(|| detail["detail"]["message"].as_str())
        .or_else(|| detail["detail"].as_str())
        .map(str::to_owned)
        .unwrap_or_else(|| body.chars().take(200).collect());
    let kind = match status {
        401 | 403 => ErrorKind::Auth,
        429 => ErrorKind::RateLimited,
        _ => ErrorKind::Other,
    };
    UiError::new(kind, format!("HTTP {status}: {msg}"))
}

pub async fn openai_speech(
    client: &reqwest::Client,
    base: &str,
    api_key: Option<&str>,
    text: &str,
    voice: Option<&str>,
    rate: u8,
    model: Option<&str>,
) -> UiResult<Vec<u8>> {
    let mut req = client
        .post(speech_url(base))
        .timeout(SYNTH_TIMEOUT)
        .json(&openai_body(text, voice, rate, model));
    if let Some(k) = api_key {
        req = req.bearer_auth(k.trim());
    }
    let resp = req.send().await.map_err(net_err)?;
    if !resp.status().is_success() {
        return Err(fail(resp).await);
    }
    Ok(resp.bytes().await.map_err(net_err)?.to_vec())
}

/// Voices of an OpenAI-compatible server: its `GET <base>/audio/voices` (`{ voices: [..] }` of strings or
/// `{ id, name }` objects) when it answers, else OpenAI's list.
pub async fn compatible_voices(
    client: &reqwest::Client,
    base: Option<&str>,
    api_key: Option<&str>,
) -> Vec<TtsVoiceVm> {
    let Some(base) = base.map(str::trim).filter(|b| !b.is_empty()) else {
        return openai_voices();
    };
    let url = format!(
        "{}/audio/voices",
        base.trim_end_matches('/').trim_end_matches("/audio/speech")
    );
    let mut req = client.get(url).timeout(Duration::from_secs(8));
    if let Some(k) = api_key.filter(|k| !k.trim().is_empty()) {
        req = req.bearer_auth(k.trim());
    }
    let Ok(resp) = req.send().await else {
        return openai_voices();
    };
    if !resp.status().is_success() {
        return openai_voices();
    }
    let Ok(body) = resp.json::<Value>().await else {
        return openai_voices();
    };
    let list = parse_compatible_voices(&body);
    if list.is_empty() { openai_voices() } else { list }
}

pub fn parse_compatible_voices(body: &Value) -> Vec<TtsVoiceVm> {
    let arr = body["voices"]
        .as_array()
        .or_else(|| body.as_array())
        .cloned()
        .unwrap_or_default();
    arr.iter()
        .filter_map(|v| {
            let id = v.as_str().map(str::to_owned).or_else(|| {
                v["id"]
                    .as_str()
                    .or_else(|| v["voice_id"].as_str())
                    .map(str::to_owned)
            })?;
            let name = v["name"]
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| id.clone());
            Some(TtsVoiceVm {
                id,
                name,
                lang: v["language"].as_str().map(str::to_owned),
                gender: None,
            })
        })
        .collect()
}

pub async fn elevenlabs_voices(client: &reqwest::Client, key: &str) -> UiResult<Vec<TtsVoiceVm>> {
    let resp = client
        .get(format!("{ELEVEN_BASE}/voices"))
        .header("xi-api-key", key.trim())
        .send()
        .await
        .map_err(net_err)?;
    if !resp.status().is_success() {
        return Err(fail(resp).await);
    }
    let body: Value = resp.json().await.map_err(net_err)?;
    Ok(parse_eleven_voices(&body))
}

pub fn parse_eleven_voices(body: &Value) -> Vec<TtsVoiceVm> {
    body["voices"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|v| {
            let id = v["voice_id"].as_str()?.to_owned();
            let name = v["name"].as_str().unwrap_or(&id).to_owned();
            let labels = &v["labels"];
            Some(TtsVoiceVm {
                id,
                name,
                lang: labels["language"]
                    .as_str()
                    .or_else(|| labels["accent"].as_str())
                    .map(str::to_owned),
                gender: labels["gender"].as_str().map(str::to_owned),
            })
        })
        .collect()
}

pub async fn elevenlabs_speech(
    client: &reqwest::Client,
    key: &str,
    text: &str,
    voice: Option<&str>,
    rate: u8,
    model: Option<&str>,
) -> UiResult<Vec<u8>> {
    let voice = voice
        .filter(|v| !v.trim().is_empty())
        .unwrap_or(ELEVEN_DEFAULT_VOICE);
    if voice.contains(['/', '?', '#']) {
        return Err(err("invalid ElevenLabs voice id"));
    }
    let body = json!({
        "text": text,
        "model_id": model.filter(|m| !m.trim().is_empty()).unwrap_or("eleven_multilingual_v2"),
        "voice_settings": { "stability": 0.5, "similarity_boost": 0.75, "speed": eleven_speed(rate) },
    });
    let resp = client
        .post(format!(
            "{ELEVEN_BASE}/text-to-speech/{voice}?output_format=mp3_44100_128"
        ))
        .timeout(SYNTH_TIMEOUT)
        .header("xi-api-key", key.trim())
        .header("Accept", "audio/mpeg")
        .json(&body)
        .send()
        .await
        .map_err(net_err)?;
    if !resp.status().is_success() {
        return Err(fail(resp).await);
    }
    Ok(resp.bytes().await.map_err(net_err)?.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_and_speeds() {
        assert_eq!(
            speech_url("http://127.0.0.1:8880/v1/"),
            "http://127.0.0.1:8880/v1/audio/speech"
        );
        assert_eq!(speech_url("http://h/v1/audio/speech"), "http://h/v1/audio/speech");
        assert_eq!(openai_speed(100), 1.0);
        assert_eq!(openai_speed(200), 2.0);
        assert_eq!(eleven_speed(200), 1.2);
        assert_eq!(eleven_speed(50), 0.7);
        let b = openai_body("hi", None, 150, Some(" "));
        assert_eq!(b["model"], "tts-1");
        assert_eq!(b["voice"], "alloy");
        assert_eq!(b["speed"], 1.5);
    }

    #[test]
    fn voice_lists() {
        assert_eq!(openai_voices().len(), OPENAI_VOICES.len());
        assert_eq!(openai_voices()[0].name, "Alloy");
        let c = parse_compatible_voices(&json!({ "voices": ["af_bella", { "id": "x", "name": "X voice" }] }));
        assert_eq!(
            c.iter().map(|v| v.id.as_str()).collect::<Vec<_>>(),
            vec!["af_bella", "x"]
        );
        assert_eq!(c[1].name, "X voice");
        let e = parse_eleven_voices(
            &json!({ "voices": [{ "voice_id": "abc", "name": "Rachel", "labels": { "gender": "female", "accent": "american" } }] }),
        );
        assert_eq!(e[0].id, "abc");
        assert_eq!(e[0].gender.as_deref(), Some("female"));
        assert_eq!(e[0].lang.as_deref(), Some("american"));
    }
}
