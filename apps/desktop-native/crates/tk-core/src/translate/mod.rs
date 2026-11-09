//! Subtitle translation providers (wave A, worker **d**; docs/redesign/15 section 4): Google (public
//! `translate.googleapis.com/translate_a/single` endpoint, no key), DeepL (`api-free.deepl.com` / `api.deepl.com`,
//! key decides), DeepLX (self-hosted endpoint), Azure Translator (`api.cognitive.microsofttranslator.com`, key +
//! region) and any OpenAI-compatible chat endpoint (`endpoint` + `model` + key; one JSON array in, one out).
//! Batches of cues (<= 50 lines / 5 000 chars per request), caching through `LocalStore::translation_*`.
//! Contract: [`TranslateConfig`] / [`Provider`] and [`translate`] are final; the body is a stub. Worker d adds one
//! private file per provider (`google.rs`, `deepl.rs`, ...) registered here.

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

/// Translate `texts` (one cue each; inline tags preserved) from `source` (`None` = detect) into `target`
/// (ISO 639-1). Returns one string per input, same order. Contract stub (worker d).
pub async fn translate(client: &reqwest::Client, cfg: &TranslateConfig, texts: &[String], source: Option<&str>, target: &str) -> Result<Vec<String>, TranslateError> {
    let _ = (client, texts, source, target);
    match cfg.provider {
        Provider::Off => Err(TranslateError::Off),
        _ => Err(TranslateError::Unsupported),
    }
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

    #[test]
    fn cache_key_is_stable_and_distinguishes_inputs() {
        let a = cache_key(Provider::Google, None, "es", "21:1", "Hello");
        assert_eq!(a, cache_key(Provider::Google, None, "es", "21:1", "Hello"));
        assert_eq!(a.len(), 64);
        assert_ne!(a, cache_key(Provider::Google, None, "en", "21:1", "Hello"));
        assert_ne!(a, cache_key(Provider::DeepL, None, "es", "21:1", "Hello"));
        assert_eq!(Provider::from_id("deeplx"), Some(Provider::DeepLx));
    }
}
