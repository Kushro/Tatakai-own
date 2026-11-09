//! Download engine (wave B, worker **h**; docs/redesign/16-parity-wave-b-contract.md section 3.5; checklist
//! `downloads.engine.hls`, `downloads.engine.mp4`, `downloads.resume`, `downloads.queue`; classic
//! `desktop/ipc/ipc-download-manager.cjs`, `core/download/*`). UI-free: tk-app's `live/downloads.rs` drives it and
//! maps rows to `DownloadVm`. h owns this directory and adds private files (`hls.rs`: playlist walk with `m3u8-rs`,
//! segment fetch with the stream headers, concat into one `.ts` -> remux is out of scope (mpv plays `.ts`); `mp4.rs`:
//! `Range` resume; `queue.rs`: `Settings::downloads.concurrent` workers, pause / resume / cancel; `library.rs`: scan
//! of the local-library dirs with `notify`, release-name matching through `crate::release`). Rows persist in
//! `LocalStore` (`store/downloads.rs`).
//!
//! Final public shape (h implements):
//! `pub struct Engine` with `Engine::new(client: reqwest::Client, store: Arc<LocalStore>, dir: PathBuf, concurrent: u8)`,
//! `enqueue(Job) -> Result<i64, DownloadError>`, `pause(id)`, `resume(id)`, `cancel(id, delete_file)`, `verify(id)`,
//! `progress() -> Vec<Progress>` (snapshot for the UI), `set_limits(speed_kib, concurrent)`, `shutdown()`.

pub mod hls;

/// What one queue row downloads (h extends; the `kind` strings mirror `DownloadKindVm::id()`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub media_id: String,
    pub episode: Option<u32>,
    /// `hls` | `mp4` | `manga` | `novel`.
    pub kind: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// Target file (episodes) or directory (chapters), absolute.
    pub target: std::path::PathBuf,
}
