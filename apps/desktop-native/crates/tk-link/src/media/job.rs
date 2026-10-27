//! Windows Job Object: the FFmpeg process tree dies with the app (`KILL_ON_JOB_CLOSE`). No-op elsewhere.

use std::io;

#[cfg(windows)]
mod imp {
    use std::io;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectExtendedLimitInformation, SetInformationJobObject,
    };

    pub struct JobObject(HANDLE);

    // SAFETY: a job handle is just a kernel object reference; the Win32 job APIs are thread-safe.
    unsafe impl Send for JobObject {}
    unsafe impl Sync for JobObject {}

    impl JobObject {
        pub fn new() -> io::Result<Self> {
            // SAFETY: plain FFI calls with valid arguments; the handle is closed in Drop.
            unsafe {
                let h = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if h.is_null() {
                    return Err(io::Error::last_os_error());
                }
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let ok = SetInformationJobObject(
                    h,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const _,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                if ok == 0 {
                    let e = io::Error::last_os_error();
                    CloseHandle(h);
                    return Err(e);
                }
                Ok(Self(h))
            }
        }

        /// Put a (just spawned) process into the job. `process` is its raw HANDLE.
        pub fn assign(&self, process: std::os::windows::io::RawHandle) -> io::Result<()> {
            // SAFETY: `process` is a live process handle owned by the caller.
            if unsafe { AssignProcessToJobObject(self.0, process as HANDLE) } == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    }

    impl Drop for JobObject {
        fn drop(&mut self) {
            // SAFETY: handle created in `new`, closed exactly once.
            unsafe { CloseHandle(self.0) };
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use std::io;

    pub struct JobObject;

    impl JobObject {
        pub fn new() -> io::Result<Self> {
            Ok(Self)
        }
    }
}

pub use imp::JobObject;

pub fn new_job() -> io::Result<JobObject> {
    JobObject::new()
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn a_job_object_can_be_created_and_a_child_assigned() {
        let job = new_job().unwrap();
        let mut child = std::process::Command::new("cmd").args(["/C", "ping -n 30 127.0.0.1 >nul"]).stdout(std::process::Stdio::null()).spawn().unwrap();
        use std::os::windows::io::AsRawHandle;
        job.assign(child.as_raw_handle()).unwrap();
        // dropping the job kills the whole tree: the 29 s ping is cut short
        let started = std::time::Instant::now();
        drop(job);
        child.wait().unwrap();
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
    }
}
