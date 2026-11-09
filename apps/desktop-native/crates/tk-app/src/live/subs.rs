//! `Backend` subtitle / translation / capture calls (wave A, worker **d**; docs/redesign/15 section 4):
//! * `fetch_subtitle`: `reqwest` GET with the stream headers (size cap 5 MiB; a local path or `file://` URL is read
//!   from disk), text decoded as UTF-8 (BOM stripped), UTF-16 with BOM, else Latin-1.
//! * `translate_cues`: `tk_core::translate::translate` over the cues the cache does not have yet, cached in
//!   `translation_cache` keyed by `tk_core::translate::cache_key`; the provider settings come from the saved
//!   `Settings::subs.translation` and the key from `b.secrets` (`keys::translation(provider.id())`), never from
//!   `Settings`. An empty request with `cache_key == tk_ui::model::subs::CLEAR_CACHE_KEY` empties the cache
//!   instead (Settings > Subtitles "Clear cache") and answers one cue whose `index` is the number of rows removed.
//! * `translation_test`: one fixed English sentence with the saved settings.
//! * `set_translation_key` / `translation_key_present`: `b.secrets.set` / `delete` / `get`.
//! * `screenshot_path`: `<Pictures>/Tatakai/<safe title> - E<ep> - <yyyyMMdd-HHmmss>.png` (`directories::UserDirs::
//!   picture_dir`, else `<data>/screenshots`), parent created, never an existing file.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use tk_core::secrets::keys;
use tk_core::translate::{self, Provider, TranslateConfig};
use tk_core::TranslateError;
use tk_ui::backend::{CueVm, TranslateRequestVm, TranslationTestVm, UiResult};
use tk_ui::model::subs::CLEAR_CACHE_KEY;
use tk_ui::model::{ErrorKind, TranslationPrefs, TranslationProviderVm, UiError};

use crate::live::LiveBackend;
use crate::settings_store;

/// Largest subtitle file `fetch_subtitle` accepts.
const MAX_SUBTITLE_BYTES: usize = 5 * 1024 * 1024;
/// The sentence "Test" translates.
const TEST_SAMPLE: &str = "Hello! This is a subtitle translation test.";

fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36")
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_default()
    })
}

fn other(e: impl std::fmt::Display) -> UiError {
    UiError::new(ErrorKind::Other, e.to_string())
}

fn translate_err(e: TranslateError) -> UiError {
    let kind = match &e {
        TranslateError::RateLimited => ErrorKind::RateLimited,
        TranslateError::Network(_) => ErrorKind::Offline,
        _ => ErrorKind::Other,
    };
    UiError::new(kind, e.to_string())
}

fn core_provider(p: TranslationProviderVm) -> Provider {
    match p {
        TranslationProviderVm::Off => Provider::Off,
        TranslationProviderVm::Google => Provider::Google,
        TranslationProviderVm::DeepL => Provider::DeepL,
        TranslationProviderVm::DeepLx => Provider::DeepLx,
        TranslationProviderVm::Azure => Provider::Azure,
        TranslationProviderVm::OpenAi => Provider::OpenAi,
    }
}

fn now_unix() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

async fn secret(b: &LiveBackend, key: String) -> UiResult<Option<String>> {
    let secrets = b.secrets.clone();
    tokio::task::spawn_blocking(move || secrets.get(&key)).await.map_err(other)?.map_err(other)
}

/// The saved translation settings plus the provider key from `Secrets`.
async fn config(b: &LiveBackend) -> UiResult<(TranslationPrefs, TranslateConfig)> {
    let prefs = settings_store::load(&b.settings_file).subs.translation;
    let provider = core_provider(prefs.provider);
    let api_key = if prefs.provider.needs_key() { secret(b, keys::translation(prefs.provider.id())).await?.unwrap_or_default() } else { String::new() };
    let cfg = TranslateConfig { provider, endpoint: prefs.endpoint.clone(), api_key, model: prefs.model.clone(), region: prefs.region.clone() };
    cfg.check().map_err(translate_err)?;
    Ok((prefs, cfg))
}

/// `Backend::set_translation_key`.
pub async fn set_translation_key(b: &LiveBackend, provider: TranslationProviderVm, key: Option<&str>) -> UiResult<()> {
    if provider == TranslationProviderVm::Off {
        return Err(other("no provider selected"));
    }
    let name = keys::translation(provider.id());
    let value = key.map(str::trim).filter(|k| !k.is_empty()).map(str::to_owned);
    let secrets = b.secrets.clone();
    tokio::task::spawn_blocking(move || match value {
        Some(v) => secrets.set(&name, &v),
        None => secrets.delete(&name),
    })
    .await
    .map_err(other)?
    .map_err(other)
}

/// `Backend::translation_key_present`.
pub async fn translation_key_present(b: &LiveBackend, provider: TranslationProviderVm) -> UiResult<bool> {
    if provider == TranslationProviderVm::Off {
        return Ok(false);
    }
    Ok(secret(b, keys::translation(provider.id())).await?.is_some_and(|k| !k.trim().is_empty()))
}

/// `Backend::fetch_subtitle`.
pub async fn fetch_subtitle(_b: &LiveBackend, url: &str, headers: &[(String, String)]) -> UiResult<String> {
    let url = url.trim();
    if let Some(path) = local_path(url) {
        let bytes = tokio::fs::read(&path).await.map_err(other)?;
        if bytes.len() > MAX_SUBTITLE_BYTES {
            return Err(other("subtitle file is too large"));
        }
        return Ok(decode_text(&bytes));
    }
    let mut req = client().get(url);
    for (name, value) in headers {
        if !name.trim().is_empty() {
            req = req.header(name.trim(), value.as_str());
        }
    }
    let resp = req.send().await.map_err(|e| UiError::new(ErrorKind::Offline, e.without_url().to_string()))?;
    let resp = resp.error_for_status().map_err(|e| other(e.without_url()))?;
    if resp.content_length().is_some_and(|n| n as usize > MAX_SUBTITLE_BYTES) {
        return Err(other("subtitle file is too large"));
    }
    let bytes = resp.bytes().await.map_err(|e| other(e.without_url()))?;
    if bytes.len() > MAX_SUBTITLE_BYTES {
        return Err(other("subtitle file is too large"));
    }
    Ok(decode_text(&bytes))
}

/// `file:///x.srt` or a plain absolute / relative path (not `http(s)://`).
fn local_path(url: &str) -> Option<PathBuf> {
    if let Some(rest) = url.strip_prefix("file://") {
        // file:///C:/x on Windows keeps the drive letter.
        let rest = if cfg!(windows) { rest.trim_start_matches('/') } else { rest };
        return Some(PathBuf::from(rest));
    }
    if url.starts_with("http://") || url.starts_with("https://") {
        return None;
    }
    Some(PathBuf::from(url))
}

/// UTF-8 (BOM stripped), UTF-16 LE / BE with BOM, else Latin-1 (every byte is a code point).
pub(crate) fn decode_text(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    let utf16 = |rest: &[u8], le: bool| {
        let units: Vec<u16> =
            rest.chunks_exact(2).map(|c| if le { u16::from_le_bytes([c[0], c[1]]) } else { u16::from_be_bytes([c[0], c[1]]) }).collect();
        String::from_utf16_lossy(&units)
    };
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(rest, true);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return utf16(rest, false);
    }
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_owned(),
        Err(_) => bytes.iter().map(|&b| char::from(b)).collect(),
    }
}

/// `Backend::translate_cues`.
pub async fn translate_cues(b: &LiveBackend, req: &TranslateRequestVm) -> UiResult<Vec<CueVm>> {
    if req.cues.is_empty() {
        if req.cache_key == CLEAR_CACHE_KEY {
            let removed = b.store.translation_clear().map_err(other)?;
            return Ok(vec![CueVm { index: u32::try_from(removed).unwrap_or(u32::MAX), start_ms: 0, end_ms: 0, text: String::new() }]);
        }
        return Ok(Vec::new());
    }
    let (prefs, cfg) = config(b).await?;
    let target = if req.target_lang.trim().is_empty() { prefs.target_lang.clone() } else { req.target_lang.trim().to_ascii_lowercase() };
    let source = req.source_lang.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let use_cache = !req.cache_key.is_empty();

    let mut out = req.cues.clone();
    let mut missing: Vec<usize> = Vec::new();
    let mut keys_of: Vec<Option<String>> = vec![None; out.len()];
    for (i, cue) in req.cues.iter().enumerate() {
        if cue.text.trim().is_empty() {
            continue;
        }
        if use_cache {
            let key = translate::cache_key(cfg.provider, source, &target, &req.cache_key, &cue.text);
            if let Ok(Some(hit)) = b.store.translation_get(&key) {
                out[i].text = hit;
                continue;
            }
            keys_of[i] = Some(key);
        }
        missing.push(i);
    }
    if missing.is_empty() {
        return Ok(out);
    }
    let texts: Vec<String> = missing.iter().map(|&i| req.cues[i].text.clone()).collect();
    let translated = translate::translate(client(), &cfg, &texts, source, &target).await.map_err(translate_err)?;
    let now = now_unix();
    for (&i, text) in missing.iter().zip(translated) {
        if let Some(key) = &keys_of[i]
            && let Err(e) = b.store.translation_put(key, cfg.provider.id(), source, &target, &text, now)
        {
            tracing::debug!(error = %e, "translation cache write failed");
        }
        out[i].text = text;
    }
    Ok(out)
}

/// `Backend::translation_test`.
pub async fn translation_test(b: &LiveBackend) -> UiResult<TranslationTestVm> {
    let (prefs, cfg) = config(b).await?;
    let started = Instant::now();
    let out = translate::translate(client(), &cfg, &[TEST_SAMPLE.to_owned()], Some("en"), &prefs.target_lang).await.map_err(translate_err)?;
    let latency_ms = u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX);
    let sample_out = out.into_iter().next().unwrap_or_default();
    if sample_out.trim().is_empty() {
        return Err(other("the provider answered an empty translation"));
    }
    Ok(TranslationTestVm { sample_in: TEST_SAMPLE.to_owned(), sample_out, latency_ms })
}

/// `Backend::screenshot_path`.
pub async fn screenshot_path(b: &LiveBackend, title: &str, episode: u32) -> UiResult<PathBuf> {
    let dir = directories::UserDirs::new()
        .and_then(|u| u.picture_dir().map(|d| d.join("Tatakai")))
        .unwrap_or_else(|| b.data_root.join("screenshots"));
    tokio::fs::create_dir_all(&dir).await.map_err(other)?;
    let now = time::OffsetDateTime::now_utc();
    let stamp = format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        now.year(),
        u8::from(now.month()),
        now.day(),
        now.hour(),
        now.minute(),
        now.second()
    );
    Ok(unique_path(&dir, &screenshot_name(title, episode, &stamp)))
}

/// `<safe title> - E<ep> - <stamp>.png`.
pub(crate) fn screenshot_name(title: &str, episode: u32, stamp: &str) -> String {
    let mut safe: String = title
        .chars()
        .map(|c| if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    safe = safe.trim_matches('.').trim().chars().take(80).collect::<String>().trim().to_owned();
    if safe.is_empty() {
        safe = "Tatakai".to_owned();
    }
    format!("{safe} - E{episode:02} - {stamp}.png")
}

/// `name`, or `name (2)` ... when a file with that name exists.
fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let stem = name.strip_suffix(".png").unwrap_or(name);
    (2..1000).map(|n| dir.join(format!("{stem} ({n}).png"))).find(|p| !p.exists()).unwrap_or(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_decoding_handles_boms_and_latin1() {
        assert_eq!(decode_text(b"\xEF\xBB\xBF1\nHola"), "1\nHola");
        assert_eq!(decode_text("ñandú".as_bytes()), "ñandú");
        assert_eq!(decode_text(b"Espa\xF1a"), "España");
        let le: Vec<u8> = [0xFF, 0xFE].into_iter().chain("Hi".encode_utf16().flat_map(u16::to_le_bytes)).collect();
        assert_eq!(decode_text(&le), "Hi");
        let be: Vec<u8> = [0xFE, 0xFF].into_iter().chain("Hi".encode_utf16().flat_map(u16::to_be_bytes)).collect();
        assert_eq!(decode_text(&be), "Hi");
    }

    #[test]
    fn screenshot_names_are_safe() {
        assert_eq!(screenshot_name("Re:Zero / Season 2?", 3, "20261109-101500"), "Re Zero Season 2 - E03 - 20261109-101500.png");
        assert_eq!(screenshot_name("  ", 12, "x"), "Tatakai - E12 - x.png");
        let dir = tempfile::tempdir().unwrap();
        let p = unique_path(dir.path(), "a.png");
        std::fs::write(&p, b"x").unwrap();
        assert_eq!(unique_path(dir.path(), "a.png"), dir.path().join("a (2).png"));
    }

    #[test]
    fn local_paths_are_recognised() {
        assert!(local_path("https://x/a.vtt").is_none());
        assert!(local_path("http://x/a.vtt").is_none());
        assert!(local_path("/tmp/a.srt").is_some());
        assert!(local_path("file:///tmp/a.srt").is_some());
    }
}
