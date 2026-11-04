//! "External mpv" player mode (docs/redesign/09 section 3.4): the machine's own mpv, with the user's own config,
//! scripts and key bindings, in its own window. The app talks to it over mpv's JSON IPC (`--input-ipc-server`):
//! a unix socket on Linux / macOS (under `$XDG_RUNTIME_DIR`, else the temp dir), a named pipe
//! (`\\.\pipe\tatakai-mpv-<pid>-<n>`) on Windows. Independent of the `mpv` feature: nothing here links libmpv.
//!
//! Spawn: `mpv --force-window=immediate --input-ipc-server=<endpoint> --title=<title> --start=<secs>
//! --referrer=.. --user-agent=.. --http-header-fields=.. --sub-file=<url>... -- <url>`. Never `--no-config`: the
//! point of this mode is the user's own setup. Header routing follows [`crate::headers::HeaderPlan`] like the
//! embedded player. IPC: `observe_property` on `time-pos`, `duration`, `pause`, `eof-reached`; `end-file` with
//! `reason: "eof"` -> [`ExternalEvent::Ended`]; the process exiting or the pipe closing -> [`ExternalEvent::Closed`]
//! (always the last event). [`ExternalPlayer::stop`] sends `quit` and kills the process if it is still alive after
//! a short grace period. Dropping an `ExternalPlayer` stops mpv: a session never outlives the screen that owns it.

use std::path::{Path, PathBuf};

use tokio::sync::broadcast;

use crate::types::{MediaSource, PlayerError};

/// What the external mpv reports (seconds).
#[derive(Debug, Clone, PartialEq)]
pub enum ExternalEvent {
    Position(f64),
    Duration(f64),
    Paused(bool),
    /// The file played to the end (mpv may stay open with `keep-open`, or exit right after: `Closed` follows).
    Ended,
    /// mpv exited (user closed the window, `q`, [`ExternalPlayer::stop`], crash). Always the last event.
    Closed,
    /// IPC or playback error (English diagnostic). Not terminal by itself.
    Error(String),
}

/// A running external mpv. Owned by the player screen.
pub struct ExternalPlayer {
    tx: broadcast::Sender<ExternalEvent>,
}

impl ExternalPlayer {
    /// Start `mpv_path` on `src` at `start_secs` with window title `title` and connect its IPC.
    /// Fails with [`PlayerError::External`] when the process cannot start or the IPC endpoint never appears.
    pub fn spawn(mpv_path: &Path, src: &MediaSource, start_secs: f64, title: &str) -> Result<Self, PlayerError> {
        // Contract stub (owner C, docs/redesign/09): process + IPC thread.
        let _ = (mpv_path, src, start_secs, title);
        let (tx, _) = broadcast::channel(64);
        let _ = Self { tx };
        Err(PlayerError::External("external mpv is not implemented yet".into()))
    }

    pub fn events(&self) -> broadcast::Receiver<ExternalEvent> {
        self.tx.subscribe()
    }

    /// Close mpv (`quit` over IPC, then kill). Idempotent; `Closed` is emitted once.
    pub fn stop(&self) {}

    /// False once the process exited.
    pub fn is_running(&self) -> bool {
        false
    }
}

fn exe_name() -> &'static str {
    if cfg!(windows) { "mpv.exe" } else { "mpv" }
}

/// Usual install locations checked after PATH.
fn well_known() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if cfg!(windows) {
        for (var, rel) in [
            ("ProgramFiles", "mpv\\mpv.exe"),
            ("LOCALAPPDATA", "Programs\\mpv\\mpv.exe"),
            ("USERPROFILE", "scoop\\apps\\mpv\\current\\mpv.exe"),
            ("ProgramData", "chocolatey\\bin\\mpv.exe"),
        ] {
            if let Some(base) = std::env::var_os(var) {
                v.push(PathBuf::from(base).join(rel));
            }
        }
    } else {
        for p in ["/usr/bin/mpv", "/usr/local/bin/mpv", "/opt/homebrew/bin/mpv", "/snap/bin/mpv"] {
            v.push(PathBuf::from(p));
        }
    }
    v
}

/// The mpv executable to use: `override_path` if given (and only it: a wrong custom path is reported as not
/// found instead of silently using another mpv), else the first `mpv` on PATH, else a well-known location.
pub fn find_mpv(override_path: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = override_path {
        return p.is_file().then(|| p.to_path_buf());
    }
    let exe = exe_name();
    std::env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .map(|dir| dir.join(exe))
        .chain(well_known())
        .find(|p| p.is_file())
}

/// First line of `mpv --version` ("mpv v0.40.0 Copyright ..."), `None` if it does not run. Blocking (spawns a
/// process): call from `spawn_blocking` or a background thread, never on the UI thread.
pub fn mpv_version(mpv_path: &Path) -> Option<String> {
    let mut cmd = std::process::Command::new(mpv_path);
    cmd.arg("--no-config").arg("--version").stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: no console flash.
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout).lines().next().map(|l| l.trim().to_owned()).filter(|l| !l.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_custom_path_is_used_alone() {
        let tmp = std::env::temp_dir().join("tk-player-find-mpv-missing").join(exe_name());
        assert_eq!(find_mpv(Some(&tmp)), None);
        let me = std::env::current_exe().unwrap();
        assert_eq!(find_mpv(Some(&me)), Some(me.clone()));
    }
}
