//! DeepL (`/v2/translate`, key in `Authorization: DeepL-Auth-Key`; `:fx` keys use the free host) and DeepLX
//! (self-hosted DeepL proxy: one text, `{ "data": "..." }` back). Ported from the classic `providers.ts`.

use serde_json::{Value, json};

use super::TranslateConfig;
use crate::error::TranslateError;

const FREE_ENDPOINT: &str = "https://api-free.deepl.com/v2/translate";
const PRO_ENDPOINT: &str = "https://api.deepl.com/v2/translate";

/// DeepL target codes (classic `DEEPL_TARGET_MAP`).
pub(super) fn target_code(lang: &str) -> String {
    match lang.to_ascii_lowercase().as_str() {
        "en" => "EN-US".to_owned(),
        "pt" => "PT-PT".to_owned(),
        "zh" => "ZH-HANS".to_owned(),
        other => other.to_ascii_uppercase(),
    }
}

/// The configured endpoint, else the free host for `:fx` keys and the pro host otherwise.
pub(super) fn endpoint(cfg: &TranslateConfig) -> String {
    let custom = cfg.endpoint.trim().trim_end_matches('/');
    if !custom.is_empty() {
        return custom.to_owned();
    }
    if cfg.api_key.trim().ends_with(":fx") { FREE_ENDPOINT.to_owned() } else { PRO_ENDPOINT.to_owned() }
}

pub(super) async fn translate(client: &reqwest::Client, cfg: &TranslateConfig, lines: &[String], source: Option<&str>, target: &str) -> Result<Vec<String>, TranslateError> {
    let mut body = json!({ "text": lines, "target_lang": target_code(target) });
    if let Some(src) = source {
        body["source_lang"] = Value::String(src.to_ascii_uppercase());
    }
    let reply: Value = client
        .post(endpoint(cfg))
        .header("Authorization", format!("DeepL-Auth-Key {}", cfg.api_key.trim()))
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    parse(&reply)
}

/// `{ "translations": [{ "text": ".." }, ..] }`.
pub(super) fn parse(reply: &Value) -> Result<Vec<String>, TranslateError> {
    let list = reply.get("translations").and_then(Value::as_array).ok_or_else(|| TranslateError::Parse("deepl: no translations".to_owned()))?;
    list.iter()
        .map(|t| t.get("text").and_then(Value::as_str).map(str::to_owned).ok_or_else(|| TranslateError::Parse("deepl: missing text".to_owned())))
        .collect()
}

pub(super) async fn translate_x(client: &reqwest::Client, cfg: &TranslateConfig, text: &str, source: Option<&str>, target: &str) -> Result<String, TranslateError> {
    let mut body = json!({ "text": text, "target_lang": target_code(target) });
    body["source_lang"] = Value::String(source.map(str::to_ascii_uppercase).unwrap_or_else(|| "auto".to_owned()));
    let reply: Value = client
        .post(cfg.endpoint.trim())
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    parse_x(&reply)
}

/// `{ "code": 200, "data": ".." }`.
pub(super) fn parse_x(reply: &Value) -> Result<String, TranslateError> {
    reply.get("data").and_then(Value::as_str).map(str::to_owned).ok_or_else(|| TranslateError::Parse("deeplx: no data".to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_follows_the_key_kind_unless_overridden() {
        let mut cfg = TranslateConfig { api_key: "abc:fx".into(), ..Default::default() };
        assert_eq!(endpoint(&cfg), FREE_ENDPOINT);
        cfg.api_key = "abc".into();
        assert_eq!(endpoint(&cfg), PRO_ENDPOINT);
        cfg.endpoint = "https://deepl.example/v2/translate/".into();
        assert_eq!(endpoint(&cfg), "https://deepl.example/v2/translate");
    }

    #[test]
    fn replies_parse_and_codes_map() {
        let r = json!({ "translations": [{ "text": "Hola" }, { "text": "Adiós" }] });
        assert_eq!(parse(&r).unwrap(), vec!["Hola".to_owned(), "Adiós".to_owned()]);
        assert!(parse(&json!({ "translations": [{}] })).is_err());
        assert_eq!(parse_x(&json!({ "code": 200, "data": "Hola" })).unwrap(), "Hola");
        assert!(parse_x(&json!({ "code": 500 })).is_err());
        assert_eq!(target_code("en"), "EN-US");
        assert_eq!(target_code("es"), "ES");
    }
}
