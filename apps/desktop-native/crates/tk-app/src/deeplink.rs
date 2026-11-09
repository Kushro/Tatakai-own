//! `tatakai://extensionserver/connect?server=<origin>&code=<code>` links from the OS (wave B, worker **j**; checklist
//! `ext.deep-link.server`; docs/redesign/13 section 3.7). Same shape as `magnet.rs` (wave A, worker i): `from_args`
//! picks the link out of the command line (implemented, pure), `main.rs` calls `remember` before the window exists,
//! `live/ext_server.rs::take_pending_link` hands it to the Extensions page, which opens the pairing sheet after the
//! user confirms. Registering the `tatakai:` scheme per user (Linux `.desktop` with `x-scheme-handler/tatakai`,
//! Windows `HKCU\Software\Classes\tatakai`) and forwarding from a second instance (`single_instance.rs`, a
//! `<data>/deeplink.pending` file like `magnet.pending`) are j's, following `magnet::set_registered`:
//! [`ensure_registered`] runs at start-up (release builds only) and `single_instance` forwards the link.

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

// ---------------------------------------------------------------------------------------------
// Registering the `tatakai:` scheme (per user, no installer, no elevation; same approach as `magnet.rs`)
// ---------------------------------------------------------------------------------------------

/// Desktop file id of the handler (Linux).
pub const DESKTOP_ID: &str = "tatakai-next-link.desktop";
/// MIME type of the scheme (Linux).
const SCHEME_MIME: &str = "x-scheme-handler/tatakai";

/// An executable inside a cargo `target/` directory (dev builds move on every build and must not be registered).
fn is_dev_build(exe: &std::path::Path) -> bool {
    exe.components().any(|c| c.as_os_str() == "target")
}

/// Whether this platform / build can register the scheme (Linux and Windows release builds; on macOS the handler
/// comes from the bundle's `Info.plist`).
pub fn supported() -> bool {
    cfg!(any(windows, target_os = "linux")) && std::env::current_exe().is_ok_and(|exe| !is_dev_build(&exe))
}

/// Make this executable the `tatakai:` handler of the user when it is not already (called at start-up, best effort:
/// `live/ext_server.rs::on_start`). Unsupported builds answer `Ok(())` without doing anything.
pub fn ensure_registered() -> std::io::Result<()> {
    if !supported() {
        return Ok(());
    }
    let exe = std::env::current_exe()?;
    if platform::is_registered(&exe) {
        return Ok(());
    }
    platform::register(&exe)
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
        "[Desktop Entry]\nType=Application\nName=Tatakai\nComment=Open Tatakai links (extension server pairing)\nExec={}\nMimeType={SCHEME_MIME};\nNoDisplay=true\nTerminal=false\n",
        desktop_exec(exe)
    )
}

#[cfg(target_os = "linux")]
mod platform {
    use std::io;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use super::{DESKTOP_ID, SCHEME_MIME};

    fn applications_dir() -> io::Result<PathBuf> {
        if let Some(p) = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).filter(|p| p.is_absolute()) {
            return Ok(p.join("applications"));
        }
        let home = std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "HOME is not set"))?;
        Ok(home.join(".local/share/applications"))
    }

    fn query_default() -> Option<String> {
        let out = Command::new("xdg-mime").args(["query", "default", SCHEME_MIME]).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
    }

    pub(super) fn is_registered(exe: &Path) -> bool {
        let Ok(dir) = applications_dir() else { return false };
        let ours = std::fs::read_to_string(dir.join(DESKTOP_ID)).is_ok_and(|text| text.contains(&super::desktop_exec(exe)));
        ours && query_default().as_deref() == Some(DESKTOP_ID)
    }

    pub(super) fn register(exe: &Path) -> io::Result<()> {
        let dir = applications_dir()?;
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(DESKTOP_ID);
        let tmp = path.with_extension("desktop.tmp");
        std::fs::write(&tmp, super::desktop_file(exe))?;
        std::fs::rename(&tmp, &path)?;
        let status = Command::new("xdg-mime")
            .args(["default", DESKTOP_ID, SCHEME_MIME])
            .status()
            .map_err(|e| io::Error::new(e.kind(), format!("xdg-mime is not available ({e}); install xdg-utils")))?;
        if !status.success() {
            return Err(io::Error::other(format!("xdg-mime default failed ({status})")));
        }
        let _ = Command::new("update-desktop-database").arg(&dir).status();
        Ok(())
    }
}

#[cfg(windows)]
mod platform {
    //! `HKCU\Software\Classes\tatakai` (per user, no elevation), like `magnet.rs` (windows-sys
    //! `Win32_System_Registry`, already requested by worker i).
    use std::io;
    use std::path::Path;

    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RRF_RT_REG_SZ, RegCloseKey, RegCreateKeyExW,
        RegGetValueW, RegSetValueExW,
    };

    const ROOT: &str = "Software\\Classes\\tatakai";
    const COMMAND: &str = "Software\\Classes\\tatakai\\shell\\open\\command";

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
        Some(String::from_utf16_lossy(&buf[..len]).trim_end_matches('\0').to_owned())
    }

    pub(super) fn is_registered(exe: &Path) -> bool {
        get_command().is_some_and(|c| c.eq_ignore_ascii_case(&command_for(exe)))
    }

    pub(super) fn register(exe: &Path) -> io::Result<()> {
        set_value(ROOT, None, "URL:Tatakai link")?;
        set_value(ROOT, Some("URL Protocol"), "")?;
        set_value(COMMAND, None, &command_for(exe))
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod platform {
    use std::path::Path;

    pub(super) fn is_registered(_exe: &Path) -> bool {
        true
    }

    pub(super) fn register(_exe: &Path) -> std::io::Result<()> {
        Ok(())
    }
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
    fn desktop_entry_quotes_the_executable() {
        let exe = std::path::Path::new("/opt/Tatakai Next/tatakai-next");
        assert_eq!(desktop_exec(exe), "\"/opt/Tatakai Next/tatakai-next\" %u");
        let file = desktop_file(exe);
        assert!(file.contains("MimeType=x-scheme-handler/tatakai;"));
        assert!(file.contains("NoDisplay=true"));
        assert!(is_dev_build(std::path::Path::new("/src/target/debug/tatakai-next")));
        assert!(!is_dev_build(exe));
    }

    #[test]
    fn remember_and_take() {
        remember("tatakai://extensionserver/connect?server=a&code=b");
        assert!(take().is_some());
        assert!(take().is_none());
    }
    // ---- F6 (parity wave B) ----
    // (PENDING is process-wide and shared with `remember_and_take`, so these stay on the pure functions.)

    #[test]
    fn link_bounds_and_parameter_names() {
        assert!(!is_connect_link(CONNECT_PREFIX), "prefix alone");
        assert!(!is_connect_link(""));
        assert!(is_connect_link("  tatakai://extensionserver/connect?server=a&code=b \n"));
        // parameters must be whole names
        assert!(!is_connect_link("tatakai://extensionserver/connect?xserver=a&xcode=b"));
        assert!(!is_connect_link("tatakai://extensionserver/connect?server=a&mycode=b"));
        // other paths of the scheme are not pairing links
        assert!(!is_connect_link("tatakai://extensionserver/other?server=a&code=b"));
        assert!(!is_connect_link("https://extensionserver/connect?server=a&code=b"));
        // 4 KiB cap
        let base = "tatakai://extensionserver/connect?server=a&code=b&pad=";
        let at_cap = format!("{base}{}", "x".repeat(4 * 1024 - base.len()));
        assert_eq!(at_cap.len(), 4 * 1024);
        assert!(is_connect_link(&at_cap));
        assert!(!is_connect_link(&format!("{at_cap}x")));
    }

    #[test]
    fn from_args_skips_the_program_and_takes_the_first_link() {
        let link = "tatakai://extensionserver/connect?server=a&code=b".to_owned();
        assert_eq!(from_args([link.clone()]), None, "argv[0] is never a link");
        assert_eq!(from_args(Vec::<String>::new()), None);
        let args = vec![
            "exe".to_owned(),
            "--flag".to_owned(),
            format!(" {link} "),
            "tatakai://extensionserver/connect?server=z&code=y".to_owned(),
        ];
        assert_eq!(from_args(args).as_deref(), Some(link.as_str()));
        assert_eq!(from_args(["exe".to_owned(), "magnet:?xt=urn:btih:abc".to_owned()]), None);
    }

    #[test]
    fn desktop_exec_escapes_special_characters() {
        let exe = std::path::Path::new("/opt/a\"b`c$d\\e%f/tk");
        assert_eq!(desktop_exec(exe), "\"/opt/a\\\"b\\`c\\$d\\\\e%%f/tk\" %u");
        let file = desktop_file(exe);
        assert!(file.starts_with("[Desktop Entry]\n"));
        assert!(file.contains(&format!("Exec={}\n", desktop_exec(exe))));
        assert_eq!(DESKTOP_ID, "tatakai-next-link.desktop");
    }

    #[test]
    fn only_a_target_component_marks_a_dev_build() {
        assert!(!is_dev_build(std::path::Path::new("/home/u/mytarget/tk")));
        assert!(!is_dev_build(std::path::Path::new("/usr/bin/target-tk")));
        assert!(is_dev_build(std::path::Path::new("C:/src/target/release/tk.exe")));
    }
}
