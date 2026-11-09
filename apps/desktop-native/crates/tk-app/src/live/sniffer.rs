//! Embed-source sniffer (wave B, worker **k**; plan 14 section 0.1 #13; checklist `sources.embed-sniffer`,
//! `sources.embed-sniffer.ranking`, `sources.embed-sniffer.adblock`, `sources.embed-sniffer.timeout-setting`;
//! classic `video/EmbedPlayer.tsx`). Contract stubs: k replaces the bodies.
//!
//! How it works once k lands: an extension lists a source with `type: "embed"` (tk-ext maps it to a web candidate
//! with `WebInfo::embed = true`, `container = "Embed"`, ranked below direct streams by `EMBED_PENALTY`). When the
//! user plays it, `LiveBackend::playback_source` calls [`playback_embed`]: the hidden web view of
//! `live/browser.rs` (`BrowserRuntime::new_browser` with `headless: true`, the extension's `browser_domains` plus the
//! embed host) opens the page with the extension's `Referer`, and a navigation / resource policy k adds to
//! `browser.rs` records every request whose URL ends in `.m3u8` / `.mp4` (or whose response is
//! `application/vnd.apple.mpegurl` / `video/mp4`), blocks pop-ups and the known ad hosts (`ADBLOCK_HOSTS`, a short
//! static list plus the extension's own `blockedDomains` hint), and closes the view after
//! `Settings::server.sniffer_timeout_secs`. The first captured URL (HLS first) becomes a `StreamVm` with the headers
//! the page used (Referer, User-Agent, cookies) and goes to mpv like any direct stream.

use std::sync::Arc;

use tk_core::sources::SourceCandidate;
use tk_core::{Media, MediaRef};
use tk_ext::BrowserRuntime;
use tk_ui::backend::{PlaybackVm, SniffRequestVm, SniffResultVm, UiResult};
use tk_ui::model::UiError;

use crate::live::LiveBackend;

/// Ad / tracker hosts the hidden view refuses while sniffing (`sources.embed-sniffer.adblock`); k extends the list.
pub const ADBLOCK_HOSTS: &[&str] = &["doubleclick.net", "googlesyndication.com", "adnxs.com", "popads.net", "propellerads.com"];

/// Media URL suffixes / content types that count as a capture.
pub const MEDIA_SUFFIXES: &[&str] = &[".m3u8", ".mp4"];

pub struct Sniffer {
    #[allow(dead_code)]
    browsers: Arc<dyn BrowserRuntime>,
}

impl Sniffer {
    pub fn new(browsers: Arc<dyn BrowserRuntime>) -> Self {
        Self { browsers }
    }
}

/// Whether a requested URL is a media URL worth capturing (pure; used by the resource policy).
pub fn is_media_url(url: &str) -> bool {
    let path = url.split(['?', '#']).next().unwrap_or("").to_ascii_lowercase();
    MEDIA_SUFFIXES.iter().any(|s| path.ends_with(s))
}

/// Whether `host` (or a parent domain of it) is on the block list.
pub fn is_blocked_host(host: &str) -> bool {
    let h = host.to_ascii_lowercase();
    ADBLOCK_HOSTS.iter().any(|b| h == *b || h.ends_with(&format!(".{b}")))
}

/// `Backend::sniff_embed`.
pub async fn sniff(_b: &LiveBackend, _req: &SniffRequestVm) -> UiResult<SniffResultVm> {
    Err(UiError::not_wired("sniff_embed"))
}

/// Called by `LiveBackend::playback_source` for `WebInfo::embed` candidates: sniff, then build the `PlaybackVm`
/// (`start_secs` from `b.start_secs(r, episode)`, `streams` = the captured URLs with headers).
pub async fn playback_embed(_b: &LiveBackend, _r: &MediaRef, _media: &Media, _episode: u32, _candidate: &SourceCandidate) -> UiResult<PlaybackVm> {
    Err(UiError::not_wired("sniff_embed"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_urls_and_blocked_hosts() {
        assert!(is_media_url("https://cdn.example/x/master.m3u8?token=1"));
        assert!(is_media_url("https://cdn.example/v.MP4"));
        assert!(!is_media_url("https://cdn.example/player.js"));
        assert!(is_blocked_host("ads.doubleclick.net"));
        assert!(!is_blocked_host("example.org"));
    }
}
