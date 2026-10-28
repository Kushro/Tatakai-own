//! Single instance via a named mutex. A second launch brings the first window to the front and exits.

/// Mutex name for the app id (per login session).
pub fn mutex_name(app_id: &str) -> String {
    format!("Local\\{app_id}.single-instance")
}

pub struct InstanceGuard {
    #[cfg(windows)]
    handle: isize,
}

#[cfg(windows)]
impl Drop for InstanceGuard {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateMutexW and is closed exactly once.
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.handle as _) };
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
        focus_existing(window_title);
        return None;
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
    Some(InstanceGuard {})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_is_session_local() {
        assert_eq!(mutex_name("app.tatakai.own.next"), "Local\\app.tatakai.own.next.single-instance");
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
