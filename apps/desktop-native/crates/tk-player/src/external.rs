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

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::broadcast;

use crate::headers::HeaderPlan;
use crate::types::{MediaSource, PlayerError};

/// Sent when an extension gives no User-Agent (same as the embedded player: CDNs block mpv's own).
const DEFAULT_UA: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";
/// How long `spawn` waits for mpv to open its IPC endpoint.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// How long `stop` lets mpv quit on its own before killing it.
const QUIT_GRACE: Duration = Duration::from_secs(3);
/// mpv closed the IPC connection but the process lingers: kill it after this long.
const LINGER: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(50);

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

/// State shared by the handle and its IPC thread.
struct Shared {
    child: Mutex<Option<Child>>,
    /// Socket path (unix) or pipe name (Windows).
    endpoint: String,
    running: AtomicBool,
    stopping: AtomicBool,
}

impl Shared {
    fn kill(&self) {
        if let Ok(mut g) = self.child.lock()
            && let Some(child) = g.as_mut()
            && matches!(child.try_wait(), Ok(None))
        {
            let _ = child.kill();
            // Reap it (SIGKILL / TerminateProcess is immediate) so no zombie is left behind.
            let _ = child.wait();
        }
    }

    /// True once the process has exited (it is reaped here).
    fn exited(&self) -> bool {
        match self.child.lock() {
            Ok(mut g) => match g.as_mut() {
                Some(child) => !matches!(child.try_wait(), Ok(None)),
                None => true,
            },
            Err(_) => true,
        }
    }
}

/// A running external mpv. Owned by the player screen.
pub struct ExternalPlayer {
    tx: broadcast::Sender<ExternalEvent>,
    /// Subscribed before the IPC thread starts, so the first `events()` caller also gets the initial values
    /// mpv sends right after `observe_property` (a broadcast drops what nobody listens to).
    first_rx: Mutex<Option<broadcast::Receiver<ExternalEvent>>>,
    shared: Arc<Shared>,
}

impl ExternalPlayer {
    /// Start `mpv_path` on `src` at `start_secs` with window title `title` and connect its IPC.
    /// Fails with [`PlayerError::External`] when the process cannot start or the IPC endpoint never appears.
    /// Blocking (waits for the endpoint, usually a few hundred ms): call from `spawn_blocking`.
    pub fn spawn(mpv_path: &Path, src: &MediaSource, start_secs: f64, title: &str) -> Result<Self, PlayerError> {
        let endpoint = new_endpoint();
        #[cfg(unix)]
        let _ = std::fs::remove_file(&endpoint);
        let mut cmd = Command::new(mpv_path);
        cmd.args(mpv_args(&endpoint, src, start_secs, title))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW: no console flash.
            cmd.creation_flags(0x0800_0000);
        }
        let child = cmd.spawn().map_err(|e| PlayerError::External(format!("cannot start {}: {e}", mpv_path.display())))?;
        tracing::info!(mpv = %mpv_path.display(), %endpoint, "external mpv started");
        let shared = Arc::new(Shared {
            child: Mutex::new(Some(child)),
            endpoint,
            running: AtomicBool::new(true),
            stopping: AtomicBool::new(false),
        });

        let conn = match connect(&shared) {
            Ok(c) => c,
            Err(e) => {
                shared.kill();
                cleanup(&shared);
                shared.running.store(false, Ordering::SeqCst);
                return Err(e);
            }
        };
        let (tx, first_rx) = broadcast::channel(64);
        let spawned = {
            let (shared, tx) = (shared.clone(), tx.clone());
            std::thread::Builder::new().name("mpv-external-ipc".into()).spawn(move || run_ipc(conn, shared, tx))
        };
        if let Err(e) = spawned {
            shared.kill();
            cleanup(&shared);
            shared.running.store(false, Ordering::SeqCst);
            return Err(PlayerError::External(e.to_string()));
        }
        Ok(Self { tx, first_rx: Mutex::new(Some(first_rx)), shared })
    }

    pub fn events(&self) -> broadcast::Receiver<ExternalEvent> {
        match self.first_rx.lock().ok().and_then(|mut r| r.take()) {
            Some(rx) => rx,
            None => self.tx.subscribe(),
        }
    }

    /// Close mpv (`quit` over IPC, then kill). Idempotent; `Closed` is emitted once.
    pub fn stop(&self) {
        if self.shared.stopping.swap(true, Ordering::SeqCst) || !self.is_running() {
            return;
        }
        // A second client connection: the IPC thread's one is busy reading (and on Windows a synchronous
        // pipe handle cannot be written while a read is pending). mpv serves several clients.
        let sent = send_line(&self.shared.endpoint, r#"{"command":["quit"]}"#).is_ok();
        let shared = self.shared.clone();
        let spawned = std::thread::Builder::new().name("mpv-external-stop".into()).spawn(move || {
            let deadline = Instant::now() + if sent { QUIT_GRACE } else { Duration::ZERO };
            while shared.running.load(Ordering::SeqCst) && Instant::now() < deadline {
                std::thread::sleep(POLL);
            }
            if shared.running.load(Ordering::SeqCst) {
                tracing::info!("external mpv did not quit in time; killing it");
                shared.kill();
            }
        });
        if spawned.is_err() {
            self.shared.kill();
        }
    }

    /// False once the process exited.
    pub fn is_running(&self) -> bool {
        self.shared.running.load(Ordering::SeqCst)
    }
}

impl Drop for ExternalPlayer {
    fn drop(&mut self) {
        self.stop();
    }
}

/// A fresh IPC endpoint name: unique per process and per spawn.
fn new_endpoint() -> String {
    static N: AtomicU64 = AtomicU64::new(0);
    let name = format!("tatakai-mpv-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed));
    #[cfg(windows)]
    {
        format!(r"\\.\pipe\{name}")
    }
    #[cfg(not(windows))]
    {
        let dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|d| d.is_dir())
            .unwrap_or_else(std::env::temp_dir);
        dir.join(format!("{name}.sock")).to_string_lossy().into_owned()
    }
}

/// mpv expands `${property}` in `--title`; a literal `$` is written `$$`.
fn escape_title(title: &str) -> String {
    title.replace('$', "$$")
}

/// The command line (pure). Never `--no-config`: this mode exists to use the user's own config.
fn mpv_args(endpoint: &str, src: &MediaSource, start_secs: f64, title: &str) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec![
        "--force-window=immediate".into(),
        format!("--input-ipc-server={endpoint}").into(),
    ];
    if !title.is_empty() {
        args.push(format!("--title={}", escape_title(title)).into());
    }
    if start_secs.is_finite() && start_secs > 0.0 {
        args.push(format!("--start={start_secs:.3}").into());
    }
    let plan = HeaderPlan::from_headers(&src.headers);
    if let Some(r) = plan.referrer {
        args.push(format!("--referrer={r}").into());
    }
    args.push(format!("--user-agent={}", plan.user_agent.as_deref().unwrap_or(DEFAULT_UA)).into());
    // `-append` adds one item without splitting on commas (cookies and the like contain them).
    for field in plan.fields {
        args.push(format!("--http-header-fields-append={field}").into());
    }
    for sub in &src.subtitles {
        if !sub.url.is_empty() {
            // `--sub-file` is `--sub-files-append`: one item, no list splitting.
            args.push(format!("--sub-file={}", sub.url).into());
        }
    }
    args.push("--".into());
    args.push(src.url.clone().into());
    args
}

#[cfg(unix)]
type Conn = std::os::unix::net::UnixStream;
#[cfg(windows)]
type Conn = std::fs::File;

#[cfg(unix)]
fn open_endpoint(endpoint: &str) -> std::io::Result<Conn> {
    std::os::unix::net::UnixStream::connect(endpoint)
}

#[cfg(windows)]
fn open_endpoint(endpoint: &str) -> std::io::Result<Conn> {
    std::fs::OpenOptions::new().read(true).write(true).open(endpoint)
}

/// One JSON command on its own short-lived connection.
fn send_line(endpoint: &str, line: &str) -> std::io::Result<()> {
    let mut c = open_endpoint(endpoint)?;
    c.write_all(line.as_bytes())?;
    c.write_all(b"\n")?;
    c.flush()
}

/// Wait for mpv to open its endpoint, then subscribe to the properties we show. Fails if mpv exits first or
/// the endpoint never appears.
fn connect(shared: &Shared) -> Result<Conn, PlayerError> {
    let deadline = Instant::now() + CONNECT_TIMEOUT;
    let mut conn = loop {
        match open_endpoint(&shared.endpoint) {
            Ok(c) => break c,
            Err(e) => {
                if shared.exited() {
                    return Err(PlayerError::External("mpv exited before opening its IPC endpoint".into()));
                }
                if Instant::now() >= deadline {
                    return Err(PlayerError::External(format!("mpv IPC endpoint never appeared: {e}")));
                }
                std::thread::sleep(POLL);
            }
        }
    };
    // Written before the IPC thread starts reading (Windows: no concurrent read/write on one handle).
    let mut hello = String::new();
    for (id, name) in OBSERVED.iter().enumerate() {
        hello.push_str(&format!("{{\"command\":[\"observe_property\",{},\"{name}\"]}}\n", id + 1));
    }
    conn.write_all(hello.as_bytes())
        .and_then(|()| conn.flush())
        .map_err(|e| PlayerError::External(format!("mpv IPC write failed: {e}")))?;
    Ok(conn)
}

const OBSERVED: [&str; 4] = ["time-pos", "duration", "pause", "eof-reached"];

/// Reads mpv's messages until the connection closes, then waits for the process to end and emits `Closed`.
fn run_ipc(conn: Conn, shared: Arc<Shared>, tx: broadcast::Sender<ExternalEvent>) {
    let mut reader = BufReader::new(conn);
    let mut line = String::new();
    let mut ended = false;
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                if let Some(ev) = parse_message(&line) {
                    if ev == ExternalEvent::Ended {
                        // `end-file` (eof) and `eof-reached` both report the end: send it once.
                        if std::mem::replace(&mut ended, true) {
                            continue;
                        }
                    } else if matches!(ev, ExternalEvent::Position(p) if p < 1.0) {
                        // Replayed from the start (mpv's own playlist / loop): a later end counts again.
                        ended = false;
                    }
                    let _ = tx.send(ev);
                }
            }
            Err(e) => {
                tracing::debug!(error = %e, "external mpv IPC closed");
                break;
            }
        }
    }
    // The connection closes when mpv quits; give it a moment to exit, then make sure it does.
    let deadline = Instant::now() + LINGER;
    while !shared.exited() {
        if Instant::now() >= deadline {
            tracing::warn!("external mpv closed its IPC but kept running; killing it");
            shared.kill();
        }
        std::thread::sleep(POLL);
    }
    cleanup(&shared);
    shared.running.store(false, Ordering::SeqCst);
    tracing::info!("external mpv exited");
    let _ = tx.send(ExternalEvent::Closed);
}

/// Remove the unix socket file (mpv normally does; a crash leaves it behind). Named pipes vanish by themselves.
fn cleanup(shared: &Shared) {
    #[cfg(unix)]
    {
        let _ = std::fs::remove_file(&shared.endpoint);
    }
    #[cfg(not(unix))]
    let _ = shared;
}

/// One IPC line -> event (pure). Command replies, `null` data and unrelated events give `None`.
fn parse_message(line: &str) -> Option<ExternalEvent> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    match v.get("event")?.as_str()? {
        "property-change" => {
            let data = v.get("data")?;
            match v.get("name")?.as_str()? {
                "time-pos" => data.as_f64().filter(|p| p.is_finite()).map(|p| ExternalEvent::Position(p.max(0.0))),
                "duration" => data.as_f64().filter(|d| d.is_finite() && *d > 0.0).map(ExternalEvent::Duration),
                "pause" => data.as_bool().map(ExternalEvent::Paused),
                "eof-reached" => (data.as_bool() == Some(true)).then_some(ExternalEvent::Ended),
                _ => None,
            }
        }
        "end-file" => match v.get("reason").and_then(|r| r.as_str()) {
            Some("eof") => Some(ExternalEvent::Ended),
            Some("error") => {
                let detail = v.get("file_error").and_then(|e| e.as_str()).unwrap_or("unknown error");
                Some(ExternalEvent::Error(format!("mpv could not play the file: {detail}")))
            }
            _ => None,
        },
        _ => None,
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

    fn strs(args: &[OsString]) -> Vec<String> {
        args.iter().map(|a| a.to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn args_route_headers_and_subtitles_and_never_disable_the_user_config() {
        let src = MediaSource {
            url: "https://cdn.example/v.m3u8".into(),
            headers: vec![
                ("Referer".into(), "https://site.example/".into()),
                ("Cookie".into(), "a=b, c=d".into()),
            ],
            subtitles: vec![crate::types::SubtitleSource { url: "https://cdn.example/es.vtt".into(), lang: "es".into(), label: "ES".into() }],
        };
        let a = strs(&mpv_args("/run/user/1000/x.sock", &src, 83.5, "Show $x · E3"));
        assert!(a.contains(&"--force-window=immediate".to_owned()));
        assert!(a.contains(&"--input-ipc-server=/run/user/1000/x.sock".to_owned()));
        assert!(a.contains(&"--title=Show $$x · E3".to_owned()));
        assert!(a.contains(&"--start=83.500".to_owned()));
        assert!(a.contains(&"--referrer=https://site.example/".to_owned()));
        assert!(a.contains(&format!("--user-agent={DEFAULT_UA}")));
        assert!(a.contains(&"--http-header-fields-append=Cookie: a=b, c=d".to_owned()));
        assert!(a.contains(&"--sub-file=https://cdn.example/es.vtt".to_owned()));
        assert!(!a.iter().any(|x| x.starts_with("--no-config") || x.starts_with("--config")));
        assert_eq!(&a[a.len() - 2..], ["--", "https://cdn.example/v.m3u8"]);
        let b = strs(&mpv_args("p", &MediaSource { url: "u".into(), ..Default::default() }, 0.0, ""));
        assert!(!b.iter().any(|x| x.starts_with("--start") || x.starts_with("--title") || x.starts_with("--referrer")));
    }

    #[test]
    fn ipc_messages_map_to_events() {
        let pc = |name: &str, data: &str| format!(r#"{{"event":"property-change","id":1,"name":"{name}","data":{data}}}"#);
        assert_eq!(parse_message(&pc("time-pos", "12.5")), Some(ExternalEvent::Position(12.5)));
        assert_eq!(parse_message(&pc("time-pos", "null")), None);
        assert_eq!(parse_message(&pc("duration", "1420.0")), Some(ExternalEvent::Duration(1420.0)));
        assert_eq!(parse_message(&pc("duration", "0")), None);
        assert_eq!(parse_message(&pc("pause", "true")), Some(ExternalEvent::Paused(true)));
        assert_eq!(parse_message(&pc("eof-reached", "true")), Some(ExternalEvent::Ended));
        assert_eq!(parse_message(&pc("eof-reached", "false")), None);
        assert_eq!(parse_message(r#"{"event":"end-file","reason":"eof"}"#), Some(ExternalEvent::Ended));
        assert!(matches!(
            parse_message(r#"{"event":"end-file","reason":"error","file_error":"loading failed"}"#),
            Some(ExternalEvent::Error(m)) if m.contains("loading failed")
        ));
        assert_eq!(parse_message(r#"{"event":"end-file","reason":"quit"}"#), None);
        assert_eq!(parse_message(r#"{"request_id":0,"error":"success"}"#), None);
        assert_eq!(parse_message("not json"), None);
    }

    #[test]
    fn endpoints_are_unique() {
        assert_ne!(new_endpoint(), new_endpoint());
    }
}
