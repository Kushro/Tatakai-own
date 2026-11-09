//! Fonts for subtitles (wave A, worker **d**): the families installed on the machine (`sub-font` picker) and the
//! app's own fonts folder `<data>/fonts` that libass also searches (`sub-fonts-dir`), where the user can drop TTF /
//! OTF files used by ASS tracks. Family names come from `fc-list` on Linux / BSD and otherwise from the `name`
//! table of the font files themselves (TTF, OTF, TTC), read without any font crate.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Font file extensions libass / fontconfig load.
const FONT_EXTS: [&str; 4] = ["ttf", "otf", "ttc", "otc"];
/// Directory walk limits (a fonts folder can be big; the picker does not need every file).
const MAX_FILES: usize = 6000;
const MAX_DEPTH: usize = 5;

/// `<data dir>/fonts`, or a folder under the temp dir when the data directory is unknown.
pub(crate) fn app_fonts_dir(data_dir: &str) -> PathBuf {
    if data_dir.trim().is_empty() { std::env::temp_dir().join("tatakai-next-fonts") } else { Path::new(data_dir).join("fonts") }
}

/// Sorted, de-duplicated family names installed on the machine plus the ones in `extra` (the app fonts folder).
pub(crate) fn installed_families(extra: &Path) -> Vec<String> {
    let mut set: BTreeSet<String> = BTreeSet::new();
    let from_fc = fc_list();
    let fc_ok = !from_fc.is_empty();
    set.extend(from_fc);
    if !fc_ok {
        for dir in system_font_dirs() {
            set.extend(families_in(&dir));
        }
    }
    set.extend(families_in(extra));
    set.into_iter().filter(|f| !f.trim().is_empty()).collect()
}

/// Families of the font files under `dir` (recursive, bounded).
pub(crate) fn families_in(dir: &Path) -> Vec<String> {
    let mut files = Vec::new();
    walk(dir, 0, &mut files);
    let mut out: BTreeSet<String> = BTreeSet::new();
    for f in files {
        if let Ok(mut file) = File::open(&f)
            && let Some(name) = family_from_reader(&mut file)
        {
            out.insert(name);
        }
    }
    out.into_iter().collect()
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > MAX_DEPTH || out.len() >= MAX_FILES {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, depth + 1, out);
        } else if p.extension().and_then(|x| x.to_str()).is_some_and(|x| FONT_EXTS.contains(&x.to_ascii_lowercase().as_str())) {
            out.push(p);
        }
        if out.len() >= MAX_FILES {
            return;
        }
    }
}

/// `fc-list : family` (fontconfig): one line per face, localised names separated by commas, `-` escaped as `\-`.
fn fc_list() -> Vec<String> {
    if cfg!(any(windows, target_os = "macos")) {
        return Vec::new();
    }
    let Ok(out) = std::process::Command::new("fc-list").args([":", "family"]).output() else { return Vec::new() };
    if !out.status.success() {
        return Vec::new();
    }
    parse_fc_list(&String::from_utf8_lossy(&out.stdout))
}

pub(crate) fn parse_fc_list(text: &str) -> Vec<String> {
    let mut set: BTreeSet<String> = BTreeSet::new();
    for line in text.lines() {
        if let Some(first) = line.split(',').next() {
            let name = first.replace("\\-", "-").replace("\\\\", "\\").trim().to_owned();
            if !name.is_empty() {
                set.insert(name);
            }
        }
    }
    set.into_iter().collect()
}

fn system_font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
    if cfg!(windows) {
        let windir = std::env::var_os("WINDIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\Windows"));
        dirs.push(windir.join("Fonts"));
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local).join("Microsoft").join("Windows").join("Fonts"));
        }
    } else if cfg!(target_os = "macos") {
        dirs.push(PathBuf::from("/System/Library/Fonts"));
        dirs.push(PathBuf::from("/Library/Fonts"));
        if let Some(h) = &home {
            dirs.push(h.join("Library").join("Fonts"));
        }
    } else {
        dirs.push(PathBuf::from("/usr/share/fonts"));
        dirs.push(PathBuf::from("/usr/local/share/fonts"));
        if let Some(h) = &home {
            dirs.push(h.join(".local").join("share").join("fonts"));
            dirs.push(h.join(".fonts"));
        }
    }
    dirs
}

fn read_exact_at<R: Read + Seek>(r: &mut R, offset: u64, len: usize) -> Option<Vec<u8>> {
    r.seek(SeekFrom::Start(offset)).ok()?;
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).ok()?;
    Some(buf)
}

fn be16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*b.get(at)?, *b.get(at + 1)?]))
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes([*b.get(at)?, *b.get(at + 1)?, *b.get(at + 2)?, *b.get(at + 3)?]))
}

/// Family name of a TrueType / OpenType font (first face of a collection): `name` table, typographic family (ID
/// 16) before family (ID 1), Windows English names before any other.
pub(crate) fn family_from_reader<R: Read + Seek>(r: &mut R) -> Option<String> {
    let head = read_exact_at(r, 0, 12)?;
    let font_at = if &head[0..4] == b"ttcf" { u64::from(be32(&head, 12).or_else(|| read_exact_at(r, 12, 4).and_then(|b| be32(&b, 0)))?) } else { 0 };
    let offset_table = read_exact_at(r, font_at, 12)?;
    let tables = usize::from(be16(&offset_table, 4)?).min(512);
    let dir = read_exact_at(r, font_at + 12, tables * 16)?;
    let (mut name_off, mut name_len) = (None, 0usize);
    for i in 0..tables {
        let rec = &dir[i * 16..i * 16 + 16];
        if &rec[0..4] == b"name" {
            name_off = Some(u64::from(be32(rec, 8)?));
            name_len = usize::try_from(be32(rec, 12)?).ok()?.min(1 << 20);
        }
    }
    let table = read_exact_at(r, name_off?, name_len)?;
    family_from_name_table(&table)
}

/// The `name` table body -> best family name.
pub(crate) fn family_from_name_table(t: &[u8]) -> Option<String> {
    let count = usize::from(be16(t, 2)?);
    let storage = usize::from(be16(t, 4)?);
    let mut best: Option<(u8, String)> = None;
    for i in 0..count {
        let at = 6 + i * 12;
        let (platform, encoding, language, name_id) = (be16(t, at)?, be16(t, at + 2)?, be16(t, at + 4)?, be16(t, at + 6)?);
        let (len, off) = (usize::from(be16(t, at + 8)?), usize::from(be16(t, at + 10)?));
        if name_id != 1 && name_id != 16 {
            continue;
        }
        let Some(raw) = t.get(storage + off..storage + off + len) else { continue };
        let text = match (platform, encoding) {
            // Unicode / Windows: UTF-16BE.
            (0, _) | (3, 1) | (3, 10) => {
                let units: Vec<u16> = raw.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
                String::from_utf16_lossy(&units)
            }
            // Macintosh Roman: ASCII is all that matters for a family name.
            (1, 0) => raw.iter().map(|&b| char::from(b)).collect(),
            _ => continue,
        };
        let text = text.trim().to_owned();
        if text.is_empty() {
            continue;
        }
        // Lower rank wins: typographic family first, Windows English first.
        let english = (platform == 3 && language == 0x409) || (platform == 1 && language == 0);
        let rank = u8::from(name_id != 16) * 2 + u8::from(!english);
        if best.as_ref().is_none_or(|(r, _)| rank < *r) {
            best = Some((rank, text));
        }
    }
    best.map(|(_, name)| name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// A minimal font: offset table + one `name` table with a Windows family (ID 1) record.
    fn tiny_font(family: &str) -> Vec<u8> {
        let utf16: Vec<u8> = family.encode_utf16().flat_map(u16::to_be_bytes).collect();
        let mut name = Vec::new();
        name.extend(0u16.to_be_bytes()); // format
        name.extend(1u16.to_be_bytes()); // count
        name.extend(18u16.to_be_bytes()); // storage offset = 6 + 12
        for v in [3u16, 1, 0x409, 1, u16::try_from(utf16.len()).unwrap(), 0] {
            name.extend(v.to_be_bytes());
        }
        name.extend(&utf16);
        let mut font = Vec::new();
        font.extend(0x0001_0000u32.to_be_bytes());
        font.extend(1u16.to_be_bytes()); // numTables
        font.extend([0u8; 6]);
        font.extend(b"name");
        font.extend(0u32.to_be_bytes()); // checksum
        font.extend(28u32.to_be_bytes()); // offset = 12 + 16
        font.extend(u32::try_from(name.len()).unwrap().to_be_bytes());
        font.extend(name);
        font
    }

    #[test]
    fn family_is_read_from_the_name_table() {
        assert_eq!(family_from_reader(&mut Cursor::new(tiny_font("Noto Sans"))).as_deref(), Some("Noto Sans"));
        assert_eq!(family_from_reader(&mut Cursor::new(b"not a font at all".to_vec())), None);
    }

    #[test]
    fn collections_use_their_first_face() {
        let face = tiny_font("Source Han Sans");
        let mut ttc = Vec::new();
        ttc.extend(b"ttcf");
        ttc.extend(0x0001_0000u32.to_be_bytes());
        ttc.extend(1u32.to_be_bytes());
        ttc.extend(16u32.to_be_bytes()); // first face right after this header
        // The face's own offsets are absolute in a collection: shift its name table by 16.
        let mut shifted = face.clone();
        shifted[20..24].copy_from_slice(&(28u32 + 16).to_be_bytes());
        ttc.extend(shifted);
        assert_eq!(family_from_reader(&mut Cursor::new(ttc)).as_deref(), Some("Source Han Sans"));
    }

    #[test]
    fn fc_list_lines_and_folder_scan() {
        let fams = parse_fc_list("DejaVu Sans,DejaVu Sans Condensed\nNoto Sans CJK JP\nDejaVu Sans\nM\\-PLUS 1p\n");
        assert_eq!(fams, vec!["DejaVu Sans".to_owned(), "M-PLUS 1p".to_owned(), "Noto Sans CJK JP".to_owned()]);
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub").join("A.TTF"), tiny_font("Alpha")).unwrap();
        std::fs::write(dir.path().join("readme.txt"), b"x").unwrap();
        assert_eq!(families_in(dir.path()), vec!["Alpha".to_owned()]);
        assert!(app_fonts_dir("/data").ends_with("fonts"));
    }
}
