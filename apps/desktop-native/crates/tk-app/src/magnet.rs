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
//! Contract: `from_args` and `is_magnet` are final. Forwarding from a second instance lives in `single_instance.rs`.

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

/// Desktop file id of the handler (Linux).
pub const DESKTOP_ID: &str = "tatakai-next-magnet.desktop";
/// MIME type of the scheme (Linux).
const SCHEME_MIME: &str = "x-scheme-handler/magnet";

/// Whether `set_registered` can work on this platform / build (see the module doc).
pub fn supported() -> bool {
    if !cfg!(any(windows, target_os = "linux")) {
        return false;
    }
    std::env::current_exe().is_ok_and(|exe| !is_dev_build(&exe))
}

/// An executable inside a cargo `target/` directory (dev builds move on every build).
fn is_dev_build(exe: &std::path::Path) -> bool {
    exe.components().any(|c| c.as_os_str() == "target")
}

/// Whether this executable is the current `magnet:` handler of the user.
pub fn is_registered() -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    platform::is_registered(&exe)
}

/// Register (`true`) or unregister (`false`) this executable as the `magnet:` handler.
pub fn set_registered(enabled: bool) -> std::io::Result<()> {
    if !supported() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "this build cannot register itself as the magnet: handler (macOS bundle or development build)",
        ));
    }
    let exe = std::env::current_exe()?;
    if enabled { platform::register(&exe) } else { platform::unregister(&exe) }
}

/// `Exec=` value of the `.desktop` file: the executable quoted per the Desktop Entry spec, then `%u`.
fn desktop_exec(exe: &std::path::Path) -> String {
    let raw = exe.display().to_string();
    let mut quoted = String::with_capacity(raw.len() + 2);
    quoted.push('"');
    for c in raw.chars() {
        match c {
            '"' | '`' | '$' | '\\' => {
                quoted.push('\\');
                quoted.push(c);
            }
            '%' => quoted.push_str("%%"),
            _ => quoted.push(c),
        }
    }
    quoted.push('"');
    format!("{quoted} %u")
}

fn desktop_file(exe: &std::path::Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=Tatakai\nComment=Open magnet links in Tatakai\nExec={}\nMimeType={SCHEME_MIME};\nNoDisplay=true\nTerminal=false\n",
        desktop_exec(exe)
    )
}

/// `mimeapps.list` without our desktop id in the `x-scheme-handler/magnet` lines (other handlers stay; a line left
/// empty goes).
fn mimeapps_without_us(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        if let Some((key, value)) = line.split_once('=')
            && key.trim() == SCHEME_MIME
        {
            let rest: Vec<&str> = value.split(';').map(str::trim).filter(|v| !v.is_empty() && *v != DESKTOP_ID).collect();
            if !rest.is_empty() {
                out.push_str(&format!("{}={};\n", key.trim(), rest.join(";")));
            }
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

#[cfg(target_os = "linux")]
mod platform {
    use std::io;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use super::{DESKTOP_ID, SCHEME_MIME};

    fn home() -> io::Result<PathBuf> {
        std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))
    }

    fn xdg(var: &str, fallback: &str) -> io::Result<PathBuf> {
        match std::env::var_os(var).map(PathBuf::from).filter(|p| p.is_absolute()) {
            Some(p) => Ok(p),
            None => Ok(home()?.join(fallback)),
        }
    }

    fn applications_dir() -> io::Result<PathBuf> {
        Ok(xdg("XDG_DATA_HOME", ".local/share")?.join("applications"))
    }

    fn desktop_path() -> io::Result<PathBuf> {
        Ok(applications_dir()?.join(DESKTOP_ID))
    }

    fn query_default() -> Option<String> {
        let out = Command::new("xdg-mime").args(["query", "default", SCHEME_MIME]).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
    }

    pub(super) fn is_registered(exe: &Path) -> bool {
        let Ok(path) = desktop_path() else { return false };
        let ours = std::fs::read_to_string(&path).is_ok_and(|text| text.contains(&super::desktop_exec(exe)));
        ours && query_default().as_deref() == Some(DESKTOP_ID)
    }

    pub(super) fn register(exe: &Path) -> io::Result<()> {
        let dir = applications_dir()?;
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(DESKTOP_ID);
        let tmp = path.with_extension("desktop.tmp");
        std::fs::write(&tmp, super::desktop_file(exe))?;
        std::fs::rename(&tmp, &path)?;
        let status = Command::new("xdg-mime").args(["default", DESKTOP_ID, SCHEME_MIME]).status().map_err(|e| {
            io::Error::new(e.kind(), format!("xdg-mime is not available ({e}); install xdg-utils"))
        })?;
        if !status.success() {
            return Err(io::Error::other(format!("xdg-mime default failed ({status})")));
        }
        // Best effort: refresh the MIME cache of the menu entries.
        let _ = Command::new("update-desktop-database").arg(&dir).status();
        Ok(())
    }

    pub(super) fn unregister(_exe: &Path) -> io::Result<()> {
        let path = desktop_path()?;
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        let list = xdg("XDG_CONFIG_HOME", ".config")?.join("mimeapps.list");
        if let Ok(text) = std::fs::read_to_string(&list) {
            let next = super::mimeapps_without_us(&text);
            if next != text {
                std::fs::write(&list, next)?;
            }
        }
        if let Ok(dir) = applications_dir() {
            let _ = Command::new("update-desktop-database").arg(dir).status();
        }
        Ok(())
    }
}

#[cfg(windows)]
mod platform {
    //! `HKCU\Software\Classes\magnet` (per user, no elevation). Needs the windows-sys feature
    //! `Win32_System_Registry` (requested in `crates/tk-app/NOTES-deps.md`).
    use std::io;
    use std::path::Path;

    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyExW,
        RegDeleteTreeW, RegGetValueW, RegSetValueExW,
    };

    const ROOT: &str = "Software\\Classes\\magnet";
    const COMMAND: &str = "Software\\Classes\\magnet\\shell\\open\\command";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn command_for(exe: &Path) -> String {
        format!("\"{}\" \"%1\"", exe.display())
    }

    fn set_value(subkey: &str, name: Option<&str>, value: &str) -> io::Result<()> {
        let key_w = wide(subkey);
        let mut key: HKEY = std::ptr::null_mut();
        // SAFETY: NUL-terminated strings that outlive the calls; `key` is closed below.
        unsafe {
            let rc = RegCreateKeyExW(
                HKEY_CURRENT_USER,
                key_w.as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                std::ptr::null(),
                &mut key,
                std::ptr::null_mut(),
            );
            if rc != ERROR_SUCCESS {
                return Err(io::Error::from_raw_os_error(rc as i32));
            }
            let name_w = name.map(wide);
            let data = wide(value);
            let bytes = (data.len() * 2) as u32;
            let rc = RegSetValueExW(
                key,
                name_w.as_ref().map_or(std::ptr::null(), |n| n.as_ptr()),
                0,
                REG_SZ,
                data.as_ptr().cast::<u8>(),
                bytes,
            );
            RegCloseKey(key);
            if rc != ERROR_SUCCESS {
                return Err(io::Error::from_raw_os_error(rc as i32));
            }
        }
        Ok(())
    }

    fn get_command() -> Option<String> {
        let key_w = wide(COMMAND);
        let mut buf = vec![0u16; 2048];
        let mut size = (buf.len() * 2) as u32;
        // SAFETY: the buffer and its size in bytes are valid for the call.
        let rc = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key_w.as_ptr(),
                std::ptr::null(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buf.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if rc != ERROR_SUCCESS {
            return None;
        }
        let len = (size as usize / 2).min(buf.len());
        let text = String::from_utf16_lossy(&buf[..len]);
        Some(text.trim_end_matches('\0').to_owned())
    }

    pub(super) fn is_registered(exe: &Path) -> bool {
        get_command().is_some_and(|c| c.eq_ignore_ascii_case(&command_for(exe)))
    }

    pub(super) fn register(exe: &Path) -> io::Result<()> {
        set_value(ROOT, None, "URL:Magnet link")?;
        set_value(ROOT, Some("URL Protocol"), "")?;
        set_value(COMMAND, None, &command_for(exe))
    }

    pub(super) fn unregister(exe: &Path) -> io::Result<()> {
        // Another program's handler is left alone.
        if !is_registered(exe) {
            return Ok(());
        }
        let root = wide(ROOT);
        // SAFETY: NUL-terminated subkey path.
        let rc = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, root.as_ptr()) };
        if rc != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(rc as i32));
        }
        Ok(())
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod platform {
    use std::path::Path;

    pub(super) fn is_registered(_exe: &Path) -> bool {
        false
    }

    pub(super) fn register(_exe: &Path) -> std::io::Result<()> {
        Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "the magnet: handler comes from the app bundle on this platform"))
    }

    pub(super) fn unregister(exe: &Path) -> std::io::Result<()> {
        register(exe)
    }
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
    fn desktop_entry_quotes_the_executable() {
        let exe = std::path::Path::new("/opt/My Apps/tatakai$next%");
        assert_eq!(desktop_exec(exe), "\"/opt/My Apps/tatakai\\$next%%\" %u");
        let file = desktop_file(exe);
        assert!(file.contains("MimeType=x-scheme-handler/magnet;\n"));
        assert!(file.contains("NoDisplay=true"));
        assert!(is_dev_build(std::path::Path::new("/src/app/target/debug/tatakai-next")));
        assert!(!is_dev_build(std::path::Path::new("/usr/bin/tatakai-next")));
    }

    #[test]
    fn unregistering_edits_only_our_mimeapps_entry() {
        let text = "[Default Applications]\nx-scheme-handler/magnet=tatakai-next-magnet.desktop;\ntext/html=firefox.desktop\n[Added Associations]\nx-scheme-handler/magnet=qbittorrent.desktop;tatakai-next-magnet.desktop;\n";
        let next = mimeapps_without_us(text);
        assert_eq!(next, "[Default Applications]\ntext/html=firefox.desktop\n[Added Associations]\nx-scheme-handler/magnet=qbittorrent.desktop;\n");
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
