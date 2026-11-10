//! The media engine behind a session generation. The real one runs FFmpeg (or, for LSFG profiles, the chain
//! FFmpeg -> `temple-lsfg` -> FFmpeg connected by pipes) inside a Windows Job Object / with `PR_SET_PDEATHSIG` on
//! Linux; tests inject a fake that PUTs synthetic segments to the sink, so session logic (window, seek,
//! back-pressure, heartbeat) is tested without FFmpeg.
//!
//! The chain follows Enigmata-Temple `crates/temple-core/src/media.rs` `run_chain` (AGPL-3.0, same author): stdout of
//! each stage is stdin of the next, the first failure kills the others and all of them are awaited; every Vulkan
//! process gets `DISABLE_LSFGVK=1` ([`super::lsfg::VULKAN_EXTRA_ENV`]).

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::job::JobObject;
use super::lsfg::VULKAN_EXTRA_ENV;
use super::probe::{self, FrameRate};

/// The first two LSFG stages; the third (the encoder that PUTs) is [`EngineJob::args`].
pub struct LsfgStages {
    /// FFmpeg A: decode + scale -> raw RGBA on stdout.
    pub decode: Vec<OsString>,
    /// `temple-lsfg`.
    pub helper: PathBuf,
    pub helper_args: Vec<OsString>,
}

pub struct EngineJob {
    /// The FFmpeg that PUTs the segments (the only process, or the last stage of the LSFG chain).
    pub args: Vec<OsString>,
    /// FFmpeg's working directory (holds the Anime4K `.hook`).
    pub workdir: PathBuf,
    pub start_segment: u32,
    /// `http://127.0.0.1:<port>/sink/<token>/<generation>`
    pub sink_base: String,
    /// LSFG profiles: the stages that feed `args` through stdin.
    pub lsfg: Option<LsfgStages>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineExit {
    Success,
    Failed,
}

#[async_trait]
pub trait EngineProcess: Send {
    async fn wait(&mut self) -> EngineExit;
    fn kill(&mut self);
}

#[async_trait]
pub trait Engine: Send + Sync {
    async fn spawn(&self, job: EngineJob) -> io::Result<Box<dyn EngineProcess>>;

    /// Frame rate of the first video stream of the loopback input (LSFG needs it). `None` = unknown.
    async fn source_rate(&self, _input_url: &str) -> Option<FrameRate> {
        None
    }
}

pub struct FfmpegEngine {
    pub program: PathBuf,
    /// `ffprobe` next to FFmpeg, when present.
    pub ffprobe: Option<PathBuf>,
    job: Arc<JobObject>,
}

impl FfmpegEngine {
    pub fn new(program: PathBuf) -> io::Result<Self> {
        let ffprobe = sibling_ffprobe(&program);
        Ok(Self { program, ffprobe, job: Arc::new(super::job::new_job()?) })
    }

    fn command(&self, program: &Path, args: &[OsString], workdir: &Path) -> tokio::process::Command {
        let mut cmd = tokio::process::Command::new(program);
        cmd.args(args).current_dir(workdir).envs(VULKAN_EXTRA_ENV).kill_on_drop(true);
        // Diagnostics: with `TATAKAI_LINK_MEDIA_LOG=<dir>` each media process appends its stderr to
        // `<dir>/<program>.log` (FFmpeg / temple-lsfg errors); otherwise stderr is discarded.
        let log = std::env::var_os("TATAKAI_LINK_MEDIA_LOG").map(PathBuf::from).and_then(|dir| {
            let name = program.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "media".into());
            std::fs::create_dir_all(&dir).ok()?;
            std::fs::OpenOptions::new().create(true).append(true).open(dir.join(format!("{name}.log"))).ok()
        });
        match log {
            Some(f) => cmd.stderr(Stdio::from(f)),
            None => cmd.stderr(Stdio::null()),
        };
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        super::job::die_with_parent(&mut cmd);
        cmd
    }

    fn adopt(&self, child: &tokio::process::Child) {
        #[cfg(windows)]
        if let Some(h) = child.raw_handle()
            && let Err(e) = self.job.assign(h)
        {
            tracing::warn!(error = %e, "could not assign a media process to the job object");
        }
        #[cfg(not(windows))]
        let _ = (&self.job, child);
    }
}

/// `ffprobe[.exe]` in FFmpeg's folder.
pub fn sibling_ffprobe(ffmpeg: &Path) -> Option<PathBuf> {
    let p = ffmpeg.with_file_name(if cfg!(windows) { "ffprobe.exe" } else { "ffprobe" });
    p.is_file().then_some(p)
}

struct Ffmpeg {
    child: tokio::process::Child,
}

#[async_trait]
impl EngineProcess for Ffmpeg {
    async fn wait(&mut self) -> EngineExit {
        match self.child.wait().await {
            Ok(s) if s.success() => EngineExit::Success,
            _ => EngineExit::Failed,
        }
    }

    fn kill(&mut self) {
        let _ = self.child.start_kill();
    }
}

/// A running process chain: one task per stage waits for it (or kills it when asked) and reports its exit.
struct Chain {
    kill: CancellationToken,
    exits: mpsc::UnboundedReceiver<(usize, bool)>,
    stages: usize,
}

impl Chain {
    fn start(children: Vec<tokio::process::Child>) -> Self {
        let kill = CancellationToken::new();
        let (tx, exits) = mpsc::unbounded_channel();
        let stages = children.len();
        for (i, mut child) in children.into_iter().enumerate() {
            let (tx, kill) = (tx.clone(), kill.clone());
            tokio::spawn(async move {
                let status = tokio::select! {
                    s = child.wait() => s,
                    () = kill.cancelled() => {
                        let _ = child.kill().await;
                        child.wait().await
                    }
                };
                let _ = tx.send((i, status.is_ok_and(|s| s.success())));
            });
        }
        Self { kill, exits, stages }
    }
}

#[async_trait]
impl EngineProcess for Chain {
    /// Success only when every stage exits with 0; the first failure kills the rest (the others usually die of the
    /// broken pipe anyway) and all are awaited.
    async fn wait(&mut self) -> EngineExit {
        let mut ok = true;
        for _ in 0..self.stages {
            match self.exits.recv().await {
                Some((i, true)) => tracing::trace!(stage = i, "media stage done"),
                Some((i, false)) => {
                    if ok {
                        tracing::warn!(stage = ["decode", "lsfg", "encode"].get(i).copied().unwrap_or("?"), "a stage of the LSFG chain failed");
                    }
                    ok = false;
                    self.kill.cancel();
                }
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if ok { EngineExit::Success } else { EngineExit::Failed }
    }

    fn kill(&mut self) {
        self.kill.cancel();
    }
}

impl Drop for Chain {
    fn drop(&mut self) {
        self.kill.cancel();
    }
}

#[async_trait]
impl Engine for FfmpegEngine {
    async fn spawn(&self, job: EngineJob) -> io::Result<Box<dyn EngineProcess>> {
        let Some(lsfg) = &job.lsfg else {
            let mut cmd = self.command(&self.program, &job.args, &job.workdir);
            cmd.stdin(Stdio::null()).stdout(Stdio::null());
            let child = cmd.spawn()?;
            self.adopt(&child);
            return Ok(Box::new(Ffmpeg { child }));
        };
        // FFmpeg A | temple-lsfg | FFmpeg B. A failed spawn drops (kill_on_drop) the stages already started.
        let mut decode = self.command(&self.program, &lsfg.decode, &job.workdir);
        decode.stdin(Stdio::null()).stdout(Stdio::piped());
        let mut a = decode.spawn()?;
        self.adopt(&a);
        let a_out: Stdio = a.stdout.take().ok_or_else(|| io::Error::other("decode stage without stdout"))?.try_into()?;
        let mut helper = self.command(&lsfg.helper, &lsfg.helper_args, &job.workdir);
        helper.stdin(a_out).stdout(Stdio::piped());
        drop(decode);
        let mut h = helper.spawn()?;
        self.adopt(&h);
        let h_out: Stdio = h.stdout.take().ok_or_else(|| io::Error::other("lsfg stage without stdout"))?.try_into()?;
        drop(helper);
        let mut encode = self.command(&self.program, &job.args, &job.workdir);
        encode.stdin(h_out).stdout(Stdio::null());
        let e = encode.spawn()?;
        self.adopt(&e);
        drop(encode);
        Ok(Box::new(Chain::start(vec![a, h, e])))
    }

    async fn source_rate(&self, input_url: &str) -> Option<FrameRate> {
        let ffprobe = self.ffprobe.as_ref()?;
        let mut cmd = tokio::process::Command::new(ffprobe);
        cmd.args(probe::ffprobe_args(input_url)).stdin(Stdio::null()).stderr(Stdio::null()).kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000);
        super::job::die_with_parent(&mut cmd);
        let out = tokio::time::timeout(std::time::Duration::from_secs(20), cmd.output()).await.ok()?.ok()?;
        if !out.status.success() || out.stdout.len() > 256 * 1024 {
            tracing::warn!("the source frame-rate probe failed");
            return None;
        }
        probe::parse_ffprobe_rate(&String::from_utf8_lossy(&out.stdout))
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// The chain with stand-in programs: `sh` stages piping into each other (no FFmpeg, no GPU).
    fn sh_job(decode: &str, helper: &str, encode: &str) -> EngineJob {
        let dir = std::env::temp_dir();
        EngineJob {
            args: vec!["-c".into(), encode.into()],
            workdir: dir,
            start_segment: 0,
            sink_base: String::new(),
            lsfg: Some(LsfgStages { decode: vec!["-c".into(), decode.into()], helper: PathBuf::from("/bin/sh"), helper_args: vec!["-c".into(), helper.into()] }),
        }
    }

    fn sh_engine() -> FfmpegEngine {
        FfmpegEngine { program: PathBuf::from("/bin/sh"), ffprobe: None, job: Arc::new(super::super::job::new_job().unwrap()) }
    }

    #[tokio::test]
    async fn a_chain_pipes_stages_and_succeeds_when_all_do() {
        let e = sh_engine();
        // stage 3 fails unless it receives what stages 1 and 2 produced
        let job = sh_job("echo frame", "read x; echo \"$x+gen\"; [ \"$DISABLE_LSFGVK\" = 1 ]", "read y; [ \"$y\" = frame+gen ]");
        let mut p = e.spawn(job).await.unwrap();
        assert_eq!(p.wait().await, EngineExit::Success);
    }

    #[tokio::test]
    async fn the_first_failure_fails_the_chain_and_kill_stops_every_stage() {
        let e = sh_engine();
        let mut p = e.spawn(sh_job("exit 3", "cat", "cat")).await.unwrap();
        assert_eq!(p.wait().await, EngineExit::Failed);
        // long-running stages die on kill
        let mut p = e.spawn(sh_job("sleep 30", "sleep 30", "sleep 30")).await.unwrap();
        let started = std::time::Instant::now();
        p.kill();
        assert_eq!(p.wait().await, EngineExit::Failed);
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
    }
}
