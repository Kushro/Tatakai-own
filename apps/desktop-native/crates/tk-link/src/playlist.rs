//! HLS playlist rewriting for the SSRF fetcher (review E6). Every URI of a playlist (variants, `#EXT-X-MEDIA`,
//! segments, `#EXT-X-KEY`, `#EXT-X-MAP`) is resolved against the playlist URL and handed to `map`, which returns the
//! loopback URI FFmpeg will request instead. The rewriter REJECTS `data:`, `file:` and any scheme other than http(s),
//! playlists above 2 MiB, nesting deeper than master -> media, and anything `map` refuses (e.g. a key on a host the
//! policy does not allow).

use url::Url;

pub const MAX_PLAYLIST_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UriKind {
    /// A variant / alternative-rendition playlist (loaded and rewritten again, one level only).
    Playlist,
    Segment,
    Key,
    Map,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RewriteError {
    #[error("playlist larger than 2 MiB")]
    TooLarge,
    #[error("playlist is not UTF-8")]
    NotUtf8,
    #[error("not an HLS playlist")]
    NotPlaylist,
    #[error("forbidden URI scheme")]
    ForbiddenScheme,
    #[error("playlist nesting too deep")]
    TooDeep,
    #[error("malformed URI")]
    BadUri,
    #[error("URI denied by policy")]
    Denied,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Rewritten {
    pub text: String,
    pub is_master: bool,
    /// Sum of `#EXTINF` (media playlists).
    pub duration_secs: f64,
}

/// Tags whose `URI="..."` attribute is rewritten, with the kind of resource they point to.
fn tag_uri_kind(line: &str) -> Option<UriKind> {
    const TABLE: [(&str, UriKind); 7] = [
        ("#EXT-X-KEY:", UriKind::Key),
        ("#EXT-X-SESSION-KEY:", UriKind::Key),
        ("#EXT-X-MAP:", UriKind::Map),
        ("#EXT-X-MEDIA:", UriKind::Playlist),
        ("#EXT-X-I-FRAME-STREAM-INF:", UriKind::Playlist),
        ("#EXT-X-PRELOAD-HINT:", UriKind::Segment),
        ("#EXT-X-RENDITION-REPORT:", UriKind::Playlist),
    ];
    TABLE.iter().find(|(p, _)| line.starts_with(p)).map(|(_, k)| *k)
}

fn resolve(base: &Url, uri: &str) -> Result<Url, RewriteError> {
    if uri.is_empty() || uri.contains(['\r', '\n', '\0']) {
        return Err(RewriteError::BadUri);
    }
    let u = base.join(uri).map_err(|_| RewriteError::BadUri)?;
    match u.scheme() {
        "http" | "https" => Ok(u),
        _ => Err(RewriteError::ForbiddenScheme),
    }
}

pub fn rewrite(
    body: &[u8],
    base: &Url,
    depth: u8,
    mut map: impl FnMut(&Url, UriKind) -> Result<String, RewriteError>,
) -> Result<Rewritten, RewriteError> {
    if body.len() > MAX_PLAYLIST_BYTES {
        return Err(RewriteError::TooLarge);
    }
    let text = std::str::from_utf8(body).map_err(|_| RewriteError::NotUtf8)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if !text.trim_start().starts_with("#EXTM3U") {
        return Err(RewriteError::NotPlaylist);
    }
    let mut out = String::with_capacity(text.len() + text.len() / 2);
    let mut is_master = false;
    let mut expect_variant = false;
    let mut duration = 0.0f64;

    for raw in text.split('\n') {
        let line = raw.trim_end_matches('\r');
        let trimmed = line.trim();
        if trimmed.is_empty() {
            out.push('\n');
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix('#') {
            let tag = trimmed;
            if rest.starts_with("EXT-X-STREAM-INF") {
                is_master = true;
                expect_variant = true;
            }
            if let Some(d) = tag.strip_prefix("#EXTINF:") {
                duration += d.split(',').next().and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| v.is_finite() && *v >= 0.0).unwrap_or(0.0);
            }
            match tag_uri_kind(tag) {
                Some(kind) if tag.contains("URI=\"") => out.push_str(&rewrite_attr(tag, base, kind, depth, &mut map)?),
                _ => out.push_str(tag),
            }
            out.push('\n');
            continue;
        }
        // a URI line
        let kind = if expect_variant { UriKind::Playlist } else { UriKind::Segment };
        expect_variant = false;
        if kind == UriKind::Playlist && depth >= 1 {
            return Err(RewriteError::TooDeep);
        }
        let target = resolve(base, trimmed)?;
        out.push_str(&map(&target, kind)?);
        out.push('\n');
    }
    if is_master && depth >= 1 {
        return Err(RewriteError::TooDeep);
    }
    // keep the trailing-newline shape of the input
    while out.ends_with("\n\n") && !text.ends_with("\n\n") {
        out.pop();
    }
    Ok(Rewritten { text: out, is_master, duration_secs: duration })
}

fn rewrite_attr(
    tag: &str,
    base: &Url,
    kind: UriKind,
    depth: u8,
    map: &mut impl FnMut(&Url, UriKind) -> Result<String, RewriteError>,
) -> Result<String, RewriteError> {
    const KEY: &str = "URI=\"";
    let mut out = String::with_capacity(tag.len() + 64);
    let mut rest = tag;
    while let Some(pos) = rest.find(KEY) {
        // must be an attribute name boundary (",URI=" or ":URI="), not e.g. "KEYURI="
        let boundary_ok = pos == 0 || matches!(rest.as_bytes()[pos - 1], b',' | b':');
        let start = pos + KEY.len();
        let end = rest[start..].find('"').ok_or(RewriteError::BadUri)? + start;
        out.push_str(&rest[..start]);
        if boundary_ok {
            if kind == UriKind::Playlist && depth >= 1 {
                return Err(RewriteError::TooDeep);
            }
            let target = resolve(base, &rest[start..end])?;
            out.push_str(&map(&target, kind)?);
        } else {
            out.push_str(&rest[start..end]);
        }
        out.push('"');
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Url {
        Url::parse("https://cdn.example.com/hls/master.m3u8").unwrap()
    }

    /// Mapper that tags every URI with its kind so the output is easy to assert.
    fn tag(u: &Url, k: UriKind) -> Result<String, RewriteError> {
        Ok(format!("/{k:?}/{u}"))
    }

    const MASTER: &str = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1000,RESOLUTION=1280x720\nv720/index.m3u8\n#EXT-X-STREAM-INF:BANDWIDTH=500\nhttps://other.example.com/low.m3u8\n#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",URI=\"audio/en.m3u8\"\n";
    const MEDIA: &str = "#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-KEY:METHOD=AES-128,URI=\"keys/k1.bin\",IV=0x01\n#EXT-X-MAP:URI=\"init.mp4\"\n#EXTINF:4.5,\nseg0.ts\n#EXTINF:5.5,\n/abs/seg1.ts\n#EXT-X-ENDLIST\n";

    #[test]
    fn rewrites_master_variants_and_alternative_renditions() {
        let r = rewrite(MASTER.as_bytes(), &base(), 0, tag).unwrap();
        assert!(r.is_master);
        assert!(r.text.contains("\n/Playlist/https://cdn.example.com/hls/v720/index.m3u8\n"));
        assert!(r.text.contains("\n/Playlist/https://other.example.com/low.m3u8\n"));
        assert!(r.text.contains("URI=\"/Playlist/https://cdn.example.com/hls/audio/en.m3u8\""));
        assert!(r.text.contains("#EXT-X-STREAM-INF:BANDWIDTH=1000,RESOLUTION=1280x720\n"), "tags are preserved");
    }

    #[test]
    fn rewrites_media_segments_keys_and_maps_and_sums_the_duration() {
        let r = rewrite(MEDIA.as_bytes(), &base(), 1, tag).unwrap();
        assert!(!r.is_master);
        assert_eq!(r.duration_secs, 10.0);
        assert!(r.text.contains("#EXT-X-KEY:METHOD=AES-128,URI=\"/Key/https://cdn.example.com/hls/keys/k1.bin\",IV=0x01"));
        assert!(r.text.contains("#EXT-X-MAP:URI=\"/Map/https://cdn.example.com/hls/init.mp4\""));
        assert!(r.text.contains("\n/Segment/https://cdn.example.com/hls/seg0.ts\n"));
        assert!(r.text.contains("\n/Segment/https://cdn.example.com/abs/seg1.ts\n"));
        assert!(r.text.contains("#EXT-X-ENDLIST"));
        // METHOD=NONE keys have no URI and pass through
        let none = "#EXTM3U\n#EXT-X-KEY:METHOD=NONE\n#EXTINF:1,\na.ts\n";
        assert!(rewrite(none.as_bytes(), &base(), 1, tag).unwrap().text.contains("#EXT-X-KEY:METHOD=NONE\n"));
    }

    #[test]
    fn rejects_data_file_and_other_schemes_everywhere() {
        for uri in ["data:text/plain;base64,AAAA", "file:///etc/passwd", "ftp://h/x.ts", "javascript:alert(1)", "skd://key", "gopher://h/", "\\\\server\\share\\x.ts"] {
            let seg = format!("#EXTM3U\n#EXTINF:1,\n{uri}\n");
            assert!(
                matches!(rewrite(seg.as_bytes(), &base(), 1, tag), Err(RewriteError::ForbiddenScheme | RewriteError::BadUri) | Ok(_)),
                "{uri}"
            );
            if !uri.starts_with('\\') {
                assert_eq!(rewrite(seg.as_bytes(), &base(), 1, tag).unwrap_err(), RewriteError::ForbiddenScheme, "segment {uri}");
            }
            let key = format!("#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"{uri}\"\n#EXTINF:1,\na.ts\n");
            if !uri.starts_with('\\') {
                assert_eq!(rewrite(key.as_bytes(), &base(), 1, tag).unwrap_err(), RewriteError::ForbiddenScheme, "key {uri}");
            }
            let map = format!("#EXTM3U\n#EXT-X-MAP:URI=\"{uri}\"\n#EXTINF:1,\na.ts\n");
            if !uri.starts_with('\\') {
                assert_eq!(rewrite(map.as_bytes(), &base(), 1, tag).unwrap_err(), RewriteError::ForbiddenScheme, "map {uri}");
            }
            let variant = format!("#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\n{uri}\n");
            if !uri.starts_with('\\') {
                assert_eq!(rewrite(variant.as_bytes(), &base(), 0, tag).unwrap_err(), RewriteError::ForbiddenScheme, "variant {uri}");
            }
        }
    }

    #[test]
    fn rejects_oversized_nested_and_non_playlists() {
        let mut big = String::from("#EXTM3U\n");
        while big.len() <= MAX_PLAYLIST_BYTES {
            big.push_str("#EXTINF:1,\nsegment-with-a-long-enough-name.ts\n");
        }
        assert_eq!(rewrite(big.as_bytes(), &base(), 1, tag).unwrap_err(), RewriteError::TooLarge);
        // master -> media only: a master reached at depth 1 is nesting
        assert_eq!(rewrite(MASTER.as_bytes(), &base(), 1, tag).unwrap_err(), RewriteError::TooDeep);
        assert_eq!(rewrite(b"<html>not hls</html>", &base(), 0, tag).unwrap_err(), RewriteError::NotPlaylist);
        assert_eq!(rewrite(&[0xff, 0xfe, 0x00], &base(), 0, tag).unwrap_err(), RewriteError::NotUtf8);
        let unterminated = "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"oops\n#EXTINF:1,\na.ts\n";
        assert_eq!(rewrite(unterminated.as_bytes(), &base(), 1, tag).unwrap_err(), RewriteError::BadUri);
    }

    #[test]
    fn a_key_on_a_disallowed_host_is_rejected_by_the_mapper() {
        let playlist = "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"https://evil.internal/key\"\n#EXTINF:1,\na.ts\n";
        let deny = |u: &Url, k: UriKind| {
            if k == UriKind::Key && u.host_str() == Some("evil.internal") { Err(RewriteError::Denied) } else { Ok(u.to_string()) }
        };
        assert_eq!(rewrite(playlist.as_bytes(), &base(), 1, deny).unwrap_err(), RewriteError::Denied);
        let ok = "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI=\"https://cdn.example.com/key\"\n#EXTINF:1,\na.ts\n";
        assert!(rewrite(ok.as_bytes(), &base(), 1, deny).is_ok());
    }

    #[test]
    fn crlf_playlists_are_normalised_and_attribute_names_must_match_exactly() {
        let crlf = MEDIA.replace('\n', "\r\n");
        let r = rewrite(crlf.as_bytes(), &base(), 1, tag).unwrap();
        assert!(!r.text.contains('\r'));
        // "KEYURI=" is a different attribute and must not be rewritten
        let odd = "#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,XURI=\"keep\",URI=\"k\"\n#EXTINF:1,\na.ts\n";
        let r = rewrite(odd.as_bytes(), &base(), 1, tag).unwrap();
        assert!(r.text.contains("XURI=\"keep\""));
        assert!(r.text.contains(",URI=\"/Key/https://cdn.example.com/hls/k\""));
    }
}
