//! `.kai` package reader (ZIP with `manifest.json` + `bundle.js`) with the install-time validation of globals.v1.md section 4.

use std::io::{Cursor, Read};
use std::path::Path;

use crate::error::InstallError;
use crate::manifest::Manifest;

pub const MAX_BUNDLE_BYTES: u64 = 10 * 1024 * 1024;
pub const MAX_ENTRIES: usize = 64;
pub const MAX_UNCOMPRESSED_BYTES: u64 = 32 * 1024 * 1024;
const MAX_MANIFEST_BYTES: u64 = 256 * 1024;

#[derive(Debug, Clone)]
pub struct Package {
    pub manifest: Manifest,
    pub manifest_json: String,
    pub bundle: String,
}

/// An entry path may not escape the archive: no `..`, no absolute path, no drive letter, no backslash tricks.
fn path_is_safe(name: &str) -> bool {
    if name.is_empty() || name.starts_with('/') || name.starts_with('\\') || name.contains(':') || name.contains('\0') {
        return false;
    }
    !name.split(['/', '\\']).any(|c| c == "..")
}

fn read_limited(entry: &mut impl Read, limit: u64, what: &str) -> Result<Vec<u8>, InstallError> {
    let mut buf = Vec::new();
    // `take(limit + 1)` defends against archives whose headers lie about the size
    entry.take(limit + 1).read_to_end(&mut buf)?;
    if buf.len() as u64 > limit {
        return Err(InstallError::invalid(format!("{what}: larger than {limit} bytes")));
    }
    Ok(buf)
}

pub fn read_kai(path: &Path) -> Result<Package, InstallError> {
    let bytes = std::fs::read(path)?;
    read_kai_bytes(&bytes)
}

pub fn read_kai_bytes(bytes: &[u8]) -> Result<Package, InstallError> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| InstallError::invalid(format!("not a valid .kai archive ({e})")))?;
    if zip.len() > MAX_ENTRIES {
        return Err(InstallError::invalid(format!("archive has more than {MAX_ENTRIES} entries")));
    }
    let mut total = 0u64;
    for i in 0..zip.len() {
        let f = zip.by_index_raw(i).map_err(|e| InstallError::invalid(format!("unreadable archive entry ({e})")))?;
        if !path_is_safe(f.name()) {
            return Err(InstallError::invalid(format!("archive entry '{}' escapes the package", f.name())));
        }
        total = total.saturating_add(f.size());
    }
    if total > MAX_UNCOMPRESSED_BYTES {
        return Err(InstallError::invalid("archive uncompressed size exceeds 32 MiB"));
    }

    let manifest_json = {
        let mut f = zip.by_name("manifest.json").map_err(|_| InstallError::invalid("manifest.json: missing from the package"))?;
        String::from_utf8(read_limited(&mut f, MAX_MANIFEST_BYTES, "manifest.json")?)
            .map_err(|_| InstallError::invalid("manifest.json: not valid UTF-8"))?
    };
    let manifest = Manifest::parse(&manifest_json)?;
    let bundle = {
        let mut f = zip.by_name("bundle.js").map_err(|_| InstallError::invalid("bundle.js: missing from the package"))?;
        if f.size() > MAX_BUNDLE_BYTES {
            return Err(InstallError::invalid("bundle.js: larger than 10 MiB"));
        }
        String::from_utf8(read_limited(&mut f, MAX_BUNDLE_BYTES, "bundle.js")?).map_err(|_| InstallError::invalid("bundle.js: not valid UTF-8"))?
    };
    Ok(Package { manifest, manifest_json, bundle })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const MANIFEST: &str = r#"{"id":"t.ext","name":"T","version":"1","type":"custom","description":"d","permissions":[]}"#;

    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in entries {
            w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    #[test]
    fn reads_a_minimal_package() {
        let p = read_kai_bytes(&zip_of(&[("manifest.json", MANIFEST.as_bytes()), ("bundle.js", b"module.exports = {};")])).unwrap();
        assert_eq!(p.manifest.id, "t.ext");
        assert_eq!(p.bundle, "module.exports = {};");
    }

    #[test]
    fn rejects_bad_packages_with_field_names() {
        let e = |entries: &[(&str, &[u8])]| read_kai_bytes(&zip_of(entries)).unwrap_err().to_string();
        assert!(e(&[("bundle.js", b"x")]).contains("manifest.json"));
        assert!(e(&[("manifest.json", MANIFEST.as_bytes())]).contains("bundle.js"));
        assert!(e(&[("manifest.json", b"{}"), ("bundle.js", b"x")]).contains("manifest."));
        assert!(read_kai_bytes(b"not a zip").is_err());
    }

    #[test]
    fn rejects_path_traversal_entries() {
        for evil in ["../evil.js", "/abs.js", "C:/x.js", "a/../../b", "..\\x"] {
            let r = read_kai_bytes(&zip_of(&[("manifest.json", MANIFEST.as_bytes()), ("bundle.js", b"x"), (evil, b"x")]));
            assert!(r.unwrap_err().to_string().contains("escapes"), "{evil}");
        }
        assert!(path_is_safe("sub/dir/file.txt"));
    }

    #[test]
    fn enforces_entry_count_and_bundle_size() {
        let names: Vec<String> = (0..70).map(|i| format!("f{i}.txt")).collect();
        let mut entries: Vec<(&str, &[u8])> = vec![("manifest.json", MANIFEST.as_bytes()), ("bundle.js", b"x")];
        entries.extend(names.iter().map(|n| (n.as_str(), b"x".as_slice())));
        assert!(read_kai_bytes(&zip_of(&entries)).unwrap_err().to_string().contains("more than 64"));

        let big = vec![b'a'; (MAX_BUNDLE_BYTES + 1) as usize];
        let r = read_kai_bytes(&zip_of(&[("manifest.json", MANIFEST.as_bytes()), ("bundle.js", &big)]));
        assert!(r.unwrap_err().to_string().contains("bundle.js"));
    }
}
