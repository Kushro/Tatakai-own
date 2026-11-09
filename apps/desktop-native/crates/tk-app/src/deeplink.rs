//! `tatakai://extensionserver/connect?server=<origin>&code=<code>` links from the OS (wave B, worker **j**; checklist
//! `ext.deep-link.server`; docs/redesign/13 section 3.7). Same shape as `magnet.rs` (wave A, worker i): `from_args`
//! picks the link out of the command line (implemented, pure), `main.rs` calls `remember` before the window exists,
//! `live/ext_server.rs::take_pending_link` hands it to the Extensions page, which opens the pairing sheet after the
//! user confirms. Registering the `tatakai:` scheme per user (Linux `.desktop` with `x-scheme-handler/tatakai`,
//! Windows `HKCU\Software\Classes\tatakai`) and forwarding from a second instance (`single_instance.rs`, a
//! `<data>/deeplink.pending` file like `magnet.pending`) are j's, following `magnet::set_registered`.

use std::sync::Mutex;

/// Scheme + host + path of the pairing link.
pub const CONNECT_PREFIX: &str = "tatakai://extensionserver/connect?";

/// A pairing deep link (case-insensitive scheme), at most 4 KiB, with both `server=` and `code=` parameters.
pub fn is_connect_link(s: &str) -> bool {
    let s = s.trim();
    if s.len() > 4 * 1024 || s.len() <= CONNECT_PREFIX.len() {
        return false;
    }
    let (head, query) = s.split_at(CONNECT_PREFIX.len());
    head.eq_ignore_ascii_case(CONNECT_PREFIX)
        && query.split('&').any(|kv| kv.starts_with("server="))
        && query.split('&').any(|kv| kv.starts_with("code="))
}

/// The first pairing link of a command line (the program name is skipped).
pub fn from_args(args: impl IntoIterator<Item = String>) -> Option<String> {
    args.into_iter().skip(1).find(|a| is_connect_link(a)).map(|a| a.trim().to_owned())
}

static PENDING: Mutex<Option<String>> = Mutex::new(None);

/// Keep a link for the UI (`take` consumes it). Anything else is ignored.
pub fn remember(link: &str) {
    if is_connect_link(link)
        && let Ok(mut p) = PENDING.lock()
    {
        *p = Some(link.trim().to_owned());
    }
}

/// The remembered link, if any (consumed).
pub fn take() -> Option<String> {
    PENDING.lock().ok().and_then(|mut p| p.take())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_pairing_links_only() {
        assert!(is_connect_link("tatakai://extensionserver/connect?server=http%3A%2F%2F10.0.0.2%3A8099&code=123456"));
        assert!(is_connect_link("TATAKAI://extensionserver/connect?code=1&server=x"));
        assert!(!is_connect_link("tatakai://extensionserver/connect?server=x"));
        assert!(!is_connect_link("magnet:?xt=urn:btih:abc"));
        assert_eq!(from_args(["exe".to_owned(), "tatakai://extensionserver/connect?server=a&code=b".to_owned()]).as_deref(), Some("tatakai://extensionserver/connect?server=a&code=b"));
    }

    #[test]
    fn remember_and_take() {
        remember("tatakai://extensionserver/connect?server=a&code=b");
        assert!(take().is_some());
        assert!(take().is_none());
    }
}
