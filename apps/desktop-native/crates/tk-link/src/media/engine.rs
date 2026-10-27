//! The media engine behind a session generation. The real one runs FFmpeg inside a Windows Job Object; tests inject a
//! fake that PUTs synthetic segments to the sink, so session logic (window, seek, back-pressure, heartbeat) is tested
//! without FFmpeg.

use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;

use super::job::JobObject;

pub struct EngineJob {
    pub args: Vec<OsString>,
    /// FFmpeg's working directory (holds the Anime4K `.hook`).
    pub workdir: PathBuf,
    pub start_segment: u32,
    /// `http://127.0.0.1:<port>/sink/<token>/<generation>`
    pub sink_base: String,
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
}

pub struct FfmpegEngine {
    pub program: PathBuf,
    job: Arc<JobObject>,
}

impl FfmpegEngine {
    pub fn new(program: PathBuf) -> io::Result<Self> {
        Ok(Self { program, job: Arc::new(super::job::new_job()?) })
    }
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

#[async_trait]
impl Engine for FfmpegEngine {
    async fn spawn(&self, job: EngineJob) -> io::Result<Box<dyn EngineProcess>> {
        let mut cmd = tokio::process::Command::new(&self.program);
        cmd.args(&job.args)
            .current_dir(&job.workdir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        let child = cmd.spawn()?;
        #[cfg(windows)]
        if let Some(h) = child.raw_handle()
            && let Err(e) = self.job.assign(h)
        {
            tracing::warn!(error = %e, "could not assign FFmpeg to the job object");
        }
        #[cfg(not(windows))]
        let _ = &self.job;
        Ok(Box::new(Ffmpeg { child }))
    }
}
