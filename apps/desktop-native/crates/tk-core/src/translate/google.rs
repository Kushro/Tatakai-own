//! Google Translate through the public `translate_a/single` endpoint (`client=gtx`): no key, the same backend the
//! classic "Google (free)" engine used. One newline-joined text per request; the reply comes back as sentence
//! segments whose concatenation keeps the newlines.

use serde_json::Value;

use crate::error::TranslateError;

const ENDPOINT: &str = "https://translate.googleapis.com/translate_a/single";

/// Google's own codes for the few languages whose ISO 639-1 code differs.
pub(super) fn code(lang: &str) -> String {
    match lang.to_ascii_lowercase().as_str() {
        "zh" => "zh-CN".to_owned(),
        "he" => "iw".to_owned(),
        other => other.to_owned(),
    }
}

pub(super) async fn translate(client: &reqwest::Client, text: &str, source: Option<&str>, target: &str) -> Result<String, TranslateError> {
    let sl = source.map(code).unwrap_or_else(|| "auto".to_owned());
    let tl = code(target);
    let body: Value = client
        .get(ENDPOINT)
        .query(&[("client", "gtx"), ("sl", sl.as_str()), ("tl", tl.as_str()), ("dt", "t"), ("q", text)])
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    parse(&body)
}

/// `[[["Hola\n","Hello\n",..],["mundo","world",..]],null,"en",..]` -> `"Hola\nmundo"`.
pub(super) fn parse(body: &Value) -> Result<String, TranslateError> {
    let segments = body.get(0).and_then(Value::as_array).ok_or_else(|| TranslateError::Parse("google: no segments".to_owned()))?;
    let mut out = String::new();
    for seg in segments {
        if let Some(s) = seg.get(0).and_then(Value::as_str) {
            out.push_str(s);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_are_concatenated_in_order() {
        let body: Value = serde_json::json!([[["Hola\n", "Hello\n", null, null, 10], ["mundo", "world", null, null, 10]], null, "en"]);
        assert_eq!(parse(&body).unwrap(), "Hola\nmundo");
        assert!(parse(&serde_json::json!({"error": 1})).is_err());
        assert_eq!(code("ZH"), "zh-CN");
        assert_eq!(code("es"), "es");
    }
}
