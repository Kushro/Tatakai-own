//! `Backend` manual torrents, preload, OS magnet handler, session history and reannounce (wave A, worker **i**;
//! docs/redesign/15 section 4; plan 14 section 0.1 #11).
//!
//! * `torrent_preview`: magnet / info-hash / `.torrent` URL / local `.torrent` path -> `TorrentEngine::preview`
//!   (librqbit `list_only`: nothing is added or downloaded) -> files + the suggested `(file index, episode)` alignment
//!   (`suggest_alignment`, from `tk_core::release::parse_release` per video file) with its confidence. The
//!   `.torrent` bytes the preview resolved are kept for `PREVIEW_TTL`, so the add that follows does not wait for the
//!   metadata again.
//! * `torrent_add_manual`: adds the torrent kept (`persist: true`) with the aligned files selected (no alignment =
//!   every file) and labelled for `media_id`, and stores the alignment (`LocalStore::set_torrent_episode_map`), which
//!   `torrent_play` reads before `pick_episode_file`. Pairs with episode `0` select a file without an episode. On a
//!   torrent already in the session the alignment is merged into the stored one and its files are added to the
//!   selection; the single pair `(index, 0)` is "download only this file" (Files page): the selection becomes exactly
//!   that file.
//! * `torrent_preload_next`: with `TorrentSettingsVm::preload_next`, the next episode's file of a torrent of the
//!   title (same batch) is selected; without one, the best torrent source of the next episode is added (empty
//!   selection, not kept) and its episode file selected. A cached debrid row needs no preload.
//! * Magnet handler: `crate::magnet::{take, is_registered, set_registered}` (blocking calls off the runtime).
//! * Session history: rows written by `live/torrent.rs`; read / cleared here.
//! * `torrent_reannounce`: `TorrentEngine::reannounce` (re-asks every tracker, see `tk-torrent`'s `tracker_probe`).

use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tk_core::release::{CandidateFile, is_video_path, parse_release, pick_episode_file};
use tk_torrent::{AddOptions, AddSource, TorrentLabel, TorrentPreview};
use tk_ui::backend::{Backend, SourceKindVm, TorrentFileVm, TorrentPreviewVm, TorrentSessionVm, UiResult};
use tk_ui::model::{ErrorKind, UiError};

use super::torrent::{engine_err, file_vm};
use super::{torrent_play, torrent_settings};
use crate::live::LiveBackend;

/// How long the `.torrent` bytes of a preview are kept for the add that follows.
const PREVIEW_TTL: Duration = Duration::from_secs(5 * 60);
/// Largest local `.torrent` file read (real ones are a few hundred KiB).
const MAX_TORRENT_FILE: u64 = 16 * 1024 * 1024;

/// `(info-hash, when, source text, add source with the resolved bytes)` of recent previews.
static PREVIEWS: Mutex<Vec<(String, Instant, String, AddSource)>> = Mutex::new(Vec::new());

fn invalid(detail: impl Into<String>) -> UiError {
    UiError::new(ErrorKind::Other, detail.into())
}

/// What the user typed or dropped: a magnet, a 40-hex info-hash, an http(s) `.torrent` URL, or a local `.torrent`
/// path (quotes and `file://` stripped).
fn add_source_of(source: &str) -> UiResult<AddSource> {
    let text = source.trim().trim_matches(['"', '\'']).trim();
    if text.is_empty() {
        return Err(invalid("empty torrent source"));
    }
    if let Some(add) = AddSource::parse(text) {
        return Ok(add);
    }
    let path_text = text.strip_prefix("file://").unwrap_or(text);
    let path = Path::new(path_text);
    if !path_text.to_ascii_lowercase().ends_with(".torrent") {
        return Err(invalid(format!("not a magnet link, info-hash or .torrent file: {text}")));
    }
    let meta = std::fs::metadata(path).map_err(|e| invalid(format!("cannot read {}: {e}", path.display())))?;
    if meta.len() > MAX_TORRENT_FILE {
        return Err(invalid(format!("{} is too large for a .torrent file", path.display())));
    }
    let bytes = std::fs::read(path).map_err(|e| invalid(format!("cannot read {}: {e}", path.display())))?;
    Ok(AddSource::TorrentBytes(bytes))
}

/// The add source for `source`: the bytes a recent preview resolved when there is one.
fn cached_source(source: &str) -> Option<AddSource> {
    let mut list = PREVIEWS.lock().ok()?;
    list.retain(|(_, at, _, _)| at.elapsed() < PREVIEW_TTL);
    let key = source.trim();
    list.iter().rev().find(|(hash, _, text, _)| text == key || hash.eq_ignore_ascii_case(key)).map(|(_, _, _, add)| add.clone())
}

fn remember_preview(source: &str, preview: &TorrentPreview, fallback: AddSource) {
    let add = preview.torrent_bytes.clone().map(AddSource::TorrentBytes).unwrap_or(fallback);
    if let Ok(mut list) = PREVIEWS.lock() {
        list.retain(|(hash, at, _, _)| at.elapsed() < PREVIEW_TTL && hash != &preview.id);
        list.push((preview.id.clone(), Instant::now(), source.trim().to_owned(), add));
        if list.len() > 16 {
            list.remove(0);
        }
    }
}

/// `(file index, episode)` from the file names, and the share of video files that got a number. A lone video
/// without a number is episode 1 at half confidence; when two files claim one episode the larger one keeps it.
pub fn suggest_alignment(files: &[TorrentFileVm]) -> (Vec<(usize, u32)>, f32) {
    let videos: Vec<&TorrentFileVm> = files.iter().filter(|f| is_video_path(&f.path)).collect();
    if videos.is_empty() {
        return (Vec::new(), 0.0);
    }
    let mut picked: Vec<(usize, u32, u64)> = Vec::new();
    let mut numbered = 0usize;
    for f in &videos {
        let Some((first, _)) = parse_release(&f.name).episodes else { continue };
        if first == 0 {
            continue;
        }
        numbered += 1;
        match picked.iter_mut().find(|(_, e, _)| *e == first) {
            Some(slot) if slot.2 < f.size_bytes => *slot = (f.index, first, f.size_bytes),
            Some(_) => {}
            None => picked.push((f.index, first, f.size_bytes)),
        }
    }
    if picked.is_empty() && videos.len() == 1 {
        return (vec![(videos[0].index, 1)], 0.5);
    }
    let confidence = numbered as f32 / videos.len() as f32;
    let mut out: Vec<(usize, u32)> = picked.into_iter().map(|(i, e, _)| (i, e)).collect();
    out.sort_by_key(|(_, e)| *e);
    (out, confidence.clamp(0.0, 1.0))
}

/// `stored` with `update` applied: a pair replaces every stored pair of the same file or the same episode.
pub fn merge_alignment(stored: &[(usize, u32)], update: &[(usize, u32)]) -> Vec<(usize, u32)> {
    let mut out: Vec<(usize, u32)> = stored
        .iter()
        .copied()
        .filter(|(i, e)| !update.iter().any(|(ui, ue)| ui == i || (*ue > 0 && ue == e)))
        .collect();
    out.extend(update.iter().copied().filter(|(_, e)| *e > 0));
    out.sort_by_key(|(i, _)| *i);
    out.dedup_by_key(|(i, _)| *i);
    out
}

/// `Backend::torrent_preview`.
pub async fn torrent_preview(b: &LiveBackend, source: &str) -> UiResult<TorrentPreviewVm> {
    let add = add_source_of(source)?;
    let engine = b.torrent.engine().await?;
    let preview = engine.preview(add.clone()).await.map_err(engine_err)?;
    remember_preview(source, &preview, add);
    let files: Vec<TorrentFileVm> = preview.files.iter().cloned().map(file_vm).collect();
    let (suggested, confidence) = suggest_alignment(&files);
    Ok(TorrentPreviewVm {
        name: preview.name,
        info_hash: preview.id,
        size_bytes: preview.size_bytes,
        files,
        suggested,
        confidence,
    })
}

/// `Backend::torrent_add_manual` (returns the torrent id).
pub async fn torrent_add_manual(b: &LiveBackend, source: &str, media_id: Option<&str>, alignment: &[(usize, u32)]) -> UiResult<String> {
    let add = match cached_source(source) {
        Some(add) => add,
        None => add_source_of(source)?,
    };
    let media_id = media_id.map(str::trim).filter(|m| !m.is_empty());
    let first_episode = alignment.iter().map(|(_, e)| *e).filter(|e| *e > 0).min();
    let label = match media_id {
        Some(id) => {
            let (title, poster) = match b.media(id).await {
                Ok((_, media)) => (media.title.clone(), media.cover.clone()),
                Err(e) => {
                    tracing::warn!(media = id, detail = %e.detail, "manual torrent: title metadata unavailable");
                    (String::new(), None)
                }
            };
            Some(TorrentLabel { media_id: id.to_owned(), title, episode: first_episode, poster })
        }
        None => None,
    };
    let engine = b.torrent.engine().await?;
    let known_hash = match &add {
        AddSource::Magnet(m) => tk_core::debrid::magnet_info_hash(m),
        AddSource::InfoHash(h) => Some(h.to_ascii_lowercase()),
        _ => None,
    };
    let existing = known_hash.as_ref().filter(|h| engine.list().iter().any(|e| &e.id == *h)).cloned();
    let indices: Vec<usize> = alignment.iter().map(|(i, _)| *i).collect();

    let tid = match existing {
        Some(tid) => {
            if let Some(label) = label.clone() {
                engine.set_label(&tid, label).map_err(engine_err)?;
            }
            if let [(index, 0)] = alignment {
                engine.set_files(&tid, &[*index]).await.map_err(engine_err)?;
            } else {
                for index in &indices {
                    engine.select_file(&tid, *index).await.map_err(engine_err)?;
                }
            }
            engine.resume(&tid).await.map_err(engine_err)?;
            tid
        }
        None => {
            let only_files = (!indices.is_empty()).then(|| indices.clone());
            let opts = AddOptions { only_files, persist: true, label: label.clone(), ..AddOptions::default() };
            engine.add(add, opts).await.map_err(engine_err)?
        }
    };
    let stored = b.store.torrent_episode_map(&tid).unwrap_or_default();
    let merged = merge_alignment(&stored, alignment);
    b.store.set_torrent_episode_map(&tid, media_id, &merged).map_err(LiveBackend::store_err)?;
    tracing::info!(torrent = %tid, files = indices.len(), media = ?media_id, "torrent added by hand");
    Ok(tid)
}

/// `Backend::torrent_preload_next`.
pub async fn torrent_preload_next(b: &LiveBackend, id: &str, episode: u32) -> UiResult<()> {
    if !torrent_settings::load(&b.paths()).preload_next {
        return Ok(());
    }
    let next = episode.saturating_add(1);
    let (_, media) = b.media(id).await?;
    if media.episodes.is_some_and(|total| next > total) {
        return Ok(());
    }
    let names = torrent_play::titles(&media);
    let engine = b.torrent.engine().await?;
    if let Some((tid, index)) = torrent_play::batch_file_for(b, &engine, id, next, &names) {
        engine.select_file(&tid, index).await.map_err(engine_err)?;
        tracing::info!(torrent = %tid, file = index, episode = next, "preloading the next episode (same torrent)");
        return Ok(());
    }
    let sources = b.sources(id, next).await?;
    let Some(best) = sources
        .iter()
        .find(|s| s.kind == SourceKindVm::Torrent && s.ext_enabled && !s.debrid.is_some_and(|d| d.cached))
    else {
        return Ok(());
    };
    let (add, label) = torrent_play::resolve_source(b, &best.id, id, Some(next)).await?;
    let tid = engine
        .add(add, AddOptions { only_files: Some(Vec::new()), persist: false, label: Some(label), ..AddOptions::default() })
        .await
        .map_err(engine_err)?;
    let files: Vec<CandidateFile> = engine
        .files(&tid)
        .map_err(engine_err)?
        .into_iter()
        .map(|f| CandidateFile { index: f.index, path: f.path, size_bytes: f.size_bytes })
        .collect();
    let Some(index) = pick_episode_file(&files, next, &names) else {
        return Err(UiError::new(ErrorKind::NoSource, tid));
    };
    engine.select_file(&tid, index).await.map_err(engine_err)?;
    tracing::info!(torrent = %tid, file = index, episode = next, "preloading the next episode");
    Ok(())
}

/// `Backend::take_pending_magnet` (`crate::magnet::take`).
pub async fn take_pending_magnet(_b: &LiveBackend) -> UiResult<Option<String>> {
    Ok(crate::magnet::take())
}

/// `Backend::magnet_handler_registered` (`crate::magnet::is_registered`).
pub async fn magnet_handler_registered(_b: &LiveBackend) -> UiResult<bool> {
    tokio::task::spawn_blocking(crate::magnet::is_registered)
        .await
        .map_err(|e| UiError::new(ErrorKind::Other, format!("magnet handler check: {e}")))
}

/// `Backend::set_magnet_handler` (`crate::magnet::set_registered`; io errors become `ErrorKind::Other` with the message).
pub async fn set_magnet_handler(_b: &LiveBackend, enabled: bool) -> UiResult<()> {
    tokio::task::spawn_blocking(move || crate::magnet::set_registered(enabled))
        .await
        .map_err(|e| UiError::new(ErrorKind::Other, format!("magnet handler: {e}")))?
        .map_err(|e| UiError::new(ErrorKind::Other, e.to_string()))
}

/// `Backend::torrent_session_history` (`b.store.torrent_sessions(limit)` mapped 1:1 to `TorrentSessionVm`).
pub async fn torrent_session_history(b: &LiveBackend, limit: u32) -> UiResult<Vec<TorrentSessionVm>> {
    let rows = b.store.torrent_sessions(limit.clamp(1, 1000)).map_err(LiveBackend::store_err)?;
    Ok(rows
        .into_iter()
        .map(|r| TorrentSessionVm {
            id: r.id,
            torrent_id: r.torrent_id,
            info_hash: r.info_hash,
            name: r.name,
            media_id: r.media_id,
            series: r.series,
            episode: r.episode,
            started_unix: r.started_at,
            ended_unix: r.ended_at,
            downloaded_bytes: r.downloaded_bytes,
            uploaded_bytes: r.uploaded_bytes,
            completed: r.completed,
        })
        .collect())
}

/// `Backend::torrent_clear_session_history` (`b.store.clear_torrent_sessions()`).
pub async fn torrent_clear_session_history(b: &LiveBackend) -> UiResult<u32> {
    let n = b.store.clear_torrent_sessions().map_err(LiveBackend::store_err)?;
    Ok(u32::try_from(n).unwrap_or(u32::MAX))
}

/// `Backend::torrent_reannounce`.
pub async fn torrent_reannounce(b: &LiveBackend, torrent_id: &str) -> UiResult<()> {
    b.torrent.reannounce(torrent_id).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(index: usize, path: &str, size: u64) -> TorrentFileVm {
        TorrentFileVm {
            index,
            path: path.to_owned(),
            name: path.rsplit('/').next().unwrap_or(path).to_owned(),
            size_bytes: size,
            downloaded_bytes: 0,
            progress: 0.0,
            selected: true,
            streams: 0,
            episode: None,
        }
    }

    #[test]
    fn batch_names_suggest_an_alignment() {
        let files = vec![
            file(0, "Show/[Group] Show - 01 [1080p].mkv", 900),
            file(1, "Show/[Group] Show - 02 [1080p].mkv", 900),
            file(2, "Show/[Group] Show - 02v2 [1080p].mkv", 950),
            file(3, "Show/Extras/NCOP.mkv", 50),
            file(4, "Show/readme.txt", 1),
        ];
        let (map, confidence) = suggest_alignment(&files);
        assert_eq!(map, [(0, 1), (2, 2)]);
        // 4 video files, 3 got a number (NCOP did not)
        assert!((confidence - 0.75).abs() < 1e-6, "{confidence}");
    }

    #[test]
    fn lone_videos_and_empty_torrents() {
        let (map, confidence) = suggest_alignment(&[file(7, "Movie Night.mkv", 5)]);
        assert_eq!((map, confidence), (vec![(7, 1)], 0.5));
        assert_eq!(suggest_alignment(&[file(0, "notes.txt", 1)]), (Vec::new(), 0.0));
    }

    #[test]
    fn alignment_merges_by_file_and_episode() {
        let stored = vec![(0, 1), (1, 2), (2, 3)];
        // file 1 is now episode 3: the old pairs of file 1 and of episode 3 go
        assert_eq!(merge_alignment(&stored, &[(1, 3)]), [(0, 1), (1, 3)]);
        // episode 0 only selects: it drops the file's old episode and stores nothing
        assert_eq!(merge_alignment(&stored, &[(2, 0)]), [(0, 1), (1, 2)]);
        assert_eq!(merge_alignment(&[], &[(4, 1), (5, 2)]), [(4, 1), (5, 2)]);
    }

    #[test]
    fn sources_typed_by_hand() {
        let hash = "0123456789abcdef0123456789abcdef01234567";
        assert!(matches!(add_source_of(&format!(" \"magnet:?xt=urn:btih:{hash}\" ")), Ok(AddSource::Magnet(_))));
        assert!(matches!(add_source_of(hash), Ok(AddSource::InfoHash(_))));
        assert!(matches!(add_source_of("https://nyaa.si/download/1.torrent"), Ok(AddSource::Url(_))));
        assert!(add_source_of("").is_err());
        assert!(add_source_of("just words").is_err());
        assert!(add_source_of("/no/such/file.torrent").is_err());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x.torrent");
        std::fs::write(&path, b"d4:infod4:name1:xee").unwrap();
        let typed = format!("file://{}", path.display());
        assert!(matches!(add_source_of(&typed), Ok(AddSource::TorrentBytes(b)) if b.starts_with(b"d4:")));
    }
}
