//! Azure Translator (`/translate?api-version=3.0`, key + region headers, one array element per line). Ported from
//! the classic `providers.ts`; `endpoint` overrides the global host (sovereign clouds, custom domains).

use serde_json::{Value, json};

use super::TranslateConfig;
use crate::error::TranslateError;

const ENDPOINT: &str = "https://api.cognitive.microsofttranslator.com/translate";

/// Azure codes for the few languages whose ISO 639-1 code differs (classic `AZURE_LANG_MAP`).
pub(super) fn code(lang: &str) -> String {
    match lang.to_ascii_lowercase().as_str() {
        "zh" => "zh-Hans".to_owned(),
        "no" => "nb".to_owned(),
        "tl" => "fil".to_owned(),
        other => other.to_owned(),
    }
}

pub(super) fn endpoint(cfg: &TranslateConfig) -> String {
    let custom = cfg.endpoint.trim().trim_end_matches('/');
    if custom.is_empty() {
        ENDPOINT.to_owned()
    } else if custom.ends_with("/translate") {
        custom.to_owned()
    } else {
        format!("{custom}/translate")
    }
}

pub(super) async fn translate(client: &reqwest::Client, cfg: &TranslateConfig, lines: &[String], source: Option<&str>, target: &str) -> Result<Vec<String>, TranslateError> {
    let body: Vec<Value> = lines.iter().map(|l| json!({ "Text": l })).collect();
    let to = code(target);
    let mut query: Vec<(&str, String)> = vec![("api-version", "3.0".to_owned()), ("to", to)];
    if let Some(src) = source {
        query.push(("from", code(src)));
    }
    let reply: Value = client
        .post(endpoint(cfg))
        .query(&query)
        .header("Ocp-Apim-Subscription-Key", cfg.api_key.trim())
        .header("Ocp-Apim-Subscription-Region", cfg.region.trim())
        .json(&body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    parse(&reply)
}

/// `[{ "translations": [{ "text": "..", "to": "es" }] }, ..]`.
pub(super) fn parse(reply: &Value) -> Result<Vec<String>, TranslateError> {
    let list = reply.as_array().ok_or_else(|| TranslateError::Parse("azure: not an array".to_owned()))?;
    list.iter()
        .map(|item| {
            item.get("translations")
                .and_then(|t| t.get(0))
                .and_then(|t| t.get("text"))
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| TranslateError::Parse("azure: missing text".to_owned()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reply_and_endpoint() {
        let r = json!([{ "translations": [{ "text": "Hola", "to": "es" }] }, { "translations": [{ "text": "Sí", "to": "es" }] }]);
        assert_eq!(parse(&r).unwrap(), vec!["Hola".to_owned(), "Sí".to_owned()]);
        assert!(parse(&json!({ "error": {} })).is_err());
        assert_eq!(endpoint(&TranslateConfig::default()), ENDPOINT);
        let cfg = TranslateConfig { endpoint: "https://my.cognitiveservices.azure.com/".into(), ..Default::default() };
        assert_eq!(endpoint(&cfg), "https://my.cognitiveservices.azure.com/translate");
        assert_eq!(code("zh"), "zh-Hans");
    }
}
