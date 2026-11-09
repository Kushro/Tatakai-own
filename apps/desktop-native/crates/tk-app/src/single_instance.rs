//! Single instance. A second launch brings the first window to the front and exits.
//!
//! Wave A (worker i, checklist `torrent.magnet-link-handler`): a second launch with a `magnet:` argument (the OS
//! scheme handler while the app is open) hands the link to the running instance through `<data>/magnet.pending`
//! (written atomically) and exits; the first instance polls that file every `PENDING_POLL` from a small thread and
//! passes the link to `magnet::remember`, where the UI picks it up (`Backend::take_pending_magnet`).
//! * Windows: a named mutex detects the running instance (as before), which is also brought to the front.
//! * Linux / macOS: an exclusive lock on `<data>/instance.lock` (`File::try_lock`, released by the OS when the
//!   process ends). Only a launch that carries a magnet defers to the running instance; a plain second launch keeps
//!   starting as it always did on these platforms.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// How often the running instance looks for a forwarded link.
const PENDING_POLL: Duration = Duration::from_secs(2);
/// File a second instance writes the forwarded link to.
pub const PENDING_FILE: &str = "magnet.pending";
#[cfg(not(windows))]
const LOCK_FILE: &str = "instance.lock";

/// Mutex name for the app id (per login session).
pub fn mutex_name(app_id: &str) -> String {
    format!("Local\\{app_id}.single-instance")
}

pub struct InstanceGuard {
    #[cfg(windows)]
    handle: isize,
    /// The held `instance.lock` (Unix); dropping it releases the lock.
    #[cfg(not(windows))]
    _lock: Option<std::fs::File>,
}

#[cfg(windows)]
impl Drop for InstanceGuard {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateMutexW and is closed exactly once.
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.handle as _) };
    }
}

/// The data directory (`AppPaths::resolve`), `None` when it cannot be resolved.
fn data_dir() -> Option<PathBuf> {
    crate::paths::AppPaths::resolve().ok().map(|p| p.root)
}

/// The `magnet:` argument of this process, if any.
fn magnet_arg() -> Option<String> {
    crate::magnet::from_args(std::env::args_os().map(|a| a.to_string_lossy().into_owned()))
}

/// Hand `link` to the running instance (atomic write of `<dir>/magnet.pending`).
pub fn forward(dir: &Path, link: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(PENDING_FILE);
    let tmp = dir.join(format!("{PENDING_FILE}.{}.tmp", std::process::id()));
    std::fs::write(&tmp, link.trim())?;
    std::fs::rename(&tmp, &path)
}

/// Take a forwarded link out of `<dir>/magnet.pending` (the file is removed). Non-magnets are dropped.
pub fn take_forwarded(dir: &Path) -> Option<String> {
    let path = dir.join(PENDING_FILE);
    let text = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(&path);
    let link = text.trim();
    crate::magnet::is_magnet(link).then(|| link.to_owned())
}

/// First instance: watch for links forwarded by later launches for the whole run.
fn watch_forwarded(dir: PathBuf) {
    // A file left by a launch that raced the previous run's exit is still a request of the user.
    let spawned = std::thread::Builder::new().name("tk-magnet-pending".into()).spawn(move || {
        loop {
            if let Some(link) = take_forwarded(&dir) {
                tracing::info!("magnet link forwarded by another launch");
                crate::magnet::remember(&link);
            }
            std::thread::sleep(PENDING_POLL);
        }
    });
    if let Err(e) = spawned {
        tracing::warn!(error = %e, "cannot watch for forwarded magnet links");
    }
}

/// `Some(guard)` if this is the first instance (keep the guard alive for the whole run), `None` if
/// another instance already runs (after asking it to show its window).
#[cfg(windows)]
pub fn acquire(app_id: &str, window_title: &str) -> Option<InstanceGuard> {
    use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
    use windows_sys::Win32::System::Threading::CreateMutexW;

    let name: Vec<u16> = mutex_name(app_id).encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `name` is NUL-terminated and outlives the call.
    let handle = unsafe { CreateMutexW(std::ptr::null(), 1, name.as_ptr()) };
    // SAFETY: reading the thread's last error right after the call.
    let already = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    if handle.is_null() {
        // Cannot tell: better to run than to refuse to start.
        tracing::warn!("CreateMutexW failed; running without single-instance protection");
        return Some(InstanceGuard { handle: 0 });
    }
    if already {
        // SAFETY: handle is valid and ours.
        unsafe { windows_sys::Win32::Foundation::CloseHandle(handle) };
        if let (Some(link), Some(dir)) = (magnet_arg(), data_dir())
            && let Err(e) = forward(&dir, &link)
        {
            tracing::warn!(error = %e, "forwarding the magnet link to the running instance failed");
        }
        focus_existing(window_title);
        return None;
    }
    if let Some(dir) = data_dir() {
        watch_forwarded(dir);
    }
    Some(InstanceGuard { handle: handle as isize })
}

#[cfg(windows)]
fn focus_existing(window_title: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, SW_RESTORE, SetForegroundWindow, ShowWindow};
    let title: Vec<u16> = window_title.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: plain Win32 calls with a NUL-terminated title.
    unsafe {
        let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
        if !hwnd.is_null() {
            ShowWindow(hwnd, SW_RESTORE);
            SetForegroundWindow(hwnd);
        }
    }
}

#[cfg(not(windows))]
pub fn acquire(_app_id: &str, _window_title: &str) -> Option<InstanceGuard> {
    let Some(dir) = data_dir() else { return Some(InstanceGuard { _lock: None }) };
    match lock_instance(&dir) {
        LockResult::Held(file) => {
            watch_forwarded(dir);
            Some(InstanceGuard { _lock: Some(file) })
        }
        LockResult::Busy => match magnet_arg() {
            Some(link) => match forward(&dir, &link) {
                Ok(()) => {
                    tracing::info!("handed the magnet link to the running instance");
                    None
                }
                Err(e) => {
                    tracing::warn!(error = %e, "forwarding the magnet link failed; starting a second instance");
                    Some(InstanceGuard { _lock: None })
                }
            },
            // Unchanged behaviour on these platforms: a plain second launch still starts.
            None => Some(InstanceGuard { _lock: None }),
        },
        LockResult::Unavailable => Some(InstanceGuard { _lock: None }),
    }
}

#[cfg(not(windows))]
enum LockResult {
    Held(std::fs::File),
    Busy,
    /// The lock file cannot be opened / locked (read-only data dir, no lock support): run unprotected.
    Unavailable,
}

#[cfg(not(windows))]
fn lock_instance(dir: &Path) -> LockResult {
    if std::fs::create_dir_all(dir).is_err() {
        return LockResult::Unavailable;
    }
    let file = match std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join(LOCK_FILE)) {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!(error = %e, "cannot open the instance lock");
            return LockResult::Unavailable;
        }
    };
    match file.try_lock() {
        Ok(()) => LockResult::Held(file),
        Err(std::fs::TryLockError::WouldBlock) => LockResult::Busy,
        Err(std::fs::TryLockError::Error(e)) => {
            tracing::warn!(error = %e, "cannot lock the instance lock");
            LockResult::Unavailable
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_is_session_local() {
        assert_eq!(mutex_name("app.tatakai.own.next"), "Local\\app.tatakai.own.next.single-instance");
    }

    #[test]
    fn forwarded_links_are_taken_once() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(take_forwarded(dir.path()), None);
        let link = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567";
        forward(dir.path(), &format!(" {link}\n")).unwrap();
        assert_eq!(take_forwarded(dir.path()).as_deref(), Some(link));
        assert_eq!(take_forwarded(dir.path()), None, "consumed");
        forward(dir.path(), "https://example.com").unwrap();
        assert_eq!(take_forwarded(dir.path()), None, "only magnets are honoured");
        assert!(!dir.path().join(PENDING_FILE).exists());
    }

    #[cfg(not(windows))]
    #[test]
    fn the_instance_lock_is_exclusive() {
        let dir = tempfile::tempdir().unwrap();
        let first = match lock_instance(dir.path()) {
            LockResult::Held(f) => f,
            _ => panic!("first lock"),
        };
        assert!(matches!(lock_instance(dir.path()), LockResult::Busy));
        drop(first);
        assert!(matches!(lock_instance(dir.path()), LockResult::Held(_)));
    }

    #[cfg(windows)]
    #[test]
    fn second_acquire_of_the_same_name_is_refused_until_the_first_drops() {
        let id = format!("tk-app-test-{}", std::process::id());
        let first = acquire(&id, "no such window title for tests").expect("first instance");
        assert!(acquire(&id, "no such window title for tests").is_none());
        drop(first);
        assert!(acquire(&id, "no such window title for tests").is_some());
    }
}
