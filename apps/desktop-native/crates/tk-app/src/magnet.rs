//! `magnet:` links from the OS (wave A, worker **i**; plan 14 section 0.1 #11, checklist `torrent.magnet-link-handler`).
//!
//! Two halves:
//! * **Receiving**: the OS starts `tatakai-next magnet:?xt=urn:btih:...`. `from_args` picks the link out of the
//!   command line (implemented, pure); `main.rs` calls `remember` with it before the window exists and
//!   `live/torrent_manual.rs::take_pending_magnet` hands it to the Torrents > Library page, which opens the "Add
//!   torrent" sheet. When another instance already runs, `single_instance::acquire` only focuses it today; i
//!   extends it (its own file in wave A: see docs/redesign/15 section 2) to forward the link - Windows:
//!   `WM_COPYDATA` to the window found by title; Linux: a `<data>/magnet.pending` file the running instance
//!   watches every 2 s from `on_start`.
//! * **Registering** (`set_registered`), per user and without an installer or elevation:
//!   - Linux: write `~/.local/share/applications/tatakai-next-magnet.desktop` (`Exec=<current_exe> %u`,
//!     `MimeType=x-scheme-handler/magnet;`, `NoDisplay=true`) and run `xdg-mime default tatakai-next-magnet.desktop
//!     x-scheme-handler/magnet`; `is_registered` = `xdg-mime query default x-scheme-handler/magnet` names that file.
//!   - Windows: `HKCU\Software\Classes\magnet` (`URL Protocol`, `shell\open\command` = `"<current_exe>" "%1"`)
//!     through `windows-sys` registry calls (`RegCreateKeyExW` / `RegSetValueExW`, no admin); `is_registered`
//!     compares the stored command with `std::env::current_exe()`.
//!   - macOS: `supported()` is false (the handler comes from the bundle's `Info.plist` `CFBundleURLTypes`, i.e.
//!     the packaging step, not the running app); the Settings row is hidden there.
//!   `supported()` is also false when `current_exe()` fails or points inside a `target/` directory (dev builds
//!   must not register a path that disappears on the next build).
//! Contract: `from_args` and `is_magnet` are final and implemented; the rest are stubs for i.

use std::sync::Mutex;

/// `magnet:?xt=urn:btih:...` (case-insensitive scheme), at most 8 KiB.
pub fn is_magnet(s: &str) -> bool {
    let s = s.trim();
    s.len() <= 8 * 1024 && s.len() > "magnet:?".len() && s[..7].eq_ignore_ascii_case("magnet:") && s.to_ascii_lowercase().contains("xt=urn:btih:")
}

/// The first `magnet:` argument of a command line (program name excluded by the caller or not: it is skipped).
pub fn from_args(args: impl IntoIterator<Item = String>) -> Option<String> {
    args.into_iter().skip(1).find(|a| is_magnet(a)).map(|a| a.trim().to_owned())
}

static PENDING: Mutex<Option<String>> = Mutex::new(None);

/// Keep a link for the UI (`take` consumes it). Non-magnets are ignored.
pub fn remember(link: &str) {
    if is_magnet(link)
        && let Ok(mut p) = PENDING.lock()
    {
        *p = Some(link.trim().to_owned());
    }
}

/// The remembered link, if any (consumed).
pub fn take() -> Option<String> {
    PENDING.lock().ok().and_then(|mut p| p.take())
}

/// Whether `set_registered` can work on this platform / build (see the module doc). Contract stub: `false`.
pub fn supported() -> bool {
    false
}

/// Whether this executable is the current `magnet:` handler of the user. Contract stub: `false`.
pub fn is_registered() -> bool {
    false
}

/// Register (`true`) or unregister (`false`) this executable as the `magnet:` handler. Contract stub.
pub fn set_registered(enabled: bool) -> std::io::Result<()> {
    let _ = enabled;
    Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "magnet handler registration is not wired in this build"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_magnet_out_of_the_command_line() {
        let args = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let m = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=x";
        assert_eq!(from_args(args(&["tatakai-next", m])).as_deref(), Some(m));
        assert_eq!(from_args(args(&["tatakai-next", "--version", &format!(" {m} ")])).as_deref(), Some(m));
        assert_eq!(from_args(args(&[m])), None, "the program name is never a link");
        assert_eq!(from_args(args(&["tatakai-next", "https://example.com"])), None);
        assert!(!is_magnet("magnet:?dn=no-hash"));
        assert!(is_magnet("MAGNET:?xt=urn:btih:abc"));
    }

    #[test]
    fn pending_link_is_consumed_once() {
        remember("not a magnet");
        assert_eq!(take(), None);
        remember("magnet:?xt=urn:btih:abc");
        assert_eq!(take().as_deref(), Some("magnet:?xt=urn:btih:abc"));
        assert_eq!(take(), None);
    }
}
