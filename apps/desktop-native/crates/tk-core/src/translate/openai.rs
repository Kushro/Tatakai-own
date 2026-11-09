//! Any OpenAI-compatible chat endpoint (OpenAI, DeepSeek, Ollama, LM Studio, ...): the base URL is completed to
//! `/v1/chat/completions` like the classic `completeOpenAICompatUrl`, the key is optional (local servers), the
//! model is sent when set, and a leading `<think>` block of reasoning models is stripped.

use serde_json::{Value, json};

use super::{TranslateConfig, language_name};
use crate::error::TranslateError;

/// Complete a pasted base URL to its chat-completions endpoint.
pub(super) fn chat_url(url: &str) -> String {
    let cleaned = url.trim().trim_end_matches('/');
    if cleaned.is_empty() || cleaned.ends_with("/chat/completions") {
        return cleaned.to_owned();
    }
    for tail in ["/responses", "/completions"] {
        if let Some(base) = cleaned.strip_suffix(tail)
            && is_version_segment(base)
        {
            return format!("{base}/chat/completions");
        }
    }
    if is_version_segment(cleaned) {
        return format!("{cleaned}/chat/completions");
    }
    match url::Url::parse(cleaned) {
        Ok(u) if u.path().is_empty() || u.path() == "/" => format!("{}/v1/chat/completions", cleaned),
        _ => cleaned.to_owned(),
    }
}

/// `.../v1`, `.../v2` ...
fn is_version_segment(s: &str) -> bool {
    s.rsplit('/').next().is_some_and(|last| last.len() >= 2 && last.starts_with('v') && last[1..].chars().all(|c| c.is_ascii_digit()))
}

/// Remove leading `<think>...</think>` blocks.
pub(super) fn strip_think(content: &str) -> String {
    let mut rest = content.trim_start();
    loop {
        let lower = rest.to_ascii_lowercase();
        if !lower.starts_with("<think>") {
            break;
        }
        match lower.find("</think>") {
            Some(end) => rest = rest[end + "</think>".len()..].trim_start(),
            None => break,
        }
    }
    rest.trim().to_owned()
}

pub(super) fn request_body(cfg: &TranslateConfig, text: &str, source: Option<&str>, target: &str) -> Value {
    let source_name = source.map(language_name).unwrap_or_else(|| "its original language".to_owned());
    let system = format!(
        "You are a professional subtitle translator. Translate the user's text from {source_name} into {}. Output ONLY the \
         translation, no explanations, no quotes. Keep exactly one output line per input line, in the same order.",
        language_name(target)
    );
    let max_tokens = (text.chars().count() * 2 + 256).clamp(256, 4096);
    let mut body = json!({
        "temperature": 0,
        "max_tokens": max_tokens,
        "stream": false,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": text },
        ],
    });
    let model = cfg.model.trim();
    if !model.is_empty() {
        body["model"] = Value::String(model.to_owned());
    }
    body
}

pub(super) async fn translate(client: &reqwest::Client, cfg: &TranslateConfig, text: &str, source: Option<&str>, target: &str) -> Result<String, TranslateError> {
    let mut req = client.post(chat_url(&cfg.endpoint)).json(&request_body(cfg, text, source, target));
    let key = cfg.api_key.trim();
    if !key.is_empty() {
        req = req.bearer_auth(key);
    }
    let reply: Value = req.send().await?.error_for_status()?.json().await?;
    parse(&reply)
}

/// `choices[0].message.content`; a `length` finish means the reply was cut.
pub(super) fn parse(reply: &Value) -> Result<String, TranslateError> {
    let choice = reply.get("choices").and_then(|c| c.get(0)).ok_or_else(|| TranslateError::Parse("openai: no choices".to_owned()))?;
    if choice.get("finish_reason").and_then(Value::as_str) == Some("length") {
        return Err(TranslateError::Parse("openai: reply truncated (max_tokens)".to_owned()));
    }
    let content = choice
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(Value::as_str)
        .ok_or_else(|| TranslateError::Parse("openai: no content".to_owned()))?;
    Ok(strip_think(content))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_urls_are_completed() {
        assert_eq!(chat_url("https://api.openai.com/v1"), "https://api.openai.com/v1/chat/completions");
        assert_eq!(chat_url("https://api.openai.com/v1/"), "https://api.openai.com/v1/chat/completions");
        assert_eq!(chat_url("http://127.0.0.1:11434"), "http://127.0.0.1:11434/v1/chat/completions");
        assert_eq!(chat_url("https://x.ai/v1/completions"), "https://x.ai/v1/chat/completions");
        assert_eq!(chat_url("https://x.ai/v1/chat/completions"), "https://x.ai/v1/chat/completions");
        assert_eq!(chat_url("https://x.ai/custom/path"), "https://x.ai/custom/path");
    }

    #[test]
    fn think_blocks_and_truncation() {
        assert_eq!(strip_think("<think>hmm</think>\n Hola"), "Hola");
        assert_eq!(strip_think("Hola"), "Hola");
        let ok = json!({ "choices": [{ "message": { "content": "<THINK>x</THINK>Hola\nAdiós" }, "finish_reason": "stop" }] });
        assert_eq!(parse(&ok).unwrap(), "Hola\nAdiós");
        let cut = json!({ "choices": [{ "message": { "content": "Ho" }, "finish_reason": "length" }] });
        assert!(parse(&cut).is_err());
    }

    #[test]
    fn body_names_languages_and_model() {
        let cfg = TranslateConfig { model: "gpt-4o-mini".into(), ..Default::default() };
        let b = request_body(&cfg, "Hello", Some("en"), "es");
        assert_eq!(b["model"], "gpt-4o-mini");
        assert!(b["messages"][0]["content"].as_str().unwrap().contains("into Spanish"));
        assert!(request_body(&TranslateConfig::default(), "x", None, "es").get("model").is_none());
    }
}
