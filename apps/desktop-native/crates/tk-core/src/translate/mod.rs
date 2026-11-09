//! Subtitle translation providers (wave A, worker **d**; docs/redesign/15 section 4): Google (public
//! `translate.googleapis.com/translate_a/single` endpoint, no key), DeepL (`api-free.deepl.com` / `api.deepl.com`,
//! key decides), DeepLX (self-hosted endpoint), Azure Translator (`api.cognitive.microsofttranslator.com`, key +
//! region) and any OpenAI-compatible chat endpoint (`endpoint` + `model` + optional key).
//! Behaviour ported from the classic `src/lib/translation/{providers,translate}.ts`: batches of cues, one output
//! per input line, line-count verification with a per-line fallback for the providers that take one joined text,
//! blank lines passed through untouched. Caching lives in tk-app (`LocalStore::translation_*`, keyed by
//! [`cache_key`]).

mod azure;
mod deepl;
mod google;
mod openai;

use crate::error::TranslateError;

/// Mirror of the UI `TranslationProviderVm`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Provider {
    #[default]
    Off,
    Google,
    DeepL,
    DeepLx,
    Azure,
    OpenAi,
}

impl Provider {
    pub fn id(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Google => "google",
            Self::DeepL => "deepl",
            Self::DeepLx => "deeplx",
            Self::Azure => "azure",
            Self::OpenAi => "openai",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        [Self::Off, Self::Google, Self::DeepL, Self::DeepLx, Self::Azure, Self::OpenAi].into_iter().find(|p| p.id() == id)
    }

    /// Most lines per request.
    fn max_lines(self) -> usize {
        match self {
            // Azure takes up to 100 array elements, DeepL 50.
            Self::Azure => 100,
            Self::OpenAi => 40,
            _ => 50,
        }
    }

    /// Most characters per request. Google goes through a GET query string, so it stays well under URL limits.
    fn max_chars(self) -> usize {
        match self {
            Self::Google => 1800,
            Self::OpenAi => 4000,
            _ => 5000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TranslateConfig {
    pub provider: Provider,
    /// Base URL for DeepLX / OpenAI-compatible; empty = provider default.
    pub endpoint: String,
    pub api_key: String,
    pub model: String,
    pub region: String,
}

impl TranslateConfig {
    /// `Err(NotConfigured)` when a required key / endpoint / region is missing.
    pub fn check(&self) -> Result<(), TranslateError> {
        let missing = |what: &str| Err(TranslateError::NotConfigured(format!("{} ({what})", self.provider.id())));
        match self.provider {
            Provider::Off => Err(TranslateError::Off),
            Provider::Google => Ok(()),
            Provider::DeepL if self.api_key.trim().is_empty() => missing("key"),
            Provider::DeepLx if self.endpoint.trim().is_empty() => missing("endpoint"),
            Provider::Azure if self.api_key.trim().is_empty() => missing("key"),
            Provider::Azure if self.region.trim().is_empty() => missing("region"),
            Provider::OpenAi if self.endpoint.trim().is_empty() => missing("endpoint"),
            _ => Ok(()),
        }
    }
}

/// Translate `texts` (one cue each; inline tags preserved) from `source` (`None` = detect) into `target`
/// (ISO 639-1). Returns one string per input, same order.
pub async fn translate(client: &reqwest::Client, cfg: &TranslateConfig, texts: &[String], source: Option<&str>, target: &str) -> Result<Vec<String>, TranslateError> {
    cfg.check()?;
    let target = target.trim();
    if target.is_empty() {
        return Err(TranslateError::NotConfigured("target language".to_owned()));
    }
    let source = source.map(str::trim).filter(|s| !s.is_empty() && *s != "auto");
    let mut out = Vec::with_capacity(texts.len());
    for range in batches(texts, cfg.provider.max_lines(), cfg.provider.max_chars()) {
        out.extend(translate_batch(client, cfg, &texts[range], source, target).await?);
    }
    Ok(out)
}

/// One request worth of lines: blank lines are kept as they are, the rest go to the provider.
async fn translate_batch(client: &reqwest::Client, cfg: &TranslateConfig, batch: &[String], source: Option<&str>, target: &str) -> Result<Vec<String>, TranslateError> {
    let sent: Vec<(usize, String)> =
        batch.iter().enumerate().filter(|(_, s)| !s.trim().is_empty()).map(|(i, s)| (i, s.clone())).collect();
    let mut out: Vec<String> = batch.to_vec();
    if sent.is_empty() {
        return Ok(out);
    }
    let lines: Vec<String> = sent.iter().map(|(_, s)| s.clone()).collect();
    let translated = match cfg.provider {
        Provider::Off => return Err(TranslateError::Off),
        // Array APIs: one element per line, no joining.
        Provider::DeepL => deepl::translate(client, cfg, &lines, source, target).await?,
        Provider::Azure => azure::translate(client, cfg, &lines, source, target).await?,
        // One-text APIs: joined with newlines, verified, per-line fallback.
        Provider::Google | Provider::DeepLx | Provider::OpenAi => joined(client, cfg, &lines, source, target).await?,
    };
    if translated.len() != lines.len() {
        return Err(TranslateError::Parse(format!("{} lines in, {} out", lines.len(), translated.len())));
    }
    for ((i, _), text) in sent.iter().zip(translated) {
        out[*i] = text;
    }
    Ok(out)
}

/// Single-text providers: newline-joined request, split back; a mismatched line count retries line by line.
async fn joined(client: &reqwest::Client, cfg: &TranslateConfig, lines: &[String], source: Option<&str>, target: &str) -> Result<Vec<String>, TranslateError> {
    let flat: Vec<String> = lines.iter().map(|l| flatten(l)).collect();
    let text = flat.join("\n");
    let reply = single(client, cfg, &text, source, target).await?;
    let parts = split_lines(&reply);
    if parts.len() == flat.len() {
        return Ok(parts);
    }
    tracing::debug!(sent = flat.len(), got = parts.len(), provider = cfg.provider.id(), "line count mismatch; translating line by line");
    let mut out = Vec::with_capacity(flat.len());
    for line in &flat {
        out.push(single(client, cfg, line, source, target).await?.trim().to_owned());
    }
    Ok(out)
}

async fn single(client: &reqwest::Client, cfg: &TranslateConfig, text: &str, source: Option<&str>, target: &str) -> Result<String, TranslateError> {
    match cfg.provider {
        Provider::Google => google::translate(client, text, source, target).await,
        Provider::DeepLx => deepl::translate_x(client, cfg, text, source, target).await,
        Provider::OpenAi => openai::translate(client, cfg, text, source, target).await,
        Provider::DeepL => Ok(deepl::translate(client, cfg, &[text.to_owned()], source, target).await?.into_iter().next().unwrap_or_default()),
        Provider::Azure => Ok(azure::translate(client, cfg, &[text.to_owned()], source, target).await?.into_iter().next().unwrap_or_default()),
        Provider::Off => Err(TranslateError::Off),
    }
}

/// Consecutive index ranges of `texts`, each with at most `max_lines` lines and `max_chars` characters (a single
/// longer line still gets its own batch).
pub(crate) fn batches(texts: &[String], max_lines: usize, max_chars: usize) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut chars = 0;
    for (i, t) in texts.iter().enumerate() {
        let len = t.chars().count() + 1;
        if i > start && (i - start >= max_lines.max(1) || chars + len > max_chars) {
            out.push(start..i);
            start = i;
            chars = 0;
        }
        chars += len;
    }
    if start < texts.len() {
        out.push(start..texts.len());
    }
    out
}

/// A cue's own line breaks become spaces so one cue = one line of the joined request.
pub(crate) fn flatten(text: &str) -> String {
    text.split(['\n', '\r']).map(str::trim).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ")
}

/// Split a provider reply back into lines, dropping a trailing empty line.
pub(crate) fn split_lines(reply: &str) -> Vec<String> {
    let mut parts: Vec<String> = reply.replace("\r\n", "\n").split('\n').map(|s| s.trim().to_owned()).collect();
    while parts.last().is_some_and(|s| s.is_empty()) {
        parts.pop();
    }
    parts
}

/// English name of a language code (prompt of the OpenAI-compatible translator); the code itself when unknown.
pub(crate) fn language_name(code: &str) -> String {
    let name = match code.to_ascii_lowercase().as_str() {
        "es" => "Spanish",
        "en" => "English",
        "pt" => "Portuguese",
        "ja" => "Japanese",
        "fr" => "French",
        "de" => "German",
        "it" => "Italian",
        "ru" => "Russian",
        "ar" => "Arabic",
        "zh" => "Chinese (Simplified)",
        "ko" => "Korean",
        "pl" => "Polish",
        "tr" => "Turkish",
        "nl" => "Dutch",
        "id" => "Indonesian",
        "vi" => "Vietnamese",
        "th" => "Thai",
        "hi" => "Hindi",
        "uk" => "Ukrainian",
        "ca" => "Catalan",
        _ => return code.to_owned(),
    };
    name.to_owned()
}

/// Cache key: lowercase hex SHA-256 of `provider | source | target | cache_key | text`. Pure; used by tk-app.
pub fn cache_key(provider: Provider, source: Option<&str>, target: &str, cache_key: &str, text: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    for part in [provider.id(), source.unwrap_or(""), target, cache_key, text] {
        h.update(part.as_bytes());
        h.update([0u8]);
    }
    hex::encode(h.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| (*x).to_owned()).collect()
    }

    #[test]
    fn cache_key_is_stable_and_distinguishes_inputs() {
        let a = cache_key(Provider::Google, None, "es", "21:1", "Hello");
        assert_eq!(a, cache_key(Provider::Google, None, "es", "21:1", "Hello"));
        assert_eq!(a.len(), 64);
        assert_ne!(a, cache_key(Provider::Google, None, "en", "21:1", "Hello"));
        assert_ne!(a, cache_key(Provider::DeepL, None, "es", "21:1", "Hello"));
        assert_eq!(Provider::from_id("deeplx"), Some(Provider::DeepLx));
    }

    #[test]
    fn batches_respect_line_and_char_limits() {
        let texts = s(&["aaaa", "bbbb", "cccc", "dddd", "eeee"]);
        assert_eq!(batches(&texts, 2, 1000), vec![0..2, 2..4, 4..5]);
        // 5 chars per line (+1): two lines fit in 10.
        assert_eq!(batches(&texts, 50, 10), vec![0..2, 2..4, 4..5]);
        // A line longer than the limit still goes, alone.
        let long = s(&["x".repeat(30).as_str(), "y"]);
        assert_eq!(batches(&long, 50, 10), vec![0..1, 1..2]);
        assert!(batches(&[], 50, 10).is_empty());
    }

    #[test]
    fn flatten_and_split_round_trip_line_counts() {
        assert_eq!(flatten("Hello\nthere\r\n  friend "), "Hello there friend");
        assert_eq!(split_lines("Hola\r\nAdiós\n\n"), s(&["Hola", "Adiós"]));
        assert_eq!(split_lines(" a \n b"), s(&["a", "b"]));
    }

    #[test]
    fn config_check_names_what_is_missing() {
        let mut cfg = TranslateConfig { provider: Provider::Google, ..Default::default() };
        assert!(cfg.check().is_ok());
        cfg.provider = Provider::DeepL;
        assert!(matches!(cfg.check(), Err(TranslateError::NotConfigured(_))));
        cfg.api_key = "k:fx".into();
        assert!(cfg.check().is_ok());
        cfg.provider = Provider::Azure;
        assert!(matches!(cfg.check(), Err(TranslateError::NotConfigured(m)) if m.contains("region")));
        cfg.region = "westeurope".into();
        assert!(cfg.check().is_ok());
        cfg.provider = Provider::DeepLx;
        assert!(cfg.check().is_err());
        cfg.provider = Provider::OpenAi;
        cfg.endpoint = "http://127.0.0.1:11434".into();
        assert!(cfg.check().is_ok());
        cfg.provider = Provider::Off;
        assert!(matches!(cfg.check(), Err(TranslateError::Off)));
    }

    #[test]
    fn language_names_for_prompts() {
        assert_eq!(language_name("ES"), "Spanish");
        assert_eq!(language_name("xx"), "xx");
    }

    #[tokio::test]
    async fn blank_only_batches_never_call_the_network() {
        let client = reqwest::Client::new();
        let cfg = TranslateConfig { provider: Provider::Google, ..Default::default() };
        let out = translate(&client, &cfg, &s(&["", "  "]), None, "es").await.unwrap();
        assert_eq!(out, s(&["", "  "]));
        let off = TranslateConfig::default();
        assert!(matches!(translate(&client, &off, &s(&["hi"]), None, "es").await, Err(TranslateError::Off)));
    }
}
