//! File naming, layout and integrity checks of the download engine (worker **h**; pure or blocking std I/O, tested).
//!
//! Layout under the download folder (`Settings::downloads.dir`, default `<Downloads>/Tatakai/Episodes`):
//! * episodes: `<dir>/<Title>/<Title> - 01.ts` (HLS; `.mp4` for fMP4 playlists or after an ffmpeg remux) or `.mp4`
//!   (direct files). While running: `<target>.part` (direct file) / `<target>.parts/NNNNN.seg` (HLS segments, the
//!   resume checkpoint: a segment file exists only once complete).
//! * manga chapters: `<dir>/<media>/<ext>/<chapter>/001.jpg ...` (contract section 4 h), novel chapters:
//!   `<dir>/<media>/<ext>/<chapter>.json`.

use std::path::{Path, PathBuf};

/// Characters Windows / macOS / Linux refuse (or that confuse shells) in a file name.
const FORBIDDEN: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
/// Longest file-name component we write (well under every file system limit).
const MAX_NAME: usize = 120;

/// A safe single path component: forbidden and control characters become `_`, runs of spaces collapse, leading /
/// trailing dots and spaces go, reserved Windows device names get a `_` suffix; empty -> `untitled`.
pub fn sanitize_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_space = false;
    for c in name.chars() {
        let c = if FORBIDDEN.contains(&c) || c.is_control() { '_' } else { c };
        if c.is_whitespace() {
            if !last_space {
                out.push(' ');
            }
            last_space = true;
        } else {
            out.push(c);
            last_space = false;
        }
    }
    let trimmed = out.trim_matches(|c: char| c == '.' || c == ' ');
    let mut s: String = trimmed.chars().take(MAX_NAME).collect();
    s = s.trim_end_matches(|c: char| c == '.' || c == ' ').to_owned();
    if s.is_empty() {
        return "untitled".to_owned();
    }
    let upper = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((upper.starts_with("COM") || upper.starts_with("LPT")) && upper.len() == 4 && upper.as_bytes()[3].is_ascii_digit());
    if reserved {
        s.push('_');
    }
    s
}

/// `<Title> - 01.<ext>` (two digits at least, three for long shows).
pub fn episode_file_name(title: &str, episode: u32, ext: &str) -> String {
    format!("{} - {episode:02}.{ext}", sanitize_name(title))
}

/// Target of an episode download.
pub fn episode_target(dir: &Path, title: &str, episode: u32, ext: &str) -> PathBuf {
    dir.join(sanitize_name(title)).join(episode_file_name(title, episode, ext))
}

/// Target directory of a manga chapter (`<dir>/<media>/<ext>/<chapter>`), novel chapters add `.json`.
pub fn chapter_target(dir: &Path, media_id: &str, ext_id: &str, chapter_id: &str) -> PathBuf {
    dir.join(sanitize_name(media_id)).join(sanitize_name(ext_id)).join(sanitize_name(chapter_id))
}

/// `<target>.part` (direct file in progress).
pub fn part_file(target: &Path) -> PathBuf {
    let mut s = target.as_os_str().to_owned();
    s.push(".part");
    PathBuf::from(s)
}

/// `<target>.parts/` (HLS segments in progress).
pub fn parts_dir(target: &Path) -> PathBuf {
    let mut s = target.as_os_str().to_owned();
    s.push(".parts");
    PathBuf::from(s)
}

/// Segment file of index `i` inside the parts directory.
pub fn segment_file(parts: &Path, i: usize) -> PathBuf {
    parts.join(format!("{i:05}.seg"))
}

/// Init segment (`#EXT-X-MAP`) inside the parts directory.
pub fn init_file(parts: &Path) -> PathBuf {
    parts.join("init.seg")
}

/// Extension of a page image from its content type, else from the URL path; `jpg` by default.
pub fn image_ext(content_type: Option<&str>, url: &str) -> &'static str {
    let ct = content_type.unwrap_or("").to_ascii_lowercase();
    for (needle, ext) in [("png", "png"), ("webp", "webp"), ("gif", "gif"), ("avif", "avif"), ("jpeg", "jpg"), ("jpg", "jpg")] {
        if ct.contains(needle) {
            return ext;
        }
    }
    let path = url.split(['?', '#']).next().unwrap_or("").to_ascii_lowercase();
    for (suffix, ext) in [(".png", "png"), (".webp", "webp"), (".gif", "gif"), (".avif", "avif"), (".jpeg", "jpg"), (".jpg", "jpg")] {
        if path.ends_with(suffix) {
            return ext;
        }
    }
    "jpg"
}

/// The file of page `index` (0-based) already in `dir`, whatever its extension (`001.png`).
pub fn existing_page(dir: &Path, index: usize) -> Option<PathBuf> {
    let prefix = format!("{:03}.", index + 1);
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| {
        p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with(&prefix) && !n.ends_with(".tmp"))
    })
}

/// Size of a file or the sum of the files under a directory (0 when missing).
pub fn path_bytes(path: &Path) -> u64 {
    match std::fs::metadata(path) {
        Ok(m) if m.is_file() => m.len(),
        Ok(m) if m.is_dir() => std::fs::read_dir(path)
            .map(|rd| rd.flatten().map(|e| path_bytes(&e.path())).sum())
            .unwrap_or(0),
        _ => 0,
    }
}

/// Files directly inside a directory.
pub fn file_count(dir: &Path) -> usize {
    std::fs::read_dir(dir).map(|rd| rd.flatten().filter(|e| e.path().is_file()).count()).unwrap_or(0)
}

/// Why a finished download is not usable any more (`downloads.integrity-check`); `None` = fine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Damage {
    Missing,
    Empty,
    /// Shorter (or longer) than the size recorded when it completed.
    Truncated,
    /// An MPEG-TS file that does not start with the sync byte.
    Corrupt,
    /// A chapter folder with fewer pages than downloaded.
    PagesMissing,
}

impl Damage {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Empty => "empty",
            Self::Truncated => "truncated",
            Self::Corrupt => "corrupt",
            Self::PagesMissing => "pages missing",
        }
    }
}

/// Check a finished row on disk: `kind` = `hls | mp4 | novel` (a file) or `manga` (a folder with `pages` files).
pub fn check(kind: &str, path: &Path, expected_bytes: Option<u64>, pages: Option<u32>) -> Option<Damage> {
    if kind == "manga" {
        if !path.is_dir() {
            return Some(Damage::Missing);
        }
        let n = file_count(path);
        if n == 0 {
            return Some(Damage::Empty);
        }
        return match pages {
            Some(p) if n < p as usize => Some(Damage::PagesMissing),
            _ => None,
        };
    }
    let meta = match std::fs::metadata(path) {
        Ok(m) if m.is_file() => m,
        _ => return Some(Damage::Missing),
    };
    if meta.len() == 0 {
        return Some(Damage::Empty);
    }
    if let Some(expected) = expected_bytes.filter(|b| *b > 0)
        && meta.len() != expected
    {
        return Some(Damage::Truncated);
    }
    let is_ts = path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("ts"));
    if is_ts {
        use std::io::Read;
        let mut first = [0u8; 1];
        let ok = std::fs::File::open(path).and_then(|mut f| f.read_exact(&mut first)).is_ok();
        if !ok || first[0] != 0x47 {
            return Some(Damage::Corrupt);
        }
    }
    None
}

/// Concatenate the init segment (if any) and `count` segments of `parts` into `out` (blocking; run it in
/// `spawn_blocking`). Writes `<out>.tmp` first and renames, so a crash never leaves a half file under `out`.
pub fn concat_segments(parts: &Path, count: usize, out: &Path) -> std::io::Result<u64> {
    use std::io::Write;
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp = out.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let mut w = std::io::BufWriter::new(std::fs::File::create(&tmp)?);
    let init = init_file(parts);
    if init.is_file() {
        std::io::copy(&mut std::fs::File::open(&init)?, &mut w)?;
    }
    for i in 0..count {
        let seg = segment_file(parts, i);
        std::io::copy(&mut std::fs::File::open(&seg)?, &mut w)?;
    }
    w.flush()?;
    drop(w);
    std::fs::rename(&tmp, out)?;
    Ok(std::fs::metadata(out)?.len())
}

/// Remove a file or directory, ignoring "not found".
pub fn remove_path(path: &Path) {
    let r = if path.is_dir() { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) };
    if let Err(e) = r
        && e.kind() != std::io::ErrorKind::NotFound
    {
        tracing::warn!(path = %path.display(), error = %e, "could not remove a download file");
    }
}

/// Remove every in-progress leftover of `target` (`.part`, `.parts/`, `.tmp`).
pub fn remove_partials(target: &Path) {
    remove_path(&part_file(target));
    remove_path(&parts_dir(target));
    let mut tmp = target.as_os_str().to_owned();
    tmp.push(".tmp");
    remove_path(Path::new(&tmp));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_safe() {
        assert_eq!(sanitize_name("Re:Zero / Season 2?"), "Re_Zero _ Season 2_");
        assert_eq!(sanitize_name("  ..Hidden..  "), "Hidden");
        assert_eq!(sanitize_name("CON"), "CON_");
        assert_eq!(sanitize_name("com1.txt"), "com1.txt_");
        assert_eq!(sanitize_name(""), "untitled");
        assert_eq!(sanitize_name("a\tb\n  c"), "a_b_ c");
        assert!(sanitize_name(&"x".repeat(500)).len() <= MAX_NAME);
        assert_eq!(episode_file_name("Frieren", 3, "ts"), "Frieren - 03.ts");
        assert_eq!(episode_file_name("One Piece", 1100, "mp4"), "One Piece - 1100.mp4");
    }

    #[test]
    fn layout_paths() {
        let t = episode_target(Path::new("/d"), "Show", 1, "ts");
        assert_eq!(t, Path::new("/d/Show/Show - 01.ts"));
        assert_eq!(part_file(&t), Path::new("/d/Show/Show - 01.ts.part"));
        assert_eq!(parts_dir(&t), Path::new("/d/Show/Show - 01.ts.parts"));
        assert_eq!(segment_file(&parts_dir(&t), 7), Path::new("/d/Show/Show - 01.ts.parts/00007.seg"));
        assert_eq!(chapter_target(Path::new("/d"), "30013", "ext.a", "ch-1"), Path::new("/d/30013/ext.a/ch-1"));
    }

    #[test]
    fn image_extensions() {
        assert_eq!(image_ext(Some("image/webp"), "https://x/a"), "webp");
        assert_eq!(image_ext(None, "https://x/a.PNG?token=1"), "png");
        assert_eq!(image_ext(Some("application/octet-stream"), "https://x/a"), "jpg");
    }

    #[test]
    fn integrity_checks() {
        let dir = tempfile::tempdir().unwrap();
        let ts = dir.path().join("a.ts");
        assert_eq!(check("hls", &ts, None, None), Some(Damage::Missing));
        std::fs::write(&ts, b"").unwrap();
        assert_eq!(check("hls", &ts, None, None), Some(Damage::Empty));
        std::fs::write(&ts, b"\x47\x01\x02\x03").unwrap();
        assert_eq!(check("hls", &ts, Some(4), None), None);
        assert_eq!(check("hls", &ts, Some(10), None), Some(Damage::Truncated));
        std::fs::write(&ts, b"\x01\x02").unwrap();
        assert_eq!(check("hls", &ts, None, None), Some(Damage::Corrupt));
        let mp4 = dir.path().join("a.mp4");
        std::fs::write(&mp4, b"\x01\x02").unwrap();
        assert_eq!(check("mp4", &mp4, Some(2), None), None);
        let ch = dir.path().join("ch");
        assert_eq!(check("manga", &ch, None, Some(2)), Some(Damage::Missing));
        std::fs::create_dir(&ch).unwrap();
        std::fs::write(ch.join("001.jpg"), b"\x01").unwrap();
        assert_eq!(check("manga", &ch, None, Some(2)), Some(Damage::PagesMissing));
        std::fs::write(ch.join("002.jpg"), b"\x01").unwrap();
        assert_eq!(check("manga", &ch, None, Some(2)), None);
        assert!(existing_page(&ch, 1).is_some());
        assert!(existing_page(&ch, 2).is_none());
        assert_eq!(path_bytes(&ch), 2);
    }

    #[test]
    fn concat_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let parts = dir.path().join("x.ts.parts");
        std::fs::create_dir_all(&parts).unwrap();
        std::fs::write(init_file(&parts), b"I").unwrap();
        std::fs::write(segment_file(&parts, 0), b"A").unwrap();
        std::fs::write(segment_file(&parts, 1), b"BC").unwrap();
        let out = dir.path().join("out").join("x.ts");
        assert_eq!(concat_segments(&parts, 2, &out).unwrap(), 4);
        assert_eq!(std::fs::read(&out).unwrap(), b"IABC");
        remove_partials(&dir.path().join("x.ts"));
        assert!(!parts.exists());
    }
}
