//! Download engine (wave B, worker **h**; docs/redesign/16-parity-wave-b-contract.md section 3.5; checklist
//! `downloads.engine.hls`, `downloads.engine.mp4`, `downloads.resume`, `downloads.queue`,
//! `downloads.integrity-check`; classic `desktop/ipc/ipc-download-manager.cjs`, `core/download/*`). UI-free: tk-app's
//! `live/downloads.rs` drives it and maps rows to `DownloadVm`. Rows persist in `LocalStore` (`store/downloads.rs`).
//!
//! * [`hls`]: playlist walk with `m3u8-rs` (variant by quality, segment plan, AES-128 keys).
//! * [`files`]: on-disk layout, safe names, integrity check (`Damage`), concatenation.
//! * [`library`]: local-library scan (video files under the library folders) and release-name guesses.
//! * [`watch`]: `notify` watcher of the library folders (new files are imported without a manual scan).
//! * `engine.rs`: [`Engine`], the queue: `Engine::new(client, store, dir, concurrent)`, `start()`, `enqueue(Job)` /
//!   `enqueue_with(Job, Meta)`, `pause(id)`, `resume(id)`, `cancel(id, delete_file)`, `remove(id, delete_file)`,
//!   `verify(id)`, `verify_completed()`, `progress() -> Vec<Progress>`, `set_limits(speed_kib, concurrent)`,
//!   `set_dir(dir)`, `shutdown()`.
//!
//! ## `Job` for each kind (the contract with workers l and m)
//!
//! | `kind` | `url` | `headers` | `target` |
//! |---|---|---|---|
//! | `hls` | master or media playlist URL | sent with every playlist / key / segment request | episode file (`.ts`; `.mp4` for fMP4 or after an ffmpeg remux) |
//! | `mp4` | direct file URL (a playlist found there switches the row to `hls`) | sent with the request | episode file |
//! | `manga` | **one page URL per line**, in reading order | sent with every page request | chapter **folder** `<dir>/<media>/<ext_id>/<chapter_id>` ([`files::chapter_target`]); pages land as `001.jpg`... |
//! | `novel` | an http(s) URL fetched as is, **or** the chapter text itself (a JSON list of paragraphs, or plain text split on blank lines) | sent with the request | `<dir>/<media>/<ext_id>/<chapter_id>.json` (the JSON list of paragraphs) |
//!
//! Finished `manga` / `novel` rows are recorded in worker l's `offline_chapters` (`ext_id` / `chapter_id` are read
//! back from the last two components of `target`, so build it with [`files::chapter_target`]). tk-app exposes
//! `live::downloads::enqueue_jobs(b, Vec<Job>)` for l / m (it also fills the row's title from the media id).

mod engine;
pub mod files;
pub mod hls;
pub mod library;
pub mod watch;

pub use engine::{Engine, Meta, Progress, content_range_total, eta, novel_json};

/// `Job::kind` values (they mirror `DownloadKindVm::id()` of tk-ui).
pub const KIND_HLS: &str = "hls";
pub const KIND_MP4: &str = "mp4";
pub const KIND_MANGA: &str = "manga";
pub const KIND_NOVEL: &str = "novel";

/// What one queue row downloads (the `kind` strings mirror `DownloadKindVm::id()`; see the table above).
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
