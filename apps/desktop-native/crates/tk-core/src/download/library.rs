//! Local library helpers (worker **h**; checklist `downloads.local-library.{import,scan,match}`; classic
//! `desktop/ipc/ipc-library.cjs`, `ImportAnimeModal.tsx`): walk the library folders for video files and guess the
//! series + episode of a file from its release name (`crate::release::parse_release`, the torrent parser). The title
//! search that turns a guess into a `MediaRef` lives in tk-app (`live/downloads.rs`), over the metadata router.

use std::path::{Path, PathBuf};

use crate::release::{is_video_path, parse_release};

/// Directories never descended into (engine leftovers, OS / tool folders).
const SKIP_DIRS: &[&str] = &["$recycle.bin", "system volume information", "node_modules", "@eadir", ".trash"];
/// Deepest folder level the scan reaches below a library root.
pub const MAX_DEPTH: usize = 6;
/// Upper bound of files one scan returns (a whole drive picked by mistake must not hang the app).
pub const MAX_FILES: usize = 20_000;

fn skip_dir(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with('.') || lower.ends_with(".parts") || SKIP_DIRS.contains(&lower.as_str())
}

/// Whether a path is a finished video file the library can hold (not an engine `.part` / `.tmp` / sample).
pub fn is_library_video(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else { return false };
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".part") || lower.ends_with(".tmp") || lower.starts_with('.') {
        return false;
    }
    let stem = lower.rsplit_once('.').map_or(lower.as_str(), |(s, _)| s);
    if stem.ends_with("sample") || stem.starts_with("sample") {
        return false;
    }
    is_video_path(&lower)
}

/// Video files under `root` (blocking; run it in `spawn_blocking`), sorted by path. A file `root` is returned as
/// is when it is a video.
pub fn walk_videos(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if root.is_file() {
        if is_library_video(root) {
            out.push(root.to_path_buf());
        }
        return out;
    }
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for entry in rd.flatten() {
            let path = entry.path();
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_dir() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if depth < MAX_DEPTH && !skip_dir(&name) {
                    stack.push((path, depth + 1));
                }
            } else if ft.is_file() && is_library_video(&path) {
                out.push(path);
                if out.len() >= MAX_FILES {
                    out.sort();
                    return out;
                }
            }
        }
    }
    out.sort();
    out
}

/// What a file name says about its content.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Guess {
    /// Series title as written (group, episode and tags removed); `None` when nothing is left.
    pub title: Option<String>,
    pub episode: Option<u32>,
    pub season: Option<u32>,
}

/// Last run of digits in `s` that looks like an episode number (1..=3 digits, not a resolution / year).
fn trailing_number(s: &str) -> Option<u32> {
    let mut best = None;
    let mut cur = String::new();
    let flush = |cur: &mut String, best: &mut Option<u32>| {
        if !cur.is_empty()
            && cur.len() <= 4
            && let Ok(n) = cur.parse::<u32>()
            && n > 0
            && n < 2000
            && !matches!(n, 480 | 576 | 720 | 1080)
        {
            *best = Some(n);
        }
        cur.clear();
    };
    for c in s.chars() {
        if c.is_ascii_digit() {
            cur.push(c);
        } else {
            flush(&mut cur, &mut best);
        }
    }
    flush(&mut cur, &mut best);
    best
}

/// Guess series and episode from a file name (`[Group] Show - 01 (1080p).mkv`, `Show S02E05.mp4`, `Show 12.mkv`).
/// When the name has no title (`01.mkv`), the parent folder name is used as the title.
pub fn guess(path: &Path) -> Guess {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let tags = parse_release(name);
    let mut title = tags.title.clone().map(|t| t.trim().to_owned()).filter(|t| !t.is_empty());
    let mut episode = tags.episodes.and_then(|(a, b)| (a == b).then_some(a));
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    if episode.is_none() && tags.episodes.is_none() {
        episode = trailing_number(stem);
    }
    if title.as_deref().is_some_and(|t| t.chars().all(|c| c.is_ascii_digit() || c.is_whitespace() || c == '-')) {
        title = None;
    }
    if title.is_none() {
        title = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .map(|n| parse_release(n).title.unwrap_or_else(|| n.to_owned()))
            .map(|t| t.trim().to_owned())
            .filter(|t| !t.is_empty());
    }
    Guess { title, episode, season: tags.season }
}

/// Normalized form of a title for comparisons (lowercase alphanumerics separated by one space).
pub fn normalize_title(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
            space = false;
        } else if !space && !out.is_empty() {
            out.push(' ');
            space = true;
        }
    }
    out.trim_end().to_owned()
}

/// How well a catalogue title matches a guessed one, 0..=100 (exact = 100, prefix / containment = 70, shared
/// words scaled below that). Used to pick the first search result only when it is plausible.
pub fn title_score(guess: &str, candidate: &str) -> u32 {
    let (g, c) = (normalize_title(guess), normalize_title(candidate));
    if g.is_empty() || c.is_empty() {
        return 0;
    }
    if g == c {
        return 100;
    }
    if c.starts_with(&g) || g.starts_with(&c) || c.contains(&g) {
        return 70;
    }
    let gw: Vec<&str> = g.split(' ').collect();
    let cw: Vec<&str> = c.split(' ').collect();
    let shared = gw.iter().filter(|w| cw.contains(w)).count();
    (shared * 60 / gw.len().max(cw.len())) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_filter() {
        assert!(is_library_video(Path::new("/a/Show - 01.mkv")));
        assert!(!is_library_video(Path::new("/a/Show - 01.mkv.part")));
        assert!(!is_library_video(Path::new("/a/sample.mkv")));
        assert!(!is_library_video(Path::new("/a/notes.txt")));
        assert!(!is_library_video(Path::new("/a/.hidden.mp4")));
    }

    #[test]
    fn guesses() {
        let g = guess(Path::new("/v/[SubsPlease] Sousou no Frieren - 12 (1080p) [A1B2C3D4].mkv"));
        assert_eq!(g.title.as_deref(), Some("Sousou no Frieren"));
        assert_eq!(g.episode, Some(12));
        let g = guess(Path::new("/v/Dandadan/05.mp4"));
        assert_eq!(g.title.as_deref(), Some("Dandadan"));
        assert_eq!(g.episode, Some(5));
        assert_eq!(trailing_number("Show 1080 12"), Some(12));
        assert_eq!(trailing_number("Show"), None);
    }

    #[test]
    fn walks_folders() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Show/.hidden")).unwrap();
        std::fs::create_dir_all(dir.path().join("Show/x.ts.parts")).unwrap();
        std::fs::write(dir.path().join("Show/Show - 01.mkv"), b"1").unwrap();
        std::fs::write(dir.path().join("Show/Show - 02.mkv.part"), b"1").unwrap();
        std::fs::write(dir.path().join("Show/.hidden/a.mkv"), b"1").unwrap();
        std::fs::write(dir.path().join("Show/x.ts.parts/00000.seg"), b"1").unwrap();
        let found = walk_videos(dir.path());
        assert_eq!(found.len(), 1);
        assert!(found[0].ends_with("Show - 01.mkv"));
    }

    #[test]
    fn title_scores() {
        assert_eq!(title_score("Sousou no Frieren", "Sousou no Frieren"), 100);
        assert_eq!(title_score("frieren", "Sousou no Frieren"), 70);
        assert!(title_score("Frieren Beyond", "Frieren: Beyond Journey's End") >= 60);
        assert_eq!(title_score("", "x"), 0);
        assert_eq!(normalize_title("Re:Zero  -  Season 2"), "re zero season 2");
    }
}
