//! Local files the novel screens show in the web view (worker **m**): narration clips (`TtsClipVm::path`) and the
//! custom reader font (`NovelPrefs::custom_font_path`). The web view cannot read arbitrary paths, so the screens
//! register each file here and the `tk-novel` asset handler ([`use_novel_assets`]) serves only registered files
//! (`/tk-novel/<kind>/<name>`), with `Range` support for the audio element. Also the small JS helpers the reader
//! and the settings row use to play a clip and to keep the screen awake.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use dioxus::desktop::use_asset_handler;
use dioxus::desktop::wry::http::Response;
use dioxus::prelude::*;

use crate::model::novel::ASSET_HANDLER;

static FILES: Mutex<Option<HashMap<String, PathBuf>>> = Mutex::new(None);

/// `<kind>/<file name>` key of a path; names keep only safe characters.
pub fn asset_key(kind: &str, path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    let safe: String = name.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')).collect();
    (!safe.is_empty() && !safe.starts_with('.')).then(|| format!("{kind}/{safe}"))
}

/// Allow the web view to load `path`; returns its URL (`/tk-novel/<kind>/<name>`).
pub fn register(kind: &str, path: &Path) -> Option<String> {
    let key = asset_key(kind, path)?;
    if let Ok(mut g) = FILES.lock() {
        let map = g.get_or_insert_with(HashMap::new);
        if map.len() > 512 {
            map.clear();
        }
        map.insert(key.clone(), path.to_path_buf());
    }
    Some(format!("/{ASSET_HANDLER}/{key}"))
}

fn lookup(key: &str) -> Option<PathBuf> {
    FILES.lock().ok().and_then(|g| g.as_ref().and_then(|m| m.get(key).cloned()))
}

pub fn mime_of(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("wav") => "audio/wav",
        Some("mp3") => "audio/mpeg",
        Some("ogg") => "audio/ogg",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    }
}

/// `bytes=a-b` of a `Range` header -> inclusive `(start, end)` within `len`.
pub fn parse_range(header: &str, len: usize) -> Option<(usize, usize)> {
    let spec = header.trim().strip_prefix("bytes=")?.split(',').next()?.trim();
    let (a, b) = spec.split_once('-')?;
    if len == 0 {
        return None;
    }
    let (start, end) = if a.is_empty() {
        let n: usize = b.parse().ok()?;
        (len.saturating_sub(n), len - 1)
    } else {
        let start: usize = a.parse().ok()?;
        let end = if b.is_empty() { len - 1 } else { b.parse::<usize>().ok()?.min(len - 1) };
        (start, end)
    };
    (start <= end && start < len).then_some((start, end))
}

/// Registers the `tk-novel` asset handler for as long as the calling component lives.
pub fn use_novel_assets() {
    use_asset_handler(ASSET_HANDLER, move |req, responder| {
        let path = req.uri().path().trim_start_matches('/');
        let key = path.strip_prefix(ASSET_HANDLER).unwrap_or(path).trim_start_matches('/').to_owned();
        let file = lookup(&key);
        let bytes = file.as_ref().and_then(|p| std::fs::read(p).ok());
        let (Some(file), Some(bytes)) = (file, bytes) else {
            let not_found = Response::builder().status(404).body(Vec::new());
            if let Ok(r) = not_found {
                responder.respond(r);
            }
            return;
        };
        let mime = mime_of(&file);
        let range = req.headers().get("range").and_then(|v| v.to_str().ok()).and_then(|h| parse_range(h, bytes.len()));
        let built = match range {
            Some((start, end)) => Response::builder()
                .status(206)
                .header("Content-Type", mime)
                .header("Accept-Ranges", "bytes")
                .header("Content-Range", format!("bytes {start}-{end}/{}", bytes.len()))
                .header("Access-Control-Allow-Origin", "*")
                .body(bytes[start..=end].to_vec()),
            None => Response::builder()
                .status(200)
                .header("Content-Type", mime)
                .header("Accept-Ranges", "bytes")
                .header("Access-Control-Allow-Origin", "*")
                .body(bytes),
        };
        if let Ok(r) = built {
            responder.respond(r);
        }
    });
}

/// JS string literal of `s`.
pub fn js_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "''".into())
}

/// Plays a clip in the shared `window.__tkNovelAudio` and answers `ended` | `error` | `stopped` once.
pub fn play_clip_js(url: &str) -> String {
    format!(
        r#"const url = {url};
const old = window.__tkNovelAudio;
if (old) {{ try {{ old.pause(); old.dispatchEvent(new Event('tkstop')); }} catch (e) {{}} }}
let src = url;
try {{ const r = await fetch(url); if (r.ok) {{ src = URL.createObjectURL(await r.blob()); }} }} catch (e) {{}}
const a = new Audio(src);
window.__tkNovelAudio = a;
const outcome = await new Promise((resolve) => {{
  a.addEventListener('ended', () => resolve('ended'));
  a.addEventListener('error', () => resolve('error'));
  a.addEventListener('tkstop', () => resolve('stopped'));
  a.play().catch(() => resolve('error'));
}});
if (src !== url) {{ try {{ URL.revokeObjectURL(src); }} catch (e) {{}} }}
if (window.__tkNovelAudio === a) {{ window.__tkNovelAudio = null; }}
dioxus.send(outcome);
return outcome;"#,
        url = js_str(url)
    )
}

/// Pause / resume / stop the current clip.
pub const PAUSE_JS: &str = "const a = window.__tkNovelAudio; if (a) { a.pause(); }";
pub const RESUME_JS: &str = "const a = window.__tkNovelAudio; if (a) { a.play().catch(() => {}); }";
pub const STOP_JS: &str =
    "const a = window.__tkNovelAudio; if (a) { try { a.pause(); a.dispatchEvent(new Event('tkstop')); } catch (e) {} }";

/// Screen Wake Lock on / off; answers whether the web view supports it.
pub fn wake_lock_js(on: bool) -> String {
    format!(
        r#"const on = {on};
if (!('wakeLock' in navigator)) {{ dioxus.send(false); return false; }}
try {{
  if (on) {{ if (!window.__tkWake) {{ window.__tkWake = await navigator.wakeLock.request('screen'); }} }}
  else if (window.__tkWake) {{ await window.__tkWake.release(); window.__tkWake = null; }}
  dioxus.send(true);
}} catch (e) {{ dioxus.send(false); }}
return true;"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_and_urls() {
        assert_eq!(asset_key("clip", Path::new("/data/tts/cache/ab12.wav")).as_deref(), Some("clip/ab12.wav"));
        assert_eq!(asset_key("font", Path::new("/f/My Font!.ttf")).as_deref(), Some("font/MyFont.ttf"));
        assert_eq!(asset_key("x", Path::new("/")), None);
        let url = register("clip", Path::new("/tmp/z.wav")).unwrap();
        assert_eq!(url, "/tk-novel/clip/z.wav");
        assert_eq!(lookup("clip/z.wav"), Some(PathBuf::from("/tmp/z.wav")));
        assert_eq!(lookup("clip/other.wav"), None);
        assert_eq!(mime_of(Path::new("a.MP3")), "audio/mpeg");
        assert_eq!(mime_of(Path::new("a.otf")), "font/otf");
    }

    #[test]
    fn ranges() {
        assert_eq!(parse_range("bytes=0-", 10), Some((0, 9)));
        assert_eq!(parse_range("bytes=2-4", 10), Some((2, 4)));
        assert_eq!(parse_range("bytes=-3", 10), Some((7, 9)));
        assert_eq!(parse_range("bytes=5-99", 10), Some((5, 9)));
        assert_eq!(parse_range("bytes=11-", 10), None);
        assert_eq!(parse_range("items=0-1", 10), None);
    }

    #[test]
    fn js_helpers_embed_literals() {
        assert!(play_clip_js("/tk-novel/clip/a'b.wav").contains("\"/tk-novel/clip/a'b.wav\""));
        assert!(wake_lock_js(true).starts_with("const on = true;"));
    }
}
