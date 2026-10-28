//! File logging to `%APPDATA%\Tatakai-Next\logs`. No telemetry, nothing leaves the machine.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;

pub const LOG_FILE: &str = "tatakai-next.log";
const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

/// Rename an oversized log to `<name>.old` (replacing the previous `.old`).
pub fn rotate_if_large(dir: &Path, max_bytes: u64) -> io::Result<bool> {
    let current = dir.join(LOG_FILE);
    match std::fs::metadata(&current) {
        Ok(m) if m.len() > max_bytes => {
            let old = dir.join(format!("{LOG_FILE}.old"));
            if old.exists() {
                std::fs::remove_file(&old)?;
            }
            std::fs::rename(&current, &old)?;
            Ok(true)
        }
        Ok(_) => Ok(false),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e),
    }
}

#[derive(Clone)]
struct SharedFile(Arc<Mutex<File>>);

impl Write for SharedFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().map_err(|_| io::Error::other("log file lock poisoned"))?.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.lock().map_err(|_| io::Error::other("log file lock poisoned"))?.flush()
    }
}

impl<'a> MakeWriter<'a> for SharedFile {
    type Writer = SharedFile;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Install the global subscriber. `RUST_LOG` overrides the default filter.
pub fn init(logs_dir: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(logs_dir)?;
    let _ = rotate_if_large(logs_dir, MAX_LOG_BYTES);
    let file = OpenOptions::new().create(true).append(true).open(logs_dir.join(LOG_FILE))?;
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,tk_ui=debug,tk_player=debug,tk_app=debug"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_ansi(false)
        .with_target(true)
        .with_writer(SharedFile(Arc::new(Mutex::new(file))))
        .try_init()
        .map_err(|e| anyhow::anyhow!("logger already installed: {e}"))?;
    std::panic::set_hook(Box::new(|info| {
        tracing::error!("panic: {info}");
    }));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotates_only_when_over_the_limit() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!rotate_if_large(tmp.path(), 10).unwrap());
        std::fs::write(tmp.path().join(LOG_FILE), "0123456789ab").unwrap();
        assert!(!rotate_if_large(tmp.path(), 100).unwrap());
        assert!(rotate_if_large(tmp.path(), 10).unwrap());
        assert!(!tmp.path().join(LOG_FILE).exists());
        assert!(tmp.path().join(format!("{LOG_FILE}.old")).is_file());
        // A second rotation replaces the previous .old.
        std::fs::write(tmp.path().join(LOG_FILE), "x".repeat(20)).unwrap();
        assert!(rotate_if_large(tmp.path(), 10).unwrap());
        assert_eq!(std::fs::read_to_string(tmp.path().join(format!("{LOG_FILE}.old"))).unwrap().len(), 20);
    }
}
